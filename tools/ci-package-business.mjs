// Consume the already installed Release TGZ. The outer owned Job contains the
// fixed-input constructor, public CLI processes and both native worker trees.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {basename, dirname, isAbsolute, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
import {beginScratch, contained} from './rust-disk.mjs';
import {hashFile, sourceDigest} from './rust-build.mjs';
import {packageSmokeEnvironment} from './rust-package-smoke.mjs';
import {readTestRecord} from './ci-test-programs.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {acceptanceCliArgs, bootstrapTinyOwner} from '../tests/rust-source-native-acceptance/session-ipc.mjs';
import {loadAgentDataFixture} from '../tests/rust-performance-paired/agent-fixture-inputs.mjs';
import {auditQuery, queryFor} from '../tests/rust-performance-paired/agent-audit.mjs';

const SELF = fileURLToPath(import.meta.url);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const save = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
const read = (path, limit = 4 << 20) => {
  const info = lstatSync(path);
  assert(info.isFile() && !info.isSymbolicLink() && info.size <= limit, 'package-business.bounded_file');
  const bytes = readFileSync(path); assert.equal(bytes.length, info.size);
  return {bytes, value: JSON.parse(bytes)};
};

export function parseBusinessOptions(args) {
  const options = {};
  for (let index = 0; index < args.length; index++) {
    const flag = args[index];
    assert(['--npm-report', '--test-record', '--out'].includes(flag), `package-business.unknown_option: ${flag}`);
    assert(!Object.hasOwn(options, flag), `package-business.duplicate_option: ${flag}`);
    const value = args[++index];
    assert(value && !value.startsWith('--'), `package-business.option_value_missing: ${flag}`);
    options[flag] = value;
  }
  assert(Object.keys(options).length === 3, 'usage: node tools/ci-package-business.mjs --npm-report <report.json> --test-record <test-programs.json> --out <new-directory>');
  return {npmReport: options['--npm-report'], testRecord: options['--test-record'], out: options['--out']};
}

export function businessEnvironment(directory, parent = process.env) {
  const clean = Object.fromEntries(Object.entries(parent).filter(([key]) => !/^GIT_/i.test(key) && key.toUpperCase() !== 'GCM_INTERACTIVE'));
  return {...packageSmokeEnvironment(directory, clean), GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: process.platform === 'win32' ? 'NUL' : '/dev/null', GIT_TERMINAL_PROMPT: '0', GIT_NO_REPLACE_OBJECTS: '1'};
}

