// Controller-only regressions: never compile Rust, launch Lua or run a test EXE.
// They validate the current bridge controller's parsing/admission contracts.
import assert from 'node:assert/strict';
import test from 'node:test';
import { join, resolve } from 'node:path';
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
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
function sealHostProof(row) {
  const bytes = Buffer.from(JSON.stringify(row.proof, null, 2) + '\r\n');
  writeFileSync(row.evidence, bytes); row.evidenceSHA256 = hash(bytes);
  // Production reads the same exact file as both proof JSON and hash input.
  row.proof = JSON.parse(bytes);
  return bytes;
}
function hostProofs(t) {
  mkdirSync(join(root, '.tmp'), {recursive: true});
  const directory = mkdtempSync(join(root, '.tmp', 'host-parity-test-'));
  t.after(() => rmSync(directory, {recursive: true, force: true}));
  const float64Bits = Array.from({length: 30}, (_, index) => {
    const bytes = Buffer.alloc(8); bytes.writeDoubleBE(index === 1 ? -0 : index === 0 ? 0 : index - 1); return bytes.toString('hex');
  });
  const rows = [
    {case: 'memory', proof: {schema: 'lycheedev.formal-lua-host-parity.v1', passed: true, case: 'memory',
      payload: 'hello', sequence: 17, nonce: '1'.repeat(32), runtime: '2'.repeat(32), ticket: '3'.repeat(32), kind: 3, state: 3, recordSHA256: 'a'.repeat(64)}},
    {case: 'numeric', proof: {schema: 'lycheedev.formal-lua-host-parity.v1', passed: true, case: 'numeric', valueCount: 30, float64Bits,
      recoveredBytesEqual: true, completedRetryInputCountUnchanged: true, resourcesReleased: true, disconnected: true, executions: 1, reportSHA256: 'b'.repeat(64)}},
  ].map(row => ({...row, evidence: join(directory, `${row.case}.json`)}));
  for (const row of rows) sealHostProof(row);
  return rows;
}
test('host parity admits both complete proofs and their original evidence bytes', t => {
  const rows = hostProofs(t), original = rows.map(row => readFileSync(row.evidence));
  const admitted = verifyHostParity(rows);
  assert.deepEqual(admitted.map(row => row.testCase), cases.slice(9));
  assert.deepEqual(admitted.map(row => row.proof), rows.map(row => row.proof));
  for (let index = 0; index < rows.length; index++) {
    assert.deepEqual(readFileSync(rows[index].evidence), original[index]);
    assert.equal(admitted[index].evidenceSHA256, hash(original[index]));
  }
});
test('host parity rejects missing or duplicate cases before field validation', t => {
  const rows = hostProofs(t);
  for (const missing of [[], rows.slice(0, 1), [rows[0], rows[0]]]) assert.throws(() => verifyHostParity(missing), /host_case_set/);
});
for (const [name, index, change] of [
  ['schema', 0, proof => {proof.schema = 'unknown';}],
  ['failed proof', 0, proof => {proof.passed = false;}],
  ['proof case identity', 0, proof => {proof.case = 'numeric';}],
  ['memory payload', 0, proof => {proof.payload = 'changed';}],
  ['memory sequence', 0, proof => {proof.sequence = 18;}],
  ['memory nonce', 0, proof => {proof.nonce = '0'.repeat(32);}],
  ['memory runtime', 0, proof => {proof.runtime = '0'.repeat(32);}],
  ['memory ticket', 0, proof => {proof.ticket = '0'.repeat(32);}],
  ['memory kind', 0, proof => {proof.kind = 4;}],
  ['memory state', 0, proof => {proof.state = 4;}],
  ['memory record hash format', 0, proof => {proof.recordSHA256 = 'bad';}],
  ['numeric value count', 1, proof => {proof.valueCount = 29;}],
  ['numeric bit count', 1, proof => {proof.float64Bits.pop();}],
  ['numeric bit format', 1, proof => {proof.float64Bits[2] = 'g'.repeat(16);}],
  ['positive zero bits', 1, proof => {proof.float64Bits[0] = '3ff0000000000000';}],
  ['negative zero bits', 1, proof => {proof.float64Bits[1] = '0000000000000000';}],
  ...['recoveredBytesEqual', 'completedRetryInputCountUnchanged', 'resourcesReleased', 'disconnected']
    .map(field => [field, 1, proof => {proof[field] = false;}]),
  ['replayed execution', 1, proof => {proof.executions = 2;}],
  ['numeric report hash format', 1, proof => {proof.reportSHA256 = 'bad';}],
]) test(`host parity rejects ${name} after admitting the complete case set`, t => {
  const rows = hostProofs(t); change(rows[index].proof); sealHostProof(rows[index]);
  assert.throws(() => verifyHostParity(rows), error => {
    assert.equal(error.code, 'ERR_ASSERTION');
    assert(!error.message.includes('host_case_set'), 'negative fixture stopped before its field check');
    return true;
  });
});
for (const index of [0, 1]) test(`host parity checks ${index === 0 ? 'memory' : 'numeric'} evidence hash after all proof fields`, t => {
  const rows = hostProofs(t), path = rows[index].evidence, before = readFileSync(path);
  writeFileSync(path, Buffer.concat([before, Buffer.from('changed evidence bytes')]));
  assert.throws(() => verifyHostParity(rows), error => {
    assert.equal(error.code, 'ERR_ASSERTION');
    assert.equal(error.actual, rows[index].evidenceSHA256);
    assert.equal(error.expected, hash(readFileSync(path)));
    return true;
  });
});
test('host parity rejects an incorrect declared evidence hash and a missing original file', t => {
  const rows = hostProofs(t), original = rows[0].evidenceSHA256;
  rows[0].evidenceSHA256 = 'f'.repeat(64);
  assert.throws(() => verifyHostParity(rows), error => error.code === 'ERR_ASSERTION' && error.expected === original);
  rows[0].evidenceSHA256 = original; rmSync(rows[0].evidence);
  assert.throws(() => verifyHostParity(rows), {code: 'ENOENT'});
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
