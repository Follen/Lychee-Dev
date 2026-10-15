// Current source-derived oracle. Historical oracle.mjs and its fixture stay immutable.
import {execFileSync} from 'node:child_process';
import {isDeepStrictEqual} from 'node:util';
import {buildFixture, compare, diffByKey, LIMITS, parseLiteral, readBounded, sha256, SOURCES, GO_COMMIT} from './oracle.mjs';
import {compareCurrentDescribeContract} from './describe-contract.mjs';
export const CURRENT_GO_COMMIT = '10b561db5706702cd42e047eaeed4ccb5e825351';
export const CURRENT_FIXTURE = 'tests/rust-parity/go-3.2.1.manifest.json';
const assert = (ok, message) => { if (!ok) throw new Error(message); };
const hashes = Object.freeze({commands: 'b56c73427279a89196dcdebc720827e4eb4bc79f55f0d76f7561d49f69eed9fa', products: '53921c7bf0adcfde2b39c467ec480a80e70c1e1fa562edc2450c75f19a1d9bb2'});
// Exact reviewed full-file identities; not regex/path-wide version normalization.
export const RETAINED_RELEASE_FIELDS = Object.freeze([
  Object.freeze({path: 'addon/Core/Runtime.lua', expected: Object.freeze({bytes: 2615, sha256: '86b0fdf6c2cf92517ae75b0552697335da90e94d2cf71593e81c8b5556bfb142'}), retained: Object.freeze({bytes: 2615, sha256: '27a38b0933e3a73dd03efd985063be326669ea17d2de445bf1431b6935897d95'}), expectedText: 'ns.Release = "3.2.1"', retainedText: 'ns.Release = "3.2.0"'}),
  Object.freeze({path: 'addon/Lychee Dev.toc', expected: Object.freeze({bytes: 2005, sha256: 'b84eded88afdf96645b283f4c4c6782438d59e9fe61d8a0ffa7f162ca46d8b98'}), retained: Object.freeze({bytes: 2005, sha256: '5c0a6ef52611166a32c61b6be440d4371543fb03be724feb6a139b3563a76d64'}), expectedText: '## Version: 3.2.1', retainedText: '## Version: 3.2.0'}),
]);
function git(root, args) {
  return execFileSync('git', ['--no-replace-objects', '--no-pager', '--no-optional-locks', '-c', 'core.fsmonitor=false', ...args], {cwd: root, timeout: 10000, maxBuffer: LIMITS.fileBytes, stdio: ['ignore', 'pipe', 'pipe']});
}
function source(root, path, commit = CURRENT_GO_COMMIT) { return git(root, ['cat-file', 'blob', `${commit}:${path}`]); }
function inventory(root, testSources = false) {
  const entries = git(root, ['ls-tree', '--full-tree', '-rz', CURRENT_GO_COMMIT, ...(testSources ? [] : ['--', 'addon'])]).toString().split('\0').filter(Boolean).filter(e => !testSources || /\t[^\r\n]+_test\.go$/.test(e));
  assert(entries.length === (testSources ? 361 : 55), 'current oracle tree count changed');
  let total = 0;
  return entries.map(entry => {
    const m = /^(100644|100755) blob ([0-9a-f]{40})\t([^\r\n]+)$/.exec(entry);
    assert(m && !/[\\:\x00-\x1f]/.test(m[3]) && m[3].split('/').every(c => c && c !== '.' && c !== '..'), 'invalid pinned tree path');
    const bytes = git(root, ['cat-file', 'blob', m[2]]);
    total += bytes.length; assert(total <= LIMITS.totalAddonBytes, 'current oracle inventory byte limit');
    return {path: m[3], bytes: bytes.length, sha256: sha256(bytes)};
  });
}
export function extractCurrentProducts(bytes) {
  assert(sha256(bytes) === hashes.products, 'current products source hash mismatch');
  // The complete source is hash-locked before this intentionally narrow extraction.
  const matches = [...bytes.toString().matchAll(/func DataProducts\(\) \[\]DataProductProfile \{\s*return (\[\]DataProductProfile\{[\s\S]*?\n\t\})\s*\}/g)];
  assert(matches.length === 1, 'current product literal shape changed');
  const rows = parseLiteral(matches[0][1]);
  assert(rows.length === 7 && new Set(rows.map(p => p.Product)).size === 7, 'current products must have seven unique identities');
  return rows.map(p => ({product: p.Product, family: p.Family, productCode: p.ProductCode, folder: p.Folder, buildSeries: p.BuildSeries, reusedSlot: p.ReusedSlot ?? false}));
}
export function buildCurrentFixture(root) {
  // A frozen stage can live below its Git checkout; tree pathspecs remain
  // repository-root relative. Candidate files still come from the supplied root.
  const objectRoot = git(root, ['rev-parse', '--show-toplevel']).toString().trim();
  const historical = buildFixture(objectRoot);
  const historicalBytes = readBounded(root, 'tests/rust-parity/go-3.2.0.manifest.json');
  assert(historicalBytes.equals(Buffer.from(JSON.stringify(historical, null, 2) + '\n')), 'historical fixture must independently authenticate');
  const commands = source(root, SOURCES.commands), productsBytes = source(root, SOURCES.products);
  assert(sha256(commands) === hashes.commands && commands.equals(source(root, SOURCES.commands, GO_COMMIT)), 'current command source must equal authenticated historical command source before adapter reuse');
  const products = extractCurrentProducts(productsBytes);
  assert(isDeepStrictEqual(products.map(p => p.product), historical.products.map(p => p.product)), 'shared command dataProductFlag identities/order changed');
  const addon = inventory(root);
  for (const allowed of RETAINED_RELEASE_FIELDS) {
    const expected = source(root, allowed.path);
    assert(isDeepStrictEqual({bytes: expected.length, sha256: sha256(expected)}, allowed.expected), 'reviewed release source changed');
    const text = expected.toString();
    assert(text.split(allowed.expectedText).length === 2, 'release field occurrence changed');
    const retained = Buffer.from(text.replace(allowed.expectedText, allowed.retainedText));
    assert(isDeepStrictEqual({bytes: retained.length, sha256: sha256(retained)}, allowed.retained), 'retained release bytes changed');
  }
  return {schema: 'lycheedev.rust-static-oracle.v1', oracle: {release: 'Go v3.2.1', commit: CURRENT_GO_COMMIT},
    extraction: 'Bounded source extraction from fixed 10b objects. Commands reuse the historical reviewed adapter only after whole command source byte equality and exact product-name/order verification. No repository code execution.',
    historicalReference: {release: historical.oracle.release, commit: GO_COMMIT, fixtureSHA256: sha256(historicalBytes), state: 'independently-authenticated; historical results not relabeled'},
    sources: [{path: SOURCES.commands, bytes: commands.length, sha256: sha256(commands)}, {path: SOURCES.products, bytes: productsBytes.length, sha256: sha256(productsBytes)}],
    commands: historical.commands, products, addon,
    retainedReleaseFields: RETAINED_RELEASE_FIELDS,
    goTestReference: {scope: 'All 361 tracked *_test.go files at fixed 3.2.1; inventory only, not translated or executed coverage.', mappingState: 'unreviewed', files: inventory(root, true)}};
}
export function loadCurrentFixture(root) {
  const oracle = buildCurrentFixture(root), bytes = readBounded(root, CURRENT_FIXTURE);
  assert(bytes.equals(Buffer.from(JSON.stringify(oracle, null, 2) + '\n')), 'current fixture differs from fixed released source');
  return {oracle, fixtureSHA256: sha256(bytes)};
}
export function compareCurrent(oracle, candidate) {
  assert(oracle?.oracle?.commit === CURRENT_GO_COMMIT, 'current comparison requires fixed 3.2.1');
  const raw = compare(oracle, candidate);
  const commandMetadata = compareCurrentDescribeContract(oracle, {schema: 'lycheedev.result.v1', ok: true, error: null, result: {commands: candidate.inventory.map(({parserFlags, ...d}) => d)}});
  const profiles = diffByKey(oracle.products, candidate.products.map(p => ({product: p.id, family: p.family, productCode: p.catalog_product, folder: p.folder, buildSeries: p.series, reusedSlot: p.reused_slot})), 'product', ['family', 'productCode', 'folder', 'buildSeries', 'reusedSlot']);
  const acceptedReleaseFields = [], unapprovedAddonChanges = [];
  for (const change of raw.addon.changed) {
    const allowed = RETAINED_RELEASE_FIELDS.find(row => row.path === change.path);
    const expected = oracle.addon.find(row => row.path === change.path), actual = candidate.addon.find(row => row.path === change.path);
    if (allowed && isDeepStrictEqual(expected, {path: allowed.path, ...allowed.expected}) && isDeepStrictEqual(actual, {path: allowed.path, ...allowed.retained})) acceptedReleaseFields.push(allowed);
    else unapprovedAddonChanges.push(change);
  }
  const addon = {state: !raw.addon.missing.length && !raw.addon.extra.length && !unapprovedAddonChanges.length ? 'pass' : 'fail', acceptedReleaseFields, unapprovedChanges: unapprovedAddonChanges};
  const checks = [commandMetadata.state, profiles.state, addon.state, raw.flags.state, raw.parserFlagArity.state, raw.advertised.state];
  const acceptedStaticStatus = checks.includes('fail') ? 'fail' : checks.some(s => s !== 'pass') ? 'unverified' : 'pass';
  return {schema: 'lycheedev.rust-current-static-differential.v1', oracle: oracle.oracle,
    historicalReference: oracle.historicalReference, rawExactStatus: raw.staticStatus, acceptedStaticStatus,
    rawExact: raw, accepted: {commandMetadata, profiles, addon},
    scope: 'Current 3.2.1 static CLI declarations, exact full profiles and addon inventory; only enumerated engine wording and retained release bytes accepted. No runtime/handler/Windows/game or package-release acceptance.',
    behavioralParity: 'unverified', execution: 'NOT_RUN'};
}
