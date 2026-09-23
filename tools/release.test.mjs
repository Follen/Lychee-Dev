// Unit coverage for the release assembly logic that is testable offline:
// version policy, dist-tag routing (REL-11), strict tag parsing, registry-state
// classification (REL-09) and the sealed tarball whitelist audit (PKG-05).
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import {
  auditTgz, classifyRegistry, distTagFor, parseOptions, parseTag, parseVcsIdentity, policyFor, registryStateCommand, REPOSITORY_URL, TARGETS,
} from './release.mjs';

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

test('registry-state returns one complete document and --out persists the same state', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'lycheedev-registry-'));
  const previousExit = process.exitCode;
  const fetchImpl = async () => ({ status: 200, text: async () => JSON.stringify({ dist: { integrity: 'sha512-match' } }) });
  try {
    const args = ['--name', 'lycheedev', '--version', '2.0.0', '--integrity', 'sha512-match'];
    const record = await registryStateCommand(args, fetchImpl);
    assert.deepEqual(record, {
      name: 'lycheedev', version: '2.0.0', url: 'https://registry.npmjs.org/lycheedev/2.0.0',
      state: 'exists-matching', integrity: 'sha512-match',
    });
    const path = join(directory, 'registry.json');
    assert.deepEqual(await registryStateCommand([...args, '--out', path], fetchImpl), {
      state: 'exists-matching', integrity: 'sha512-match',
    });
    assert.deepEqual(JSON.parse(readFileSync(path, 'utf8')), record);
  } finally {
    process.exitCode = previousExit;
    rmSync(directory, { recursive: true, force: true });
  }
});

