/** Install the vendored skill into ~/.agents/skills/lycheedev. */

import fs from 'node:fs';
import path from 'node:path';

import { copyTree, exists, removeTree, skillTargetDir, vendorRoot } from './paths.js';

export function vendoredSkillRoot() {
  return path.join(vendorRoot, 'skill');
}

export function skillVersion() {
  const skillFile = path.join(vendoredSkillRoot(), 'SKILL.md');
  try {
    const text = fs.readFileSync(skillFile, 'utf8');
    const match = text.match(/^version:\s*(.+)$/m);
    return match ? match[1].trim() : null;
  } catch {
    return null;
  }
}

function countFiles(dir) {
  let total = 0;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) total += countFiles(path.join(dir, entry.name));
    else total += 1;
  }
  return total;
}

/**
 * Replace the installed skill with the vendored copy.
 *
 * The skill is replaced rather than merged: a stale reference file that no
 * longer ships would otherwise stay discoverable forever.
 */
export function installSkill() {
  const source = vendoredSkillRoot();
  if (!exists(source)) {
    return { ok: false, error: 'vendored skill payload is missing; reinstall the package' };
  }
  const target = skillTargetDir();
  removeTree(target);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  copyTree(source, target);
  return { ok: true, target, files: countFiles(target) };
}
