// Execute only read-only admission commands from an exact sealed build artifact.
// This is availability evidence, never Source, Skill, mailbox or release parity.
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { verifyArtifact } from './rust-build.mjs';
import { beginScratch } from './rust-disk.mjs';
import { ownedSpawnSync } from './rust-owned-process.mjs';
const root = resolve(process.cwd());
const [recordPath, ...extra] = process.argv.slice(2);
if (!recordPath || extra.length) throw new Error('usage: node tools/rust-cli-smoke.mjs <artifact.json>');
const artifact = verifyArtifact(root, recordPath);
const job = beginScratch(root, 'ci-cli-smoke', {command: process.argv, inputs: {binarySHA256: artifact.binarySHA256}});
let state = 'failed', handlesReaped = true;
const logs = [], observations = [];
try {
  for (const args of [['version', '--format=json'], ['--help', '--format=json'], ['describe', '--format=json']]) {
    const index = observations.length;
    handlesReaped = false;
    const result = ownedSpawnSync(artifact.sourceBinary, args, {
      cwd: root, env: {...process.env}, evidence: job.evidence, timeout: 120000,
    });
    handlesReaped = result.handlesReaped === true;
    logs.push(...result.logs);
    const stdout = join(job.evidence, `cli-${index}.stdout.json`);
    const stderr = join(job.evidence, `cli-${index}.stderr.bin`);
    writeFileSync(stdout, result.stdout); writeFileSync(stderr, result.stderr);
    logs.push(stdout, stderr);
    if (!handlesReaped || result.error || result.status !== 0) throw new Error(`cli-smoke.execution:${result.error?.message ?? result.status}`);
    const envelope = JSON.parse(readFileSync(stdout, 'utf8'));
    if (envelope.ok !== true || !envelope.result) throw new Error('cli-smoke.invalid_envelope');
    if (args[0] === 'version' && typeof envelope.result.version !== 'string') throw new Error('cli-smoke.version');
    if (args[0] !== 'version' && !Array.isArray(envelope.result.commands)) throw new Error('cli-smoke.commands');
    observations.push({args, exit: result.status, stdout, stderr});
  }
  const after = verifyArtifact(root, recordPath);
  if (after.binarySHA256 !== artifact.binarySHA256) throw new Error('cli-smoke.artifact_changed');
  const report = join(job.evidence, 'cli-smoke.json');
  writeFileSync(report, JSON.stringify({schema: 'lycheedev.cli-admission-smoke.v1', binarySHA256: artifact.binarySHA256, observations, scope: 'read-only version/help/describe; no business parity claim'}, null, 2)+'\n');
  logs.push(report); console.log(readFileSync(report, 'utf8')); state = 'succeeded';
} finally {
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
  if (handlesReaped) job.cleanup();
}
