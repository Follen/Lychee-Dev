// Controlled Node checker/fault tests, not sealed-EXE acceptance evidence.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { APPROVED_ENGINE_SUMMARIES, compareDescribeContract } from './describe-contract.mjs';
import { GO_COMMIT, sha256 } from './oracle.mjs';
import { runCliSmoke } from '../../tools/rust-cli-smoke.mjs';

const fixtureBytes = readFileSync(new URL('./go-3.2.0.manifest.json', import.meta.url));
const frozen = JSON.parse(fixtureBytes);
function commands() { return frozen.commands.map(({parserFlags, ...definition}) => structuredClone(definition)); }
function envelope(rows = commands()) { return {schema: 'lycheedev.result.v1', ok: true, result: {commands: rows}, error: null}; }
function engineChanges(value = envelope()) {
  for (const change of APPROVED_ENGINE_SUMMARIES) {
    const command = value.result.commands.find(row => row.path === change.path);
    assert.equal(command.summary, change.expected);
    command.summary = change.actual;
  }
  return value;
}

test('all 84 fixed Go definitions and every ordered metadata field match exactly', () => {
  const value = envelope(), before = structuredClone(value);
  const report = compareDescribeContract(frozen, value);
  assert.equal(frozen.oracle.commit, GO_COMMIT); assert.equal(report.rawDiff.expectedCount, 84);
  assert.equal(report.rawExactMatch, true); assert.equal(report.acceptedContractMatch, true);
  assert.equal(report.state, 'pass'); assert.deepEqual(report.approvedEngineSummaryChanges, []);
  assert.deepEqual(value, before, 'comparison must not mutate the response');
});

test('only two exact engine substitutions are accepted and both original differences remain visible', () => {
  const report = compareDescribeContract(frozen, engineChanges());
  assert.equal(report.rawExactMatch, false); assert.equal(report.rawDiff.state, 'fail');
  assert.equal(report.acceptedContractMatch, true); assert.equal(report.state, 'pass');
  assert.deepEqual(report.rawDiff.changed.map(row => row.path), ['doctor', 'source batch']);
  assert.deepEqual(report.approvedEngineSummaryChanges.map(row => ({path: row.path, expected: row.expected, actual: row.actual})), APPROVED_ENGINE_SUMMARIES);
});

test('nearby engine wording, different source bytes, and changes on another command are not approved', () => {
  for (const change of APPROVED_ENGINE_SUMMARIES) {
    const value = engineChanges();
    value.result.commands.find(row => row.path === change.path).summary += ' ';
    const report = compareDescribeContract(frozen, value);
    assert.equal(report.acceptedContractMatch, false);
    assert.ok(report.unapprovedChanges.some(row => row.path === change.path));
    const oracle = structuredClone(frozen);
    oracle.commands.find(row => row.path === change.path).summary += ' ';
    assert.equal(compareDescribeContract(oracle, engineChanges()).acceptedContractMatch, false);
  }
  const value = engineChanges();
  value.result.commands.find(row => row.path === 'source session close').summary = APPROVED_ENGINE_SUMMARIES[0].actual;
  assert.equal(compareDescribeContract(frozen, value).acceptedContractMatch, false);
});

test('missing live doctor or any implemented Source public route remains an actual failure', () => {
  for (const path of ['live doctor', 'source refs', 'source context', 'source batch', 'source session status', 'source session close']) {
    const value = engineChanges(); value.result.commands = value.result.commands.filter(row => row.path !== path);
    const report = compareDescribeContract(frozen, value);
    assert.equal(report.state, 'fail'); assert.equal(report.acceptedContractMatch, false);
    assert.deepEqual(report.rawDiff.missing, [path]);
  }
});

test('equal command counts cannot conceal a missing path behind an extra path', () => {
  const value = envelope(), path = value.result.commands[0].path;
  value.result.commands[0].path = 'unreviewed extra';
  const report = compareDescribeContract(frozen, value);
  assert.deepEqual(report.rawDiff.missing, [path]); assert.deepEqual(report.rawDiff.extra, ['unreviewed extra']);
  assert.equal(report.acceptedContractMatch, false);
});

test('all metadata and ordered flags remain exact even on an approved-summary command', () => {
  for (const [field, mutate] of [
    ['mutates', row => { row.mutates = !row.mutates; }],
    ['arguments', row => { row.arguments = ['<invented>']; }],
    ['flags', row => { row.flags.reverse(); }],
    ['flags', row => { row.flags[0] += ' changed default'; }],
  ]) {
    const value = engineChanges(); mutate(value.result.commands.find(row => row.path === 'doctor'));
    const report = compareDescribeContract(frozen, value);
    assert.equal(report.acceptedContractMatch, false);
    assert.ok(report.unapprovedChanges.find(row => row.path === 'doctor').changes.some(row => row.field === field));
    assert.equal(report.approvedEngineSummaryChanges.length, 2);
  }
});

