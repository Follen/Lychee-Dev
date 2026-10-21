// Read-only quarantine checks. Never execute an archived Lua/Rust source file.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {lstatSync, readFileSync, readdirSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import test from 'node:test';
import {ORACLE as CURRENT_GO_ORACLE} from '../tests/rust-parity/formal-lua-3.2.4/assertions.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const archive = 'tests/history/native-v4-6699bbff';
const RETIREMENT_GO_ORACLE = '3766f9cacb52e93a803a626187e2a1d54a445468';
const originals = [
  'addon/Bridge/FaultRunner.lua',
  'addon/Bridge/NativeAdapter.lua',
  'addon/Bridge/NativeInputState.lua',
  'addon/Bridge/NativeProtocol.lua',
  'addon/Bridge/NativeRuntime.lua',
  'addon/Bridge/NativeWire.lua',
  'addon/Lychee Dev Rust.toc',
  'crates/lycheedev-live/tests/native_e2e.rs',
  'protocol/native-v4-adapter-host-fixture.lua',
  'protocol/native-v4-cost-test.lua',
  'protocol/native-v4-host-fixture.lua',
  'protocol/native-v4-routing-test.lua',
  'protocol/native-v4-test.lua',
  'tests/addon/t_native_adapter.lua',
  'tests/addon/t_native_candidate_closure.lua',
  'tests/addon/t_native_v4.lua',
];
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
function read(path) {
  assert(!path.split('/').some(part => !part || part === '.' || part === '..'));
  const full = join(root, path);
  let cursor = full;
  for (;;) {
    assert(!lstatSync(cursor).isSymbolicLink(), `symlink: ${cursor}`);
    const parent = dirname(cursor);
    if (parent === cursor) break;
    cursor = parent;
  }
  const info = lstatSync(full);
  assert(info.isFile() && info.size <= (2 << 20), path);
  const bytes = readFileSync(full);
  assert.equal(bytes.length, info.size, path);
  return bytes;
}
function manifest() {
  const bytes = read(`${archive}/manifest.json`);
  // The immutable source manifest is itself fixed, independently of its rows.
  assert.equal(digest(bytes), 'c9988f7763c634921a588088b9e1023bfcedd55c267c93b62306b88c3a860574');
  const value = JSON.parse(bytes);
  assert.equal(value.schema, 'lycheedev.legacy-native-v4-source.v1');
  assert.equal(value.sourceCommit, '6699bbff2d04075e25fafd587cb0de1618561f01');
  // The frozen archive records the Go baseline when these sources retired.
  assert.equal(value.currentGoOracle, RETIREMENT_GO_ORACLE);
  assert.equal(value.state, 'historical_source_only');
  assert.equal(value.executableEntry, null);
  assert.deepEqual(value.files.map(row => row.originalPath), originals);
  return value;
}

test('all sixteen retired files retain exact original bytes and identities', () => {
  const value = manifest();
  for (const row of value.files) {
    assert.equal(row.historicalPath, `${archive}/source/${row.originalPath}`);
    assert.equal(row.historicalSHA256, row.originalSHA256);
    assert(row.reason.length > 40, row.originalPath);
    const bytes = read(row.historicalPath);
    assert.equal(bytes.length, row.bytes, row.originalPath);
    assert.equal(digest(bytes), row.originalSHA256, row.originalPath);
    assert.equal(createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex'), row.gitBlob);
    assert.throws(() => lstatSync(join(root, row.originalPath)), {code: 'ENOENT'});
  }
});

test('historical source is exactly the frozen set, with no runnable package or shim', () => {
  const found = [];
  let entries = 0;
  function walk(path) {
    assert(++entries <= 128, 'archive entry budget');
    const info = lstatSync(join(root, path));
    assert(!info.isSymbolicLink(), path);
    if (info.isDirectory()) {
      for (const child of readdirSync(join(root, path)).sort()) walk(`${path}/${child}`);
    } else {
      assert(info.isFile(), path);
      found.push(path);
    }
  }
  walk(`${archive}/source`);
  assert.deepEqual(found.sort(), originals.map(path => `${archive}/source/${path}`).sort());
  assert(!found.some(path => /\/(?:Cargo.toml|package.json|[^/]*_test.go)$/.test(path)));
  assert.match(read(`${archive}/README.md`).toString(), /There is no executable entry for this archive/);
});

test('current discovery keeps the formal Driver and excludes only the historical native roots', () => {
  const currentBytes = read('tests/history/legacy-ui-00fd643/current-oracle-inputs-3.2.4.json');
  assert.equal(digest(currentBytes), 'd6e6f3ee9128388237e5a9462e4a9976a8282af6f434eb49f91961559089c5d8');
  const current = JSON.parse(currentBytes);
  assert.equal(current.oracleCommit, CURRENT_GO_ORACLE);
  assert.equal(current.previousOracle.commit, RETIREMENT_GO_ORACLE);
  const addonTests = read('tests/addon/addon_test.go').toString();
  assert(addonTests.includes('filepath.Glob("t_*.lua")'));
  const catalog = read('tests/baseline/catalog.mjs').toString();
  assert.match(catalog, /go\('BASE-17'[^\n]*'tests\/addon', 'TestWorkbenchLuaSuites', 'TestUnifiedTOCContract'\)/);
  const discovered = readdirSync(join(root, 'tests/addon')).filter(path => /^t_.*\.lua$/.test(path));
  for (const name of ['t_native_adapter.lua', 't_native_candidate_closure.lua', 't_native_v4.lua']) {
    assert(!discovered.includes(name), name);
  }
  const cargo = read('crates/lycheedev-live/Cargo.toml').toString();
  assert(!cargo.includes('native_e2e'), 'do not add an explicit historical integration target');
  const driver = read('crates/lycheedev-live/tests/formal_lua_bridge.rs').toString();
  assert(driver.includes('driver::Driver'));
  assert(driver.includes('durable_journal::DurableChannelJournal'));
  assert.equal((driver.match(/^case!\(formal_lua_recovery_/gm) ?? []).length, 9);
});

test('formal addon loads and rejection fixtures remain separate from historical source', () => {
  const toc = read('addon/Lychee Dev.toc').toString();
  const selected = toc.split(/\r?\n/).map(line => line.trim()).filter(line => line && !line.startsWith('#'));
  for (const path of originals.filter(path => path.startsWith('addon/'))) {
    assert(!selected.some(line => `addon/${line.replaceAll('\\', '/')}` === path), path);
  }
  assert(selected.includes('Bridge\\SlotRuntime.lua'));
  assert(selected.includes('Bridge\\MemoryProtocol.lua'));
  const rejected = read('tools/addon-package.test.mjs').toString();
  for (const path of originals.filter(path => path.startsWith('addon/'))) {
    assert(rejected.includes(`'${path.slice('addon/'.length)}'`), path);
  }
  const delivery = read('tests/rust-parity/addon-delivery-3.2.2.test.mjs').toString();
  assert(delivery.includes("assert.ok(!source.includes('Bridge/NativeRuntime.lua'), path)"));
});
