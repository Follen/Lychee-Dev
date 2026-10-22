// CI test-program production and execution.
//
// BUILD stage: `cargo test --no-run` runs exactly once for the test/development
// configuration; the resulting test harness executables (and the binary targets
// their compile-time CARGO_BIN_EXE_* paths point at) are copied into the build
// artifact with a sealed record.
//
// VALIDATE stage: the recorded executables are executed directly. There is no
// Cargo invocation and no "not found -> build" fallback: a missing record, a
// stale source tree, a changed byte or a path from another workspace fails with
// an explicit error. This is what makes "每种必要配置只编译一次" observable.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { runCargo, sourceDigest, ownedUserEnvironment } from './rust-build.mjs';
import { ownedSpawnSync } from './rust-owned-process.mjs';
import { runPrebuiltBridge } from './rust-formal-lua-bridge.mjs';

const repository = resolve(import.meta.dirname ?? fileURLToPath(new URL('.', import.meta.url)), '..');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export const RECORD_SCHEMA = 'lycheedev.ci-test-programs.v1';
const CARGO_ARGS = Object.freeze(['test', '--locked', '--offline', '--workspace', '--all-targets', '--no-run', '--message-format=json-render-diagnostics', '-j', '1']);
// Only real libtest harnesses are executed. Benches/examples are not business
// tests, and targets that declare `harness = false` are helper programs for a
// parent test (e.g. the guarded spawn fixture), not standalone tests.
const harnessKinds = new Set(['lib', 'bin', 'test', 'proc-macro']);

function contained(root, path) {
  const absolute = resolve(root, path), remainder = relative(root, absolute);
  if (!absolute.startsWith(root + sep) || remainder === '' || isAbsolute(remainder) || remainder.startsWith('..')) {
    throw new Error(`ci-test-programs.outside_repository: ${path}`);
  }
  return absolute;
}

function gitHead(root) {
  const result = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8', windowsHide: true, shell: false, timeout: 60000 });
  if (result.error || result.status !== 0) throw new Error(`ci-test-programs.git_failed: ${result.error?.message ?? result.status}`);
  return result.stdout.trim();
}

function cargoMessages(stdout) {
  const messages = [];
  for (const line of stdout.split(/\r?\n/)) {
    if (!line.trim()) continue;
    try { messages.push(JSON.parse(line)); } catch { throw new Error(`ci-test-programs.unparseable_cargo_message: ${line.slice(0, 200)}`); }
  }
  return messages;
}

// `[[test]] name = "..." harness = false` entries describe helper executables.
function harnesslessTargets(root) {
  const helpers = new Set();
  for (const entry of readdirSync(join(root, 'crates'), { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const manifest = join(root, 'crates', entry.name, 'Cargo.toml');
    if (!existsSync(manifest)) continue;
    const blocks = readFileSync(manifest, 'utf8').replaceAll('\r\n', '\n').split(/^\[\[test\]\]$/m).slice(1);
    for (const block of blocks) {
      const name = /^name\s*=\s*"([^"]+)"/m.exec(block)?.[1];
      if (name && /^harness\s*=\s*false\s*$/m.test(block)) helpers.add(name);
    }
  }
  return helpers;
}

function slug(value) {
  return value.replace(/[^A-Za-z0-9._-]+/g, '_').replace(/^_+|_+$/g, '');
}

function copyProgram(root, source, destination, bytes) {
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(source, destination);
  if (hash(readFileSync(destination)) !== hash(bytes)) throw new Error(`ci-test-programs.copy_changed: ${destination}`);
}

