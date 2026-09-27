// npm owns replacement of the CLI package. Native delivery owns target identity,
// receipts and recoverable addon/skill replacement. No native process stays alive
// while npm replaces its executable (Windows denies replacing a running image).
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, openSync, closeSync, readFileSync, writeFileSync, renameSync, rmSync, realpathSync } from 'node:fs';
import { delimiter, dirname, join, resolve } from 'node:path';

function readJSON(file) {
  const info = lstatSync(file);
  if (!info.isFile() || info.size > 1024 * 1024) throw new Error('update.invalid_state_file');
  return JSON.parse(readFileSync(file, 'utf8'));
}

function npmEntry() {
  for (const directory of [dirname(process.execPath), ...(process.env.PATH ?? '').split(delimiter)]) {
    const path = join(directory, 'node_modules', 'npm', 'bin', 'npm-cli.js');
    if (existsSync(path)) return path;
  }
  throw new Error('update.npm_missing: npm-cli.js was not found beside Node or a PATH entry');
}

function command(file, args, timeout = 600000) {
  const result = spawnSync(file, args, { encoding: 'utf8', shell: false, windowsHide: true, timeout, maxBuffer: 16 * 1024 * 1024 });
  if (result.error) throw result.error;
  return result;
}

function nativeResult(binary, args) {
  const run = command(binary, args);
  let envelope;
  try { envelope = JSON.parse(run.stdout); } catch { throw new Error(`update.invalid_native_result: ${run.stderr.slice(0, 2000)}`); }
  if (run.status !== 0 || !envelope.ok) {
    const error = new Error(envelope.error?.message ?? 'update.native_failed');
    error.envelope = envelope; error.exit = run.status || 5;
    throw error;
  }
  return envelope;
}

function frozenTargets(plan) {
  if (!Array.isArray(plan?.targets) || plan.targets.length > 64) throw new Error('update.invalid_targets');
  return plan.targets.map(target => {
    if (!['skill', 'addon'].includes(target.component) || typeof target.path !== 'string' || !target.path) throw new Error('update.invalid_target');
    return { component: target.component, path: target.path };
  });
}

function takeLock(directory) {
  const file = join(directory, 'lock.json');
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const descriptor = openSync(file, 'wx');
      writeFileSync(descriptor, JSON.stringify({ pid: process.pid })); closeSync(descriptor);
      return () => rmSync(file);
    } catch (error) {
      if (error.code !== 'EEXIST') throw error;
      const owner = readJSON(file);
      if (!Number.isSafeInteger(owner.pid) || owner.pid < 1) throw new Error('update.invalid_lock');
      try { process.kill(owner.pid, 0); } catch (error) {
        if (error.code === 'ESRCH') {
          // Serialize stale reclamation separately. Two retriers must not remove
          // each other's newly acquired owner file after observing one dead PID.
          const reclaim = join(directory, 'reclaim.lock');
          let descriptor;
          try { descriptor = openSync(reclaim, 'wx'); } catch { throw new Error('update.busy: stale-lock recovery is already in progress'); }
          try {
            const current = readJSON(file);
            if (current.pid !== owner.pid) throw new Error('update.busy: owner changed during recovery');
            try { process.kill(current.pid, 0); throw new Error('update.busy'); } catch (error) {
              if (error.code !== 'ESRCH') throw error;
            }
            rmSync(file);
          } finally { closeSync(descriptor); rmSync(reclaim); }
          continue;
        }
      }
      throw new Error('update.busy: another updater owns this npm prefix');
    }
  }
  throw new Error('update.busy');
}

