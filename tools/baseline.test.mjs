import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { offlineCases, liveCases, manualCases } from '../tests/baseline/catalog.mjs';
import { goCaseResult, modulePath, options, parseGoEvents, parseTAP, repository, runCommand } from './baseline-common.mjs';

function fixture(t) {
  const output = mkdtempSync(join(tmpdir(), 'lycheedev-baseline-'));
  t.after(() => rmSync(output, { recursive: true, force: true }));
  return { schema: 'lycheedev.baseline.v1', mode: 'offline', output, source: { commit: 'fixture', dirty: false },
    checks: [], cases: [...liveCases, ...manualCases].map(c => ({ ...c, state: 'not_run' })) };
}
test('catalog IDs and required test names resolve to real Go tests', () => {
  const all = [...offlineCases, ...liveCases, ...manualCases];
  assert.equal(new Set(all.map(c => c.id)).size, all.length);
  for (const spec of offlineCases) {
    const dir = join(repository, spec.package);
    const source = readdirSync(dir).filter(f => f.endsWith('_test.go')).map(f => readFileSync(join(dir, f), 'utf8')).join('\n');
    for (const name of spec.tests) {
      const [entry, suite, profile] = name.split('/');
      assert.match(source, new RegExp(`^func ${entry}\\(t \\*testing\\.T\\)`, 'm'), `${spec.id}: ${name}`);
      if (suite) {
        assert(spec.luaSuites?.includes(suite));
        assert(spec.profiles?.includes(profile));
        assert(readdirSync(dir).includes(suite), `${spec.id}: missing ${suite}`);
        assert(source.includes(`"${profile}"`), `${spec.id}: undiscovered ${profile}`);
      }
    }
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
test('process transport preserves argument boundaries and nonzero exit with valid stdout', async t => {
  const { output } = fixture(t);
  const result = await runCommand(output, 'process', process.execPath,
    ['-e', 'console.log(JSON.stringify(process.argv.slice(1))); process.exit(5)', 'path with spaces', 'literal; $()']);
  assert.equal(result.code, 5);
  assert.deepEqual(JSON.parse(readFileSync(result.stdout)), ['path with spaces', 'literal; $()']);
  assert.ok(readFileSync(join(output, 'process.command.json'), 'utf8').includes('startedAt'));
});
