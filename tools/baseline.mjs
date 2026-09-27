// Offline functional baseline. Full Go/Lua coverage plus distribution and skill
// contracts; no game input. Windows amd64 is the supported product platform.
import { readFileSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { offlineCases, liveCases, manualCases } from '../tests/baseline/catalog.mjs';
import { createReport, goCaseResult, options, parseGoEvents, parseTAP, repository, runCommand, saveReport, sourceIdentity } from './baseline-common.mjs';

export async function main(argv = process.argv.slice(2)) {
  const opts = options(argv, ['--out', '--lua51', '--luals-archive', '--luals-runtime', '--help']);
  if (opts['--help']) {
    console.log('node tools/baseline.mjs [--out <new-directory>] [--lua51 <lua.exe>] [--luals-archive <pinned.zip>] [--luals-runtime <runtime-directory>]');
    return 0;
  }
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('baseline requires Windows amd64');
  const env = { ...process.env, LYCHEEDEV_REQUIRE_LUA51: '1' };
  for (const [flag, key] of [['--lua51', 'LYCHEEDEV_LUA51'], ['--luals-archive', 'LYCHEEDEV_LUALS_ARCHIVE'], ['--luals-runtime', 'LYCHEEDEV_LUALS_TEST_RUNTIME']]) {
    if (opts[flag]) env[key] = resolve(opts[flag]);
  }
  const report = createReport('offline', opts['--out'], [...offlineCases, ...liveCases, ...manualCases]);
  saveReport(report);
  console.log(`Baseline evidence: ${report.output}`);
  async function check(id, title, command, args, inspect) {
    const entry = { id, title, state: 'running' }; report.checks.push(entry); saveReport(report);
    console.log(`RUN   ${id} ${title}`);
    try {
      const run = await runCommand(report.output, id, command, args, { env });
      Object.assign(entry, { run, state: run.code === 0 && !run.error && !run.signal ? 'passed' : 'failed' });
      if (inspect) inspect(entry, readFileSync(run.stdout, 'utf8'));
      if (run.code !== 0 || run.error || run.signal) entry.state = 'failed';
    } catch (error) { Object.assign(entry, { state: 'failed', reason: error.message }); }
    saveReport(report); console.log(`${entry.state.toUpperCase().padEnd(7)} ${id}`);
  }
  await check('BUILD', 'Build all packages', 'go', ['build', './...']);
  await check('VET', 'Vet all packages', 'go', ['vet', './...']);
  await check('GO', 'All Go tests and mandatory Lua 5.1 suites', 'go', ['test', '-json', '-parallel=4', '-count=1', '-timeout=10m', './...'], (entry, text) => {
    const evidence = parseGoEvents(text);
    entry.tests = evidence.tests.size;
    entry.packages = evidence.packages.size;
    entry.skipped = [...evidence.tests].filter(([, state]) => state === 'skip').map(([name]) => name);
    entry.failed = [...evidence.tests].filter(([, state]) => state === 'fail').map(([name]) => name);
    report.cases = report.cases.map(c => c.kind === 'go' ? goCaseResult(c, evidence) : c);
    report.cases.push(goCaseResult({ id: 'LUALS', title: 'Real LuaLS LSP integration', package: 'internal/codebase', tests: ['TestRealLuaLSMapsAPIReferencesAndDefinitions'] }, evidence));
    if (entry.failed.length || [...evidence.packages.values()].includes('fail')) entry.state = 'failed';
  });
  const nodeFiles = [
    ...readdirSync(join(repository, 'tools')).filter(f => f.endsWith('.test.mjs')).map(f => `tools/${f}`),
    ...readdirSync(join(repository, 'packages/npm/lycheedev/test')).filter(f => f.endsWith('.test.mjs')).map(f => `packages/npm/lycheedev/test/${f}`),
  ].sort();
  await check('NODE', 'Distribution, launcher and regression runner tests', process.execPath, ['--test', '--test-reporter=tap', ...nodeFiles], (entry, text) => Object.assign(entry, parseTAP(text)));
  await check('VERSION', 'Release version consistency', process.execPath, ['tools/version.mjs', '--check']);
  await check('SKILL', 'Skill and CLI command contract consistency', process.execPath, ['tools/skill-contract.mjs']);
  const after = sourceIdentity(report.output);
  report.checks.push({ id: 'SOURCE', title: 'Source unchanged during the run', state: after.treeSha256 === report.source.treeSha256 ? 'passed' : 'failed', after });
  const required = [...report.checks, ...report.cases.filter(c => c.kind === 'go' || c.id === 'LUALS')];
  report.state = required.some(c => c.state === 'failed') ? 'failed' : required.every(c => c.state === 'passed') ? 'passed' : 'blocked';
  report.finishedAt = new Date().toISOString(); saveReport(report);
  console.log(`Offline baseline: ${report.state}. Report: ${join(report.output, 'report.json')}`);
  return report.state === 'passed' ? 0 : report.state === 'blocked' ? 2 : 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().then(code => { process.exitCode = code; }).catch(error => { console.error(error.message); process.exitCode = 1; });
}
