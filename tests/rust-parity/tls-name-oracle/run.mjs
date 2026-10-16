// This replaces only the existing remote:: invocation; it never repeats Cargo.
// Go has its own registered scratch/cache and bounded, reaped process tree.
import assert from 'node:assert/strict';
import {appendFileSync, mkdirSync, readFileSync, realpathSync, writeFileSync} from 'node:fs';
import {basename, dirname, join, resolve} from 'node:path';
import {hashFile, runCargo, sourceDigest} from '../../../tools/rust-build.mjs';
import {beginScratch, contained, sha256} from '../../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../../tools/rust-owned-process.mjs';
import {compare, rustObservations} from './compare.mjs';

export const CARGO_ARGS = Object.freeze(['test', '--locked', '--offline', '-p', 'lycheedev-data', '--lib', 'remote::', '-j', '1', '--', '--skip', 'hotfix::', '--nocapture']);
const OWNER = 'ci-data-remote';
const UUID = /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/;
const ORACLE = 'tests/rust-parity/tls-name-oracle/main.go';
const FIXTURES = 'crates/lycheedev-data/src/remote/transport/raw_uri/tls_fixtures';
const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n', {flag: 'wx'});

// Accept an owner UUID only, never a user-selected log path or newest-owner guess.
export function sealedRust(root, id) {
  assert(UUID.test(id), 'TLS Rust owner must be one exact UUID');
  root = realpathSync(root);
  const evidence = contained(root, join(root, '.tmp/rust-disk/evidence', id));
  const path = contained(root, join(evidence, 'receipt.json'));
  const receiptSHA256 = hashFile(path, 1 << 20);
  const receipt = JSON.parse(readFileSync(path));
  assert.equal(receipt.schema, 'lycheedev.disk-owner.v1');
  assert.equal(receipt.id, id);
  assert.equal(receipt.owner, OWNER);
  assert(['succeeded', 'failed', 'retired'].includes(receipt.state), 'Rust owner must be terminal');
  assert.equal(receipt.handlesReaped, true, 'Rust process tree must be reaped');
  assert.deepEqual(receipt.command, ['cargo', ...CARGO_ARGS]);
  assert.equal(receipt.commandSHA256, sha256(Buffer.from(JSON.stringify(receipt.command))));
  assert.equal(receipt.inputHashes.sourceTree, sourceDigest(root), 'Rust observations must come from the current exact source tree');
  assert(Array.isArray(receipt.sealedLogs) && receipt.sealedLogs.length >= 3);
  const seen = new Set();
  for (const log of receipt.sealedLogs) {
    const checked = contained(evidence, log.path);
    assert(!seen.has(checked), 'duplicate sealed path');
    seen.add(checked);
    assert.equal(hashFile(checked, 16 << 20), log.sha256, 'sealed Rust log integrity');
  }
  function log(name) {
    const file = join(evidence, name);
    assert(seen.has(file), `missing sealed ${name}`);
    return file;
  }
  const command = JSON.parse(readFileSync(log('command.json')));
  assert.equal(command.command, 'cargo');
  assert.deepEqual(command.args, CARGO_ARGS);
  assert.equal(realpathSync(command.cwd), root);
  assert.equal(command.oneShot, true);
  assert.equal(command.inputHashes.sourceTree, receipt.inputHashes.sourceTree);
  assert(Number.isInteger(command.exit), 'Rust command needs a terminal exit code');
  const stdout = log('stdout.bin');
  return {id, path, receiptSHA256, stdout,
    stdoutSHA256: hashFile(stdout, 16 << 20), sourceTree: receipt.inputHashes.sourceTree,
    exit: command.exit, commandError: command.error ?? null};
}

export function runRust(root, cargo = runCargo) {
  let output, error;
  try { output = cargo(root, CARGO_ARGS, {owner: OWNER, oneShot: true}); }
  catch (failure) { error = failure; }
  const path = output?.buildEvidence ?? error?.buildEvidence;
  // Admission failures have no executable evidence; do not adopt an older run.
  const rust = path ? sealedRust(root, basename(dirname(path))) : null;
  if (rust) assert.equal(resolve(path), resolve(rust.path));
  return {rust, stdout: output?.stdout ?? '', stderr: output?.stderr ?? '', error: error?.message ?? null,
    passed: !error && rust?.exit === 0 && !rust.commandError};
}

