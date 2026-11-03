// DuckDB's pinned Windows static library requires GCC's emulated TLS ABI.
// GCC 16 switched to native TLS; use this exact portable GCC 14 toolchain.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const nativeToolchain = JSON.parse(readFileSync(join(root, 'release/tools/mingw.json'), 'utf8'));
const defaultRoot = join(root, '.tmp/toolchains/mingw64');

function run(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8', windowsHide: true, timeout: 120000 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`native_toolchain.command_failed: ${command}\n${result.stderr}`);
  return result.stdout.trim();
}

export function nativeBuildEnv(base = process.env) {
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('native_toolchain.windows_amd64_required');
  const directory = resolve(base.LYCHEEDEV_MINGW_ROOT || defaultRoot);
  const cc = join(directory, 'bin/gcc.exe'), cxx = join(directory, 'bin/g++.exe');
  if (!existsSync(cc) || !existsSync(cxx)) throw new Error('native_toolchain.missing: run node tools/native-toolchain.mjs prepare');
  if (run(cc, ['-dumpfullversion']) !== nativeToolchain.version || run(cc, ['-dumpmachine']) !== nativeToolchain.target) {
    throw new Error('native_toolchain.compiler_mismatch');
  }
  return { ...base, CGO_ENABLED: '1', CC: `"${cc}"`, CXX: `"${cxx}"`, LYCHEEDEV_MINGW_ROOT: directory };
}

export async function prepareNativeToolchain() {
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('native_toolchain.windows_amd64_required');
  const cache = dirname(defaultRoot);
  mkdirSync(cache, { recursive: true });
  const archive = join(cache, 'mingw.7z');
  if (!existsSync(archive)) {
    const response = await fetch(nativeToolchain.url);
    if (!response.ok) throw new Error(`native_toolchain.download: HTTP ${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer());
    if (bytes.length !== nativeToolchain.bytes || createHash('sha256').update(bytes).digest('hex') !== nativeToolchain.sha256) throw new Error('native_toolchain.archive_mismatch');
    writeFileSync(archive, bytes);
  }
  const bytes = readFileSync(archive);
  if (bytes.length !== nativeToolchain.bytes || createHash('sha256').update(bytes).digest('hex') !== nativeToolchain.sha256) throw new Error('native_toolchain.archive_mismatch');
  run('tar', ['-xf', archive, '-C', cache]);
  const env = nativeBuildEnv({ ...process.env, LYCHEEDEV_MINGW_ROOT: defaultRoot });
  if (process.env.GITHUB_ENV) {
    appendFileSync(process.env.GITHUB_ENV, ['CGO_ENABLED', 'CC', 'CXX', 'LYCHEEDEV_MINGW_ROOT'].map(key => `${key}=${env[key]}\n`).join(''));
  }
  return { version: nativeToolchain.version, sha256: nativeToolchain.sha256, directory: defaultRoot };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] !== 'prepare') throw new Error('usage: node tools/native-toolchain.mjs prepare');
  console.log(JSON.stringify(await prepareNativeToolchain()));
}
