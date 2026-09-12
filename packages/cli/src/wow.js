/** Locate World of Warcraft installations, their builds, and running instances. */

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { addonFolderName, exists, isDir } from './paths.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const instancesScript = path.join(here, 'py', 'instances.py');

/**
 * Locate a Python 3 interpreter.
 *
 * This lives here instead of `python.js` on purpose: `python.js` only depends
 * on `paths.js`, and importing it from here would create a cycle.
 */
export function findPythonBin() {
  if (process.env.LYCHEEDEV_PYTHON) return process.env.LYCHEEDEV_PYTHON;
  const candidates = process.platform === 'win32'
    ? ['python', 'python3', 'py']
    : ['python3', 'python'];
  for (const candidate of candidates) {
    try {
      const out = execFileSync(candidate, ['-c', 'import sys; print(sys.version_info[0])'], {
        encoding: 'utf8', windowsHide: true, timeout: 15000,
      });
      if (out.trim().startsWith('3')) return candidate;
    } catch {
      // try the next candidate
    }
  }
  return null;
}

/**
 * Every flavor folder Blizzard ships. `id` is the stable key used by the CLI;
 * `folder` is what Blizzard names the directory; `toc` is the addon catalog
 * that serves it, or null when the addon does not support that build.
 *
 * The Titan folder is `_classic_titan_`, which holds client 3.80.2 — the build
 * the addon's Wrath TOC targets (Interface 38002). The previous value
 * `_classic_arena_` does not exist, so an installed Titan client was silently
 * skipped by both `install` and `doctor`.
 */
export const CLIENT_FLAVORS = [
  { id: 'retail', folder: '_retail_', label: 'Retail', exe: 'Wow.exe', toc: 'Lychee Dev_Mainline.toc' },
  { id: 'classic', folder: '_classic_', label: 'Classic', exe: 'WowClassic.exe', toc: 'Lychee Dev_Mists.toc' },
  { id: 'titan', folder: '_classic_titan_', label: 'Classic Titan', exe: 'WowClassic.exe', toc: 'Lychee Dev_Wrath.toc' },
  { id: 'classicEra', folder: '_classic_era_', label: 'Classic Era', exe: 'WowClassic.exe', toc: null },
  { id: 'anniversary', folder: '_anniversary_', label: 'Anniversary', exe: 'WowClassic.exe', toc: null },
  { id: 'beta', folder: '_beta_', label: 'Beta', exe: 'WowB.exe', toc: null },
];

/** Flavors with an addon catalog, i.e. the builds the addon can install into. */
export function supportedFlavors() {
  return CLIENT_FLAVORS.filter((flavor) => flavor.toc);
}

export function flavorById(id) {
  return CLIENT_FLAVORS.find((flavor) => flavor.id === id) || null;
}

export function flavorByFolder(folder) {
  return CLIENT_FLAVORS.find((flavor) => flavor.folder === folder) || null;
}

const KNOWN_ROOTS = {
  win32: [
    'C:\\Program Files (x86)\\World of Warcraft',
    'C:\\Program Files\\World of Warcraft',
    'D:\\World of Warcraft',
    'D:\\Game\\World of Warcraft',
    'E:\\World of Warcraft',
    'E:\\Game\\World of Warcraft',
  ],
  darwin: ['/Applications/World of Warcraft'],
  linux: [],
};

function looksLikeWowRoot(dir) {
  if (!isDir(dir)) return false;
  return CLIENT_FLAVORS.some((flavor) => isDir(path.join(dir, flavor.folder)));
}

/** Roots on this machine that contain at least one known flavor folder. */
export function detectWowRoots() {
  const candidates = [];
  const seen = new Set();
  const push = (dir) => {
    if (!dir || seen.has(dir)) return;
    seen.add(dir);
    if (looksLikeWowRoot(dir)) candidates.push(dir);
  };

  for (const root of KNOWN_ROOTS[process.platform] || []) push(root);
  for (const drive of ['C', 'D', 'E', 'F']) {
    push(`${drive}:\\World of Warcraft`);
    push(`${drive}:\\Game\\World of Warcraft`);
  }
  return candidates;
}

/** Client version, read from the executable's own file metadata. */
function exeVersion(exePath) {
  try {
    const out = execFileSync('powershell', [
      '-NoProfile', '-Command',
      `(Get-Item -LiteralPath '${exePath.replace(/'/g, "''")}').VersionInfo.FileVersion`,
    ], { encoding: 'utf8', windowsHide: true, timeout: 20000 });
    const value = out.replace(/^\uFEFF/, '').trim();
    return value || null;
  } catch {
    return null;
  }
}

/**
 * Describe every flavor present under a root, including builds the addon does
 * not serve, so `doctor` can explain them instead of ignoring them silently.
 */
