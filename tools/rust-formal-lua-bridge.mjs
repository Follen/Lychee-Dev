// Fixed Go recovery bridge only. No arbitrary executable/fixture arguments,
// game input, Go execution, release publication, or process-runner replacement.
import assert from 'node:assert/strict';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readdirSync, writeFileSync} from 'node:fs';
import {basename, dirname, isAbsolute, join, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {beginScratch, contained} from './rust-disk.mjs';
import {ownedUserEnvironment, runCargo, sourceDigest, verifyArtifact} from './rust-build.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {verifyCandidateArtifact, verifyInterpreter} from './rust-formal-lua.mjs';
import {BASE, ORACLE, MANIFEST_SHA256, RELEASE_FIELDS_SHA256, SUPPORT as ORACLE_SUPPORT, authenticate, cleanLuaEnvironment, hash, materialize,
  processPassed, processReaped, regular, verifyMaterial, verifyOwnerEvidence} from '../tests/rust-parity/formal-lua-3.2.3/assertions.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// Optional paths select sealed records and a fresh output directory, never a
// bare executable. All original identity/receipt validation remains mandatory.
export function bridgePaths(argv = [], repository = root) {
  assert(Array.isArray(argv) && (argv.length === 0 || argv.length === 3), 'formal-bridge.path_arguments');
  const values = argv.length ? argv : ['.tmp/ci-formal-lua-runtime/build-report.json', '.tmp/ci-cli-artifact/artifact.json', '.tmp/ci-formal-lua-bridge-test'];
  assert(values.every(value => typeof value === 'string' && value.length > 0 && value.length <= 4096 && !value.startsWith('--') && !value.includes('\0')), 'formal-bridge.path_arguments');
  const [runtimePath, candidatePath, artifactDirectory] = values.map(value => contained(repository, resolve(repository, value)));
  assert.equal(basename(runtimePath), 'build-report.json', 'formal-bridge.interpreter_record_required');
  assert.equal(basename(candidatePath), 'artifact.json', 'formal-bridge.candidate_record_required');
  assert(!existsSync(artifactDirectory), 'formal-bridge.artifact_directory_exists');
  return {runtimePath, candidatePath, artifactDirectory, artifactPath: join(artifactDirectory, 'artifact.json')};
}
const args = Object.freeze(['test', '--locked', '--offline', '-p', 'lycheedev-live', '--test', 'formal_lua_bridge', '--no-run', '--message-format=json-render-diagnostics', '-j', '1']);
const cases = Object.freeze(['normal', 'after_bind', 'after_prepare', 'after_commit', 'after_confirm', 'after_release', 'reload_bind', 'compile_error', 'cleanup_error'].map(value => `formal_lua_recovery_${value}`));
export const SUPPORT = Object.freeze(['tools/rust-formal-lua-bridge.mjs', 'tools/rust-formal-lua.mjs', 'tools/rust-build.mjs', 'tools/rust-cargo.mjs', 'tools/rust-disk.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', ...ORACLE_SUPPORT]);
const supportIdentity = () => Object.fromEntries(SUPPORT.map(path => [path, hash(regular(join(root, path)))]));
const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n', {flag: 'wx'});

// The artifact must come from this Cargo invocation's structured output, not a
// newest-file scan or a guessed target path. This helper never launches it.
export function selectTestExecutable(stdout, target) {
  assert(typeof stdout === 'string' && Buffer.byteLength(stdout) <= 16 << 20, 'formal-bridge.cargo_output_budget');
  const messages = stdout.split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
  assert(messages.length <= 65536, 'formal-bridge.cargo_message_budget');
  const found = messages.filter(row => row.reason === 'compiler-artifact' && row.target?.name === 'formal_lua_bridge'
    && row.target?.kind?.includes('test') && row.profile?.test === true && row.executable);
  assert.equal(found.length, 1, 'formal-bridge.test_executable_cardinality');
  const row = found[0], executable = resolve(row.executable);
  const within = relative(resolve(target), executable);
  assert(within && !isAbsolute(within) && within !== '..' && !within.startsWith('../') && !within.startsWith('..\\'), 'formal-bridge.test_executable_channel');
  assert.match(basename(executable), /^formal_lua_bridge-[a-f0-9]+\.exe$/);
  const complete = messages.filter(row => row.reason === 'build-finished');
  assert.equal(complete.length, 1, 'formal-bridge.cargo_completion_cardinality');
  assert.equal(complete[0].success, true, 'formal-bridge.cargo_completion_failed');
  return {row, executable};
}
export function verifyTestOutput(stdout) {
  const text = stdout.toString('utf8').replaceAll('\r\n', '\n');
  const rows = [...text.matchAll(/^test (.+) \.\.\. ok$/gm)].map(match => match[1]).sort();
  assert.deepEqual(rows, [...cases].sort(), 'formal-bridge.expected_cases');
  assert.equal([...text.matchAll(/^test result: /gm)].length, 1, 'formal-bridge.completion_cardinality');
  assert.match(text, /^test result: ok\. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in /m);
  return {passed: 9, failed: 0, ignored: 0, filtered: 0, cases: rows};
}
function buildTest(support, inputs, candidate, {artifactDirectory, artifactPath}) {
  assert(!existsSync(artifactDirectory), 'formal-bridge.artifact_directory_exists');
  return runCargo(root, [...args], {owner: 'formal-lua-bridge-build', oneShot: false, isolateUser: true,
    onSuccess(invocation) {
      assert.deepEqual(supportIdentity(), support, 'formal-bridge.build_support_changed');
      assert.deepEqual(authenticate(root), inputs, 'formal-bridge.build_fixture_changed');
      assert.equal(invocation.inputHashes.sourceTree, candidate.sourceTree, 'formal-bridge.candidate_source_mismatch');
      const {row, executable} = selectTestExecutable(invocation.stdout, invocation.target);
      const bytes = regular(contained(root, executable), 128 << 20);
      mkdirSync(artifactDirectory);
      const binary = join(artifactDirectory, 'formal_lua_bridge.exe');
      copyFileSync(executable, binary);
      assert.equal(hash(regular(binary, 128 << 20)), hash(bytes));
      assert.equal(hash(regular(executable, 128 << 20)), hash(bytes));
      const artifact = {schema: 'lycheedev.rust-build-artifact.v1', profile: 'test', sourceRoot: root,
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
  assert.deepEqual(artifact, expected, 'formal-bridge.test_artifact_changed');
  assert.equal(artifact.schema, 'lycheedev.rust-build-artifact.v1');
  assert.equal(artifact.sameInvocation, true); assert.equal(artifact.sourceRoot, root);
  assert.deepEqual(artifact.command, ['cargo', ...args]);
  assert.equal(artifact.inputHashes.sourceTree, sourceDigest(root), 'formal-bridge.source_changed');
  assert.deepEqual(supportIdentity(), support); assert.deepEqual(authenticate(root), inputs);
  const evidence = verifyOwnerEvidence(root, artifact.buildEvidence, 'formal-lua-bridge-build');
  const command = evidence.json('command.json');
  assert.equal(command.command, 'cargo'); assert.deepEqual(command.args, args); assert.equal(command.cwd, root);
  assert.equal(command.exit, 0); assert(!command.error && !command.signal);
  assert.deepEqual(command.inputHashes, artifact.inputHashes); assert.equal(command.environmentSHA256, artifact.environmentSHA256);
  assert.equal(evidence.processes.length, 1);
  const request = evidence.processes[0].request;
  assert.equal(request.command, 'cargo'); assert.deepEqual(request.args, args); assert.equal(request.cwd, root); assert.equal(request.cargoBuild, true);
  const stdout = evidence.logs.get(join(dirname(artifact.buildEvidence), 'stdout.bin'));
  assert(stdout, 'formal-bridge.build_stdout_missing');
  const selected = selectTestExecutable(stdout.toString('utf8'), artifact.channel);
  assert.deepEqual(selected.row, artifact.cargoArtifact);
  return {artifactSHA256: hash(regular(artifactPath)), buildReceiptSHA256: evidence.receiptSHA256};
}
function collectFiles(directory, cap = 64 << 20) {
  const files = []; let bytes = 0, count = 0;
  function walk(path) {
    const metadata = lstatSync(path);
    assert(!metadata.isSymbolicLink() && ++count <= 2048, 'formal-bridge.evidence_path_budget');
    if (metadata.isDirectory()) for (const child of readdirSync(path).sort()) walk(join(path, child));
    else {
      assert(metadata.isFile() && metadata.size <= 16 << 20, 'formal-bridge.evidence_file_budget');
      bytes += metadata.size; assert(bytes <= cap, 'formal-bridge.evidence_byte_budget');
      files.push(path);
    }
  }
  walk(directory); return files;
}
export function main(argv = []) {
  const paths = bridgePaths(argv);
  const {runtimePath, candidatePath} = paths;
  assert.equal(process.platform, 'win32'); assert.equal(process.arch, 'x64');
  const support = supportIdentity(), inputs = authenticate(root);
  assert.equal(support[`${BASE}/release-fields.json`], RELEASE_FIELDS_SHA256, 'formal-bridge.release_fields_identity');
  const runtime = verifyInterpreter(runtimePath), candidate = verifyCandidateArtifact(root, candidatePath);
  const test = buildTest(support, inputs, candidate, paths);
  const testIdentity = verifyTestArtifact(test, support, inputs, paths);
  const job = beginScratch(root, 'formal-lua-bridge-recovery', {command: ['node', 'tools/rust-formal-lua-bridge.mjs', ...argv],
    inputs: {oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, interpreterSHA256: runtime.report.executableSHA256,
      interpreterReportSHA256: runtime.reportSHA256, candidateArtifactSHA256: candidate.artifactSHA256,
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
    save(join(job.evidence, 'identity.json'), {runtime, candidate, test, testIdentity, support, inputs});
    const checkIdentity = () => {
      assert.deepEqual(verifyInterpreter(runtimePath), runtime);
      assert.deepEqual(verifyCandidateArtifact(root, candidatePath), candidate);
      assert.deepEqual(verifyTestArtifact(test, support, inputs, paths), testIdentity);
      verifyMaterial(fixture, inputs);
    };
    checkIdentity(); handlesReaped = false;
    const result = ownedSpawnSync(test.sourceBinary, ['--test-threads=1', '--nocapture'], {cwd: root,
      env: cleanLuaEnvironment({...ownedUserEnvironment(job.directory), LYCHEEDEV_FORMAL_LUA_BRIDGE_ADMISSION: admissionPath}),
      evidence: job.evidence, timeout: 255000, cargoBuild: false, msvcCompile: false});
    handlesReaped = processReaped(result) === true;
    checkIdentity();
    assert(processPassed(result), `formal-bridge.process_failed:${result.error?.message ?? result.status}`);
    const counts = verifyTestOutput(result.stdout);
    const lifetimes = readdirSync(caseRoot).sort().map(name => {
      const proof = JSON.parse(regular(join(caseRoot, name, 'peer-lifetime.json')));
      assert.equal(proof.schema, 'lycheedev.formal-lua-peer-lifetime.v1');
      assert.equal(proof.rootReaped, true); assert.equal(proof.rootExit, 0);
      assert.equal(proof.jobEmpty, true); assert.equal(proof.workersJoined, true);
      assert.equal(proof.forced, false); assert.deepEqual(proof.errors, []);
      return {case: name, ...proof};
    });
    assert.deepEqual(lifetimes.map(row => row.case).sort(), ['normal', 'bind', 'prepare', 'commit', 'confirm', 'release', 'reload_bind', 'compile_error', 'cleanup_error'].sort());
    save(join(job.evidence, 'report.json'), {schema: 'lycheedev.formal-lua-bridge-recovery.v1', state: 'passed',
      oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, releaseFieldsSHA256: RELEASE_FIELDS_SHA256,
      previousOracle: inputs.previousOracle, increment: inputs.increment, goTest: 'TestSlotDriverLuaRecoveryWithoutBusinessReplay', counts, lifetimes,
      testBinarySHA256: test.binarySHA256, testIdentity, candidateArtifactSHA256: candidate.artifactSHA256,
      interpreterReportSHA256: runtime.reportSHA256, handlesReaped,
      scope: 'Real formal Rust Driver, codec, DurableChannelJournal and authenticated fixed Go 3.2.3 Lua. Readiness/clock/input transport are explicit fixtures. No real client or the two other deferred bridge tests. Inner handles use existing standard-library/JobGuard Drop; no independently checked inner-handle-close claim.'});
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
      assert(logs.reduce((total, path) => total + lstatSync(path).size, 0) <= 64 << 20, 'formal-bridge.total_evidence_budget');
      job.seal({state: !handlesReaped ? 'interrupted' : succeeded ? 'succeeded' : 'failed', handlesReaped, logs});
      if (succeeded && handlesReaped) job.cleanup();
    } catch (error) { failure ??= error; succeeded = false; }
  }
  if (failure) throw Object.assign(failure, {bridgeEvidence: job.receipt, scratch: job.directory});
  return {passed: succeeded, report: join(job.evidence, 'report.json'), receipt: job.receipt, expectedCases: cases};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { console.log(JSON.stringify(main(process.argv.slice(2)), null, 2)); }
  catch (error) { console.error(JSON.stringify({error: error.message, bridgeEvidence: error.bridgeEvidence, scratch: error.scratch})); process.exitCode = 1; }
}