export function update(root, args, nativePath) {
  root = realpathSync(root);
  const prefix = dirname(dirname(root));
  if (root.toLowerCase() !== join(prefix, 'node_modules', 'lycheedev').toLowerCase() || !existsSync(join(prefix, 'lycheedev.cmd'))) {
    throw new Error('update.global_install_required: use a global npm installation, or update --release for native payloads');
  }
  const npm = npmEntry();
  // Validate arguments and all existing targets before downloads or npm writes.
  const plan = nativeResult(nativePath(root), [...args, '--plan', '--format=json']);
  const directory = join(prefix, '.lycheedev-update');
  mkdirSync(directory, { recursive: true });
  if (lstatSync(directory).isSymbolicLink() || realpathSync(directory).toLowerCase() !== resolve(directory).toLowerCase()) throw new Error('update.redirected_state');
  const unlock = takeLock(directory);
  const journal = join(directory, 'transaction.json');
  const save = value => {
    writeFileSync(`${journal}.next`, JSON.stringify(value));
    renameSync(`${journal}.next`, journal);
  };
  const runNpm = flags => {
    const output = command(process.execPath, [npm, ...flags]);
    if (output.status !== 0) throw new Error(`update.npm_failed: ${output.stderr.slice(-4000)}`);
    return output;
  };
  let transaction;
  try {
    if (existsSync(journal)) {
      transaction = readJSON(journal);
      if (transaction.schema !== 'lycheedev.npm-update.v1' || transaction.prefix !== prefix || JSON.stringify(transaction.args) !== JSON.stringify(args)) {
        throw new Error('update.pending_conflict: retry the original update arguments before selecting other targets');
      }
    } else {
      process.stderr.write('lycheedev: preparing the latest release in an isolated directory\n');
      const packed = JSON.parse(runNpm(['pack', 'lycheedev@latest', '--ignore-scripts', '--json', '--pack-destination', directory]).stdout);
      const entry = packed[0];
      if (packed.length !== 1 || entry.name !== 'lycheedev' || !/^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(entry.version) || entry.filename !== `lycheedev-${entry.version}.tgz`) throw new Error('update.invalid_package');
      transaction = { schema: 'lycheedev.npm-update.v1', prefix, args, version: entry.version, integrity: entry.integrity, targets: plan.result.targets };
      save(transaction);
    }
    if (!/^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(transaction.version)) throw new Error('update.invalid_version');
    const tarball = join(directory, `lycheedev-${transaction.version}.tgz`);
    const tarInfo = lstatSync(tarball);
    if (!tarInfo.isFile() || tarInfo.size > 512 * 1024 * 1024) throw new Error('update.package_size');
    const integrity = `sha512-${createHash('sha512').update(readFileSync(tarball)).digest('base64')}`;
    if (integrity !== transaction.integrity) throw new Error('update.package_integrity');
    const staging = join(directory, 'staging');
    runNpm(['install', '--prefix', staging, '--ignore-scripts', '--no-audit', '--no-fund', '--package-lock=false', tarball]);
    const stagedRoot = join(staging, 'node_modules', 'lycheedev');
    const stagedPackage = JSON.parse(readFileSync(join(stagedRoot, 'package.json'), 'utf8'));
    if (stagedPackage.version !== transaction.version || stagedPackage.name !== 'lycheedev') throw new Error('update.version_mismatch');
    const selected = frozenTargets({ targets: transaction.targets });
    const targetFile = join(directory, 'targets.json');
    writeFileSync(targetFile, JSON.stringify(selected));
    // Preserve only --home from the validated original args; target discovery is
    // frozen in the journal, output rendering belongs to this launcher.
    const home = [];
    for (let i = 1; i < args.length; i++) {
      if (args[i] === '--home') home.push('--home', args[++i]);
      else if (args[i].startsWith('--home=')) home.push(args[i]);
    }
    const nativeArgs = ['update', '--file', targetFile, ...home, '--format=json'];
    nativeResult(nativePath(stagedRoot), [...nativeArgs, '--release', stagedRoot, '--plan']);
    process.stderr.write(`lycheedev: installing ${transaction.version} and synchronizing ${transaction.targets.length} target(s)\n`);
    runNpm(['install', '--global', '--prefix', prefix, '--ignore-scripts', '--no-audit', '--no-fund', tarball]);
    const installed = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
    if (installed.version !== transaction.version) throw new Error('update.installed_version_mismatch');
    const outcome = nativeResult(nativePath(root), [...nativeArgs, '--release', root]);
    outcome.result.cli = { version: transaction.version, state: 'current', prefix };
    // Remove only the updater-owned staging and pinned archive after native
    // verification. A failure retains the exact release and targets for retry.
    // Commit success first: a cache deletion failure must not leave a journal
    // pointing at a tarball which has already been removed.
    rmSync(journal);
    try {
      if (realpathSync(staging).toLowerCase() !== resolve(staging).toLowerCase()) throw new Error('redirected staging directory');
      rmSync(staging, { recursive: true, force: true });
      rmSync(tarball);
      rmSync(targetFile);
    } catch (error) { outcome.warnings.push(`Update completed; temporary download cleanup needs attention at ${directory}: ${error.message}`); }
    return { envelope: outcome, status: 0 };
  } catch (error) {
    const envelope = error.envelope ?? { schema: 'lycheedev.result.v1', ok: false, operationId: '', context: {}, result: { complete: false }, captures: [], warnings: [], error: { code: 'update.failed', message: error.message, stage: 'update', retryable: true } };
    envelope.context.updateRecovery = journal;
    return { envelope, status: error.exit || 5 };
  } finally { unlock(); }
}
