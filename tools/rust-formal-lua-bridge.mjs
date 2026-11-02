// Current Mailbox v1 bridge controller: real Rust formal Driver/codec/durable
// journal against the current addon Lua writer and tests/protocol Lua peer. No
// Go execution, fixed oracle, arbitrary fixture argument, game input, release
// publication or process-runner replacement.
import assert from 'node:assert/strict';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readdirSync, writeFileSync} from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beginScratch, contained } from './rust-disk.mjs';
import { ownedUserEnvironment, runCargo, sourceDigest, verifyArtifact } from './rust-build.mjs';
import { ownedSpawnSync } from './rust-owned-process.mjs';
import { cleanLuaEnvironment, hash, regular, verifyCandidateArtifact, verifyInterpreter } from './rust-formal-lua.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const ADMISSION_SCHEMA = 'lycheedev.formal-lua-bridge-admission.v2';
// The peer loads exactly these current product inputs. The admission records
// their exact bytes; the current addon is the product input, never a frozen
// historical candidate.
export const BRIDGE_INPUTS = Object.freeze([
  'addon/Bridge/CaptureWriter.lua',
  'addon/Bridge/MemoryProtocol.lua',
  'addon/Bridge/SlotProtocol.lua',
  'tests/protocol/slot_server.lua',
]);
const TOC_INTERFACES = Object.freeze(['120105', '120100', '50504', '38002', '16001', '11509', '20506']);
const EVENT_CATALOGS = Object.freeze(['Anniversary', 'Era', 'Forever', 'Mainline', 'Mists', 'Titan'].map(name => `Modules\\Events\\CatalogData_${name}.lua`).sort());
export const SUPPORT = Object.freeze(['tools/rust-formal-lua-bridge.mjs', 'tools/rust-formal-lua.mjs', 'tools/rust-build.mjs', 'tools/rust-cargo.mjs',
  'tools/rust-disk.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/lua-tool-supply.mjs', 'tools/tgz.mjs']);
const TEST_SUPPORT = Object.freeze(['crates/lycheedev-live/tests/formal_lua_bridge.rs', 'crates/lycheedev-live/tests/support/formal_lua_fixture.rs',
  'crates/lycheedev-live/tests/support/formal_lua_peer.rs']);
const supportIdentity = () => Object.fromEntries([...SUPPORT, ...TEST_SUPPORT].map(path => [path, hash(regular(join(root, path)))]));
const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });

// Optional paths select sealed records and a fresh output directory, never a
// bare executable. All identity/receipt validation remains mandatory.
export function bridgePaths(argv = [], repository = root) {
  assert(Array.isArray(argv) && (argv.length === 0 || argv.length === 3), 'formal-bridge.path_arguments');
  const values = argv.length ? argv : ['.tmp/ci-formal-lua-runtime/build-report.json', '.tmp/ci-cli-artifact/artifact.json', '.tmp/ci-formal-lua-bridge-test'];
  assert(values.every(value => typeof value === 'string' && value.length > 0 && value.length <= 4096 && !value.startsWith('--') && !value.includes('\0')), 'formal-bridge.path_arguments');
  const [runtimePath, candidatePath, artifactDirectory] = values.map(value => contained(repository, resolve(repository, value)));
  assert.equal(basename(runtimePath), 'build-report.json', 'formal-bridge.interpreter_record_required');
  assert.equal(basename(candidatePath), 'artifact.json', 'formal-bridge.candidate_record_required');
  assert(!existsSync(artifactDirectory), 'formal-bridge.artifact_directory_exists');
  return { runtimePath, candidatePath, artifactDirectory, artifactPath: join(artifactDirectory, 'artifact.json') };
}
const args = Object.freeze(['test', '--locked', '--offline', '-p', 'lycheedev-live', '--test', 'formal_lua_bridge', '--no-run', '--message-format=json-render-diagnostics', '-j', '1']);
const cases = Object.freeze([...['normal', 'after_bind', 'after_prepare', 'after_commit', 'after_confirm', 'after_release', 'reload_bind', 'compile_error', 'cleanup_error'].map(value => `formal_lua_recovery_${value}`),
  'formal_lua_memory_record_parity', 'formal_lua_numeric_report_preserves_finite_numbers']);
