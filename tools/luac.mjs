// Build-time preparation only. npm and Source never download or execute a PATH compiler.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const luaSource = { url: 'https://www.lua.org/ftp/lua-5.1.5.tar.gz', sha256: '2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333' };
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
function run(command, args, cwd, env = process.env) {
  const result = spawnSync(command, args, { cwd, env, shell: false, windowsHide: true, encoding: 'utf8', timeout: 120000, maxBuffer: 4 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`luac.build_failed: ${command}: ${result.error || result.status}\n${result.stdout}\n${result.stderr}`);
  return result.stdout + result.stderr;
}
function lastVersion(directory, accept = () => true) { return readdirSync(directory).filter(name => /^\d+(\.\d+)+$/.test(name) && accept(name)).sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).at(-1); }
function compiler() {
  const locator = join(process.env['ProgramFiles(x86)'] || 'C:/Program Files (x86)', 'Microsoft Visual Studio/Installer/vswhere.exe');
  const installation = run(locator, ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'], repository).trim();
  if (!installation) throw new Error('luac.msvc_missing');
  const tools = join(installation, 'VC/Tools/MSVC'); const version = lastVersion(tools); if (!version) throw new Error('luac.msvc_missing');
  const sdk = join(process.env['ProgramFiles(x86)'] || 'C:/Program Files (x86)', 'Windows Kits/10'); const sdkVersion = lastVersion(join(sdk, 'Include'), name => existsSync(join(sdk, 'Include', name, 'ucrt/errno.h')) && existsSync(join(sdk, 'Lib', name, 'ucrt/x64/libucrt.lib'))); if (!sdkVersion) throw new Error('luac.sdk_missing');
  const bin = join(tools, version, 'bin/Hostx64/x64');
  return { executable: join(bin, 'cl.exe'), version, sdkVersion,
    env: { ...process.env, PATH: `${bin};${process.env.PATH}`, INCLUDE: [join(tools, version, 'include'), ...['ucrt', 'shared', 'um', 'winrt'].map(name => join(sdk, 'Include', sdkVersion, name))].join(';'), LIB: [join(tools, version, 'lib/x64'), ...['ucrt', 'um'].map(name => join(sdk, 'Lib', sdkVersion, name, 'x64'))].join(';') } };
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
export async function buildLuac(output, { sourceArchive, writeIdentity = false } = {}) {
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('luac.windows_amd64_required');
  const root = resolve(output); mkdirSync(root); // Never adopt or erase an existing build directory.
  let source = sourceArchive ? readFileSync(sourceArchive) : Buffer.from(await (await fetch(luaSource.url, { signal: AbortSignal.timeout(30000) })).arrayBuffer());
  if (source.length > 1024 * 1024 || hash(source) !== luaSource.sha256) throw new Error('luac.source_integrity');
  writeFileSync(join(root, 'lua-5.1.5.tar.gz'), source, { flag: 'wx' });
  run('tar', ['-xzf', join(root, 'lua-5.1.5.tar.gz'), '-C', root], root);
  const sourceRoot = join(root, 'lua-5.1.5/src'), toolchain = compiler();
  const sources = readdirSync(sourceRoot).filter(name => name.endsWith('.c') && name !== 'lua.c').sort();
  if (!sources.includes('luac.c') || !sources.includes('print.c') || !sources.includes('lvm.c')) throw new Error('luac.source_incomplete');
  const executable = join(root, 'luac.exe');
  const compilerVersion = run(toolchain.executable, ['/Bv', '/nologo', '/c', '/TC', 'luac.c', '/Fo:identity.obj'], sourceRoot, toolchain.env);
  const args = ['/nologo', '/O2', '/MT', '/Brepro', '/TC', `/Fe:${executable}`, ...sources, '/link', '/Brepro', '/INCREMENTAL:NO', '/OPT:REF', '/OPT:ICF'];
  run(toolchain.executable, args, sourceRoot, toolchain.env);
  const bytes = readFileSync(executable), imports = verifySystemImports(bytes), version = run(executable, ['-v'], root).trim();
  if (!version.startsWith('Lua 5.1.5')) throw new Error('luac.runtime_version');
  const license = readFileSync(join(root, 'lua-5.1.5/COPYRIGHT')); writeFileSync(join(root, 'LICENSE'), license, { flag: 'wx' });
  const manifest = { schema: 'lycheedev.luac-build.v1', runtime: { schema: 'lycheedev.luac-runtime.v1', version: '5.1.5', platform: 'windows-amd64', executable: 'luac.exe', sha256: hash(bytes), bytes: bytes.length, license: 'LICENSE', licenseSHA256: hash(license) }, source: { ...luaSource, archive: 'lua-5.1.5.tar.gz', bytes: source.length }, toolchain: { family: 'msvc', version: toolchain.version, sdkVersion: toolchain.sdkVersion }, arguments: ['/O2', '/MT', '/Brepro', '/TC', '/link', '/Brepro', '/INCREMENTAL:NO', '/OPT:REF', '/OPT:ICF'], imports };
  writeFileSync(join(root, 'runtime.json'), JSON.stringify(manifest.runtime, null, 2) + '\n', { flag: 'wx' });
  writeFileSync(join(root, 'build-report.json'), JSON.stringify({ manifest, compilerVersion, sourceRoot, executable }, null, 2) + '\n', { flag: 'wx' });
  if (writeIdentity) writeFileSync(join(repository, 'release/tools/luac.json'), JSON.stringify(manifest, null, 2) + '\n');
  return manifest;
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
  const [command, output, archive, write] = process.argv.slice(2); if (command !== 'build' || !output) throw new Error('usage: node tools/luac.mjs build <new-directory> [source-archive] [--write-identity]');
  console.log(JSON.stringify(await buildLuac(output, { sourceArchive: archive === '--write-identity' ? undefined : archive, writeIdentity: write === '--write-identity' || archive === '--write-identity' })));
}
