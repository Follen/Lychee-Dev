// Contract output checks only. No process launch, source rewriting or result fallback.
import assert from 'node:assert/strict';
export const lines = output => output.toString('utf8').split(/\r?\n/);
export function listedTests(output, expected) {
  const actual = lines(output).filter(line => line.endsWith(': test')).map(line => line.slice(0, -6));
  assert.deepEqual(actual, expected, 'contract.exact_test_inventory');
  return actual;
}
export function suiteResult(output, names) {
  const rows = lines(output), summaries = rows.filter(line => /^test result: ok\. /.test(line));
  assert.equal(summaries.length, 1, 'contract.one_test_summary');
  const match = /^test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;/.exec(summaries[0]);
  assert(match && Number(match[1]) === names.length && names.length > 0, 'contract.exact_test_count');
  // --nocapture can insert JSON trace lines after a test prefix. The libtest
  // summary and previously exact --list provide the full suite denominator.
  return {passed: names.length, names};
}
export function traceRows(output, prefix) {
  return lines(output).filter(line => line.startsWith(prefix)).map(line => JSON.parse(line.slice(prefix.length)));
}
export function completeVariant(suites, parity, outcomes, checkpoints, expectedTests) {
  assert.deepEqual(Object.keys(suites), Object.keys(expectedTests), 'contract.suites');
  for (const [name, names] of Object.entries(expectedTests)) assert.deepEqual(suites[name], {passed: names.length, names}, 'contract.suite_names');
  assert.equal(Object.values(suites).reduce((n, suite) => n + suite.passed, 0), 31, 'contract.31_tests');
  assert.equal(parity.length, 450, 'contract.450_traces');
  assert.equal(parity.filter(row => row.fault.axis === '').length, 28, 'contract.success_cases');
  assert.equal(new Set(parity.map(row => JSON.stringify([row.case, row.fault]))).size, parity.length, 'contract.unique_cases');
  for (const name of ['callback-checkpoints', 'unavailable-checkpoints']) {
    const rows = parity.filter(row => row.case === name), success = rows.find(row => row.fault.axis === '');
    assert(success && Number.isSafeInteger(success.checks) && Number.isSafeInteger(success.steps) && success.checks > 3 && success.steps > 3, 'contract.checkpoint_counts');
    assert.equal(rows.length, 2 * (success.checks + success.steps) + 2, 'contract.fault_denominator');
    for (const axis of ['check', 'step']) for (let ordinal = 1; ordinal <= success[axis === 'check' ? 'checks' : 'steps']; ordinal++) for (const persistent of [false, true]) {
      assert.equal(rows.filter(row => row.fault.axis === axis && row.fault.ordinal === ordinal && row.fault.persistent === persistent).length, 1, 'contract.missing_fault');
    }
  }
  const fixtures = ['empty', 'selected-string', 'unicode-crlf', 'chunk-boundary', 'malformed', 'malformed-after-chunk'];
  assert.deepEqual(outcomes.map(row => row.case), fixtures, 'contract.6_constructor_outcomes');
  assert.deepEqual(checkpoints.map(row => row.case), fixtures, 'contract.6_constructor_checkpoint_sets');
  assert(checkpoints.every(row => Number.isSafeInteger(row.checkpoints) && row.checkpoints > 1 && row.singleShotAndPersistent === true), 'contract.constructor_fault_exhaustion');
  return {suites, parity, outcomes, checkpoints};
}
export function compareVariants(results) {
  assert.deepEqual(Object.keys(results), ['A', 'B', 'C'], 'contract.three_real_variants');
  for (const side of ['B', 'C']) for (const field of ['suites', 'parity', 'outcomes']) assert.deepEqual(results.A[field], results[side][field], `contract.${side}.${field}`);
  // Checkpoint count is constructor-specific. Exhaustion is required separately.
}
