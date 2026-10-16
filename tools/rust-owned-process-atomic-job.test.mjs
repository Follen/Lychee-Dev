import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash, randomUUID} from 'node:crypto';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';

const windows = {skip: process.platform !== 'win32'};
const wrapper = fileURLToPath(new URL('./rust-owned-process.ps1', import.meta.url));
const source = readFileSync(wrapper, 'utf8');

function fixture(t, label, {legacyName, v2 = false} = {}) {
  const evidence = join(process.cwd(), '.tmp', `atomic-job-${label}-${randomUUID()}`);
  mkdirSync(evidence, {recursive: true});
  t.diagnostic(`Preserved atomic Job evidence: ${evidence}`);
  const id = randomUUID(), base = join(evidence, `process-${id}`);
  const path = legacyName ? join(evidence, legacyName) : base + '.request.json';
  const marker = join(evidence, 'root-ran');
  const request = {
    command: process.execPath, args: ['-e', `require('fs').writeFileSync(${JSON.stringify(marker)},'ran');process.stdout.write('legacy')`],
    cwd: process.cwd(), timeoutMs: 10000, orphanMs: 1000, cargoBuild: false,
    stdout: base + '.stdout.bin', stderr: base + '.stderr.bin', receipt: base + '.receipt.json',
  };
  if (v2) Object.assign(request, {schema: 'lycheedev.owned-request.v2', runId: id,
    identity: base + '.identity.json', rootIdentity: base + '.root.json'});
  return {evidence, id, base, path, marker, request};
}

function run(f) {
  writeFileSync(f.path, JSON.stringify(f.request));
  const original = readFileSync(f.path);
  const execution = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
    '-File', wrapper, '-Request', f.path], {
    encoding: 'utf8', windowsHide: true, shell: false, timeout: 60000, maxBuffer: 1 << 20,
  });
  writeFileSync(join(f.evidence, 'wrapper-result.json'), JSON.stringify({status: execution.status,
    signal: execution.signal, error: execution.error?.message, stdout: execution.stdout, stderr: execution.stderr}, null, 2));
  assert.equal(execution.error, undefined);
  assert.deepEqual(readFileSync(f.path), original, 'legacy request bytes must remain unchanged');
  return execution;
}

function proof(f) { return JSON.parse(readFileSync(f.request.receipt)); }

