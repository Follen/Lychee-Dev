// Windows public-CLI acceptance against an independently authenticated full
// Retail corpus. No build, game interaction, source execution or release action.
import {existsSync, lstatSync, readdirSync, readFileSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {verifyArtifact, hashFile} from './rust-build.mjs';
import {beginScratch} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';

const root = resolve('.');
const [artifactPath, ...extra] = process.argv.slice(2);
if (!artifactPath || extra.length) throw new Error('usage: node tools/rust-source-full-corpus.mjs <artifact.json>');
if (process.platform !== 'win32') throw new Error('source-full-corpus.windows_required');
const artifact = verifyArtifact(root, artifactPath);
const runner = join(root, 'tests/rust-source-full-corpus/run.mjs');
const fixture = join(root, 'tests/rust-source-full-corpus/retail-oracle.json');
const testInputs = Object.fromEntries(['tools/rust-source-full-corpus.mjs', 'tests/rust-source-full-corpus/run.mjs', 'tests/rust-source-full-corpus/assertions.mjs', 'tests/rust-source-full-corpus/retail-oracle.json'].map(path => [path, hashFile(join(root, path))]));
const job = beginScratch(root, 'source-full-corpus', {
  command: process.argv,
  inputs: {binarySHA256: artifact.binarySHA256, oracleSHA256: hashFile(fixture), runnerSHA256: hashFile(runner), testInputs},
  admissionOptions: {requiredBytes: 512 << 20, scratchReserveBytes: 512 << 20},
});
let state = 'failed', handlesReaped = true;
const logs = [];
try {
  // One outer Job owns the controller and every CLI/Git/native worker child.
  // Keeping the Job across invocations also permits later session-reuse checks.
  handlesReaped = false;
  const run = ownedSpawnSync(process.execPath, [runner, artifact.sourceBinary, join(job.directory, 'home'), job.evidence], {
    cwd: root, env: {...process.env}, evidence: job.evidence, timeout: 900000,
  });
  handlesReaped = run.handlesReaped === true; logs.push(...run.logs);
  const observationPath = join(job.evidence, 'report.json');
  const observation = existsSync(observationPath) ? JSON.parse(readFileSync(observationPath, 'utf8')) : null;
  const after = verifyArtifact(root, artifactPath);
  const stable = after.binarySHA256 === artifact.binarySHA256 && Object.entries(testInputs).every(([path, expected]) => hashFile(join(root, path)) === expected);
  const passed = stable && handlesReaped && !run.error && run.status === 0 && observation?.state === 'passed_selected_static_cases';
  const report = {
    schema: 'lycheedev.source-full-corpus-owned.v1', passed, binarySHA256: artifact.binarySHA256,
    profile: artifact.profile, buildInputs: artifact.inputHashes, artifactStable: stable,
    process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof},
    observation: observationPath, scope: 'Full fixed Retail import/index and selected public static CLI quality cases. Semantic/session/Go differential/performance acceptance remain separate.',
  };
  writeFileSync(join(job.evidence, 'owned-report.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({passed, report: join(job.evidence, 'owned-report.json'), checks: observation?.checks?.map(({caseId, state, reason}) => ({caseId, state, reason}))}, null, 2));
  state = passed ? 'succeeded' : 'failed';
  if (!passed) process.exitCode = 1;
} finally {
  // Even a controller/artifact-read exception retains all completed evidence.
  for (const name of readdirSync(job.evidence)) {
    const path = join(job.evidence, name);
    if (name !== 'receipt.json' && lstatSync(path).isFile() && !logs.includes(path)) logs.push(path);
  }
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
  if (handlesReaped) job.cleanup();
}