// This authenticates consumption of the same archive, not another candidate
// assembled from build outputs. No install, Cargo or package production exists.
export function authenticateBusinessInputs(root, options, {testRecordReader = readTestRecord, treeDigest = sourceDigest} = {}) {
  root = resolve(root);
  const npmPath = contained(root, resolve(root, options.npmReport)), npm = read(npmPath);
  const report = npm.value;
  assert.equal(report.schema, 'lycheedev.rust-npm-smoke.v1');
  assert(['partial_passed', 'passed'].includes(report.state) && report.npmSmoke === 'passed' && report.assemblyOnly === false, 'package-business.npm_validation_required');
  assert.equal(report.profile, 'release', 'package-business.release_required');
  const sourceTree = treeDigest(root);
  assert.equal(report.sourceTreeSHA256, sourceTree, 'package-business.stale_source');
  const productionPath = contained(root, resolve(root, report.packageReport)), production = read(productionPath);
  assert.equal(sha(production.bytes), report.packageReportSHA256, 'package-business.package_report_changed');
  assert.equal(production.value.schema, 'lycheedev.rust-npm-package.v1');
  assert.equal(production.value.state, 'packed');
  for (const field of ['profile', 'commit', 'sourceTreeSHA256', 'packageVersion', 'tgzSHA256', 'binarySHA256', 'releaseManifestSHA256', 'resourceInventorySHA256', 'compilerSHA256']) {
    assert.equal(report[field], production.value[field], `package-business.package_identity: ${field}`);
  }
  assert.equal(report.tgzPathBase, 'workspace');
  assert.equal(production.value.tgzPathBase, 'report');
  assert(!isAbsolute(production.value.tgz), 'package-business.relative_tgz_required');
  const tgz = contained(dirname(productionPath), resolve(dirname(productionPath), production.value.tgz));
  assert.equal(contained(root, resolve(root, report.tgz)), tgz, 'package-business.tgz_identity');
  assert.equal(hashFile(tgz), report.tgzSHA256, 'package-business.tgz_changed');
  const installed = contained(dirname(npmPath), resolve(report.installedRoot));
  assert.equal(installed, resolve(report.installedPrefix, 'node_modules/lycheedev'), 'package-business.installed_layout');
  const manifestPath = contained(installed, join(installed, 'release.json')), manifest = read(manifestPath);
  assert.equal(sha(manifest.bytes), report.releaseManifestSHA256, 'package-business.installed_manifest_changed');
  assert.equal(manifest.value.schema, 'lycheedev.release.v1');
  assert.equal(manifest.value.runtime, 'rust');
  assert.equal(manifest.value.version, report.packageVersion);
  assert.equal(manifest.value.commit, report.commit);
  assert.equal(read(join(installed, 'package.json')).value.version, report.packageVersion);
  const executable = manifest.value.binaries?.['windows-amd64'];
  assert.equal(executable?.path, 'native/windows-amd64/lycheedev.exe');
  assert.equal(executable.sha256, report.binarySHA256);
  const binary = contained(installed, join(installed, executable.path));
  assert.equal(lstatSync(binary).size, executable.bytes);
  assert.equal(hashFile(binary), report.binarySHA256, 'package-business.installed_binary_changed');
  assert(Array.isArray(manifest.value.resources));
  assert.equal(sha(Buffer.from(JSON.stringify(manifest.value.resources))), report.resourceInventorySHA256);
  for (const resource of manifest.value.resources) {
    const path = contained(join(installed, 'payload'), resolve(installed, 'payload', resource.path));
    assert.equal(lstatSync(path).size, resource.bytes);
    assert.equal(hashFile(path), resource.sha256, `package-business.installed_resource_changed: ${resource.path}`);
  }
  assert.equal(hashFile(join(installed, 'payload/tool/luac/luac.exe')), report.compilerSHA256);
  const testPath = contained(root, resolve(root, options.testRecord)), record = testRecordReader(root, testPath);
  assert.equal(resolve(record.root), root, 'package-business.test_workspace');
  assert.equal(record.sourceTree, sourceTree, 'package-business.test_source');
  assert.equal(record.commit, report.commit, 'package-business.test_commit');
  const candidates = record.programs.filter(row => row.package === 'lycheedev-data' && row.target === 'agent_fixture_prepare' && row.kinds.includes('test'));
  assert.equal(candidates.length, 1, 'package-business.fixture_program_required');
  const fixture = candidates[0], fixtureBinary = contained(root, resolve(root, fixture.path));
  assert.equal(lstatSync(fixtureBinary).size, fixture.bytes);
  assert.equal(hashFile(fixtureBinary), fixture.sha256, 'package-business.fixture_program_changed');
  const tools = ['tools/ci-package-business.mjs', 'tests/rust-source-native-acceptance/session-ipc.mjs', 'tests/rust-performance-paired/agent-query-run.mjs', 'tests/rust-performance-paired/agent-audit.mjs', 'tests/rust-performance-paired/agent-fixture-inputs.mjs', 'tools/rust-agent-client.mjs'];
  return {root, binary, installed, fixtureBinary, fixtureSHA256: fixture.sha256, npmPath, productionPath, tgz, testPath,
    npmReportSHA256: sha(npm.bytes), packageReportSHA256: sha(production.bytes), tgzSHA256: report.tgzSHA256,
    binarySHA256: report.binarySHA256, sourceTreeSHA256: sourceTree, testRecordSHA256: hashFile(testPath),
    testInputs: Object.fromEntries(tools.map(path => [path, hashFile(join(root, path))]))};
}

