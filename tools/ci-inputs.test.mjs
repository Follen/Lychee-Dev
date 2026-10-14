import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { modulePath, repository } from './baseline-common.mjs';
import { foreverCommit, requiredCases, retailCommit, unitDigests, verifyRequiredIntegrations, verifyUnitDocument } from './ci-inputs.mjs';

const jsonl = events => events.map(JSON.stringify).join('\n');
function passingEvents() {
  return requiredCases.flatMap(spec => [
    ...spec.tests.map(Test => ({ Package: modulePath + spec.package, Test, Action: 'pass' })),
    { Package: modulePath + spec.package, Action: 'pass' },
  ]);
}

test('CI required integrations are existing tests and retain all release LuaLS gates', () => {
  const names = new Set();
  for (const spec of requiredCases) {
    const source = readdirSync(join(repository, spec.package)).filter(name => name.endsWith('_test.go')).map(name => readFileSync(join(repository, spec.package, name), 'utf8')).join('\n');
    for (const name of spec.tests.filter(name => !name.includes('/'))) {
      assert.match(source, new RegExp(`^func ${name}\\(t \\*testing\\.T\\)`, 'm'));
      names.add(name);
    }
  }
  assert.equal(names.size, 15);
  const release = readFileSync(join(repository, '.github/workflows/toolkit-release.yml'), 'utf8');
  const releaseNames = release.match(/\$required = @\(([^\n]+)\)/)[1].matchAll(/'(Test[^']+)'/g);
  for (const [, name] of releaseNames) assert.ok(names.has(name), `release gate missing from PR CI: ${name}`);
});

test('CI exact passes include package termination and each required child', () => {
  const events = passingEvents();
  assert.equal(verifyRequiredIntegrations(jsonl(events)).state, 'passed');
  for (let index = 0; index < events.length; index++) {
    assert.equal(verifyRequiredIntegrations(jsonl(events.filter((_, i) => i !== index))).state, 'failed', `missing ${JSON.stringify(events[index])}`);
    for (const Action of ['skip', 'fail']) {
      assert.equal(verifyRequiredIntegrations(jsonl(events.map((event, i) => i === index ? { ...event, Action } : event))).state, 'failed');
    }
  }
  const parent = events.find(event => event.Test === 'TestRealRuntimePublicInterfaces');
  assert.equal(verifyRequiredIntegrations(jsonl([...events, { ...parent, Test: parent.Test + '/unexpected-child', Action: 'skip' }])).state, 'failed');
  assert.throws(() => verifyRequiredIntegrations(''));
  assert.throws(() => verifyRequiredIntegrations('not JSON'));
});

test('fixed documentation pins match the existing Go source contracts; arbitrary bytes fail', () => {
  const source = readFileSync(join(repository, 'internal/codebase/flow/rules_test.go'), 'utf8');
  for (const value of [retailCommit, foreverCommit, ...Object.values(unitDigests)]) assert.ok(source.includes(value));
  for (const variant of ['retail', 'forever', 'unknown']) assert.throws(() => verifyUnitDocument(variant, Buffer.from('synthetic documentation')), /digest mismatch/);
});

test('CI documentation adapter uses the production generator and retains its receipt', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-ci-definitions-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const docs = join(root, 'Interface/AddOns/Blizzard_APIDocumentationGenerated');
  mkdirSync(docs, { recursive: true });
  writeFileSync(join(docs, 'UnitDocumentation.lua'), 'local Unit={Name="Unit", Type="System", Functions={{Name="UnitHealth", Type="Function", Arguments={{Name="unit", Type="string"}}, Returns={{Name="health", Type="number"}}}}}; APIDocumentation:AddDocumentationTable(Unit)\n');
  const output = join(root, 'generated');
  const result = spawnSync('go', ['run', 'tools/ci-source-definitions.go', root, retailCommit, output], { cwd: repository, encoding: 'utf8', timeout: 120000 });
  assert.equal(result.status, 0, result.error?.message ?? result.stderr);
  assert.match(readFileSync(join(output, 'api.d.lua'), 'utf8'), /function UnitHealth\(unit\)/);
  const receipt = JSON.parse(readFileSync(join(output, 'manifest.json'), 'utf8'));
  assert.equal(receipt.manifest.identity.commit, retailCommit);
  assert.equal(receipt.manifest.documents, 1);
  assert.match(receipt.manifest.definitionsSHA256, /^[a-f0-9]{64}$/);
  assert.equal(typeof receipt.coverage.complete, 'boolean');
});
