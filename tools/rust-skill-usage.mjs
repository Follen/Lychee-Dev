// Real Windows packaged-Skill workflow acceptance. No builds, game or publish.
import {existsSync, lstatSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {verifyArtifact, hashFile} from './rust-build.mjs';
import {beginScratch, contained} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {inventory, inventoryHash, jsonFile, verifyCandidate} from '../tests/rust-skill-usage/assertions.mjs';

const root = resolve('.');
const [artifactArg, candidateArg, ...extra] = process.argv.slice(2);
if (!artifactArg || !candidateArg || extra.length) throw new Error('usage: node tools/rust-skill-usage.mjs <artifact.json> <candidate-report.json>');
if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('skill-usage.windows_amd64_required');
const artifactPath = contained(root, resolve(artifactArg)), candidatePath = contained(root, resolve(candidateArg));
const artifact = verifyArtifact(root, artifactPath), artifactSHA256 = hashFile(artifactPath);
const workflow = jsonFile(join(root, 'tests/rust-skill-usage/workflow.json'));
const candidate = verifyCandidate(root, candidatePath, artifact, workflow);
if (candidate.report.artifactRecordSHA256 !== artifactSHA256) throw new Error('skill-usage.sealed_artifact_record_mismatch');
const inputs = ['tools/rust-skill-usage.mjs', 'tests/rust-skill-usage/run.mjs', 'tests/rust-skill-usage/assertions.mjs', 'tests/rust-skill-usage/workflow.json', 'tests/rust-source-full-corpus/assertions.mjs', 'tests/rust-source-full-corpus/retail-oracle.json'];
const testInputs = Object.fromEntries(inputs.map(path => [path, hashFile(join(root, path))]));
const job = beginScratch(root, 'skill-real-usage', {
  command: process.argv,
  inputs: {binarySHA256: artifact.binarySHA256, artifactSHA256, candidateReportSHA256: candidate.reportSHA256, packageSHA256: candidate.packageSHA256, testInputs},
  admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 1 << 30},
});
let state = 'failed', handlesReaped = true;
const logs = [];
const save = (name, value) => writeFileSync(join(job.evidence, name), JSON.stringify(value, null, 2) + '\n');
try {
  save('candidate-identity.json', {candidateReport: candidate.report, candidateReportSHA256: candidate.reportSHA256, artifact, artifactRecordSHA256: artifactSHA256, packageSHA256: candidate.packageSHA256, files: candidate.files, testInputs});
  // The outer Job owns controller, Skill and package launchers, every CLI, Git,
  // native worker/session descendant. No per-command orphan exemptions.
  handlesReaped = false;
  const run = ownedSpawnSync(process.execPath, [join(root, 'tests/rust-skill-usage/run.mjs'), candidate.packageRoot, job.directory, job.evidence, candidate.packageSHA256], {
    cwd: root, env: {...process.env}, evidence: job.evidence, timeout: 1200000,
  });
  handlesReaped = run.handlesReaped === true; logs.push(...run.logs);
  const observationPath = join(job.evidence, 'report.json');
  const observation = existsSync(observationPath) ? jsonFile(observationPath) : null;
  const after = verifyArtifact(root, artifactPath);
  const stable = after.binarySHA256 === artifact.binarySHA256 && hashFile(artifactPath) === artifactSHA256 && hashFile(candidatePath) === candidate.reportSHA256
    && inventoryHash(inventory(candidate.packageRoot)) === candidate.packageSHA256
    && Object.entries(testInputs).every(([path, expected]) => hashFile(join(root, path)) === expected);
  const passed = stable && handlesReaped && !run.error && run.status === 0 && observation?.state === 'passed_selected_skill_workflows';
  save('owned-report.json', {schema: 'lycheedev.skill-usage-owned.v1', passed, binarySHA256: artifact.binarySHA256, artifactRecordSHA256: artifactSHA256,
    candidateReportSHA256: candidate.reportSHA256, packageSHA256: candidate.packageSHA256, inputsStable: stable, testInputs,
    process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof}, observation: observationPath,
    scope: 'Independent real packaged Skill/CLI use against exact Retail source, selected static and ordinary semantic research and bounded recovery. Synthetic prefix layout is not npm install acceptance. Full source authenticity/corpus, lifecycle, full Go differential, performance and game acceptance remain separate.'});
  console.log(JSON.stringify({passed, report: join(job.evidence, 'owned-report.json'), checks: observation?.checks?.map(({caseId, state, reason}) => ({caseId, state, reason}))}, null, 2));
  state = passed ? 'succeeded' : 'failed';
  if (!passed) process.exitCode = 1;
} catch (error) {
  save('controller-failure.json', {error: error.message, handlesReaped});
  throw error;
} finally {
  for (const name of readdirSync(job.evidence)) {
    const path = join(job.evidence, name);
    if (name !== 'receipt.json' && lstatSync(path).isFile() && !logs.includes(path)) logs.push(path);
  }
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
  if (handlesReaped) job.cleanup();
}