export function scanClients(wowRoot) {
  const found = [];
  for (const flavor of CLIENT_FLAVORS) {
    const flavorDir = path.join(wowRoot, flavor.folder);
    if (!isDir(flavorDir)) continue;
    const addonsDir = path.join(flavorDir, 'Interface', 'AddOns');
    const exePath = path.join(flavorDir, flavor.exe);
    const hasExe = exists(exePath);
    found.push({
      ...flavor,
      flavorDir,
      addonsDir,
      addonsDirExists: isDir(addonsDir),
      exePath: hasExe ? exePath : null,
      version: hasExe ? exeVersion(exePath) : null,
      installedDir: path.join(addonsDir, addonFolderName),
      installed: flavor.toc ? exists(path.join(addonsDir, addonFolderName, flavor.toc)) : false,
    });
  }
  return found;
}

/** Account-level SavedVariables candidates, newest first. */
export function findSavedVariablesCandidates(wowRoot, flavorFolder) {
  const accountsDir = path.join(wowRoot, flavorFolder, 'WTF', 'Account');
  if (!isDir(accountsDir)) return [];
  const results = [];
  for (const account of fs.readdirSync(accountsDir, { withFileTypes: true })) {
    if (!account.isDirectory()) continue;
    const file = path.join(accountsDir, account.name, 'SavedVariables', `${addonFolderName}.lua`);
    if (!exists(file)) continue;
    let mtime = 0;
    try {
      mtime = fs.statSync(file).mtimeMs;
    } catch {
      mtime = 0;
    }
    results.push({ account: account.name, path: file, mtime });
  }
  return results.sort((a, b) => b.mtime - a.mtime);
}

/**
 * Running game instances.
 *
 * Window handles need the Win32 enumeration API, which Node cannot call, so a
 * small read-only helper does it: `src/py/instances.py` enumerates visible
 * windows and reads process image paths without opening a process.
 *
 * Blizzard ships every non-retail client as `WowClassic.exe`, so the process
 * name cannot identify a build; the executable path can, because each install
 * lives under a `_flavor_` directory.
 */
export function findInstances({ pythonBin = null } = {}) {
  if (process.platform !== 'win32') return [];
  const python = pythonBin || findPythonBin();
  if (!python || !exists(instancesScript)) return [];

  let rows = [];
  try {
    const out = execFileSync(python, ['-B', instancesScript], {
      encoding: 'utf8',
      windowsHide: true,
      timeout: 30000,
      env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
    });
    const parsed = JSON.parse(out.replace(/^\uFEFF/, '') || '[]');
    rows = Array.isArray(parsed) ? parsed : [parsed];
  } catch {
    return [];
  }

  return rows
    .map((row) => {
      const flavor = flavorByFolder(row.flavorFolder);
      return { ...row, supported: Boolean(flavor && flavor.toc) };
    })
    .sort((a, b) => (a.flavorId || '').localeCompare(b.flavorId || '') || a.pid - b.pid);
}

/**
 * Choose which running instance a command targets.
 *
 * One instance needs no flag. Several instances of different builds are picked
 * with `--client`. Several instances of the *same* build are genuinely
 * ambiguous, so the caller must pass `--instance <index>` after seeing the
 * table; guessing would type into the wrong game window.
 */
export function resolveInstance(instances, { clientId = null, index = null } = {}) {
  const supported = instances.filter((item) => item.supported);
  if (supported.length === 0) {
    return { error: 'no supported World of Warcraft instance is running' };
  }
  if (index !== null && index !== undefined) {
    const chosen = supported[Number(index)];
    if (!chosen) {
      return { error: `--instance ${index} is out of range (0..${supported.length - 1})` };
    }
    return { instance: chosen };
  }
  const candidates = clientId ? supported.filter((item) => item.flavorId === clientId) : supported;
  if (candidates.length === 0) {
    return { error: `no running instance for client '${clientId}'` };
  }
  if (candidates.length === 1) {
    return { instance: candidates[0] };
  }
  return {
    error: `${candidates.length} instances match; pass --instance <index>`,
    candidates,
  };
}

/**
 * Find the instance a saved pin refers to.
 *
 * A pin stores the build plus which instance of that build, because window
 * handles and pids change every launch. The number shown to the user counts
 * among supported instances, so the same index works here.
 */
export function matchPinned(instances, pin) {
  if (!pin || !pin.flavor) return null;
  const supported = instances.filter((item) => item.supported);
  const ofFlavor = supported.filter((item) => item.flavorId === pin.flavor);
  if (ofFlavor.length === 0) return null;
  return ofFlavor[pin.ordinal || 0] || ofFlavor[0];
}

export function formatInstance(instance, index) {
  const flavor = instance.flavorLabel || instance.flavorFolder || 'unknown';
  const suffix = instance.supported ? '' : '  (the addon does not serve this build)';
  const hwnd = instance.hwnd ? `0x${instance.hwnd.toString(16)}` : '-';
  const title = instance.title ? `  "${instance.title}"` : '';
  return `  [${index}] ${flavor.padEnd(14)} pid=${String(instance.pid).padEnd(8)} `
    + `hwnd=${hwnd.padEnd(12)}${title}${suffix}`;
}