test('atomic startup retains the handle list and persists exact root before Resume', () => {
  assert.match(source, /InitializeProcThreadAttributeList\(attributes,2,/);
  assert.match(source, /new IntPtr\(0x20002\)/);
  assert.match(source, /new IntPtr\(0x2000D\)/);
  assert.ok(source.indexOf('owned.atomic_job_attribute') < source.indexOf('if(!CreateProcessW'));
  assert.ok(source.indexOf('using(var file=new FileStream(rootIdentity') < source.indexOf('if(ResumeThread'));
  assert.doesNotMatch(source, /AssignProcessToJobObject/);
  assert.match(source, /file\.Flush\(true\)/);
  assert.match(source, /assigned=true; \/\/ A successful JOB_LIST/);
});

test('unversioned direct consumers keep caller-chosen basenames and generated identity is explicit', () => {
  assert.match(source, /Existing direct PS1 consumers use unversioned requests/);
  assert.match(source, /\$null -ne \$requestData.schema -and/);
  assert.match(source, /\$requestData\|Add-Member -NotePropertyMembers/);
  assert.match(source, /else\{\$runId=\[Guid\]::NewGuid\(\)\.ToString\('D'\);\$identitySource='legacy-generated'\}/);
  assert.match(source, /identitySource=\$identitySource;requestPath=\$requestPath/);
  assert.match(source, /startupIdentity=\[string\]\$requestData.identity;rootIdentity=/);
  assert.match(source, /owned.start_identity_run/);
  assert.match(source, /owned.start_identity_path/);
});

for (const [label, options, expectedSource] of [
  ['legacy-uuid', {}, 'legacy-request-basename'],
  ['legacy-name', {legacyName: 'request.json'}, 'legacy-generated'],
  ['v2', {v2: true}, 'request-v2'],
]) test(`Windows ${label} executes through atomic Job creation with exact independent startup identity`, windows, t => {
  const f = fixture(t, label, options), execution = run(f);
  assert.equal(execution.status, 0, execution.stderr);
  assert.equal(readFileSync(f.request.stdout, 'utf8'), 'legacy');
  const p = proof(f), root = JSON.parse(readFileSync(p.rootIdentity)), start = JSON.parse(readFileSync(p.startupIdentity));
  for (const key of ['rootReaped', 'treeReaped', 'handlesClosed', 'outputReadersComplete']) assert.equal(p[key], true);
  assert.equal(root.jobAssignedAtCreation, true);
  assert.equal(root.pid, p.pid);
  assert.equal(root.creationFileTime, p.creationFileTime);
  assert.equal(start.runId, p.runId);
  if (options.legacyName) assert.notEqual(start.runId, f.id);
  else assert.equal(start.runId, f.id);
  assert.equal(start.identitySource, expectedSource);
  assert.equal(p.identitySource, expectedSource);
  assert.equal(start.requestPath, f.path);
  assert.equal(start.requestSHA256, createHash('sha256').update(readFileSync(f.path)).digest('hex'));
  assert.equal(start.historicalReapProof, false);
  assert.equal(root.historicalReapProof, false);
  assert.equal(start.bootClock, 'QueryUnbiasedInterruptTime');
  assert.match(start.wrapperCreationFileTime, /^[1-9]\d*$/);
  assert.match(start.bootUptime100ns, /^\d+$/);
});

test('Windows explicit v2 conflicting path cannot create a root or overwrite unrelated evidence', windows, t => {
  const f = fixture(t, 'invalid-path', {v2: true}), foreign = join(f.evidence, 'foreign.json');
  writeFileSync(foreign, 'preserve');
  f.request.identity = foreign;
  const execution = run(f);
  assert.notEqual(execution.status, 0);
  assert.match(execution.stderr, /owned.start_identity_path/);
  assert.equal(readFileSync(foreign, 'utf8'), 'preserve');
  assert.equal(existsSync(f.marker), false);
  assert.equal(existsSync(f.base + '.root.json'), false);
  assert.equal(existsSync(f.request.receipt), false);
});

test('Windows startup identity CreateNew failure leaves existing bytes and starts no root', windows, t => {
  const f = fixture(t, 'start-failure', {v2: true});
  writeFileSync(f.request.identity, 'original startup evidence');
  const execution = run(f);
  assert.notEqual(execution.status, 0);
  assert.equal(readFileSync(f.request.identity, 'utf8'), 'original startup evidence');
  assert.equal(existsSync(f.marker), false);
  assert.equal(existsSync(f.request.rootIdentity), false);
  assert.equal(existsSync(f.request.receipt), false);
});

test('Windows root identity CreateNew failure never resumes and reaps exact already-owned root', windows, t => {
  const f = fixture(t, 'root-failure', {v2: true});
  writeFileSync(f.request.rootIdentity, 'original root evidence');
  const execution = run(f);
  assert.notEqual(execution.status, 0);
  assert.equal(readFileSync(f.request.rootIdentity, 'utf8'), 'original root evidence');
  assert.equal(existsSync(f.marker), false, 'no user code before durable exact root evidence');
  const p = proof(f);
  assert.ok(p.pid > 0);
  assert.match(p.creationFileTime, /^[1-9]\d*$/);
  assert.equal(p.rootReaped, true);
  assert.equal(p.treeReaped, true);
  assert.equal(p.activeProcesses, 0);
  assert.equal(p.handlesClosed, true);
  assert.equal(p.forced, true);
  assert.equal(p.outputReadersComplete, false, 'unstarted readers do not become a successful run');
  assert.match(p.error, /System\.IO\.IOException/);
});
