import test from 'node:test';
import assert from 'node:assert/strict';
import {assessEvidence, benchmarkCases, oracleTests, oracleCommit, packageName} from '../tests/data-regression/evidence-policy.mjs';

const metricUnits = ['ns/op', 'B/op', 'allocs/op', 'network-B/op', 'requests/op', 'charged-work/op',
  'charged-retained-B/op', 'charged-metadata-B/op', 'prepare-ms/op', 'definitions-ms/op', 'execute-ms/op',
  'source-B/op', 'decoded-hits/op', 'decoded-reused-B/op', 'content-hashes/op', 'content-hash-B/op',
  'content-decodes/op', 'extracted-B/op', 'verified-blob-reads/op', 'verified-blob-B/op', 'fragment-loads/op', 'block-hits/op'];
const packagePass = {Package: packageName, Action: 'pass'};
const jsonl = events => events.map(event => JSON.stringify(event)).join('\n') + '\n';
function benchmarks(names = benchmarkCases, units = metricUnits) {
  return names.map(name => ({Package: packageName, Action: 'output', Test: name,
    Output: `1\t${units.map(unit => `1 ${unit}`).join('\t')}\n`}));
}
const benchmark = (events, extras = {}) => assessEvidence({mode: 'benchmark', exitCode: 0, text: jsonl(events), ...extras});
const oracle = (events, extras = {}) => assessEvidence({mode: 'fixture-oracle', exitCode: 0, text: jsonl(events), ...extras});

test('benchmark evidence requires all 20 exact cases, metrics and package pass', () => {
  assert.equal(benchmark([...benchmarks(), packagePass]).state, 'verified');
  for (const events of [[], [packagePass], [...benchmarks()], [...benchmarks(benchmarkCases.slice(1)), packagePass],
    [...benchmarks(benchmarkCases.map(name => name.replace('single', 'renamed'))), packagePass],
    [...benchmarks(benchmarkCases, metricUnits.filter(unit => unit !== 'charged-retained-B/op')), packagePass]]) {
    assert.equal(benchmark(events).state, 'failed');
  }
});
test('benchmark repetitions, incomplete metrics, skips and failures cannot pass accidentally', () => {
  const complete = [...benchmarks(), packagePass];
  assert.equal(benchmark(complete, {count: 2}).state, 'failed');
  assert.equal(benchmark([...benchmarks(), ...benchmarks(), packagePass], {count: 2}).state, 'verified');
  assert.equal(benchmark([...benchmarks(), ...benchmarks(), packagePass]).state, 'failed');
  assert.equal(benchmark([...complete, {Package: packageName, Action: 'skip', Test: benchmarkCases[0]}]).state, 'not_run');
  assert.equal(benchmark([...complete, {Package: packageName, Action: 'fail', Test: benchmarkCases[0]}]).state, 'failed');
  assert.equal(benchmark(complete, {exitCode: 1}).state, 'failed');
  const broken = benchmarks(); broken[0].Output = broken[0].Output.replace('1 ns/op', 'NaN ns/op');
  assert.equal(benchmark([...broken, packagePass]).state, 'failed');
});
test('fixture evidence requires the parent and every known child pass; skip stays not_run', () => {
  const passes = oracleTests.map(Test => ({Package: packageName, Action: 'pass', Test}));
  assert.equal(oracle([...passes, packagePass]).state, 'verified');
  for (const events of [[], [packagePass], [...passes], [...passes.slice(1), packagePass], [...passes.slice(0, -1), packagePass]]) {
    assert.equal(oracle(events).state, 'failed');
  }
  assert.equal(oracle([...passes, packagePass, {Package: packageName, Action: 'skip', Test: oracleTests[0]}]).state, 'not_run');
  assert.equal(oracle([...passes, packagePass], {text: 'not JSON'}).state, 'failed');
  assert.equal(oracle([...passes, packagePass], {text: 'null\n'}).state, 'failed');
  assert.equal(oracle([...passes, packagePass], {text: JSON.stringify({...packagePass, Output: 1})}).state, 'failed');
});
test('storage requires a complete verified fixed-oracle comparison artifact', () => {
  const comparison = {schema: 'lycheedev.test.casc-comparison.v1', state: 'verified', scope: 'pinned-storage-file',
    commit: oracleCommit, complete: true, sha256: 'a'.repeat(64), contentKey: 'b'.repeat(32), bytes: 264,
    fileDataID: 123, localeMask: 0x40, contentVariant: 'standard', pin: {FullBuild: '12.1.0.69875'}, oracle: {mode: 'storage'}};
  const assess = (artifact, exitCode = 0) => assessEvidence({mode: 'storage-oracle', text: '', comparison: artifact, exitCode});
  assert.equal(assess(comparison).state, 'verified');
  for (const artifact of [undefined, {}, {...comparison, state: 'not_run'}, {...comparison, complete: false},
    {...comparison, sha256: ''}, {...comparison, commit: 'floating-master'}, {...comparison, bytes: -1}]) {
    assert.equal(assess(artifact).state, 'failed');
  }
  assert.equal(assess(comparison, 1).state, 'failed');
});
