// Test-only official Lua 5.1 interpreter + fixed Go Lua acceptance. No Cargo,
// game input, old Go execution, installed tool changes or release publishing.
import assert from 'node:assert/strict';
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beginScratch, contained } from './rust-disk.mjs';
import { ownedSpawnSync } from './rust-owned-process.mjs';
import { fixedToolchain, luaSource, selectCompiler, verifySystemImports } from './luac.mjs';
import { ownedUserEnvironment, sourceDigest, verifyArtifact } from './rust-build.mjs';
import { BRIDGE_GAPS, LIMITS, MANIFEST_SHA256, ORACLE, SUPPORT as ORACLE_SUPPORT, authenticate, cases, cleanLuaEnvironment, hash, materialize, processReaped, regular, runCases, verifyMaterial, verifyOwnerEvidence, verifyUnifiedTOC } from '../tests/rust-parity/formal-lua-3.2.4/assertions.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const BUILD_ARGUMENTS = Object.freeze(['/nologo', '/Bv', '/O2', '/MT', '/Brepro', '/TC']);
const LINK_ARGUMENTS = Object.freeze(['/link', '/Brepro', '/INCREMENTAL:NO', '/OPT:REF', '/OPT:ICF']);
const SOURCES = Object.freeze(['lapi.c', 'lauxlib.c', 'lbaselib.c', 'lcode.c', 'ldblib.c', 'ldebug.c', 'ldo.c', 'ldump.c', 'lfunc.c', 'lgc.c', 'linit.c', 'liolib.c', 'llex.c', 'lmathlib.c', 'lmem.c', 'loadlib.c', 'lobject.c', 'lopcodes.c', 'loslib.c', 'lparser.c', 'lstate.c', 'lstring.c', 'lstrlib.c', 'ltable.c', 'ltablib.c', 'ltm.c', 'lua.c', 'lundump.c', 'lvm.c', 'lzio.c']);
export const SUPPORT = Object.freeze(['tools/rust-formal-lua.mjs', ...ORACLE_SUPPORT, 'tools/luac.mjs', 'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-build-auxiliary.json', 'tools/rust-disk.mjs', 'tools/rust-build.mjs', 'tools/rust-emmy-identity.mjs', 'release/rust-disk-policy.json', 'release/tools/luac.json']);
const identity = paths => Object.fromEntries(paths.map(path => [path, hash(regular(join(root, path)))]));
const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
function windowsOnly() { assert(process.platform === 'win32' && process.arch === 'x64', 'formal-lua.windows_amd64_required'); }
function evidenceFiles(job) {
  return readdirSync(job.evidence).filter(name => name !== 'receipt.json').map(name => join(job.evidence, name)).filter(path => lstatSync(path).isFile());
}
function evidenceBytes(job) { return evidenceFiles(job).reduce((total, path) => total + lstatSync(path).size, 0); }
function finish(job, state, handlesReaped) {
  job.seal({ state: handlesReaped ? state : 'interrupted', handlesReaped, logs: evidenceFiles(job) });
  if (handlesReaped) job.cleanup();
}
async function sourceBytes(path) {
  if (path) return regular(contained(root, resolve(path)), 1 << 20);
  const response = await fetch(luaSource.url, { signal: AbortSignal.timeout(30000) });
  assert(response.ok && response.body, `formal-lua.source_http: ${response.status}`);
  const parts = []; let bytes = 0;
  for await (const part of response.body) { bytes += part.length; assert(bytes <= 1 << 20, 'formal-lua.source_budget'); parts.push(part); }
  return Buffer.concat(parts);
}
export async function buildInterpreter(output, sourceArchive) {
  windowsOnly();
  const destination = contained(root, resolve(output)), support = identity(SUPPORT);
  const pinned = JSON.parse(regular(join(root, 'release/tools/luac.json'))), selected = fixedToolchain(pinned.toolchain);
  assert.equal(luaSource.sha256, '2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333');
  const job = beginScratch(root, 'formal-lua-build', { command: process.argv, inputs: { oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, support, sourceSHA256: luaSource.sha256, toolchain: selected }, admissionOptions: { requiredBytes: 64 << 20, scratchReserveBytes: 64 << 20 } });
  const deadline = Date.now() + 10 * 60000;
  let handlesReaped = true, state = 'failed', sequence = 0;
  function run(command, args, cwd, env = process.env, msvcCompile = false) {
    assert(Date.now() + LIMITS.cleanupMs + 1000 < deadline, 'formal-lua.build_deadline');
    assert(evidenceBytes(job) + LIMITS.nextProcessReserve <= LIMITS.evidence, 'formal-lua.build_evidence_budget');
    const remaining = deadline - Date.now() - LIMITS.cleanupMs;
    assert(remaining > 1000, 'formal-lua.build_deadline');
    handlesReaped = false;
    const result = ownedSpawnSync(command, args, { cwd, env: cleanLuaEnvironment(ownedUserEnvironment(job.directory, env)), evidence: job.evidence, timeout: Math.min(120000, remaining), cargoBuild: false, msvcCompile });
    handlesReaped = processReaped(result) === true;
    save(join(job.evidence, `command-${sequence++}.json`), { command, args, cwd, msvcCompile, exit: result.status, error: result.error?.message, handlesReaped, proof: result.proof });
    assert(handlesReaped && !result.error && result.status === 0, `formal-lua.build_command_failed: ${result.error?.message ?? result.status}`);
    return Buffer.concat([result.stdout ?? Buffer.alloc(0), result.stderr ?? Buffer.alloc(0)]).toString('utf8');
  }
  try {
    mkdirSync(destination);
    const archive = await sourceBytes(sourceArchive); assert.equal(hash(archive), luaSource.sha256, 'formal-lua.source_identity');
    writeFileSync(join(destination, 'lua-5.1.5.tar.gz'), archive, { flag: 'wx' });
    run('tar', ['-xzf', join(destination, 'lua-5.1.5.tar.gz'), '-C', job.directory], job.directory);
    const sourceRoot = join(job.directory, 'lua-5.1.5/src');
    const found = readdirSync(sourceRoot).filter(name => name.endsWith('.c') && !['luac.c', 'print.c'].includes(name)).sort();
    assert.deepEqual(found, SOURCES, 'formal-lua.interpreter_source_set');
    const sourceFiles = readdirSync(sourceRoot).filter(name => /\.[ch]$/.test(name)).sort().map(path => ({ path, sha256: hash(regular(join(sourceRoot, path))) }));
    const locator = join(process.env['ProgramFiles(x86)'] || 'C:/Program Files (x86)', 'Microsoft Visual Studio/Installer/vswhere.exe');
    const installations = run(locator, ['-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'], root).trim().split(/\r?\n/).map(s => s.trim()).filter(Boolean);
    const compiler = selectCompiler(installations, selected);
    const toolFiles = [compiler.executable, compiler.linker].map(path => ({ path, sha256: hash(regular(path, 64 << 20)) }));
    const executable = join(destination, 'lua.exe');
    const args = [...BUILD_ARGUMENTS, `/Fe:${executable}`, ...SOURCES, ...LINK_ARGUMENTS];
    const compilerVersion = run(compiler.executable, args, sourceRoot, compiler.env, true);
    for (const file of toolFiles) assert.equal(hash(regular(file.path, 64 << 20)), file.sha256, 'formal-lua.compiler_changed');
    for (const file of sourceFiles) assert.equal(hash(regular(join(sourceRoot, file.path))), file.sha256, 'formal-lua.compiler_source_changed');
    const bytes = regular(executable, 16 << 20), imports = verifySystemImports(bytes);
    const version = run(executable, ['-v'], destination).trim(); assert.match(version, /^Lua 5\.1\.5\s/);
    const probe = run(executable, ['-e', 'assert(_VERSION=="Lua 5.1"); assert(loadstring("return 6*7")()==42); io.write("formal-lua-interpreter-ok\\n")'], destination);
    assert.equal(probe.replaceAll('\r\n', '\n'), 'formal-lua-interpreter-ok\n');
    const license = regular(join(job.directory, 'lua-5.1.5/COPYRIGHT')); assert.equal(hash(license), pinned.runtime.licenseSHA256);
    writeFileSync(join(destination, 'LICENSE'), license, { flag: 'wx' });
    assert.deepEqual(identity(SUPPORT), support, 'formal-lua.support_changed');
    assert.equal(hash(regular(executable, 16 << 20)), hash(bytes), 'formal-lua.interpreter_changed');
    assert(Date.now() <= deadline, 'formal-lua.build_deadline');
    const report = { schema: 'lycheedev.formal-lua-interpreter.v1', oracle: ORACLE, manifestSHA256: MANIFEST_SHA256, version: '5.1.5', platform: 'windows-amd64', executable, executableSHA256: hash(bytes), executableBytes: bytes.length,
      source: { ...luaSource, bytes: archive.length, archive: join(destination, 'lua-5.1.5.tar.gz') }, license: { path: join(destination, 'LICENSE'), sha256: hash(license) }, toolchain: selected,
      compilerVersion, toolFiles, sourceFiles, arguments: args, imports, buildEvidence: job.receipt, support, interpreterProbe: probe.trim() };
    save(join(destination, 'build-report.json'), report);
    // The report is sealed in the owning evidence, so later runs cannot accept a
    // hand-written runtime manifest or a luac.exe relabeled as an interpreter.
    save(join(job.evidence, 'interpreter-report.json'), report);
    state = 'succeeded'; return { report: join(destination, 'build-report.json'), executableSHA256: report.executableSHA256, buildEvidence: job.receipt };
  } catch (error) {
    save(join(job.evidence, 'failure.json'), { error: error.message, handlesReaped, destination });
    throw Object.assign(error, { buildEvidence: job.receipt });
  } finally { finish(job, state, handlesReaped); }
}
export function verifyInterpreter(reportPath) {
  const path = contained(root, resolve(reportPath)), bytes = regular(path), report = JSON.parse(bytes);
  assert.equal(report.schema, 'lycheedev.formal-lua-interpreter.v1');
  assert.equal(report.oracle, ORACLE); assert.equal(report.manifestSHA256, MANIFEST_SHA256);
  assert.equal(report.version, '5.1.5'); assert.equal(report.platform, 'windows-amd64');
  assert.equal(report.source.sha256, luaSource.sha256); assert.equal(report.source.url, luaSource.url);
  assert.equal(report.interpreterProbe, 'formal-lua-interpreter-ok');
  const pinned = JSON.parse(regular(join(root, 'release/tools/luac.json')));
  assert.deepEqual(report.toolchain, fixedToolchain(pinned.toolchain));
  assert.deepEqual(report.support, identity(SUPPORT), 'formal-lua.runtime_support_identity');
  const executable = contained(root, report.executable), runtime = regular(executable, 16 << 20);
  assert.equal(hash(runtime), report.executableSHA256); assert.equal(runtime.length, report.executableBytes);
  assert.deepEqual(verifySystemImports(runtime), report.imports);
  assert.equal(hash(regular(contained(root, report.source.archive), 1 << 20)), luaSource.sha256);
  assert.equal(hash(regular(contained(root, report.license.path))), report.license.sha256);
  assert.equal(report.license.sha256, pinned.runtime.licenseSHA256);
  const receiptPath = contained(root, report.buildEvidence), evidence = verifyOwnerEvidence(root, receiptPath, 'formal-lua-build'), { receipt } = evidence;
  assert.equal(receipt.inputHashes.oracle, ORACLE); assert.equal(receipt.inputHashes.manifestSHA256, MANIFEST_SHA256);
  assert.deepEqual(receipt.inputHashes.support, report.support); assert.equal(receipt.inputHashes.sourceSHA256, luaSource.sha256);
  assert.deepEqual(receipt.inputHashes.toolchain, report.toolchain);
  const sealedReport = evidence.logs.get(join(dirname(receiptPath), 'interpreter-report.json'));
  assert(sealedReport && hash(sealedReport) === hash(bytes), 'formal-lua.interpreter_report_not_sealed');
  assert.equal(evidence.processes.length, 5, 'formal-lua.interpreter_build_process_set');
  for (let index = 0; index < 5; index++) {
    const command = evidence.json(`command-${index}.json`);
    const process = evidence.processes.filter(({ request }) => request.command === command.command && request.cwd === command.cwd && JSON.stringify(request.args) === JSON.stringify(command.args));
    assert.equal(process.length, 1, 'formal-lua.build_command_not_owned');
    assert.equal(command.exit, 0); assert(!command.error && command.handlesReaped, 'formal-lua.build_command_failed');
    assert.deepEqual(command.proof, process[0].proof);
    if (index === 2) {
      assert.equal(command.command, report.toolFiles[0].path); assert.equal(command.msvcCompile, true);
      assert.deepEqual(command.args, report.arguments);
      assert.deepEqual(command.args, [...BUILD_ARGUMENTS, `/Fe:${report.executable}`, ...SOURCES, ...LINK_ARGUMENTS]);
    }
  }
  return { report, reportSHA256: hash(bytes), buildReceiptSHA256: evidence.receiptSHA256 };
}
export function verifyCandidateArtifact(repository, artifactPath) {
  const base = resolve(repository), path = contained(base, resolve(artifactPath)), artifact = verifyArtifact(base, path);
  assert.equal(artifact.schema, 'lycheedev.rust-build-artifact.v1');
  assert.equal(artifact.sourceRoot, base, 'formal-lua.candidate_source_root');
  assert.equal(artifact.sameInvocation, true);
  const evidence = verifyOwnerEvidence(base, contained(base, artifact.buildEvidence), 'rust-build');
  const command = evidence.json('command.json');
  assert.equal(command.command, 'cargo'); assert.equal(command.args?.[0], 'build'); assert.equal(command.cwd, base);
  assert.equal(command.exit, 0); assert(!command.error && !command.signal, 'formal-lua.candidate_build_failed');
  assert.deepEqual(artifact.command, ['cargo', ...command.args]); assert.deepEqual(evidence.receipt.command, artifact.command);
  assert.deepEqual(command.inputHashes, artifact.inputHashes); assert.deepEqual(evidence.receipt.inputHashes, artifact.inputHashes);
  assert.equal(command.environmentSHA256, artifact.environmentSHA256);
  assert.equal(evidence.processes.length, 1, 'formal-lua.candidate_build_process_set');
  const request = evidence.processes[0].request;
  assert.equal(request.command, 'cargo'); assert.equal(request.cwd, base); assert.equal(request.cargoBuild, true);
  assert.deepEqual(request.args, command.args);
  const sourceTree = sourceDigest(base);
  assert.equal(sourceTree, artifact.inputHashes.sourceTree, 'formal-lua.candidate_source_changed');
  return { artifact, artifactSHA256: hash(regular(path)), buildReceiptSHA256: evidence.receiptSHA256, sourceTree };
}
export function runAcceptance(interpreterPath, artifactPath) {
  windowsOnly();
  const started = Date.now(), deadline = started + LIMITS.totalMs;
  const runtime = verifyInterpreter(interpreterPath), candidate = verifyCandidateArtifact(root, resolve(artifactPath));
  const { artifact, artifactSHA256 } = candidate;
  const inputs = authenticate(root), support = identity(SUPPORT);
  const job = beginScratch(root, 'formal-lua-acceptance', { command: process.argv, inputs: { oracle: ORACLE, manifestSHA256: inputs.manifestSHA256, interpreterReportSHA256: runtime.reportSHA256, interpreterSHA256: runtime.report.executableSHA256, artifactSHA256, binarySHA256: artifact.binarySHA256, support }, admissionOptions: { requiredBytes: 128 << 20, scratchReserveBytes: 32 << 20 } });
  const target = join(job.directory, 'fixture');
  let state = 'failed', handlesReaped = true;
  try {
    materialize(root, target, inputs); verifyUnifiedTOC(target);
    save(join(job.evidence, 'identity.json'), { inputs, support, runtime, candidate });
    const checkIdentity = () => {
      assert.deepEqual(authenticate(root), inputs, 'formal-lua.checkout_changed');
      assert.deepEqual(identity(SUPPORT), support, 'formal-lua.support_changed');
      assert.deepEqual(verifyInterpreter(interpreterPath), runtime, 'formal-lua.runtime_changed');
      assert.deepEqual(verifyArtifact(root, resolve(artifactPath)), artifact, 'formal-lua.artifact_changed');
      assert.equal(hash(regular(contained(root, resolve(artifactPath)))), artifactSHA256, 'formal-lua.artifact_record_changed');
      verifyMaterial(target, inputs);
    };
    let sequence = 0;
    const observations = runCases(cases(inputs), { deadline, checkIdentity, evidenceBytes: () => evidenceBytes(job),
      execute(row, timeout) {
        handlesReaped = false;
        const result = ownedSpawnSync(runtime.report.executable, row.args, { cwd: join(target, row.cwd), env: cleanLuaEnvironment(ownedUserEnvironment(job.directory)), evidence: job.evidence, timeout, cargoBuild: false, msvcCompile: false });
        handlesReaped = processReaped(result) === true;
        return result;
      }, onResult(row) { save(join(job.evidence, `case-${String(sequence++).padStart(3, '0')}.json`), row); },
    });
    let identityError; try { checkIdentity(); assert.deepEqual(verifyCandidateArtifact(root, resolve(artifactPath)), candidate, 'formal-lua.candidate_changed'); } catch (error) { identityError = error.message; }
    const elapsedMs = Date.now() - started, totalDeadlineExceeded = elapsedMs > LIMITS.totalMs;
    const passed = !identityError && !totalDeadlineExceeded && handlesReaped && observations.length === LIMITS.cases && observations.every(row => row.state === 'passed');
    const report = { schema: 'lycheedev.formal-lua-acceptance.v1', state: passed ? 'passed_selected_lua_suites' : 'failed', passed, oracle: ORACLE, manifestSHA256: inputs.manifestSHA256,
      releaseFieldsSHA256: inputs.releaseFieldsSHA256, previousOracle: inputs.previousOracle, increment: inputs.increment,
      interpreterSHA256: runtime.report.executableSHA256, interpreterReportSHA256: runtime.reportSHA256, artifactSHA256, binarySHA256: artifact.binarySHA256,
      inputsStable: !identityError, identityError, handlesReaped, elapsedMs, totalDeadlineExceeded, limits: LIMITS, profiles: [...new Set(observations.filter(row => row.kind === 'addon').map(row => row.profile))],
      observations, staticTOC: { source: 'TestUnifiedTOCContract', state: 'passed_static' }, deferredHostTests: BRIDGE_GAPS, historicalAddonExtras: inputs.historicalAddonExtras, retainedReleaseFields: inputs.retainedReleaseFields,
      scope: '216 fixed Go 3.2.4 addon Lua suite/profile executions plus four standalone protocol Lua suites. Real Lua assertions, WoW API mocks. No Go host tests, Rust bridge integration, real client, seven-product or historical-consumer cleanup claim; the sealed Rust artifact is identity-bound but is not exercised by these Lua suites.' };
    save(join(job.evidence, 'report.json'), report);
    state = passed ? 'succeeded' : 'failed';
    return { passed, report: join(job.evidence, 'report.json'), receipt: job.receipt, counts: Object.fromEntries(['passed', 'failed', 'not_run'].map(value => [value, observations.filter(row => row.state === value).length])), deferredHostTests: BRIDGE_GAPS };
  } catch (error) {
    save(join(job.evidence, 'controller-failure.json'), { error: error.message, handlesReaped });
    throw Object.assign(error, { acceptanceEvidence: job.receipt });
  } finally { finish(job, state, handlesReaped); }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, first, second, ...extra] = process.argv.slice(2);
  assert(first && !extra.length && (command === 'build' || command === 'run' && second), 'usage: node tools/rust-formal-lua.mjs build <new-directory> [official-source-archive] | run <interpreter-build-report.json> <sealed-rust-artifact.json>');
  const result = command === 'build' ? await buildInterpreter(first, second) : runAcceptance(first, second);
  console.log(JSON.stringify(result, null, 2));
  if (result.passed === false) process.exitCode = 1;
}