export function packageBusiness(args, {root = process.cwd(), execute = ownedSpawnSync, authenticate = authenticateBusinessInputs, scratch = beginScratch} = {}) {
  const options = parseBusinessOptions(args); root = resolve(root);
  const out = contained(root, resolve(root, options.out));
  assert(!existsSync(out), 'package-business.output_exists');
  mkdirSync(out, {recursive: true});
  mkdirSync(join(out, 'evidence'));
  const path = join(out, 'report.json');
  const report = {schema: 'lycheedev.ci-package-business.v1', state: 'failed', npmPath: resolve(root, options.npmReport), testPath: resolve(root, options.testRecord),
    source: {state: 'not_run'}, data: {state: 'not_run'}, inputsStable: false, handlesReaped: true,
    scope: 'Minimal fixed local Source native and synthetic DB2 Agent business through the installed Release TGZ; no real-client or full-product acceptance.'};
  let input;
  try { input = authenticate(root, options); Object.assign(report, input, {state: 'running'}); }
  catch (error) { report.error = error.message; save(path, report); return report; }
  save(path, report);
  let job;
  try { job = scratch(root, 'ci-package-business', {command: [process.execPath, SELF, ...args], inputs: input}); }
  catch (error) { report.state = 'failed'; report.error = error.message; save(path, report); return report; }
  const requestPath = join(job.directory, 'business-request.json');
  const logs = [path], workerPath = join(out, 'evidence/worker.json'); let state = 'failed';
  try {
    save(requestPath, {schema: 'lycheedev.ci-package-business-request.v1', input, out, scratch: job.directory, ownerReceipt: job.receipt});
    copyFileSync(requestPath, join(out, 'evidence/business-request.json'));
    const env = businessEnvironment(job.directory);
    for (const key of ['HOME', 'TEMP', 'APPDATA', 'LOCALAPPDATA']) mkdirSync(env[key], {recursive: true});
    report.handlesReaped = false; save(path, report);
    const run = execute(process.execPath, [SELF, '--worker', requestPath], {cwd: root, env, evidence: job.evidence, timeout: 600000});
    report.handlesReaped = run.handlesReaped === true; logs.push(...(run.logs ?? []));
    const ownedLogs = [], ownedDirectory = join(out, 'evidence/owned'); mkdirSync(ownedDirectory);
    for (const original of run.logs ?? []) {
      const checked = contained(root, original), destination = join(ownedDirectory, basename(checked)), digest = hashFile(checked, 16 << 20);
      copyFileSync(checked, destination); assert.equal(hashFile(destination, 16 << 20), digest);
      ownedLogs.push({path: destination, sha256: digest});
    }
    report.process = {exit: run.status, signal: run.signal, error: run.error?.message, proof: run.proof, logs: ownedLogs};
    if (existsSync(workerPath)) {
      const worker = read(workerPath).value;
      assert.equal(worker.schema, 'lycheedev.ci-package-business-worker.v1');
      report.source = worker.source; report.data = worker.data; report.worker = workerPath;
    }
    // Run even after a failed command: report whether the consumed inputs stayed
    // identical, and never convert a failed/reaped command into success.
    const after = authenticate(root, options);
    report.inputsStable = JSON.stringify(after) === JSON.stringify(input);
    const passed = report.inputsStable && report.handlesReaped && !run.error && run.status === 0
      && report.source.state === 'passed' && report.source.closed === true
      && report.data.state === 'passed' && report.data.closed === true && report.data.complete === true;
    report.state = passed ? 'passed' : 'failed'; state = passed ? 'succeeded' : 'failed';
  } catch (error) { report.state = 'failed'; report.error = error.message; }
  finally {
    save(path, report);
    const collect = directory => { for (const entry of readdirSync(directory, {withFileTypes: true})) {
      const file = contained(out, join(directory, entry.name)); assert(!entry.isSymbolicLink(), 'package-business.evidence_link');
      if (entry.isDirectory()) collect(file);
      else { assert(entry.isFile() && logs.length < 512 && lstatSync(file).size <= 16 << 20, 'package-business.evidence_budget'); if (!logs.includes(file)) logs.push(file); }
    } };
    collect(out);
    job.seal({state: report.handlesReaped ? state : 'interrupted', handlesReaped: report.handlesReaped, logs});
    if (report.handlesReaped) job.cleanup();
  }
  return report;
}

