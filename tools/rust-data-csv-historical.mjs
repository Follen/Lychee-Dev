// Explicit historical SQL consumer. Not current Agent qualification.
// Explicit optional old/new CSV diagnostic. No builds, downloads or changes to
// the ordinary paired Data entry. Run only in the existing Windows test queue.
import assert from 'node:assert/strict';
import {existsSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {beginScratch} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {hashFile, sourceDigest, verifyArtifact} from './rust-build.mjs';
import {bounded, parse} from '../tests/rust-performance-paired/data-inputs.mjs';
import {CSV_OUTER_TIMEOUT_MS, CSV_SIDES, assertCSVPolicy, csvEnvironment, csvRunnerIdentity, evidenceFiles, loadCSVPreparation, validateCSVConditions, validateCSVRequest, verifyCSVArtifactRecord, verifyCSVObserved} from '../tests/rust-performance-paired/historical-data-csv-diagnostics.mjs';

const [configuration, conditionsFile, ...extra] = process.argv.slice(2);
assert(configuration && conditionsFile && !extra.length, 'usage: node tools/rust-data-csv-diagnostics.mjs <old-new-config.json> <machine-conditions.json>');
assert.equal(process.platform, 'win32', 'CSV diagnostics require the owned Windows queue');
const request = validateCSVRequest(parse(bounded(configuration, 16384))), conditions = validateCSVConditions(parse(bounded(conditionsFile, 16384)));
const runnerRoot = resolve(import.meta.dirname, '..'), inputs = csvRunnerIdentity(runnerRoot), env = csvEnvironment(process.env);
function authenticateBytes() {
  assertCSVPolicy(request.ownerRoot);
  for (const side of CSV_SIDES) {
    const input = request[side]; assert.equal(hashFile(input.artifact, 65536), input.artifactSHA256);
    const artifact = verifyArtifact(input.artifactRoot, input.artifact); verifyCSVArtifactRecord(input, artifact);
    assert.equal(sourceDigest(input.sourceRoot), input.sourceTreeSHA256);
  }
  assert.deepEqual(csvRunnerIdentity(runnerRoot), inputs); return loadCSVPreparation(request.fixturePreparation).provenance;
}
const fixtureProducer = authenticateBytes();
const job = beginScratch(request.ownerRoot, 'paired-data-csv-diagnostics', {command: process.argv, inputs: {request: hashFile(configuration), conditions: hashFile(conditionsFile), runner: inputs,
  baseline: request.baseline.binarySHA256, candidate: request.candidate.binarySHA256, fixtureProducer}, admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 1 << 30}});
let state = 'failed', handlesReaped = true, report;
const started = performance.now();
try {
  const configPath = join(job.evidence, 'configuration.json');
  writeFileSync(configPath, JSON.stringify({request, conditions, runnerRoot, inputs, scratch: job.directory, evidence: job.evidence}, null, 2) + '\n');
  handlesReaped = false;
  const run = ownedSpawnSync(process.execPath, [join(runnerRoot, 'tests/rust-performance-paired/historical-data-csv-diagnostics.mjs'), configPath], {cwd: request.ownerRoot, env, evidence: job.evidence, timeout: CSV_OUTER_TIMEOUT_MS});
  handlesReaped = run.handlesReaped === true;
  const path = join(job.evidence, 'report.json'), observed = existsSync(path) ? parse(bounded(path, 16 << 20)) : null;
  let stable = false, failure;
  try {
    assert.deepEqual(authenticateBytes(), fixtureProducer); verifyCSVObserved(run, observed);
    assert.deepEqual(observed.request, request); assert.deepEqual(observed.inputs, inputs); stable = true;
  } catch (error) { failure = error.message; }
  report = {schema: 'lycheedev.data-csv-owned.v1', state: stable ? 'csv_candidates_observed' : 'csv_candidates_failed', performanceAcceptanceEvaluated: false, fullB04Acceptance: false,
    request, runnerRoot, inputs, fixtureProducer, observation: path, artifactAndRunnerStable: stable, failure, wallMillis: performance.now() - started,
    completion: 'This observation is not owner retirement. The original receipt must be retired/handlesReaped=true and the wrapper must exit zero before the run is complete.',
    owner: job.receipt, process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof}};
  writeFileSync(join(job.evidence, 'owned-report.json'), JSON.stringify(report, null, 2) + '\n');
  state = stable ? 'succeeded' : 'failed'; if (!stable) process.exitCode = 1;
} finally {
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs: evidenceFiles(job.evidence)});
  if (handlesReaped && state === 'succeeded') job.cleanup();
}
// In particular, a cleanup exception cannot print a successful completion.
console.log(JSON.stringify({state: report.state, report: join(job.evidence, 'owned-report.json'), owner: job.receipt, ownerRetired: state === 'succeeded'}, null, 2));
