// Dedicated B01 describe and first fixed-Retail index/query comparison only.
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {APPROVED_ENGINE_SUMMARIES} from '../rust-parity/describe-contract.mjs';
import {assertAnchor, assertEnvelope} from '../rust-source-full-corpus/assertions.mjs';

export const GO_COMMIT = '3766f9cacb52e93a803a626187e2a1d54a445468';
export const REPETITIONS = 20;
export const TARGET_RATIO = 0.30;
export const CASES = Object.freeze([
  {id: 'B01-describe', args: ['describe']},
  {id: 'B02-retail-index', args: ['source', 'index']},
  {id: 'B02-retail-query', args: ['source', 'query', 'GetActionButtonForID', '--mode', 'precise', '--topic', 'lua', '--limit', '10']},
]);
export const orderFor = iteration => iteration % 2 === 0 ? ['go', 'rust'] : ['rust', 'go'];
export function elapsed(proof) {
  assert.equal(proof?.schema, 'lycheedev.owned-process.v1');
  for (const key of ['rootReaped', 'treeReaped', 'handlesClosed', 'toolIdentityLocksClosed', 'outputReadersComplete']) assert.equal(proof[key], true, key);
  assert.equal(proof.activeProcesses, 0); assert.equal(proof.forced, false);
  assert(!proof.error && !proof.deadlineHit && !proof.outputLimit, 'failed process cannot yield accepted timing');
  const t = proof.timing;
  assert.equal(t?.schema, 'lycheedev.owned-timing.v1', 'actual CLI clock required');
  assert(Number.isSafeInteger(t.frequency) && t.frequency > 0);
  for (const key of ['startTicks', 'rootExitTicks', 'treeReapedTicks']) assert.match(t[key], /^[1-9]\d*$/);
  const start = BigInt(t.startTicks), root = BigInt(t.rootExitTicks), end = BigInt(t.treeReapedTicks);
  assert(end >= root && root >= start && end > start, 'ordered monotonic timing required');
  const millis = Number(end - start) * 1000 / t.frequency;
  assert(Number.isFinite(millis) && millis >= 1000 / t.frequency * 100, 'timing window under 100 clock ticks; enlarge equivalent work before acceptance');
  return {controllableMillis: millis, rootObservedMillis: Number(root - start) * 1000 / t.frequency, reapAndDrainMillis: Number(end - root) * 1000 / t.frequency, resolutionMillis: 1000 / t.frequency};
}

