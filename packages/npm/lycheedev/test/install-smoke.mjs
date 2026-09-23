// Development-only host package smoke. This is not the five-platform release
// assembly or release gate: one host binary plus current addon/skill sources.
import { createHash } from 'node:crypto';
import { appendFileSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repository = resolve(packageRoot, '../../..');
const npmCLI = process.argv[2];
if (!npmCLI || !existsSync(npmCLI)) throw new Error('Pass the absolute npm-cli.js path; no shell or download fallback is used');
const target = `${process.platform === 'win32' ? 'windows' : process.platform}-${process.arch === 'x64' ? 'amd64' : process.arch}`;
if (!['windows-amd64', 'linux-amd64', 'linux-arm64', 'darwin-amd64', 'darwin-arm64'].includes(target)) throw new Error('unsupported test host');
const root = mkdtempSync(join(tmpdir(), 'lycheedev npm 隔离-'));
const stage = join(root, 'package');
const prefix = join(root, 'prefix');
const cache = join(root, 'cache');
const userconfig = join(root, 'empty.npmrc');
writeFileSync(userconfig, '');
mkdirSync(join(stage, 'bin'), { recursive: true });
mkdirSync(join(stage, 'native', target), { recursive: true });
copyFileSync(join(packageRoot, 'package.json'), join(stage, 'package.json'));
copyFileSync(join(packageRoot, 'THIRD_PARTY_NOTICES'), join(stage, 'THIRD_PARTY_NOTICES'));
copyFileSync(join(packageRoot, 'bin/lycheedev.mjs'), join(stage, 'bin/lycheedev.mjs'));
const sourceThirdPartyNotices = readFileSync(join(packageRoot, 'THIRD_PARTY_NOTICES'));
const manifest = JSON.parse(readFileSync(join(stage, 'package.json'), 'utf8'));
// Version and privacy come from the single version source; a -dev version is
// the only private shape (tools/release.mjs policyFor).
const sourceVersion = JSON.parse(readFileSync(join(repository, 'release/version.json'), 'utf8')).version;
const { policyFor } = await import('../../../../tools/release.mjs');
const policy = policyFor(sourceVersion);
assert.equal(manifest.version, sourceVersion);
assert.equal(manifest.private === true, policy.privateMustBeTrue);
function run(binary, args, cwd = root, extraEnv = {}) {
  const result = spawnSync(binary, args, { cwd, env: { ...process.env, ...extraEnv }, encoding: 'utf8', shell: false, windowsHide: true, timeout: 180000, maxBuffer: 4 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`${binary}: ${result.error || result.status}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}
const binaryPath = `native/${target}/lycheedev${process.platform === 'win32' ? '.exe' : ''}`;
run(process.execPath, ['tools/version.mjs', '--check'], repository);
const commit = run('git', ['rev-parse', 'HEAD'], repository).trim();
assert.match(commit, /^[a-f0-9]{40}$/);
const workspaceDirty = run('git', ['status', '--porcelain'], repository).trim().length > 0;
run('go', ['build', '-buildvcs=true', '-trimpath', '-o', join(stage, binaryPath), './cmd/lycheedev'], repository, { CGO_ENABLED: '0' });
const binaryBytes = readFileSync(join(stage, binaryPath));
const resources = [];
function stageResources(source, relative) {
  for (const entry of readdirSync(source, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const name = `${relative}/${entry.name}`;
    assert(!entry.isSymbolicLink(), `linked payload: ${name}`);
    if (entry.isDirectory()) { stageResources(join(source, entry.name), name); continue; }
    assert(entry.isFile(), `non-file payload: ${name}`);
    assert(!/(^|\/)(?:vendor|node_modules|tests?|\.git)(\/|$)|\.py$/.test(name), `unexpected runtime payload: ${name}`);
    const content = readFileSync(join(source, entry.name));
    const destination = join(stage, 'payload', name);
    mkdirSync(dirname(destination), { recursive: true });
    writeFileSync(destination, content);
    resources.push({ path: name, bytes: content.length, sha256: createHash('sha256').update(content).digest('hex') });
  }
}
assert.equal(readFileSync(join(repository, 'addon/Bridge/Definitions.lua'), 'utf8'),
  'local _, ns = ...\nns.ProbeDefinitions = {schema="lycheedev.queue.v1",entries={\n}}\n',
  'refuse to package a modified or nonempty probe queue');
stageResources(join(repository, 'addon'), 'addon');
stageResources(join(repository, 'skills/lycheedev'), 'skill');
assert.equal(run('git', ['rev-parse', 'HEAD'], repository).trim(), commit, 'HEAD changed during assembly');
assert.equal(run('git', ['status', '--porcelain'], repository).trim().length > 0, workspaceDirty, 'worktree state changed during assembly');
writeFileSync(join(stage, 'release.json'), JSON.stringify({ schema: 'lycheedev.release.v1', version: manifest.version, commit, resources, binaries: { [target]: { path: binaryPath, bytes: binaryBytes.length, sha256: createHash('sha256').update(binaryBytes).digest('hex') } } }, null, 2));
const flags = ['--userconfig', userconfig, '--cache', cache, '--offline', '--ignore-scripts', '--no-audit', '--no-fund'];
const packed = JSON.parse(run(process.execPath, [npmCLI, 'pack', '--json', '--pack-destination', root, ...flags], stage))[0];
const tgz = join(root, packed.filename);
const bytes = readFileSync(tgz);
const digest = createHash('sha256').update(bytes).digest('hex');
assert(packed.files.some(file => file.path === binaryPath));
assert(packed.files.some(file => file.path === 'THIRD_PARTY_NOTICES'));
for (const resource of resources) assert(packed.files.some(file => file.path === `payload/${resource.path}`), `missing packed ${resource.path}`);
assert(!packed.files.some(file => /(^test\/|\.py$|node_modules|workspace\.json)/.test(file.path)));
run(process.execPath, [npmCLI, 'install', '--global', '--prefix', prefix, tgz, ...flags]);
const installed = process.platform === 'win32' ? join(prefix, 'node_modules/lycheedev') : join(prefix, 'lib/node_modules/lycheedev');
assert.deepEqual(readFileSync(join(installed, 'THIRD_PARTY_NOTICES')), sourceThirdPartyNotices);
const launcher = join(installed, 'bin/lycheedev.mjs');
const version = JSON.parse(run(process.execPath, [launcher, 'version', '--format=json']));
assert.equal(version.ok, true);
assert.equal(version.result.version, manifest.version);
assert.equal(version.result.commit, commit);
assert.equal(version.result.workspaceDirty, workspaceDirty);
assert.equal(version.result.platform, target.replace('windows-', 'windows/').replace('linux-', 'linux/').replace('darwin-', 'darwin/'));
const shim = process.platform === 'win32' ? join(prefix, 'lycheedev.cmd') : join(prefix, 'bin/lycheedev');
const shimVersion = JSON.parse(process.platform === 'win32'
  ? run('pwsh', ['-NoProfile', '-NonInteractive', '-Command', '& $env:LYCHEEDEV_SMOKE_SHIM version --format=json; exit $LASTEXITCODE'], root, { LYCHEEDEV_SMOKE_SHIM: shim })
  : run(shim, ['version', '--format=json']));
assert.deepEqual(shimVersion, version);
const home = join(root, 'fresh-home');
const initialized = JSON.parse(run(process.execPath, [launcher, 'init', '--home', home, '--format=json']));
assert.equal(initialized.ok, true);
for (const command of [['live', 'status', 'OP-missing'], ['live', 'session', 'SESSION-missing'], ['target', 'show', 'PIN-missing'], ['evidence', 'show', 'CAP-missing'], ['evidence', 'verify', 'CAP-missing']]) {
  const read = spawnSync(process.execPath, [launcher, ...command, '--home', home, '--format=json'], {
    cwd: root, encoding: 'utf8', shell: false, windowsHide: true, timeout: 30000,
  });
  assert.ifError(read.error);
  assert.notEqual(read.status, 0);
  const result = JSON.parse(read.stdout);
  assert.equal(result.error.code, 'vault.record_missing');
  assert.equal(result.result, null);
  assert.deepEqual(readdirSync(join(home, 'state')), [], 'read initialized database');
  assert.deepEqual(readdirSync(join(home, 'locks')), [], 'read created schema lock');
}
const describe = JSON.parse(run(process.execPath, [launcher, 'describe', '--format=json']));
assert.equal(describe.ok, true);
assert(describe.result.commands.some(command => command.path === 'data hotfix'));
// A real installed process discovers a portable project lock from a child
// directory; neither the data snapshot nor a global last-used target is passed.
const projectDirectory = join(root, 'project-context');
const projectChild = join(projectDirectory, 'src');
mkdirSync(projectChild, { recursive: true });
const projectSpec = join(root, 'project-selection.json');
writeFileSync(projectSpec, JSON.stringify({ data: {
  product: 'retail', region: 'cn', language: 'zhCN', fullBuild: '12.1.0.69875',
  buildConfig: 'a'.repeat(32), cdnConfig: 'b'.repeat(32), definitionCommit: 'c'.repeat(40),
} }));
const projectCLI = (args, cwd = root) => JSON.parse(run(process.execPath, [launcher, ...args, '--format=json'], cwd));
const projectPin = projectCLI(['target', 'resolve', '--file', projectSpec, '--home', home]).result.id;
projectCLI(['project', 'init', '--path', projectDirectory, '--product', 'retail']);
projectCLI(['project', 'lock', '--path', projectDirectory, '--snapshot', projectPin, '--home', home]);
assert.equal(projectCLI(['project', 'status'], projectChild).result.lock.selection.id, projectPin);
const projectCache = Buffer.alloc(44 + 32 + 2);
projectCache.write('XFTH'); projectCache.writeUInt32LE(9, 4); projectCache.writeUInt32LE(69875, 8);
projectCache.write('XFTH', 44); projectCache.writeUInt32LE(5, 48);
projectCache.writeUInt32LE(0xabcdef01, 60); projectCache.writeUInt32LE(42, 64);
projectCache.writeUInt32LE(2, 68); projectCache[72] = 1; projectCache[77] = 0xff;
const projectCachePath = join(root, 'project-cache.bin');
writeFileSync(projectCachePath, projectCache);
const projectQuery = projectCLI(['data', 'hotfix', '--source', 'dbcache', '--snapshot', projectPin, '--dbcache', projectCachePath, '--home', home], projectChild);
assert.equal(projectQuery.ok, true);
assert.equal(projectQuery.context.project, projectDirectory);
assert.equal(projectQuery.result.page.entries[0].payloadHex, '00ff');
for (const capture of projectQuery.captures) assert.equal(projectCLI(['evidence', 'verify', capture.id, '--home', home]).ok, true);
const projectContext = { status: 'passed', snapshot: projectPin, query: projectQuery, scope: 'installed CLI project lock discovery and synthetic local Hotfix query' };
const hotfixOutput = run('go', ['test', '-count=1', '-run', '^TestHotfixNamedTableAndLatestCLI$', '-v', './internal/command'], repository, {
  LYCHEEDEV_HOTFIX_NODE: process.execPath,
  LYCHEEDEV_HOTFIX_LAUNCHER: launcher,
});
assert(hotfixOutput.includes(`using installed hotfix launcher: ${launcher}`), 'Hotfix query did not use installed package');
assert(hotfixOutput.includes('--- PASS: TestHotfixNamedTableAndLatestCLI'), 'installed Hotfix integration failed');
const hotfixQuery = { status: 'passed', scope: 'synthetic local cache and pinned offline definitions; real installed CLI decoding, latest pagination and evidence verification', launcher, output: hotfixOutput };
const assetOutput = run('go', ['test', '-count=1', '-run', '^(TestAssetExportProjectOffline|TestAssetImageExport(ProjectOffline|FailuresPreserveOutput))$', '-v', './internal/command'], repository, {
  LYCHEEDEV_ASSET_NODE: process.execPath,
  LYCHEEDEV_ASSET_LAUNCHER: launcher,
});
assert(assetOutput.includes(`using installed asset launcher: ${launcher}`), 'Asset export did not use installed package');
assert(assetOutput.includes('--- PASS: TestAssetExportProjectOffline'), 'installed asset export integration failed');
assert(assetOutput.includes('--- PASS: TestAssetImageExportProjectOffline'), 'installed image asset export integration failed');
assert(assetOutput.includes('--- PASS: TestAssetImageExportFailuresPreserveOutput'), 'installed image asset failure integration failed');
const assetExport = { status: 'passed', scope: 'synthetic authenticated CDN cache; installed CLI project selection, raw and PNG/WebP image export with decoded pixels, mip/channel options, overwrite/conflict, failed-query preservation and evidence verification', launcher, output: assetOutput };
const sessionContract = describe.result.commands.find(command => command.path === 'live session');
assert(sessionContract, 'installed CLI is missing session evidence query');
assert.equal(sessionContract.mutates, false);
assert.equal(describe.result.commands.find(command => command.path === 'live bind')?.mutates, true);
assert.equal(describe.result.commands.find(command => command.path === 'live resume')?.mutates, true);
assert.equal(describe.result.commands.find(command => command.path === 'live run')?.mutates, true);
const probeFile = join(root, 'probe.lua');
writeFileSync(probeFile, 'return 42\n');
const absentRunHome = join(root, 'absent-run-home');
const runAdmission = [];
for (const extra of [[], ['--session', 'SESSION-missing', '--account', 'Account-A', '--file', probeFile]]) {
  const attempt = spawnSync(process.execPath, [launcher, 'live', 'run', ...extra, '--home', absentRunHome, '--format=json'], {
    cwd: root, encoding: 'utf8', shell: false, windowsHide: true, timeout: 30000,
  });
  assert.ifError(attempt.error);
  assert.notEqual(attempt.status, 0);
  const result = JSON.parse(attempt.stdout);
  assert.equal(result.ok, false);
  assert.equal(result.operationId, '');
  assert.equal(result.result, null);
  if (extra.length === 0) assert.equal(result.error.code, 'command.invalid_arguments');
  assert(!existsSync(absentRunHome), 'run admission created an implicit workspace');
  runAdmission.push(result);
}
const recoveryOutput = run('go', ['test', '-count=1', '-run', '^TestOperationReportLifecycleUsesArchivedEvidence$', '-v', './internal/live'], repository, {
  LYCHEEDEV_RECOVERY_NODE: process.execPath,
  LYCHEEDEV_RECOVERY_LAUNCHER: launcher,
});
assert(recoveryOutput.includes(`using installed recovery launcher: ${launcher}`), 'recovery did not use installed package');
assert(recoveryOutput.includes('--- PASS: TestOperationReportLifecycleUsesArchivedEvidence'), 'recovery integration test did not pass');
const terminalRecovery = { status: 'passed', scope: 'synthetic completed operation, real installed CLI owner retirement, not game execution', launcher, output: recoveryOutput };
const incompleteBind = spawnSync(process.execPath, [launcher, 'live', 'bind', '--format=json'], {
  cwd: root, encoding: 'utf8', shell: false, windowsHide: true, timeout: 30000,
});
assert.ifError(incompleteBind.error);
assert.equal(incompleteBind.status, 2);
assert.equal(JSON.parse(incompleteBind.stdout).error.code, 'command.invalid_arguments');
const absentSessionHome = join(root, 'absent-session-home');
const missingSession = spawnSync(process.execPath, [launcher, 'live', 'session', 'SESSION-missing', '--home', absentSessionHome, '--format=json'], {
  cwd: root, encoding: 'utf8', shell: false, windowsHide: true, timeout: 30000,
});
assert.ifError(missingSession.error);
assert.notEqual(missingSession.status, 0);
const sessionFailure = JSON.parse(missingSession.stdout);
assert.equal(sessionFailure.ok, false);
assert.equal(sessionFailure.result, null, 'failed lookup returned a fabricated empty session');
assert(!existsSync(absentSessionHome), 'historical session lookup created a workspace');
let nativeBinding = { status: 'not-run', scope: 'owned synthetic Windows window, not WoW' };
if (process.env.LYCHEEDEV_TEST_DESKTOP === '1') {
  assert.equal(target, 'windows-amd64', 'native binding fixture requires Windows amd64');
  const output = run('go', ['test', '-count=1', '-run', '^TestLiveBindNativeCLI$', '-v', './internal/command'], repository, {
    LYCHEEDEV_BINDING_NODE: process.execPath,
    LYCHEEDEV_BINDING_LAUNCHER: launcher,
  });
  assert(output.includes(`using installed npm launcher: ${launcher}`), 'native test did not use installed launcher');
  assert(output.includes('--- PASS: TestLiveBindNativeCLI'), 'native binding test was skipped or failed');
  nativeBinding = { ...nativeBinding, status: 'passed', launcher, output };
}
function cli(...args) {
  const result = JSON.parse(run(process.execPath, [launcher, ...args, '--format=json']));
  assert.equal(result.ok, true);
  return result;
}
function conflict(...args) {
  const result = spawnSync(process.execPath, [launcher, ...args, '--format=json'], { cwd: root, encoding: 'utf8', shell: false, windowsHide: true, timeout: 30000, maxBuffer: 4 * 1024 * 1024 });
  assert.ifError(result.error);
  assert.equal(result.status, 3, result.stderr || result.stdout);
  const response = JSON.parse(result.stdout);
  assert.equal(response.ok, false);
  assert.equal(response.error.code, 'delivery.installation_conflict');
  return response;
}
// Every deployment uses the npm-installed payload, never the source checkout.
for (const resource of resources) {
  const content = readFileSync(join(installed, 'payload', resource.path));
  assert.equal(content.length, resource.bytes);
  assert.equal(createHash('sha256').update(content).digest('hex'), resource.sha256);
}
const skillParent = join(root, 'isolated-agent-skills');
mkdirSync(skillParent);
const skillPath = join(skillParent, 'lycheedev');
const skillInstall = cli('skill', 'install', '--release', installed, '--path', skillPath);
assert.equal(cli('skill', 'status', '--path', skillPath).result.state, 'managed');
assert.deepEqual(readFileSync(join(skillPath, 'SKILL.md')), readFileSync(join(installed, 'payload/skill/SKILL.md')));
const skillRecovery = join(root, 'skill-recovery');
const skillUpgrade = cli('skill', 'install', '--release', installed, '--path', skillPath, '--output', skillRecovery);
const skillResume = cli('skill', 'install', '--resume', '--path', skillPath, '--output', skillRecovery);
const client = join(root, 'simulated-game', '_retail_');
mkdirSync(join(client, 'Interface/AddOns'), { recursive: true });
writeFileSync(join(client, '.flavor.info'), 'Product Flavor!STRING:0\nwow\n');
writeFileSync(join(client, 'version.txt'), '12.1.0.69875\n');
const sentinel = join(client, 'WTF/Account/Test/SavedVariables/sentinel.lua');
mkdirSync(dirname(sentinel), { recursive: true });
writeFileSync(sentinel, 'sentinel = true\n');
const addonInstall = cli('addon', 'install', '--release', installed, '--installation', client);
assert.equal(cli('addon', 'status', '--installation', client).result.installation.state, 'managed');
const addonRecovery = join(root, 'addon-recovery');
const addonUpgrade = cli('addon', 'install', '--release', installed, '--installation', client, '--output', addonRecovery);
const addonResume = cli('addon', 'install', '--resume', '--installation', client, '--output', addonRecovery);
const addonEdited = join(client, 'Interface/AddOns/Lychee Dev/Core/Runtime.lua');
const addonOriginal = readFileSync(addonEdited);
writeFileSync(addonEdited, '-- isolated user edit\n');
const addonConflict = conflict('addon', 'remove', '--installation', client, '--output', join(root, 'rejected-addon-removal'));
assert.equal(readFileSync(addonEdited, 'utf8'), '-- isolated user edit\n');
assert(!existsSync(join(root, 'rejected-addon-removal')));
writeFileSync(addonEdited, addonOriginal);
const addonRemove = cli('addon', 'remove', '--installation', client, '--output', join(root, 'removed-addon'));
assert.equal(cli('addon', 'status', '--installation', client).result.installation.state, 'absent');
assert.equal(readFileSync(sentinel, 'utf8'), 'sentinel = true\n');
const skillOriginal = readFileSync(join(skillPath, 'SKILL.md'));
writeFileSync(join(skillPath, 'SKILL.md'), 'isolated user edit\n');
const skillConflict = conflict('skill', 'install', '--release', installed, '--path', skillPath);
assert.equal(readFileSync(join(skillPath, 'SKILL.md'), 'utf8'), 'isolated user edit\n');
writeFileSync(join(skillPath, 'SKILL.md'), skillOriginal);
const skillRemove = cli('skill', 'remove', '--path', skillPath, '--output', join(root, 'removed-skill'));
assert.equal(cli('skill', 'status', '--path', skillPath).result.state, 'absent');
assert.equal(createHash('sha256').update(readFileSync(tgz)).digest('hex'), digest);
const report = { scope: 'development host binary and current addon/skill payload; simulated deployment, not game or release acceptance', target, root, tgz, sha256: digest, commit, workspaceDirty, resources, compressedBytes: bytes.length, unpackedBytes: packed.unpackedSize, ignoreScripts: true, version, shimVersion, initialized, describe, projectContext, hotfixQuery, assetExport, runAdmission, sessionFailure, nativeBinding, terminalRecovery, skillInstall, skillUpgrade, skillResume, skillRemove, skillConflict, addonInstall, addonUpgrade, addonResume, addonRemove, addonConflict };
writeFileSync(join(root, 'report.json'), JSON.stringify(report, null, 2));
if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `report=${join(root, 'report.json')}\n`);
process.stdout.write(JSON.stringify({ root, target, sha256: digest, compressedBytes: bytes.length, unpackedBytes: packed.unpackedSize, status: 'passed' }) + '\n');
