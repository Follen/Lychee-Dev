// Managed official Lua 5.1.5 interpreter for the current addon and Mailbox v1
// bridge regressions. Build-time preparation only: no Cargo, game input, PATH
// Lua, installed-tool change, network download in consumers, or release action.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beginScratch, contained } from './rust-disk.mjs';
import { ownedSpawnSync } from './rust-owned-process.mjs';
import { fixedToolchain, luaSource, selectCompiler, verifySystemImports } from './luac.mjs';
import { ownedUserEnvironment, sourceDigest, verifyArtifact } from './rust-build.mjs';
import { IMPORT_SCHEMA, verifyLuaToolImport } from './lua-tool-supply.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const BUILD_ARGUMENTS = Object.freeze(['/nologo', '/Bv', '/O2', '/MT', '/Brepro', '/TC']);
const LINK_ARGUMENTS = Object.freeze(['/link', '/Brepro', '/INCREMENTAL:NO', '/OPT:REF', '/OPT:ICF']);
const SOURCES = Object.freeze(['lapi.c', 'lauxlib.c', 'lbaselib.c', 'lcode.c', 'ldblib.c', 'ldebug.c', 'ldo.c', 'ldump.c', 'lfunc.c', 'lgc.c', 'linit.c', 'liolib.c', 'llex.c', 'lmathlib.c', 'lmem.c', 'loadlib.c', 'lobject.c', 'lopcodes.c', 'loslib.c', 'lparser.c', 'lstate.c', 'lstring.c', 'lstrlib.c', 'ltable.c', 'ltablib.c', 'ltm.c', 'lua.c', 'lundump.c', 'lvm.c', 'lzio.c']);
export const INTERPRETER_SCHEMA = 'lycheedev.formal-lua-interpreter.v1';
// Runner source identity, not a frozen reference product: these files define and
// consume the current bridge, so a changed runner cannot silently pair with an
// interpreter report built by an older controller.
export const SUPPORT = Object.freeze([
  'tools/rust-formal-lua.mjs', 'tools/rust-formal-lua-bridge.mjs', 'tools/luac.mjs',
  'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-build-auxiliary.json',
  'tools/rust-disk.mjs', 'tools/rust-build.mjs', 'tools/rust-cargo.mjs', 'tools/rust-emmy-identity.mjs',
  'release/rust-disk-policy.json', 'release/tools/luac.json',
]);
export const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export function regular(path, cap = 1 << 20) {
  const metadata = lstatSync(path);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > cap) throw new Error('formal-lua.invalid_file');
  const bytes = readFileSync(path);
  if (bytes.length !== metadata.size) throw new Error('formal-lua.file_changed');
  return bytes;
}
const identity = paths => Object.fromEntries(paths.map(path => [path, hash(regular(join(root, path)))]));
export function cleanLuaEnvironment(environment) {
  const clean = Object.fromEntries(Object.entries(environment).filter(([name]) => !name.toUpperCase().startsWith('LUA')));
  // Empty module paths prevent host Lua modules from entering the peer; the
  // interpreter version itself comes only from the authenticated record.
  return { ...clean, LUA_PATH: '', LUA_CPATH: '' };
}
function save(path, value) { writeFileSync(path, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' }); }
function windowsOnly() { assert(process.platform === 'win32' && process.arch === 'x64', 'formal-lua.windows_amd64_required'); }
function evidenceFiles(job) {
  return readdirSync(job.evidence).filter(name => name !== 'receipt.json').map(name => join(job.evidence, name)).filter(path => lstatSync(path).isFile());
}
function evidenceBytes(job) { return evidenceFiles(job).reduce((total, path) => total + lstatSync(path).size, 0); }
export function preserveInterpreterEvidence(receiptPath, destination) {
  const receiptBytes = regular(receiptPath), receipt = JSON.parse(receiptBytes);
  assert.equal(receipt.schema, 'lycheedev.disk-owner.v1');
  assert.equal(receipt.owner, 'formal-lua-build');
  assert(['succeeded', 'retired'].includes(receipt.state) && receipt.handlesReaped === true && receipt.activePid === null);
  const evidence = join(destination, 'evidence'); mkdirSync(evidence);
  for (const row of receipt.sealedLogs) {
    const source = contained(dirname(receiptPath), row.path), bytes = regular(source);
    assert.equal(hash(bytes), row.sha256, 'formal-lua.sealed_log_corrupt');
    writeFileSync(join(evidence, basename(row.path)), bytes, {flag: 'wx'});
  }
  writeFileSync(join(evidence, 'receipt.json'), receiptBytes, {flag: 'wx'});
}
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
  const job = beginScratch(root, 'formal-lua-build', { command: process.argv, inputs: { support, sourceSHA256: luaSource.sha256, toolchain: selected }, admissionOptions: { requiredBytes: 64 << 20, scratchReserveBytes: 64 << 20 } });
  const deadline = Date.now() + 10 * 60000;
  let handlesReaped = true, state = 'failed', sequence = 0;
  function run(command, args, cwd, env = process.env, msvcCompile = false) {
    assert(Date.now() < deadline, 'formal-lua.build_deadline');
    assert(evidenceBytes(job) + (1 << 20) < 64 << 20, 'formal-lua.build_evidence_budget');
    handlesReaped = false;
    const result = ownedSpawnSync(command, args, { cwd, env: cleanLuaEnvironment(ownedUserEnvironment(job.directory, env)), evidence: job.evidence, timeout: Math.min(120000, Math.max(1000, deadline - Date.now() - 1000)), cargoBuild: false, msvcCompile });
    handlesReaped = result.handlesReaped === true;
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
    const report = { schema: INTERPRETER_SCHEMA, version: '5.1.5', platform: 'windows-amd64', executable, executableSHA256: hash(bytes), executableBytes: bytes.length,
      source: { ...luaSource, bytes: archive.length, archive: join(destination, 'lua-5.1.5.tar.gz') }, license: { path: join(destination, 'LICENSE'), sha256: hash(license) }, toolchain: selected,
      compilerVersion, toolFiles, sourceFiles, arguments: args, imports, buildEvidence: join(destination, 'evidence/receipt.json'), support, interpreterProbe: probe.trim() };
    save(join(destination, 'build-report.json'), report);
    // The report is sealed in the owning evidence, so later runs cannot accept a
    // hand-written runtime manifest or a luac.exe relabeled as an interpreter.
    save(join(job.evidence, 'interpreter-report.json'), report);
    state = 'succeeded';
    return { report: join(destination, 'build-report.json'), executableSHA256: report.executableSHA256, buildEvidence: report.buildEvidence };
  } catch (error) {
    save(join(job.evidence, 'failure.json'), { error: error.message, handlesReaped, destination });
    throw Object.assign(error, { buildEvidence: job.receipt });
  } finally { finish(job, state, handlesReaped); if (state === 'succeeded' && handlesReaped) preserveInterpreterEvidence(job.receipt, destination); }
}
export function verifyInterpreter(reportPath) {
  const path = contained(root, resolve(reportPath)), bytes = regular(path), report = JSON.parse(bytes);
  if (report.schema === IMPORT_SCHEMA) return verifyLuaToolImport(path);
  assert.equal(report.schema, INTERPRETER_SCHEMA);
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
  return {report, reportSHA256: hash(bytes), ...verifyInterpreterEvidence(report, bytes)};
}
export function verifyInterpreterEvidence(report, reportBytes) {
  const receiptPath = contained(root, report.buildEvidence), receiptBytes = regular(receiptPath), receipt = JSON.parse(receiptBytes);
  assert.equal(receipt.schema, 'lycheedev.disk-owner.v1');
  assert.equal(receipt.owner, 'formal-lua-build');
  assert.equal(receipt.category, 'scratch');
  assert.ok(['succeeded', 'retired'].includes(receipt.state));
  assert.equal(receipt.handlesReaped, true);
  assert.equal(receipt.activePid, null);
  for (const log of receipt.sealedLogs) {
    const preserved = contained(dirname(receiptPath), join(dirname(receiptPath), basename(log.path)));
    assert.equal(hash(regular(preserved)), log.sha256, 'formal-lua.sealed_log_corrupt');
  }
  const sealedReport = join(dirname(receiptPath), 'interpreter-report.json');
  assert.equal(hash(regular(sealedReport)), hash(reportBytes), 'formal-lua.interpreter_report_not_sealed');
  return {buildReceiptSHA256: hash(receiptBytes)};
}

export function verifyCandidateArtifact(repository, artifactPath) {
  const base = resolve(repository), path = contained(base, resolve(artifactPath)), artifact = verifyArtifact(base, path);
  assert.equal(artifact.schema, 'lycheedev.rust-build-artifact.v1');
  assert.equal(artifact.sourceRoot, base, 'formal-lua.candidate_source_root');
  assert.equal(artifact.sameInvocation, true);
  const sourceTree = sourceDigest(base);
  assert.equal(sourceTree, artifact.inputHashes.sourceTree, 'formal-lua.candidate_source_changed');
  return { artifact, artifactSHA256: hash(regular(path)), buildReceiptSHA256: hash(regular(contained(base, artifact.buildEvidence))), sourceTree };
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, first, second, ...extra] = process.argv.slice(2);
  assert(first && !extra.length && (command === 'build' || command === 'verify' || command === 'run'),
    'usage: node tools/rust-formal-lua.mjs build <new-directory> [official-source-archive] | verify <interpreter-build-report.json> | run <interpreter-build-report.json> <sealed-rust-artifact.json>');
  if (command === 'build') console.log(JSON.stringify(await buildInterpreter(first, second), null, 2));
  else if (command === 'verify') console.log(JSON.stringify(verifyInterpreter(first), null, 2));
  else {
    // Kept as a compatibility-free explicit pair check: both records are
    // verified and printed; business execution lives in the bridge controller.
    assert(second, 'run requires the candidate artifact');
    console.log(JSON.stringify({ interpreter: verifyInterpreter(first), candidate: verifyCandidateArtifact(root, resolve(second)) }, null, 2));
  }
}
