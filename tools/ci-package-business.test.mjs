// Node-only custody and failure-boundary checks. No native program is launched.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {mkdirSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {dirname, join, relative, resolve} from 'node:path';
import {after, test} from 'node:test';
import {authenticateBusinessInputs, businessEnvironment, packageBusiness, parseBusinessOptions} from './ci-package-business.mjs';

const repository = resolve(import.meta.dirname, '..'), base = join(repository, '.tmp', `package-business-boundaries-${process.pid}`);
after(() => rmSync(base, {recursive: true, force: true}));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
let next = 0;
function fixture(t) {
  const root = join(base, String(++next)); mkdirSync(root, {recursive: true});
  t.after(() => { rmSync(root, {recursive: true, force: true}); });
  const put = (path, value) => { mkdirSync(dirname(path), {recursive: true}); writeFileSync(path, value); };
  const json = (path, value) => put(path, JSON.stringify(value));
  const read = path => JSON.parse(readFileSync(path));
  const sourceTree = 'a'.repeat(64), commit = 'b'.repeat(40), version = '3.5.0';
  const npmPath = join(root, 'npm/report.json'), productionPath = join(root, 'production/report.json');
  const installedPrefix = join(root, 'npm/installed'), installed = join(installedPrefix, 'node_modules/lycheedev');
  const binaryBytes = Buffer.from('sealed Release executable'), compiler = Buffer.from('sealed compiler');
  const binary = join(installed, 'native/windows-amd64/lycheedev.exe'); put(binary, binaryBytes);
  put(join(installed, 'payload/tool/luac/luac.exe'), compiler);
  const resources = [{path: 'tool/luac/luac.exe', bytes: compiler.length, sha256: hash(compiler)}];
  const manifest = {schema: 'lycheedev.release.v1', runtime: 'rust', commit, version, resources,
    binaries: {'windows-amd64': {path: 'native/windows-amd64/lycheedev.exe', bytes: binaryBytes.length, sha256: hash(binaryBytes)}}};
  json(join(installed, 'release.json'), manifest); json(join(installed, 'package.json'), {version});
  const tgz = join(root, 'production/package.tgz'); put(tgz, 'same sealed TGZ');
  const production = {schema: 'lycheedev.rust-npm-package.v1', state: 'packed', profile: 'release', commit, packageVersion: version,
    sourceTreeSHA256: sourceTree, tgz: 'package.tgz', tgzPathBase: 'report', tgzSHA256: hash(readFileSync(tgz)), binarySHA256: hash(binaryBytes),
    releaseManifestSHA256: hash(readFileSync(join(installed, 'release.json'))), resourceInventorySHA256: hash(Buffer.from(JSON.stringify(resources))), compilerSHA256: hash(compiler)};
  json(productionPath, production);
  const npm = {...production, schema: 'lycheedev.rust-npm-smoke.v1', state: 'partial_passed', npmSmoke: 'passed', assemblyOnly: false,
    tgz: relative(root, tgz), tgzPathBase: 'workspace', installedPrefix, installedRoot: installed,
    packageReport: relative(root, productionPath), packageReportSHA256: hash(readFileSync(productionPath))};
  json(npmPath, npm);
  const helper = join(root, 'tests/agent_fixture_prepare.exe'); put(helper, 'precompiled input constructor');
  const testPath = join(root, 'tests/test-programs.json');
  const record = {schema: 'lycheedev.ci-test-programs.v1', root, commit, sourceTree, programs: [{package: 'lycheedev-data', target: 'agent_fixture_prepare', kinds: ['test'], path: relative(root, helper), bytes: readFileSync(helper).length, sha256: hash(readFileSync(helper))}]};
  json(testPath, record);
  for (const tool of ['tools/ci-package-business.mjs', 'tests/rust-source-native-acceptance/session-ipc.mjs', 'tests/rust-performance-paired/agent-query-run.mjs', 'tests/rust-performance-paired/agent-audit.mjs', 'tests/rust-performance-paired/agent-fixture-inputs.mjs', 'tools/rust-agent-client.mjs']) put(join(root, tool), 'sealed fixture tool');
  const options = {npmReport: npmPath, testRecord: testPath, out: join(root, 'business')};
  const auth = () => authenticateBusinessInputs(root, options, {testRecordReader: (_root, path) => read(path), treeDigest: () => sourceTree});
  const args = ['--npm-report', npmPath, '--test-record', testPath, '--out', options.out];
  let terminal, cleaned = false;
  const scratch = () => {
    const directory = join(root, 'scratch'), evidence = join(root, 'evidence'); mkdirSync(directory); mkdirSync(evidence);
    return {directory, evidence, receipt: join(evidence, 'receipt.json'), seal(value) { terminal = value; }, cleanup() { cleaned = true; }};
  };
  return {root, npmPath, npm, productionPath, production, installed, binary, helper, tgz, testPath, record, options, args, auth, json, read, scratch,
    get terminal() { return terminal; }, get cleaned() { return cleaned; }};
}
function observed(args, source = {state: 'passed', complete: false, closed: true}, data = {state: 'passed', complete: true, closed: true}) {
  const request = JSON.parse(readFileSync(args[2]));
  writeFileSync(join(request.out, 'evidence/worker.json'), JSON.stringify({schema: 'lycheedev.ci-package-business-worker.v1', source, data}));
  return request;
}
const passed = {status: 0, signal: null, handlesReaped: true, logs: [], stdout: Buffer.alloc(0), stderr: Buffer.alloc(0)};

