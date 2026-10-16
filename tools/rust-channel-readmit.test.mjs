import test from 'node:test';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import {capture, sha256, validateTransition} from './rust-channel-recovery.mjs';
import {AUTHORIZATION, BOOT_FENCE, DATA_IDENTITIES, OWNER, RESOURCE_PATHS, checkedPath, equalSnapshot, inspect, loadPreparation, validateAdmission, validateIdle} from './rust-channel-readmit.mjs';

const runId = '11299fbf-7a09-4dc8-ba5d-e440145136f6';
const token = '00000000-0000-4000-8000-000000000002';
const boot = (counter = '9007199254740993', machine = 'a'.repeat(64)) => ({schema: 'lycheedev.windows-boot-counter.v1', uptime100ns: counter, machineSHA256: machine});
function row(path, value) { const bytes = Buffer.from(JSON.stringify(value)); return {path, bytes: bytes.length, sha256: sha256(bytes), base64: bytes.toString('base64')}; }
function observation() {
  return {schema: 'lycheedev.channel-live-idle.v1', originalFilesLocked: 3, locksHeldThroughCommit: true,
    resourcePaths: [...RESOURCE_PATHS], dataIdentities: structuredClone(DATA_IDENTITIES), lockedResources: RESOURCE_PATHS.length, failedResources: 0,
    relatedProcessesBefore: [], relatedProcessesAfter: [], restartManagerBefore: [], restartManagerAfter: [],
    watcherEvents: 0, watcherErrors: 0, observationMilliseconds: 10000,
    targetBefore: {files: 12613, sha256: 'e'.repeat(64)}, targetAfter: {files: 12613, sha256: 'e'.repeat(64)},
    beforeBoot: boot(), afterBoot: boot('9007199354740993')};
}
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'rust-readmit-model-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const evidence = join(root, '.tmp/rust-disk/evidence', OWNER), base = join(evidence, `process-${runId}`);
  mkdirSync(evidence, {recursive: true}); mkdirSync(join(root, '.tmp/rust-disk/channels'), {recursive: true});
  const command = ['cargo', 'test', '--locked', '--offline', '-p', 'lycheedev-data', '--lib'];
  const rows = [row('.tmp/rust-disk/channels/ordinary.lease', {schema: 'lycheedev.build-lease.v1', owner: 'local-test', token, pid: 17, state: 'active', channel: 'ordinary'}),
    row(`.tmp/rust-disk/evidence/${OWNER}/receipt.json`, {schema: 'lycheedev.disk-owner.v1', id: OWNER, owner: 'local-test', directory: join(root, '.tmp/rust-scratch', OWNER), state: 'active', activePid: 17, command, commandSHA256: sha256(Buffer.from(JSON.stringify(command)))}),
    row(`.tmp/rust-disk/evidence/${OWNER}/process-${runId}.request.json`, {command: 'cargo', args: command.slice(1), cwd: root, stdout: base + '.stdout.bin', stderr: base + '.stderr.bin', receipt: base + '.receipt.json'})];
  for (const item of rows) writeFileSync(join(root, item.path), Buffer.from(item.base64, 'base64'));
  const snapshot = capture(root, OWNER), prepared = {schema: 'lycheedev.channel-recovery-prepare.v1', fenceId: BOOT_FENCE, root, snapshot, boot: boot(),
    toolHashes: {javascript: sha256(readFileSync(new URL('./rust-channel-recovery.mjs', import.meta.url))), powershell: sha256(readFileSync(new URL('./rust-channel-recovery.ps1', import.meta.url)))}, oldOwnerHistoricalState: 'unknown', oldOwnerReaped: false};
  const directory = join(root, '.tmp/rust-disk/recovery', OWNER, BOOT_FENCE), raw = Buffer.from(JSON.stringify(prepared)), digest = sha256(raw);
  mkdirSync(directory, {recursive: true}); writeFileSync(join(directory, 'prepared.json'), raw);
  const generation = randomUUID(), attempt = join(directory, 'readmission', generation); mkdirSync(attempt, {recursive: true});
  const intent = {schema: 'lycheedev.channel-readmission-intent.v1', ownerId: OWNER, bootFenceId: BOOT_FENCE, generation, preparationSHA256: digest,
    authorization: AUTHORIZATION, originalLeaseSHA256: snapshot.files[0].sha256, originalLeaseToken: token, machineSHA256: boot().machineSHA256,
    resourcePaths: [...RESOURCE_PATHS], archive: join(attempt, 'ordinary.lease.original'), target: join(root, 'target')};
  const record = {schema: 'lycheedev.channel-readmission.v1', state: 'prepared', mode: 'user-authorized-stop-current-idle',
    ...Object.fromEntries(['ownerId', 'bootFenceId', 'generation', 'preparationSHA256', 'authorization', 'originalLeaseSHA256', 'originalLeaseToken', 'archive', 'target'].map(key => [key, intent[key]])),
    channel: 'ordinary', oldOwnerHistoricalState: 'unknown', oldOwnerReaped: false, originalOwnerReceiptUnchanged: true, scratchPreserved: true, observation: observation()};
  writeFileSync(join(attempt, 'intent.json'), JSON.stringify(intent));
  return {root, snapshot, prepared, directory, digest, generation, attempt, intent, record, raw};
}

