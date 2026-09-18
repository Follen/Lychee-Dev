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
 *
 * A folder name is a location, not an identity: the game reuses a test folder
 * for whatever is on the test track. `_classic_beta_` currently carries WoW:
 * Forever (`1.60.1`) for players on that track, so the folder alone cannot
 * decide the product. `resolveProduct` reads the client's own `.flavor.info`
 * product code first and falls back to the build version, and only then to the
 * folder default recorded here.
 */
export const CLIENT_FLAVORS = [
  { id: 'retail', folder: '_retail_', label: 'Retail', exe: 'Wow.exe', toc: 'Lychee Dev_Mainline.toc', product: 'wow', buildPrefix: null },
  { id: 'classic', folder: '_classic_', label: 'Classic', exe: 'WowClassic.exe', toc: 'Lychee Dev_Mists.toc', product: 'wow_classic', buildPrefix: null },
  { id: 'titan', folder: '_classic_titan_', label: 'Classic Titan', exe: 'WowClassic.exe', toc: 'Lychee Dev_Wrath.toc', product: 'wow_classic_titan', buildPrefix: null },
  // The Forever test track. Listed for both layouts so either one is detected.
  { id: 'forever', folder: '_classic_beta_', label: 'Forever', exe: 'WowClassic.exe', toc: 'Lychee Dev_Forever.toc', product: 'wow_forever', buildPrefix: '1.60.' },
  { id: 'forever', folder: '_forever_', label: 'Forever', exe: 'WowClassic.exe', toc: 'Lychee Dev_Forever.toc', product: 'wow_forever', buildPrefix: '1.60.' },
  // The MoP-era classic test track historically used the same `_classic_beta_`
  // folder, so keep it known and explain it instead of hiding the folder.
  { id: 'classicBeta', folder: '_classic_beta_', label: 'Classic Beta', exe: 'WowClassic.exe', toc: null, product: 'wow_classic_beta', buildPrefix: '5.5.' },
  { id: 'classicEra', folder: '_classic_era_', label: 'Classic Era', exe: 'WowClassic.exe', toc: null, product: 'wow_classic_era', buildPrefix: null },
  { id: 'anniversary', folder: '_anniversary_', label: 'Anniversary', exe: 'WowClassic.exe', toc: null, product: 'wow_anniversary', buildPrefix: null },
  { id: 'beta', folder: '_beta_', label: 'Beta', exe: 'WowB.exe', toc: null, product: 'wow_beta', buildPrefix: null },
];

/** Flavors with an addon catalog, i.e. the builds the addon can install into. */
export function supportedFlavors() {
  return CLIENT_FLAVORS.filter((flavor) => flavor.toc);
}

/**
 * The client's own product code, read from the `.flavor.info` file the launcher
 * writes next to the executable. It is ground truth for which product a folder
 * actually holds, and the only signal that stays correct when Blizzard reuses a
 * folder name. Returns null when the file is absent or unreadable.
 */
export function readFlavorProduct(flavorDir) {
  for (const name of ['.flavor.info', 'flavor.info']) {
    const file = path.join(flavorDir, name);
    if (!exists(file)) continue;
    let text;
    try {
      text = fs.readFileSync(file, 'utf8');
    } catch {
      continue;
    }
    const match = text.match(/\b(wow[a-z0-9_]*)\b/i);
    if (match) return match[1].toLowerCase();
  }
  return null;
}

/**
 * The product a flavor folder actually holds. The client's declared product
 * wins; then the build version, because a reused folder still reports its own
 * version; then the folder default.
 */
export function resolveProduct(flavor, flavorDir, version) {
  return readFlavorProduct(flavorDir)
    || (version && flavor.buildPrefix && version.startsWith(flavor.buildPrefix) ? flavor.product : null)
    || flavor.product;
}

/**
 * Pick the flavor entry that matches a client's real identity: the product, or
 * the build when the product is unknown, and only then the folder default.
 */
export function flavorForClient({ folder, product = null, version = null }) {
  const byFolder = CLIENT_FLAVORS.filter((flavor) => flavor.folder === folder);
  if (product) {
    const exact = byFolder.find((flavor) => flavor.product === product);
    if (exact) return exact;
  }
  if (version) {
    const byBuild = byFolder.find((flavor) => flavor.buildPrefix && version.startsWith(flavor.buildPrefix));
    if (byBuild) return byBuild;
  }
  return byFolder[0] || null;
}

/** Every folder name the CLI recognises, for error messages and documentation. */
export function knownFolders() {
  return [...new Set(CLIENT_FLAVORS.map((flavor) => flavor.folder))];
}

export function flavorById(id) {
  return CLIENT_FLAVORS.find((flavor) => flavor.id === id) || null;
}

/**
 * The first flavor registered for a folder. A folder may have several entries
 * (see `_classic_beta_`), so use `flavorForClient` when the client's own
 * product or build is known.
 */
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
 * Every flavor folder's executable version under one root, in a single call.
 *
 * The version has to come from the executable's file metadata, which needs
 * PowerShell on Windows, and starting it once per client costs about a second on
 * a full install. One invocation returns the whole map, so the cost is paid once
 * no matter how many clients are present.
 */
