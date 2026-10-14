// Execute admission commands or an isolated scratch-home doctor from a sealed artifact.
// This is availability evidence, never Source, Skill, mailbox or release parity.
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { verifyArtifact } from './rust-build.mjs';
import { beginScratch } from './rust-disk.mjs';
import { ownedSpawnSync } from './rust-owned-process.mjs';
const root = resolve(process.cwd());
const [recordPath, ...extra] = process.argv.slice(2);
const doctor = extra.length === 1 && extra[0] === '--doctor';
if (!recordPath || (extra.length && !doctor)) throw new Error('usage: node tools/rust-cli-smoke.mjs <artifact.json> [--doctor]');
const artifact = verifyArtifact(root, recordPath);
const job = beginScratch(root, 'ci-cli-smoke', {command: process.argv, inputs: {binarySHA256: artifact.binarySHA256}});
let state = 'failed', handlesReaped = true;
const logs = [], observations = [];
try {
  const commands = [['version', '--format=json'], ['--help', '--format=json'], ['describe', '--format=json']];
  if (doctor) commands.splice(0, commands.length,
    ['init', '--home', join(job.directory, 'home'), '--format=json'],
    ['doctor', '--home', join(job.directory, 'home'), '--offline', '--format=json']);
  for (const args of commands) {
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
    if (['--help', 'describe'].includes(args[0]) && !Array.isArray(envelope.result.commands)) throw new Error('cli-smoke.commands');
    if (args[0] === 'doctor') {
      const check = envelope.result.checks?.find(check => check.id === 'emmy.runtime');
      if (check?.ok !== true || check.scope !== 'embedded-worker-fixture' || check.lifecycle?.retired !== true) throw new Error('cli-smoke.emmy_evidence_missing');
      const release = envelope.result.checks?.find(check => check.id === 'release.integrity');
      if (!release || release.ok !== false || release.status !== 'warn') throw new Error('cli-smoke.unpackaged_release_warning_missing');
    }
    observations.push({args, exit: result.status, stdout, stderr});
  }
  const after = verifyArtifact(root, recordPath);
  if (after.binarySHA256 !== artifact.binarySHA256) throw new Error('cli-smoke.artifact_changed');
  const report = join(job.evidence, 'cli-smoke.json');
  writeFileSync(report, JSON.stringify({schema: 'lycheedev.cli-admission-smoke.v1', binarySHA256: artifact.binarySHA256, observations, scope: doctor ? 'isolated workspace and embedded doctor worker; no user-source or release parity claim' : 'read-only version/help/describe; no business parity claim'}, null, 2)+'\n');
  logs.push(report); console.log(readFileSync(report, 'utf8')); state = 'succeeded';
} finally {
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
  if (handlesReaped) job.cleanup();
}
