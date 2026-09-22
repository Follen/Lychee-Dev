#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { readFileSync, realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const targets = new Map([
  ['win32:x64', 'windows-amd64'],
  ['linux:x64', 'linux-amd64'],
  ['linux:arm64', 'linux-arm64'],
  ['darwin:x64', 'darwin-amd64'],
  ['darwin:arm64', 'darwin-arm64'],
]);

export function nativePath(root, platform = process.platform, arch = process.arch) {
  const target = targets.get(`${platform}:${arch}`);
  if (!target) throw new Error(`distribution.unsupported_platform: ${platform}/${arch}`);
  const manifest = JSON.parse(readFileSync(resolve(root, 'release.json'), 'utf8'));
  const pkg = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));
  if (manifest.schema !== 'lycheedev.release.v1' || manifest.version !== pkg.version) {
    throw new Error('distribution.version_mismatch');
  }
  const expected = `native/${target}/lycheedev${platform === 'win32' ? '.exe' : ''}`;
  const entry = manifest.binaries?.[target];
  if (!entry || entry.path !== expected || !/^[a-f0-9]{64}$/.test(entry.sha256) || !Number.isSafeInteger(entry.bytes) || entry.bytes < 1) {
    throw new Error(`distribution.invalid_binary_record: ${target}`);
  }
  const binary = realpathSync(resolve(root, expected));
  const rel = relative(realpathSync(root), binary);
  if (isAbsolute(rel) || rel === '..' || rel.startsWith(`..${sep}`)) throw new Error('distribution.binary_outside_package');
  const stat = statSync(binary);
  if (!stat.isFile() || stat.size !== entry.bytes || stat.size > 256 * 1024 * 1024) throw new Error('distribution.binary_size');
  const digest = createHash('sha256').update(readFileSync(binary)).digest('hex');
  if (digest !== entry.sha256) throw new Error('distribution.binary_integrity');
  return binary;
}

export function launch(root, args, run = spawnSync) {
  const result = run(nativePath(root), args, { stdio: 'inherit', shell: false, windowsHide: true });
  if (result.error) throw result.error;
  if (result.signal) return { signal: result.signal };
  if (!Number.isInteger(result.status)) throw new Error('distribution.missing_exit_status');
  return { status: result.status };
}

if (process.argv[1] && realpathSync(resolve(process.argv[1])) === fileURLToPath(import.meta.url)) {
  try {
    const [major, minor] = process.versions.node.split('.').map(Number);
    if (major < 22 || (major === 22 && minor < 14)) throw new Error('distribution.node_version: requires Node >=22.14.0');
    const result = launch(resolve(dirname(fileURLToPath(import.meta.url)), '..'), process.argv.slice(2));
    if (result.signal) process.kill(process.pid, result.signal);
    else process.exitCode = result.status;
  } catch (error) {
    process.stderr.write(`lycheedev: ${error.message}\n`);
    process.exitCode = 3;
  }
}