test('registry-state CLI prints one JSON document without --out', () => {
  const directory = mkdtempSync(join(tmpdir(), 'lycheedev-registry-cli-'));
  try {
    const preload = join(directory, 'fetch.mjs');
    writeFileSync(preload, 'globalThis.fetch = async () => ({ status: 404, text: async () => "" });\n');
    const script = fileURLToPath(new URL('./release.mjs', import.meta.url));
    const run = spawnSync(process.execPath, ['--import', pathToFileURL(preload).href, script, 'registry-state',
      '--name', 'lycheedev', '--version', '2.0.1'], { encoding: 'utf8' });
    assert.equal(run.status, 0, run.stderr);
    assert.deepEqual(JSON.parse(run.stdout), {
      name: 'lycheedev', version: '2.0.1',
      url: 'https://registry.npmjs.org/lycheedev/2.0.1', state: 'absent',
    });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('VCS identity parsing accepts real `go version -m` output with indented build lines (REL-01)', () => {
  // Real output shape: every build line starts with a tab before the "build" key.
  const output = [
    '/out/lycheedev.exe: executable',
    '\tpath\tgithub.com/follenfang/lycheedev/cmd/lycheedev',
    '\tmod\tgithub.com/follenfang/lycheedev\t(development)',
    '\tdep\tgithub.com/chai2010/gettext-go@v1.0.3',
    '\tbuild\t-compiler=gc',
    '\tbuild\tvcs=git',
    `\tbuild\tvcs.revision=${'8'.repeat(40)}`,
    '\tbuild\tvcs.time=2026-09-21T01:49:46Z',
    '\tbuild\tvcs.modified=true',
  ].join('\n');
  assert.deepEqual(
    parseVcsIdentity(output),
    { revision: '8'.repeat(40), modified: 'true' },
  );
  // A binary without a stamp parses to undefined so the gate still rejects it.
  assert.deepEqual(parseVcsIdentity('\tbuild\t-compiler=gc\n'), { revision: undefined, modified: undefined });
});

test('repeated options accumulate so seal can fold multiple test reports (§6)', () => {
  assert.deepEqual(parseOptions(['--out', 'x']), { '--out': 'x' });
  assert.deepEqual(parseOptions(['--flag']), { '--flag': true });
  assert.deepEqual(
    parseOptions(['--out', 'd', '--report', 'a.json', '--report', 'b.json', '--report', 'c.json']),
    { '--out': 'd', '--report': ['a.json', 'b.json', 'c.json'] },
  );
});

test('development versions are private and unpublished; release versions publish', () => {
  const dev = policyFor('2.0.0-dev');
  assert.equal(dev.privateMustBeTrue, true);
  assert.equal(dev.development, true);
  assert.equal(dev.npmDistTag, null);
  assert.equal(policyFor('2.0.0').privateMustBeTrue, false);
  assert.equal(policyFor('2.0.0-rc.2').privateMustBeTrue, false);
  assert.equal(policyFor('2.0.0-rc.2').npmDistTag, 'next');
});

test('REL-11: release candidates only enter next, finals enter latest', () => {
  assert.equal(distTagFor('2.0.0-rc.1'), 'next');
  assert.equal(distTagFor('2.0.0'), 'latest');
  assert.equal(distTagFor('2.0.1'), 'latest');
  assert.throws(() => distTagFor('2.0.0-beta.1'), /invalid_version/);
  assert.throws(() => distTagFor('2.0.0-dev'), /invalid_version/);
});

test('REL-01: the tag is exactly v<semver> including -rc.N', () => {
  assert.equal(parseTag('v2.0.0'), '2.0.0');
  assert.equal(parseTag('v2.0.0-rc.12'), '2.0.0-rc.12');
  for (const tag of ['2.0.0', 'v2.0', 'v2.0.0-rc', 'v2.0.0-beta.1', 'v2.0.0 ', 'v2.0.0\n', 'refs/tags/v2.0.0', undefined]) {
    assert.throws(() => parseTag(tag), /invalid_tag/, `accepted ${JSON.stringify(tag)}`);
  }
});

test('REL-09: unknown registry responses are never treated as absent', () => {
  assert.deepEqual(classifyRegistry({ status: 404 }).state, 'absent');
  assert.deepEqual(classifyRegistry({ status: 200, body: JSON.stringify({ dist: { integrity: 'sha512-a' } }), expectedIntegrity: 'sha512-a' }).state, 'exists-matching');
  assert.deepEqual(classifyRegistry({ status: 200, body: JSON.stringify({ dist: { integrity: 'sha512-b' } }), expectedIntegrity: 'sha512-a' }).state, 'exists-conflicting');
  for (const attempt of [
    { status: 403 }, { status: 500 }, { status: 200, body: '<html>gateway</html>' },
    { status: 200, body: JSON.stringify({ dist: {} }) }, { status: 0, body: '' },
  ]) assert.equal(classifyRegistry(attempt).state, 'unknown', JSON.stringify(attempt));
});

function fixturePackage(version = '2.0.0') {
  const commit = 'a'.repeat(40);
  const files = new Map();
  const binaries = {};
  for (const entry of TARGETS) {
    const bytes = Buffer.from(`binary-${entry.target}`);
    binaries[entry.target] = { path: entry.binary, bytes: bytes.length, sha256: sha256(bytes) };
    files.set(`package/${entry.binary}`, bytes);
  }
  const resources = [];
  for (const path of ['skill/SKILL.md', 'addon/Lychee Dev_Mainline.toc', 'addon/Lychee Dev_Mists.toc',
    'addon/Lychee Dev_Wrath.toc', 'addon/Lychee Dev_Forever.toc', 'addon/Core/Runtime.lua']) {
    const bytes = Buffer.from(`content of ${path}\n`);
    resources.push({ path, bytes: bytes.length, sha256: sha256(bytes) });
    files.set(`package/payload/${path}`, bytes);
  }
  const correspondingSource = {
    repository: REPOSITORY_URL, commit, tag: `v${version}`,
    archive: `lycheedev-${version}-corresponding-source.tar.gz`, sha256: 'c'.repeat(64),
  };
  const releaseJson = { schema: 'lycheedev.release.v1', version, commit, binaries, resources, correspondingSource };
  const packageJson = {
    name: 'lycheedev', version, private: version.endsWith('-dev') || undefined,
    type: 'module', bin: { lycheedev: 'bin/lycheedev.mjs' },
    files: ['bin/', 'native/', 'payload/', 'release.json', 'README.md', 'LICENSE', 'THIRD_PARTY_NOTICES'],
    engines: { node: '>=22.14.0' },
    publishConfig: { access: 'public', registry: 'https://registry.npmjs.org/' },
    repository: { type: 'git', url: `git+${REPOSITORY_URL}.git`, directory: 'packages/npm/lycheedev' },
  };
  files.set('package/package.json', Buffer.from(JSON.stringify(packageJson)));
  files.set('package/release.json', Buffer.from(JSON.stringify(releaseJson)));
  files.set('package/bin/lycheedev.mjs', Buffer.from('#!/usr/bin/env node\n'));
  files.set('package/README.md', Buffer.from('# lycheedev\n'));
  files.set('package/LICENSE', Buffer.from('license text\n'));
  files.set('package/THIRD_PARTY_NOTICES', Buffer.from('notices\n'));
  return { files, releaseJson };
}

test('PKG-05: the sealed tarball whitelist accepts the release shape', () => {
  const { files } = fixturePackage();
  const entries = [...files].map(([name, bytes]) => ({ name, bytes, mode: 0o644 }));
  const audit = auditTgz(entries, { version: '2.0.0', expectedCommit: 'a'.repeat(40) });
  assert.deepEqual(audit.violations, []);
  assert.equal(audit.ok, true);
});

test('PKG-05/REL-08: extra, banned, missing and tampered content all fail closed', () => {
  const base = () => [...fixturePackage().files].map(([name, bytes]) => ({ name, bytes, mode: 0o644 }));
  const mutate = (entries, fn) => { fn(entries); return auditTgz(entries, { version: '2.0.0', expectedCommit: 'a'.repeat(40) }); };

  assert.match(mutate(base(), entries => entries.push({ name: 'package/tests/investigation.md', bytes: Buffer.from('x') })).violations.join(), /forbidden entry|banned content/);
  assert.match(mutate(base(), entries => entries.push({ name: 'package/payload/addon/automation.py', bytes: Buffer.from('x') })).violations.join(), /forbidden entry|banned content/);
  assert.match(mutate(base(), entries => { entries.find(entry => entry.name === 'package/LICENSE').name = 'package/LICENSE.moved'; }).violations.join(), /missing entry: package\/LICENSE/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/payload/addon/Core/Runtime.lua');
    entry.bytes = Buffer.from('tampered\n');
  }).violations.join(), /resource record mismatch/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/release.json');
    const manifest = JSON.parse(entry.bytes.toString('utf8'));
    manifest.resources.push({ path: 'addon/Evil.lua', bytes: 1, sha256: 'd'.repeat(64) });
    entry.bytes = Buffer.from(JSON.stringify(manifest));
  }).violations.join(), /resource missing from payload/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/release.json');
    const manifest = JSON.parse(entry.bytes.toString('utf8'));
    delete manifest.correspondingSource;
    entry.bytes = Buffer.from(JSON.stringify(manifest));
  }).violations.join(), /correspondingSource/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/release.json');
    const manifest = JSON.parse(entry.bytes.toString('utf8'));
    manifest.correspondingSource.commit = 'b'.repeat(40);
    entry.bytes = Buffer.from(JSON.stringify(manifest));
  }).violations.join(), /correspondingSource\.commit/);
  assert.match(mutate(base(), entries => {
    const entry = entries.find(candidate => candidate.name === 'package/package.json');
    const pkg = JSON.parse(entry.bytes.toString('utf8'));
    pkg.dependencies = { leftpad: '1.0.0' };
    entry.bytes = Buffer.from(JSON.stringify(pkg));
  }).violations.join(), /dependencies must be empty/);
  assert.match(mutate(base(), entries => {
    entries.push({
      name: 'package/payload/addon/Bridge/Definitions.lua',
      bytes: Buffer.from('local _, ns = ...\nns.ProbeDefinitions = {schema="lycheedev.queue.v1",entries={\n  {id=1},\n}}\n'),
      mode: 0o644,
    });
  }).violations.join(), /task blocks/);
});

test('the audit binds commit and version of the assembly (REL-01/05)', () => {
  const entries = [...fixturePackage().files].map(([name, bytes]) => ({ name, bytes, mode: 0o644 }));
  assert.match(auditTgz(entries, { version: '9.9.9' }).violations.join(), /package version/);
  assert.match(auditTgz(entries, { version: '2.0.0', expectedCommit: 'b'.repeat(40) }).violations.join(), /release\.json commit/);
});

test('a missing LICENSE fails with the license decision pending error', t => {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-nolicense-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const name of ['README.md', 'THIRD_PARTY_NOTICES']) writeFileSync(join(root, name), `${name}\n`);
  const { files } = fixturePackage();
  const entries = [...files].map(([name, bytes]) => ({ name, bytes, mode: 0o644 }));
  const audit = auditTgz(entries, { version: '2.0.0', expectedCommit: 'a'.repeat(40), sourceRoot: root });
  assert.equal(audit.ok, false);
  assert.match(audit.violations.join(), /license decision pending/);
});
