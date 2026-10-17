// Formal Windows diagnostic wrapper; reuses the exact sealed product, never builds.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {beginScratch} from './rust-disk.mjs';
import {hashFile} from './rust-build.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {validateConditions, sourcePhaseEnvironment} from '../tests/rust-performance-paired/source-candidates.mjs';
import {inspectRetainedCold, authenticateRetainedCold, validateColdReference, assertDiskPolicy, validateRequest, verifyCandidateRecord, runnerIdentity, evidenceFiles, verifyObservedReport} from '../tests/rust-performance-paired/source-pack-scenarios.mjs';

export function runOwned(configuration, conditionsPath, resumePath) {
  assert.equal(process.platform, 'win32', 'pack diagnostics require Windows');
  for (const path of [configuration, conditionsPath, ...(resumePath ? [resumePath] : [])]) { const s = lstatSync(path); assert(s.isFile() && !s.isSymbolicLink() && s.size <= 16384, 'bounded configuration required'); }
  const request = validateRequest(JSON.parse(readFileSync(configuration))), conditions = validateConditions(JSON.parse(readFileSync(conditionsPath)));
  const runnerRoot = resolve(import.meta.dirname, '..'), inputs = runnerIdentity(runnerRoot), env = sourcePhaseEnvironment(process.env);
  const diskPolicy = assertDiskPolicy(request.ownerRoot);
  verifyCandidateRecord(request.candidate);
  const resumeCold = resumePath ? validateColdReference(JSON.parse(readFileSync(resumePath))) : undefined;
  if (resumeCold) authenticateRetainedCold(request, resumeCold);
  const job = beginScratch(request.ownerRoot, 'source-pack2-diagnostics', {command: process.argv, inputs: {configuration: hashFile(configuration), conditions: hashFile(conditionsPath), runner: inputs, ...(resumeCold ? {retainedCold: resumeCold} : {})}, admissionOptions: {requiredBytes: 16 << 20, scratchReserveBytes: 1 << 20}});
  let state = 'failed', handlesReaped = true;
  try {
    const configPath = join(job.evidence, 'configuration.json'); writeFileSync(configPath, JSON.stringify({request, conditions, runnerRoot, inputs, scratch: job.directory, evidence: job.evidence, ...(resumeCold ? {resumeCold} : {})}, null, 2) + '\n');
    handlesReaped = false;
    const run = ownedSpawnSync(process.execPath, [join(runnerRoot, 'tests/rust-performance-paired/source-pack-scenarios.mjs'), configPath], {cwd: request.ownerRoot, env, evidence: job.evidence, timeout: 3600000});
    handlesReaped = run.handlesReaped === true;
    const reportPath = join(job.evidence, 'report.json'), observed = existsSync(reportPath) ? JSON.parse(readFileSync(reportPath)) : null;
    let failure;
    try { assert.deepEqual(runnerIdentity(runnerRoot), inputs); verifyCandidateRecord(request.candidate); verifyObservedReport(run, observed, request, inputs, resumeCold); state = 'succeeded'; }
    catch (error) { failure = error.message; }
    const report = {diskPolicy, schema: 'lycheedev.source-pack-scenarios-owned.v1', state, performanceAcceptanceEvaluated: false, fullB01B07Acceptance: false, importedSamples: 0, observation: reportPath, request, inputs, failure, process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof}};
    writeFileSync(join(job.evidence, 'owned-report.json'), JSON.stringify(report, null, 2) + '\n');
    console.log(JSON.stringify({state, report: join(job.evidence, 'owned-report.json'), blocker: observed?.blocker ?? failure}));
    return state === 'succeeded';
  } finally { job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs: evidenceFiles(job.evidence)}); if (handlesReaped && state === 'succeeded') job.cleanup(); }
}
export function inspectCold(configuration, conditionsPath, destination) {
  assert.equal(process.platform, 'win32');
  for (const path of [configuration, conditionsPath]) { const s = lstatSync(path); assert(s.isFile() && !s.isSymbolicLink() && s.size <= 16384); }
  const request = validateRequest(JSON.parse(readFileSync(configuration))); validateConditions(JSON.parse(readFileSync(conditionsPath))); assertDiskPolicy(request.ownerRoot); verifyCandidateRecord(request.candidate);
  assert(!existsSync(destination), 'inspection never overwrites a reference'); const reference = inspectRetainedCold(request);
  writeFileSync(destination, JSON.stringify(reference, null, 2) + '\n', {flag: 'wx'});
  console.log(JSON.stringify({state: 'retained_cold_reference_written', path: resolve(destination), sha256: hashFile(destination), reference, productInvocations: 0})); return reference;
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const [configuration, conditions, mode, reference, ...extra] = process.argv.slice(2);
  assert(configuration && conditions && !extra.length && ((!mode && !reference) || (['--inspect-cold', '--resume-cold'].includes(mode) && reference)), 'usage: node tools/rust-source-pack-diagnostics.mjs <pack2-config.json> <machine-conditions.json> [--inspect-cold NEW-reference.json | --resume-cold reference.json]');
  if (mode === '--inspect-cold') inspectCold(configuration, conditions, reference);
  else if (!runOwned(configuration, conditions, reference)) process.exitCode = 1;
}
