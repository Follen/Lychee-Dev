// CLI metadata only. This fixed 3.2.3 oracle reuses historical command bytes
// after independent source verification; no historical result is relabeled.
// Never derive expected metadata from Rust commands.json or executed describe.
import assert from 'node:assert/strict';
import { extractPinnedGo, parseLiteral, sha256 } from './oracle.mjs';

export const GO_323 = Object.freeze({
  release: 'Go v3.2.3', commit: '3766f9cacb52e93a803a626187e2a1d54a445468',
});
export const HISTORICAL_FIXTURE_SHA256 = '78e04b069c1760da67caf7dc192fda93b51b56df5769c479c530554496bb363c';
export const GO_323_SOURCES = Object.freeze({
  commands: Object.freeze({path: 'internal/command/command_contract.go',
    sha256: 'b56c73427279a89196dcdebc720827e4eb4bc79f55f0d76f7561d49f69eed9fa'}),
  products: Object.freeze({path: 'internal/selection/products.go',
    sha256: '53921c7bf0adcfde2b39c467ec480a80e70c1e1fa562edc2450c75f19a1d9bb2'}),
});

// Explicit approved engine descriptions. Both old and new strings are exact;
// these do not authorize changes to other metadata or general text replacement.
const summaries = [
  ['doctor',
    'Aggregate read-only workspace, data, LuaLS and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check',
    'Aggregate read-only workspace, data, embedded Emmy worker smoke and source checks; when a live target or CON is selected, include the same checks as live doctor; exit 3 means an unhealthy check'],
  ['source batch',
    'Run 1-32 query/refs/context requests from a JSON file against one fixed snapshot, sharing preparation and a request-scoped LuaLS session; each item retains its own cursor and coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins',
    'Run 1-32 query/refs/context requests from a JSON file against one fixed snapshot, sharing preparation and a request-scoped native Emmy session; each item retains its own cursor and coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins'],
  ['source session status',
    'Inspect the opt-in LuaLS broker for this home without starting it',
    'Inspect the opt-in native Source session for this home without starting it'],
  ['source session close',
    'Retire the verified opt-in LuaLS broker and release its worktree lease',
    'Retire the verified opt-in native Source session and release its worktree lease'],
  ['source validate',
    'Validate one pinned addon closure with --path/--toc/--snapshot, or a fixed multi-client --matrix <config.json>; add --semantic for bundled LuaLS using fixed client API metadata; report unresolved coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins',
    'Validate one pinned addon closure with --path/--toc/--snapshot, or a fixed multi-client --matrix <config.json>; add --semantic for native Emmy analysis using fixed client API metadata; report unresolved coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins'],
];

export function loadCliContractOracle(historicalBytes) {
  assert.equal(sha256(historicalBytes), HISTORICAL_FIXTURE_SHA256, 'historical CLI fixture identity');
  const historical = JSON.parse(historicalBytes);
  assert.equal(historical.oracle.commit, '3057a9666ab078270b63c7f06de748069b0d1f4b');
  assert.equal(historical.commands.length, 84);
  const expected = structuredClone(historical.commands).map(({parserFlags, ...command}) => command);
  for (const [path, before, after] of summaries) {
    const command = expected.find(command => command.path === path);
    assert.equal(command.summary, before, `${path}: exact historical summary`);
    command.summary = after;
  }
  // The one approved Rust addition is independent of the candidate inventory.
  // Full metadata and its insertion point are fixed, not a path-wide exemption.
  const before = expected.findIndex(command => command.path === 'data db2');
  assert.equal(expected[before - 1].path, 'data sql');
  assert.ok(!expected.some(command => command.path === 'data cache prepare'));
  expected.splice(before, 0, {
    path: 'data cache prepare',
    summary: 'Materialize one requested static table as a bounded, authenticated Data generation; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins',
    mutates: true,
    arguments: ['<Table>'],
    flags: ['--home <root>', '--format text|json|jsonl', '--project <directory>',
      '--snapshot <pin>', '--installation <client-or-game-root>', '--cdn', '--offline',
      '--max-bytes <n>', '--key-file <WoW.txt|keys.json>',
      '--timeout-seconds <1-3600> (default 300; entire query)'],
  });
  return {oracle: GO_323, historicalReference: historical.oracle,
    commands: historical.commands, expected};
}

// Independent provenance check for the frozen Git blobs. Callers supply bytes
// from GO_323 and the historical products blob, never a moving branch/describe.
// This verifier executes no Go, Rust, Lua, repository config or product code.
export function verifyGo323CommandInheritance({commands, products, historicalProducts}, historicalBytes) {
  for (const [name, bytes] of Object.entries({commands, products})) {
    assert.equal(sha256(bytes), GO_323_SOURCES[name].sha256, `Go 3.2.3 ${name} source identity`);
  }
  const {commands: frozen} = loadCliContractOracle(historicalBytes);
  // The command source is byte-identical to 3.2.0; extractPinnedGo authenticates
  // both inputs before applying its original reviewed public-definition adapter.
  const extracted = extractPinnedGo(commands, historicalProducts);
  assert.deepEqual(extracted.commands, frozen, 'source-derived commands and all parser flags');
  const literals = [...products.toString().matchAll(/func DataProducts\(\) \[\]DataProductProfile \{\s*return (\[\]DataProductProfile\{[\s\S]*?\n\t\})\s*\}/g)];
  assert.equal(literals.length, 1, 'fixed current product literal');
  // Only names/order feed CLI dataProductFlag. Other profile fields changed
  // between releases and are deliberately outside this CLI-only inheritance.
  const names = parseLiteral(literals[0][1]).map(product => product.Product);
  assert.deepEqual(names, extracted.products.map(product => product.product), 'CLI product names/order');
  return {oracle: GO_323, commandCount: frozen.length, productNames: names,
    sources: GO_323_SOURCES, historicalFixtureSHA256: HISTORICAL_FIXTURE_SHA256};
}
