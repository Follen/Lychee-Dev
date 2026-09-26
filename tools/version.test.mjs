import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { synchronizeVersion, versionedLuaFixtures, versionedSignalSamples } from './version.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// The addon ships one flat multi-interface manifest; there are no per-client
// TOC variants to synchronize.
const targets = ['internal/buildinfo/version.go', 'packages/npm/lycheedev/package.json', 'packages/npm/lycheedev/package-lock.json', 'addon/Core/Runtime.lua', 'addon/Lychee Dev.toc', ...versionedLuaFixtures, ...versionedSignalSamples, 'tests/addon/env.lua', 'tests/addon/t_about.lua'];
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-version-'));
  t.after(() => rmSync(root, { recursive: true }));
  for (const file of ['release/version.json', ...targets]) {
    mkdirSync(dirname(join(root, file)), { recursive: true });
    writeFileSync(join(root, file), readFileSync(join(repo, file)));
  }
  return root;
}
function changeSource(root, version) {
  writeFileSync(join(root, 'release/version.json'), JSON.stringify({ schema: 'lycheedev.version.v1', version }));
}

test('current source is synchronized without writes', () => {
  assert.equal(synchronizeVersion(repo).changed.length, 0);
});
test('check rejects drift without repair; explicit generation updates every deliverable', t => {
  const root = fixture(t);
  const original = new Map(targets.map(path => [path, readFileSync(join(root, path), 'utf8')]));
  changeSource(root, '2.0.0-rc.1');
  assert.throws(() => synchronizeVersion(root), /version.drift/);
  for (const [path, text] of original) assert.equal(readFileSync(join(root, path), 'utf8'), text);
  const written = synchronizeVersion(root, true);
  assert.deepEqual(written.changed.sort(), [...targets].sort());
  assert.equal(synchronizeVersion(root).version, '2.0.0-rc.1');
  const lock = JSON.parse(readFileSync(join(root, 'packages/npm/lycheedev/package-lock.json')));
  assert.equal(lock.version, '2.0.0-rc.1');
  assert.equal(lock.packages[''].version, lock.version);
  assert.equal(synchronizeVersion(root, true).changed.length, 0);
});
test('malformed or duplicated target prevents any partial generation', t => {
  const root = fixture(t);
  changeSource(root, '2.0.0-rc.1');
  const generated = readFileSync(join(root, targets[0]), 'utf8');
  writeFileSync(join(root, 'addon/Lychee Dev.toc'), '## Version: old\n## Version: other\n');
  assert.throws(() => synchronizeVersion(root, true), /version.invalid_target/);
  assert.equal(readFileSync(join(root, targets[0]), 'utf8'), generated);
});
test('invalid source version cannot enter generated Go or Lua', t => {
  const root = fixture(t);
  for (const version of ['2.0', '02.0.0', '2.0.0-01', '2.0.0"\n', null]) {
    changeSource(root, version);
    assert.throws(() => synchronizeVersion(root, true), /version.invalid_source/);
  }
});
