// Execute admission commands, an isolated doctor, or fixed-Go describe checks
// from a sealed artifact. Command-surface evidence is not business parity.
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { verifyArtifact } from './rust-build.mjs';
import { beginScratch } from './rust-disk.mjs';
import { ownedSpawnSync } from './rust-owned-process.mjs';
import { buildFixture, readBounded, sha256 } from '../tests/rust-parity/oracle.mjs';
import { compareDescribeContract } from '../tests/rust-parity/describe-contract.mjs';

function frozenOracle(root) {
  const oracle = buildFixture(root);
  const bytes = readBounded(root, 'tests/rust-parity/go-3.2.0.manifest.json');
  if (!bytes.equals(Buffer.from(JSON.stringify(oracle, null, 2) + '\n'))) throw new Error('cli-smoke.oracle_fixture_changed');
  return {oracle, fixtureSHA256: sha256(bytes)};
}

// Dependency injection is only for controlled Node checker/fault fixtures.
// The executable entrypoint below always uses the real custody/process tools.
export function runCliSmoke(root, recordPath, {mode = 'admission', verify = verifyArtifact,
  begin = beginScratch, execute = ownedSpawnSync, loadOracle = frozenOracle} = {}) {
  if (!['admission', 'doctor', 'contract'].includes(mode)) throw new Error('cli-smoke.invalid_mode');
  root = resolve(root);
  const artifact = verify(root, recordPath);
  const job = begin(root, 'ci-cli-smoke', {command: process.argv, inputs: {binarySHA256: artifact.binarySHA256}});
  let state = 'failed', handlesReaped = true, contract;
  const logs = [], observations = [];
  const reportPath = join(job.evidence, 'cli-smoke.json');
  const report = {schema: 'lycheedev.cli-admission-smoke.v1', mode, binarySHA256: artifact.binarySHA256,
    sourceTreeSHA256: artifact.inputHashes?.sourceTree, observations,
    scope: mode === 'doctor' ? 'isolated workspace and embedded doctor worker; no user-source or release parity claim'
      : mode === 'contract' ? 'executed describe command surface against frozen Go; no business parity claim'
        : 'read-only version/help/describe; no business parity claim'};
  try {
    const frozen = mode === 'contract' ? loadOracle(root) : null;
    if (frozen) report.fixtureSHA256 = frozen.fixtureSHA256;
    const commands = mode === 'doctor' ? [
      ['init', '--home', join(job.directory, 'home'), '--format=json'],
      ['doctor', '--home', join(job.directory, 'home'), '--offline', '--format=json'],
    ] : mode === 'contract' ? [['describe', '--format=json']]
      : [['version', '--format=json'], ['--help', '--format=json'], ['describe', '--format=json']];
    for (const args of commands) {
      const index = observations.length;
      handlesReaped = false;
      const result = execute(artifact.sourceBinary, args, {
        cwd: root, env: {...process.env}, evidence: job.evidence, timeout: 120000,
      });
      handlesReaped = result.handlesReaped === true;
      logs.push(...(result.logs ?? []));
      const stdout = join(job.evidence, `cli-${index}.stdout.json`);
      const stderr = join(job.evidence, `cli-${index}.stderr.bin`);
      writeFileSync(stdout, result.stdout ?? Buffer.alloc(0)); writeFileSync(stderr, result.stderr ?? Buffer.alloc(0));
      logs.push(stdout, stderr);
      observations.push({args, exit: result.status, stdout, stderr, handlesReaped, error: result.error?.message ?? null});
      if (!handlesReaped || result.error || result.status !== 0) throw new Error(`cli-smoke.execution:${result.error?.message ?? result.status}`);
      const envelope = JSON.parse(readFileSync(stdout, 'utf8'));
      if (mode === 'contract') {
        contract = {...compareDescribeContract(frozen.oracle, envelope),
          binarySHA256: artifact.binarySHA256, sourceTreeSHA256: artifact.inputHashes?.sourceTree,
          fixtureSHA256: frozen.fixtureSHA256, observation: observations[index]};
        const path = join(job.evidence, 'cli-contract.json');
        writeFileSync(path, JSON.stringify(contract, null, 2) + '\n');
        logs.push(path); report.contract = {path, state: contract.state, rawExactMatch: contract.rawExactMatch,
          acceptedContractMatch: contract.acceptedContractMatch, approvedEngineSummaryChanges: contract.approvedEngineSummaryChanges};
      } else {
        if (envelope.ok !== true || !envelope.result) throw new Error('cli-smoke.invalid_envelope');
        if (args[0] === 'version' && typeof envelope.result.version !== 'string') throw new Error('cli-smoke.version');
        if (['--help', 'describe'].includes(args[0]) && !Array.isArray(envelope.result.commands)) throw new Error('cli-smoke.commands');
        if (args[0] === 'doctor') {
          const check = envelope.result.checks?.find(check => check.id === 'emmy.runtime');
          if (check?.ok !== true || check.scope !== 'embedded-worker-fixture' || check.lifecycle?.retired !== true) throw new Error('cli-smoke.emmy_evidence_missing');
          const release = envelope.result.checks?.find(check => check.id === 'release.integrity');
          if (!release || release.ok !== false || release.status !== 'warn') throw new Error('cli-smoke.unpackaged_release_warning_missing');
        }
      }
    }
    const after = verify(root, recordPath);
    if (after.binarySHA256 !== artifact.binarySHA256) throw new Error('cli-smoke.artifact_changed');
    state = contract && !contract.acceptedContractMatch ? 'failed' : 'succeeded';
    return {state, exitCode: state === 'succeeded' ? 0 : 1, reportPath, contract};
  } catch (error) {
    report.error = error.message;
    throw error;
  } finally {
    report.state = handlesReaped ? state : 'interrupted';
    writeFileSync(reportPath, JSON.stringify(report, null, 2) + '\n');
    logs.push(reportPath);
    job.seal({state: report.state, handlesReaped, logs});
    if (handlesReaped) job.cleanup();
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const [recordPath, ...extra] = process.argv.slice(2);
  const mode = extra.length === 0 ? 'admission' : extra.length === 1 && extra[0] === '--doctor' ? 'doctor'
    : extra.length === 1 && extra[0] === '--contract' ? 'contract' : null;
  if (!recordPath || !mode) throw new Error('usage: node tools/rust-cli-smoke.mjs <artifact.json> [--doctor | --contract]');
  const result = runCliSmoke(process.cwd(), recordPath, {mode});
  console.log(readFileSync(result.reportPath, 'utf8'));
  if (result.contract) console.log(JSON.stringify(result.contract, null, 2));
  process.exitCode = result.exitCode;
}
