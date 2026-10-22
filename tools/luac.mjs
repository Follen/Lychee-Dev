// Build-time preparation only. npm and Source never download or execute a PATH compiler.
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beginScratch, contained } from './rust-disk.mjs';
import { ownedSpawnSync } from './rust-owned-process.mjs';
const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const luaSource = { url: 'https://www.lua.org/ftp/lua-5.1.5.tar.gz', sha256: '2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333' };
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export function fixedToolchain(value) {
  if (value?.family !== 'msvc' || !/^\d+\.\d+\.\d+$/.test(value.version ?? '') || !/^\d+\.\d+\.\d+\.\d+$/.test(value.sdkVersion ?? '')) throw new Error('luac.invalid_fixed_toolchain');
  return { family: 'msvc', version: value.version, sdkVersion: value.sdkVersion };
}
export function parseBuildOptions(argv) {
  const [command, ...args] = argv, positional = [], options = {}, seen = new Set();
  if (command !== 'build') throw new Error('usage: node tools/luac.mjs build <new-directory> [source-archive] [--msvc-version <exact> --sdk-version <exact>] [--write-identity]');
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (!arg.startsWith('--')) { positional.push(arg); continue; }
    if (!['--source-archive', '--msvc-version', '--sdk-version', '--write-identity'].includes(arg)) throw new Error(`luac.unknown_option: ${arg}`);
    if (seen.has(arg)) throw new Error(`luac.duplicate_option: ${arg}`);
    seen.add(arg);
    if (arg === '--write-identity') options.writeIdentity = true;
    else { const value = args[++i]; if (!value || value.startsWith('--')) throw new Error(`luac.option_value_missing: ${arg}`); options[arg] = value; }
  }
  if (!positional[0] || positional.length > 2) throw new Error('luac.invalid_positionals');
  if (positional[1] && options['--source-archive']) throw new Error('luac.duplicate_source_archive');
  if (seen.has('--msvc-version') !== seen.has('--sdk-version')) throw new Error('luac.toolchain_pair_required');
  return { output: positional[0], sourceArchive: positional[1] ?? options['--source-archive'], writeIdentity: options.writeIdentity ?? false,
    ...(seen.has('--msvc-version') ? { toolchain: fixedToolchain({ family: 'msvc', version: options['--msvc-version'], sdkVersion: options['--sdk-version'] }) } : {}) };
}
// Search installed products for the exact requested toolchain, never latest or PATH.
// A different installed version is not a substitute for missing pinned tools.
export function selectCompiler(installations, requested, environment = process.env, exists = existsSync) {
  const { version, sdkVersion } = fixedToolchain(requested);
  const matches = [...new Set(installations)].filter(installation => ['cl.exe', 'link.exe'].every(name => exists(join(installation, 'VC/Tools/MSVC', version, 'bin/Hostx64/x64', name))));
  if (!matches.length) throw new Error(`luac.msvc_missing: ${version}`);
  if (matches.length !== 1) throw new Error(`luac.msvc_ambiguous: ${version}`);
  const tools = join(matches[0], 'VC/Tools/MSVC', version), bin = join(tools, 'bin/Hostx64/x64');
  const sdk = join(environment['ProgramFiles(x86)'] || 'C:/Program Files (x86)', 'Windows Kits/10');
  const required = [join(tools, 'include/vcruntime.h'), join(tools, 'lib/x64/libcmt.lib'), join(tools, 'lib/x64/libvcruntime.lib'),
    ...['ucrt/errno.h', 'shared/sdkddkver.h', 'um/windows.h', 'winrt/winstring.h'].map(name => join(sdk, 'Include', sdkVersion, name)),
    join(sdk, 'Lib', sdkVersion, 'ucrt/x64/libucrt.lib'), join(sdk, 'Lib', sdkVersion, 'um/x64/kernel32.lib')];
  if (required.some(path => !exists(path))) throw new Error(`luac.toolchain_incomplete: MSVC ${version}, SDK ${sdkVersion}`);
  const env = Object.fromEntries(Object.entries(environment).filter(([name]) => !['PATH', 'INCLUDE', 'LIB', 'LIBPATH', 'CL', '_CL_', 'LINK', '_LINK_'].includes(name.toUpperCase())));
  const inheritedPath = Object.entries(environment).find(([name]) => name.toUpperCase() === 'PATH')?.[1] ?? '';
  return { executable: join(bin, 'cl.exe'), linker: join(bin, 'link.exe'), version, sdkVersion,
    env: { ...env, PATH: `${bin};${inheritedPath}`, INCLUDE: [join(tools, 'include'), ...['ucrt', 'shared', 'um', 'winrt'].map(name => join(sdk, 'Include', sdkVersion, name))].join(';'), LIB: [join(tools, 'lib/x64'), ...['ucrt', 'um'].map(name => join(sdk, 'Lib', sdkVersion, name, 'x64'))].join(';') } };
}
function compiler(requested, run) {
  const locator = join(process.env['ProgramFiles(x86)'] || 'C:/Program Files (x86)', 'Microsoft Visual Studio/Installer/vswhere.exe');
  const installations = run(locator, ['-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'], repository).trim().split(/\r?\n/).map(row => row.trim()).filter(Boolean);
  return selectCompiler(installations, requested);
}
function regularBytes(path, maximum) {
  const stat = lstatSync(path);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > maximum) throw new Error('luac.file_budget_or_shape');
  const bytes = readFileSync(path);
  if (bytes.length !== stat.size) throw new Error('luac.file_changed');
  return bytes;
}
async function downloadSource() {
  const response = await fetch(luaSource.url, { signal: AbortSignal.timeout(30000) });
  if (!response.ok || !response.body) throw new Error(`luac.source_download: ${response.status}`);
  const parts = []; let length = 0;
  for await (const part of response.body) { length += part.length; if (length > 1024 * 1024) throw new Error('luac.source_budget'); parts.push(part); }
  return Buffer.concat(parts);
}
export function peImports(bytes) {
  if (bytes.length < 256 || bytes.toString('ascii', 0, 2) !== 'MZ') throw new Error('luac.pe_invalid');
  const pe = bytes.readUInt32LE(60); if (pe + 264 > bytes.length || bytes.toString('ascii', pe, pe + 4) !== 'PE\0\0' || bytes.readUInt16LE(pe + 4) !== 0x8664) throw new Error('luac.pe_not_amd64');
  const count = bytes.readUInt16LE(pe + 6), optional = pe + 24, size = bytes.readUInt16LE(pe + 20);
  if (bytes.readUInt16LE(optional) !== 0x20b || count > 96 || optional + size + count * 40 > bytes.length) throw new Error('luac.pe_invalid');
  const rva = address => {
    for (let i = 0; i < count; i++) { const section = optional + size + i * 40, virtual = bytes.readUInt32LE(section + 12), length = Math.max(bytes.readUInt32LE(section + 8), bytes.readUInt32LE(section + 16)); if (address >= virtual && address < virtual + length) { const offset = bytes.readUInt32LE(section + 20) + address - virtual; if (offset >= bytes.length) break; return offset; } }
    throw new Error('luac.pe_invalid_rva');
  };
  const address = bytes.readUInt32LE(optional + 112 + 8); if (!address) return [];
  let offset = rva(address); const names = [];
  for (let i = 0; i < 256; i++, offset += 20) { if (offset + 20 > bytes.length) throw new Error('luac.pe_invalid'); const nameRVA = bytes.readUInt32LE(offset + 12); if (!nameRVA) return names; const start = rva(nameRVA); const end = bytes.indexOf(0, start); if (end < start || end - start > 256) throw new Error('luac.pe_invalid'); names.push(bytes.toString('ascii', start, end).toLowerCase()); }
  throw new Error('luac.pe_import_limit');
}
export function verifySystemImports(bytes) { const imports = peImports(bytes); for (const name of imports) if (!['kernel32.dll', 'advapi32.dll', 'user32.dll', 'ntdll.dll'].includes(name) && !name.startsWith('api-ms-win-core-')) throw new Error(`luac.external_runtime_dependency: ${name}`); return imports; }
export async function buildLuac(output, { sourceArchive, writeIdentity = false, toolchain: requested } = {}) {
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('luac.windows_amd64_required');
  const root = contained(repository, resolve(output));
  const pinnedBytes = readFileSync(join(repository, 'release/tools/luac.json'));
  const selected = fixedToolchain(requested ?? JSON.parse(pinnedBytes).toolchain);
  const job = beginScratch(repository, 'luac-build', { command: process.argv, inputs: { sourceSHA256: luaSource.sha256, priorIdentitySHA256: hash(pinnedBytes), toolchain: selected }, admissionOptions: { requiredBytes: 32 << 20, scratchReserveBytes: 32 << 20 } });
  const logs = []; let state = 'failed', handlesReaped = true, sequence = 0;
  function run(command, args, cwd, env = process.env, msvcCompile = false) {
    handlesReaped = false;
    // Only the exact reviewed cl.exe compile below requests the separate MSVC
    // admission. Never borrow Cargo approval; any unknown helper still fails.
    const result = ownedSpawnSync(command, args, { cwd, env, evidence: job.evidence, timeout: 120000, cargoBuild: false, msvcCompile });
    handlesReaped = result.handlesReaped === true; logs.push(...result.logs);
    const record = join(job.evidence, `command-${sequence++}.json`);
    writeFileSync(record, JSON.stringify({ command, args, cwd, exit: result.status, error: result.error?.message, handlesReaped }, null, 2) + '\n'); logs.push(record);
    if (!handlesReaped || result.error || result.status !== 0) throw new Error(`luac.build_failed: ${command}: ${result.error?.message ?? result.status}`);
    return Buffer.concat([result.stdout ?? Buffer.alloc(0), result.stderr ?? Buffer.alloc(0)]).toString('utf8');
  }
  try {
    mkdirSync(root); // Never adopt or erase an existing output directory.
    const source = sourceArchive ? regularBytes(sourceArchive, 1024 * 1024) : await downloadSource();
    if (hash(source) !== luaSource.sha256) throw new Error('luac.source_integrity');
    writeFileSync(join(root, 'lua-5.1.5.tar.gz'), source, { flag: 'wx' });
    run('tar', ['-xzf', join(root, 'lua-5.1.5.tar.gz'), '-C', job.directory], job.directory);
    const sourceRoot = join(job.directory, 'lua-5.1.5/src'), toolchain = compiler(selected, run);
    const toolFiles = [toolchain.executable, toolchain.linker].map(path => { const bytes = regularBytes(path, 64 << 20); return { path, bytes: bytes.length, sha256: hash(bytes) }; });
    const sources = readdirSync(sourceRoot).filter(name => name.endsWith('.c') && name !== 'lua.c').sort();
    if (!sources.includes('luac.c') || !sources.includes('print.c') || !sources.includes('lvm.c')) throw new Error('luac.source_incomplete');
    const executable = join(root, 'luac.exe');
    const arguments_ = ['/Bv', '/O2', '/MT', '/Brepro', '/TC', '/link', '/Brepro', '/INCREMENTAL:NO', '/OPT:REF', '/OPT:ICF'];
    const args = ['/nologo', ...arguments_.slice(0, 5), `/Fe:${executable}`, ...sources, ...arguments_.slice(5)];
    const compilerVersion = run(toolchain.executable, args, sourceRoot, toolchain.env, true);
    for (const tool of toolFiles) if (hash(regularBytes(tool.path, 64 << 20)) !== tool.sha256) throw new Error('luac.toolchain_changed');
    const bytes = regularBytes(executable, 16 << 20), imports = verifySystemImports(bytes), version = run(executable, ['-v'], root).trim();
    if (!version.startsWith('Lua 5.1.5')) throw new Error('luac.runtime_version');
    const license = regularBytes(join(job.directory, 'lua-5.1.5/COPYRIGHT'), 1 << 20); writeFileSync(join(root, 'LICENSE'), license, { flag: 'wx' });
    const manifest = { schema: 'lycheedev.luac-build.v1', runtime: { schema: 'lycheedev.luac-runtime.v1', version: '5.1.5', platform: 'windows-amd64', executable: 'luac.exe', sha256: hash(bytes), bytes: bytes.length, license: 'LICENSE', licenseSHA256: hash(license) }, source: { ...luaSource, archive: 'lua-5.1.5.tar.gz', bytes: source.length }, toolchain: selected, arguments: arguments_, imports };
    writeFileSync(join(root, 'runtime.json'), JSON.stringify(manifest.runtime, null, 2) + '\n', { flag: 'wx' });
    const report = join(root, 'build-report.json');
    writeFileSync(report, JSON.stringify({ manifest, compilerVersion, toolFiles, sourceRoot, executable, buildEvidence: job.receipt, priorIdentitySHA256: hash(pinnedBytes), matchesPriorRuntime: JSON.stringify(manifest.runtime) === JSON.stringify(JSON.parse(pinnedBytes).runtime) }, null, 2) + '\n', { flag: 'wx' }); logs.push(report);
    if (writeIdentity) writeFileSync(join(repository, 'release/tools/luac.json'), JSON.stringify(manifest, null, 2) + '\n');
    state = 'succeeded'; return manifest;
  } catch (error) {
    const failure = join(job.evidence, 'failure.json'); writeFileSync(failure, JSON.stringify({ error: error.message, output: root, toolchain: selected }, null, 2) + '\n'); logs.push(failure);
    throw Object.assign(error, { buildEvidence: job.receipt });
  } finally {
    job.seal({ state: handlesReaped ? state : 'interrupted', handlesReaped, logs });
    if (handlesReaped) job.cleanup();
  }
}
export function stageLuac(stage, resources, root) {
  const expected = JSON.parse(readFileSync(join(repository, 'release/tools/luac.json'))), actual = JSON.parse(readFileSync(join(root, 'runtime.json')));
  if (JSON.stringify(expected.runtime) !== JSON.stringify(actual)) throw new Error('luac.runtime_identity_mismatch');
  for (const [name, digest, length] of [['luac.exe', expected.runtime.sha256, expected.runtime.bytes], ['LICENSE', expected.runtime.licenseSHA256], [expected.source.archive, expected.source.sha256, expected.source.bytes], ['runtime.json']]) {
    const bytes = readFileSync(join(root, name)); if (digest && hash(bytes) !== digest || length && bytes.length !== length) throw new Error(`luac.runtime_integrity: ${name}`);
    if (name === 'luac.exe') verifySystemImports(bytes);
    const path = `tool/luac/${name}`; mkdirSync(dirname(join(stage, 'payload', path)), { recursive: true }); writeFileSync(join(stage, 'payload', path), bytes); resources.push({ path, bytes: bytes.length, sha256: hash(bytes) });
  }
  resources.sort((a,b) => a.path.localeCompare(b.path));
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { output, ...options } = parseBuildOptions(process.argv.slice(2));
  console.log(JSON.stringify(await buildLuac(output, options)));
}
