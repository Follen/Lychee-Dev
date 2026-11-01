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
import {copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { runCargo, sourceDigest, ownedUserEnvironment, validateCargoJobs, preserveBuildEvidence } from './rust-build.mjs';
import { ownedSpawnSync } from './rust-owned-process.mjs';
import { runPrebuiltBridge } from './rust-formal-lua-bridge.mjs';

const repository = resolve(import.meta.dirname ?? fileURLToPath(new URL('.', import.meta.url)), '..');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
export const RECORD_SCHEMA = 'lycheedev.ci-test-programs.v1';
export const TEST_SHARDS = Object.freeze({
  'data-delivery': Object.freeze(['lycheedev-data', 'lycheedev-delivery', 'lycheedev-binding-contracts', 'lycheedev-publish']),
  'source-runtime': Object.freeze(['lycheedev-source', 'lycheedev-live', 'lycheedev-cli', 'lycheedev-core', 'lycheedev-windows', 'lycheedev-emmy-adapter']),
});
const CARGO_ARGS = Object.freeze(['test', '--locked', '--offline', '--workspace', '--all-targets', '--no-run', '--message-format=json-render-diagnostics']);
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

export function buildTestPrograms({ root = repository, output, owner = 'ci-test-programs-build', jobs = 1, timings = false, execute = ownedSpawnSync } = {}) {
  root = resolve(root);
  const args = [...CARGO_ARGS, '-j', String(validateCargoJobs(jobs))];
  if (timings) args.push('--timings');
  if (!output) throw new Error('ci-test-programs.output_required');
  const out = contained(root, output);
  if (existsSync(out)) throw new Error(`ci-test-programs.output_exists: ${output}`);
  mkdirSync(out, { recursive: true });
  const helpers = harnesslessTargets(root);
  const record = runCargo(root, args, {
    owner, oneShot: false, isolateUser: true, execute,
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
        const isHelper = kinds.includes('test') && helpers.has(row.target.name);
        if (isHelper) {
          excluded.push({ package: packageName(row), target: row.target.name, reason: 'harness=false helper program' });
        }
        const source = contained(root, row.executable);
        const bytes = readFileSync(source);
        // The current debug binary target is 134,360,576 B, above the former
        // 128 MiB guard. Keep a fixed 512 MiB bound that still rejects runaway
        // outputs while accepting the actual debug CLI executor.
        if (bytes.length > 512 << 20) throw new Error(`ci-test-programs.executable_budget: ${source}`);
        const entry = { package: packageName(row), target: row.target.name, kinds, executable: hash(source).slice(0, 12) + '-' + slug(row.target.name) };
        if (isTestHarness && !isHelper) {
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
        command: ['cargo', ...args], environmentSHA256: invocation.environmentSHA256, buildEvidence: invocation.buildEvidence,
        programs: tests, support, excluded, producedAtUtc: new Date().toISOString(),
      };
      return value;
    },
  });
  if (record.schema !== RECORD_SCHEMA) throw new Error('ci-test-programs.invalid_record');
  Object.assign(record, preserveBuildEvidence(root, record.buildEvidence, out));
  writeFileSync(join(out, 'test-programs.json'), `${JSON.stringify(record, null, 2)}\n`, {flag: 'wx'});
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

const programIdentity = program => ({package: program.package, target: program.target, executable: program.path, sha256: program.sha256});
const defaultTestArguments = Object.freeze(['--test-threads=1']);
function selectedPrograms(record, shard, packages, targets) {
  if (shard !== 'all' && !Object.hasOwn(TEST_SHARDS, shard)) throw new Error(`ci-test-programs.unknown_shard: ${shard}`);
  if (shard !== 'all' && (packages !== undefined || targets !== undefined)) throw new Error('ci-test-programs.shard_filters_conflict');
  return record.programs.filter(program => (shard === 'all' || TEST_SHARDS[shard].includes(program.package))
    && (!packages?.length || packages.includes(program.package)) && (!targets?.length || targets.includes(program.target)));
}
function saveRunReport(root, path, report) {
  if (!path) return;
  path = contained(root, path); mkdirSync(dirname(path), {recursive: true});
  writeFileSync(path, `${JSON.stringify(report, null, 2)}\n`);
}

export function runTestPrograms({ root = repository, recordPath, shard = 'all', packages, targets, reportPath, formalLuaReport, formalBridge = runPrebuiltBridge, executor = ownedSpawnSync, arguments: extraArguments = defaultTestArguments } = {}) {
  root = resolve(root);
  const record = readTestRecord(root, recordPath);
  const testRecordSHA256 = hash(readFileSync(contained(root, recordPath)));
  const selected = selectedPrograms(record, shard, packages, targets);
  if (shard !== 'all' && JSON.stringify(extraArguments) !== JSON.stringify(defaultTestArguments)) throw new Error('ci-test-programs.shard_arguments');
  prepareSupport(root, record);
  if (!selected.length) throw new Error(`ci-test-programs.no_selected_programs: packages=${(packages ?? []).join(',') || '*'} targets=${(targets ?? []).join(',') || '*'}`);
  if(selected.some(isFormalBridge) && !formalLuaReport) throw new Error('ci-test-programs.formal_lua_report_required');
  const runRoot = join(root, '.tmp', 'ci', 'run');
  mkdirSync(runRoot, { recursive: true });
  const evidence = join(runRoot, 'evidence');
  mkdirSync(evidence, { recursive: true });
  const environment = ownedUserEnvironment(join(runRoot, 'user'));
  const results = [];
  let handlesReaped = true, failed = 0, unreapedError, runError, report;
  try {
    for (const program of selected) {
      const executable = verifyEntry(root, program);
      const started = Date.now();
      handlesReaped = false;
      if(isFormalBridge(program)){
        try {
          const bridge = formalBridge({runtimePath: contained(root, formalLuaReport), recordPath: contained(root, recordPath), record, program});
          handlesReaped = true;
          const ok = bridge.passed === true;
          results.push({package: program.package, target: program.target, executable: program.path, sha256: program.sha256, exit: 0, ok, handlesReaped, millis: Date.now() - started, bridgeReport: bridge.report});
          if (!ok) failed++;
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
      const filtered = !extraArguments.includes('--list') && extraArguments.some(arg => arg === '--exact' || arg === '--ignored');
      const summary = filtered ? [...String(result.stdout ?? '').matchAll(/^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed;/gm)].at(-1) : undefined;
      const executedTests = summary ? Number(summary[1]) + Number(summary[2]) : undefined;
      const scopeError = filtered && handlesReaped && !result.error && result.status === 0 && !(executedTests > 0)
        ? 'ci-test-programs.filtered_no_tests_executed' : undefined;
      const ok = handlesReaped && !result.error && !scopeError && result.status === 0;
      results.push({ package: program.package, target: program.target, executable: program.path, sha256: program.sha256, exit: result.status, signal: result.signal, ok, handlesReaped, error: result.error?.message ?? scopeError, millis: Date.now() - started,
        ...(executedTests !== undefined ? {executedTests} : {}),
        ...(!handlesReaped ? { proof: result.proof, logs: result.logs } : {}) });
      if (!ok) failed++;
      if (!handlesReaped) { unreapedError = result.error; break; }
    }
    if (unreapedError) throw unreapedError;
    if (failed) throw new Error(`ci-test-programs.tests_failed: ${failed} program(s)`);
  } catch (error) { runError = error; throw error; }
  finally {
    report = {
      schema: 'lycheedev.ci-test-run.v1', state: runError || failed || results.length !== selected.length ? 'failed' : 'passed', sourceTree: record.sourceTree,
      commit: record.commit, testRecordSHA256, shard, plannedPrograms: selected.map(programIdentity),
      handlesReaped,
      packages: packages ?? [], targets: targets ?? [], arguments: extraArguments,
      programs: selected.length, attempted: results.length, passed: results.filter(row => row.ok === true).length, failed, results,
      ...(runError ? {error: runError.message} : {}),
    };
    saveRunReport(root, reportPath, report);
    if (!handlesReaped) process.stderr.write('ci-test-programs.warning: a test program tree was not reaped\n');
  }
  return report;
}

export function aggregateTestReports({root = repository, recordPath, reportPaths = [], reportPath} = {}) {
  root = resolve(root);
  const record = readTestRecord(root, recordPath), testRecordSHA256 = hash(readFileSync(contained(root, recordPath)));
  const results = [], shardReports = [], observed = new Set(), seenShards = new Set();
  const report = {schema: 'lycheedev.ci-test-run.v1', state: 'failed', commit: record.commit, sourceTree: record.sourceTree, testRecordSHA256,
    shard: 'aggregate', plannedPrograms: record.programs.map(programIdentity), packages: [], targets: [], arguments: [],
    programs: record.programs.length, attempted: 0, passed: 0, failed: 0, handlesReaped: false, results, shardReports,
    coverage: {complete: false, expectedPrograms: record.programs.length, observedPrograms: 0, shards: 0}};
  const check = (condition, reason) => { if (!condition) throw new Error(`ci-test-programs.aggregate_${reason}`); };
  try {
    const all = new Map();
    for (const program of record.programs) {
      check(typeof program.path === 'string' && program.path.length > 0 && /^[a-f0-9]{64}$/.test(program.sha256)
        && typeof program.package === 'string' && program.package.length > 0 && typeof program.target === 'string' && program.target.length > 0, 'record_program_identity');
      contained(root, program.path);
      check(!all.has(program.path), 'record_program_duplicate'); all.set(program.path, programIdentity(program));
    }
    check(all.size > 0, 'empty_record');
    check(reportPaths.length === Object.keys(TEST_SHARDS).length, 'shard_count');
    let sharedArguments;
    for (const input of reportPaths) {
      const path = contained(root, input), bytes = readFileSync(path), group = JSON.parse(bytes);
      check(group.schema === report.schema, 'schema');
      check(group.commit === record.commit && group.sourceTree === record.sourceTree && group.testRecordSHA256 === testRecordSHA256, 'record_identity');
      check(Object.hasOwn(TEST_SHARDS, group.shard), 'unknown_shard');
      check(!seenShards.has(group.shard), 'duplicate_shard'); seenShards.add(group.shard);
      const expected = new Map(selectedPrograms(record, group.shard).map(program => [program.path, programIdentity(program)]));
      check(expected.size > 0, 'empty_shard');
      check(Array.isArray(group.packages) && group.packages.length === 0 && Array.isArray(group.targets) && group.targets.length === 0, 'shard_filters');
      check(JSON.stringify(group.arguments) === JSON.stringify(defaultTestArguments), 'arguments');
      if (sharedArguments === undefined) sharedArguments = group.arguments;
      check(JSON.stringify(group.arguments) === JSON.stringify(sharedArguments), 'arguments_differ');
      check(Array.isArray(group.plannedPrograms) && group.plannedPrograms.length === expected.size, 'plan_count');
      const planned = new Set();
      for (const entry of group.plannedPrograms) {
        check(expected.has(entry.executable) && !planned.has(entry.executable), 'plan_program'); planned.add(entry.executable);
        const identity = expected.get(entry.executable);
        check(entry.package === identity.package && entry.target === identity.target && entry.sha256 === identity.sha256, 'plan_identity');
      }
      check(Array.isArray(group.results) && group.results.length === expected.size, 'result_count');
      const actual = new Set();
      for (const row of group.results) {
        check(expected.has(row.executable) && !actual.has(row.executable) && !observed.has(row.executable), 'result_program'); actual.add(row.executable);
        const identity = expected.get(row.executable);
        check(row.package === identity.package && row.target === identity.target && row.sha256 === identity.sha256, 'result_identity');
        check(row.exit === 0 && row.ok === true && row.handlesReaped === true && !row.error && !row.signal, 'result_not_passed');
        check(Number.isSafeInteger(row.millis) && row.millis >= 0, 'result_millis');
      }
      check(group.state === 'passed' && group.handlesReaped === true && group.programs === expected.size && group.attempted === expected.size
        && group.passed === expected.size && group.failed === 0, 'summary_not_passed');
      for (const key of actual) observed.add(key);
      results.push(...group.results);
      shardReports.push({shard: group.shard, path: relative(root, path).replaceAll('\\', '/'), sha256: hash(bytes),
        programs: group.programs, attempted: group.attempted, passed: group.passed, failed: group.failed, state: group.state});
    }
    check(Object.keys(TEST_SHARDS).every(shard => seenShards.has(shard)), 'shard_missing');
    check(observed.size === all.size && [...all.keys()].every(key => observed.has(key)), 'coverage_incomplete');
    report.arguments = sharedArguments;
    report.state = 'passed'; report.handlesReaped = true; report.coverage.complete = true;
    return report;
  } catch (error) { report.error = error.message; throw error; }
  finally {
    report.attempted = results.length; report.passed = results.filter(row => row.ok === true).length;
    report.failed = results.filter(row => row.ok !== true).length;
    report.coverage.observedPrograms = observed.size; report.coverage.shards = shardReports.length;
    saveRunReport(root, reportPath, report);
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
      if ((command === 'run' && ['--package', '--target', '--arguments', '--formal-lua'].includes(arg)
        || command === 'build' && arg === '--owner') && (!value || arg !== '--arguments' && value.startsWith('--'))) {
        throw new Error(`ci-test-programs.option_value_missing: ${arg}`);
      }
      if (arg === '--shard-report' && command === 'aggregate') {
        if (!value || value.startsWith('--')) throw new Error('ci-test-programs.option_value_missing: --shard-report');
        (options.reportPaths ??= []).push(value); index++;
      }
      else if (arg === '--shard' && command === 'run') {
        if (options.shard !== undefined) throw new Error('ci-test-programs.duplicate_option: --shard');
        if (!value || value.startsWith('--')) throw new Error('ci-test-programs.option_value_missing: --shard');
        options.shard = value; index++;
      }
      else if (arg === '--package' && command === 'run') { options.packages = parseList(value); index++; }
      else if (arg === '--target' && command === 'run') { options.targets = parseList(value); index++; }
      else if (arg === '--report' && ['run', 'aggregate'].includes(command)) {
        if (!value || value.startsWith('--')) throw new Error('ci-test-programs.option_value_missing: --report');
        if (options.reportPath !== undefined) throw new Error('ci-test-programs.duplicate_option: --report');
        options.reportPath = value; index++;
      }
      else if (arg === '--arguments' && command === 'run') { options.arguments = parseList(value); index++; }
      else if (arg === '--formal-lua' && command === 'run') { options.formalLuaReport = value; index++; }
      else if (arg === '--owner' && command === 'build') { options.owner = value; index++; }
      else if (arg === '--jobs' && command === 'build') {
        if (options.jobs !== undefined) throw new Error('ci-test-programs.duplicate_option: --jobs');
        if (value === undefined || value.startsWith('--')) throw new Error('ci-test-programs.option_value_missing: --jobs');
        options.jobs = validateCargoJobs(value); index++;
      }
      else if (arg === '--timings' && command === 'build') {
        if (options.timings) throw new Error('ci-test-programs.duplicate_option: --timings');
        options.timings = true;
      }
      else if (arg.startsWith('--')) throw new Error(`ci-test-programs.unknown_option: ${arg}`);
      else rest.push(arg);
    }
    if (command === 'build') {
      if (rest.length !== 1) throw new Error('usage: node tools/ci-test-programs.mjs build <new-output-directory> [--owner name] [--jobs 1..4] [--timings]');
      const record = buildTestPrograms({ output: rest[0], ...(options.owner ? { owner: options.owner } : {}), ...(options.jobs !== undefined ? { jobs: options.jobs } : {}), ...(options.timings ? {timings: true} : {}) });
      process.stdout.write(`${JSON.stringify({ schema: record.schema, programs: record.programs.length, support: record.support.length, excluded: record.excluded, sourceTree: record.sourceTree }, null, 2)}\n`);
    } else if (command === 'run') {
      if (rest.length !== 1) throw new Error('usage: node tools/ci-test-programs.mjs run <test-programs.json> [--shard data-delivery|source-runtime] [--package a,b] [--target a,b] [--report path] [--formal-lua build-report.json] [--arguments --test-threads=1]');
      const report = runTestPrograms({ recordPath: rest[0], ...options });
      process.stdout.write(`${JSON.stringify({ state: report.state, programs: report.programs, passed: report.passed, failed: report.failed }, null, 2)}\n`);
    } else if (command === 'aggregate') {
      if (rest.length !== 1 || !options.reportPath) throw new Error('usage: node tools/ci-test-programs.mjs aggregate <test-programs.json> --shard-report <report.json> --shard-report <report.json> --report <summary.json>');
      const report = aggregateTestReports({recordPath: rest[0], ...options});
      process.stdout.write(`${JSON.stringify({state: report.state, coverage: report.coverage})}\n`);
    } else throw new Error('usage: node tools/ci-test-programs.mjs build <new-directory> | run <test-programs.json> [options]');
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 1; }
}