export function buildTestPrograms({ root = repository, output, owner = 'ci-test-programs-build' } = {}) {
  root = resolve(root);
  if (!output) throw new Error('ci-test-programs.output_required');
  const out = contained(root, output);
  if (existsSync(out)) throw new Error(`ci-test-programs.output_exists: ${output}`);
  mkdirSync(out, { recursive: true });
  const helpers = harnesslessTargets(root);
  const record = runCargo(root, [...CARGO_ARGS], {
    owner, oneShot: false,
    onSuccess(invocation) {
      const messages = cargoMessages(invocation.stdout.toString());
      const rows = messages.filter(row => row.reason === 'compiler-artifact' && row.executable && row.target && row.profile);
      const tests = [];
      const support = [];
      const excluded = [];
      const seen = new Set();
      for (const row of rows) {
        const kinds = row.target.kind ?? [];
        if (!kinds.some(kind => harnessKinds.has(kind))) continue;
        if (seen.has(row.executable)) continue;
        seen.add(row.executable);
        const isTestHarness = row.profile.test === true;
        if (isTestHarness && kinds.includes('test') && helpers.has(row.target.name)) {
          excluded.push({ package: packageName(row), target: row.target.name, reason: 'harness=false helper program' });
          continue;
        }
        const source = contained(root, row.executable);
        const bytes = readFileSync(source);
        // The current debug binary target is 134,360,576 B, above the former
        // 128 MiB guard. Keep a fixed 512 MiB bound that still rejects runaway
        // outputs while accepting the actual debug CLI executor.
        if (bytes.length > 512 << 20) throw new Error(`ci-test-programs.executable_budget: ${source}`);
        const entry = { package: packageName(row), target: row.target.name, kinds, executable: hash(source).slice(0, 12) + '-' + slug(row.target.name) };
        if (isTestHarness) {
          const destination = join(out, 'programs', `${entry.package}__${entry.executable}.exe`);
          copyProgram(root, source, destination, bytes);
          tests.push({ ...entry, path: relative(root, destination).replaceAll('\\', '/'), sha256: hash(bytes), bytes: bytes.length });
        } else {
          // Compile-time CARGO_BIN_EXE_* paths point into the cargo target
          // directory. Recreate that exact absolute path from this artifact in
          // validate before running the tests.
          const destination = join(out, 'support', `${entry.package}__${entry.executable}.exe`);
          copyProgram(root, source, destination, bytes);
          support.push({ ...entry, path: relative(root, destination).replaceAll('\\', '/'), destination: source, sha256: hash(bytes), bytes: bytes.length });
        }
      }
      const complete = messages.filter(row => row.reason === 'build-finished');
      assert.equal(complete.length, 1, 'ci-test-programs.cargo_completion_cardinality');
      assert.equal(complete[0].success, true, 'ci-test-programs.cargo_completion_failed');
      const value = {
        schema: RECORD_SCHEMA, root, commit: gitHead(root), sourceTree: invocation.inputHashes.sourceTree, cargoTargetDir: invocation.target,
        command: ['cargo', ...CARGO_ARGS], environmentSHA256: invocation.environmentSHA256, buildEvidence: invocation.buildEvidence,
        programs: tests, support, excluded, producedAtUtc: new Date().toISOString(),
      };
      writeFileSync(join(out, 'test-programs.json'), `${JSON.stringify(value, null, 2)}\n`);
      return value;
    },
  });
  if (record.schema !== RECORD_SCHEMA) throw new Error('ci-test-programs.invalid_record');
  return record;
}

export function packageName(row) {
  const value = String(row.package_id ?? ''), fragment = value.slice(value.lastIndexOf('#') + 1);
  if (fragment.includes('@')) return fragment.slice(0, fragment.indexOf('@'));
  // Cargo omits the name when a path package has its directory's name:
  // path+file:///.../lycheedev-live#3.5.0.
  if (value.startsWith('path+file:')) {
    const path = decodeURIComponent(new URL(value.slice(5, value.lastIndexOf('#'))).pathname);
    return path.replace(/\/$/, '').split('/').at(-1);
  }
  throw new Error(`ci-test-programs.package_identity: ${value}`);
}

export function readTestRecord(root, recordPath) {
  const path = contained(root, recordPath);
  if (!existsSync(path)) throw new Error(`ci-test-programs.record_missing: ${recordPath}`);
  const record = JSON.parse(readFileSync(path, 'utf8'));
  if (record.schema !== RECORD_SCHEMA) throw new Error(`ci-test-programs.unknown_record: ${record.schema}`);
  if (resolve(record.root) !== root) throw new Error(`ci-test-programs.workspace_mismatch: built at ${record.root}`);
  if (record.commit !== gitHead(root)) throw new Error('ci-test-programs.commit_mismatch');
  const current = sourceDigest(root);
  if (record.sourceTree !== current) throw new Error('ci-test-programs.source_tree_mismatch');
  return record;
}

function verifyEntry(root, entry) {
  const path = contained(root, entry.path);
  if (!existsSync(path)) throw new Error(`ci-test-programs.program_missing: ${entry.path}`);
  const bytes = readFileSync(path);
  if (bytes.length !== entry.bytes || hash(bytes) !== entry.sha256) throw new Error(`ci-test-programs.program_changed: ${entry.path}`);
  return path;
}

function prepareSupport(root, record) {
  const cargoTarget = resolve(record.cargoTargetDir);
  if (!cargoTarget.startsWith(root + sep)) throw new Error('ci-test-programs.support_target_outside_repository');
  for (const entry of record.support ?? []) {
    const binary = verifyEntry(root, entry);
    const destination = resolve(entry.destination);
    if (!destination.startsWith(cargoTarget + sep)) throw new Error(`ci-test-programs.support_destination_outside_target: ${entry.destination}`);
    mkdirSync(dirname(destination), { recursive: true });
    copyFileSync(binary, destination);
  }
}

export const isFormalBridge = program => program.target === 'formal_lua_bridge' && program.kinds.includes('test');

