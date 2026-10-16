// Explicit opt-in Windows acceptance. No builds, downloads of executables,
// game interaction, publication, CI dispatch or user-home selection here.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {verifyArtifact, hashFile} from './rust-build.mjs';
import {beginScratch} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {verifyGoPackage} from '../tests/rust-performance-paired/inputs.mjs';

const [artifactPath, goSource, goPackage, luaLSArchive, conditionsPath, ...extra] = process.argv.slice(2);
assert(artifactPath && goSource && goPackage && luaLSArchive && conditionsPath && !extra.length,
  'usage: node tools/rust-performance-paired.mjs <optimized-artifact.json> <clean-fixed-Go-checkout> <Go-dev-npm-stage> <pinned-LuaLS.zip> <machine-conditions.json>');
assert.equal(process.platform, 'win32', 'paired-performance.windows_required');
const root = resolve('.'), rust = verifyArtifact(root, artifactPath), go = verifyGoPackage(goPackage, luaLSArchive);
assert(['release', 'symbols'].includes(rust.profile), 'paired-performance.optimized_rust_required');
assert(lstatSync(conditionsPath).isFile() && lstatSync(conditionsPath).size <= 16384);
const conditions = JSON.parse(readFileSync(conditionsPath));
for (const field of ['machine', 'powerAndThermal', 'competingLoad', 'disk', 'operator']) assert(typeof conditions[field] === 'string' && conditions[field].trim(), `machine-conditions.${field} required; disclose unknowns explicitly`);
const paths = ['tools/rust-performance-paired.mjs', 'tests/rust-performance-paired/run.mjs', 'tests/rust-performance-paired/contract.mjs', 'tests/rust-performance-paired/inputs.mjs', 'tests/rust-source-full-corpus/assertions.mjs', 'tests/rust-source-full-corpus/retail-oracle.json', 'tests/rust-parity/describe-contract.mjs', 'tests/rust-parity/oracle.mjs', 'tools/rust-build.mjs', 'tools/rust-disk.mjs', 'tools/rust-emmy-identity.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-owned-memory.mjs', 'tools/rust-build-auxiliary.json', 'tools/zip.mjs', 'release/rust-disk-policy.json'];
const inputs = Object.fromEntries(paths.map(path => [path, hashFile(join(root, path))]));
const job = beginScratch(root, 'paired-performance-selected', {command: process.argv, inputs: {rust: rust.binarySHA256, go: go.packageSHA256, files: inputs}, admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 1 << 30}});
let state = 'failed', handlesReaped = true;
const start = performance.now();
try {
  const configPath = join(job.evidence, 'configuration.json');
  writeFileSync(configPath, JSON.stringify({root, evidence: job.evidence, scratch: job.directory, rustArtifact: resolve(artifactPath), rust, go, goSource: resolve(goSource), luaLSArchive: resolve(luaLSArchive), conditions}, null, 2) + '\n');
  handlesReaped = false;
  // Outer Job owns the controller, every nested sample Job/wrapper, Git audit
  // and descendants. No unrelated process discovery or taskkill is used.
  const run = ownedSpawnSync(process.execPath, [join(root, 'tests/rust-performance-paired/run.mjs'), configPath], {cwd: root, env: {...process.env}, evidence: job.evidence, timeout: 18000000});
  handlesReaped = run.handlesReaped === true;
  const path = join(job.evidence, 'report.json');
  const observation = existsSync(path) ? JSON.parse(readFileSync(path)) : null;
  const stable = verifyArtifact(root, artifactPath).binarySHA256 === rust.binarySHA256 && verifyGoPackage(goPackage, luaLSArchive).packageSHA256 === go.packageSHA256 && Object.entries(inputs).every(([path, hash]) => hashFile(join(root, path)) === hash);
  const passed = stable && handlesReaped && run.status === 0 && !run.error && observation?.state === 'passed_selected_paths_full_acceptance_open' && observation?.groups?.length === 6 && observation.groups.every(group => group.state === 'passed_selected_path');
  const report = {schema: 'lycheedev.paired-performance-owned.v1', passedSelectedPaths: passed, fullB01B07Acceptance: false, state: passed ? 'passed_selected_paths_full_acceptance_open' : 'failed', observation: path, inputs, rust, go, artifactAndRunnerStable: stable, wallMillis: performance.now() - start, process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof}, cpu: 'not_measured', io: 'not_measured', memory: 'Per-sample nested Jobs only; do not sum overlapping outer and nested process membership.'};
  writeFileSync(join(job.evidence, 'owned-report.json'), JSON.stringify(report, null, 2) + '\n');
  state = passed ? 'succeeded' : 'failed';
  console.log(JSON.stringify({state: report.state, report: join(job.evidence, 'owned-report.json'), groups: observation?.groups}, null, 2));
  if (!passed) process.exitCode = 1;
} finally {
  const logs = readdirSync(job.evidence).filter(name => name !== 'receipt.json' && lstatSync(join(job.evidence, name)).isFile()).map(name => join(job.evidence, name));
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
  if (handlesReaped && state === 'succeeded') job.cleanup();
}