test('command order changes fail independently of equal fields and membership', () => {
  const value = envelope(); value.result.commands.reverse();
  const report = compareDescribeContract(frozen, value);
  assert.equal(report.rawDiff.state, 'pass'); assert.equal(report.commandOrder.matches, false);
  assert.equal(report.rawExactMatch, false); assert.equal(report.acceptedContractMatch, false);
});

test('malformed envelopes, duplicate paths/flags, missing or unknown metadata fail closed', () => {
  const mutations = [
    value => { value.schema = 'other'; }, value => { value.ok = false; }, value => { delete value.error; },
    value => { delete value.result; }, value => { value.result.commands = null; },
    value => { value.result.commands.push(value.result.commands[0]); },
    value => { value.result.commands[0].flags.push(value.result.commands[0].flags[0]); },
    value => { value.result.commands[0].mutates = 'true'; },
    value => { delete value.result.commands[0].arguments; },
    value => { value.result.commands[0].undocumented = true; },
  ];
  for (const mutate of mutations) {
    const value = envelope(); mutate(value); const report = compareDescribeContract(frozen, value);
    assert.equal(report.state, 'fail'); assert.equal(report.rawExactMatch, false);
    assert.equal(report.acceptedContractMatch, false); assert.equal(typeof report.invalid, 'string');
  }
  assert.throws(() => compareDescribeContract({...frozen, oracle: {commit: 'moving-main'}}, envelope()), /wrong_oracle/);
});

function runnerFixture(t, response = engineChanges()) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-describe-checker-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const evidence = join(root, 'evidence'), directory = join(root, 'scratch');
  mkdirSync(evidence); mkdirSync(directory);
  const calls = [], events = [], artifact = {sourceBinary: join(root, 'not-executed.exe'), binarySHA256: 'a'.repeat(64), inputHashes: {sourceTree: 'b'.repeat(64)}};
  const f = {root, evidence, calls, events, response, verified: 0, sealed: null, cleaned: false};
  f.options = {mode: 'contract', verify: () => { f.verified++; return structuredClone(artifact); },
    loadOracle: () => ({oracle: frozen, fixtureSHA256: sha256(fixtureBytes)}),
    begin: () => ({directory, evidence, seal: value => {
      for (const log of value.logs) assert.ok(readFileSync(log).length >= 0, 'all evidence exists before seal');
      events.push('seal'); f.sealed = value;
      f.savedReport = JSON.parse(readFileSync(join(evidence, 'cli-smoke.json')));
    }, cleanup: () => { assert.ok(f.sealed); events.push('cleanup'); f.cleaned = true; }}),
    execute: (binary, args) => {
      assert.equal(binary, artifact.sourceBinary); calls.push(args);
      return {status: 0, handlesReaped: true, logs: [], stdout: Buffer.from(JSON.stringify(f.response)), stderr: Buffer.alloc(0)};
    }};
  f.run = () => runCliSmoke(root, 'synthetic-artifact.json', f.options);
  return f;
}

test('contract runner invokes describe from sealed identity and persists both comparison layers', t => {
  const f = runnerFixture(t), result = f.run();
  assert.deepEqual(f.calls, [['describe', '--format=json']]); assert.equal(f.verified, 2);
  assert.equal(result.exitCode, 0); assert.equal(result.contract.rawExactMatch, false);
  assert.equal(result.contract.acceptedContractMatch, true); assert.equal(result.contract.binarySHA256, 'a'.repeat(64));
  assert.equal(result.contract.fixtureSHA256, sha256(fixtureBytes));
  assert.equal(f.savedReport.state, 'succeeded'); assert.equal(f.sealed.state, 'succeeded');
  assert.deepEqual(f.events, ['seal', 'cleanup']);
  assert.deepEqual(JSON.parse(readFileSync(join(f.evidence, 'cli-contract.json'))), result.contract);
});