export function runTestPrograms({ root = repository, recordPath, packages, targets, reportPath, formalLuaReport, formalBridge = runPrebuiltBridge, executor = ownedSpawnSync, arguments: extraArguments = ['--test-threads=1'] } = {}) {
  root = resolve(root);
  const record = readTestRecord(root, recordPath);
  prepareSupport(root, record);
  const selected = record.programs.filter(program =>
    (!packages?.length || packages.includes(program.package)) && (!targets?.length || targets.includes(program.target)));
  if (!selected.length) throw new Error(`ci-test-programs.no_selected_programs: packages=${(packages ?? []).join(',') || '*'} targets=${(targets ?? []).join(',') || '*'}`);
  if(selected.some(isFormalBridge) && !formalLuaReport) throw new Error('ci-test-programs.formal_lua_report_required');
  const runRoot = join(root, '.tmp', 'ci', 'run');
  mkdirSync(runRoot, { recursive: true });
  const evidence = join(runRoot, 'evidence');
  mkdirSync(evidence, { recursive: true });
  const environment = ownedUserEnvironment(join(runRoot, 'user'));
  const results = [];
  let handlesReaped = true, failed = 0, unreapedError;
  try {
    for (const program of selected) {
      const executable = verifyEntry(root, program);
      const started = Date.now();
      handlesReaped = false;
      if(isFormalBridge(program)){
        try {
          const bridge = formalBridge({runtimePath: contained(root, formalLuaReport), recordPath: contained(root, recordPath), record, program});
          handlesReaped = true;
          results.push({package: program.package, target: program.target, executable: program.path, sha256: program.sha256, exit: 0, ok: bridge.passed, handlesReaped, millis: Date.now() - started, bridgeReport: bridge.report});
        } catch(error) {
          handlesReaped = error.handlesReaped === true;
          failed++;
          results.push({package: program.package, target: program.target, executable: program.path, sha256: program.sha256, exit: null, ok: false, handlesReaped, millis: Date.now() - started, error: error.message, bridgeEvidence: error.bridgeEvidence});
          if(!handlesReaped){unreapedError = error; break;}
        }
        continue;
      }
      const result = executor(executable, extraArguments, { cwd: root, env: environment, evidence, timeout: 900000 });
      handlesReaped = result.handlesReaped === true;
      if (result.stdout) process.stdout.write(result.stdout);
      if (result.stderr) process.stderr.write(result.stderr);
      const ok = handlesReaped && !result.error && result.status === 0;
      results.push({ package: program.package, target: program.target, executable: program.path, sha256: program.sha256, exit: result.status, signal: result.signal, ok, handlesReaped, error: result.error?.message, millis: Date.now() - started,
        ...(!handlesReaped ? { proof: result.proof, logs: result.logs } : {}) });
      if (!ok) failed++;
      if (!handlesReaped) { unreapedError = result.error; break; }
    }
    const report = {
      schema: 'lycheedev.ci-test-run.v1', state: failed ? 'failed' : 'passed', sourceTree: record.sourceTree,
      packages: packages ?? [], targets: targets ?? [], arguments: extraArguments,
      programs: selected.length, attempted: results.length, passed: results.filter(row => row.ok).length, failed, results,
    };
    if (reportPath) {
      const path = contained(root, reportPath);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, `${JSON.stringify(report, null, 2)}\n`);
    }
    if (unreapedError) throw unreapedError;
    if (failed) throw new Error(`ci-test-programs.tests_failed: ${failed} program(s)`);
    return report;
  } finally {
    if (!handlesReaped) process.stderr.write('ci-test-programs.warning: a test program tree was not reaped\n');
  }
}

function parseList(value) {
  if (!value) return undefined;
  return value.split(',').map(item => item.trim()).filter(Boolean);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const argv = process.argv.slice(2), command = argv[0];
    const options = {};
    const rest = [];
    for (let index = 1; index < argv.length; index++) {
      const arg = argv[index], value = argv[index + 1];
      if (arg === '--package') { options.packages = parseList(value); index++; }
      else if (arg === '--target') { options.targets = parseList(value); index++; }
      else if (arg === '--report') { options.reportPath = value; index++; }
      else if (arg === '--arguments') { options.arguments = parseList(value); index++; }
      else if (arg === '--formal-lua') { options.formalLuaReport = value; index++; }
      else if (arg === '--owner') { options.owner = value; index++; }
      else if (arg.startsWith('--')) throw new Error(`ci-test-programs.unknown_option: ${arg}`);
      else rest.push(arg);
    }
    if (command === 'build') {
      if (rest.length !== 1) throw new Error('usage: node tools/ci-test-programs.mjs build <new-output-directory> [--owner name]');
      const record = buildTestPrograms({ output: rest[0], ...(options.owner ? { owner: options.owner } : {}) });
      process.stdout.write(`${JSON.stringify({ schema: record.schema, programs: record.programs.length, support: record.support.length, excluded: record.excluded, sourceTree: record.sourceTree }, null, 2)}\n`);
    } else if (command === 'run') {
      if (rest.length !== 1) throw new Error('usage: node tools/ci-test-programs.mjs run <test-programs.json> [--package a,b] [--target a,b] [--report path] [--formal-lua build-report.json] [--arguments --test-threads=1]');
      const report = runTestPrograms({ recordPath: rest[0], ...options });
      process.stdout.write(`${JSON.stringify({ state: report.state, programs: report.programs, passed: report.passed, failed: report.failed }, null, 2)}\n`);
    } else throw new Error('usage: node tools/ci-test-programs.mjs build <new-directory> | run <test-programs.json> [options]');
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