async function worker(requestPath) {
  assert.equal(process.platform, 'win32', 'package-business.windows_required');
  const request = read(requestPath).value;
  assert.equal(request.schema, 'lycheedev.ci-package-business-request.v1');
  const {input, out, scratch} = request;
  assert.equal(resolve(input.root), resolve('.'), 'package-business.worker_workspace');
  const owner = read(contained(input.root, request.ownerReceipt)).value;
  assert.equal(owner.schema, 'lycheedev.disk-owner.v1');
  assert.equal(owner.owner, 'ci-package-business'); assert.equal(owner.state, 'active');
  assert.equal(owner.category, 'scratch'); assert.equal(owner.retention, 'rebuildable');
  assert.equal(resolve(owner.directory), resolve(scratch));
  assert.equal(resolve(requestPath), join(resolve(scratch), 'business-request.json'));
  contained(input.root, out); contained(input.root, scratch);
  const report = {schema: 'lycheedev.ci-package-business-worker.v1', source: {state: 'not_run'}, data: {state: 'not_run'}, commands: []};
  const evidence = join(out, 'evidence'), reportPath = join(evidence, 'worker.json'), deadline = Date.now() + 540000;
  let fatal = false, sequence = 0;
  const run = (label, executable, args, {cwd = scratch, env = process.env, timeout = 300000} = {}) => {
    assert(!fatal, 'package-business.prior_process_failure');
    const remaining = deadline - Date.now(); assert(remaining > 1000, 'package-business.deadline');
    const result = spawnSync(executable, args, {cwd, env, windowsHide: true, shell: false, timeout: Math.min(timeout, remaining), maxBuffer: 8 << 20});
    const prefix = join(evidence, `${String(++sequence).padStart(3, '0')}-${label}`);
    writeFileSync(prefix + '.stdout', result.stdout ?? Buffer.alloc(0));
    writeFileSync(prefix + '.stderr', result.stderr ?? Buffer.alloc(0));
    report.commands.push({label, executable, args, exit: result.status, signal: result.signal, error: result.error?.message, stdout: prefix + '.stdout', stderr: prefix + '.stderr'});
    fatal ||= Boolean(result.error || result.signal);
    save(reportPath, report);
    assert(!fatal && result.status === 0, `package-business.command_failed: ${label}: ${result.error?.message ?? result.status}`);
    return result.stdout;
  };
  const home = join(scratch, 'source-home');
  const invoke = (label, args, options = {}) => {
    const envelope = JSON.parse(run(label, input.binary, acceptanceCliArgs(args, home, options), {timeout: options.timeout}));
    assert.equal(envelope.schema, 'lycheedev.result.v1'); assert.equal(envelope.ok, true);
    return {result: envelope.result, envelope};
  };
  save(reportPath, report);
  let sourceError;
  try {
    const source = await bootstrapTinyOwner({binary: input.binary, home, release: input.installed, evidenceDir: join(evidence, 'source'), invoke});
    report.source = {state: 'passed', snapshot: source.selection.id, commit: source.commit, ownerPid: source.ownerPid, native: true, complete: source.opened.result.complete, coverage: source.opened.result.coverage};
  } catch (error) { sourceError = error; fatal ||= error.fatalProcessFailure === true; report.source = {state: 'failed', error: error.message}; }
  finally {
    if (!fatal && existsSync(join(home, 'workspace.json'))) {
      try {
        const closed = invoke('source-close', ['source', 'session', 'close']); assert.equal(closed.result.state, 'closed');
        const status = invoke('source-closed-status', ['source', 'session', 'status']); assert.equal(status.result.state, 'closed');
        report.source.closed = true;
      } catch (error) { report.source = {...report.source, state: 'failed', cleanupError: error.message}; sourceError ??= error; }
    }
    save(reportPath, report);
  }
  if (!fatal) {
    try {
      const directory = join(scratch, 'catalog');
      run('prepare-catalog-input', input.fixtureBinary, ['prepare_catalog_agent_fixture', '--ignored', '--exact', '--nocapture', '--test-threads=1'], {env: {...process.env, LYCHEEDEV_CATALOG_FIXTURE: directory}, timeout: 60000});
      const fixture = loadAgentDataFixture(directory, 'catalog');
      const queryPath = join(scratch, 'data-request.json');
      save(queryPath, {schema: 'lycheedev.paired-agent-request.recovered.v1', binary: input.binary, home: fixture.identity.home, snapshot: fixture.identity.snapshot,
        installation: fixture.identity.installation, root: scratch, query: queryFor('catalog')});
      const packet = JSON.parse(run('data-agent-business', process.execPath, [join(input.root, 'tests/rust-performance-paired/agent-query-run.mjs'), queryPath]));
      const audited = auditQuery(packet, {kind: 'catalog', fixture, cacheMode: 'cold', format: 'rows', home: fixture.identity.home, evidence, label: 'catalog'});
      report.data = {state: 'passed', snapshot: fixture.identity.snapshot, rows: audited.value.rows.length, columns: audited.value.columns, complete: true,
        engine: packet.result.provenance.engine, capture: packet.capture.id, captureArchive: audited.captureArchive, closed: true, offline: true, fixture: fixture.receipt};
    } catch (error) { report.data = {state: 'failed', error: error.message}; }
  } else { report.data.reason = 'Source process failure requires outer Job retirement before further work'; }
  save(reportPath, report);
  if (sourceError || report.source.state !== 'passed' || report.data.state !== 'passed') process.exitCode = 1;
}

if (process.argv[1] && resolve(process.argv[1]) === SELF) {
  try {
    if (process.argv[2] === '--worker') { assert.equal(process.argv.length, 4); await worker(process.argv[3]); }
    else {
      assert.equal(process.platform, 'win32', 'package-business.windows_required');
      const report = packageBusiness(process.argv.slice(2));
      console.log(JSON.stringify({state: report.state, source: report.source.state, data: report.data.state, handlesReaped: report.handlesReaped}));
      if (report.state !== 'passed') process.exitCode = 1;
    }
  } catch (error) { process.stderr.write(error.stack + '\n'); process.exitCode = 1; }
}