test('CLI requires exactly its three explicit inputs and rejects accidental build options', () => {
  assert.deepEqual(parseBusinessOptions(['--npm-report', 'npm.json', '--test-record', 'tests.json', '--out', 'new']), {npmReport: 'npm.json', testRecord: 'tests.json', out: 'new'});
  for (const args of [[], ['--out'], ['--out', '--npm-report', 'x'], ['--artifact', 'x'], ['--out', 'a', '--out', 'b']]) assert.throws(() => parseBusinessOptions(args));
});

test('the same installed Release, TGZ, producer record and prebuilt constructor authenticate', t => {
  const f = fixture(t), input = f.auth();
  assert.equal(input.binary, f.binary); assert.equal(input.fixtureBinary, f.helper);
  assert.equal(input.packageReportSHA256, hash(readFileSync(f.productionPath)));
  assert.equal(input.tgzSHA256, hash(readFileSync(f.tgz)));
});

test('debug, unfinished npm validation, changed TGZ/resource/program and ambiguous fixture are rejected before execution', t => {
  const cases = [
    f => { f.npm.profile = 'debug'; f.json(f.npmPath, f.npm); },
    f => { f.npm.npmSmoke = 'not_run'; f.json(f.npmPath, f.npm); },
    f => writeFileSync(f.tgz, 'different TGZ'),
    f => writeFileSync(join(f.installed, 'payload/tool/luac/luac.exe'), 'changed compiler'),
    f => writeFileSync(f.helper, 'changed constructor'),
    f => { f.record.programs.push({...f.record.programs[0]}); f.json(f.testPath, f.record); },
    f => { f.record.programs = []; f.json(f.testPath, f.record); },
    f => { f.npm.packageReportSHA256 = '0'.repeat(64); f.json(f.npmPath, f.npm); },
    f => { f.record.sourceTree = '0'.repeat(64); f.json(f.testPath, f.record); },
  ];
  for (const mutate of cases) {
    const f = fixture(t); mutate(f); let calls = 0;
    const report = packageBusiness(f.args, {root: f.root, authenticate: f.auth, scratch: f.scratch, execute() { calls++; }});
    assert.equal(report.state, 'failed'); assert.equal(report.source.state, 'not_run'); assert.equal(report.data.state, 'not_run');
    assert.equal(report.handlesReaped, true); assert.equal(calls, 0);
    assert.equal(f.read(join(f.options.out, 'report.json')).state, 'failed');
  }
});

test('one owned worker consumes installed bytes and keeps partial Source coverage explicit', t => {
  const f = fixture(t); let calls = 0;
  const report = packageBusiness(f.args, {root: f.root, authenticate: f.auth, scratch: f.scratch, execute(command, args, options) {
    calls++; assert.equal(command, process.execPath); assert.equal(args[1], '--worker');
    const request = observed(args); assert.equal(request.input.binary, f.binary); assert.equal(request.input.fixtureBinary, f.helper);
    assert(options.env.HOME.startsWith(request.scratch)); assert(options.env.TEMP.startsWith(request.scratch));
    assert.equal(options.env.LYCHEEDEV_BIN, undefined); assert.equal(options.env.GIT_CONFIG_NOSYSTEM, '1');
    const receipt = join(options.evidence, 'mock-reap.receipt.json'); writeFileSync(receipt, '{"treeReaped":true}');
    return {...passed, logs: [receipt]};
  }});
  assert.equal(calls, 1); assert.equal(report.state, 'passed'); assert.equal(report.source.complete, false);
  assert.equal(report.data.complete, true); assert.equal(report.inputsStable, true); assert.equal(f.cleaned, true);
  assert.equal(f.terminal.state, 'succeeded'); assert.equal(f.terminal.handlesReaped, true);
  assert.equal(report.process.logs.length, 1);
  assert.equal(readFileSync(report.process.logs[0].path, 'utf8'), '{"treeReaped":true}');
  assert(report.process.logs[0].path.startsWith(join(f.options.out, 'evidence/owned')));
});