const FLAVOR_EXE_CANDIDATES = ['Wow.exe', 'WowClassic.exe', 'WowB.exe'];

export function readExecutableVersions(wowRoot) {
  const dirs = [...new Set(CLIENT_FLAVORS.map((flavor) => flavor.folder))];
  const script = [
    '$ErrorActionPreference = "SilentlyContinue"',
    '$root = $env:LYCHEEDEV_VERSIONS_ROOT',
    '$names = $env:LYCHEEDEV_VERSIONS_FOLDERS -split ","',
    '$exes = @("Wow.exe", "WowClassic.exe", "WowB.exe")',
    'foreach ($name in $names) {',
    '  foreach ($exe in $exes) {',
    '    $path = Join-Path $root ($name + "\\" + $exe)',
    '    if (Test-Path -LiteralPath $path) {',
    '      $version = (Get-Item -LiteralPath $path).VersionInfo.FileVersion',
    '      Write-Output ($name + "`t" + $version)',
    '      break',
    '    }',
    '  }',
    '}',
  ].join('\n');

  const versions = new Map();
  if (process.platform !== 'win32') return versions;
  try {
    // The root and folder list travel as environment variables: passing them as
    // arguments to `-Command` lets PowerShell reinterpret a Windows path, which
    // silently yields an empty map and a much slower per-client fallback.
    const out = execFileSync('powershell', [
      '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', script,
    ], {
      encoding: 'utf8',
      windowsHide: true,
      timeout: 60000,
      env: {
        ...process.env,
        LYCHEEDEV_VERSIONS_ROOT: wowRoot,
        LYCHEEDEV_VERSIONS_FOLDERS: dirs.join(','),
      },
    });
    for (const line of out.replace(/^\uFEFF/, '').split(/\r?\n/)) {
      const [folder, version] = line.split('\t');
      if (!folder || !version) continue;
      versions.set(folder.trim(), version.trim());
    }
  } catch {
    return versions;
  }
  return versions;
}

/**
 * Describe every flavor present under a root, including builds the addon does
 * not serve, so `doctor` can explain them instead of ignoring them silently.
 */
