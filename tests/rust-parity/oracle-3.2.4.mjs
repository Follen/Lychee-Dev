// Current, source-only Go 3.2.4 reference plus separately approved Rust deltas.
// No historical fixture, adapter, or result is rewritten or relabeled.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import { GO_COMMIT, LIMITS, SOURCES, readBounded, sha256, compare, diffByKey } from './oracle.mjs';
import { extractCurrentProducts } from './oracle-3.2.2.mjs';
import { GO_324, loadCliContractOracle, verifyGo324CommandInheritance } from './cli-contract-oracle-3.2.4.mjs';
import { CANDIDATE_RELEASE_FIELDS } from './rust-3.5.0-release-fields.mjs';
export { GO_324 };

// Only the two release fields accept the exact independent Rust 3.5.0 bytes.
// Every other addon file, including 3.2.4 binding/input fixes, must match Go 3.2.4.
const release324Hashes = Object.freeze({
  'addon/Core/Runtime.lua': '41abb954717ddfa43ceb1b9de318d877614e60439e79b524432415e5d12cbd9b',
  'addon/Lychee Dev.toc': '76d83edeb0bd52719bd41e316a5f1c9ca285e5c67f4e51531ddf41fe6344b175',
});
assert.deepEqual(CANDIDATE_RELEASE_FIELDS.map(row => row.path).sort(), Object.keys(release324Hashes).sort());
export const RELEASE_324_FIELDS = Object.freeze(CANDIDATE_RELEASE_FIELDS.map(row => Object.freeze({
  path: row.path,
  expected: Object.freeze({bytes: row.expected.bytes, sha256: release324Hashes[row.path]}),
  retained: row.retained,
  expectedText: row.expectedText.replace('3.2.2', '3.2.4'), retainedText: row.retainedText,
})));
function git(root, args) {
  return execFileSync('git', ['--no-replace-objects', '--no-pager', '--no-optional-locks', '-c', 'core.fsmonitor=false', ...args],
    {cwd: root, timeout: 10000, maxBuffer: LIMITS.fileBytes, stdio: ['ignore', 'pipe', 'pipe']});
}
function source(root, path, commit = GO_324.commit) { return git(root, ['cat-file', 'blob', `${commit}:${path}`]); }
export function expectedArities(commands) {
  const result = Object.create(null);
  for (const command of commands) for (const flag of command.parserFlags) {
    if (Object.hasOwn(result, flag.name)) assert.equal(result[flag.name], flag.value);
    result[flag.name] = flag.value;
  }
  assert.equal(Object.keys(result).length, 115, 'exact frozen arity inventory');
  assert.ok(!Object.hasOwn(result, '--data-derived'));
  result['--data-derived'] = false;
  assert.ok(!Object.hasOwn(result, '--result-ttl-seconds'));
  result['--result-ttl-seconds'] = true;
  assert.ok(!Object.hasOwn(result, '--data'));
  result['--data'] = false;
  return result;
}
export function buildRelease324Fixture(root) {
  const historicalBytes = readBounded(root, 'tests/rust-parity/go-3.2.0.manifest.json');
  const cli = loadCliContractOracle(historicalBytes);
  const commands = source(root, SOURCES.commands), products = source(root, SOURCES.products);
  const provenance = verifyGo324CommandInheritance({commands, products,
    historicalProducts: source(root, SOURCES.products, GO_COMMIT)}, historicalBytes);
  const entries = git(root, ['ls-tree', '--full-tree', '-rz', GO_324.commit, '--', 'addon']).toString().split('\0').filter(Boolean);
  assert.equal(entries.length, 55, 'fixed Go 3.2.4 addon inventory');
  let total = 0;
  const addon = entries.map(entry => {
    const m = /^(100644|100755) blob ([0-9a-f]{40})\t(addon\/[^\r\n]+)$/.exec(entry);
    assert.ok(m && !/[\\:\x00-\x1f]/.test(m[3]) && m[3].split('/').every(c => c && c !== '.' && c !== '..'), 'fixed regular addon path');
    const bytes = git(root, ['cat-file', 'blob', m[2]]);
    total += bytes.length; assert.ok(total <= LIMITS.totalAddonBytes);
    return {path: m[3], bytes: bytes.length, sha256: sha256(bytes)};
  });
  for (const row of RELEASE_324_FIELDS) {
    const bytes = source(root, row.path);
    assert.deepEqual({bytes: bytes.length, sha256: sha256(bytes)}, row.expected);
    const text = bytes.toString('utf8');
    assert.equal(text.split(row.expectedText).length, 2, 'one exact release literal');
    const retained = Buffer.from(text.replace(row.expectedText, row.retainedText));
    assert.deepEqual({bytes: retained.length, sha256: sha256(retained)}, row.retained, 'release-only candidate delta');
  }
  const arities = expectedArities(cli.commands);
  const acceptedCommands = cli.expected.map(command => ({...command, parserFlags: command.flags.map(display => {
    const name = display.split(/\s+/)[0];
    assert.ok(Object.hasOwn(arities, name), `unreviewed expected flag: ${name}`);
    return {name, display, value: arities[name]};
  })}));
  return {schema: 'lycheedev.release-324-static-oracle.v1', oracle: GO_324,
    commands: cli.commands, products: extractCurrentProducts(products), addon,
    acceptedCommands, expectedArities: arities, provenance,
    sourceIdentities: Object.entries({commands, products}).map(([key, bytes]) => ({path: SOURCES[key], bytes: bytes.length, sha256: sha256(bytes)}))};
}
export function compareRelease324(oracle, candidate) {
  assert.equal(oracle?.oracle?.commit, GO_324.commit, 'fixed Go 3.2.4 required');
  const rawExact = compare(oracle, candidate);
  const acceptedAddon = oracle.addon.map(row => {
    const allowed = RELEASE_324_FIELDS.find(allowed => allowed.path === row.path);
    if (!allowed) return row;
    assert.deepEqual(row, {path: allowed.path, ...allowed.expected}, 'fixed release reference');
    return {path: allowed.path, ...allowed.retained};
  });
  const accepted = compare({...oracle, commands: oracle.acceptedCommands, addon: acceptedAddon}, candidate);
  // compare() checks complete declared fields; also reject unknown keys and order drift.
  const expectedMetadata = oracle.acceptedCommands.map(({parserFlags, ...row}) => row);
  const commandMetadata = {state: isDeepStrictEqual(candidate.inventory, expectedMetadata) ? 'pass' : 'fail',
    expectedCount: expectedMetadata.length, actualCount: candidate.inventory.length};
  const profiles = diffByKey(oracle.products, candidate.products.map(p => ({product: p.id, family: p.family,
    productCode: p.catalog_product, folder: p.folder, buildSeries: p.series, reusedSlot: p.reused_slot})),
  'product', ['family', 'productCode', 'folder', 'buildSeries', 'reusedSlot']);
  const parserInventory = candidate.arities === null ? {state: 'unverified'} : diffByKey(
    Object.entries(oracle.expectedArities).map(([name, value]) => ({name, value})),
    Object.entries(candidate.arities).map(([name, value]) => ({name, value})), 'name', ['value']);
  const states = [accepted.staticStatus, commandMetadata.state, profiles.state, parserInventory.state];
  return {schema: 'lycheedev.release-324-candidate-static-differential.v1', oracle: oracle.oracle,
    rawExactStatus: rawExact.staticStatus,
    acceptedStaticStatus: states.includes('fail') ? 'fail' : states.some(s => s !== 'pass') ? 'unverified' : 'pass',
    rawExact, accepted, commandMetadata, profiles, parserInventory, provenance: oracle.provenance,
    sourceIdentities: oracle.sourceIdentities, acceptedReleaseFields: RELEASE_324_FIELDS,
    scope: 'Fixed Go 3.2.4 source declarations, seven full product profiles, and exact addon bytes; exact approved CLI extensions and release-only deltas are separate from raw differences.',
    behavioralParity: 'unverified', execution: 'NOT_RUN'};
}
