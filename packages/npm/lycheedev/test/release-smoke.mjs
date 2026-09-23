// Release-shape package smoke (REL-03/06/08/13, PKG-01/02/05/08): audits and
// installs the SEALED npm tarball produced by `node tools/release.mjs assemble`
// and runs every check from the real installed directory. Unlike
// install-smoke.mjs this consumes a finished tgz and never re-packs.
//
//   node packages/npm/lycheedev/test/release-smoke.mjs <lycheedev-x.y.z.tgz> <npm-cli.js> [--report <path>]
import { createHash } from 'node:crypto';
import { existsSync, mkdtempSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { readTgz } from '../../../../tools/tgz.mjs';
import { auditTgz, policyFor, TARGETS } from '../../../../tools/release.mjs';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repository = resolve(packageRoot, '../../..');
const [tgzPath, npmCLI, ...rest] = process.argv.slice(2);
if (!tgzPath || !existsSync(tgzPath) || !npmCLI || !existsSync(npmCLI)) {
  throw new Error('Usage: node release-smoke.mjs <sealed tgz> <npm-cli.js absolute path>; no shell or download fallback is used');
}
const reportPath = rest.includes('--report') ? rest[rest.indexOf('--report') + 1] : undefined;
const target = `${process.platform === 'win32' ? 'windows' : process.platform}-${process.arch === 'x64' ? 'amd64' : process.arch}`;
const hostPlatform = process.platform === 'win32' ? 'windows/amd64' : `${process.platform}/${process.arch === 'x64' ? 'amd64' : process.arch}`;
const root = mkdtempSync(join(tmpdir(), 'lycheedev 发布隔离-'));
const prefix = join(root, 'prefix 空格 中文');
const cache = join(root, 'cache');
const userconfig = join(root, 'empty.npmrc');
writeFileSync(userconfig, '');

function run(binary, args, cwd = root, extraEnv = {}) {
  const result = spawnSync(binary, args, {
    cwd, env: { ...process.env, ...extraEnv }, encoding: 'utf8', shell: false, windowsHide: true,
    timeout: 300000, maxBuffer: 16 * 1024 * 1024,
  });
  if (result.error || result.status !== 0) throw new Error(`${binary}: ${result.error || result.status}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

// 1. Content whitelist audit and metadata of the sealed tarball (PKG-05/REL-08).
const tgzBytes = readFileSync(tgzPath);
const entries = readTgz(tgzBytes);
const manifestEntry = entries.find(entry => entry.name === 'package/release.json');
assert(manifestEntry, 'sealed tgz has no release.json');
const manifest = JSON.parse(Buffer.from(manifestEntry.bytes).toString('utf8'));
const versionRecord = JSON.parse(readFileSync(join(repository, 'release/version.json'), 'utf8'));
const audit = auditTgz(entries, {
  version: versionRecord.version,
  expectedCommit: manifest.commit,
  sourceRoot: packageRoot, // README/LICENSE/THIRD_PARTY_NOTICES byte-identity with source
});
assert.equal(audit.violations.length, 0, `tgz audit violations: ${audit.violations.join('; ')}`);
assert.equal(audit.ok, true);
const policy = policyFor(manifest.version);
const unpackedBytes = entries.reduce((total, entry) => total + entry.bytes.length, 0);

// 2. Fresh isolated install from the actual tgz with install scripts disabled,
//    through paths that contain spaces and Chinese characters (REL-03).
const installStarted = process.hrtime.bigint();
run(process.execPath, [npmCLI, 'install', '--global', '--prefix', prefix, resolve(tgzPath),
  '--userconfig', userconfig, '--cache', cache, '--offline', '--ignore-scripts', '--no-audit', '--no-fund']);
const installMs = Number(process.hrtime.bigint() - installStarted) / 1e6;
const installed = process.platform === 'win32' ? join(prefix, 'node_modules/lycheedev') : join(prefix, 'lib/node_modules/lycheedev');
const launcher = join(installed, 'bin/lycheedev.mjs');

// 3. Notices, README and LICENSE must survive installation byte-identically.
for (const name of ['THIRD_PARTY_NOTICES', 'README.md', 'LICENSE']) {
  assert(existsSync(join(installed, name)), `installed package is missing ${name}`);
  assert.deepEqual(readFileSync(join(installed, name)), readFileSync(join(packageRoot, name)), `${name} is not byte-identical to source`);
}

// 4. Per-platform launcher selection and binary content integrity from the
//    real installed directory (PKG-01/02).
const launcherModule = await import(`${pathToFileURL(launcher).href}?release-smoke`);
const launcherSelection = {};
for (const entry of TARGETS) {
  const platform = entry.target.startsWith('windows') ? 'win32' : entry.target.split('-')[0];
  const arch = entry.target.endsWith('arm64') ? 'arm64' : 'x64';
  const resolved = launcherModule.nativePath(installed, platform, arch);
  assert.equal(resolved, join(installed, ...entry.binary.split('/')), `launcher selected the wrong binary for ${entry.target}`);
  const bytes = readFileSync(resolved);
  const record = manifest.binaries[entry.target];
  assert.equal(bytes.length, record.bytes, `binary size mismatch ${entry.target}`);
  assert.equal(sha256(bytes), record.sha256, `binary digest mismatch ${entry.target} (REL-06)`);
  launcherSelection[entry.target] = { path: entry.binary, bytes: bytes.length, sha256: record.sha256 };
}
assert.throws(() => launcherModule.nativePath(installed, 'win32', 'arm64'), /unsupported_platform/);
const hostBinary = join(installed, ...TARGETS.find(entry => entry.target === target).binary.split('/'));
const executableBit = process.platform === 'win32' ? null : (statSync(hostBinary).mode & 0o111) !== 0;
if (process.platform !== 'win32') assert.equal(executableBit, true, 'Unix binary lost its executable bit in the installed package');

// 5. version and offline business smoke from the INSTALLED entry (PKG-02).
const version = JSON.parse(run(process.execPath, [launcher, 'version', '--format=json']));
assert.equal(version.ok, true);
assert.equal(version.result.version, manifest.version, 'installed version mismatch (REL-01)');
assert.equal(version.result.commit, manifest.commit, 'installed commit mismatch (REL-05)');
assert.equal(version.result.platform, hostPlatform);
const describe = JSON.parse(run(process.execPath, [launcher, 'describe', '--format=json']));
assert.equal(describe.ok, true);
assert(describe.result.commands.some(command => command.path === 'data hotfix'));

const home = join(root, 'fresh-home 空格');
const initialized = JSON.parse(run(process.execPath, [launcher, 'init', '--home', home, '--format=json']));
assert.equal(initialized.ok, true);
for (const command of [['target', 'show', 'PIN-missing'], ['evidence', 'show', 'CAP-missing'], ['live', 'status', 'OP-missing']]) {
  const read = spawnSync(process.execPath, [launcher, ...command, '--home', home, '--format=json'], {
    cwd: root, encoding: 'utf8', shell: false, windowsHide: true, timeout: 60000,
  });
  assert.ifError(read.error);
  assert.notEqual(read.status, 0);
  const result = JSON.parse(read.stdout);
  assert.equal(result.error.code, 'vault.record_missing');
  assert.equal(result.result, null);
}
const selectionFile = join(root, 'selection.json');
writeFileSync(selectionFile, JSON.stringify({ data: {
  product: 'retail', region: 'cn', language: 'zhCN', fullBuild: '12.1.0.69875',
  buildConfig: 'a'.repeat(32), cdnConfig: 'b'.repeat(32), definitionCommit: 'c'.repeat(40),
} }));
const pin = JSON.parse(run(process.execPath, [launcher, 'target', 'resolve', '--file', selectionFile, '--home', home, '--format=json'])).result.id;
const cacheFile = join(root, 'DBCache.bin');
const bytes = Buffer.alloc(44 + 32 + 2);
bytes.write('XFTH'); bytes.writeUInt32LE(9, 4); bytes.writeUInt32LE(69875, 8);
bytes.write('XFTH', 44); bytes.writeUInt32LE(5, 48);
bytes.writeUInt32LE(0xabcdef01, 60); bytes.writeUInt32LE(42, 64);
bytes.writeUInt32LE(2, 68); bytes[72] = 1; bytes[77] = 0xff;
writeFileSync(cacheFile, bytes);
const hotfix = JSON.parse(run(process.execPath, [launcher, 'data', 'hotfix', '--source', 'dbcache', '--snapshot', pin, '--dbcache', cacheFile, '--home', home, '--format=json']));
assert.equal(hotfix.ok, true);
assert.equal(hotfix.result.page.entries[0].payloadHex, '00ff');
for (const capture of hotfix.captures) {
  assert.equal(JSON.parse(run(process.execPath, [launcher, 'evidence', 'verify', capture.id, '--home', home, '--format=json'])).ok, true);
}
const business = { status: 'passed', scope: 'installed CLI target pinning, synthetic local Hotfix decode and evidence verification; no network', pin };

// 6. PKG-08: Linux/macOS live must report unsupported, never fake success.
let liveSupport = { status: 'not_applicable', scope: 'host is Windows' };
if (process.platform !== 'win32') {
  const attempt = spawnSync(process.execPath, [launcher, 'live', 'instances', '--home', home, '--format=json'], {
    cwd: root, encoding: 'utf8', shell: false, windowsHide: true, timeout: 60000,
  });
  assert.ifError(attempt.error);
  const result = JSON.parse(attempt.stdout);
  assert.equal(result.ok, false, 'live discovery faked success on a platform without native capture');
  assert.equal(result.error.code, 'desktop.unsupported_platform', `live must report desktop.unsupported_platform, got ${result.error.code}`);
  assert.notEqual(attempt.status, 0);
  liveSupport = { status: 'passed', scope: 'live reports desktop.unsupported_platform and exits non-zero; no fake success' };
}

const report = {
  scope: 'sealed release tarball content audit, isolated --ignore-scripts install and offline business smoke; no game execution',
  target, root, tgz: resolve(tgzPath), sha256: sha256(tgzBytes), version: manifest.version, commit: manifest.commit,
  correspondingSource: manifest.correspondingSource, audit, entryCount: audit.entryCount,
  compressedBytes: tgzBytes.length, unpackedBytes, installMs: Math.round(installMs), // REL-14 measurements
  policy: { development: policy.development, npmDistTag: policy.npmDistTag },
  ignoreScripts: true, executableBit, launcherSelection, versionOutput: version.result, describeCommands: describe.result.commands.length,
  initialized, business, liveSupport,
};
if (reportPath) writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`);
if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `report=${reportPath ?? join(root, 'release-smoke-report.json')}\n`);
if (!reportPath) writeFileSync(join(root, 'release-smoke-report.json'), `${JSON.stringify(report, null, 2)}\n`);
process.stdout.write(`${JSON.stringify({
  status: 'passed', target, sha256: report.sha256, compressedBytes: tgzBytes.length, unpackedBytes, installMs: report.installMs,
})}\n`);
