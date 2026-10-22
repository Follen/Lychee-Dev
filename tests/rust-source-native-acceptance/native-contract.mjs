// Current native-research evidence checks shared by the Source native
// acceptance checker. Moved here from the retired rust-skill-usage migration
// reconciliation module (CLEAN-03/04); no historical workflow or fixed Go Skill
// baseline is consulted. The contract checked is the live native evidence shape.
import assert from 'node:assert/strict';

export function assertBoundedCoverage(result) {
  assert.equal(typeof result.complete, 'boolean', 'explicit completion');
  assert.equal(typeof result.truncated, 'boolean', 'explicit truncation');
  assert(result.coverage && typeof result.coverage === 'object', 'explicit coverage');
  if (result.truncated || result.coverage.complete === false || result.coverage.semantic === 'unavailable') assert.equal(result.complete, false, 'limits cannot imply complete semantics');
}

function assertNativeExecution(result, oracle) {
  assertBoundedCoverage(result);
  const evidence = result.nativeResearch;
  assert.equal(evidence?.schema, 'lycheedev.source-native-research.v1', 'ordinary semantic route must return actual native evidence');
  assert.equal(evidence.entryEvidence?.sourceExecution, false, 'studied source was not executed');
  assert.equal(evidence.lifecycle?.owner, 'source_owned_process', 'owned native worker');
  assert.equal(evidence.lifecycle?.mode, 'one_shot', 'ordinary call has one-shot lifecycle');
  assert.equal(evidence.lifecycle?.retired, true, 'one-shot worker actually retired');
  assert.equal(result.semanticRuntime?.engine, 'native-common-source', 'selected native semantic engine');
  assert.equal(typeof result.semanticRuntime.rulesVersion, 'string'); assert(result.semanticRuntime.rulesVersion.length > 0, 'native rules identity');
  assert.equal(evidence.analysis?.engine, 'native_common_facts', 'actual native fact projection');
  assert(['complete', 'partial'].includes(evidence.analysis.state), 'explicit native analysis state');
  assert.equal(evidence.analysis.sourceExecution, false, 'native analysis did not execute studied source');
  assert.match(evidence.inputIdentity, /^[a-f0-9]{64}$/, 'authenticated native input identity');
  assert.equal(evidence.analysis.inputIdentity, evidence.inputIdentity, 'native input identity is consistent');
  assert(Number.isInteger(evidence.analysis.sourceFiles) && evidence.analysis.sourceFiles >= oracle.counts['.lua'], 'native request includes fixed Retail Lua inputs; this does not claim complete semantic resolution');
  for (const field of ['engine', 'state', 'inputIdentity', 'sourceFiles', 'sourceExecution']) assert.deepEqual(result.semanticAnalysis?.[field], evidence.analysis[field], `outer/native analysis ${field}`);
  assert.equal(result.coverage.semantic, evidence.analysis.state, 'coverage preserves actual native state');
  if (evidence.analysis.state === 'partial') assert.equal(result.complete, false, 'partial native facts stay partial');
  assert.equal(evidence.relations, undefined, 'native evidence does not bypass public pagination');
  return evidence;
}
export function assertNativeCaller(row, callsite) {
  assert.equal(row.source, callsite.source, 'native call source is its authenticated caller');
  assert.equal(row.referenceExpression, callsite.referenceExpression, 'original reference expression retained');
  assert.equal(row.caller?.kind, callsite.caller.kind, 'explicit caller kind');
  assert.equal(row.caller?.name, callsite.caller.name, 'authenticated callable name');
  assert.equal(row.caller?.position?.path, callsite.path, 'caller original source path');
  assert.equal(row.caller?.position?.coveringRange?.start?.line, callsite.caller.line - 1, 'caller original declaration line');
  assert.equal(row.caller?.syntaxClosure?.file, callsite.site.file, 'caller admitted syntax file');
  assert(row.caller?.syntaxClosure && row.caller.syntaxClosure.start < row.site.start && row.caller.syntaxClosure.end >= row.site.end, 'nearest callable syntax contains callsite');
}
export function assertNativeEvidence(result, oracle, callsite) {
  const evidence = assertNativeExecution(result, oracle);
  // Production intentionally removes nativeResearch.relations and projects
  // those rows into the bounded public refs page. Check the native-origin row
  // there, never an independently present static-extraction row at that site.
  assert(Array.isArray(result.relations), 'public native relation page');
  const rows = result.relations.filter(row => ['path', 'line', 'kind', 'target'].every(key => row[key] === callsite[key]) && row.basis?.includes('native_common_source_facts'));
  assert(rows.length > 0, 'exact original callsite must have native-origin evidence');
  for (const row of rows) {
    assertNativeCaller(row, callsite);
    assert.deepEqual(row.site, callsite.site, 'native callsite exact original byte range');
    assert(['resolved', 'static_candidate'].includes(row.state), 'native relation strength remains explicit');
    assert.equal(typeof row.proof, 'string'); assert(row.proof.length > 0, 'native reference proof identity');
    assert(['known', 'known_absent', 'unavailable', 'not_applicable', 'selector_required'].includes(row.lexical?.state), 'native lexical result retained');
    assert(Array.isArray(row.observations), 'native observations retained, even when empty/partial');
    for (const observation of row.observations) {
      assert.equal(typeof observation.proof, 'string'); assert(observation.proof.length > 0, 'native observation proof identity');
      assert.equal(typeof observation.occurrence?.kind, 'string', 'native observation occurrence');
      assert(['known', 'known_absent', 'unavailable', 'not_applicable', 'selector_required'].includes(observation.selected?.state), 'native selected-value result');
    }
    if (row.state === 'resolved') assert(row.observations.length > 0 && row.observations.every(observation => observation.selected.state === 'known'), 'resolved call requires selected-value observations');
  }
  return evidence;
}
