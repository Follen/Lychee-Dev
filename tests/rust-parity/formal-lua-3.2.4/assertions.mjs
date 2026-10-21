// Fixed Go 3.2.4 inputs; prior manifests and observations retain their identity.
import assert from 'node:assert/strict';
import {mkdirSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';
import {BASE as PREVIOUS_BASE, PREVIOUS_BASE as HISTORICAL_BASE,
  MANIFEST_SHA256 as PREVIOUS_MANIFEST_SHA256, ORACLE as PREVIOUS_ORACLE,
  CHANGED_FIXTURES as PREVIOUS_CHANGED_FIXTURES, SUPPORT as PREVIOUS_SUPPORT,
  fixturePath as previousFixturePath, manifest as previousManifest,
  PROFILES, gitBlob, hash, regular, tree, verifyMaterial} from '../formal-lua-3.2.3/assertions.mjs';

export {BRIDGE_GAPS, LIMITS, PROFILES, cases, cleanLuaEnvironment, evaluate, gitBlob, hash,
  processPassed, processReaped, regular, runCases, tree, verifyMaterial, verifyOwnerEvidence,
  verifyUnifiedTOC} from '../formal-lua-3.2.3/assertions.mjs';
export {PREVIOUS_BASE, HISTORICAL_BASE};
export const BASE = 'tests/rust-parity/formal-lua-3.2.4';
export const ORACLE = '29bdbdaff5f064ec212ff2064380b1ccac652eda';
export const MANIFEST_SHA256 = 'fb48cb3d55f22b553881bece10be49d451418cdc137814ec05e2964aae93baf6';
export const RELEASE_FIELDS_SHA256 = 'c459550c8f467a1978303aa401d6f3aba5f5ed5f43e77111802d7e78bc3060e4';
export const CHANGED_FIXTURES = Object.freeze(['tests/addon/env.lua', 'tests/addon/t_about.lua', 'tests/addon/t_focus_bindings.lua']);
export const CHANGED_ADDON = Object.freeze(['addon/Bridge/FocusBindings.lua', 'addon/Bridge/InputState.lua', 'addon/Core/Runtime.lua', 'addon/Lychee Dev.toc']);
export const SUPPORT = Object.freeze([
  `${BASE}/assertions.mjs`, `${BASE}/manifest.json`, `${BASE}/release-fields.json`, `${BASE}/go-test-map.json`,
  ...PREVIOUS_SUPPORT,
]);
export function manifest(root) {
  const bytes = regular(join(root, BASE, 'manifest.json'));
  assert.equal(hash(bytes), MANIFEST_SHA256, 'formal-lua.manifest_identity');
  const value = JSON.parse(bytes), previous = previousManifest(root);
  assert.equal(hash(regular(join(root, PREVIOUS_BASE, 'manifest.json'))), PREVIOUS_MANIFEST_SHA256, 'formal-lua.previous_manifest_identity');
  assert.equal(value.schema, 'lycheedev.formal-lua-oracle.v1');
  assert.deepEqual(value.oracle, {release: 'Go v3.2.4', commit: ORACLE});
  assert.deepEqual(value.previousOracle, {...previous.oracle, base: PREVIOUS_BASE, manifestSHA256: PREVIOUS_MANIFEST_SHA256});
  assert.equal(previous.oracle.commit, PREVIOUS_ORACLE);
  assert.deepEqual(value.profiles, PROFILES);
  assert.equal(value.fixtures.length, 58); assert.equal(value.candidateAddon.length, 55);
  for (const [key, changed] of [['fixtures', CHANGED_FIXTURES], ['candidateAddon', CHANGED_ADDON]]) {
    assert.deepEqual(value[key].map(row => row.path), previous[key].map(row => row.path), 'formal-lua.increment_inventory');
    assert.deepEqual(value[key].filter((row, index) => row.sha256 !== previous[key][index].sha256).map(row => row.path), changed, 'formal-lua.increment_changes');
    value[key].forEach((row, index) => {
      if (!changed.includes(row.path)) assert.deepEqual(row, previous[key][index], 'formal-lua.reused_identity');
    });
  }
  const fields = regular(join(root, BASE, 'release-fields.json'));
  assert.equal(hash(fields), RELEASE_FIELDS_SHA256, 'formal-lua.release_fields_identity');
  const retainedReleaseFields = JSON.parse(fields);
  assert.deepEqual(retainedReleaseFields.map(row => row.path), ['addon/Core/Runtime.lua', 'addon/Lychee Dev.toc']);
  return {...value, retainedReleaseFields};
}
export function fixturePath(root, path) {
  return CHANGED_FIXTURES.includes(path) ? join(root, BASE, 'oracle', path) : previousFixturePath(root, path);
}
export function authenticate(root) {
  const expected = manifest(root), previous = previousManifest(root);
  // Preserve both historical byte inventories, including superseded fixtures.
  // The previous checker authenticates both immutable manifest identities.
  const historical = JSON.parse(regular(join(root, HISTORICAL_BASE, 'manifest.json')));
  assert.deepEqual(tree(join(root, HISTORICAL_BASE, 'oracle')), historical.fixtures, 'formal-lua.historical_fixture_identity');
  assert.deepEqual(tree(join(root, PREVIOUS_BASE, 'oracle')), previous.fixtures.filter(row => PREVIOUS_CHANGED_FIXTURES.includes(row.path)), 'formal-lua.previous_fixture_identity');
  assert.deepEqual(tree(join(root, BASE, 'oracle')), expected.fixtures.filter(row => CHANGED_FIXTURES.includes(row.path)), 'formal-lua.increment_fixture_identity');
  const current = tree(join(root, 'addon')).map(row => ({...row, path: `addon/${row.path}`}));
  const lookup = new Map(current.map(row => [row.path, row]));
  const addon = expected.candidateAddon.map(row => {
    const retained = expected.retainedReleaseFields.find(field => field.path === row.path);
    if (!retained) { assert.deepEqual(lookup.get(row.path), row, `formal-lua.candidate_addon_identity: ${row.path}`); return row; }
    assert.deepEqual({bytes: row.bytes, sha256: row.sha256}, retained.expected, 'formal-lua.release_oracle_identity');
    const bytes = regular(join(root, row.path));
    assert.deepEqual({bytes: bytes.length, sha256: hash(bytes)}, retained.retained, 'formal-lua.retained_release_identity');
    const text = bytes.toString('utf8');
    assert.equal(text.split(retained.retainedText).length, 2, 'formal-lua.release_field_cardinality');
    const restored = Buffer.from(text.replace(retained.retainedText, retained.expectedText));
    assert.equal(hash(restored), row.sha256, 'formal-lua.release_only_difference');
    assert.equal(gitBlob(restored), row.gitBlob, 'formal-lua.release_only_git_identity');
    return lookup.get(row.path);
  });
  const required = new Set(expected.candidateAddon.map(row => row.path));
  return {oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, releaseFieldsSHA256: RELEASE_FIELDS_SHA256,
    previousOracle: expected.previousOracle, increment: expected.increment, fixtures: expected.fixtures, addon,
    retainedReleaseFields: expected.retainedReleaseFields, historicalAddonExtras: current.filter(row => !required.has(row.path))};
}
export function materialize(root, target, identity = authenticate(root)) {
  assert.deepEqual(authenticate(root), identity, 'formal-lua.materialize_input_identity');
  mkdirSync(target);
  for (const [rows, fixture] of [[identity.fixtures, true], [identity.addon, false]]) {
    for (const row of rows) {
      const bytes = regular(fixture ? fixturePath(root, row.path) : join(root, row.path));
      assert.equal(bytes.length, row.bytes); assert.equal(hash(bytes), row.sha256); assert.equal(gitBlob(bytes), row.gitBlob);
      const path = join(target, row.path); mkdirSync(dirname(path), {recursive: true}); writeFileSync(path, bytes, {flag: 'wx'});
    }
  }
  verifyMaterial(target, identity);
}
