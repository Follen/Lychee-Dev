// Read-only custody/current-consumer checks. Never execute historical source,
// Lua, Cargo, Go, package installation or the explicit bridge-input lab.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {lstatSync, readFileSync, readdirSync} from 'node:fs';
import {dirname, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import test from 'node:test';
import {offlineCases, liveCases} from '../tests/baseline/catalog.mjs';
import {goCaseResult, modulePath} from './baseline-common.mjs';
import {ORACLE as CURRENT_GO_ORACLE} from '../tests/rust-parity/formal-lua-3.2.4/assertions.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const archive = 'tests/history/legacy-ui-00fd643';
const RETIREMENT_GO_ORACLE = '3766f9cacb52e93a803a626187e2a1d54a445468';
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
function read(path) {
  assert(!path.split('/').some(part => !part || part === '.' || part === '..'));
  const full = join(root, path);
  let cursor = full;
  for (;;) {
    assert(!lstatSync(cursor).isSymbolicLink(), cursor);
    if (cursor === root) break;
    cursor = dirname(cursor);
  }
  const stat = lstatSync(full);
  assert(stat.isFile() && stat.size <= (2 << 20), path);
  return readFileSync(full);
}
const manifestBytes = read(`${archive}/manifest.json`);
const manifest = JSON.parse(manifestBytes);
const priorOracleBytes = read(`${archive}/current-oracle-inputs.json`);
const priorOracle = JSON.parse(priorOracleBytes);
const oracleBytes = read(`${archive}/current-oracle-inputs-3.2.4.json`);
const oracle = JSON.parse(oracleBytes);
const fixture = JSON.parse(read('tests/fixtures/legacy-addon/manifest.json'));
const extras = ['Definitions', 'Identity', 'InputProtocol', 'InputSignal', 'Investigation', 'MatrixSymbol', 'ProbeQueue', 'ProbeRunner', 'ReceiptView', 'Receiver', 'Reentry', 'ReportStore', 'Session', 'StartupBeacon'];

test('104 historical originals retain exact bytes, Git identities and non-executable suffixes', () => {
  assert.equal(sha(manifestBytes), 'fe00766237b3dd3589393a9bf1f95ef38405288ced5bb76bd0332cdac73a8f29');
  assert.equal(manifest.sourceCommit, '00fd643908badaf282a5ddbd1e06a88c0e291d29');
  // This is the immutable retirement-time fact, not today's behavior baseline.
  assert.equal(manifest.currentGoOracle, RETIREMENT_GO_ORACLE);
  assert.equal(manifest.executableEntry, null);
  assert.equal(manifest.files.length, 104);
  const expected = [];
  for (const row of manifest.files) {
    assert.equal(row.historicalPath, `${archive}/source/${row.originalPath}.source`);
    const bytes = read(row.historicalPath);
    assert.equal(bytes.length, row.bytes, row.originalPath);
    assert.equal(sha(bytes), row.originalSHA256, row.originalPath);
    assert.equal(row.originalSHA256, row.historicalSHA256);
    assert.equal(createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex'), row.gitBlob);
    if (row.disposition === 'retired_from_current_discovery') {
      assert.throws(() => lstatSync(join(root, row.originalPath)), {code: 'ENOENT'});
    }
    expected.push(row.historicalPath);
  }
  const found = [];
  let entries = 0;
  function walk(path) {
    assert(++entries <= 250, 'archive entry budget');
    const stat = lstatSync(join(root, path));
    assert(!stat.isSymbolicLink(), path);
    if (stat.isDirectory()) for (const child of readdirSync(join(root, path))) walk(`${path}/${child}`);
    else { assert(stat.isFile() && path.endsWith('.source'), path); found.push(path); }
  }
  walk(`${archive}/source`);
  assert.deepEqual(found.sort(), expected.sort());
  assert.match(read(`${archive}/README.md`).toString(), /There is no executable entry for this archive/);
});

test('current Lua consumers are fixed Go324 inputs while retirement-time Go323 identities remain unchanged', () => {
  assert.equal(sha(priorOracleBytes), 'f5764f0ef2132f8a8a0ad9ba08e65ef39a3758b9612d9d1fc849d39a083a6e6a');
  assert.equal(sha(oracleBytes), 'd6e6f3ee9128388237e5a9462e4a9976a8282af6f434eb49f91961559089c5d8');
  assert.equal(oracle.oracleCommit, CURRENT_GO_ORACLE);
  assert.equal(oracle.oracleRelease, 'Go v3.2.4');
  assert.deepEqual(oracle.previousOracle, {path: `${archive}/current-oracle-inputs.json`, commit: RETIREMENT_GO_ORACLE, sha256: sha(priorOracleBytes)});
  assert.equal(oracle.files.length, 50);
  assert.deepEqual(oracle.files.map(row => row.path), priorOracle.files.map(row => row.path));
  for (const row of oracle.files) {
    assert.equal(row.oracleCommit, CURRENT_GO_ORACLE);
    const original = execFileSync('git', ['--no-pager', '--no-optional-locks', '-c', 'core.fsmonitor=false', 'cat-file', 'blob', `${CURRENT_GO_ORACLE}:${row.path}`], {cwd: root, timeout: 10000, maxBuffer: 2 << 20});
    assert.equal(sha(original), row.oracleSHA256, row.path);
    if (row.substitution) {
      assert.equal(row.substitution, 'one release literal 3.2.4 -> 3.5.0');
      const text = original.toString('utf8'); assert.equal(text.split('3.2.4').length, 2, row.path);
      assert.equal(sha(Buffer.from(text.replace('3.2.4', '3.5.0'))), row.currentSHA256, row.path);
    } else if (row.adaptation) {
      const prior = priorOracle.files.find(file => file.path === row.path);
      assert.equal(row.adaptation, prior.adaptation); assert.equal(row.oracleSHA256, prior.oracleSHA256);
      assert.equal(row.currentSHA256, prior.currentSHA256, 'Preserve the existing explicit consumer adaptation');
    } else assert.equal(row.currentSHA256, row.oracleSHA256, row.path);
    assert.equal(sha(read(row.path)), row.currentSHA256, row.path);
  }
  assert.deepEqual(oracle.files.filter(row => row.substitution).map(row => row.path).sort(), ['tests/addon/env.lua', 'tests/addon/t_about.lua']);
  const expected = oracle.files.map(row => row.path).filter(path => /^tests\/addon\/t_.*\.lua$/.test(path)).sort();
  const actual = readdirSync(join(root, 'tests/addon')).filter(name => /^t_.*\.lua$/.test(name)).map(name => `tests/addon/${name}`).sort();
  assert.equal(expected.length, 36);
  assert.deepEqual(actual, expected);
  assert.match(read('tests/addon/addon_test.go').toString(), /filepath.Glob\("t_\*\.lua"\)/);
  assert.match(read('crates/lycheedev-live/tests/formal_lua_bridge.rs').toString(), /driver::Driver/);
  assert.equal((read('crates/lycheedev-live/tests/formal_lua_bridge.rs').toString().match(/^case!\(formal_lua_recovery_/gm) ?? []).length, 9);
  assert(read('tests/protocol/slot_driver_test.go').toString().includes('TestSlotDriverLuaRecoveryWithoutBusinessReplay'));
});

test('BASE-08/10/12 require every current Lua child and never accept only the parent pass', () => {
  for (const id of ['BASE-08', 'BASE-10', 'BASE-12']) {
    const spec = offlineCases.find(c => c.id === id);
    assert(spec && spec.luaSuites.length >= 3);
    assert.deepEqual(spec.profiles, ['retail', 'classic', 'titan', 'forever', 'classic-era', 'anniversary']);
    assert.equal(spec.tests.length, spec.luaSuites.length * 6);
    const prefix = `${modulePath}${spec.package}:`;
    const evidence = {tests: new Map([[`${prefix}TestWorkbenchLuaSuites`, 'pass']]), packages: new Map()};
    assert.equal(goCaseResult(spec, evidence).state, 'failed');
    for (const name of spec.tests) evidence.tests.set(prefix + name, 'pass');
    assert.equal(goCaseResult(spec, evidence).state, 'passed');
    evidence.tests.set(prefix + spec.tests[0], 'skip');
    assert.equal(goCaseResult(spec, evidence).state, 'blocked');
    evidence.tests.set(prefix + spec.tests[0], 'fail');
    assert.equal(goCaseResult(spec, evidence).state, 'failed');
  }
  assert.equal(liveCases.length, 0);
  assert.throws(() => lstatSync(join(root, 'tools/live-baseline.mjs')), {code: 'ENOENT'});
  assert.deepEqual(offlineCases.find(c => c.id === 'BASE-17').tests, ['TestWorkbenchLuaSuites', 'TestUnifiedTOCContract']);
});

test('Definitions and QR retain identical explicit fixtures and actual queue/QR/lab consumers', () => {
  assert.equal(fixture.files.length, 2);
  for (const row of fixture.files) {
    const original = manifest.files.find(item => item.originalPath === row.originalPath);
    assert(original);
    assert.equal(row.sha256, original.originalSHA256);
    const bytes = read(row.path);
    assert.equal(bytes.length, row.bytes);
    assert.equal(sha(bytes), row.sha256);
  }
  const queue = read('internal/bridge/queue_test.go').toString();
  assert(queue.includes('TestHistoricalQueueFixtureEmpty'));
  assert(queue.includes('../../tests/fixtures/legacy-addon/Definitions.lua'));
  assert(queue.includes('TestAcknowledgementFieldRejected'));
  const qr = read('tests/qrfixture/qrfixture_test.go').toString();
  assert(qr.includes('"tests", "fixtures", "legacy-addon", "MatrixSymbol.lua"'));
  assert(read('tests/qrfixture/fractional_test.go').toString().includes('../fixtures/legacy-addon/MatrixSymbol.lua'));
  const lab = read('tests/bridge-input/lab.mjs').toString();
  assert(lab.includes('tests/fixtures/legacy-addon/MatrixSymbol.lua'));
  assert(lab.includes(fixture.files.find(row => row.path.endsWith('/MatrixSymbol.lua')).sha256));
  assert(read('tests/fixtures/legacy-addon/MatrixSymbol.lua').toString().includes('Patrick Gundlach'));
  assert(read('packages/npm/lycheedev/test/install-smoke.mjs').toString().includes("existsSync(join(repository, 'addon/Bridge/Definitions.lua')), false"));
});

test('all fourteen production extras are absent while their negative ZIP cases stay active', () => {
  const toc = read('addon/Lychee Dev.toc').toString();
  const negative = read('tools/addon-package.test.mjs').toString();
  for (const name of extras) {
    assert.throws(() => lstatSync(join(root, 'addon/Bridge', `${name}.lua`)), {code: 'ENOENT'});
    assert(!toc.includes(`Bridge\\${name}.lua`));
    assert(negative.includes(`'Bridge/${name}.lua'`), name);
  }
  assert(negative.includes("'Bridge/NativeRuntime.lua'"));
  assert(read('tests/rust-parity/addon-delivery-3.2.2.test.mjs').toString().includes("assert.ok(!source.includes('Bridge/NativeRuntime.lua'), path)"));
});
