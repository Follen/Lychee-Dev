// Opt-in fixed three-pair Windows diagnostics using two existing sealed EXEs.
// No product build, installation, publication or change to 20-pair acceptance.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {beginScratch} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {hashFile, sourceDigest, verifyArtifact} from './rust-build.mjs';
import {PAIRS, OUTER_TIMEOUT_MS, evidenceFiles, runnerIdentity, sourcePhaseEnvironment, validateConditions, validateRequest, verifyCandidateRecord, verifyObservedReport} from '../tests/rust-performance-paired/source-candidates.mjs';

const [configuration, conditionsPath, ...extra] = process.argv.slice(2);
assert(configuration && conditionsPath && !extra.length, 'usage: node tools/rust-source-candidate-diagnostics.mjs <candidate-config.json> <machine-conditions.json>');
assert.equal(process.platform, 'win32', 'source-candidates.windows_required');
for (const path of [configuration, conditionsPath]) assert(lstatSync(path).isFile() && lstatSync(path).size <= 16384, 'bounded configuration file required');
const request = validateRequest(JSON.parse(readFileSync(configuration)));
const conditions = validateConditions(JSON.parse(readFileSync(conditionsPath)));
const runnerRoot = resolve(import.meta.dirname, '..'), inputs = runnerIdentity(runnerRoot);
const env = sourcePhaseEnvironment(process.env);
// Fast pre-admission byte authentication; source/commit/Emmy identity is fully
// verified by the controller inside the outer Job before any product call.
for (const side of ['baseline', 'candidate']) {
  const candidate = request[side];
  assert.equal(hashFile(candidate.artifact), candidate.artifactSHA256);
  const artifact = verifyArtifact(candidate.artifactRoot, candidate.artifact);
  verifyCandidateRecord(candidate, artifact);
  assert.equal(sourceDigest(candidate.sourceRoot), artifact.inputHashes.sourceTree);
}
const job = beginScratch(request.ownerRoot, 'source-candidate-diagnostics', {command: process.argv, inputs: {request: hashFile(configuration), conditions: hashFile(conditionsPath), runner: inputs, baseline: request.baseline.binarySHA256, candidate: request.candidate.binarySHA256}, admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 1 << 30}});
let state = 'failed', handlesReaped = true;
const started = performance.now();
try {
  const configPath = join(job.evidence, 'configuration.json');
  writeFileSync(configPath, JSON.stringify({request, conditions, runnerRoot, inputs, scratch: job.directory, evidence: job.evidence}, null, 2) + '\n');
  handlesReaped = false;
  const run = ownedSpawnSync(process.execPath, [join(runnerRoot, 'tests/rust-performance-paired/source-candidates.mjs'), configPath], {cwd: request.ownerRoot, env, evidence: job.evidence, timeout: OUTER_TIMEOUT_MS});
  handlesReaped = run.handlesReaped === true;
  const reportPath = join(job.evidence, 'report.json');
  const observed = existsSync(reportPath) ? JSON.parse(readFileSync(reportPath)) : null;
  let stable = false, integrityFailure;
  try {
    assert.deepEqual(runnerIdentity(runnerRoot), inputs);
    for (const side of ['baseline', 'candidate']) {
      const candidate = request[side], artifact = verifyArtifact(candidate.artifactRoot, candidate.artifact);
      assert.equal(hashFile(candidate.artifact), candidate.artifactSHA256); verifyCandidateRecord(candidate, artifact);
      assert.equal(sourceDigest(candidate.sourceRoot), artifact.inputHashes.sourceTree);
    }
    verifyObservedReport(run, observed);
    assert.deepEqual(observed.request, request); assert.deepEqual(observed.inputs, inputs);
    stable = true;
  } catch (error) { integrityFailure = error.message; }
  const passed = stable;
  const report = {schema: 'lycheedev.source-candidate-diagnostics-owned.v1', state: passed ? 'source_candidates_observed' : 'source_candidates_failed', performanceAcceptanceEvaluated: false, fullB01B07Acceptance: false, requiredPairs: PAIRS, request, runnerRoot, inputs, observation: reportPath, artifactAndRunnerStable: stable, integrityFailure, wallMillis: performance.now() - started, process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof}};
  writeFileSync(join(job.evidence, 'owned-report.json'), JSON.stringify(report, null, 2) + '\n');
  state = passed ? 'succeeded' : 'failed';
  console.log(JSON.stringify({state: report.state, report: join(job.evidence, 'owned-report.json'), observation: observed?.observation}, null, 2));
  if (!passed) process.exitCode = 1;
} finally {
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs: evidenceFiles(job.evidence)});
  if (handlesReaped && state === 'succeeded') job.cleanup();
}
