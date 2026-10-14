import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createWGCObservation, main } from './wgc-observation.mjs';

const pkg = 'github.com/follenfang/lycheedev/internal/desktop';
const name = 'TestWGCLifetimeBetweenSessions';
const commit = 'a'.repeat(40);
const event = (Action, Test) => ({ Package: pkg, Action, ...(Test ? { Test } : {}) });
const records = (outcome = 'pass') => [event('start'), event('run', name), event(outcome, name), event(outcome)];
const jsonl = rows => rows.map(JSON.stringify).join('\n') + '\n';

test('complete native pass and failure remain distinct with original exit and identity', () => {
  assert.equal(createWGCObservation(jsonl(records()), 0, commit).state, 'passed');
  const failed = createWGCObservation(jsonl(records('fail')), 1, commit);
  assert.equal(failed.state, 'failed');
  assert.equal(failed.exitCode, 1);
  assert.equal(failed.commit, commit);
});

test('compile errors, skips, truncation, contradiction and wrong test never become completed observations', () => {
  for (const rows of [[], records().slice(0, -1), records().slice(0, 2), records('skip'),
    [{ Action: 'build-fail', ImportPath: pkg }],
    [...records(), event('pass')],
    [event('start'), event('run', name), event('fail', name), event('pass', name), event('pass')],
    records().map(row => row.Test ? { ...row, Test: 'TestOther' } : row),
    records().filter(row => row.Action !== 'run')]) {
    assert.throws(() => createWGCObservation(jsonl(rows), 1, commit));
  }
  assert.throws(() => createWGCObservation('{bad json}', 1, commit));
  assert.throws(() => createWGCObservation(jsonl(records()), 1, commit));
  assert.throws(() => createWGCObservation(jsonl(records('fail')), 0, commit));
  assert.throws(() => createWGCObservation(jsonl(records()), 0, 'unverified'));
});

test('CLI preserves failed capture as data but rejects malformed evidence without publishing a result', t => {
  const dir = mkdtempSync(join(tmpdir(), 'wgc-observation-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const previousSummary = process.env.GITHUB_STEP_SUMMARY;
  process.env.GITHUB_STEP_SUMMARY = join(dir, 'summary.md');
  t.after(() => {
    if (previousSummary === undefined) delete process.env.GITHUB_STEP_SUMMARY;
    else process.env.GITHUB_STEP_SUMMARY = previousSummary;
  });
  const warnings = [];
  t.mock.method(console, 'log', value => warnings.push(value));
  const events = join(dir, 'events.jsonl'), output = join(dir, 'result.json');
  writeFileSync(events, jsonl(records('fail')));
  assert.equal(main([events, '1', commit, output]).state, 'failed');
  assert.equal(JSON.parse(readFileSync(output, 'utf8')).exitCode, 1);
  assert.match(readFileSync(process.env.GITHUB_STEP_SUMMARY, 'utf8'), /FAILED, verification remains open/);
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /^::warning::/);
  writeFileSync(events, '{broken');
  const before = readFileSync(output, 'utf8');
  assert.throws(() => main([events, '1', commit, output]));
  assert.equal(readFileSync(output, 'utf8'), before);
});
