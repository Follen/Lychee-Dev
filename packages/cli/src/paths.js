/** Shared paths and small filesystem helpers. */

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));

/** Package root (the directory that holds package.json). */
export const packageRoot = path.resolve(here, '..');

/** Vendored payload shipped inside the npm package. */
export const vendorRoot = path.join(packageRoot, 'vendor');

/** The addon folder name WoW expects. The TOC files reference it, so it keeps
 * its display capitalization even though the npm package name is lowercase. */
export const addonFolderName = 'Lychee Dev';

export function homeDir() {
  return process.env.LYCHEEDEV_HOME || os.homedir();
}

export function dataDir() {
  return path.join(homeDir(), '.lycheedev');
}

export function configPath() {
  return path.join(dataDir(), 'config.json');
}

/** pip --target directory that holds the automation helper dependencies. */
export function pythonDepsDir() {
  return path.join(dataDir(), 'python');
}

export function skillsDir() {
  return path.join(homeDir(), '.agents', 'skills');
}

export function skillTargetDir() {
  return path.join(skillsDir(), 'lycheedev');
}

export function exists(target) {
  try {
    fs.statSync(target);
    return true;
  } catch {
    return false;
  }
}

export function isDir(target) {
  try {
    return fs.statSync(target).isDirectory();
  } catch {
    return false;
  }
}

export function readJson(target, fallback = null) {
  try {
    return JSON.parse(fs.readFileSync(target, 'utf8'));
  } catch {
    return fallback;
  }
}

export function writeJson(target, value) {
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
}

/** Copy a directory tree, skipping build noise. */
export function copyTree(from, to, { filter = null } = {}) {
  fs.mkdirSync(to, { recursive: true });
  for (const entry of fs.readdirSync(from, { withFileTypes: true })) {
    const source = path.join(from, entry.name);
    if (filter && !filter(source, entry)) continue;
    const target = path.join(to, entry.name);
    if (entry.isDirectory()) {
      copyTree(source, target, { filter });
    } else if (entry.isFile()) {
      fs.copyFileSync(source, target);
    }
  }
}

/** Drop a directory tree if it is there. */
export function removeTree(target) {
  fs.rmSync(target, { recursive: true, force: true });
}