test('failed or omitted business cannot pass merely because the owned worker exited zero', t => {
  for (const [source, data] of [[{state: 'failed'}, {state: 'passed'}], [{state: 'passed'}, {state: 'not_run'}], [{state: 'partial'}, {state: 'passed'}], [{state: 'passed'}, {state: 'passed'}]]) {
    const f = fixture(t);
    const report = packageBusiness(f.args, {root: f.root, authenticate: f.auth, scratch: f.scratch, execute(_command, args) { observed(args, source, data); return passed; }});
    assert.equal(report.state, 'failed'); assert.equal(report.inputsStable, true); assert.equal(f.cleaned, true);
  }
});

test('an unverified process tree preserves scratch and never licenses success', t => {
  const f = fixture(t);
  const report = packageBusiness(f.args, {root: f.root, authenticate: f.auth, scratch: f.scratch, execute(_command, args) { observed(args); return {...passed, handlesReaped: false}; }});
  assert.equal(report.state, 'failed'); assert.equal(report.handlesReaped, false);
  assert.equal(f.cleaned, false); assert.equal(f.terminal.state, 'interrupted');
});

test('changed inputs after worker completion retain results but fail the aggregate', t => {
  const f = fixture(t);
  const report = packageBusiness(f.args, {root: f.root, authenticate: f.auth, scratch: f.scratch, execute(_command, args) { observed(args); writeFileSync(f.tgz, 'mutated after execution'); return passed; }});
  assert.equal(report.state, 'failed'); assert.equal(report.source.state, 'passed'); assert.equal(report.data.state, 'passed');
  assert.equal(report.inputsStable, false); assert.match(report.error, /tgz_changed/); assert.equal(f.cleaned, true);
});

test('launch exceptions and admission failure produce terminal failure without false reaping', t => {
  const f = fixture(t);
  const report = packageBusiness(f.args, {root: f.root, authenticate: f.auth, scratch: f.scratch, execute() { throw Error('original launch failed'); }});
  assert.equal(report.state, 'failed'); assert.equal(report.source.state, 'not_run'); assert.equal(report.handlesReaped, false);
  assert.equal(f.cleaned, false); assert.equal(f.terminal.state, 'interrupted');
  const g = fixture(t), blocked = packageBusiness(g.args, {root: g.root, authenticate: g.auth, scratch() { throw Error('disk quota'); }, execute() { assert.fail('must not launch'); }});
  assert.equal(blocked.state, 'failed'); assert.equal(blocked.handlesReaped, true); assert.equal(blocked.error, 'disk quota');
});

test('isolation removes inherited product/npm/Git overrides and blocks global Git configuration', () => {
  const env = businessEnvironment(join(base, 'isolation'), {PATH: 'tools', HOME: 'real-user', LYCHEEDEV_BIN: 'foreign.exe', LYCHEEDEV_HOME: 'foreign-home', npm_config_prefix: 'global', GIT_DIR: 'foreign', git_work_tree: 'foreign', GCM_INTERACTIVE: 'always'});
  assert.equal(env.PATH, 'tools'); assert.equal(env.GIT_DIR, undefined); assert.equal(env.git_work_tree, undefined); assert.equal(env.GCM_INTERACTIVE, undefined);
  assert.equal(env.npm_config_prefix, undefined); assert.equal(env.LYCHEEDEV_BIN, undefined);
  assert.notEqual(env.HOME, 'real-user'); assert.notEqual(env.LYCHEEDEV_HOME, 'foreign-home'); assert.equal(env.GIT_TERMINAL_PROMPT, '0');
});

test('request-file failure after scratch admission seals failure before any child starts', t => {
  const f = fixture(t); let calls = 0;
  const report = packageBusiness(f.args, {root: f.root, authenticate: f.auth, scratch() {
    const job = f.scratch(); rmSync(job.directory, {recursive: true}); return job;
  }, execute() { calls++; assert.fail('must not launch'); }});
  assert.equal(report.state, 'failed'); assert.equal(calls, 0); assert.equal(report.handlesReaped, true);
  assert.equal(f.terminal.state, 'failed'); assert.equal(f.cleaned, true);
});
