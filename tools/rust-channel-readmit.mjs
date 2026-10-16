// One explicit user-authorized, current-idleness readmission. No old reap claim.
// The boot-fence tools and original preparation are immutable dependencies.
import {spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import {closeSync, existsSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, realpathSync, writeFileSync} from 'node:fs';
import {dirname, isAbsolute, join, relative, resolve, sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import {bootKey, capture, sha256, validatePreparation} from './rust-channel-recovery.mjs';

export const OWNER = '9f053173-8472-4860-be36-19a455a32e62';
export const BOOT_FENCE = 'cc31783a-9ed6-47a8-a150-e656b88bd90e';
export const AUTHORIZATION = 'PR14-user-authorized-stop-and-rerun-2026-10-16';
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const HASH = /^[0-9a-f]{64}$/;
const tools = dirname(fileURLToPath(import.meta.url));
function need(ok, code) { if (!ok) throw new Error(`readmit.${code}`); }
export function checkedPath(root, path) {
  const rel = relative(root, resolve(path));
  need(rel && !isAbsolute(rel) && rel !== '..' && !rel.startsWith(`..${sep}`), 'path_outside_repo');
  let cursor = root;
  for (const part of rel.split(sep)) { cursor = join(cursor, part); if (existsSync(cursor)) need(!lstatSync(cursor).isSymbolicLink(), 'reparse_boundary'); }
  return cursor;
}
export function equalSnapshot(expected, actual) {
  need(JSON.stringify(actual) === JSON.stringify(expected), 'original_evidence_changed');
  return true;
}
export function validateIdle(evidence, resources) {
  need(evidence?.schema === 'lycheedev.channel-live-idle.v1', 'observation_schema');
  need(evidence.originalFilesLocked === 3 && evidence.locksHeldThroughCommit === true, 'lock_lifetime');
  need(JSON.stringify(evidence.resourcePaths) === JSON.stringify(resources), 'resource_set');
  need(Array.isArray(evidence.dataIdentities) && evidence.dataIdentities.length === DATA_IDENTITIES.length && DATA_IDENTITIES.every((expected, i) => ['path', 'bytes', 'sha256'].every(key => evidence.dataIdentities[i]?.[key] === expected[key])), 'data_identity');
  need(evidence.lockedResources === resources.length && evidence.failedResources === 0, 'resource_lock_conflict');
  need(['relatedProcessesBefore', 'relatedProcessesAfter', 'restartManagerBefore', 'restartManagerAfter'].every(key => Array.isArray(evidence[key])), 'process_observation_missing');
  need(evidence.relatedProcessesBefore?.length === 0 && evidence.relatedProcessesAfter?.length === 0 && evidence.restartManagerBefore?.length === 0 && evidence.restartManagerAfter?.length === 0, 'current_process_or_resource_busy');
  need(evidence.watcherEvents === 0 && evidence.watcherErrors === 0 && evidence.observationMilliseconds >= 10000, 'writer_observed');
  need(Number.isSafeInteger(evidence.targetBefore?.files) && evidence.targetBefore.files >= 0, 'target_observation_missing');
  need(HASH.test(evidence.targetBefore?.sha256) && evidence.targetBefore.sha256 === evidence.targetAfter?.sha256 && evidence.targetBefore.files === evidence.targetAfter.files, 'target_changed');
  need(bootKey(evidence.beforeBoot) === bootKey(evidence.afterBoot) && BigInt(evidence.afterBoot.uptime100ns) >= BigInt(evidence.beforeBoot.uptime100ns), 'host_or_boot_changed');
  return true;
}
export function loadPreparation(root, digest) {
  root = realpathSync(root); need(HASH.test(digest), 'preparation_digest');
  const directory = checkedPath(root, join(root, '.tmp/rust-disk/recovery', OWNER, BOOT_FENCE));
  const raw = readFileSync(checkedPath(root, join(directory, 'prepared.json')));
  need(raw.length <= 4 << 20 && sha256(raw) === digest, 'preparation_digest');
  const prepared = validatePreparation(JSON.parse(raw));
  need(prepared.snapshot.files[2].path === `.tmp/rust-disk/evidence/${OWNER}/process-11299fbf-7a09-4dc8-ba5d-e440145136f6.request.json`, 'original_request_identity');
  need(prepared.root === root && prepared.snapshot.ownerId === OWNER && prepared.fenceId === BOOT_FENCE, 'preparation_target');
  need(prepared.toolHashes.javascript === sha256(readFileSync(join(tools, 'rust-channel-recovery.mjs'))) && prepared.toolHashes.powershell === sha256(readFileSync(join(tools, 'rust-channel-recovery.ps1'))), 'original_boot_tools_changed');
  need(!existsSync(join(directory, 'ordinary.lease.original')), 'boot_fence_already_committed');
  return {root, directory, prepared};
}
export function validateAdmission(intent, record) {
  need(record?.schema === 'lycheedev.channel-readmission.v1' && record.state === 'prepared' && record.mode === 'user-authorized-stop-current-idle', 'record_schema');
  for (const field of ['ownerId', 'bootFenceId', 'generation', 'preparationSHA256', 'originalLeaseSHA256', 'originalLeaseToken', 'archive', 'target', 'authorization']) need(record[field] === intent[field], 'record_identity');
  need(record.oldOwnerHistoricalState === 'unknown' && record.oldOwnerReaped === false && record.originalOwnerReceiptUnchanged === true && record.scratchPreserved === true && record.channel === 'ordinary', 'historical_claim');
  validateIdle(record.observation, intent.resourcePaths);
  need(bootKey(record.observation.beforeBoot) === intent.machineSHA256, 'different_machine');
  return true;
}
function flushNew(path, bytes) { const fd = openSync(path, 'wx'); try { writeFileSync(fd, bytes); fsyncSync(fd); } finally { closeSync(fd); } }
// Filled from the independently reported resource set, not caller-supplied paths.
export const RESOURCE_PATHS = Object.freeze([
  'target/debug/.cargo-lock', 'target/debug/.cargo-build-lock', 'target/debug/.cargo-artifact-lock',
  'target/release/.cargo-lock', 'target/release/.cargo-build-lock', 'target/release/.cargo-artifact-lock',
  'target/debug/deps/lycheedev_data-8a8a1a4e2cb0a02b.exe', 'target/debug/deps/lycheedev_data-8a8a1a4e2cb0a02b.pdb',
  'target/debug/lycheedev.exe', 'target/debug/lycheedev.pdb', 'target/.rustc_info.json', 'Cargo.lock',
]);
export const DATA_IDENTITIES = Object.freeze([
  {path: 'target/debug/deps/lycheedev_data-8a8a1a4e2cb0a02b.exe', bytes: 63029248, sha256: '5093d4a541a2b44fc4e7fbcfcabf28b7a7e8ead5e32210b5f29cc589b9274458'},
  {path: 'target/debug/deps/lycheedev_data-8a8a1a4e2cb0a02b.pdb', bytes: 139988992, sha256: 'e1c4e6f4c3f3f759dcafd8a665975fa1a6ae3b69b8a1396023d4c338f4846cdc'},
]);
export function apply(root, digest, authorized = false) {
  need(process.platform === 'win32', 'windows_required'); need(authorized === true, 'explicit_stop_authorization_required');
  const loaded = loadPreparation(root, digest); ({root} = loaded);
  const {prepared, directory} = loaded;
  equalSnapshot(prepared.snapshot, capture(root, OWNER));
  const generation = randomUUID(), attempt = checkedPath(root, join(directory, 'readmission', generation));
  mkdirSync(attempt, {recursive: true});
  const lease = JSON.parse(Buffer.from(prepared.snapshot.files[0].base64, 'base64'));
  const intent = {schema: 'lycheedev.channel-readmission-intent.v1', ownerId: OWNER, bootFenceId: BOOT_FENCE, generation,
    authorization: AUTHORIZATION, preparationSHA256: digest, originalLeaseSHA256: prepared.snapshot.files[0].sha256,
    originalLeaseToken: lease.token, machineSHA256: bootKey(prepared.boot), resourcePaths: RESOURCE_PATHS,
    archive: join(attempt, 'ordinary.lease.original'), target: join(root, 'target'),
    tools: {javascript: sha256(readFileSync(import.meta.filename)), powershell: sha256(readFileSync(join(tools, 'rust-channel-readmit.ps1')))},
    oldOwnerHistoricalState: 'unknown', oldOwnerReaped: false};
  const intentBytes = Buffer.from(JSON.stringify(intent, null, 2) + '\n');
  flushNew(join(attempt, 'intent.json'), intentBytes);
  console.error(JSON.stringify({state: 'readmission_attempt_reserved', generation, intent: join(attempt, 'intent.json'), leaseRelease: 'not_claimed'}));
  const run = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', join(tools, 'rust-channel-readmit.ps1'),
    '-Root', root, '-Generation', generation, '-IntentSHA256', sha256(intentBytes), '-CoordinatorPid', String(process.pid)],
    {encoding: 'utf8', windowsHide: true, shell: false, timeout: 120000, maxBuffer: 4 << 20});
  flushNew(join(attempt, 'wrapper-result.json'), Buffer.from(JSON.stringify({status: run.status, signal: run.signal, error: run.error?.message, stdout: run.stdout, stderr: run.stderr}, null, 2) + '\n'));
  if (run.error || run.status !== 0) { const error = new Error(`readmit.native_failed:${run.error?.message || run.stderr || run.status}`); error.generation = generation; throw error; }
  return inspect(root, digest, generation);
}
export function inspect(root, digest, generation) {
  const {root: canonical, directory, prepared} = loadPreparation(root, digest); root = canonical;
  need(UUID.test(generation), 'generation');
  const attempt = checkedPath(root, join(directory, 'readmission', generation));
  const intent = JSON.parse(readFileSync(checkedPath(root, join(attempt, 'intent.json'))));
  need(intent.schema === 'lycheedev.channel-readmission-intent.v1' && intent.ownerId === OWNER && intent.bootFenceId === BOOT_FENCE && intent.generation === generation && intent.preparationSHA256 === digest && intent.authorization === AUTHORIZATION && intent.originalLeaseSHA256 === prepared.snapshot.files[0].sha256 && intent.machineSHA256 === bootKey(prepared.boot) && intent.archive === join(attempt, 'ordinary.lease.original') && intent.target === join(root, 'target') && JSON.stringify(intent.resourcePaths) === JSON.stringify(RESOURCE_PATHS), 'intent_identity');
  need(intent.originalLeaseToken === JSON.parse(Buffer.from(prepared.snapshot.files[0].base64, 'base64')).token, 'intent_token');
  const archive = checkedPath(root, intent.archive), recordPath = checkedPath(root, join(attempt, 'admission.json'));
  if (existsSync(recordPath)) validateAdmission(intent, JSON.parse(readFileSync(recordPath)));
  if (existsSync(archive)) {
    need(existsSync(recordPath), 'archive_without_admission');
    need(readFileSync(archive).toString('base64') === prepared.snapshot.files[0].base64, 'archive_integrity');
    for (const row of prepared.snapshot.files.slice(1)) need(sha256(readFileSync(checkedPath(root, join(root, row.path)))) === row.sha256, 'original_evidence_changed');
    return {state: 'readmission_commit_observed', generation, archive, admission: recordPath, oldOwnerReaped: false, target: intent.target,
      currentLease: existsSync(join(root, '.tmp/rust-disk/channels/ordinary.lease')) ? 'present_do_not_modify' : 'absent'};
  }
  equalSnapshot(prepared.snapshot, capture(root, OWNER));
  return {state: existsSync(recordPath) ? 'prepared_not_committed' : 'attempt_not_committed', generation, oldOwnerReaped: false, lease: 'original_unchanged', retry: 'A new apply attempt must reacquire every lock and repeat live observation; preserve this attempt.'};
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  try {
    const [command, root, digest, selector, ...rest] = process.argv.slice(2); need(root && digest && !rest.length, 'usage');
    const result = command === 'apply' && selector === '--user-authorized-stop' ? apply(root, digest, true)
      : command === 'inspect' && selector ? inspect(root, digest, selector) : (() => { throw new Error('usage: apply <repo> <original prepared SHA256> --user-authorized-stop | inspect <repo> <original prepared SHA256> <generation UUID>'); })();
    console.log(JSON.stringify(result, null, 2));
  } catch (error) { console.error(JSON.stringify({error: error.message, generation: error.generation, leaseRelease: 'not_claimed', next: 'Inspect this exact attempt after any uncertain response. Never delete a lease or rewrite an old receipt.'})); process.exitCode = 1; }
}