test('contract mismatch returns exit 1 only after retaining raw output, diff, summary and ownership seal', t => {
  const value = engineChanges(); value.result.commands = value.result.commands.filter(row => row.path !== 'live doctor');
  const f = runnerFixture(t, value), result = f.run();
  assert.equal(result.exitCode, 1); assert.equal(f.savedReport.state, 'failed'); assert.equal(f.sealed.state, 'failed');
  assert.equal(f.verified, 2); assert.equal(f.cleaned, true);
  assert.deepEqual(result.contract.rawDiff.missing, ['live doctor']);
  assert.equal(result.contract.approvedEngineSummaryChanges.length, 2);
  for (const file of ['cli-0.stdout.json', 'cli-0.stderr.bin', 'cli-contract.json', 'cli-smoke.json']) {
    assert.ok(f.sealed.logs.includes(join(f.evidence, file)), file);
  }
});

test('invalid envelope persists a failing contract report rather than a passing empty inventory', t => {
  const f = runnerFixture(t, {ok: true, result: {commands: []}}), result = f.run();
  assert.equal(result.exitCode, 1); assert.equal(result.contract.state, 'fail');
  assert.match(result.contract.invalid, /envelope/); assert.equal(f.savedReport.state, 'failed');
});

test('execution failure and malformed JSON retain original logs and a failed summary', t => {
  for (const [status, stdout] of [[3, 'original execution failure'], [0, '{bad JSON']]) {
    const f = runnerFixture(t);
    f.options.execute = () => ({status, handlesReaped: true, stdout: Buffer.from(stdout), stderr: Buffer.from('original stderr'), logs: []});
    assert.throws(f.run);
    assert.equal(f.savedReport.state, 'failed'); assert.equal(f.sealed.state, 'failed'); assert.equal(f.cleaned, true);
    assert.equal(readFileSync(join(f.evidence, 'cli-0.stdout.json'), 'utf8'), stdout);
    assert.equal(readFileSync(join(f.evidence, 'cli-0.stderr.bin'), 'utf8'), 'original stderr');
    assert.equal(typeof f.savedReport.error, 'string');
  }
});

test('unreaped process is interrupted and retained rather than cleaned', t => {
  const f = runnerFixture(t);
  f.options.execute = () => ({status: 0, handlesReaped: false, stdout: Buffer.from('{}'), stderr: Buffer.alloc(0), logs: []});
  assert.throws(f.run, /execution/); assert.equal(f.sealed.state, 'interrupted');
  assert.equal(f.savedReport.state, 'interrupted'); assert.equal(f.cleaned, false); assert.deepEqual(f.events, ['seal']);
});

test('changed binary after describe invalidates the overall report despite matching command metadata', t => {
  const f = runnerFixture(t), verify = f.options.verify;
  f.options.verify = () => { const artifact = verify(); if (f.verified === 2) artifact.binarySHA256 = 'c'.repeat(64); return artifact; };
  assert.throws(f.run, /artifact_changed/); assert.equal(f.sealed.state, 'failed'); assert.equal(f.savedReport.state, 'failed');
});

test('ordinary admission and isolated doctor preserve their original commands and checks', t => {
  const f = runnerFixture(t); f.options.mode = 'admission';
  f.options.execute = (_binary, args) => {
    f.calls.push(args);
    return {status: 0, handlesReaped: true, logs: [], stderr: Buffer.alloc(0),
      stdout: Buffer.from(JSON.stringify({ok: true, result: args[0] === 'version' ? {version: 'synthetic'} : {commands: []}}))};
  };
  assert.equal(f.run().exitCode, 0); assert.deepEqual(f.calls.map(row => row[0]), ['version', '--help', 'describe']);
  const d = runnerFixture(t); d.options.mode = 'doctor';
  d.options.execute = (_binary, args) => {
    d.calls.push(args);
    return {status: 0, handlesReaped: true, logs: [], stderr: Buffer.alloc(0),
      stdout: Buffer.from(JSON.stringify({ok: true, result: {checks: [
        {id: 'emmy.runtime', ok: true, scope: 'embedded-worker-fixture', lifecycle: {retired: true}},
        {id: 'release.integrity', ok: false, status: 'warn'},
      ]}}))};
  };
  assert.equal(d.run().exitCode, 0); assert.deepEqual(d.calls.map(row => row[0]), ['init', 'doctor']);
});

test('CLI rejects mixed and unknown modes before opening an artifact', () => {
  const script = new URL('../../tools/rust-cli-smoke.mjs', import.meta.url);
  for (const args of [['missing.json', '--contract', '--doctor'], ['missing.json', '--unknown']]) {
    const result = spawnSync(process.execPath, [fileURLToPath(script), ...args], {encoding: 'utf8', timeout: 20000});
    assert.notEqual(result.status, 0); assert.match(result.stderr, /usage:/);
  }
});
