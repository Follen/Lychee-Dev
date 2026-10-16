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
test('only the current host-verified Lua report follows the product release', () => {
  assert.deepEqual(versionedLuaFixtures, ['tests/protocol/report.lua']);
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
test('missing current Lua or signal targets fail without any partial generation', t => {
  for (const missing of [...versionedLuaFixtures, ...versionedSignalSamples, 'tests/addon/env.lua', 'tests/addon/t_about.lua']) {
    const root = fixture(t);
    const original = new Map(targets.filter(path => path !== missing).map(path => [path, readFileSync(join(root, path))]));
    rmSync(join(root, missing));
    changeSource(root, '3.5.1');
    for (const write of [false, true]) {
      assert.throws(() => synchronizeVersion(root, write), {code: 'ENOENT', path: join(root, missing)});
      for (const [path, bytes] of original) assert.deepEqual(readFileSync(join(root, path)), bytes, path);
    }
  }
});
test('release generation preserves archived oracles and fixed-version Lua fixtures', t => {
  const root = fixture(t);
  const retired = ['acknowledge', 'identity', 'loaded_ready', 'persistence', 'queue',
    'queue_ack', 'reentry', 'runtime_epoch', 'investigation', 'receiver', 'receiver_rollback'];
  const retained = [
    ...retired.map(name => `tests/history/legacy-ui-00fd643/source/tests/protocol/${name}.lua.source`),
    ...['formal-lua', 'formal-lua-3.2.1', 'formal-lua-3.2.2'].flatMap(name =>
      ['env.lua', 't_about.lua'].map(file => `tests/rust-parity/${name}/oracle/tests/addon/${file}`)),
    'tests/protocol/writer.lua', 'tests/protocol/slot_server.lua',
  ];
  const original = new Map(retained.map(path => [path, readFileSync(join(repo, path))]));
  for (const [path, bytes] of original) {
    mkdirSync(dirname(join(root, path)), {recursive: true});
    writeFileSync(join(root, path), bytes);
  }
  changeSource(root, '3.5.1');
  synchronizeVersion(root, true);
  assert.equal(synchronizeVersion(root).changed.length, 0);
  for (const [path, bytes] of original) assert.deepEqual(readFileSync(join(root, path)), bytes, path);
  for (const name of retired) assert.throws(() => readFileSync(join(root, `tests/protocol/${name}.lua`)), {code: 'ENOENT'});
});
test('invalid source version cannot enter generated Go or Lua', t => {
  const root = fixture(t);
  for (const version of ['2.0', '02.0.0', '2.0.0-01', '2.0.0"\n', null]) {
    changeSource(root, version);
    assert.throws(() => synchronizeVersion(root, true), /version.invalid_source/);
  }
});

const rustTargets = ['Cargo.toml', 'Cargo.lock', 'crates/lycheedev-binding-contracts/Cargo.toml', 'crates/lycheedev-emmy-adapter/Cargo.toml', 'tests/rust-local-pure/Cargo.lock', 'release/npm-channel.json'];
function rustFixture(t) {
  const root = fixture(t);
  for (const path of rustTargets) {
    mkdirSync(dirname(join(root, path)), {recursive:true});
    writeFileSync(join(root, path), readFileSync(join(repo, path)));
  }
  return root;
}
test('all nine owned lock packages and exact owned dependency advance without third-party lock edits', t => {
  const root = rustFixture(t), path = join(root, 'Cargo.lock');
  const blocks = text => text.split('[[package]]\n').filter(block => !/^name = "lycheedev-/.test(block));
  const original = blocks(readFileSync(path, 'utf8'));
  changeSource(root, '3.5.1'); synchronizeVersion(root, true);
  const lock = readFileSync(path, 'utf8');
  assert.equal((lock.match(/name = "lycheedev-[^"\n]+"\nversion = "3.5.1"/g)||[]).length,9);
  assert.deepEqual(blocks(lock), original);
  assert.match(readFileSync(join(root, rustTargets[2]),'utf8'), /^version = "3.5.1"$/m);
  assert.match(readFileSync(join(root, rustTargets[3]),'utf8'), /version = "=3.5.1"/);
  assert.equal(JSON.parse(readFileSync(join(root,'release/npm-channel.json'))).version,'3.5.1');
  assert.equal(synchronizeVersion(root).changed.length,0);
});
test('missing duplicate registry or unexpected owned lock packages reject before any write', t => {
  const root = rustFixture(t), lockPath = join(root,'Cargo.lock'), original=readFileSync(lockPath,'utf8');
  const block = original.match(/\[\[package\]\]\nname = "lycheedev-emmy-adapter"[\s\S]*?(?=\n\[\[package\]\]|$)/)[0];
  changeSource(root,'3.5.1');
  const before = readFileSync(join(root,'Cargo.toml'));
  for(const value of [original.replace(block,''),original+'\n'+block,original.replace(block,block+'\nsource = "registry+untrusted"\n'),original+'\n[[package]]\nname = "lycheedev-unreviewed"\nversion = "3.5.0"\n']) {
    writeFileSync(lockPath,value);
    assert.throws(()=>synchronizeVersion(root,true),/version.invalid_owned_lock/);
    assert.deepEqual(readFileSync(join(root,'Cargo.toml')),before);
  }
});
test('current independent release agrees across all nine manifests and the unmodified engine dependency', () => {
  const {version}=JSON.parse(readFileSync(join(repo,'release/version.json')));
  const workspace=readFileSync(join(repo,'Cargo.toml'),'utf8');
  const members=[...workspace.match(/^members = \[(.+)\]$/m)[1].matchAll(/"([^"]+)"/g)].map(m=>m[1]);
  assert.equal(members.length,9); assert.equal(new Set(members).size,9);
  for (const member of members) {
    const manifest=readFileSync(join(repo,member,'Cargo.toml'),'utf8');
    assert(manifest.includes('version.workspace = true') || manifest.includes(`version = "${version}"`),member);
  }
  const adapter=readFileSync(join(repo,'crates/lycheedev-emmy-adapter/Cargo.toml'),'utf8');
  assert(adapter.includes(`lycheedev-binding-contracts = { path = "../lycheedev-binding-contracts", version = "=${version}" }`));
  const identity=JSON.parse(readFileSync(join(repo,'third_party/emmy/identity.json')));
  assert.equal(identity.base.commit,'08b0873962e83bb8307e9a81052e3f41578fbc78');
  assert.equal(identity.historicalPrivatePatchRecovered,false);
  assert.equal(identity.identity,'public-upstream-unmodified-v1');
  assert.deepEqual(identity.patches,[]);
  assert.equal(identity.patchedTreeSHA256,identity.base.treeSHA256);
  assert.equal(identity.files,identity.base.files);
  assert.equal(identity.bytes,identity.base.bytes);
});
