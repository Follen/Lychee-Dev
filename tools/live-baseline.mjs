// Real-client baseline. Uses the production complete execute API; never sends
// keys, edits an installation, abandons unknown work or selects another actor.
import { randomUUID } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { liveCases, manualCases } from '../tests/baseline/catalog.mjs';
import { assessLive, createReport, options, repository, runCommand, sameLiveEvidence, saveReport, sha256 } from './baseline-common.mjs';

export async function runLive(report, opts, run = runCommand) {
  const cli = opts['--cli'] ?? process.env.LYCHEEDEV_CLI ?? 'lycheedev';
  const session = opts['--session'];
  const common = [...(opts['--home'] ? ['--home', resolve(opts['--home'])] : []), '--format', 'json'];
  report.target = { session, home: opts['--home'] ? resolve(opts['--home']) : 'CLI default' };
  report.runId = randomUUID();
  mkdirSync(join(report.output, 'probes'));
  for (const spec of report.cases.filter(c => c.file)) {
    const path = join(report.output, 'probes', spec.file);
    copyFileSync(join(repository, 'tests/baseline/probes', spec.file), path);
    spec.request = `baseline-${report.runId}-${spec.id}`;
    spec.sha256 = sha256(readFileSync(path));
    spec.args = ['live', 'execute', '--session', session, '--file', path, '--request', spec.request, '--budget-seconds', String(spec.budget),
      ...(opts['--account'] ? ['--account', opts['--account']] : []), ...common];
  }
  saveReport(report);
  let first = null;
  async function invoke(id, args, spec) {
    const processResult = await run(report.output, id, cli, args, { timeout: 600_000 });
    let envelope;
    try { envelope = JSON.parse(readFileSync(processResult.stdout, 'utf8')); } catch { /* classifier fails closed */ }
    return { ...assessLive(processResult, envelope, spec), run: processResult, envelope,
      operationId: envelope?.operationId, nextAction: envelope?.result?.nextAction ?? envelope?.error?.details?.nextAction };
  }
  for (const spec of report.cases.filter(c => c.id.startsWith('LIVE-'))) {
    console.log(`RUN   ${spec.id} ${spec.title}`);
    spec.state = 'running'; saveReport(report);
    try {
      if (spec.id === 'LIVE-02') {
        const original = report.cases.find(c => c.id === 'LIVE-01');
        spec.attempts = [];
        for (const [index, args] of [original.args, ['live', 'resume', first.operationId, ...common]].entries()) {
          const result = await invoke(`${spec.id}-${index + 1}`, args, original);
          if (result.state === 'passed' && !sameLiveEvidence(first, result.envelope)) Object.assign(result, { state: 'failed', reason: 'repeated operation changed identity or evidence' });
          spec.attempts.push(result); saveReport(report);
          if (result.state !== 'passed') break;
        }
        spec.state = spec.attempts.find(a => a.state !== 'passed')?.state ?? 'passed';
      } else {
        Object.assign(spec, await invoke(spec.id, spec.args, spec));
        if (spec.id === 'LIVE-01') first = spec.envelope;
      }
    } catch (error) { Object.assign(spec, { state: 'failed', reason: error.message }); }
    saveReport(report); console.log(`${spec.state.toUpperCase().padEnd(7)} ${spec.id}`);
    if (spec.state !== 'passed') break;
  }
  const selected = report.cases.filter(c => c.id.startsWith('LIVE-'));
  report.state = selected.some(c => c.state === 'failed') ? 'failed' : selected.every(c => c.state === 'passed') ? 'passed' : 'blocked';
  report.finishedAt = new Date().toISOString(); saveReport(report);
  return report.state === 'passed' ? 0 : report.state === 'blocked' ? 2 : 1;
}

export async function main(argv = process.argv.slice(2)) {
  const opts = options(argv, ['--cli', '--session', '--account', '--home', '--out', '--help']);
  if (opts['--help']) {
    console.log('node tools/live-baseline.mjs --session <session-id> [--cli <lycheedev.exe>] [--account <account>] [--home <workspace>] [--out <new-directory>]');
    return 0;
  }
  if (!opts['--session']) throw new Error('provide --session from live connect; this suite retains that target');
  if (process.platform !== 'win32' || process.arch !== 'x64') throw new Error('live baseline requires Windows amd64');
  const report = createReport('live', opts['--out'], [...liveCases, ...manualCases]);
  saveReport(report); console.log(`Baseline evidence: ${report.output}`);
  const cli = opts['--cli'] ?? process.env.LYCHEEDEV_CLI ?? 'lycheedev';
  for (const [id, args] of [['CLI', ['version', '--format', 'json']], ['INSTANCES', ['live', 'instances', '--passive', '--format', 'json']]]) {
    const check = { id, title: id === 'CLI' ? 'CLI identity' : 'Passive client inventory', state: 'running' };
    report.checks.push(check); saveReport(report);
    try {
      check.run = await runCommand(report.output, id, cli, args);
      check.envelope = JSON.parse(readFileSync(check.run.stdout, 'utf8'));
      if (check.run.code !== 0 || check.envelope.schema !== 'lycheedev.result.v1' || check.envelope.ok !== true) throw new Error('preflight did not return a successful CLI result');
      check.state = 'passed';
    } catch (error) {
      check.state = 'blocked'; check.reason = error.message;
      report.state = 'blocked'; report.finishedAt = new Date().toISOString(); saveReport(report);
      return 2;
    }
    saveReport(report);
  }
  const code = await runLive(report, opts);
  console.log(`Live baseline: ${report.state}. Report: ${join(report.output, 'report.json')}`);
  return code;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().then(code => { process.exitCode = code; }).catch(error => { console.error(error.message); process.exitCode = 1; });
}
