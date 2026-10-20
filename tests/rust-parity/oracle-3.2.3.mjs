// Current, source-only Go 3.2.3 reference plus separately approved Rust deltas.
// No historical fixture, adapter, or result is rewritten or relabeled.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import { GO_COMMIT, LIMITS, SOURCES, readBounded, sha256, compare, diffByKey } from './oracle.mjs';
import { extractCurrentProducts } from './oracle-3.2.2.mjs';
import { GO_323, loadCliContractOracle, verifyGo323CommandInheritance } from './cli-contract-oracle-3.2.3.mjs';
import { CANDIDATE_RELEASE_FIELDS } from './rust-3.5.0-release-fields.mjs';
export { GO_323 };

// These complete Go 3.2.3 identities differ from 3.2.2 only by release text.
// The retained candidate identities were already reviewed for Rust 3.5.0.
const release323Hashes = Object.freeze({
  'addon/Core/Runtime.lua': '282cc677046af77325bd20c88fed24bcbb62aafddbe0bf7ff7ce1bb46dc454df',
  'addon/Lychee Dev.toc': '0b8f7eef49e6834e543576f6276224c43f4b288cc628e0e7564a40df03e4bedc',
});
assert.deepEqual(CANDIDATE_RELEASE_FIELDS.map(row => row.path).sort(), Object.keys(release323Hashes).sort());
export const RELEASE_323_FIELDS = Object.freeze(CANDIDATE_RELEASE_FIELDS.map(row => Object.freeze({
  path: row.path,
  expected: Object.freeze({bytes: row.expected.bytes, sha256: release323Hashes[row.path]}),
  retained: row.retained,
  expectedText: row.expectedText.replace('3.2.2', '3.2.3'), retainedText: row.retainedText,
})));
function git(root, args) {
  return execFileSync('git', ['--no-replace-objects', '--no-pager', '--no-optional-locks', '-c', 'core.fsmonitor=false', ...args],
    {cwd: root, timeout: 10000, maxBuffer: LIMITS.fileBytes, stdio: ['ignore', 'pipe', 'pipe']});
}
function source(root, path, commit = GO_323.commit) { return git(root, ['cat-file', 'blob', `${commit}:${path}`]); }
export function expectedArities(commands) {
  const result = Object.create(null);
  for (const command of commands) for (const flag of command.parserFlags) {
    if (Object.hasOwn(result, flag.name)) assert.equal(result[flag.name], flag.value);
    result[flag.name] = flag.value;
  }
  assert.equal(Object.keys(result).length, 115, 'exact frozen arity inventory');
  assert.ok(!Object.hasOwn(result, '--data-derived'));
  result['--data-derived'] = false;
  return result;
}
export function buildRelease323Fixture(root) {
  const historicalBytes = readBounded(root, 'tests/rust-parity/go-3.2.0.manifest.json');
  const cli = loadCliContractOracle(historicalBytes);
  const commands = source(root, SOURCES.commands), products = source(root, SOURCES.products);
  const provenance = verifyGo323CommandInheritance({commands, products,
    historicalProducts: source(root, SOURCES.products, GO_COMMIT)}, historicalBytes);
  const entries = git(root, ['ls-tree', '--full-tree', '-rz', GO_323.commit, '--', 'addon']).toString().split('\0').filter(Boolean);
  assert.equal(entries.length, 55, 'fixed Go 3.2.3 addon inventory');
  let total = 0;
  const addon = entries.map(entry => {
    const m = /^(100644|100755) blob ([0-9a-f]{40})\t(addon\/[^\r\n]+)$/.exec(entry);
    assert.ok(m && !/[\\:\x00-\x1f]/.test(m[3]) && m[3].split('/').every(c => c && c !== '.' && c !== '..'), 'fixed regular addon path');
    const bytes = git(root, ['cat-file', 'blob', m[2]]);
    total += bytes.length; assert.ok(total <= LIMITS.totalAddonBytes);
    return {path: m[3], bytes: bytes.length, sha256: sha256(bytes)};
  });
  for (const row of RELEASE_323_FIELDS) {
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
  return {schema: 'lycheedev.release-323-static-oracle.v1', oracle: GO_323,
    commands: cli.commands, products: extractCurrentProducts(products), addon,
    acceptedCommands, expectedArities: arities, provenance,
    sourceIdentities: Object.entries({commands, products}).map(([key, bytes]) => ({path: SOURCES[key], bytes: bytes.length, sha256: sha256(bytes)}))};
}
export function compareRelease323(oracle, candidate) {
  assert.equal(oracle?.oracle?.commit, GO_323.commit, 'fixed Go 3.2.3 required');
  const rawExact = compare(oracle, candidate);
  const acceptedAddon = oracle.addon.map(row => {
    const allowed = RELEASE_323_FIELDS.find(allowed => allowed.path === row.path);
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
  return {schema: 'lycheedev.release-323-candidate-static-differential.v1', oracle: oracle.oracle,
    rawExactStatus: rawExact.staticStatus,
    acceptedStaticStatus: states.includes('fail') ? 'fail' : states.some(s => s !== 'pass') ? 'unverified' : 'pass',
    rawExact, accepted, commandMetadata, profiles, parserInventory, provenance: oracle.provenance,
    sourceIdentities: oracle.sourceIdentities, acceptedReleaseFields: RELEASE_323_FIELDS,
    scope: 'Fixed Go 3.2.3 source declarations, seven full product profiles, and exact addon bytes; exact approved CLI extensions and release-only deltas are separate from raw differences.',
    behavioralParity: 'unverified', execution: 'NOT_RUN'};
}
