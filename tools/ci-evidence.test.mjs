import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { groups, verifyEvidence } from './ci-evidence.mjs';
import { classifyChanges, checkLinks } from './ci-scope.mjs';
import { modulePath, repository } from './baseline-common.mjs';

const expected = { commit: 'a'.repeat(40), tree: 'b'.repeat(40) };
const jsonl = events => events.map(JSON.stringify).join('\n');
function fixture(group) {
  const packages = Object.keys(groups[group]).map(name => ({ importPath: modulePath + name, hasTests: true, instrumented: true }));
  const events = Object.entries(groups[group]).flatMap(([name, tests]) => {
    const Package = modulePath + name;
    return [{ Package, Action: 'start' }, ...(tests.length ? tests : ['TestExample']).flatMap(Test => [
      { Package, Action: 'run', Test }, { Package, Action: 'pass', Test },
    ]), { Package, Action: 'pass' }];
  });
  return { profile: `mode: atomic\n${packages[0].importPath}/file.go:1.1,1.2 1 1\n`, events: jsonl(events),
    identity: { testedCommit: expected.commit, testedTree: expected.tree, requestedPrHead: expected.commit,
      goVersion: 'go version go1.27.1 windows/amd64', goos: 'windows', goarch: 'amd64', exitCode: 0,
      command: ['go', 'test', '-json', './...'], packages } };
}

test('reused groups reference real production tests and the full Lua matrix', () => {
  for (const [group, cases] of Object.entries(groups)) {
    assert.equal(verifyEvidence(group, fixture(group), expected).state, 'passed');
    for (const [path, tests] of Object.entries(cases)) {
      const code = readdirSync(join(repository, path)).filter(p => p.endsWith('_test.go')).map(p => readFileSync(join(repository, path, p), 'utf8')).join('\n');
      for (const name of tests.filter(name => !name.includes('/'))) assert.match(code, new RegExp(`^func ${name}\\(`, 'm'));
    }
  }
});

test('reuse rejects wrong identity, invalid events, zero execution, unfinished and failed packages', () => {
  const input = fixture('process');
  for (const key of ['commit', 'tree']) assert.throws(() => verifyEvidence('process', input, { ...expected, [key]: 'c'.repeat(40) }), /mismatch/);
  assert.throws(() => verifyEvidence('process', { ...input, profile: null }, expected));
  assert.throws(() => verifyEvidence('process', { ...input, events: 'invalid' }, expected));
  const events = input.events.split('\n').map(JSON.parse);
  for (const removed of ['start', 'run', 'pass']) {
    const index = events.findIndex(e => e.Action === removed);
    assert.throws(() => verifyEvidence('process', { ...input, events: jsonl(events.filter((_, i) => i !== index)) }, expected));
  }
  for (const action of ['skip', 'fail']) {
    assert.throws(() => verifyEvidence('process', { ...input, events: jsonl(events.map(e => e.Test && e.Action === 'pass' ? { ...e, Action: action } : e)) }, expected));
  }
  const empty = events.filter(e => !e.Test);
  assert.throws(() => verifyEvidence('process', { ...input, events: jsonl(empty) }, expected), /missing completed/);
});

test('Lua missing child fails, while unavailable second doctor volume is explicitly retained as skip', () => {
  const addon = fixture('addon');
  assert.throws(() => verifyEvidence('addon', { ...addon, events: jsonl(addon.events.split('\n').map(JSON.parse).filter(e => !e.Test?.includes('/'))) }, expected), /missing executed/);
  const doctor = fixture('doctor');
  doctor.events = jsonl(doctor.events.split('\n').map(JSON.parse).map(e => e.Test === 'TestLiveDoctorUpgradeDefaultArchiveAcrossVolumes' && e.Action === 'pass' ? { ...e, Action: 'skip' } : e));
  const result = verifyEvidence('doctor', doctor, expected);
  assert.equal(result.results[0].skips[0].test, 'TestLiveDoctorUpgradeDefaultArchiveAcrossVolumes');
});

test('only known development prose receives lightweight CI; runtime docs, Skill and unknown paths stay full', () => {
  assert.equal(classifyChanges(['Design.md', 'docs/development.md']), 'docs');
  for (const paths of [[], ['AGENTS.md'], ['docs/new.md'], ['docs/live-protocol.md'], ['skills/lycheedev/SKILL.md'], ['.github/workflows/toolkit-ci.yml'], ['Design.md', 'go.mod']]) assert.equal(classifyChanges(paths), 'full');
  assert.ok(checkLinks(['Design.md', 'docs/development.md']) > 0);
});
