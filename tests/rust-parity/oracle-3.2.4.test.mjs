// Offline static provenance and negative controls; no product execution claim.
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readBounded, sha256 } from './oracle.mjs';
import { GO_323 } from './cli-contract-oracle-3.2.3.mjs';
import { GO_324_SOURCES, loadCliContractOracle, verifyGo324CommandInheritance } from './cli-contract-oracle-3.2.4.mjs';
import { GO_324, RELEASE_324_FIELDS, buildRelease324Fixture, compareRelease324 } from './oracle-3.2.4.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const blob = (commit, path) => execFileSync('git', ['--no-replace-objects', 'cat-file', 'blob', `${commit}:${path}`],
  {cwd: root, timeout: 10000});
const oracle = buildRelease324Fixture(root);
const historical = readBounded(root, 'tests/rust-parity/go-3.2.0.manifest.json');
function candidate() {
  const inventory = oracle.acceptedCommands.map(({parserFlags, ...row}) => structuredClone(row));
  return {inventory, arities: structuredClone(oracle.expectedArities),
    advertised: {state: 'static-extracted', commands: structuredClone(inventory)},
    products: oracle.products.map(p => ({id: p.product, family: p.family, catalog_product: p.productCode,
      folder: p.folder, series: p.buildSeries, reused_slot: p.reusedSlot})),
    addon: oracle.addon.map(row => {
      const release = RELEASE_324_FIELDS.find(release => release.path === row.path);
      return release ? {path: row.path, ...release.retained} : structuredClone(row);
    }), sourceTracks: {forever: 'forever'}};
}
test('fixed Go 3.2.4 authenticates unchanged CLI inheritance and its own complete addon tree', () => {
  assert.equal(GO_324.commit, '29bdbdaff5f064ec212ff2064380b1ccac652eda');
  assert.equal(GO_324.release, 'Go v3.2.4');
  assert.equal(oracle.oracle.commit, GO_324.commit);
  assert.equal(oracle.commands.length, 84); assert.equal(oracle.acceptedCommands.length, 71);
  assert.equal(oracle.products.length, 7); assert.equal(oracle.addon.length, 55);
  assert.equal(oracle.provenance.oracle.commit, GO_324.commit);
  assert.equal(oracle.provenance.inheritedMetadataReference.commit, GO_323.commit);
  assert.equal(oracle.provenance.historicalFixtureSHA256, sha256(historical));
  for (const [name, source] of Object.entries(GO_324_SOURCES)) {
    assert.equal(sha256(blob(GO_324.commit, source.path)), source.sha256, name);
    assert.deepEqual(blob(GO_324.commit, source.path), blob(GO_323.commit, source.path));
  }
  const result = compareRelease324(oracle, candidate());
  assert.equal(result.acceptedStaticStatus, 'pass'); assert.equal(result.rawExactStatus, 'fail');
  assert.equal(result.behavioralParity, 'unverified'); assert.equal(result.execution, 'NOT_RUN');
  assert.equal(loadCliContractOracle(historical).historicalReference.commit, '3057a9666ab078270b63c7f06de748069b0d1f4b');
});
test('3.2.4 binding and input fixes require exact current upstream bytes, with no extra allowance', () => {
  assert.deepEqual(RELEASE_324_FIELDS.map(row => row.path), ['addon/Core/Runtime.lua', 'addon/Lychee Dev.toc']);
  for (const path of ['addon/Bridge/FocusBindings.lua', 'addon/Bridge/InputState.lua']) {
    const current = blob(GO_324.commit, path), old = blob(GO_323.commit, path);
    assert.notEqual(sha256(current), sha256(old));
    assert.deepEqual(oracle.addon.find(row => row.path === path), {path, bytes: current.length, sha256: sha256(current)});
    const c = candidate();
    Object.assign(c.addon.find(row => row.path === path), {bytes: old.length, sha256: sha256(old)});
    assert.equal(compareRelease324(oracle, c).acceptedStaticStatus, 'fail');
  }
});
test('current oracle rejects relabeling, corrupt provenance and all addon identity changes', () => {
  assert.throws(() => compareRelease324({...oracle, oracle: GO_323}, candidate()), /fixed Go 3.2.4 required/);
  assert.throws(() => verifyGo324CommandInheritance({commands: Buffer.from('changed')}, historical), /Go 3.2.4 commands source identity/);
  assert.throws(() => loadCliContractOracle(Buffer.concat([historical, Buffer.from(' ')])), /historical CLI fixture identity/);
  for (const row of oracle.addon) {
    const c = candidate(); c.addon.find(item => item.path === row.path).sha256 = '0'.repeat(64);
    assert.equal(compareRelease324(oracle, c).acceptedStaticStatus, 'fail', row.path);
  }
});
test('current 3.2.4 entry rejects invalid invocation before any candidate comparison', () => {
  const result = spawnSync(process.execPath, ['tests/rust-parity/run-3.2.4.mjs', '--accept-gaps'],
    {cwd: root, encoding: 'utf8', timeout: 20000});
  assert.equal(result.status, 2); assert.match(result.stderr, /Usage:/);
});
