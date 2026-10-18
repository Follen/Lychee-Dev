// Only the outer existing Windows Job may run this controller. Every child
// (exporter, npm, installed Skill, package launcher and CLI) stays in that Job.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve, delimiter} from 'node:path';
import {performance} from 'node:perf_hooks';
import {DatabaseSync} from 'node:sqlite';
import {hashFile} from '../../tools/rust-build.mjs';
import {CURRENT_GO_CONTRACT, inventory, inventoryHash, jsonFile, loadCurrentWorkflow, pendingIncrementTasks, within} from './assertions.mjs';
import {archiveFixture, bounded, digest, loadFixture, npmOfflineFlags, seedDataHome, MISSING_KEYS, RESTORED_KEYS} from './data-inputs.mjs';
import {CASES, CHECKS, archiveBytes, archiveCaptureProofs, assertDataReport, assertEnvelope, dataArgs, pendingDataTasks, requestFor, targetArgs} from './data-assertions.mjs';

assert.equal(process.platform, 'win32'); assert.equal(process.arch, 'x64');
const [configPath, ...extra] = process.argv.slice(2); assert(configPath && !extra.length);
const config = jsonFile(configPath), {scratch, evidence, root} = config;
assert.equal(resolve('.'), root); assert.equal(config.node, process.execPath); assert.equal(config.platform, process.platform);
const owner = jsonFile(join(evidence, 'receipt.json'));
assert.equal(owner.owner, 'skill-data-usage'); assert.equal(owner.state, 'active'); assert.equal(owner.directory, scratch);
assert.equal(config.ownerId, owner.id); assert.equal(configPath, join(evidence, 'configuration.json'));
assert.deepEqual(readdirSync(scratch), []); assert.equal(inventoryHash(inventory(config.sourcePackage)), config.packageSHA256);
const workflow = loadCurrentWorkflow(root);
const report = {schema: 'lycheedev.skill-data-usage.v2', goContract: CURRENT_GO_CONTRACT, skillTree: 'b9e162ace7abe7ea4a2feecef822283767eb1210', state: 'running', completeSkillAcceptance: false, config, startedAt: new Date().toISOString(), checks: [], commands: [], fixtures: {}, seeded: {}, workspaces: {}, queries: {}, keys: {}, proofs: {}, completedTasks: [], pendingTasks: pendingIncrementTasks(workflow), boundaries: ['Synthetic authenticated local CASC inputs; no actual client observation.', 'Logical memoryBudget is actual product output, not RSS or OS Job pressure evidence.', 'Pressure recovery and all Live tasks remain NOT_RUN.', 'New revision: prior lost-file Node results are not inherited.']};
const save = () => writeFileSync(join(evidence, 'report.json'), JSON.stringify(report, null, 2) + '\n');
save();
const deadline = performance.now() + 1140000;
let fatal = false, totalBytes = 0;
const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !/^(LYCHEEDEV_|NPM_CONFIG_)/i.test(name) && !['PATH', 'HOME', 'USERPROFILE', 'TEMP', 'TMP', 'NODE_OPTIONS', 'NODE_PATH'].includes(name.toUpperCase())));
Object.assign(env, {PATH: config.prefix + delimiter + (process.env.PATH ?? process.env.Path ?? ''), HOME: join(scratch, 'user'), USERPROFILE: join(scratch, 'user'), TEMP: join(scratch, 'temp'), TMP: join(scratch, 'temp')});
for (const path of [env.HOME, env.TEMP]) mkdirSync(path);
writeFileSync(join(scratch, 'user.npmrc'), '', {flag: 'wx'}); writeFileSync(join(scratch, 'global.npmrc'), '', {flag: 'wx'});
function command(id, executable, args, {cwd = scratch, kind = 'public', environment = {}} = {}) {
  assert(!fatal, 'prior process failure requires outer Job retirement');
  const remaining = Math.floor(deadline - performance.now());
  if (remaining < 1000 || report.commands.length >= 80 || totalBytes >= 128 << 20) { fatal = true; throw new Error('skill-data.total_controller_budget'); }
  const began = performance.now(); const run = spawnSync(executable, args, {cwd, env: {...env, ...environment}, shell: false, windowsHide: true, timeout: Math.min(120000, remaining), maxBuffer: 12 << 20});
  const stdout = run.stdout ?? Buffer.alloc(0), stderr = run.stderr ?? Buffer.alloc(0); totalBytes += stdout.length + stderr.length;
  const row = {id, executable, args, cwd, kind, exit: run.status, signal: run.signal, error: run.error?.message, wallMillis: performance.now() - began, stdout: archiveBytes(evidence, `${String(report.commands.length).padStart(3, '0')}-${id}.stdout.bin`, stdout), stderr: archiveBytes(evidence, `${String(report.commands.length).padStart(3, '0')}-${id}.stderr.bin`, stderr)};
  report.commands.push(row); save();
  if (run.error || run.signal || totalBytes > 128 << 20) fatal = true;
  assert.equal(run.error, undefined); assert.equal(run.signal, null); assert.equal(run.status, 0, `${id}: ${stderr.toString().slice(0, 400)}`);
  return stdout;
}
function invoke(id, args, kind = 'catalog') {
  const home = join(scratch, `${kind}-home`), raw = command(id, process.execPath, [config.launcher, ...args, '--home', home, '--format', 'json'], {environment: {LYCHEEDEV_HOME: home}});
  const envelope = JSON.parse(raw); assertEnvelope(envelope); return envelope;
}
function check(caseId, work) {
  assert.equal(CHECKS[report.checks.length], caseId, 'fixed ordered check');
  try { const detail = work() ?? {}; report.checks.push({caseId, state: 'passed', ...detail}); save(); }
  catch (error) { report.checks.push({caseId, state: 'failed', reason: error.message}); save(); throw error; }
}
const fixtures = {};
function proof(id, envelope, kind, entry) {
  report.proofs[id] = archiveCaptureProofs({caseId: id, envelope, entry, fixture: fixtures[kind], home: join(scratch, `${kind}-home`), evidence, output: join(scratch, `${id}.csv`), verify: (caseId, capture) => invoke(caseId, ['evidence', 'verify', capture], kind)}); save();
}
function initialize(kind) {
  const home = join(scratch, `${kind}-home`); assert(!existsSync(home)); invoke(`${kind}-init`, ['init'], kind);
  report.workspaces[kind] = archiveBytes(evidence, `${kind}-workspace.json`, bounded(join(home, 'workspace.json')));
  seedDataHome(fixtures[kind], home);
  const db = new DatabaseSync(join(home, 'state/toolkit.sqlite'), {readOnly: true});
  let rows; try { rows = db.prepare('SELECT key,generation,value FROM documents ORDER BY key').all().map(row => ({key: row.key, generation: row.generation, valueBase64: Buffer.from(row.value).toString('base64')})); } finally { db.close(); }
  assert.deepEqual(rows, fixtures[kind].documents.map(doc => ({key: doc.key, generation: 1, valueBase64: doc.bytes.toString('base64')})));
  report.seeded[kind] = archiveBytes(evidence, `${kind}-seeded.json`, Buffer.from(JSON.stringify(rows)));
}
function dataCase(entry) {
  if (entry.mode !== 'schema') {
    const raw = Buffer.from(JSON.stringify(requestFor(entry)) + '\n'); writeFileSync(join(scratch, `${entry.id}.query.json`), raw, {flag: 'wx'});
    report.queries[entry.id] = archiveBytes(evidence, `${entry.id}.query.json`, raw);
  }
  const envelope = invoke(entry.id, dataArgs(entry, config, fixtures[entry.fixture]), entry.fixture);
  proof(entry.id, envelope, entry.fixture, entry);
}
try {
  for (const [index, kind] of ['catalog', 'partial'].entries()) check(`export-${kind}`, () => {
    const path = join(scratch, `${kind}-fixture`); assert(!existsSync(path));
    const exporter = config.exporters.executables[index]; assert.equal(hashFile(exporter.path, 128 << 20), exporter.sha256);
    command(`export-${kind}`, exporter.path, [kind === 'catalog' ? 'prepare_catalog_public_fixture' : 'prepare_skill_partial_fixture', '--exact', '--ignored', '--nocapture'], {cwd: root, kind: 'exporter', environment: kind === 'catalog' ? {LYCHEEDEV_CATALOG_FIXTURE: path} : {LYCHEEDEV_SKILL_PARTIAL_FIXTURE: path, LYCHEEDEV_SKILL_OWNER_RECEIPT: join(evidence, 'receipt.json')}});
    fixtures[kind] = loadFixture(path, kind); report.fixtures[kind] = archiveFixture(fixtures[kind], kind, evidence);
  });
  check('npm-installed-package', () => {
    assert.equal(hashFile(config.npm.path), config.npm.sha256);
    const raw = command('npm-pack', process.execPath, [config.npm.path, 'pack', config.sourcePackage, '--json', '--pack-destination', scratch, ...npmOfflineFlags(scratch)], {kind: 'npm'});
    const packed = JSON.parse(raw); assert.equal(packed.length, 1); assert.equal(packed[0].filename, `lycheedev-${config.version}.tgz`);
    const tgz = bounded(config.tgz, 128 << 20); assert.equal(digest(tgz, 'sha1'), packed[0].shasum); assert.equal('sha512-' + Buffer.from(digest(tgz, 'sha512'), 'hex').toString('base64'), packed[0].integrity);
    report.tgz = {path: config.tgz, bytes: tgz.length, sha256: digest(tgz)};
    command('npm-install', process.execPath, [config.npm.path, 'install', '--global', '--prefix', config.prefix, config.tgz, ...npmOfflineFlags(scratch)], {kind: 'npm'});
    const installed = inventory(join(config.prefix, 'node_modules/lycheedev')); assert.deepEqual(installed, config.packageFiles); assert.equal(inventoryHash(installed), config.packageSHA256);
    report.installedInventory = archiveBytes(evidence, 'installed-inventory.json', Buffer.from(JSON.stringify(installed)));
    report.npmShim = archiveBytes(evidence, 'npm-lycheedev.cmd', bounded(join(config.prefix, 'lycheedev.cmd')));
    assert(!existsSync(join(config.prefix, 'lycheedev.exe')), 'npm prefix must choose installed package launcher');
    const baseline = jsonFile(join(root, 'tests/rust-skill-usage/baselines/go3.2.3-skill.json'));
    assert.equal(baseline.tree, report.skillTree);
    for (const row of baseline.files) assert.equal(hashFile(within(join(config.prefix, 'node_modules/lycheedev/payload/skill'), row.path)), row.sha256);
  });
  for (const [id, args] of [['version', ['version']], ['describe', ['describe']], ['help', ['--help']], ['sql-help', ['data', 'sql', '--help']]]) check(id, () => { invoke(id, args); });
  for (const kind of ['catalog', 'partial']) {
    check(`${kind}-init-and-seed`, () => initialize(kind));
    check(`${kind}-target-resolve`, () => { const envelope = invoke(`${kind}-target-resolve`, targetArgs(fixtures[kind]), kind); proof(`${kind}-target-resolve`, envelope, kind); });
    check(`${kind}-target-show`, () => { const envelope = invoke(`${kind}-target-show`, ['target', 'show', fixtures[kind].identity.snapshot], kind); assert.equal(envelope.result.id, fixtures[kind].identity.snapshot); assert.deepEqual(envelope.result.data, fixtures[kind].identity.pin); });
    if (kind === 'partial') { writeFileSync(join(scratch, 'sample.keys'), MISSING_KEYS, {flag: 'wx'}); report.keys.missing = archiveBytes(evidence, 'sample-missing.keys', bounded(join(scratch, 'sample.keys'))); }
    for (const entry of CASES.filter(entry => entry.fixture === kind && (kind === 'catalog' || entry.partial))) check(entry.id, () => dataCase(entry));
  }
  check('restore-keys-same-pin', () => {
    assert.equal(readFileSync(join(scratch, 'sample.keys'), 'utf8'), MISSING_KEYS); writeFileSync(join(scratch, 'sample.keys'), RESTORED_KEYS);
    report.keys.restored = archiveBytes(evidence, 'sample-restored.keys', bounded(join(scratch, 'sample.keys')));
  });
  check('restored-target-show', () => { const envelope = invoke('restored-target-show', ['target', 'show', fixtures.partial.identity.snapshot], 'partial'); assert.equal(envelope.result.id, fixtures.partial.identity.snapshot); });
  for (const entry of CASES.filter(entry => entry.fixture === 'partial' && !entry.partial)) check(entry.id, () => dataCase(entry));
  for (const kind of ['catalog', 'partial']) assert.deepEqual(inventory(fixtures[kind].directory, {maxFiles: 256, maxBytes: 64 << 20}), fixtures[kind].files, 'fixture inputs unchanged');
  assert.equal(inventoryHash(inventory(config.sourcePackage)), config.packageSHA256); assert.equal(hashFile(config.tgz, 128 << 20), report.tgz.sha256);
  report.state = 'passed_selected_data_workflow'; report.pendingTasks = pendingDataTasks(workflow); report.completedTasks = [{taskId: 'g323-data-query', runtimeVerified: true, basis: 'actual_owned_npm_installed_skill_and_independent_full_capture_replay', captures: 16}];
  assertDataReport(report, workflow); report.endedAt = new Date().toISOString(); save();
} catch (error) {
  report.state = 'failed'; report.completedTasks = []; report.pendingTasks = pendingIncrementTasks(workflow); report.error = error.stack;
  for (const id of CHECKS.slice(report.checks.length)) report.checks.push({caseId: id, state: 'not_run', reason: 'prior failure; no dependent command launched'});
  report.endedAt = new Date().toISOString(); save(); process.exitCode = 1;
}
