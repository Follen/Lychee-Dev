// Thin launcher for the lycheedev CLI. It only locates the installed CLI and
// forwards argv, stdio and the exit code; every capability lives in the native
// binary distributed by the npm package or the native archives. This script
// never implements, retries or interprets toolkit work.
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { delimiter, join } from 'node:path';

const forwarded = process.argv.slice(2);
const windows = process.platform === 'win32';
const executable = windows ? 'lycheedev.exe' : 'lycheedev';

function fromPath() {
  for (const directory of (process.env.PATH ?? '').split(delimiter)) {
    if (!directory) continue;
    const candidate = join(directory, executable);
    if (existsSync(candidate)) return candidate;
  }
  return undefined;
}

function fromNpmGlobal() {
  const npm = windows ? 'npm.cmd' : 'npm';
  const found = spawnSync(npm, ['prefix', '-g'], { encoding: 'utf8', shell: false, windowsHide: true, timeout: 30000 });
  if (found.status !== 0 || !found.stdout) return undefined;
  const root = found.stdout.trim().split(/\r?\n/)[0];
  const launcher = join(root, 'node_modules', 'lycheedev', 'bin', 'lycheedev.mjs');
  if (!existsSync(launcher)) return undefined;
  return { command: process.execPath, args: [launcher] };
}

let resolution;
if (process.env.LYCHEEDEV_BIN) {
  if (!existsSync(process.env.LYCHEEDEV_BIN)) {
    console.error(`lycheedev launcher: LYCHEEDEV_BIN does not exist: ${process.env.LYCHEEDEV_BIN}`);
    process.exit(3);
  }
  resolution = { command: process.env.LYCHEEDEV_BIN, args: [] };
} else {
  const onPath = fromPath();
  resolution = onPath
    ? { command: onPath, args: [] }
    : fromNpmGlobal();
}
if (!resolution) {
  console.error('lycheedev launcher: installed CLI not found. Install it with `npm install -g lycheedev` or set LYCHEEDEV_BIN to the native binary.');
  process.exit(3);
}

const result = spawnSync(resolution.command, [...resolution.args, ...forwarded], {
  stdio: 'inherit',
  shell: false,
  windowsHide: true,
});
if (result.error) {
  console.error(`lycheedev launcher: ${result.error.message}`);
  process.exit(5);
}
process.exit(result.status ?? 1);
