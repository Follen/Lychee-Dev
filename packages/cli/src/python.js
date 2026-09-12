/** Install and run the Python automation helper that the CLI delegates to. */

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

import { dataDir, exists, pythonDepsDir, vendorRoot } from './paths.js';

const REQUIREMENTS = path.join(vendorRoot, 'skill', 'scripts', 'requirements.txt');
export const automationScript = path.join(vendorRoot, 'skill', 'scripts', 'automation.py');

/** Candidate interpreters, in the order we prefer them. */
export function pythonCandidates(explicit = null) {
  if (explicit) return [explicit];
  const list = [];
  if (process.env.LYCHEEDEV_PYTHON) list.push(process.env.LYCHEEDEV_PYTHON);
  if (process.platform === 'win32') {
    list.push('python', 'python3', 'py');
  } else {
    list.push('python3', 'python');
  }
  return list;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    windowsHide: true,
    ...options,
  });
  return {
    ok: result.status === 0,
    status: result.status,
    stdout: (result.stdout || '').trim(),
    stderr: (result.stderr || '').trim(),
    error: result.error ? result.error.message : null,
  };
}

/** Find a working Python 3 interpreter, or null. */
export function findPython(explicit = null) {
  for (const candidate of pythonCandidates(explicit)) {
    const probe = run(candidate, ['-c', 'import sys; print(sys.version_info[0])']);
    if (probe.ok && probe.stdout.startsWith('3')) {
      return { bin: candidate, version: run(candidate, ['--version']).stdout };
    }
  }
  return null;
}

export function depsInstalled() {
  const marker = path.join(pythonDepsDir(), 'zxingcpp');
  return exists(marker);
}

/** Environment that makes the vendored helper import the installed packages. */
export function pythonEnv() {
  const deps = pythonDepsDir();
  const existing = process.env.PYTHONPATH;
  return {
    ...process.env,
    PYTHONPATH: existing ? `${deps}${path.delimiter}${existing}` : deps,
    PYTHONDONTWRITEBYTECODE: '1',
    // Player names, window titles and zone names are not ASCII; without this
    // the helper's stdout is encoded with the console code page and comes back
    // as mojibake.
    PYTHONIOENCODING: 'utf-8',
  };
}

/**
 * Install the pinned optional dependencies into ~/.lycheedev/python.
 *
 * They live outside the package so npm can stay dependency-free and a global
 * install never needs write access to site-packages.
 */
export function installDeps({ pythonBin, quiet = false, onLine = null } = {}) {
  const python = pythonBin || (findPython() || {}).bin;
  if (!python) {
    return { ok: false, error: 'no Python 3 interpreter found on PATH' };
  }
  if (!exists(REQUIREMENTS)) {
    return { ok: false, error: `requirements file is missing: ${REQUIREMENTS}` };
  }
  fs.mkdirSync(dataDir(), { recursive: true });
  const args = ['-m', 'pip', 'install', '--upgrade', '--no-input',
    '--target', pythonDepsDir(), '-r', REQUIREMENTS];
  const result = run(python, args, {
    stdio: quiet ? 'pipe' : 'inherit',
    env: process.env,
  });
  if (!result.ok && quiet && onLine) onLine(result.stderr);
  if (result.ok && onLine) onLine('python dependencies installed');
  return { ok: result.ok, status: result.status, error: result.error, dir: pythonDepsDir() };
}

/** Run the delegated automation CLI with the pinned dependency path. */
export function runAutomation(args, { pythonBin = null, inherit = true } = {}) {
  const python = pythonBin || (findPython() || {}).bin;
  if (!python) {
    return { ok: false, status: 1, stderr: 'no Python 3 interpreter found on PATH' };
  }
  return run(python, ['-B', automationScript, ...args], {
    stdio: inherit ? 'inherit' : 'pipe',
    env: pythonEnv(),
  });
}

export function requirementsPath() {
  return REQUIREMENTS;
}