const peerCases = Object.freeze(['normal', 'bind', 'prepare', 'commit', 'confirm', 'release', 'reload_bind', 'compile_error', 'cleanup_error', 'memory', 'numeric']);

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
  return { row, executable };
}
export function verifyTestOutput(stdout) {
  const text = stdout.toString('utf8').replaceAll('\r\n', '\n');
  const rows = [...text.matchAll(/^test (.+) \.\.\. ok$/gm)].map(match => match[1]).sort();
  assert.deepEqual(rows, [...cases].sort(), 'formal-bridge.expected_cases');
  assert.equal([...text.matchAll(/^test result: /gm)].length, 1, 'formal-bridge.completion_cardinality');
  assert.match(text, /^test result: ok\. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in /m);
  return { passed: 11, failed: 0, ignored: 0, filtered: 0, cases: rows };
}
export function verifyPeerLifetimes(rows) {
  assert.deepEqual(rows.map(row => row.case).sort(), [...peerCases].sort(), 'formal-bridge.peer_case_set');
  for (const proof of rows) {
    assert.equal(proof.schema, 'lycheedev.formal-lua-peer-lifetime.v1');
    assert.equal(proof.rootReaped, true); assert.equal(proof.rootExit, 0);
    assert.equal(proof.jobEmpty, true); assert.equal(proof.workersJoined, true);
    assert.equal(proof.forced, false); assert.deepEqual(proof.errors, []);
  }
  return rows;
}
export function verifyHostParity(rows) {
  assert.deepEqual(rows.map(row => row.case).sort(), ['memory', 'numeric'], 'formal-bridge.host_case_set');
  return rows.map(({ case: name, evidence, evidenceSHA256, proof }) => {
    assert.equal(proof.schema, 'lycheedev.formal-lua-host-parity.v1'); assert.equal(proof.passed, true);
    assert.equal(proof.case, name);
    if (name === 'memory') {
      assert.equal(proof.payload, 'hello'); assert.equal(proof.sequence, 17);
      assert.equal(proof.nonce, '1'.repeat(32)); assert.equal(proof.runtime, '2'.repeat(32)); assert.equal(proof.ticket, '3'.repeat(32));
      assert.equal(proof.kind, 3); assert.equal(proof.state, 3); assert.match(proof.recordSHA256, /^[a-f0-9]{64}$/);
    } else {
      assert.equal(proof.valueCount, 30); assert.equal(proof.float64Bits.length, 30);
      assert(proof.float64Bits.every(value => /^[a-f0-9]{16}$/.test(value)));
      assert.deepEqual(proof.float64Bits.slice(0, 2), ['0000000000000000', '8000000000000000']);
      for (const key of ['recoveredBytesEqual', 'completedRetryInputCountUnchanged', 'resourcesReleased', 'disconnected']) assert.equal(proof[key], true);
      assert.equal(proof.executions, 1); assert.match(proof.reportSHA256, /^[a-f0-9]{64}$/);
    }
    assert.equal(evidenceSHA256, hash(regular(evidence)));
    return { testCase: name === 'memory' ? cases[9] : cases[10], evidence, evidenceSHA256, proof };
  });
}
// One manifest for every supported client. The current addon TOC contract and
// the exact peer inputs are the admitted fixture, with no frozen candidate.
export function verifyBridgeInputs(repository = root) {
  const addon = join(repository, 'addon');
  const toc = regular(join(addon, 'Lychee Dev.toc'), 1 << 20).toString('utf8').replaceAll('\r\n', '\n');
  const lines = toc.split('\n').map(line => line.trimEnd());
  const loads = lines.filter(line => line && !line.startsWith('#'));
  assert(loads.length > 0, 'formal-bridge.toc_empty');
  assert.match(loads[0], /^Core[\\/]ClientGate\.lua$/, 'formal-bridge.toc_first_load');
  const interfaceLine = lines.find(line => line.startsWith('## Interface:'));
  assert(interfaceLine, 'formal-bridge.toc_interface_missing');
  const declared = new Set(interfaceLine.slice('## Interface:'.length).split(',').map(value => value.trim()).filter(Boolean));
  for (const value of TOC_INTERFACES) assert(declared.has(value), `formal-bridge.toc_interface_missing: ${value}`);
  for (const load of loads) assert(existsSync(join(addon, load.replaceAll('\\', '/'))), `formal-bridge.toc_load_missing: ${load}`);
  const catalogs = loads.filter(load => /^Modules[\\/]Events[\\/]CatalogData_/.test(load)).sort();
  assert.deepEqual(catalogs, [...EVENT_CATALOGS], 'formal-bridge.toc_event_catalogs');
  return BRIDGE_INPUTS.map(path => {
    const bytes = regular(join(repository, path), 2 << 20);
    assert.equal(bytes.length, lstatSync(join(repository, path)).size);
    return { path, bytes: bytes.length, sha256: hash(bytes) };
  });
}
function buildTest(support, inputs, candidate, { artifactDirectory, artifactPath }) {
  assert(!existsSync(artifactDirectory), 'formal-bridge.artifact_directory_exists');
  return runCargo(root, [...args], { owner: 'formal-lua-bridge-build', oneShot: false, isolateUser: true,
    onSuccess(invocation) {
      assert.deepEqual(supportIdentity(), support, 'formal-bridge.build_support_changed');
      assert.equal(invocation.inputHashes.sourceTree, candidate.sourceTree, 'formal-bridge.candidate_source_mismatch');
      const { row, executable } = selectTestExecutable(invocation.stdout, invocation.target);
      const bytes = regular(contained(root, executable), 128 << 20);
      mkdirSync(artifactDirectory);
      const binary = join(artifactDirectory, 'formal_lua_bridge.exe');
      copyFileSync(executable, binary);
      assert.equal(hash(regular(binary, 128 << 20)), hash(bytes));
      const artifact = { schema: 'lycheedev.rust-build-artifact.v1', profile: 'test', sourceRoot: root,
        sourceBinary: binary, binarySHA256: hash(bytes), bytes: bytes.length,
        inputHashes: invocation.inputHashes, environmentSHA256: invocation.environmentSHA256,
        channel: invocation.target, sameInvocation: true, command: ['cargo', ...args], buildEvidence: invocation.buildEvidence,
        cargoArtifact: row, support };
      save(artifactPath, artifact);
      return artifact;
    },
  });
}
function verifyTestArtifact(expected, support, candidate, { artifactPath }) {
  const artifact = verifyArtifact(root, artifactPath);
  assert.deepEqual(artifact, expected, 'formal-bridge.test_artifact_changed');
  assert.equal(artifact.schema, 'lycheedev.rust-build-artifact.v1');
  assert.equal(artifact.sameInvocation, true); assert.equal(artifact.sourceRoot, root);
  assert.deepEqual(artifact.command, ['cargo', ...args]);
  assert.equal(artifact.inputHashes.sourceTree, sourceDigest(root), 'formal-bridge.source_changed');
  assert.equal(artifact.inputHashes.sourceTree, candidate.sourceTree, 'formal-bridge.candidate_source_mismatch');
  assert.deepEqual(supportIdentity(), support);
  const receipt = regular(contained(root, artifact.buildEvidence), 1 << 20);
  return { artifactSHA256: hash(regular(artifactPath)), buildReceiptSHA256: hash(receipt) };
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
// CI supplies an already validated test-program record. Recheck its bytes and
// selected executable around the run; this route never invokes Cargo or builds.
export function runPrebuiltBridge({runtimePath, recordPath, record, program}) {
  assert.equal(record.schema, 'lycheedev.ci-test-programs.v1');
  assert.equal(record.root, root); assert.equal(record.sourceTree, sourceDigest(root));
  const selected = record.programs.filter(row => row.target === 'formal_lua_bridge' && row.kinds.includes('test'));
  assert.equal(selected.length, 1, 'formal-bridge.prebuilt_cardinality');
  assert.deepEqual(program, selected[0]);
  const recordBytes = regular(contained(root, recordPath), 16 << 20);
  assert.deepEqual(JSON.parse(recordBytes), record);
  const executable = contained(root, program.path);
  const test = {sourceBinary: executable, binarySHA256: program.sha256, inputHashes: {sourceTree: record.sourceTree}};
  const testIdentity = {record: recordPath, recordSHA256: hash(recordBytes), program};
  const checkTest = () => {
    assert.equal(hash(regular(recordPath, 16 << 20)), testIdentity.recordSHA256, 'formal-bridge.test_record_changed');
    const bytes = regular(executable, 512 << 20);
    assert.equal(bytes.length, program.bytes); assert.equal(hash(bytes), program.sha256, 'formal-bridge.test_changed');
    assert.equal(sourceDigest(root), record.sourceTree, 'formal-bridge.source_changed');
  };
  checkTest();
  return executeBridge({runtimePath, test, testIdentity, checkTest, command: ['node', 'tools/ci-test-programs.mjs', 'run', recordPath]});
}
// The Rust fixture consumes resolved paths in its current execution admission.
// This projection is not a new production report: the original import record,
// producer report and receipt remain unchanged and are checked around the run.
export function admitInterpreter(runtimePath, runtime, directory) {
  if (!runtime.supplyManifestSHA256) return { report: runtimePath, reportSHA256: runtime.reportSHA256, buildReceiptSHA256: runtime.buildReceiptSHA256 };
  const path = join(directory, 'interpreter-admission.json');
  const projection = { ...runtime.report, admissionProjection: {
    importRecord: runtimePath, importRecordSHA256: runtime.reportSHA256,
    producerReportSHA256: runtime.producerReportSHA256, supplyManifestSHA256: runtime.supplyManifestSHA256,
  } };
  save(path, projection);
  return { report: path, reportSHA256: hash(regular(path)), buildReceiptSHA256: runtime.buildReceiptSHA256 };
}

export function main(argv = []) {
  const paths = bridgePaths(argv), {runtimePath, candidatePath} = paths;
  assert.equal(process.platform, 'win32'); assert.equal(process.arch, 'x64');
  const support = supportIdentity(), candidate = verifyCandidateArtifact(root, candidatePath);
  const test = buildTest(support, verifyBridgeInputs(root), candidate, paths);
  const testIdentity = verifyTestArtifact(test, support, candidate, paths);
  const checkTest = () => assert.deepEqual(verifyTestArtifact(test, support, candidate, paths), testIdentity, 'formal-bridge.test_changed');
  return executeBridge({runtimePath, test, testIdentity, checkTest, candidate, command: ['node', 'tools/rust-formal-lua-bridge.mjs', ...argv]});
}
function executeBridge({runtimePath, test, testIdentity, checkTest, candidate = null, command}) {
  assert.equal(process.platform, 'win32'); assert.equal(process.arch, 'x64');
  const support = supportIdentity(), runtime = verifyInterpreter(runtimePath);
  const job = beginScratch(root, 'formal-lua-bridge-recovery', { command,
    inputs: { interpreterSHA256: runtime.report.executableSHA256, interpreterReportSHA256: runtime.reportSHA256,
      candidateArtifactSHA256: candidate?.artifactSHA256 ?? null, testArtifact: testIdentity, testBinarySHA256: test.binarySHA256, support },
    admissionOptions: { requiredBytes: 128 << 20, scratchReserveBytes: 64 << 20 } });
  const caseRoot = join(job.directory, 'cases');
  const admissionPath = join(job.directory, 'admission.json');
  let handlesReaped = true, succeeded = false, failure;
  try {
    mkdirSync(caseRoot);
    const fixture = { root, files: verifyBridgeInputs(root) };
    const interpreterAdmission = admitInterpreter(runtimePath, runtime, job.evidence);
    save(admissionPath, { schema: ADMISSION_SCHEMA, interpreter: interpreterAdmission,
      fixture, scratch: { receipt: job.receipt, cases: caseRoot } });
    save(join(job.evidence, 'identity.json'), { runtime, candidate, test, testIdentity, support, fixture });
    const checkIdentity = () => {
      assert.deepEqual(verifyInterpreter(runtimePath), runtime, 'formal-bridge.interpreter_changed');
      checkTest();
      assert.deepEqual(verifyBridgeInputs(root), fixture.files, 'formal-bridge.inputs_changed');
    };
    checkIdentity(); handlesReaped = false;
    const result = ownedSpawnSync(test.sourceBinary, ['--test-threads=1', '--nocapture'], { cwd: root,
      env: cleanLuaEnvironment({ ...ownedUserEnvironment(job.directory), LYCHEEDEV_FORMAL_LUA_BRIDGE_ADMISSION: admissionPath }),
      evidence: job.evidence, timeout: 255000, cargoBuild: false, msvcCompile: false });
    handlesReaped = result.handlesReaped === true;
    checkIdentity();
    assert(!result.error && result.status === 0, `formal-bridge.process_failed:${result.error?.message ?? result.status}`);
    const counts = verifyTestOutput(result.stdout);
    const lifetimes = verifyPeerLifetimes(readdirSync(caseRoot).sort().map(name => {
      const proof = JSON.parse(regular(join(caseRoot, name, 'peer-lifetime.json')));
      return { case: name, ...proof };
    }));
    const hostParity = verifyHostParity(['memory', 'numeric'].map(name => {
      const evidence = join(caseRoot, name, 'host-parity.json'), bytes = regular(evidence);
      return { case: name, evidence, evidenceSHA256: hash(bytes), proof: JSON.parse(bytes) };
    }));
    save(join(job.evidence, 'report.json'), { schema: 'lycheedev.formal-lua-bridge-recovery.v2', state: 'passed',
      interpreterSHA256: runtime.report.executableSHA256, interpreterReportSHA256: runtime.reportSHA256,
      candidateArtifactSHA256: candidate?.artifactSHA256 ?? null, testBinarySHA256: test.binarySHA256, testIdentity, handlesReaped, counts, lifetimes, hostParity,
      fixtures: fixture.files,
      scope: 'Real formal Rust Driver, codec and DurableChannelJournal against the current addon Lua writer and tests/protocol peer: nine recovery cases, exact memory record encoding/decoding and thirty finite Float64bits with durable recovery and no replay. Readiness/clock/input transport are explicit fixtures. No frozen oracle, no real client. Inner handles use existing standard-library/JobGuard Drop; no independently checked inner-handle-close claim.' });
    succeeded = true;
  } catch (error) {
    failure = error;
    save(join(job.evidence, 'failure.json'), { error: error.message, handlesReaped, scratchRetained: true });
  } finally {
    // Seal journal/result/record/pipe evidence before any successful cleanup.
    // Failures and uncertain cleanup always retain their original scratch.
    let logs;
    try {
      logs = [...collectFiles(job.evidence).filter(path => basename(path) !== 'receipt.json'),
        ...(existsSync(admissionPath) ? [admissionPath] : []), ...(existsSync(caseRoot) ? collectFiles(caseRoot) : [])];
      assert(logs.reduce((total, path) => total + lstatSync(path).size, 0) <= 64 << 20, 'formal-bridge.total_evidence_budget');
      job.seal({ state: !handlesReaped ? 'interrupted' : succeeded ? 'succeeded' : 'failed', handlesReaped, logs });
      if (succeeded && handlesReaped) job.cleanup();
    } catch (error) { failure ??= error; succeeded = false; }
  }
  if (failure) throw Object.assign(failure, { bridgeEvidence: job.receipt, scratch: job.directory, handlesReaped });
  return { passed: succeeded, report: join(job.evidence, 'report.json'), receipt: job.receipt, expectedCases: cases };
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { console.log(JSON.stringify(main(process.argv.slice(2)), null, 2)); }
  catch (error) { console.error(JSON.stringify({ error: error.message, bridgeEvidence: error.bridgeEvidence, scratch: error.scratch })); process.exitCode = 1; }
}
