import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { offlineCases, liveCases, manualCases } from '../tests/baseline/catalog.mjs';
import { assessLive, goCaseResult, modulePath, options, parseGoEvents, parseTAP, repository, runCommand, sameLiveEvidence } from './baseline-common.mjs';
import { runLive } from './live-baseline.mjs';

function fixture(t) {
  const output = mkdtempSync(join(tmpdir(), 'lycheedev-baseline-'));
  t.after(() => rmSync(output, { recursive: true, force: true }));
  return { schema: 'lycheedev.baseline.v1', mode: 'live', output, source: { commit: 'fixture', dirty: false },
    checks: [], cases: [...liveCases, ...manualCases].map(c => ({ ...c, state: 'not_run' })) };
}
function envelope(spec = liveCases[0]) {
  return { schema: 'lycheedev.result.v1', ok: !spec.expectedFailure, operationId: `OP-${spec.id}`, result: {
    operationId: `OP-${spec.id}`, goal: 'finished', complete: true, cleanup: 'complete',
    business: { state: spec.expectedFailure ? 'failed' : 'passed' },
    report: { state: 'verified', bodyCapture: 'CAP-body', receiptCapture: 'CAP-receipt', sha256: 'digest', content: {
      acceptedBudgetSeconds: spec.budget, error: spec.expectedFailure ? 'BASELINE_EXPECTED_FAILURE' : undefined,
      result: { passed: true, baseline: spec.id, sum: 55, unobstructed: true, scope: 'character-v1',
        pages: ['runner', 'objects', 'events', 'trace', 'diagnostics', 'exports', 'automation', 'about', 'settings'] },
    } }, display: { state: 'cleared', capture: 'CAP-clear' },
  } };
}
const processResult = { code: 0, error: null, signal: null };

