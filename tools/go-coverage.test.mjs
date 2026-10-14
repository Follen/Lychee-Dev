import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createCoverageReport, parseCoverProfile } from './go-coverage.mjs';

const root = 'example.com/project';
const one = `${root}/internal/one`, two = `${root}/cmd/two`, helper = `${root}/internal/testkit`;
const profile = `mode: atomic\n${one}/a.go:1.1,1.2 9 0\n${one}/a.go:2.1,2.2 1 1\n${two}/b.go:1.1,1.2 1 1\n`;
const identity = () => ({ testedCommit: 'a'.repeat(40), testedTree: 'b'.repeat(40), requestedPrHead: 'a'.repeat(40),
  goVersion: 'go version go1.27.1 windows/amd64', goos: 'windows', goarch: 'amd64',
  command: ['go', 'test', '-json', '-covermode=atomic', `-coverpkg=${one},${two}`, '-coverprofile=cover.out', '-p=2', '-parallel=4', '-count=1', '-timeout=20m', './...'],
  exitCode: 0, packages: [{ importPath: one, hasTests: true, instrumented: true },
    { importPath: two, hasTests: false, instrumented: true }, { importPath: helper, hasTests: false, instrumented: false, helper: true }] });
const event = (Package, Action, Test) => ({ Package, Action, ...(Test ? { Test } : {}) });
const sampleEvents = () => [event(one, 'start'), event(one, 'run', 'TestExample'), event(one, 'pass', 'TestExample'), event(one, 'pass'),
  event(two, 'start'), event(two, 'pass'), event(helper, 'start'), event(helper, 'skip')];
const jsonl = events => `${events.map(e => JSON.stringify(e)).join('\n')}\n`;
const report = (overrides = {}) => createCoverageReport({ profile, events: jsonl(sampleEvents()), identity: identity(), ...overrides });

test('coverage is statement-weighted per package and total, not an average of package percentages', () => {
  const result = parseCoverProfile(profile);
  assert.equal(result.statements, 11);
  assert.equal(result.coveredStatements, 2);
  assert.equal(result.percent, 18.1818);
  assert.deepEqual(result.packages.map(p => [p.importPath, p.percent]), [[two, 100], [one, 10]]);
  const zero = parseCoverProfile(`mode: atomic\n${one}/a.go:1.1,1.2 0 0\n`);
  assert.equal(zero.percent, null);
});

test('identical source blocks merge execution evidence without double-counting statements', () => {
  for (const mode of ['set', 'count', 'atomic']) {
    const result = parseCoverProfile(`mode: ${mode}\n${one}/a.go:1.1,1.2 4 0\n${one}/a.go:1.1,1.2 4 1\n${one}/a.go:1.1,1.2 4 1\n`);
    assert.equal(result.statements, 4);
    assert.equal(result.coveredStatements, 4);
    assert.equal(result.blocks, 1);
    assert.equal(result.duplicateBlocks, 2);
  }
  assert.equal(parseCoverProfile(`mode: atomic\n${one}/a.go:1.1,1.2 1 9223372036854775807\n`).percent, 100);
});

test('malformed profiles, impossible counts and inconsistent duplicate statement counts are rejected', () => {
  for (const input of ['', 'mode: bogus\n', 'mode: atomic\nnot a block\n',
    `mode: atomic\n${one}/a.go:1.1,1.2 2 0\n${one}/a.go:1.1,1.2 3 1\n`,
    `mode: atomic\n${one}/a.go:1.1,1.2 -1 0\n`,
    `mode: atomic\n${one}/a.go:1.1,1.2 1 -1\n`,
    `mode: set\n${one}/a.go:1.1,1.2 1 2\n`,
    `mode: atomic\n${one}/a.go:0.1,1.2 1 1\n`,
    `mode: atomic\n${one}/a.go:2.1,1.2 1 1\n`,
    `mode: atomic\n${one}/a.go:1.1,1.2 9007199254740992 1\n`,
    `mode: atomic\n${one}/a.go:1.1,1.2 1 9223372036854775808\n`]) {
    assert.throws(() => parseCoverProfile(input), /invalid|mismatched/);
  }
  assert.throws(() => parseCoverProfile(`mode: atomic\n${one}/a.go:1.1,1.2 9007199254740991 1\n${one}/a.go:2.1,2.2 1 0\n`), /safe integer/);
});

test('execution identity, no-test packages and helpers remain explicit in a passing run', () => {
  const result = report();
  assert.equal(result.state, 'passed');
  assert.equal(result.tests.passed, 1);
  assert.equal(result.tests.skipped, 0);
  assert.equal(result.metadata.testedCommit, identity().testedCommit);
  assert.equal(result.metadata.testedTree, identity().testedTree);
  assert.deepEqual(result.metadata.command, identity().command);
  assert.equal(result.metadata.testedRequestedHead, true);
  assert.deepEqual(result.tests.packages.map(p => [p.hasTests, p.helper, p.result]), [[true, false, 'pass'], [false, false, 'pass'], [false, true, 'skip']]);
  assert.match(result.inputs.profileSha256, /^[0-9a-f]{64}$/);
  const other = identity(); other.requestedPrHead = 'c'.repeat(40);
  assert.equal(report({ identity: other }).metadata.testedRequestedHead, false);
});