function fields(value, names, optional = []) {
  const result = {};
  for (const name of names) { assert(Object.hasOwn(value, name) || optional.includes(name), `missing public field ${name}`); if (Object.hasOwn(value, name)) result[name] = value[name]; }
  return result;
}
function normalizeDescribe(result, engine) {
  assert(Array.isArray(result.commands) && result.commands.length > 0);
  const value = structuredClone(result), changes = [];
  assert.equal(new Set(value.commands.map(c => c.path)).size, value.commands.length, 'duplicate commands');
  if (engine === 'rust') for (const row of value.commands) {
    const approved = APPROVED_ENGINE_SUMMARIES.find(v => v.path === row.path && v.actual === row.summary);
    if (approved) { changes.push(approved); row.summary = approved.expected; }
  }
  return {value, changes};
}
export function semantic(caseId, envelope, engine, oracle, pin) {
  const result = assertEnvelope({exit: 0, envelope});
  assert.equal(envelope.error, null, 'successful envelope cannot hide an error');
  if (caseId === 'B01-describe') return normalizeDescribe(result, engine);
  if (caseId === 'B02-retail-index') {
    assert.equal(result.repository, oracle.repository); assert.equal(result.product, oracle.product); assert.equal(result.commit, oracle.commit);
    assert.equal(result.documents, oracle.documents); assert.equal(result.skippedDocuments ?? 0, 0); assert.equal(result.complete, true);
    assert.equal(result.schema, 'lycheedev.source-map.v2'); assert.equal(result.storage, 'file-cache'); assert.equal(result.parser, 'source-v1');
    return {value: {...fields(result, ['schema', 'storage', 'parser', 'repository', 'product', 'commit', 'documents', 'declarations', 'relationships', 'assets', 'diagnostics', 'complete']), skippedDocuments: result.skippedDocuments ?? 0, diagnosticSample: result.diagnosticSample ?? []}, changes: []};
  }
  assert.equal(caseId, 'B02-retail-query');
  assertAnchor(result, oracle.anchors[0], oracle, pin);
  assert.equal(result.complete, true); assert.equal(result.truncated, false); assert.equal(result.coverage.complete, true);
  // Project the fixed Go 3.2.3 public result, never Rust's Emmy-only provenance,
  // checker counters or matchCoverage. Keep all rows, ordering, scores, IDs,
  // excerpts, original-byte hashes, diagnostics, cursors and completeness.
  const value = fields(result, ['sourceId', 'product', 'requestedRef', 'matchedTag', 'resolvedCommit', 'snapshotId', 'complete', 'truncated'], ['requestedRef']);
  value.results = result.results.map(row => fields(row, ['symbolId', 'kind', 'name', 'path', 'matchedBy', 'role', 'confidence', 'excerpt', 'contentHash', 'line', 'score', 'scoreParts'], ['symbolId', 'name', 'confidence', 'scoreParts']));
  value.relations = (result.relations ?? []).map(row => fields(row, ['source', 'target', 'kind', 'confidence', 'path', 'line'], ['source']));
  value.suggestions = result.suggestions ?? []; value.nextCursor = result.nextCursor ?? null;
  value.coverage = {...fields(result.coverage, ['analyzedDocuments', 'skippedDocuments', 'diagnostics', 'complete']), diagnosticSample: result.coverage.diagnosticSample ?? []};
  return {value, changes: []};
}
export function compareSamples(caseId, go, rust, oracle, pin) {
  const expected = semantic(caseId, go, 'go', oracle, pin), actual = semantic(caseId, rust, 'rust', oracle, pin);
  assert(isDeepStrictEqual(expected.value, actual.value), 'fixed Go public semantic fields differ; both complete raw outputs are retained');
  return {resultEquivalent: true, approvedEngineSummaryChanges: actual.changes};
}
export function statistics(values) {
  assert(values.length > 0 && values.every(v => Number.isFinite(v) && v > 0));
  const sorted = [...values].sort((a, b) => a - b), middle = Math.floor(sorted.length / 2);
  return {count: sorted.length, median: sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2, p95: sorted[Math.ceil(sorted.length * .95) - 1], min: sorted[0], max: sorted.at(-1)};
}
export function summarize(pairs) {
  return CASES.flatMap(({id}) => ['cold', 'warm'].map(cacheMode => {
    const rows = pairs.filter(row => row.caseId === id && row.cacheMode === cacheMode);
    const iterations = new Set(rows.map(r => r.iteration));
    const identities = new Set(rows.flatMap(row => [row.go?.home, row.rust?.home]));
    const valid = rows.length === REPETITIONS && iterations.size === REPETITIONS && [...iterations].every(n => Number.isInteger(n) && n >= 0 && n < REPETITIONS)
      && identities.size === REPETITIONS * 2 && !identities.has(undefined) && rows.every(row => {
        const cold = cacheMode === 'cold' ? row : pairs.find(p => p.caseId === id && p.iteration === row.iteration && p.cacheMode === 'cold');
        return row.resultEquivalent === true && cold?.resultEquivalent === true && row.go?.valid === true && row.rust?.valid === true
          && orderFor(row.iteration).join() === row.order?.join()
          && (cacheMode !== 'warm' || id === 'B01-describe' || ['go', 'rust'].every(engine => row[engine].warmReuse?.verified === true))
          && ['go', 'rust'].every(engine => row[engine].home === cold[engine]?.home && row[engine].binary === cold[engine]?.binary && row[engine].snapshot === cold[engine]?.snapshot);
      });
    const allMeasured = rows.length > 0 && rows.every(row => ['go', 'rust'].every(engine => Number.isFinite(row[engine]?.controllableMillis) && row[engine].controllableMillis > 0));
    const timings = allMeasured ? Object.fromEntries(['go', 'rust'].map(engine => [engine, statistics(rows.map(row => row[engine].controllableMillis))])) : null;
    const ratio = timings ? timings.rust.median / timings.go.median : null;
    return {caseId: id, cacheMode, requiredPairs: REPETITIONS, attemptedPairs: rows.length, correctAndComplete: valid, timings, medianRatio: ratio, targetRatio: TARGET_RATIO, state: !valid ? 'failed_correctness_or_missing_samples' : ratio <= TARGET_RATIO ? 'passed_selected_path' : 'failed_performance'};
  }));
}
