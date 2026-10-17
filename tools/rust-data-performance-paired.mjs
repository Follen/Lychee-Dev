// Opt-in B04 sibling of the existing paired entry; same owners/artifacts/clocks.
// No build, download, installation, game input or publication is performed.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {verifyArtifact, hashFile, sourceDigest} from './rust-build.mjs';
import {beginScratch} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {verifyGoPackage} from '../tests/rust-performance-paired/inputs.mjs';

const [artifact, goSource, goPackage, luaLSArchive, conditionsFile, catalog, content, images, sharpModule, ...extra] = process.argv.slice(2);
assert([artifact, goSource, goPackage, luaLSArchive, conditionsFile, catalog, content, images, sharpModule].every(Boolean) && (!extra.length || extra.length === 1 && extra[0] === '--acceptance'),
  'usage: node tools/rust-data-performance-paired.mjs <optimized-artifact.json> <clean-Go323-checkout> <Go-dev-npm-stage> <pinned-LuaLS.zip> <conditions.json> <catalog-fixture> <17MiB-fixture> <large-image-fixture> <installed-sharp-module> [--acceptance]');
assert.equal(process.platform, 'win32');
const root = resolve('.'), acceptance = extra.length === 1, rust = verifyArtifact(root, artifact), go = verifyGoPackage(goPackage, luaLSArchive);
assert(['release', 'symbols'].includes(rust.profile)); assert.equal(sourceDigest(root), rust.inputHashes.sourceTree);
assert(lstatSync(conditionsFile).size <= 16384);
const conditions = JSON.parse(readFileSync(conditionsFile));
for (const key of ['machine', 'powerAndThermal', 'competingLoad', 'disk', 'operator']) assert(typeof conditions[key] === 'string' && conditions[key].trim());
const paths = ['tools/rust-data-performance-paired.mjs', 'tests/rust-performance-paired/data-run.mjs', 'tests/rust-performance-paired/data-contract.mjs', 'tests/rust-performance-paired/data-inputs.mjs',
  'tests/rust-performance-paired/contract.mjs', 'tests/rust-performance-paired/inputs.mjs', 'tests/rust-source-full-corpus/assertions.mjs', 'tests/rust-parity/describe-contract.mjs', 'tests/rust-parity/oracle.mjs',
  'tools/rust-build.mjs', 'tools/rust-disk.mjs', 'tools/rust-emmy-identity.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-owned-memory.mjs', 'tools/rust-build-auxiliary.json', 'tools/zip.mjs', 'release/rust-disk-policy.json'];
const inputs = Object.fromEntries(paths.map(path => [path, hashFile(join(root, path))]));
const job = beginScratch(root, 'paired-data', {command: process.argv, inputs: {rust: rust.binarySHA256, go: go.packageSHA256, files: inputs}, admissionOptions: {requiredBytes: 1 << 30, scratchReserveBytes: 1 << 30}});
let state = 'failed', handlesReaped = true;
const start = performance.now();
try {
  const config = {root, evidence: job.evidence, scratch: job.directory, acceptance, inputs, rustArtifact: resolve(artifact), rust, go, goSource: resolve(goSource), luaLSArchive: resolve(luaLSArchive),
    conditions, fixtures: {catalog: resolve(catalog), content: resolve(content), images: resolve(images)}, sharpModule: resolve(sharpModule)};
  const configuration = join(job.evidence, 'configuration.json'); writeFileSync(configuration, JSON.stringify(config, null, 2) + '\n');
  handlesReaped = false;
  const run = ownedSpawnSync(process.execPath, [join(root, 'tests/rust-performance-paired/data-run.mjs'), configuration], {cwd: root, env: {...process.env}, evidence: job.evidence, timeout: 18000000});
  handlesReaped = run.handlesReaped === true;
  const reportPath = join(job.evidence, 'report.json'), observed = existsSync(reportPath) ? JSON.parse(readFileSync(reportPath)) : null;
  const stable = verifyArtifact(root, artifact).binarySHA256 === rust.binarySHA256 && sourceDigest(root) === rust.inputHashes.sourceTree && verifyGoPackage(goPackage, luaLSArchive).packageSHA256 === go.packageSHA256 && Object.entries(inputs).every(([path, sha]) => hashFile(join(root, path)) === sha);
  const passed = stable && handlesReaped && !run.error && run.status === 0 && observed?.state === (acceptance ? 'passed_selected_data_paths_full_acceptance_open' : 'equivalent_data_observation_only');
  const report = {schema: 'lycheedev.paired-data-owned.v1', state: passed ? observed.state : 'failed', performanceAcceptanceEvaluated: acceptance,
    fullB04Acceptance: false, fullB01B07Acceptance: false, observation: reportPath, inputs, rust, go, artifactAndRunnerStable: stable, wallMillis: performance.now() - start,
    process: {exit: run.status, error: run.error?.message, handlesReaped, proof: run.proof}};
  writeFileSync(join(job.evidence, 'owned-report.json'), JSON.stringify(report, null, 2) + '\n');
  state = passed ? 'succeeded' : 'failed';
  console.log(JSON.stringify({state: report.state, report: join(job.evidence, 'owned-report.json'), groups: observed?.groups}, null, 2));
  if (!passed) process.exitCode = 1;
} finally {
  const logs = readdirSync(job.evidence).filter(name => name !== 'receipt.json' && lstatSync(join(job.evidence, name)).isFile()).map(name => join(job.evidence, name));
  job.seal({state: handlesReaped ? state : 'interrupted', handlesReaped, logs});
  if (handlesReaped && state === 'succeeded') job.cleanup();
}
