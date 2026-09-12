/** Install the addon payload into a client's Interface/AddOns directory. */

import fs from 'node:fs';
import path from 'node:path';

import { addonFolderName, copyTree, exists, removeTree, vendorRoot } from './paths.js';

export function vendoredAddonRoot() {
  return path.join(vendorRoot, 'addon');
}

export function addonTargetDir(addonsDir) {
  return path.join(addonsDir, addonFolderName);
}

/** Read the shipped version out of the vendored TOC. */
export function payloadVersion() {
  const toc = path.join(vendoredAddonRoot(), `${addonFolderName}_Mainline.toc`);
  try {
    const text = fs.readFileSync(toc, 'utf8');
    const match = text.match(/^##\s*Version:\s*(.+)$/m);
    return match ? match[1].trim() : null;
  } catch {
    return null;
  }
}

/** Version of whatever is currently installed, if any. */
export function installedVersion(addonsDir) {
  const target = addonTargetDir(addonsDir);
  for (const toc of [`${addonFolderName}_Mainline.toc`, `${addonFolderName}_Mists.toc`,
    `${addonFolderName}_Wrath.toc`]) {
    const file = path.join(target, toc);
    if (!exists(file)) continue;
    const match = fs.readFileSync(file, 'utf8').match(/^##\s*Version:\s*(.+)$/m);
    if (match) return match[1].trim();
  }
  return null;
}

export function countFiles(dir) {
  if (!exists(dir)) return 0;
  let total = 0;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) total += countFiles(path.join(dir, entry.name));
    else total += 1;
  }
  return total;
}

/**
 * Copy the vendored addon over the installed copy.
 *
 * The destination is replaced rather than merged so a file removed upstream
 * cannot linger. `auto.lua` is local task data, so it is preserved when the
 * caller asks for it: the empty shipped registry must not wipe pending work.
 */
export function installAddon(addonsDir, { preserveRegistry = true } = {}) {
  const source = vendoredAddonRoot();
  if (!exists(source)) {
    return { ok: false, error: 'vendored addon payload is missing; reinstall the package' };
  }
  const target = addonTargetDir(addonsDir);

  let savedRegistry = null;
  const registry = path.join(target, 'Modules', 'Automation', 'auto', 'auto.lua');
  if (preserveRegistry && exists(registry)) {
    const text = fs.readFileSync(registry, 'utf8');
    if (/^-- BEGIN LYCHEE DEV TASK /m.test(text)) savedRegistry = text;
  }

  removeTree(target);
  fs.mkdirSync(addonsDir, { recursive: true });
  copyTree(source, target);

  if (savedRegistry) {
    fs.writeFileSync(registry, savedRegistry, 'utf8');
  }

  return {
    ok: true,
    target,
    files: countFiles(target),
    version: payloadVersion(),
    keptRegistry: Boolean(savedRegistry),
  };
}
