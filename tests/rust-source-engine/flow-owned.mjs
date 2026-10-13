// Run the original business requests through the native query seam.
import assert from 'node:assert/strict';
import {existsSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {hashFile} from '../../tools/rust-build.mjs';
import {flowExecutionContract} from './flow-execution-contract.mjs';
import {verifyFlowScenario} from './flow-independent.mjs';

const root = resolve('.'), [binaryArg, outputArg, selected] = process.argv.slice(2);
assert(binaryArg && outputArg, 'usage: flow-owned.mjs <probe.exe> <new-output> [case-id]');
const binary = resolve(binaryArg), out = resolve(outputArg), contract = flowExecutionContract(root);
assert(!existsSync(out)); mkdirSync(out, {recursive: true});
const binarySHA256 = hashFile(binary);
const all = [...contract.originalCases, ...contract.selectorCases, ...contract.additionalCases];
const requested = selected ? all.filter(row => row.id === selected) : all;
assert(requested.length > 0);
const results = [];
for (const spec of requested) {
  const directory = join(out, spec.id); mkdirSync(directory);
  const input = join(directory, 'request.json');
  writeFileSync(input, JSON.stringify({case: spec, materials: contract.materials, bindings: contract.bindings}));
  const started = performance.now();
  const execution = ownedSpawnSync(binary, ['--spec', input], {
    cwd: root, evidence: directory, timeout: 2000, maxBuffer: 4 << 20,
  });
  const ownerMilliseconds = performance.now() - started;
  writeFileSync(join(directory, 'stdout.jsonl'), execution.stdout);
  writeFileSync(join(directory, 'stderr.txt'), execution.stderr);
  let business, error, terminal;
  try {
    assert.equal(execution.status, 0, `exit ${execution.status}: ${execution.stderr.toString()}`);
    assert(!execution.error && execution.handlesReaped);
    const frames = execution.stdout.toString('utf8').trim().split(/\r?\n/).map(JSON.parse);
    assert.deepEqual(frames.map(row => row.event), ['build_identity', 'input_identity', 'scenario', 'end']);
    terminal = frames.at(-1);
    assert.equal(terminal.complete, true, terminal.error ?? 'transport incomplete');
    assert.equal(terminal.sourceExecution, false); assert.equal(terminal.configExecution, false);
    const scenario = frames.find(row => row.event === 'scenario');
    assert.equal(scenario.case, spec.id);
    if (spec.id === 'F36') {
      assert.equal(scenario.selection_complete, false);
      assert.equal(scenario.entry_resolution.state, 'selector_required');
      const candidates = scenario.entry_resolution.candidates;
      assert.equal(candidates.length, 2);
      assert.equal(new Set(candidates.map(row => row.id)).size, 2);
      const bytes = Buffer.from(spec.originals[0].text);
      const closures = candidates.map(row => bytes.subarray(row.closure.start, row.closure.end).toString('utf8'));
      assert(closures.some(text => text.includes('Use(Source())')));
      assert(closures.some(text => text.includes('return nil')));
      assert.equal(scenario.graph, null); assert.equal(scenario.flow, null);
      business = {case: spec.id, findings: 0, selectorRequired: true};
    } else business = verifyFlowScenario(spec, scenario.graph, scenario.flow);
  } catch (failure) { error = failure.message; }
  const lifecyclePassed = execution.handlesReaped && ownerMilliseconds <= (spec.budget.maxMillis ?? 2000);
  results.push({case: spec.id, businessPassed: !error, lifecyclePassed, business, error,
    ownerMilliseconds, workerMillisecondsThroughDrop: terminal?.elapsedMillisecondsThroughDrop,
    exit: execution.status, handlesReaped: execution.handlesReaped, proof: execution.proof});
  writeFileSync(join(out, 'results.json'), JSON.stringify(results, null, 2));
}
assert.equal(hashFile(binary), binarySHA256, 'probe changed during suite');
const businessPassed = results.every(row => row.businessPassed), lifecyclePassed = results.every(row => row.lifecyclePassed);
const report = {schema: 'lycheedev.source-flow-business.v1', binary, binarySHA256,
  originalCases: results.filter(row => /^F\d\d$/.test(row.case)).length,
  additionalCases: results.filter(row => !/^F\d\d$/.test(row.case)).length,
  businessPassed, lifecyclePassed, passed: businessPassed && lifecyclePassed, results,
  scope: 'Common-facts flow on original requests and concrete guard/join cases; no default Source or production API acceptance'};
writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2));
console.log(JSON.stringify({businessPassed, lifecyclePassed, cases: results.length,
  failures: results.filter(row => !row.businessPassed).map(row => ({case: row.case, error: row.error})), report: join(out, 'report.json')}));
if (!report.passed) process.exitCode = 1;