test('same live boot can support current-idle readmission without weakening the original boot gate', () => {
  assert.equal(validateIdle(observation(), RESOURCE_PATHS), true);
  assert.throws(() => validateTransition(boot(), boot('9007199354740993')), /new_full_boot_unproven/);
});
test('fixed scope contains all twelve reported resources plus three separately held originals', () => {
  assert.equal(RESOURCE_PATHS.length, 12);
  for (const profile of ['debug', 'release']) for (const name of ['.cargo-lock', '.cargo-build-lock', '.cargo-artifact-lock']) assert(RESOURCE_PATHS.includes(`target/${profile}/${name}`));
  assert.equal(DATA_IDENTITIES.length, 2);
  assert(RESOURCE_PATHS.includes('target/debug/lycheedev.exe'));
});
test('missing lock, released critical section, altered resource or unverified Data identity rejects', () => {
  for (const change of [x => x.originalFilesLocked--, x => x.locksHeldThroughCommit = false, x => x.lockedResources--, x => x.failedResources++, x => x.resourcePaths.pop(), x => x.dataIdentities[0].sha256 = 'b'.repeat(64)]) {
    const x = observation(); change(x); assert.throws(() => validateIdle(x, RESOURCE_PATHS));
  }
});
test('current processes, resource users, watcher activity/errors, incomplete observation and metadata drift reject', () => {
  for (const change of [x => x.relatedProcessesBefore.push({pid: 17}), x => x.relatedProcessesAfter.push({pid: 17}), x => x.restartManagerBefore.push({pid: 17}), x => x.restartManagerAfter.push({pid: 17}), x => x.watcherEvents++, x => x.watcherErrors++, x => x.observationMilliseconds--, x => x.targetAfter.sha256 = 'f'.repeat(64), x => x.targetAfter.files--, x => delete x.relatedProcessesAfter]) {
    const x = observation(); change(x); assert.throws(() => validateIdle(x, RESOURCE_PATHS));
  }
});
test('host change, counter regression and inexact numeric uptime reject', () => {
  for (const changed of [boot('1'), boot('9007199354740993', 'b'.repeat(64)), boot(9007199354740993)]) {
    const x = observation(); x.afterBoot = changed; assert.throws(() => validateIdle(x, RESOURCE_PATHS));
  }
});
test('original preparation and all original byte identities remain authoritative', t => {
  const f = fixture(t); assert.deepEqual(loadPreparation(f.root, f.digest).prepared, f.prepared);
  assert.equal(equalSnapshot(f.snapshot, capture(f.root, OWNER)), true);
  const lease = join(f.root, f.snapshot.files[0].path); writeFileSync(lease, readFileSync(lease, 'utf8') + '\n');
  assert.throws(() => equalSnapshot(f.snapshot, capture(f.root, OWNER)), /original_evidence_changed/);
  assert.deepEqual(readFileSync(join(f.directory, 'prepared.json')), f.raw);
});
test('false historical reap, foreign generation, token and target cannot be relabeled as admission', t => {
  const f = fixture(t); assert.equal(validateAdmission(f.intent, f.record), true);
  for (const change of [x => x.oldOwnerReaped = true, x => x.oldOwnerHistoricalState = 'reaped', x => x.scratchPreserved = false, x => x.originalOwnerReceiptUnchanged = false, x => x.generation = randomUUID(), x => x.originalLeaseToken = randomUUID(), x => x.target = join(f.root, 'target2'), x => x.channel = 'symbols']) {
    const x = structuredClone(f.record); change(x); assert.throws(() => validateAdmission(f.intent, x));
  }
});
test('read-only inspection distinguishes an attempt/prepared record from the atomic commit', t => {
  const f = fixture(t); assert.equal(inspect(f.root, f.digest, f.generation).state, 'attempt_not_committed');
  writeFileSync(join(f.attempt, 'admission.json'), JSON.stringify(f.record));
  assert.equal(inspect(f.root, f.digest, f.generation).state, 'prepared_not_committed');
  assert.equal(readFileSync(join(f.root, f.snapshot.files[0].path)).toString('base64'), f.snapshot.files[0].base64);
});
test('lost response is resolved by exact archived bytes without deleting a new ordinary lease', t => {
  const f = fixture(t); writeFileSync(join(f.attempt, 'admission.json'), JSON.stringify(f.record));
  const lease = join(f.root, f.snapshot.files[0].path); renameSync(lease, f.intent.archive);
  assert.equal(inspect(f.root, f.digest, f.generation).state, 'readmission_commit_observed');
  writeFileSync(lease, 'new generation lease'); assert.equal(inspect(f.root, f.digest, f.generation).currentLease, 'present_do_not_modify');
  assert.equal(readFileSync(lease, 'utf8'), 'new generation lease'); assert.deepEqual(readFileSync(join(f.directory, 'prepared.json')), f.raw);
  for (const item of f.snapshot.files.slice(1)) assert.equal(readFileSync(join(f.root, item.path)).toString('base64'), item.base64);
});
test('corrupt archive and foreign path/digest reject without touching existing evidence', t => {
  const f = fixture(t); writeFileSync(join(f.attempt, 'admission.json'), JSON.stringify(f.record));
  renameSync(join(f.root, f.snapshot.files[0].path), f.intent.archive); writeFileSync(f.intent.archive, 'corrupt');
  assert.throws(() => inspect(f.root, f.digest, f.generation), /archive_integrity/);
  assert.throws(() => loadPreparation(f.root, 'f'.repeat(64)), /preparation_digest/);
  assert.throws(() => checkedPath(f.root, join(f.root, '../foreign')), /path_outside_repo/);
});
test('production commit holds originals and all coordination locks through final live checks and no-replace rename', () => {
  const ps = readFileSync(new URL('./rust-channel-readmit.ps1', import.meta.url), 'utf8');
  assert.match(ps, /ToolkitChannelFence\+LockedFile/); assert.match(ps, /\[IO.FileShare\]::None/);
  assert.match(ps, /Write-NewFlushed \$recordPath[\s\S]*Related-Processes\)\.Count[\s\S]*\$originalLocks\[0\]\.RenameNoReplace\(\$archive\)[\s\S]*finally\{/);
  assert.match(ps, /\$created -ceq \$selfCreated/); assert.match(ps, /\$created -ceq \$coordinatorCreated/);
  assert.doesNotMatch(ps, /Stop-Process|TerminateProcess|TerminateJobObject|RmShutdown|Restart-Computer|Remove-Item|Move-Item/);
  assert.doesNotMatch(ps, /Assert-NewBoot|new_full_boot_unproven/);
  assert.match(ps, /original_boot_tools_changed/);
  assert.match(ps, /catch\{\$primaryFailure=\$_;throw\}finally/);
  assert.match(ps, /try\{\$held\[\$i\]\.Dispose\(\)\}catch\{\$cleanupErrors\.Add/);
  assert.match(ps, /renameReturnedSuccess=\$committed/);
});

test('Windows fixture compiles actual native observers and holds every fixture lock across no-replace rename', {skip: process.platform !== 'win32'}, t => {
  const directory = join(process.cwd(), '.tmp', `channel-readmit-native-${randomUUID()}`);
  mkdirSync(directory, {recursive: true}); t.diagnostic(`Preserved native readmission fixture: ${directory}`);
  const productionSource = readFileSync(new URL('./rust-channel-readmit.ps1', import.meta.url), 'utf8');
  const cleanupMarker = '}catch{$primaryFailure=$_;throw}finally{\n';
  const cleanupBody = productionSource.slice(productionSource.lastIndexOf(cleanupMarker) + cleanupMarker.length).trim().slice(0, -1);
  const nativeSource = productionSource.match(/# BEGIN READMIT NATIVE:[\s\S]*?Add-Type -TypeDefinition @'\r?\n([\s\S]*?)\r?\n'@/);
  const legacySource = readFileSync(new URL('./rust-channel-recovery.ps1', import.meta.url), 'utf8').match(/Add-Type -TypeDefinition @'\r?\n([\s\S]*?)\r?\n'@/);
  assert(nativeSource && legacySource);
  const path = fileURLToPath(new URL('./rust-channel-readmit.ps1', import.meta.url)).replaceAll("'", "''"), script = join(directory, 'native-fixture.ps1');
  writeFileSync(script, '\uFEFF' + `$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
[void][Management.Automation.Language.Parser]::ParseFile('${path}',[ref]$tokens,[ref]$errors)
if($errors.Count -ne 0){throw ($errors|Out-String)}
Add-Type -TypeDefinition @'
${legacySource[1]}
'@
Add-Type -TypeDefinition @'
${nativeSource[1]}
'@
$target=Join-Path $PSScriptRoot 'target';[void][IO.Directory]::CreateDirectory($target)
$lease=Join-Path $PSScriptRoot 'fixture.lease';$owner=Join-Path $PSScriptRoot 'owner.json';$request=Join-Path $PSScriptRoot 'request.json';$cargo=Join-Path $target '.cargo-build-lock';$archive=Join-Path $PSScriptRoot 'archive';$blocked=Join-Path $PSScriptRoot 'existing-archive'
foreach($p in @($lease,$owner,$request,$cargo)){[IO.File]::WriteAllText($p,'original bytes')}
[IO.File]::WriteAllText($blocked,'must preserve')
$held=New-Object 'System.Collections.Generic.List[System.IDisposable]';$watch=$null
try{
 $original=New-Object ToolkitChannelFence+LockedFile $lease,$true;$held.Add($original)
 foreach($p in @($owner,$request)){$held.Add((New-Object ToolkitChannelFence+LockedFile $p,$false))}
 $held.Add([IO.File]::Open($cargo,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None))
 $own=[Diagnostics.Process]::GetCurrentProcess();try{$created=$own.StartTime.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture)}finally{$own.Dispose()}
 $users=@([ToolkitReadmit]::ResourceUsers([string[]]@($lease,$owner,$request,$cargo))|Where-Object {-not ($_.pid -eq $PID -and $_.creationFileTime -ceq $created)})
 if($users.Count -ne 0){throw 'fixture_foreign_resource_user'}
 foreach($p in @($lease,$owner,$request,$cargo)){$refused=$false;try{[IO.File]::WriteAllText($p,'bad')}catch{$refused=$true};if(-not $refused){throw 'writer_not_excluded'}}
 $refused=$false;try{$original.RenameNoReplace($blocked)}catch{$refused=$true};if(-not $refused -or -not [IO.File]::Exists($lease) -or [IO.File]::ReadAllText($blocked) -cne 'must preserve'){throw 'no_replace_contract'}
 $watch=New-Object ToolkitReadmit+Watch $target
 [IO.File]::WriteAllText((Join-Path $target 'fixture-write'),'writer')
 $timer=[Diagnostics.Stopwatch]::StartNew();while($watch.Events -eq 0 -and $timer.ElapsedMilliseconds -lt 5000){Start-Sleep -Milliseconds 10}
 if($watch.Events -eq 0 -or $watch.Errors -ne 0){throw 'watcher_did_not_observe_fixture_write'}
 $original.RenameNoReplace($archive)
 $refused=$false;try{[IO.File]::WriteAllText($cargo,'bad')}catch{$refused=$true};if(-not $refused){throw 'lock_released_before_commit'}
 if([IO.File]::Exists($lease) -or -not [IO.File]::Exists($archive)){throw 'fixture_rename_missing'}
}finally{if($watch){$watch.Dispose()};for($i=$held.Count-1;$i -ge 0;$i--){$held[$i].Dispose()}}
if([IO.File]::ReadAllText($archive) -cne 'original bytes'){throw 'archive_bytes_changed'}
Add-Type -TypeDefinition 'using System;public static class ReadmitCleanupFixture{public static int attempts;public sealed class Bad:IDisposable{public void Dispose(){attempts++;throw new Exception("fixture-close-failed");}}public sealed class Good:IDisposable{public void Dispose(){attempts++;}}}'
$watch=New-Object ReadmitCleanupFixture+Bad;$coordinator=New-Object ReadmitCleanupFixture+Bad;$self=New-Object ReadmitCleanupFixture+Good
$held=New-Object 'System.Collections.Generic.List[System.IDisposable]';$held.Add((New-Object ReadmitCleanupFixture+Bad));$held.Add((New-Object ReadmitCleanupFixture+Good))
try{throw 'fixture-primary-retained'}catch{$primaryFailure=$_}
$committed=$true;$Generation='fixture-cleanup'
${cleanupBody}
if([ReadmitCleanupFixture]::attempts -ne 5 -or $cleanupErrors.Count -ne 3 -or $primaryFailure.Exception.Message -cne 'fixture-primary-retained'){throw 'cleanup_did_not_preserve_all_attempts_and_primary'}
@{passed=$true;scope='actual native compile, Restart Manager, exact self identity, watcher write detection, original/resource lock lifetime, no-replace failure and commit; own fixtures only'}|ConvertTo-Json -Compress
`);
  const run = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script], {encoding: 'utf8', windowsHide: true, shell: false, timeout: 60000, maxBuffer: 1 << 20});
  writeFileSync(join(directory, 'wrapper-result.json'), JSON.stringify({status: run.status, error: run.error?.message, stdout: run.stdout, stderr: run.stderr}, null, 2));
  assert.equal(run.error, undefined); assert.equal(run.status, 0, run.stderr); assert.equal(JSON.parse(run.stdout.replace(/^\uFEFF/, '')).passed, true);
});
