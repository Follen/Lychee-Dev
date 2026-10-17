// Rust-only offline Live diagnostic; no paired Go host performance claim.
// No arbitrary executable/fixture arguments,
// game input, Go execution, release publication, or process-runner replacement.
import assert from 'node:assert/strict';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readdirSync, writeFileSync} from 'node:fs';
import {basename, dirname, isAbsolute, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {performance} from 'node:perf_hooks';
import {verifyPerformanceReport} from '../tests/rust-performance-paired/live-contract.mjs';
import {beginScratch, contained} from './rust-disk.mjs';
import {runCargo, sourceDigest, verifyArtifact} from './rust-build.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {verifyInterpreter} from './rust-formal-lua.mjs';
import {BASE, ORACLE, MANIFEST_SHA256, RELEASE_FIELDS_SHA256, SUPPORT as ORACLE_SUPPORT, authenticate, cleanLuaEnvironment, hash, materialize,
  processPassed, processReaped, regular, verifyMaterial, verifyOwnerEvidence} from '../tests/rust-parity/formal-lua-3.2.3/assertions.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// Optional paths select sealed records and a fresh output directory, never a
// bare executable. All original identity/receipt validation remains mandatory.
export function performancePaths(argv = [], repository = root) {
  assert(Array.isArray(argv) && (argv.length === 0 || argv.length === 2), 'live-offline.path_arguments');
  const values = argv.length ? argv : ['.tmp/ci-formal-lua-runtime/build-report.json', '.tmp/ci-live-offline-performance-test'];
  assert(values.every(value => typeof value === 'string' && value.length > 0 && value.length <= 4096 && !value.startsWith('--') && !value.includes('\0')), 'live-offline.path_arguments');
  const [runtimePath, artifactDirectory] = values.map(value => contained(repository, resolve(repository, value)));
  assert.equal(basename(runtimePath), 'build-report.json', 'live-offline.interpreter_record_required');
  assert(!existsSync(artifactDirectory), 'live-offline.artifact_directory_exists');
  return {runtimePath, artifactDirectory, artifactPath: join(artifactDirectory, 'artifact.json')};
}
const args = Object.freeze(['test', '--release', '--locked', '--offline', '-p', 'lycheedev-live', '--lib', '--no-run', '--message-format=json-render-diagnostics', '-j', '1']);
export const TEST = 'go_mailbox_v1::connection::projection::offline_performance::offline_driver_journal_performance';
const executeArgs = Object.freeze([TEST, '--exact', '--ignored', '--test-threads=1', '--nocapture']);
export const SUPPORT = Object.freeze(['tools/rust-live-offline-performance.mjs', 'tests/rust-performance-paired/live-contract.mjs', 'tools/rust-formal-lua.mjs', 'tools/rust-build.mjs', 'tools/rust-cargo.mjs', 'tools/rust-disk.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', ...ORACLE_SUPPORT]);
const supportIdentity = () => Object.fromEntries(SUPPORT.map(path => [path, hash(regular(join(root, path)))]));
const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n', {flag: 'wx'});

// The artifact must come from this Cargo invocation's structured output, not a
// newest-file scan or a guessed target path. This helper never launches it.
export function selectTestExecutable(stdout, target) {
  assert(typeof stdout === 'string' && Buffer.byteLength(stdout) <= 16 << 20, 'live-offline.cargo_output_budget');
  const messages = stdout.split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
  assert(messages.length <= 65536, 'live-offline.cargo_message_budget');
  const found = messages.filter(row => row.reason === 'compiler-artifact' && row.target?.name === 'lycheedev_live'
    && row.target?.kind?.includes('lib') && row.profile?.test === true && row.executable);
  assert.equal(found.length, 1, 'live-offline.test_executable_cardinality');
  const row = found[0], executable = resolve(row.executable);
  const within = relative(resolve(target), executable);
  assert(within && !isAbsolute(within) && within !== '..' && !within.startsWith('../') && !within.startsWith('..\\'), 'live-offline.test_executable_channel');
  assert.match(basename(executable), /^lycheedev_live-[a-f0-9]+\.exe$/);
  const complete = messages.filter(row => row.reason === 'build-finished');
  assert.equal(complete.length, 1, 'live-offline.cargo_completion_cardinality');
  assert.equal(complete[0].success, true, 'live-offline.cargo_completion_failed');
  return {row, executable};
}
export function verifyTestOutput(stdout) {
  const text = stdout.toString('utf8').replaceAll('\r\n', '\n');
  const rows = [...text.matchAll(/^test (.+) \.\.\. ok$/gm)].map(match => match[1]);
  assert.deepEqual(rows, [TEST], 'live-offline.expected_case');
  assert.equal([...text.matchAll(/^test result: /gm)].length, 1, 'live-offline.completion_cardinality');
  assert.match(text, /^test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out; finished in /m);
  return {passed: 1, failed: 0, ignored: 0, test: TEST};
}
function buildTest(support, inputs, {artifactDirectory, artifactPath}) {
  assert(!existsSync(artifactDirectory), 'live-offline.artifact_directory_exists');
  return runCargo(root, [...args], {owner: 'live-offline-performance-build', oneShot: true,
    onSuccess(invocation) {
      assert.deepEqual(supportIdentity(), support, 'live-offline.build_support_changed');
      assert.deepEqual(authenticate(root), inputs, 'live-offline.build_fixture_changed');
      const {row, executable} = selectTestExecutable(invocation.stdout, invocation.target);
      const bytes = regular(contained(root, executable), 128 << 20);
      mkdirSync(artifactDirectory);
      const binary = join(artifactDirectory, 'live_offline_performance.exe');
      copyFileSync(executable, binary);
      assert.equal(hash(regular(binary, 128 << 20)), hash(bytes));
      assert.equal(hash(regular(executable, 128 << 20)), hash(bytes));
      const artifact = {schema: 'lycheedev.rust-build-artifact.v1', profile: 'release-test', sourceRoot: root,
        sourceBinary: binary, binarySHA256: hash(bytes), bytes: bytes.length,
        inputHashes: invocation.inputHashes, environmentSHA256: invocation.environmentSHA256,
        channel: invocation.target, sameInvocation: true, command: ['cargo', ...args], buildEvidence: invocation.buildEvidence,
        cargoArtifact: row, oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, support};
      save(artifactPath, artifact);
      return artifact;
    },
  });
}
function verifyTestArtifact(expected, support, inputs, {artifactPath}) {
  const artifact = verifyArtifact(root, artifactPath);
  assert.deepEqual(artifact, expected, 'live-offline.test_artifact_changed');
  assert.equal(artifact.schema, 'lycheedev.rust-build-artifact.v1');
  assert.equal(artifact.sameInvocation, true); assert.equal(artifact.sourceRoot, root);
  assert.deepEqual(artifact.command, ['cargo', ...args]);
  assert.equal(artifact.inputHashes.sourceTree, sourceDigest(root), 'live-offline.source_changed');
  assert.deepEqual(supportIdentity(), support); assert.deepEqual(authenticate(root), inputs);
  const evidence = verifyOwnerEvidence(root, artifact.buildEvidence, 'live-offline-performance-build');
  const command = evidence.json('command.json');
  assert.equal(command.command, 'cargo'); assert.deepEqual(command.args, args); assert.equal(command.cwd, root);
  assert.equal(command.exit, 0); assert(!command.error && !command.signal);
  assert.deepEqual(command.inputHashes, artifact.inputHashes); assert.equal(command.environmentSHA256, artifact.environmentSHA256);
  assert.equal(evidence.processes.length, 1);
  const request = evidence.processes[0].request;
  assert.equal(request.command, 'cargo'); assert.deepEqual(request.args, args); assert.equal(request.cwd, root); assert.equal(request.cargoBuild, true);
  const stdout = evidence.logs.get(join(dirname(artifact.buildEvidence), 'stdout.bin'));
  assert(stdout, 'live-offline.build_stdout_missing');
  const selected = selectTestExecutable(stdout.toString('utf8'), artifact.channel);
  assert.deepEqual(selected.row, artifact.cargoArtifact);
  return {artifactSHA256: hash(regular(artifactPath)), buildReceiptSHA256: evidence.receiptSHA256};
}
function collectFiles(directory, cap = 64 << 20) {
  const files = []; let bytes = 0, count = 0;
  function walk(path) {
    const metadata = lstatSync(path);
    assert(!metadata.isSymbolicLink() && ++count <= 2048, 'live-offline.evidence_path_budget');
    if (metadata.isDirectory()) for (const child of readdirSync(path).sort()) walk(join(path, child));
    else {
      assert(metadata.isFile() && metadata.size <= 16 << 20, 'live-offline.evidence_file_budget');
      bytes += metadata.size; assert(bytes <= cap, 'live-offline.evidence_byte_budget');
      files.push(path);
    }
  }
  walk(directory); return files;
}
export function main(argv = []) {
  const controllerStarted = performance.now();
  const paths = performancePaths(argv);
  const {runtimePath} = paths;
  assert.equal(process.platform, 'win32'); assert.equal(process.arch, 'x64');
  const support = supportIdentity(), inputs = authenticate(root);
  assert.equal(support[`${BASE}/release-fields.json`], RELEASE_FIELDS_SHA256, 'live-offline.release_fields_identity');
  const runtime = verifyInterpreter(runtimePath);
  const test = buildTest(support, inputs, paths);
  const testIdentity = verifyTestArtifact(test, support, inputs, paths);
  const job = beginScratch(root, 'live-offline-performance', {command: ['node', 'tools/rust-live-offline-performance.mjs', ...argv],
    inputs: {oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, interpreterSHA256: runtime.report.executableSHA256,
      interpreterReportSHA256: runtime.reportSHA256,
      testArtifact: testIdentity, testBinarySHA256: test.binarySHA256, support},
    admissionOptions: {requiredBytes: 128 << 20, scratchReserveBytes: 64 << 20}});
  const fixture = join(job.directory, 'fixture'), caseRoot = join(job.directory, 'cases');
  const admissionPath = join(job.directory, 'admission.json');
  let handlesReaped = true, succeeded = false, failure;
  try {
    materialize(root, fixture, inputs); mkdirSync(caseRoot);
    save(admissionPath, {interpreter: {report: runtimePath, reportSHA256: runtime.reportSHA256,
      buildReceiptSHA256: runtime.buildReceiptSHA256}, fixture: {root: fixture, oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, releaseFieldsSHA256: RELEASE_FIELDS_SHA256},
      scratch: {receipt: job.receipt, cases: caseRoot}});
    save(join(job.evidence, 'identity.json'), {runtime, test, testIdentity, support, inputs});
    const checkIdentity = () => {
      assert.deepEqual(verifyInterpreter(runtimePath), runtime);
      assert.deepEqual(verifyTestArtifact(test, support, inputs, paths), testIdentity);
      verifyMaterial(fixture, inputs);
    };
    checkIdentity(); handlesReaped = false;
    const invocationStarted = performance.now();
    const result = ownedSpawnSync(test.sourceBinary, [...executeArgs], {cwd: root,
      env: cleanLuaEnvironment({...process.env, LYCHEEDEV_FORMAL_LUA_BRIDGE_ADMISSION: admissionPath}),
      evidence: job.evidence, timeout: 690000, cargoBuild: false, msvcCompile: false});
    const ownedInvocationWallMillis = performance.now() - invocationStarted;
    handlesReaped = processReaped(result) === true;
    checkIdentity();
    assert(processPassed(result), `live-offline.process_failed:${result.error?.message ?? result.status}`);
    const counts = verifyTestOutput(result.stdout);
    const measurement = JSON.parse(regular(join(caseRoot, 'performance.json'), 4 << 20));
    const validated = verifyPerformanceReport(measurement);
    const lifetimes = readdirSync(caseRoot).filter(name => name !== 'performance.json').sort().map(name => {
      const proof = JSON.parse(regular(join(caseRoot, name, 'peer-lifetime.json')));
      assert.equal(proof.schema, 'lycheedev.formal-lua-peer-lifetime.v1');
      assert.equal(proof.rootReaped, true); assert.equal(proof.rootExit, 0);
      assert.equal(proof.jobEmpty, true); assert.equal(proof.workersJoined, true);
      assert.equal(proof.forced, false); assert.deepEqual(proof.errors, []);
      return {case: name, ...proof};
    });
    assert.deepEqual(lifetimes.map(row => row.case), validated.caseNames);
    save(join(job.evidence, 'report.json'), {schema: 'lycheedev.live-offline-controller.v1', state: 'passed',
      oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, releaseFieldsSHA256: RELEASE_FIELDS_SHA256,
      counts, lifetimes, measurement, validation: validated, ownedInvocationWallMillis,
      controllerThroughVerificationMillis: performance.now() - controllerStarted,
      testBinarySHA256: test.binarySHA256, testIdentity,
      interpreterReportSHA256: runtime.reportSHA256, handlesReaped,
      scope: 'Rust-only optimized offline Live diagnostic. Complete wrapper-inclusive owned invocation wall includes request preparation, PowerShell startup, root startup, test checks, peer work, cleanup and result collection; compilation and controller preparation are separately included in controllerThroughVerificationMillis. No Go host timing, no ratio, no game input or latency. Inner handles retain the existing std/JobGuard Drop boundary.'});
    succeeded = true;
  } catch (error) {
    failure = error;
    save(join(job.evidence, 'failure.json'), {error: error.message, handlesReaped, scratchRetained: true});
  } finally {
    // Seal journal/result/record/pipe evidence before any successful cleanup.
    // Failures and uncertain cleanup always retain their original scratch.
    let logs;
    try {
      logs = [...collectFiles(job.evidence).filter(path => basename(path) !== 'receipt.json'),
        ...(existsSync(admissionPath) ? [admissionPath] : []), ...(existsSync(caseRoot) ? collectFiles(caseRoot) : [])];
      assert(logs.reduce((total, path) => total + lstatSync(path).size, 0) <= 64 << 20, 'live-offline.total_evidence_budget');
      job.seal({state: !handlesReaped ? 'interrupted' : succeeded ? 'succeeded' : 'failed', handlesReaped, logs});
      if (succeeded && handlesReaped) job.cleanup();
    } catch (error) { failure ??= error; succeeded = false; }
  }
  if (failure) throw Object.assign(failure, {performanceEvidence: job.receipt, scratch: job.directory});
  return {passed: succeeded, report: join(job.evidence, 'report.json'), receipt: job.receipt, expectedTest: TEST, expectedSamples: 27, controllerWallMillis: performance.now() - controllerStarted};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { console.log(JSON.stringify(main(process.argv.slice(2)), null, 2)); }
  catch (error) { console.error(JSON.stringify({error: error.message, performanceEvidence: error.performanceEvidence, scratch: error.scratch})); process.exitCode = 1; }
}
