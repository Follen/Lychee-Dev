// Thin owner for the Data-specific controller; no new Job/pressure mechanism.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, readdirSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {beginScratch, contained} from '../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {currentWorkflowInputs, inventory, inventoryHash, jsonFile, loadCurrentWorkflow, pendingIncrementTasks, verifyCandidate} from './assertions.mjs';
import {loadExporters} from './data-inputs.mjs';
import {assertDataReport, pendingDataTasks} from './data-assertions.mjs';
import {assertDataSkillDirectory, assertDataSkillInventory, dataSkillIdentity} from './data-skill-contract.mjs';

export function findNpm(node = process.execPath, environment = process.env) {
  const paths = [environment.npm_execpath, join(dirname(node), 'node_modules/npm/bin/npm-cli.js')].filter(Boolean);
  for (const path of paths) {
    if (!existsSync(path)) continue;
    const packagePath = join(dirname(dirname(path)), 'package.json');
    if (!existsSync(packagePath)) continue;
    const pkg = jsonFile(packagePath); if (pkg.name !== 'npm') continue;
    return {path: resolve(path), sha256: hashFile(path), packagePath, packageSHA256: hashFile(packagePath), version: pkg.version};
  }
  throw new Error('skill-data.installed_npm_cli_required');
}
export function runOwnedData(root, argv) {
  assert.equal(argv[0], '--historical-oracle-only', 'retired SQL Skill controller is historical, never current Agent acceptance');
  argv = argv.slice(1);
  assert.equal(process.platform, 'win32'); assert.equal(process.arch, 'x64');
  assert.equal(argv.length, 3, 'usage: --data <artifact.json> <candidate-report.json> <fresh-exporter-buildEvidence-receipt.json>');
  root = resolve(root);
  // The source witness/cwd remains the sealed product tree; a newer runner may
  // be selected without rebuilding or relabeling that product's sourceDigest.
  const runnerRoot = resolve(import.meta.dirname, '../..');
  const sharedTools = ['tools/rust-build.mjs', 'tools/rust-emmy-identity.mjs', 'tools/rust-disk.mjs',
    'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-build-auxiliary.json'];
  for (const path of sharedTools) assert.equal(hashFile(join(runnerRoot, path)), hashFile(join(root, path)), `unchanged product identity/ownership tool: ${path}`);
  const [artifactPath, candidatePath, exportersPath] = argv.map(path => contained(root, resolve(path)));
  const artifact = verifyArtifact(root, artifactPath), artifactSHA256 = hashFile(artifactPath), sourceTree = sourceDigest(root);
  assert.equal(artifact.inputHashes.sourceTree, sourceTree, 'current matching product artifact required');
  const workflow = loadCurrentWorkflow(runnerRoot), candidate = verifyCandidate(root, candidatePath, artifact, workflow);
  assert.equal(candidate.report.artifactRecordSHA256, artifactSHA256);
  assertDataSkillDirectory(join(root, 'skills/lycheedev'));
  assertDataSkillDirectory(join(candidate.packageRoot, 'payload/skill'));
  const skillIdentity = dataSkillIdentity(assertDataSkillInventory(candidate.files));
  const exporters = loadExporters(root, exportersPath, sourceTree), npm = findNpm();
  const inputNames = ['tools/rust-skill-usage.mjs', ...['data-owned.mjs', 'data-run.mjs', 'data-inputs.mjs', 'data-exporters.mjs', 'data-assertions.mjs', 'data-skill-contract.mjs', 'data-runner.test.mjs', 'assertions.mjs'].map(name => `tests/rust-skill-usage/${name}`), ...currentWorkflowInputs(workflow), 'tests/rust-performance-paired/data-inputs.mjs', 'tests/rust-performance-paired/data-contract.mjs', 'tests/rust-performance-paired/inputs.mjs', 'tests/rust-performance-paired/contract.mjs', 'tests/rust-parity/describe-contract.mjs', 'tests/rust-source-full-corpus/assertions.mjs', 'tools/rust-build.mjs', 'tools/rust-disk.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-emmy-identity.mjs', 'tools/zip.mjs'];
  const testInputs = Object.fromEntries([...new Set([...inputNames, ...sharedTools])].map(path => [path, hashFile(join(runnerRoot, path))]));
  const nodeSHA256 = hashFile(process.execPath), job = beginScratch(root, 'skill-data-usage', {command: process.argv, inputs: {sourceTree, runnerRoot, skillIdentity, artifactSHA256, binarySHA256: artifact.binarySHA256, candidateSHA256: candidate.reportSHA256, packageSHA256: candidate.packageSHA256, exporterReceiptSHA256: exporters.receiptSHA256, testInputs, nodeSHA256, npm}, admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 1 << 30}});
  const prefix = join(job.directory, 'npm-prefix');
  const config = {schema: 'lycheedev.skill-data-configuration.v2', root, runnerRoot, skillIdentity, scratch: job.directory, evidence: job.evidence, ownerId: job.id, platform: process.platform, arch: process.arch, node: process.execPath, nodeVersion: process.version, nodeSHA256, npm, sourceTree, testInputs, artifactPath, artifactSHA256, binarySHA256: artifact.binarySHA256, candidatePath, candidateSHA256: candidate.reportSHA256, sourcePackage: candidate.packageRoot, packageSHA256: candidate.packageSHA256, packageFiles: candidate.files, version: candidate.manifest.version, exporters, prefix, launcher: join(prefix, 'node_modules/lycheedev/payload/skill/scripts/lycheedev.mjs'), tgz: join(job.directory, `lycheedev-${candidate.manifest.version}.tgz`)};
  const save = (name, value) => writeFileSync(join(job.evidence, name), JSON.stringify(value, null, 2) + '\n');
  let state = 'failed', handlesReaped = true; const logs = [];
  try {
    save('configuration.json', config); save('candidate-identity.json', {artifact, candidateReport: candidate.report, candidateManifest: candidate.manifest});
    handlesReaped = false;
    const run = ownedSpawnSync(process.execPath, [join(runnerRoot, 'tests/rust-skill-usage/data-run.mjs'), '--historical-oracle-only', join(job.evidence, 'configuration.json')], {cwd: root, env: {...process.env}, evidence: job.evidence, timeout: 1200000});
    handlesReaped = run.handlesReaped === true; logs.push(...run.logs);
    const report = existsSync(join(job.evidence, 'report.json')) ? jsonFile(join(job.evidence, 'report.json'), 12 << 20) : null;
    let contractError;
    try {
      assert.deepEqual(report.config, config, 'same original owner, host, runtime, package and source configuration');
      assertDataReport(report, workflow);
      assert.equal(sourceDigest(root), sourceTree); assert.equal(hashFile(artifactPath), artifactSHA256); assert.equal(verifyArtifact(root, artifactPath).binarySHA256, artifact.binarySHA256);
      assert.equal(hashFile(candidatePath), candidate.reportSHA256); assert.equal(inventoryHash(inventory(candidate.packageRoot)), candidate.packageSHA256);
      assert.deepEqual(loadExporters(root, exportersPath, sourceTree), exporters);
      assert.equal(hashFile(process.execPath), nodeSHA256); assert.deepEqual(findNpm(), npm);
      for (const [path, expected] of Object.entries(testInputs)) assert.equal(hashFile(join(runnerRoot, path)), expected);
      assertDataSkillDirectory(join(root, 'skills/lycheedev'));
      assert.deepEqual(inventory(join(prefix, 'node_modules/lycheedev')), candidate.files);
      assert.equal(hashFile(join(prefix, 'node_modules/lycheedev/native/windows-amd64/lycheedev.exe'), 128 << 20), artifact.binarySHA256);
      assert.equal(hashFile(config.tgz, 128 << 20), report.tgz.sha256); assert.equal(lstatSync(config.tgz).size, report.tgz.bytes);
      assert.equal(hashFile(join(prefix, 'lycheedev.cmd')), report.npmShim.sha256);
    } catch (error) { contractError = error.message; }
    const passed = handlesReaped && !run.error && run.status === 0 && !contractError;
    state = passed ? 'succeeded' : 'failed';
    save('owned-report.json', {schema: 'lycheedev.skill-data-owned.v2', passed, completeSkillAcceptance: false, pendingTasks: passed ? pendingDataTasks(workflow) : pendingIncrementTasks(workflow), contractError, sourceTree, runnerRoot, skillIdentity, testInputs, artifactSHA256, binarySHA256: artifact.binarySHA256, candidateSHA256: candidate.reportSHA256, packageSHA256: candidate.packageSHA256, exporterReceiptSHA256: exporters.receiptSHA256, observation: join(job.evidence, 'report.json'), process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof}});
    console.log(JSON.stringify({passed, report: join(job.evidence, 'owned-report.json'), completeSkillAcceptance: false, pendingTasks: passed ? pendingDataTasks(workflow) : pendingIncrementTasks(workflow), contractError}, null, 2));
    if (!passed) process.exitCode = 1;
  } catch (error) { save('controller-failure.json', {error: error.stack, handlesReaped}); throw error; }
  finally {
    for (const name of readdirSync(job.evidence)) { const path = join(job.evidence, name); if (name !== 'receipt.json' && lstatSync(path).isFile() && !logs.includes(path)) logs.push(path); }
    job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
    // Failed runs remain inspectable; only a successful sealed owner is retired.
    if (handlesReaped && state === 'succeeded') job.cleanup();
  }
}
