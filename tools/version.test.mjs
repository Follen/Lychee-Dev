import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {synchronizeVersion} from './version.mjs';

const repo = resolve(import.meta.dirname, '..');
const targets = ['Cargo.toml', 'Cargo.lock', 'crates/lycheedev-binding-contracts/Cargo.toml',
  'crates/lycheedev-emmy-adapter/Cargo.toml', 'release/npm-channel.json',
  'packages/npm/lycheedev/package.json', 'packages/npm/lycheedev/package-lock.json',
  'addon/Core/Runtime.lua', 'addon/Lychee Dev.toc', 'tests/addon/env.lua', 'tests/addon/t_about.lua'];
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-version-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  for (const path of ['release/version.json', ...targets]) {
    mkdirSync(dirname(join(root, path)), {recursive: true});
    writeFileSync(join(root, path), readFileSync(join(repo, path)));
  }
  return root;
}
function version(root, value) { writeFileSync(join(root, 'release/version.json'), JSON.stringify({schema: 'lycheedev.version.v1', version: value})); }
test('current Rust, addon and npm versions agree', () => assert.equal(synchronizeVersion(repo).changed.length, 0));
test('check is read-only and explicit generation updates current deliverables', t => {
  const root = fixture(t), original = targets.map(path => [path, readFileSync(join(root, path))]);
  version(root, '3.5.1');
  assert.throws(() => synchronizeVersion(root), /version.drift/);
  for (const [path, bytes] of original) assert.deepEqual(readFileSync(join(root, path)), bytes);
  assert.deepEqual(synchronizeVersion(root, true).changed.sort(), [...targets].sort());
  assert.equal(synchronizeVersion(root).version, '3.5.1');
});
test('invalid target prevents partial generation', t => {
  const root = fixture(t), original = readFileSync(join(root, 'Cargo.toml'));
  version(root, '3.5.1');
  writeFileSync(join(root, 'addon/Lychee Dev.toc'), '## Version: a\n## Version: b\n');
  assert.throws(() => synchronizeVersion(root, true), /version.invalid_target/);
  assert.deepEqual(readFileSync(join(root, 'Cargo.toml')), original);
});
test('invalid source version is rejected', t => {
  const root = fixture(t);
  for (const value of ['3.5', '03.5.0', '3.5.0-01', null]) {
    version(root, value);
    assert.throws(() => synchronizeVersion(root), /version.invalid_source/);
  }
});