test('catalog IDs and required test names resolve to real Go tests', () => {
  const all = [...offlineCases, ...liveCases, ...manualCases];
  assert.equal(new Set(all.map(c => c.id)).size, all.length);
  for (const spec of offlineCases) {
    const dir = join(repository, spec.package);
    const source = readdirSync(dir).filter(f => f.endsWith('_test.go')).map(f => readFileSync(join(dir, f), 'utf8')).join('\n');
    for (const name of spec.tests) assert.match(source, new RegExp(`^func ${name}\\(t \\*testing\\.T\\)`, 'm'), `${spec.id}: ${name}`);
  }
});
test('strict options preserve spaces and reject obsolete flags, duplicates and missing values', () => {
  assert.deepEqual(options(['--cli', 'C:\\Program Files\\cli.exe'], ['--cli']), { '--cli': 'C:\\Program Files\\cli.exe' });
  for (const args of [['--skip-hide'], ['--connect', '--pid 1'], ['--cli'], ['--cli', '--out'], ['--cli', 'a', '--cli', 'b']]) {
    assert.throws(() => options(args, ['--cli', '--out']));
  }
});
test('Go evidence requires executed tests and package termination', () => {
  for (const text of ['', 'ok no tests to run', JSON.stringify({ Package: 'pkg', Action: 'pass' }), JSON.stringify({ Package: 'pkg', Test: 'TestA', Action: 'pass' })]) assert.throws(() => parseGoEvents(text));
  const lines = [{ Package: `${modulePath}pkg`, Test: 'TestA', Action: 'pass' }, { Package: `${modulePath}pkg`, Action: 'pass' }];
  const evidence = parseGoEvents(lines.map(JSON.stringify).join('\n'));
  const spec = { package: 'pkg', tests: ['TestA'] };
  assert.equal(goCaseResult(spec, evidence).state, 'passed');
  assert.equal(goCaseResult({ ...spec, tests: ['TestRenamed'] }, evidence).state, 'failed');
  evidence.tests.set(`${modulePath}pkg:TestA/required-client`, 'skip');
  assert.equal(goCaseResult(spec, evidence).state, 'blocked');
  evidence.tests.set(`${modulePath}pkg:TestA/required-client`, 'fail');
  assert.equal(goCaseResult(spec, evidence).state, 'failed');
});
test('Node zero tests, missing summary, skips and TODOs never pass', () => {
  const summary = (tests, pass, fail, skipped = 0, todo = 0) => `# tests ${tests}\n# pass ${pass}\n# fail ${fail}\n# cancelled 0\n# skipped ${skipped}\n# todo ${todo}\n`;
  assert.throws(() => parseTAP('TAP version 13\n'));
  assert.throws(() => parseTAP(summary(0, 0, 0)));
  assert.equal(parseTAP(summary(2, 2, 0)).state, 'passed');
  assert.equal(parseTAP(summary(2, 1, 1)).state, 'failed');
  assert.equal(parseTAP(summary(2, 1, 0, 1)).state, 'blocked');
  assert.equal(parseTAP(summary(2, 1, 0, 0, 1)).state, 'blocked');
});
test('live verifier accepts expected business failure only with complete evidence', () => {
  assert.equal(assessLive(processResult, envelope(), liveCases[0]).state, 'passed');
  const spec = liveCases[2], failed = envelope(spec);
  assert.equal(assessLive({ ...processResult, code: 5 }, failed, spec).state, 'passed');
  failed.result.report.content.error = 'some unrelated failure';
  assert.equal(assessLive({ ...processResult, code: 5 }, failed, spec).state, 'failed');
  assert.equal(assessLive({ ...processResult, code: 6 }, envelope(), spec).state, 'blocked');
  assert.equal(assessLive({ ...processResult, code: null, signal: 'SIGTERM' }, envelope(), spec).state, 'blocked');
});
for (const [label, mutate] of [
  ['wrong exit', (_, run) => { run.code = 5; }],
  ['false ok', e => { e.ok = false; }],
  ['wrong schema', e => { e.schema = 'wrong'; }],
  ['wrong operation', e => { e.result.operationId = 'OP-other'; }],
  ['abandoned', e => { e.result.cleanup = 'abandoned'; }],
  ['not complete', e => { e.result.complete = false; }],
  ['unavailable report', e => { e.result.report.state = 'unavailable'; }],
  ['missing report capture', e => { delete e.result.report.bodyCapture; }],
  ['missing receipt capture', e => { delete e.result.report.receiptCapture; }],
  ['pending display', e => { e.result.display.state = 'pending'; }],
  ['missing clear proof', e => { delete e.result.display.capture; }],
  ['wrong sum', e => { e.result.report.content.result.sum = 54; }],
  ['wrong budget', e => { e.result.report.content.acceptedBudgetSeconds = 120; }],
  ['wrong probe', e => { e.result.report.content.result.baseline = 'other'; }],
]) test(`live verifier refuses ${label}`, () => {
  const e = envelope(), run = { ...processResult }; mutate(e, run);
  assert.equal(assessLive(run, e, liveCases[0]).state, 'failed');
});
test('page coverage and transport overlay are asserted before Finish', () => {
  const spec = liveCases[3], e = envelope(spec);
  assert.equal(assessLive(processResult, e, spec).state, 'passed');
  e.result.report.content.result.pages.pop();
  assert.equal(assessLive(processResult, e, spec).state, 'failed');
  const overlay = envelope(spec); overlay.result.report.content.result.unobstructed = false;
  assert.equal(assessLive(processResult, overlay, spec).state, 'failed');
});
test('repeated completion requires the identical archived evidence', () => {
  const first = envelope(), changed = envelope();
  assert.ok(sameLiveEvidence(first, changed));
  changed.result.display.capture = 'CAP-new';
  assert.equal(sameLiveEvidence(first, changed), false);
  assert.equal(sameLiveEvidence({}, {}), false);
});
test('live runner stops after pending and retains request, raw output and operation', async t => {
  const report = fixture(t); let calls = 0;
  const code = await runLive(report, { '--session': 'SESSION-fixed', '--cli': 'unused' }, async (out, id, cli, args) => {
    calls++; assert.equal(args[args.indexOf('--session') + 1], 'SESSION-fixed');
    const onDisk = JSON.parse(readFileSync(join(out, 'report.json')));
    assert.equal(onDisk.cases[0].state, 'running');
    assert.ok(onDisk.cases[0].request && onDisk.cases[0].sha256);
    const stdout = join(out, `${id}.stdout`);
    writeFileSync(stdout, JSON.stringify({ schema: 'lycheedev.result.v1', ok: false, operationId: 'OP-pending', result: { complete: false } }));
    return { ...processResult, code: 6, stdout };
  });
  assert.equal(code, 2); assert.equal(calls, 1);
  assert.equal(report.cases[0].operationId, 'OP-pending');
  assert.ok(report.cases.slice(1).every(c => c.state === 'not_run'));
});
test('live runner uses complete execute, exact repeat and resume, preserving manual not_run', async t => {
  const report = fixture(t), calls = [];
  assert.equal(await runLive(report, { '--session': 'SESSION-fixed', '--cli': 'unused', '--home': 'C:/space path/home' }, async (out, id, cli, args) => {
    calls.push(args);
    const spec = liveCases.find(c => c.id === id) ?? liveCases[0];
    const stdout = join(out, `${id}.stdout`); writeFileSync(stdout, JSON.stringify(envelope(spec)));
    return { ...processResult, code: spec.expectedFailure ? 5 : 0, stdout };
  }), 0);
  assert.equal(calls.length, 5);
  assert.deepEqual(calls[1], calls[0]);
  assert.deepEqual(calls[2].slice(0, 3), ['live', 'resume', 'OP-LIVE-01']);
  assert.ok(report.cases.filter(c => c.kind === 'manual').every(c => c.state === 'not_run'));
  assert.equal(new Set(report.cases.filter(c => c.request).map(c => c.request)).size, 3);
});
test('process transport preserves argument boundaries and nonzero exit with valid stdout', async t => {
  const { output } = fixture(t);
  const result = await runCommand(output, 'process', process.execPath,
    ['-e', 'console.log(JSON.stringify(process.argv.slice(1))); process.exit(5)', 'path with spaces', 'literal; $()']);
  assert.equal(result.code, 5);
  assert.deepEqual(JSON.parse(readFileSync(result.stdout)), ['path with spaces', 'literal; $()']);
  assert.ok(readFileSync(join(output, 'process.command.json'), 'utf8').includes('startedAt'));
});
