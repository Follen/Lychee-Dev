/**
 * Self test for the packaged payload. Runs without network access and without a
 * World of Warcraft install, so CI can verify a release candidate.
 *
 * Usage: node scripts/selftest.mjs
 */

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
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

const repoRoot = path.resolve(packageRoot, '..', '..');

const pkg = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'));
const vendorAddon = path.join(packageRoot, 'vendor', 'addon');
const vendorSkill = path.join(packageRoot, 'vendor', 'skill');

/** `## Version: 1.0.6` in a TOC (the addon release version). */
function tocVersion(text) {
  const match = text.match(/^##\s*Version:\s*(\S+)\s*$/m);
  return match ? match[1] : null;
}

/** `## Interface: 16001` in a TOC (the client build the TOC targets). */
function tocInterface(text) {
  const match = text.match(/^##\s*Interface:\s*(\d+)\s*$/m);
  return match ? match[1] : null;
}

/** `badge/release-v1.0.6-` in a README shields.io badge. */
function readmeVersion(text) {
  const match = text.match(/badge\/release-v(\d+\.\d+\.\d+)/);
  return match ? match[1] : null;
}

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
  ? fs.readdirSync(vendorAddon).filter((name) => name.endsWith('.toc')).sort()
  : [];
// One TOC per supported client. Four as of Forever; a stale count here means a
// client was added or dropped without the payload following.
check('one TOC per supported client shipped', tocFiles.length === 4, tocFiles.join(', '));

let tocEntries = 0;
const tocInterfaces = new Map();
for (const toc of tocFiles) {
  const dir = vendorAddon;
  const text = fs.readFileSync(path.join(dir, toc), 'utf8');
  const listed = text.split(/\r?\n/).map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#'));
  const missing = listed.filter((rel) => !fs.existsSync(path.join(dir, rel.replace(/\\/g, path.sep))));
  check(`${toc}: all ${listed.length} listed files exist`, missing.length === 0, missing.join(', '));
  tocEntries += listed.length;

  const interfaceVersion = tocInterface(text);
  // Two TOCs claiming one Interface would make WoW load both, or the wrong one.
  check(`${toc}: declares a unique Interface`,
    Boolean(interfaceVersion) && !tocInterfaces.has(interfaceVersion),
    interfaceVersion || 'missing');
  if (interfaceVersion) tocInterfaces.set(interfaceVersion, toc);
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
const shippedVersion = mainlineToc
  ? tocVersion(fs.readFileSync(path.join(vendorAddon, mainlineToc), 'utf8'))
  : null;

// Version drift is a release defect, not a cosmetic one: the addon TOC and
// package.json are released together and the git tag must match both. The
// README badges drift silently because nothing else reads them, so assert on
// all three instead of maintaining them by hand.
check('shipped payload declares a version', Boolean(shippedVersion), String(shippedVersion));
check('vendored TOC version matches package.json',
  Boolean(shippedVersion) && shippedVersion === pkg.version,
  `toc ${shippedVersion} vs package ${pkg.version}`);

for (const readme of ['README.md', 'README_zhCN.md']) {
  const file = path.join(repoRoot, readme);
  if (!fs.existsSync(file)) {
    // The npm package does not ship the READMEs, so this only runs from a
    // repository checkout; skip rather than fail a tarball-only test.
    console.log(`  skip ${readme}: not present in this checkout`);
    continue;
  }
  const found = readmeVersion(fs.readFileSync(file, 'utf8'));
  check(`${readme} release badge matches package.json`,
    Boolean(found) && found === pkg.version,
    `readme ${found} vs package ${pkg.version}`);
}

check('skill ships SKILL.md', fs.existsSync(path.join(vendorSkill, 'SKILL.md')));
check('skill ships the automation reference', fs.existsSync(path.join(vendorSkill, 'references', 'automation.md')));
check('skill ships automation.py', fs.existsSync(path.join(vendorSkill, 'scripts', 'automation.py')));
check('skill ships requirements.txt', fs.existsSync(path.join(vendorSkill, 'scripts', 'requirements.txt')));

// A folder name is a location, not an identity: the game reuses a test folder
// for whatever is on the test track, so `_classic_beta_` carries WoW: Forever
// (1.60.x) on that track. Build a synthetic root to prove the folder is
// resolved from the client's own identity rather than its name.
const { scanClients, flavorForClient } = await import('../src/wow.js');

function fakeClientRoot({ folder, flavorInfo, version }) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lychee-root-'));
  const flavorDir = path.join(root, folder);
  fs.mkdirSync(path.join(flavorDir, 'Interface', 'AddOns'), { recursive: true });
  if (flavorInfo) fs.writeFileSync(path.join(flavorDir, '.flavor.info'), flavorInfo, 'utf8');
  if (version) fs.writeFileSync(path.join(flavorDir, 'version.txt'), `${version}\n`, 'utf8');
  return root;
}

function scanOne(options) {
  const root = fakeClientRoot(options);
  try {
    return scanClients(root).find((client) => client.folder === options.folder) || null;
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

const asForever = scanOne({
  folder: '_classic_beta_',
  flavorInfo: 'Product Flavor!STRING:0\nwow_forever\n',
  version: '1.60.1.69893',
});
check('reused test folder resolves to Forever from its product code',
  Boolean(asForever) && asForever.id === 'forever' && asForever.toc === 'Lychee Dev_Forever.toc',
  JSON.stringify(asForever && { id: asForever.id, toc: asForever.toc }));

const asClassicBeta = scanOne({
  folder: '_classic_beta_',
  flavorInfo: 'Product Flavor!STRING:0\nwow_classic_beta\n',
  version: '5.5.0.63000',
});
check('the same folder still resolves to Classic Beta for the MoP test track',
  Boolean(asClassicBeta) && asClassicBeta.id === 'classicBeta' && asClassicBeta.toc === null,
  JSON.stringify(asClassicBeta && { id: asClassicBeta.id, toc: asClassicBeta.toc }));

// No product file: the build version is the fallback signal.
const byVersion = scanOne({ folder: '_classic_beta_', version: '1.60.1.69893' });
check('build version alone is enough to identify Forever',
  Boolean(byVersion) && byVersion.id === 'forever',
  JSON.stringify(byVersion && byVersion.id));

const dedicated = scanOne({
  folder: '_forever_',
  flavorInfo: 'Product Flavor!STRING:0\nwow_forever\n',
  version: '1.60.1.69893',
});
check('a dedicated _forever_ folder is detected too',
  Boolean(dedicated) && dedicated.id === 'forever',
  JSON.stringify(dedicated && dedicated.id));

check('an unsupported folder is not reported as a supported client',
  (flavorForClient({ folder: '_classic_era_', product: 'wow_classic_era' }) || {}).toc === null);

// --- telling same-build windows apart -----------------------------------------
// Two windows of one build are only unusable when they cannot be named. These
// are pure functions, so the rule is pinned here instead of only in the field.
const {
  describeAmbiguity, identityKey, identityLabel, matchIdentity, matchPinned,
} = await import('../src/wow.js');

const window = (pid, identity, flavorId = 'retail') => ({
  pid, hwnd: pid, flavorId, flavorLabel: 'Retail', supported: true, identity,
});

const twoNamed = [
  window(1, { id: '荔枝', realm: '白银之手' }),
  window(2, { id: '测试小号', realm: '白银之手' }),
];
const named = describeAmbiguity(twoNamed);
check('two same-build windows with different characters are distinguishable',
  named.sameBuildNamed === true && named.sameBuildCollision === false,
  JSON.stringify({ named: named.sameBuildNamed, collision: named.sameBuildCollision }));

const twoAnonymous = [window(1, null), window(2, null)];
const anonymous = describeAmbiguity(twoAnonymous);
check('two same-build windows without markers stay ambiguous',
  anonymous.sameBuildCollision === true && anonymous.sameBuildNamed === false,
  JSON.stringify({ collision: anonymous.sameBuildCollision }));

const twoIdentical = [
  window(1, { id: '荔枝', realm: '白银之手' }),
  window(2, { id: '荔枝', realm: '白银之手' }),
];
check('two windows reporting the same character stay ambiguous',
  describeAmbiguity(twoIdentical).sameBuildCollision === true);

check('a bare name resolves to its window',
  (matchIdentity(twoNamed, '测试小号') || {}).instance === twoNamed[1]);
check('a Name-Realm form resolves to its window',
  (matchIdentity(twoNamed, '荔枝-白银之手') || {}).instance === twoNamed[0]);
check('an unknown name is an error, not a guess',
  Boolean(matchIdentity(twoNamed, 'nobody').error));
// Same name on two realms must not silently pick one.
const crossRealm = [
  window(1, { id: '荔枝', realm: 'A' }),
  window(2, { id: '荔枝', realm: 'B' }),
];
check('a bare name matching two realms asks for Name-Realm',
  Boolean(matchIdentity(crossRealm, '荔枝').error)
    && matchIdentity(crossRealm, '荔枝-B').instance === crossRealm[1]);

check('identityKey includes the realm so equal names differ',
  identityKey(crossRealm[0]) !== identityKey(crossRealm[1]));
check('identityLabel renders character-realm',
  identityLabel(twoNamed[0]) === '荔枝-白银之手', String(identityLabel(twoNamed[0])));

// A pin that names the character must survive a reordering of the windows, which
// is exactly what an ordinal-based pin cannot do.
const reordered = [
  window(9, { id: '测试小号', realm: '白银之手' }),
  window(7, { id: '荔枝', realm: '白银之手' }),
];
check('a character pin follows the character across a reorder',
  (matchPinned(reordered, { flavor: 'retail', ordinal: 0, character: identityKey(twoNamed[0]) })
    || {}).pid === 7);
check('an ordinal pin still works for windows with no marker',
  (matchPinned(twoAnonymous, { flavor: 'retail', ordinal: 1 }) || {}).pid === 2);

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

// The CLI forwards `identify` to the vendored helper, so a stale vendor copy
// would silently remove window discovery. Probe it with no target: the helper
// must reject the *arguments*, not the *command name*.
const identify = run(['identify']);
const identifyErr = `${identify.stderr}${identify.stdout}`;
check('vendored helper exposes the identify command',
  !identifyErr.includes('invalid choice'),
  identifyErr.split('\n').find((line) => line.includes('invalid choice')) || '');

console.log(`\n${checks} checks`);
