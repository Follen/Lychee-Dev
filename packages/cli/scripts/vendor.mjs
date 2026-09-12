/**
 * Stage the shipped payload into `vendor/` so the npm package is self-contained.
 *
 * The addon copy mirrors the release ZIP rules in AGENTS.md: runtime files only,
 * never tests, tools, docs or investigation data. The skill copy is the exact
 * source that gets installed into ~/.agents/skills.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(here, '..');
const repoRoot = path.resolve(packageRoot, '..', '..');

const addonSource = path.join(repoRoot, 'add-on');
const skillSource = path.join(repoRoot, 'Lychee Dev skill');
const vendorRoot = path.join(packageRoot, 'vendor');

const ADDON_EXCLUDE = new Set(['tests', 'tools', 'docs', 'publish', 'Analyze', '__pycache__']);
const SKILL_EXCLUDE = new Set(['__pycache__', 'scripts', 'references']);

function rm(target) {
  fs.rmSync(target, { recursive: true, force: true });
}

function copyFiltered(from, to, excludeDirs) {
  fs.mkdirSync(to, { recursive: true });
  for (const entry of fs.readdirSync(from, { withFileTypes: true })) {
    if (entry.name.startsWith('.') && entry.name !== '.gitattributes') continue;
    if (entry.isDirectory() && excludeDirs.has(entry.name)) continue;
    if (entry.isDirectory() && entry.name === '__pycache__') continue;
    const source = path.join(from, entry.name);
    const target = path.join(to, entry.name);
    if (entry.isDirectory()) {
      copyFiltered(source, target, excludeDirs);
    } else if (entry.isFile()) {
      if (entry.name.endsWith('.pyc')) continue;
      fs.copyFileSync(source, target);
    }
  }
}

/** The skill ships with its scripts and references so the CLI can call them. */
function copySkill(from, to) {
  fs.mkdirSync(to, { recursive: true });
  for (const entry of fs.readdirSync(from, { withFileTypes: true })) {
    if (entry.name === '__pycache__') continue;
    const source = path.join(from, entry.name);
    const target = path.join(to, entry.name);
    if (entry.isDirectory()) {
      copySkill(source, target);
    } else if (entry.isFile()) {
      if (entry.name.endsWith('.pyc')) continue;
      fs.copyFileSync(source, target);
    }
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

function main() {
  for (const required of [addonSource, skillSource]) {
    if (!fs.existsSync(required)) {
      console.error(`vendor: missing source directory ${required}`);
      process.exit(1);
    }
  }

  rm(vendorRoot);

  const addonTarget = path.join(vendorRoot, 'addon');
  copyFiltered(addonSource, addonTarget, ADDON_EXCLUDE);

  const skillTarget = path.join(vendorRoot, 'skill');
  copySkill(skillSource, skillTarget);

  // The registry must ship empty: a task block is local investigation data.
  const registry = path.join(addonTarget, 'Modules', 'Automation', 'auto', 'auto.lua');
  if (fs.existsSync(registry)) {
    const text = fs.readFileSync(registry, 'utf8');
    if (/^-- BEGIN LYCHEE DEV TASK /m.test(text)) {
      console.error('vendor: the task registry contains local task blocks; refusing to ship it');
      console.error('vendor: remove them with the skill tooling before packaging');
      process.exit(1);
    }
  }

  console.log(`vendor: addon ${countFiles(addonTarget)} files -> ${path.relative(packageRoot, addonTarget)}`);
  console.log(`vendor: skill ${countFiles(skillTarget)} files -> ${path.relative(packageRoot, skillTarget)}`);
}

main();
