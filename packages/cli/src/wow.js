/** Locate World of Warcraft installations and their client flavors. */

import fs from 'node:fs';
import path from 'node:path';

import { addonFolderName, exists, isDir } from './paths.js';

/**
 * Flavor folders the addon supports. `id` is the stable key used by the CLI and
 * by the Python automation profile; `folder` is what Blizzard names it.
 */
export const CLIENT_FLAVORS = [
  { id: 'retail', folder: '_retail_', label: 'Retail (Midnight)' },
  { id: 'classic', folder: '_classic_', label: 'Classic (Mists)' },
  { id: 'titan', folder: '_classic_arena', label: 'Classic Titan (Wrath)' },
  { id: 'classicEra', folder: '_classic_era_', label: 'Classic Era' },
];

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

/** Directories that contain at least one known flavor folder. */
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

/**
 * Describe the flavors present under a root. A given root may hold several
 * (retail + classic), and each gets its own AddOns directory.
 */
export function scanClients(wowRoot) {
  const found = [];
  for (const flavor of CLIENT_FLAVORS) {
    const flavorDir = path.join(wowRoot, flavor.folder);
    const addonsDir = path.join(flavorDir, 'Interface', 'AddOns');
    if (!isDir(flavorDir)) continue;
    found.push({
      ...flavor,
      flavorDir,
      addonsDir,
      addonsDirExists: isDir(addonsDir),
      installed: exists(path.join(addonsDir, addonFolderName, `${addonFolderName}_Mainline.toc`))
        || exists(path.join(addonsDir, addonFolderName, `${addonFolderName}_Mists.toc`))
        || exists(path.join(addonsDir, addonFolderName, `${addonFolderName}_Wrath.toc`)),
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

/** `Lychee Dev_Mainline.toc` style file for a flavor. */
export function tocSuffixFor(flavorId) {
  switch (flavorId) {
    case 'retail':
      return 'Mainline';
    case 'classic':
      return 'Mists';
    case 'titan':
      return 'Wrath';
    default:
      return null;
  }
}