test('skipped tests and subtests are named and remain unrun without adding a CI gate', () => {
  const events = sampleEvents();
  events.splice(2, 0, event(one, 'run', 'TestExample/optional'), event(one, 'skip', 'TestExample/optional'));
  const result = report({ events: jsonl(events) });
  assert.equal(result.state, 'passed_with_skips');
  assert.equal(result.tests.passed, 1);
  assert.equal(result.tests.skipped, 1);
  assert.deepEqual(result.tests.skippedTests, [{ package: one, test: 'TestExample/optional' }]);
  const selected = identity(); selected.packages[2].hasTests = true;
  assert.equal(report({ identity: selected }).state, 'passed_with_skips');
});

test('test, package, build and process failures cannot become success when coverage exists', () => {
  const events = sampleEvents(); events[2].Action = 'fail'; events[3].Action = 'fail';
  const result = report({ events: jsonl(events) });
  assert.equal(result.state, 'failed');
  assert.equal(result.coverage.percent, 18.1818);
  assert.deepEqual(result.tests.failedTests, [{ package: one, test: 'TestExample' }]);
  const processFailure = identity(); processFailure.exitCode = 1;
  assert.equal(report({ identity: processFailure }).state, 'failed');
  const buildEvents = [{ ImportPath: `${one} [${one}.test]`, Action: 'build-output', Output: 'compile error\n' },
    { ImportPath: `${one} [${one}.test]`, Action: 'build-fail' }, ...sampleEvents()];
  assert.equal(report({ events: jsonl(buildEvents) }).state, 'failed');
});

test('truncated events, missing package results/profiles and unknown process exit stay incomplete', () => {
  assert.equal(report({ events: jsonl(sampleEvents().slice(0, -1)) }).state, 'incomplete');
  assert.equal(report({ events: jsonl(sampleEvents().slice(0, 2)) }).tests.incomplete, 1);
  assert.equal(report({ events: null }).state, 'incomplete');
  assert.equal(report({ profile: null }).state, 'incomplete');
  const stopped = identity(); stopped.exitCode = null;
  assert.equal(report({ identity: stopped }).state, 'incomplete');
});

test('a selected package with no profile blocks is explicit, without inventing zero coverage or a gate', () => {
  const missing = report({ profile: `mode: atomic\n${one}/a.go:1.1,1.2 1 1\n` });
  assert.equal(missing.state, 'passed');
  assert.deepEqual(missing.coverage.packagesWithoutBlocks, [two]);
  assert.equal(missing.coverage.packages.some(p => p.importPath === two), false);
  assert.equal(report({ profile: 'mode: atomic\n' }).coverage.percent, null);
});

test('bad JSON, contradictory events and out-of-scope coverage cannot silently produce success', () => {
  const invalid = report({ events: `${jsonl(sampleEvents())}{bad json}\n` });
  assert.equal(invalid.state, 'invalid');
  assert.equal(invalid.tests.passed, 1);
  assert.equal(report({ profile: 'mode: atomic\ninvalid\n' }).state, 'invalid');
  assert.equal(report({ profile: `${profile}${helper}/helper.go:1.1,1.2 1 1\n` }).state, 'invalid');
  assert.equal(report({ events: jsonl([...sampleEvents(), event(one, 'pass')]) }).state, 'invalid');
  assert.equal(report({ events: jsonl(sampleEvents().filter((_, i) => i !== 1)) }).state, 'invalid');
  const wrong = identity(); wrong.packages.push({ ...wrong.packages[0] });
  assert.throws(() => report({ identity: wrong }), /duplicate identity package/);
  const short = identity(); short.testedCommit = 'a1b2c3d';
  assert.throws(() => report({ identity: short }), /full Git object ID/);
});

test('CLI emits the same JSON to stdout and artifact, and keeps failed evidence with a failing exit', t => {
  const directory = mkdtempSync(join(tmpdir(), 'lycheedev-go-coverage-'));
  t.after(() => rmSync(directory, { recursive: true }));
  const paths = Object.fromEntries(['profile', 'events', 'identity', 'out'].map(key => [key, join(directory, `${key}.txt`)]));
  writeFileSync(paths.profile, profile); writeFileSync(paths.events, jsonl(sampleEvents()));
  const args = Object.entries(paths).flatMap(([key, value]) => [`--${key}`, value]);
  const tool = fileURLToPath(new URL('./go-coverage.mjs', import.meta.url));
  for (const exitCode of [0, 1]) {
    writeFileSync(paths.identity, JSON.stringify({ ...identity(), exitCode }));
    const run = spawnSync(process.execPath, [tool, ...args], { encoding: 'utf8', windowsHide: true });
    assert.equal(run.status, exitCode, run.stderr);
    const file = JSON.parse(readFileSync(paths.out, 'utf8'));
    assert.deepEqual(JSON.parse(run.stdout), file);
    assert.equal(file.metadata.exitCode, exitCode);
    assert.equal(file.state, exitCode ? 'failed' : 'passed');
  }
});
