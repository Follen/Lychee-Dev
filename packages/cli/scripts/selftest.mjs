/**
 * Self test for the packaged payload. Runs without network access and without a
 * World of Warcraft install, so CI can verify a release candidate.
 *
 * Usage: node scripts/selftest.mjs
 */

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(here, '..');

let checks = 0;
function check(label, condition, detail = '') {
  checks += 1;
  if (condition) {
    console.log(`  ok   ${label}`);
    return true;
  }
  console.log(`  FAIL ${label}${detail ? ` - ${detail}` : ''}`);
  process.exitCode = 1;
  return false;
}

function run(args) {
  return spawnSync(process.execPath, [path.join(packageRoot, 'bin', 'lycheedev.js'), ...args], {
    encoding: 'utf8',
    windowsHide: true,
  });
}

const pkg = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'));
const vendorAddon = path.join(packageRoot, 'vendor', 'addon');
const vendorSkill = path.join(packageRoot, 'vendor', 'skill');

console.log('lycheedev selftest\n');

check('package name matches the published name', pkg.name === 'lycheedev', pkg.name);
check('bin entry is defined', Boolean(pkg.bin && pkg.bin.lycheedev), JSON.stringify(pkg.bin));
check('bin target exists', fs.existsSync(path.join(packageRoot, pkg.bin.lycheedev)));
check('npm package stays dependency-free', !pkg.dependencies || Object.keys(pkg.dependencies).length === 0);
check('vendor/ is published', Array.isArray(pkg.files) && pkg.files.includes('vendor/'));
check('node engine floor is declared', Boolean(pkg.engines && pkg.engines.node));

check('vendored addon is present', fs.existsSync(vendorAddon));
check('vendored skill is present', fs.existsSync(vendorSkill));

// Every TOC listed in the addon payload must exist: a missing file is a broken
// install that WoW reports only as a Lua error at load time.
const tocFiles = fs.existsSync(vendorAddon)
  ? fs.readdirSync(vendorAddon).filter((name) => name.endsWith('.toc'))
  : [];
check('three client TOCs shipped', tocFiles.length === 3, tocFiles.join(', '));

let tocEntries = 0;
for (const toc of tocFiles) {
  const dir = vendorAddon;
  const text = fs.readFileSync(path.join(dir, toc), 'utf8');
  const listed = text.split(/\r?\n/).map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#'));
  const missing = listed.filter((rel) => !fs.existsSync(path.join(dir, rel.replace(/\\/g, path.sep))));
  check(`${toc}: all ${listed.length} listed files exist`, missing.length === 0, missing.join(', '));
  tocEntries += listed.length;
}

// The registry is local task data; a release must never carry it.
const registry = path.join(vendorAddon, 'Modules', 'Automation', 'auto', 'auto.lua');
if (fs.existsSync(registry)) {
  const text = fs.readFileSync(registry, 'utf8');
  check('task registry ships empty', !/^-- BEGIN LYCHEE DEV TASK /m.test(text));
  check('task registry declares the table', text.includes('ns.AutomationTaskDefinitions'));
} else {
  check('task registry is present', false, registry);
}

const mainlineToc = tocFiles.find((name) => name.includes('Mainline')) || tocFiles[0];
check('shipped payload declares a version', Boolean(mainlineToc)
  && /^##\s*Version:\s*\S+/m.test(fs.readFileSync(path.join(vendorAddon, mainlineToc), 'utf8')));

check('skill ships SKILL.md', fs.existsSync(path.join(vendorSkill, 'SKILL.md')));
check('skill ships the automation reference', fs.existsSync(path.join(vendorSkill, 'references', 'automation.md')));
check('skill ships automation.py', fs.existsSync(path.join(vendorSkill, 'scripts', 'automation.py')));
check('skill ships requirements.txt', fs.existsSync(path.join(vendorSkill, 'scripts', 'requirements.txt')));

const version = run(['--version']);
check('cli --version works', version.status === 0 && version.stdout.trim() === pkg.version,
  `${version.status}: ${version.stdout.trim()}`);

const help = run(['--help']);
check('cli --help lists install', help.status === 0 && help.stdout.includes('install'));

const doctor = run(['doctor']);
check('cli doctor runs without a config', doctor.status === 0 || doctor.status === 1,
  `status ${doctor.status}`);

const unknown = run(['nope']);
check('unknown command exits non-zero', unknown.status !== 0);

console.log(`\n${checks} checks`);
