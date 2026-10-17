// Formal Windows diagnostic wrapper; reuses the exact sealed product, never builds.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {beginScratch} from './rust-disk.mjs';
import {hashFile} from './rust-build.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {validateConditions, sourcePhaseEnvironment} from '../tests/rust-performance-paired/source-candidates.mjs';
import {assertDiskPolicy, validateRequest, verifyCandidateRecord, runnerIdentity, evidenceFiles, verifyObservedReport} from '../tests/rust-performance-paired/source-pack-scenarios.mjs';

export function runOwned(configuration, conditionsPath) {
  assert.equal(process.platform, 'win32', 'pack diagnostics require Windows');
  for (const path of [configuration, conditionsPath]) { const s = lstatSync(path); assert(s.isFile() && !s.isSymbolicLink() && s.size <= 16384, 'bounded configuration required'); }
  const request = validateRequest(JSON.parse(readFileSync(configuration))), conditions = validateConditions(JSON.parse(readFileSync(conditionsPath)));
  const runnerRoot = resolve(import.meta.dirname, '..'), inputs = runnerIdentity(runnerRoot), env = sourcePhaseEnvironment(process.env);
  const diskPolicy = assertDiskPolicy(request.ownerRoot);
  verifyCandidateRecord(request.candidate);
  const job = beginScratch(request.ownerRoot, 'source-pack2-diagnostics', {command: process.argv, inputs: {configuration: hashFile(configuration), conditions: hashFile(conditionsPath), runner: inputs}, admissionOptions: {requiredBytes: 16 << 20, scratchReserveBytes: 1 << 20}});
  let state = 'failed', handlesReaped = true;
  try {
    const configPath = join(job.evidence, 'configuration.json'); writeFileSync(configPath, JSON.stringify({request, conditions, runnerRoot, inputs, scratch: job.directory, evidence: job.evidence}, null, 2) + '\n');
    handlesReaped = false;
    const run = ownedSpawnSync(process.execPath, [join(runnerRoot, 'tests/rust-performance-paired/source-pack-scenarios.mjs'), configPath], {cwd: request.ownerRoot, env, evidence: job.evidence, timeout: 3600000});
    handlesReaped = run.handlesReaped === true;
    const reportPath = join(job.evidence, 'report.json'), observed = existsSync(reportPath) ? JSON.parse(readFileSync(reportPath)) : null;
    let failure;
    try { assert.deepEqual(runnerIdentity(runnerRoot), inputs); verifyCandidateRecord(request.candidate); verifyObservedReport(run, observed, request, inputs); state = 'succeeded'; }
    catch (error) { failure = error.message; }
    const report = {diskPolicy, schema: 'lycheedev.source-pack-scenarios-owned.v1', state, performanceAcceptanceEvaluated: false, fullB01B07Acceptance: false, importedSamples: 0, observation: reportPath, request, inputs, failure, process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof}};
    writeFileSync(join(job.evidence, 'owned-report.json'), JSON.stringify(report, null, 2) + '\n');
    console.log(JSON.stringify({state, report: join(job.evidence, 'owned-report.json'), blocker: observed?.blocker ?? failure}));
    return state === 'succeeded';
  } finally { job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs: evidenceFiles(job.evidence)}); if (handlesReaped && state === 'succeeded') job.cleanup(); }
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const [configuration, conditions, ...extra] = process.argv.slice(2); assert(configuration && conditions && !extra.length, 'usage: node tools/rust-source-pack-diagnostics.mjs <pack2-config.json> <machine-conditions.json>');
  if (!runOwned(configuration, conditions)) process.exitCode = 1;
}