export function scanClients(wowRoot) {
  const versions = readExecutableVersions(wowRoot);
  const found = [];
  const seenDirs = new Set();
  for (const flavor of CLIENT_FLAVORS) {
    const flavorDir = path.join(wowRoot, flavor.folder);
    if (!isDir(flavorDir)) continue;
    // Two entries can share one folder; describe that folder only once.
    if (seenDirs.has(flavorDir)) continue;
    seenDirs.add(flavorDir);

    const addonsDir = path.join(flavorDir, 'Interface', 'AddOns');
    const exePath = path.join(flavorDir, flavor.exe);
    const hasExe = exists(exePath);
    // The batch result is keyed by folder; fall back to a direct read for an
    // install whose executable name does not match the recorded one.
    const version = versions.get(flavor.folder)
      || (hasExe ? exeVersion(exePath) : null);
    // Which product this folder really holds decides id, label and catalog.
    const product = resolveProduct(flavor, flavorDir, version);
    const resolved = flavorForClient({ folder: flavor.folder, product, version }) || flavor;

    found.push({
      ...flavor,
      ...resolved,
      flavorDir,
      addonsDir,
      addonsDirExists: isDir(addonsDir),
      exePath: hasExe ? exePath : null,
      version,
      folderProduct: product,
      installedDir: path.join(addonsDir, addonFolderName),
      installed: resolved.toc ? exists(path.join(addonsDir, addonFolderName, resolved.toc)) : false,
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
 * name cannot identify a build. The executable path narrows it to an install
 * folder, but a folder is a location rather than an identity: the helper also
 * reports the folder's `.flavor.info` product and `version.txt` build, and
 * those decide which client the window actually is.
 *
 * The answer is memoized for the life of the process. A single command resolves
 * the target more than once (once to pick the window, once to forward), and
 * re-enumerating would both pay for another interpreter start and risk the two
 * lookups disagreeing about which windows exist.
 */
let instancesCache = null;

export function findInstances({ pythonBin = null, refresh = false } = {}) {
  if (!refresh && instancesCache !== null) return instancesCache;
  instancesCache = enumerateInstances(pythonBin);
  return instancesCache;
}

function enumerateInstances(pythonBin) {
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
      const resolved = flavorForClient({
        folder: row.flavorFolder,
        product: row.product || null,
        version: row.version || null,
      });
      return {
        ...row,
        flavorId: resolved ? resolved.id : row.flavorId,
        flavorLabel: resolved ? resolved.label : row.flavorLabel,
        supported: Boolean(resolved && resolved.toc),
      };
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
  // A pin taken while the windows carried identity markers names the character,
  // which survives the window being closed and reopened far better than an
  // ordinal does. Fall back to the ordinal for pins saved before identity was
  // available, and for builds whose windows show no marker.
  if (pin.character) {
    const named = ofFlavor.filter((item) => identityKey(item) === pin.character);
    if (named.length === 1) return named[0];
  }
  return ofFlavor[pin.ordinal || 0] || ofFlavor[0];
}

export function formatInstance(instance, index) {
  const flavor = instance.flavorLabel || instance.flavorFolder || 'unknown';
  const suffix = instance.supported ? '' : '  (the addon does not serve this build)';
  const hwnd = instance.hwnd ? `0x${instance.hwnd.toString(16)}` : '-';
  const version = instance.version ? `  v${instance.version}` : '';
  const title = instance.title ? `  "${instance.title}"` : '';
  const line = `  [${index}] ${flavor.padEnd(14)} pid=${String(instance.pid).padEnd(8)} `
    + `hwnd=${hwnd.padEnd(12)}${version}${title}${suffix}`;
  // Identity comes from the in-game marker when the caller asked for it; it is
  // the only thing that says which character is behind the window.
  const identity = instance.identity;
  if (!identity) return line;
  const name = identity.realm ? `${identity.id}-${identity.realm}` : identity.id;
  return `${line}\n        character: ${name}`
    + `${identity.client ? `  client: ${identity.client}` : ''}`
    + `${identity.build ? `  build: ${identity.build}` : ''}`;
}

/**
 * A stable key for the character a window reports, or null when it reported none.
 *
 * The identity marker is the only thing that says which character is behind a
 * window, so it is what makes two same-build windows distinguishable. Realm is
 * part of the key because two realms can carry a character with the same name.
 */
export function identityKey(instance) {
  const identity = instance && instance.identity;
  if (!identity || !identity.id) return null;
  return `${identity.id}\u0000${identity.realm || ''}`;
}

/** The `character-realm` label shown to the user, or null without a marker. */
export function identityLabel(instance) {
  const identity = instance && instance.identity;
  if (!identity || !identity.id) return null;
  return identity.realm ? `${identity.id}-${identity.realm}` : identity.id;
}

/**
 * Decide whether several running instances can be told apart.
 *
 * Different builds are safe: the install path identifies them, so `--client`
 * picks one. Several windows of the *same* build are only a problem when they
 * cannot be named: if each window shows an identity marker with a distinct
 * character, the user can choose one by name. Identical characters — or windows
 * that showed no marker — remain genuinely indistinguishable, and pinning an
 * ordinal there would only look precise while risking a command in the wrong
 * game, so the caller still asks for one window.
 */
export function describeAmbiguity(instances) {
  const supported = instances.filter((item) => item.supported);
  const byBuild = new Map();
  for (const instance of supported) {
    if (!byBuild.has(instance.flavorId)) byBuild.set(instance.flavorId, []);
    byBuild.get(instance.flavorId).push(instance);
  }
  const duplicates = [...byBuild.entries()].filter(([, list]) => list.length > 1);

  // Within each colliding build, group the windows by the character they report.
  // Every window without a marker shares one anonymous group: a pid is not
  // something the user can recognise or confirm, so two unmarked windows of one
  // build must stay ambiguous rather than looking separable.
  const collisions = [];
  for (const [flavorId, list] of duplicates) {
    const byCharacter = new Map();
    for (const instance of list) {
      const key = identityKey(instance) || '\u0000anonymous';
      if (!byCharacter.has(key)) byCharacter.set(key, []);
      byCharacter.get(key).push(instance);
    }
    const sameCharacter = [...byCharacter.values()].filter((group) => group.length > 1);
    collisions.push({ flavorId, instances: list, byCharacter, sameCharacter });
  }

  return {
    supported,
    duplicates,
    collisions,
    // Several builds open: picking one is unambiguous.
    multipleBuilds: byBuild.size > 1,
    // Same build, but every window carries its own character: the user can pick.
    sameBuildNamed: collisions.some((entry) => entry.sameCharacter.length === 0
      && entry.byCharacter.size > 1),
    // Same build and no way to separate them: still ask for a single window.
    sameBuildCollision: collisions.some((entry) => entry.sameCharacter.length > 0),
  };
}

/**
 * Resolve the character a user named to the window that reports it.
 *
 * Accepts `Name-Realm` or a bare `Name`. A bare name that matches several
 * windows is ambiguous and returns the candidates instead of guessing.
 */
export function matchIdentity(instances, query) {
  const wanted = String(query || '').trim().toLowerCase();
  if (!wanted) return { error: 'no character name given' };
  const supported = instances.filter((item) => item.supported);
  const matches = supported.filter((instance) => {
    const label = identityLabel(instance);
    if (!label) return false;
    const lower = label.toLowerCase();
    return lower === wanted || lower.split('-')[0] === wanted;
  });
  if (matches.length === 0) {
    return { error: `no running window reports the character '${query}'`, candidates: supported };
  }
  if (matches.length > 1) {
    return { error: `${matches.length} windows report '${query}'; use Name-Realm`, candidates: matches };
  }
  return { instance: matches[0] };
}
