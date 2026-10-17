// Opt-in benchmark-only Windows controller. No product CLI, Go, game, network,
// production digest replacement, new FFI, or performance-acceptance claim.
import assert from 'node:assert/strict';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readdirSync, writeFileSync} from 'node:fs';
import {basename, dirname, isAbsolute, join, relative, resolve, sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import {performance} from 'node:perf_hooks';
import {beginScratch, contained} from './rust-disk.mjs';
import {hashFile, runCargo, sourceDigest, verifyArtifact} from './rust-build.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {bounded, decodeFixtureBLTE, digest, loadDataFixture, parse} from '../tests/rust-performance-paired/data-inputs.mjs';
import {elapsed, startupBreakdown} from '../tests/rust-performance-paired/contract.mjs';
import {processPassed, processReaped, regular, verifyOwnerEvidence} from '../tests/rust-parity/formal-lua-3.2.2/assertions.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const SCHEMA = 'lycheedev.md5-stream-benchmark.v1';
export const INPUTS = Object.freeze({
  root: Object.freeze({bytes: 44, updates: 1, sha256: '554c8419b746697073fe5d0e956e380b98e7b439e925ca28ec8001d457a41d44', md5: '8cea531c64ce544535550008b29e781c'}),
  payload: Object.freeze({bytes: 17825792, updates: 272, sha256: '397fa93fdb906d5a278075e087c5b1bef5c5f5fbfb3a080b438a3ba30f435db3', md5: 'c9fa02f009aa62f0b0504a86cc983e2b'}),
});
// This controller admits only the existing fixed content fixture's three
// archive records. Bound and authenticate encoded input before its decoder runs.
export const ENCODED_FIXTURE = Object.freeze({
  index: Object.freeze({path: 'game/Data/data/0000000001.idx', bytes: 94, sha256: '315bdd6da2f530e2c094c588be86ccde64723a1946f9a11bd9e7d3851afee800'}),
  archive: Object.freeze({path: 'game/Data/data/data.000', bytes: 174716, sha256: 'cf6aae3657915c3ff64a06d01d218aa5841c1633e05a8ef438ef3c09457f4074'}),
});
export const MODES = Object.freeze(['md5_clone', 'md5_consume', 'sha256_md5_consume']);
export const BUILD_ARGS = Object.freeze(['build', '--release', '--locked', '--offline', '--target', 'x86_64-pc-windows-msvc', '-p', 'lycheedev-windows', '--example', 'digest_benchmark', '--message-format=json-render-diagnostics', '-j', '1']);
export const SUPPORT = Object.freeze([
  'tools/rust-md5-benchmark.mjs', 'tools/rust-build.mjs', 'tools/rust-disk.mjs', 'tools/rust-owned-process.mjs',
  'tools/rust-owned-process.ps1', 'tools/rust-build-auxiliary.json', 'tools/rust-emmy-identity.mjs', 'tools/zip.mjs', 'release/rust-disk-policy.json',
  'tests/rust-performance-paired/data-inputs.mjs', 'tests/rust-performance-paired/data-contract.mjs', 'tests/rust-performance-paired/inputs.mjs',
  'tests/rust-performance-paired/contract.mjs', 'tests/rust-source-full-corpus/assertions.mjs', 'tests/rust-parity/describe-contract.mjs',
  'tests/rust-parity/oracle.mjs', 'tests/rust-parity/formal-lua-3.2.2/assertions.mjs', 'tests/rust-parity/rust-3.5.0-release-fields.mjs',
]);
const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n', {flag: 'wx'});
const supportIdentity = () => Object.fromEntries(SUPPORT.map(path => [path, hashFile(regularPath(join(root, path)), 16 << 20)]));
function regularPath(path, maximum = 16 << 20) { regular(path, maximum); return path; }
function keys(value, expected, label) { assert(value && typeof value === 'object' && !Array.isArray(value), label); assert.deepEqual(Object.keys(value).sort(), [...expected].sort(), label); }
function uint(value, label) { assert(Number.isSafeInteger(value) && value >= 0, label); }
export const orderFor = iteration => iteration % 2 ? [...MODES].reverse() : [...MODES];
// Preserve the checked repository release profile/configuration. A raw env
// digest records provenance but cannot prove that inherited flags optimized it.
export function verifyBuildEnvironment(environment) {
  for (const [name, value] of Object.entries(environment)) if (value !== undefined && value !== '')
    assert(!/^(?:RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|CARGO_BUILD_(?:RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|RUSTFLAGS|TARGET)|CARGO_TARGET_.*_(?:RUSTFLAGS|RUNNER|LINKER)|CARGO_PROFILE_RELEASE_.*)$/i.test(name), `md5-benchmark.uncontrolled_build_environment:${name}`);
}

export function benchmarkPaths(argv, repository = root) {
  assert(Array.isArray(argv) && argv.length === 2, 'md5-benchmark.expected_content_fixture_and_fresh_artifacts');
  assert(argv.every(value => typeof value === 'string' && value.length > 0 && value.length <= 4096 && !value.startsWith('--') && !value.includes('\0')), 'md5-benchmark.path_arguments');
  const [fixtureDirectory, artifactDirectory] = argv.map(value => contained(repository, resolve(repository, value)));
  assert(!existsSync(artifactDirectory), 'md5-benchmark.artifact_directory_exists');
  assert(!artifactDirectory.startsWith(fixtureDirectory + sep), 'md5-benchmark.artifacts_inside_fixture');
  return {fixtureDirectory, artifactDirectory, artifactPath: join(artifactDirectory, 'artifact.json')};
}

export function preflightBenchmarkFixture(directory) {
  for (const [name, expected] of Object.entries(ENCODED_FIXTURE)) {
    const bytes = bounded(join(directory, expected.path), expected.bytes);
    assert.equal(bytes.length, expected.bytes, `md5-benchmark.fixed_${name}_bytes`);
    assert.equal(digest(bytes), expected.sha256, `md5-benchmark.fixed_${name}_sha256`);
  }
}
// The optional loader exists only for negative fault tests; runtime uses the
// original read-only authenticator. Every call is preflighted before decoding.
export function loadBenchmarkFixture(directory, load = loadDataFixture) {
  preflightBenchmarkFixture(directory);
  const fixture = load(directory, 'content');
  preflightBenchmarkFixture(directory);
  return fixture;
}

// Called only after loadDataFixture(directory, 'content') authenticates all
// inventory/SQLite/BLTE/content identities. Reuse its bounded frame decoder.
export function extractFixtureRoot(fixture) {
  const expected = INPUTS.root, object = fixture.objects[expected.sha256];
  assert(object && object.decodedBytes === expected.bytes && object.ckey === expected.md5, 'md5-benchmark.authenticated_root_required');
  const content = fixture.identity.content;
  assert.equal(content.sha256, INPUTS.payload.sha256); assert.equal(content.contentKey, INPUTS.payload.md5);
  assert.equal(content.decodedBytes, INPUTS.payload.bytes); assert.equal(content.fillByte, 120);
  const archive = bounded(join(fixture.directory, 'game/Data/data/data.000'));
  const index = bounded(join(fixture.directory, 'game/Data/data/0000000001.idx'), 1 << 20);
  assert(index.length >= 40 && (index.length - 40) % 18 === 0 && (index.length - 40) / 18 <= 256, 'md5-benchmark.index_shape');
  const found = [];
  for (let at = 40; at < index.length; at += 18) {
    assert.equal(index[at + 9], 0);
    const offset = index.readUInt32BE(at + 10), bytes = index.readUInt32LE(at + 14);
    assert(bytes >= 39 && offset + bytes <= archive.length, 'md5-benchmark.archive_range');
    const decoded = decodeFixtureBLTE(archive.subarray(offset + 30, offset + bytes));
    assert.equal(index.subarray(at, at + 9).toString('hex'), decoded.ekey.slice(0, 18));
    if (digest(decoded.raw) !== expected.sha256) continue;
    assert.equal(decoded.raw.length, expected.bytes); assert.equal(digest(decoded.raw, 'md5'), expected.md5);
    assert.equal(decoded.ekey, object.ekey); assert.equal(bytes - 30, object.encodedBytes);
    found.push(Buffer.from(decoded.raw));
  }
  assert.equal(found.length, 1, 'md5-benchmark.exact_root_cardinality');
  return found[0];
}

export function selectBenchmarkExecutable(stdout, target, repository = root) {
  assert(typeof stdout === 'string' && Buffer.byteLength(stdout) <= 8 << 20, 'md5-benchmark.cargo_output_budget');
  const lines = stdout.split(/\r?\n/); if (lines.at(-1) === '') lines.pop();
  assert(lines.length <= 65536 && lines.every(line => line.length > 0), 'md5-benchmark.cargo_message_budget');
  const messages = lines.map(line => parse(Buffer.from(line)));
  const found = messages.filter(row => row.reason === 'compiler-artifact' && row.target?.name === 'digest_benchmark' && row.target?.kind?.includes('example') && row.executable);
  assert.equal(found.length, 1, 'md5-benchmark.example_executable_cardinality');
  const row = found[0];
  assert.deepEqual(row.target.kind, ['example']); assert.deepEqual(row.target.crate_types, ['bin']);
  assert.equal(resolve(row.target.src_path), join(resolve(repository), 'crates/lycheedev-windows/examples/digest_benchmark.rs'));
  assert.equal(resolve(row.manifest_path), join(resolve(repository), 'crates/lycheedev-windows/Cargo.toml'));
  assert.equal(row.profile?.test, false); assert.equal(row.profile?.opt_level, '3'); assert.equal(row.profile?.debug_assertions, false);
  assert(isAbsolute(row.executable), 'md5-benchmark.absolute_artifact_required');
  const executable = resolve(row.executable), within = relative(resolve(target), executable);
  assert(within && !isAbsolute(within) && within !== '..' && !within.startsWith('../') && !within.startsWith('..\\'), 'md5-benchmark.executable_channel');
  assert.match(basename(executable), /^digest_benchmark(?:-[a-f0-9]+)?\.exe$/);
  const finished = messages.filter(row => row.reason === 'build-finished');
  assert.equal(finished.length, 1, 'md5-benchmark.build_completion_cardinality'); assert.equal(finished[0].success, true);
  return {row, executable};
}

function jsonLines(stdout, expected, {canonical = false, flatUniqueKeys = false} = {}) {
  assert(Buffer.isBuffer(stdout) && stdout.length <= 1 << 20, 'md5-benchmark.stdout_budget');
  const text = stdout.toString('utf8'); assert(Buffer.from(text).equals(stdout), 'md5-benchmark.stdout_utf8');
  const lines = text.split(/\r?\n/); if (lines.at(-1) === '') lines.pop();
  assert.equal(lines.length, expected, 'md5-benchmark.stdout_row_count');
  assert(lines.every(line => line.length > 0), 'md5-benchmark.empty_stdout_row');
  return lines.map(line => {
    const value = parse(Buffer.from(line));
    // The stream writer emits compact JSON containing integer timings only.
    // Round-tripping rejects duplicate members and ambiguous numeric forms.
    if (canonical) assert.equal(JSON.stringify(value), line, 'md5-benchmark.noncanonical_stream_json');
    // Legacy output deliberately retains fixed-decimal float formatting. Its
    // flat ASCII field names can be checked without changing those raw bytes.
    if (flatUniqueKeys) {
      const names = [...line.matchAll(/"([A-Za-z][A-Za-z0-9]*)"\s*:/g)].map(match => match[1]);
      assert.equal(names.length, Object.keys(value).length, 'md5-benchmark.legacy_duplicate_or_escaped_key');
      assert.equal(new Set(names).size, names.length, 'md5-benchmark.legacy_duplicate_key');
    }
    return value;
  });
}
export function verifyLegacyOutput(stdout) {
  const rows = jsonLines(stdout, 3, {flatUniqueKeys: true});
  rows.forEach((row, index) => {
    keys(row, ['mib', 'rustCryptoMillis', 'cngMillis', 'sameDigest', 'ratio'], 'md5-benchmark.legacy_fields');
    assert.equal(row.mib, [1, 16, 64][index]); assert.equal(row.sameDigest, true);
    for (const name of ['rustCryptoMillis', 'cngMillis', 'ratio']) assert(Number.isFinite(row[name]) && row[name] >= 0, 'md5-benchmark.legacy_timing');
  });
  return {rows, scope: 'Legacy whole-slice RustCrypto/CNG MD5 only; unchanged no-argument output and checks. CNG is not streaming.'};
}
export function verifyStreamOutput(stdout) {
  const rows = jsonLines(stdout, 38, {canonical: true}), contract = rows[0], samples = rows.slice(1, -1), complete = rows.at(-1);
  assert.deepEqual(contract, {schema: SCHEMA, kind: 'contract', pairs: 6, chunkBytes: 65536, messages: 2, updatesPerPair: 273,
    inputConstructionAndAuthenticationTimed: false, correctness: {rfcVectors: 7, binaryBoundaryCases: 8, splitPolicies: 4, cancelCases: 10, deadlineCases: 10, cngInitialStopCases: 2}, cngStreaming: false});
  let cursor = 0;
  for (let iteration = 0; iteration < 6; iteration++) for (const mode of orderFor(iteration)) for (const message of ['root', 'payload']) {
    const row = samples[cursor++], input = INPUTS[message];
    keys(row, ['schema', 'kind', 'iteration', 'mode', 'message', 'bytes', 'updates', 'md5', 'sha256', 'initNanos', 'updateNanos', 'cloneNanos', 'finalizeNanos', 'dropNanos', 'totalNanos', 'unattributedNanos'], 'md5-benchmark.sample_fields');
    assert.equal(row.schema, SCHEMA); assert.equal(row.kind, 'sample'); assert.equal(row.iteration, iteration);
    assert.equal(row.mode, mode); assert.equal(row.message, message); assert.equal(row.bytes, input.bytes); assert.equal(row.updates, input.updates);
    assert.equal(row.md5, input.md5); assert.equal(row.sha256, mode === 'sha256_md5_consume' ? input.sha256 : null);
    const phases = ['initNanos', 'updateNanos', 'cloneNanos', 'finalizeNanos', 'dropNanos', 'unattributedNanos'];
    for (const name of [...phases, 'totalNanos']) uint(row[name], name);
    assert.equal(phases.reduce((sum, name) => sum + BigInt(row[name]), 0n), BigInt(row.totalNanos), 'md5-benchmark.nonoverlapping_time_partition');
    if (mode !== 'md5_clone') assert.equal(row.cloneNanos, 0, 'md5-benchmark.consume_has_no_clone');
  }
  assert.deepEqual(complete, {schema: SCHEMA, kind: 'complete', samples: 36, correctnessPassed: true});
  return {contract, samples, complete, scope: 'All 36 unfiltered Instant wall samples. Root and payload are separate messages. No per-sample CPU, I/O, real-client or paired-Go performance claim.'};
}

function sourceIdentity() {
  return {sourceTree: sourceDigest(root), cargoLockSHA256: hashFile(join(root, 'Cargo.lock')), workspaceManifestSHA256: hashFile(join(root, 'Cargo.toml')),
    exampleSHA256: hashFile(join(root, 'crates/lycheedev-windows/examples/digest_benchmark.rs')), cargoConfigurationSHA256: hashFile(join(root, '.cargo/config.toml')), support: supportIdentity()};
}
function buildBenchmark(identity, toolchain, paths) {
  let buildEvidence;
  try { return runCargo(root, [...BUILD_ARGS], {owner: 'md5-benchmark-build', oneShot: true, onSuccess(invocation) {
    buildEvidence = invocation.buildEvidence;
    assert.deepEqual(sourceIdentity(), identity, 'md5-benchmark.source_changed_during_build');
    assert.equal(invocation.inputHashes.sourceTree, identity.sourceTree);
    const {row, executable} = selectBenchmarkExecutable(invocation.stdout, invocation.target);
    const bytes = regular(contained(root, executable), 128 << 20), binarySHA256 = digest(bytes);
    assert(!existsSync(paths.artifactDirectory), 'md5-benchmark.artifact_directory_exists'); mkdirSync(paths.artifactDirectory);
    const sourceBinary = join(paths.artifactDirectory, 'digest_benchmark.exe'); copyFileSync(executable, sourceBinary);
    assert.equal(hashFile(sourceBinary, 128 << 20), binarySHA256); assert.equal(hashFile(executable, 128 << 20), binarySHA256);
    const artifact = {schema: 'lycheedev.rust-build-artifact.v1', profile: 'release-example', sourceRoot: root, sourceBinary, binarySHA256, bytes: bytes.length,
      inputHashes: invocation.inputHashes, environmentSHA256: invocation.environmentSHA256, channel: invocation.target, sameInvocation: true,
      command: ['cargo', ...BUILD_ARGS], buildEvidence: invocation.buildEvidence, cargoArtifact: row, source: identity, toolchain};
    save(paths.artifactPath, artifact); return artifact;
  }}); } catch (error) { if (buildEvidence) error.buildEvidence ??= buildEvidence; throw error; }
}
function verifyBenchmarkArtifact(expected, identity, paths) {
  const artifact = verifyArtifact(root, paths.artifactPath); assert.deepEqual(artifact, expected, 'md5-benchmark.artifact_changed');
  assert.deepEqual(sourceIdentity(), identity, 'md5-benchmark.source_changed');
  const evidence = verifyOwnerEvidence(root, artifact.buildEvidence, 'md5-benchmark-build'), command = evidence.json('command.json');
  assert.equal(command.command, 'cargo'); assert.deepEqual(command.args, BUILD_ARGS); assert.equal(command.cwd, root);
  assert.equal(command.exit, 0); assert(!command.error && !command.signal); assert.equal(command.oneShot, true);
  assert.deepEqual(command.inputHashes, artifact.inputHashes); assert.equal(command.environmentSHA256, artifact.environmentSHA256);
  assert.equal(evidence.processes.length, 1); const {request, proof} = evidence.processes[0];
  assert.equal(request.command, 'cargo'); assert.deepEqual(request.args, BUILD_ARGS); assert.equal(request.cwd, root); assert.equal(request.cargoBuild, true);
  const selected = selectBenchmarkExecutable(evidence.logs.get(join(dirname(artifact.buildEvidence), 'stdout.bin')).toString('utf8'), artifact.channel);
  assert.deepEqual(selected.row, artifact.cargoArtifact);
  assert(isAbsolute(proof.binary)); const invokedCargoSHA256 = hashFile(regularPath(proof.binary, 128 << 20), 128 << 20);
  if (proof.helperAdmission?.cargo) assert.equal(proof.helperAdmission.cargo.sha256, invokedCargoSHA256);
  return {artifactSHA256: hashFile(paths.artifactPath), buildReceiptSHA256: evidence.receiptSHA256,
    invokedCargo: {path: proof.binary, sha256: invokedCargoSHA256, scope: 'Observed Cargo process binary; may be a rustup shim'}, profile: artifact.cargoArtifact.profile};
}
function collectLogs(directory) {
  const paths = []; let bytes = 0;
  function visit(path) {
    const info = lstatSync(path); assert(!info.isSymbolicLink(), 'md5-benchmark.evidence_reparse');
    if (info.isDirectory()) for (const name of readdirSync(path).sort()) visit(join(path, name));
    else { assert(info.isFile() && info.size <= 16 << 20 && paths.length < 256 && (bytes += info.size) <= 64 << 20, 'md5-benchmark.evidence_budget'); if (basename(path) !== 'receipt.json') paths.push(path); }
  }
  visit(directory); return paths;
}

export function main(argv) {
  const controllerStarted = performance.now(), paths = benchmarkPaths(argv);
  assert.equal(process.platform, 'win32', 'md5-benchmark.windows_required'); assert.equal(process.arch, 'x64');
  const job = beginScratch(root, 'md5-benchmark', {command: ['node', 'tools/rust-md5-benchmark.mjs', ...argv],
    admissionOptions: {requiredBytes: 128 << 20, scratchReserveBytes: 64 << 20}});
  let handlesReaped = true, succeeded = false, failure;
  const report = {schema: 'lycheedev.md5-benchmark-controller.v1', state: 'running', performanceAcceptanceEvaluated: false, paths, preparation: [], invocations: [],
    metrics: {microbenchmark: 'Rust Instant wall nanoseconds only', cpu: 'GetProcessTimes on original benchmark invocation root; no per-sample, descendant or owner CPU', io: 'not_measured'},
    scope: 'Two benchmark invocations: unchanged legacy whole-slice CNG comparison and fixed authenticated Root/17 MiB Rust stream comparison. Rust-only diagnostic; no CLI release rebuild or production change.'};
  function invoke(label, command, args, startupDiagnostics) {
    assert(handlesReaped, 'md5-benchmark.unreaped_previous_process');
    handlesReaped = false; const started = performance.now();
    const run = ownedSpawnSync(command, args, {cwd: root, env: {...process.env}, evidence: job.evidence, timeout: 180000,
      cargoBuild: false, msvcCompile: false, startupDiagnostics});
    handlesReaped = processReaped(run) === true;
    const row = {label, command, args, wrapperWallMillis: performance.now() - started, exit: run.status, signal: run.signal,
      error: run.error?.message ?? null, handlesReaped, proof: run.proof ?? null, logs: run.logs ?? []};
    (startupDiagnostics ? report.invocations : report.preparation).push(row);
    assert(processPassed(run), `md5-benchmark.process_failed:${label}:${run.error?.message ?? run.status}`);
    assert.equal(run.stderr.length, 0, `md5-benchmark.unexpected_stderr:${label}`);
    if (startupDiagnostics) {
      assert.equal(resolve(run.proof.binary).toLowerCase(), resolve(command).toLowerCase(), 'md5-benchmark.invoked_binary_path');
      row.observedInvokedBinary = {path: run.proof.binary, sha256: hashFile(regularPath(run.proof.binary, 128 << 20), 128 << 20)};
      row.nativeWall = elapsed(run.proof); row.startup = startupBreakdown(run.proof, {required: true});
    }
    return {row, run};
  }
  function toolchain(label) {
    const {run, row} = invoke(label, 'rustc', ['-Vv'], false), text = run.stdout.toString('utf8');
    assert(text.length < 16384 && /^rustc \S+/m.test(text) && /^commit-hash: [0-9a-f]{40}$/m.test(text) && /^host: x86_64-pc-windows-msvc$/m.test(text), 'md5-benchmark.toolchain_metadata');
    assert(isAbsolute(run.proof.binary));
    const result = {versionVerbose: text, observedInvokedBinary: run.proof.binary, invokedBinarySHA256: hashFile(regularPath(run.proof.binary, 128 << 20), 128 << 20),
      scope: 'Owned rustc -Vv output and observed invoked binary; binary may be rustup shim, not a claim to hash every compiler library'};
    row.toolchain = result; return result;
  }
  try {
    verifyBuildEnvironment(process.env);
    const identity = sourceIdentity(), fixture = loadBenchmarkFixture(paths.fixtureDirectory), rootBytes = extractFixtureRoot(fixture);
    report.sourceBefore = identity; report.fixture = fixture.receipt; report.encodedFixture = ENCODED_FIXTURE; report.inputs = INPUTS;
    const rootFile = join(job.directory, 'authenticated-root.bin'); writeFileSync(rootFile, rootBytes, {flag: 'wx'});
    writeFileSync(join(job.evidence, 'authenticated-root.bin'), rootBytes, {flag: 'wx'});
    const beforeToolchain = toolchain('rustc-before-build'); report.toolchainBefore = beforeToolchain;
    const buildStarted = performance.now();
    const artifact = buildBenchmark(identity, beforeToolchain, paths); report.buildWallMillis = performance.now() - buildStarted; report.artifact = artifact;
    const artifactBefore = verifyBenchmarkArtifact(artifact, identity, paths); report.artifactBefore = artifactBefore;
    const checkInputs = () => {
      assert.deepEqual(loadBenchmarkFixture(paths.fixtureDirectory).receipt, fixture.receipt, 'md5-benchmark.fixture_changed');
      assert.equal(hashFile(rootFile), INPUTS.root.sha256); assert.equal(lstatSync(rootFile).size, INPUTS.root.bytes);
      assert.deepEqual(verifyBenchmarkArtifact(artifact, identity, paths), artifactBefore, 'md5-benchmark.binary_or_build_provenance_changed');
    };
    checkInputs();
    const legacy = invoke('legacy-whole-slice', artifact.sourceBinary, [], true);
    legacy.row.measurement = verifyLegacyOutput(legacy.run.stdout); checkInputs();
    const stream = invoke('fixture-stream', artifact.sourceBinary, ['--fixture-stream', rootFile], true);
    stream.row.measurement = verifyStreamOutput(stream.run.stdout); checkInputs();
    const afterToolchain = toolchain('rustc-after-benchmarks'); report.toolchainAfter = afterToolchain;
    assert.deepEqual(afterToolchain, beforeToolchain, 'md5-benchmark.toolchain_changed'); checkInputs();
    report.sourceAfter = sourceIdentity(); report.artifactAfter = verifyBenchmarkArtifact(artifact, identity, paths);
    report.state = 'observed_diagnostic_only'; succeeded = true;
  } catch (error) {
    failure = error; report.state = 'failed';
    if (error.buildEvidence) {
      try {
        const receipt = parse(regular(contained(root, error.buildEvidence), 1 << 20));
        report.buildFailureOwner = {receipt: error.buildEvidence, state: receipt.state, handlesReaped: receipt.handlesReaped};
        if (receipt.handlesReaped !== true) handlesReaped = false;
      } catch (receiptError) { handlesReaped = false; report.buildFailureOwner = {receipt: error.buildEvidence, error: receiptError.message}; }
    }
    report.failure = {error: error.message, buildEvidence: error.buildEvidence ?? null, handlesReaped, scratchRetained: true};
  } finally {
    report.handlesReaped = handlesReaped; report.controllerThroughVerificationMillis = performance.now() - controllerStarted;
    try {
      save(join(job.evidence, 'report.json'), report);
      job.seal({state: !handlesReaped ? 'interrupted' : succeeded ? 'succeeded' : 'failed', handlesReaped, logs: collectLogs(job.evidence)});
      // Keep every failed/uncertain scratch; only sealed success may retire it.
      if (succeeded && handlesReaped) job.cleanup();
    } catch (error) { failure ??= error; succeeded = false; }
  }
  if (failure) throw Object.assign(failure, {benchmarkEvidence: job.receipt, scratch: job.directory});
  return {passed: succeeded, state: report.state, report: join(job.evidence, 'report.json'), receipt: job.receipt, artifact: paths.artifactPath,
    benchmarkInvocations: 2, metadataInvocations: 2, legacyRows: 3, streamSamples: 36, performanceAcceptanceEvaluated: false};
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { console.log(JSON.stringify(main(process.argv.slice(2)), null, 2)); }
  catch (error) { console.error(JSON.stringify({error: error.message, benchmarkEvidence: error.benchmarkEvidence, buildEvidence: error.buildEvidence, scratch: error.scratch})); process.exitCode = 1; }
}
