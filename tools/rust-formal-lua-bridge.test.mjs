// Controller-only regressions: never compile Rust, launch Lua or run a test EXE.
// They validate the current bridge controller's parsing/admission contracts.
import assert from 'node:assert/strict';
import test from 'node:test';
import { join, resolve } from 'node:path';
import { lstatSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { BRIDGE_INPUTS, SUPPORT, admitInterpreter, bridgePaths, selectTestExecutable, verifyBridgeInputs, verifyHostParity, verifyPeerLifetimes, verifyTestOutput } from './rust-formal-lua-bridge.mjs';
import { importLuaToolSupply } from './lua-tool-supply.mjs';
import { hash, verifyInterpreter } from './rust-formal-lua.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
const target = resolve('target');
const artifact = { reason: 'compiler-artifact', target: { name: 'formal_lua_bridge', kind: ['test'] }, profile: { test: true }, executable: join(target, 'debug/deps/formal_lua_bridge-a1b2c3.exe') };
const complete = { reason: 'build-finished', success: true };
const jsonl = rows => rows.map(row => JSON.stringify(row)).join('\n') + '\n';
const cases = [...['normal', 'after_bind', 'after_prepare', 'after_commit', 'after_confirm', 'after_release', 'reload_bind', 'compile_error', 'cleanup_error'].map(name => `formal_lua_recovery_${name}`),
  'formal_lua_memory_record_parity', 'formal_lua_numeric_report_preserves_finite_numbers'];
const output = (names = cases, summary = '11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;') => Buffer.from(
  `running ${names.length} tests\n${names.map(name => `test ${name} ... ok`).join('\n')}\n\ntest result: ok. ${summary} finished in 0.20s\n`);

test('bridge sources retain owned execution, user isolation and no frozen reference', () => {
  const bridge = readFileSync(join(root, 'tools/rust-formal-lua-bridge.mjs'), 'utf8');
  assert.match(bridge, /oneShot: false, isolateUser: true/);
  assert.match(bridge, /cleanLuaEnvironment\(\{ \.\.\.ownedUserEnvironment\(job\.directory\), LYCHEEDEV_FORMAL_LUA_BRIDGE_ADMISSION: admissionPath \}\)/);
  assert.match(bridge, /timeout: 255000/);
  assert.match(bridge, /--test-threads=1/);
  assert.match(bridge, /test', '--locked', '--offline/);
  for (const forbidden of ['tests/rust-parity', 'formal-lua-3.2', 'go-test-map', 'fixed Go']) assert.equal(bridge.includes(forbidden), false, forbidden);
  const formal = readFileSync(join(root, 'tools/rust-formal-lua.mjs'), 'utf8');
  assert.match(formal, /cleanLuaEnvironment\(ownedUserEnvironment\(job\.directory, env\)\)/);
  assert.match(formal, /luaSource\.sha256/);
  assert.match(formal, /release\/tools\/luac\.json/);
  assert.equal(SUPPORT.includes('tools/rust-formal-lua.mjs'), true);
});

test('bridge paths require a report, an artifact record and a fresh directory', () => {
  const paths = bridgePaths([], root);
  assert.equal(paths.runtimePath.endsWith('build-report.json'), true);
  assert.equal(paths.candidatePath.endsWith('artifact.json'), true);
  assert.throws(() => bridgePaths(['one', 'two'], root), /path_arguments/);
  assert.throws(() => bridgePaths(['a', 'b', 'c', 'd'], root), /path_arguments/);
  assert.throws(() => bridgePaths(['x/report.json', 'x/artifact.json', 'x/out'], root), /interpreter_record_required/);
});

test('portable interpreter admission maps runtime paths while preserving original production records', () => {
  const directory = resolve(root, '.tmp', `bridge-import-test-${process.pid}`);
  try {
    const imported = importLuaToolSupply(join(directory, 'supply'));
    const runtimePath = imported.lua51.report, runtime = verifyInterpreter(runtimePath);
    const original = readFileSync(runtimePath), producer = readFileSync(join(imported.lua51.directory, 'producer/build-report.json'));
    const execution = join(directory, 'execution'); mkdirSync(execution);
    const admitted = admitInterpreter(runtimePath, runtime, execution), bytes = readFileSync(admitted.report), report = JSON.parse(bytes);
    assert.equal(report.schema, 'lycheedev.formal-lua-interpreter.v1');
    assert.equal(hash(bytes), admitted.reportSHA256);
    assert.equal(hash(readFileSync(report.executable)), report.executableSHA256);
    assert.equal(hash(readFileSync(report.buildEvidence)), admitted.buildReceiptSHA256);
    assert.equal(report.admissionProjection.importRecordSHA256, hash(original));
    assert.equal(report.admissionProjection.producerReportSHA256, hash(producer));
    assert.deepEqual(readFileSync(runtimePath), original);
    assert.deepEqual(readFileSync(join(imported.lua51.directory, 'producer/build-report.json')), producer);
    assert.deepEqual(verifyInterpreter(runtimePath), runtime);
  } finally { rmSync(directory, {recursive: true, force: true}); }
});

test('selects exactly the current named integration artifact', () => {
  const selected = selectTestExecutable(jsonl([{ reason: 'compiler-artifact', target: { name: 'dependency' } }, artifact, complete]), target);
  assert.equal(selected.executable, artifact.executable);
  assert.deepEqual(selected.row, artifact);
});
for (const [name, rows] of [
  ['missing', [complete]], ['duplicate', [artifact, artifact, complete]],
  ['wrong target', [{ ...artifact, target: { name: 'native_e2e', kind: ['test'] } }, complete]],
  ['ordinary binary', [{ ...artifact, target: { name: 'formal_lua_bridge', kind: ['bin'] } }, complete]],
  ['non-test profile', [{ ...artifact, profile: { test: false } }, complete]],
  ['missing completion', [artifact]], ['failed completion', [artifact, { ...complete, success: false }]],
  ['duplicate completion', [artifact, complete, complete]],
  ['foreign path', [{ ...artifact, executable: resolve('foreign/formal_lua_bridge-a1b2c3.exe') }, complete]],
  ['wrong exe basename', [{ ...artifact, executable: join(target, 'debug/deps/native_e2e-a1.exe') }, complete]],
]) test(`rejects ${name}`, () => assert.throws(() => selectTestExecutable(jsonl(rows), target)));
test('requires structured Cargo JSON', () => assert.throws(() => selectTestExecutable('not JSON\n', target)));
test('accepts all eleven exact zero-skip case results', () => {
  assert.equal(verifyTestOutput(output()).passed, 11);
});
for (const [name, bytes, summary] of [
  ['missing case', output(cases.slice(1)), '11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;'],
  ['extra case', output([...cases, 'formal_lua_extra']), '11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;'],
  ['failure', Buffer.from(`${cases.map(c => `test ${c} ... ok`).join('\n')}\ntest result: FAILED. 10 passed; 1 failed; finished in 0.1s\n`), '11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;'],
  ['ignored', output(cases, '10 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;'), 'ignored'],
]) test(`rejects ${name}`, () => assert.throws(() => verifyTestOutput(bytes)));
const lifetime = name => ({ case: name, schema: 'lycheedev.formal-lua-peer-lifetime.v1', rootReaped: true, rootExit: 0, jobEmpty: true, workersJoined: true, forced: false, errors: [] });
test('accepts eleven exact peer lifetimes under the current process owner', () => {
  assert.equal(verifyPeerLifetimes(cases.map((name, index) => lifetime(['normal', 'bind', 'prepare', 'commit', 'confirm', 'release', 'reload_bind', 'compile_error', 'cleanup_error', 'memory', 'numeric'][index]))).length, 11);
});
test('rejects incomplete peer lifetime evidence', () => {
  assert.throws(() => verifyPeerLifetimes([lifetime('normal')]), /peer_case_set/);
  assert.throws(() => verifyPeerLifetimes(cases.map(name => ({ ...lifetime(name), forced: true }))));
});
test('host parity proofs remain exact for bytes, numbers and recovery', () => {
  const proof = rows => rows.map(({ case: name, proof }) => ({ case: name, evidence: join(root, name), evidenceSHA256: 'unused', proof }));
  assert.throws(() => verifyHostParity([]), /host_case_set/);
  assert.throws(() => verifyHostParity(proof([{ case: 'memory', proof: { schema: 'lycheedev.formal-lua-host-parity.v1', passed: true, case: 'memory', payload: 'hello', sequence: 17, nonce: '1'.repeat(32), runtime: '2'.repeat(32), ticket: '3'.repeat(32), kind: 3, state: 3, recordSHA256: 'a'.repeat(64) } }])), /host_case_set/);
});
test('current addon TOC and peer inputs are the admitted fixture', () => {
  const files = verifyBridgeInputs(root);
  assert.deepEqual(files.map(row => row.path), [...BRIDGE_INPUTS]);
  for (const row of files) {
    assert.equal(row.bytes, lstatSync(join(root, row.path)).size);
    assert.match(row.sha256, /^[a-f0-9]{64}$/);
  }
  assert.equal(files.length, 4);
});

test('precompiled CI bridge uses the recorded test target and never builds a replacement',()=>{
 const source=readFileSync(join(root,'tools/rust-formal-lua-bridge.mjs'),'utf8'),body=source.slice(source.indexOf('export function runPrebuiltBridge'),source.indexOf('export function main'));
 assert.match(body,/lycheedev\.ci-test-programs\.v1/);assert.match(body,/formal_lua_bridge/);assert.match(body,/test_record_changed/);assert.match(body,/program.bytes/);assert.match(body,/program.sha256/);
 assert.doesNotMatch(body,/runCargo\(|buildTest\(|verifyCandidateArtifact\(/);
 const execution=source.slice(source.indexOf('function executeBridge'));
 assert.match(execution,/admitInterpreter\(runtimePath, runtime, job.evidence\)/);
 assert.doesNotMatch(execution,/buildReceipt: /);
 assert.match(execution,/verifyTestOutput\(result.stdout\)/);assert.match(execution,/verifyPeerLifetimes/);assert.match(execution,/verifyHostParity/);
});
