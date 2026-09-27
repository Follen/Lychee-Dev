#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { readFileSync, realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

// The 2.0 distribution ships a windows-amd64 binary only; every other platform
// fails closed with distribution.unsupported_platform (no downloads, no fallbacks).
const targets = new Map([
  ['win32:x64', 'windows-amd64'],
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
  let activeArgs = process.argv.slice(2);
  try {
    const [major, minor] = process.versions.node.split('.').map(Number);
    if (major < 22 || (major === 22 && minor < 14)) throw new Error('distribution.node_version: requires Node >=22.14.0');
    const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
    let args = process.argv.slice(2);
    // Common flags may precede the command. Normalize only those two shared
    // flags; leave command admission and every other flag to the native parser.
    let commandIndex = 0;
    while (commandIndex < args.length) {
      const flag = args[commandIndex];
      if (flag === '--home' || flag === '--format') commandIndex += 2;
      else if (flag.startsWith('--home=') || flag.startsWith('--format=')) commandIndex++;
      else break;
    }
    if (args[commandIndex] === 'update') args = ['update', ...args.slice(0, commandIndex), ...args.slice(commandIndex + 1)];
    activeArgs = args;
    const managedUpdate = args[0] === 'update' && !args.some(arg => ['--help', '-h', '--plan', '--release'].includes(arg) || arg.startsWith('--release='));
    let result;
    if (managedUpdate) {
      const { update } = await import('./update.mjs');
      const formatIndex = args.indexOf('--format');
      const format = args.find(arg => arg.startsWith('--format='))?.split('=')[1] ?? (formatIndex >= 0 ? args[formatIndex + 1] : 'text');
      const forwarded = args.filter((arg, index) => arg !== '--format' && !arg.startsWith('--format=') && args[index - 1] !== '--format');
      // Keep native argument validation authoritative, including output format.
      if (!['json', 'jsonl', 'text'].includes(format) || args.filter(arg => arg === '--format' || arg.startsWith('--format=')).length > 1) throw Object.assign(new Error('update.invalid_format'), { exit: 2 });
      result = update(root, forwarded, nativePath);
      if (format === 'jsonl') {
        process.stdout.write(`${JSON.stringify({ type: 'begin', schema: result.envelope.schema })}\n`);
        if (result.envelope.ok) process.stdout.write(`${JSON.stringify({ type: 'record', result: result.envelope.result })}\n${JSON.stringify({ type: 'end', envelope: result.envelope })}\n`);
        else process.stdout.write(`${JSON.stringify({ type: 'error', envelope: result.envelope })}\n`);
      } else if (format === 'text' && result.envelope.ok) process.stdout.write(`${JSON.stringify(result.envelope.result, null, 2)}\n`);
      else process.stdout.write(`${JSON.stringify(result.envelope, null, 2)}\n`);
    } else result = launch(root, args);
    if (result.signal) process.kill(process.pid, result.signal);
    else process.exitCode = result.status;
  } catch (error) {
    const args = activeArgs;
    if (args[0] === 'update' && (args.includes('json') || args.includes('--format=json') || args.includes('jsonl') || args.includes('--format=jsonl'))) {
      const envelope = error.envelope ?? { schema: 'lycheedev.result.v1', ok: false, operationId: '', context: {}, result: { complete: false }, captures: [], warnings: [], error: { code: 'update.failed', message: error.message, stage: 'update', retryable: false } };
      const jsonl = args.includes('jsonl') || args.includes('--format=jsonl');
      if (jsonl) process.stdout.write(`${JSON.stringify({ type: 'begin', schema: envelope.schema })}\n${JSON.stringify({ type: 'error', envelope })}\n`);
      else process.stdout.write(`${JSON.stringify(envelope)}\n`);
    } else process.stderr.write(`lycheedev: ${error.message}\n`);
    process.exitCode = error.exit || 3;
  }
}