export function runGo(root, id, execute = ownedSpawnSync) {
  root = realpathSync(root);
  const rust = sealedRust(root, id);
  const sources = [ORACLE, 'tests/rust-parity/tls-name-oracle/run.mjs', 'tests/rust-parity/tls-name-oracle/compare.mjs',
    ...['root.pem', 'server.pem', 'server-key.pem', 'server.p12'].map(name => `${FIXTURES}/${name}`)];
  const inputs = Object.fromEntries(sources.map(path => [path, hashFile(join(root, path), 1 << 20)]));
  const job = beginScratch(root, 'ci-tls-name-go-oracle', {
    command: ['go', 'run', ORACLE, '-fixtures', FIXTURES],
    inputs: {...inputs, rustReceipt: rust.receiptSHA256, rustStdout: rust.stdoutSHA256},
    admissionOptions: {requiredBytes: 512 << 20, scratchReserveBytes: 512 << 20}
  });
  let logs = [], handlesReaped = true;
  const report = {schema: 'lycheedev.tls-name-owned-comparison.v1', state: 'failed', rust,
    scope: 'Only the five private-root loopback cases; no default Windows system-root equivalence claim',
    go: [], comparison: null, errors: []};
  const reportPath = join(job.evidence, 'comparison.json');
  try {
    const cache = join(job.directory, 'go-cache'), temporary = join(job.directory, 'tmp');
    mkdirSync(cache); mkdirSync(temporary);
    const env = {...process.env, GOTOOLCHAIN: 'local', GOPROXY: 'off', GOSUMDB: 'off', GOWORK: 'off',
      GOENV: 'off', GOFLAGS: '', CGO_ENABLED: '0', GO111MODULE: 'off',
      GOCACHE: cache, GOTMPDIR: temporary, TEMP: temporary, TMP: temporary};
    const invoke = (command, args, timeout) => {
      // A thrown executor after starting a process does not establish reaping.
      handlesReaped = false;
      const result = execute(command, args, {cwd: root, env, evidence: job.evidence, timeout, orphanMs: 5000});
      logs.push(...(result.logs ?? []));
      handlesReaped = result.handlesReaped === true;
      report.go.push({command, args, exit: result.status, error: result.error?.message ?? null,
        handlesReaped, proof: result.proof ?? null});
      assert(handlesReaped, 'Go process tree not reaped');
      return result;
    };
    const version = invoke('go', ['version'], 15000);
    assert.equal(version.status, 0); assert(!version.error);
    assert.equal(version.stdout.toString().trim(), 'go version go1.27.1 windows/amd64');
    const go = version.proof?.binary;
    assert(go && basename(go).toLowerCase() === 'go.exe', 'owned Go binary identity required');
    const toolSHA256 = hashFile(go);
    report.goTool = {path: go, sha256: toolSHA256, version: version.stdout.toString().trim()};
    const result = invoke(go, ['run', join(root, ORACLE), '-fixtures', join(root, FIXTURES)], 120000);
    const stdout = join(job.evidence, 'go-results.json'), stderr = join(job.evidence, 'go-stderr.bin');
    writeFileSync(stdout, result.stdout, {flag: 'wx'}); writeFileSync(stderr, result.stderr, {flag: 'wx'});
    logs.push(stdout, stderr);
    assert.equal(resolve(result.proof?.binary ?? ''), resolve(go));
    assert.equal(hashFile(go), toolSHA256, 'Go executable changed during oracle');
    for (const path of sources) assert.equal(hashFile(join(root, path), 1 << 20), inputs[path], `oracle input changed: ${path}`);
    assert.equal(hashFile(rust.path, 1 << 20), rust.receiptSHA256, 'Rust receipt changed');
    assert.equal(hashFile(rust.stdout, 16 << 20), rust.stdoutSHA256, 'Rust output changed');
    // Preserve and compare actual observations even if the Go expectation check failed.
    report.comparison = compare(JSON.parse(result.stdout.toString('utf8')), rustObservations(readFileSync(rust.stdout, 'utf8')));
    if (result.status !== 0 || result.error) report.errors.push(`Go oracle failed: ${result.error?.message ?? result.status}`);
    if (report.comparison.differences.length) report.errors.push('Go/Rust private-root TLS observations differ');
    if (report.errors.length === 0) report.state = 'succeeded';
  } catch (error) {
    report.errors.push(error.message);
  } finally {
    if (!handlesReaped) report.state = 'interrupted';
    report.handlesReaped = handlesReaped;
    save(reportPath, report);
    job.seal({state: report.state, handlesReaped, logs: [...logs, reportPath]});
    // Only a successful, reaped oracle may retire its registered cache/temp.
    // Failed or unknown execution retains scratch for diagnosis.
    if (report.state === 'succeeded' && handlesReaped) job.cleanup();
  }
  return {...report, receipt: job.receipt, reportPath};
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  assert.equal(process.platform, 'win32', 'actual TLS comparison requires Windows');
  const mode = process.argv[2];
  assert(process.argv.length === 3 && ['rust', 'go'].includes(mode), 'usage: node run.mjs rust|go');
  if (mode === 'rust') {
    const result = runRust(process.cwd());
    process.stdout.write(result.stdout); process.stderr.write(result.stderr);
    // GITHUB_OUTPUT is the CI-provided step output channel, never an observation sink.
    if (result.rust && process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `tls_rust_owner=${result.rust.id}\n`);
    console.log(JSON.stringify({rust: result.rust, error: result.error}));
    if (!result.passed) process.exitCode = 1;
  } else {
    const result = runGo(process.cwd(), process.env.LYCHEEDEV_TLS_RUST_OWNER);
    console.log(JSON.stringify(result, null, 2));
    if (result.state !== 'succeeded') process.exitCode = 1;
  }
}
