import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { closeSync, mkdirSync, openSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const modulePath = 'github.com/follenfang/lycheedev/';
export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export function options(argv, allowed) {
  const result = {};
  for (let i = 0; i < argv.length; i++) {
    const key = argv[i];
    if (!allowed.includes(key) || key in result) throw new Error(`unknown or duplicate option: ${key}`);
    if (key === '--help') { result[key] = true; continue; }
    const value = argv[++i];
    if (!value || value.startsWith('--')) throw new Error(`missing value: ${key}`);
    result[key] = value;
  }
  return result;
}

export function sourceIdentity(output) {
  const git = args => {
    const run = spawnSync('git', args, { cwd: repository, encoding: 'utf8', windowsHide: true, maxBuffer: 32 * 1024 * 1024 });
    if (run.status !== 0) throw new Error(`git identity unavailable: ${run.stderr || run.error}`);
    return run.stdout;
  };
  const digest = createHash('sha256');
  const files = [...new Set(git(['ls-files', '-z', '--cached', '--others', '--exclude-standard']).split('\0').filter(Boolean))].sort();
  for (const file of files) {
    const path = resolve(repository, file);
    const rel = relative(output, path);
    if (!rel || (!rel.startsWith('..') && !isAbsolute(rel))) continue;
    let bytes;
    try { bytes = readFileSync(path); } catch (error) { if (error.code !== 'ENOENT') throw error; }
    digest.update(`${file}\0${bytes ? sha256(bytes) : 'deleted'}\n`);
  }
  return { commit: git(['rev-parse', 'HEAD']).trim(), dirty: git(['status', '--porcelain']).length > 0,
    treeSha256: digest.digest('hex'), version: JSON.parse(readFileSync(join(repository, 'release/version.json'), 'utf8')).version };
}

export function createReport(mode, out, cases) {
  const output = resolve(out ?? join(repository, '.tmp', `baseline-${mode}-${Date.now()}`));
  mkdirSync(dirname(output), { recursive: true });
  // Never overwrite a previous run, including one interrupted during input.
  mkdirSync(output);
  return { schema: 'lycheedev.baseline.v1', mode, output, startedAt: new Date().toISOString(),
    source: sourceIdentity(output), state: 'running', checks: [], cases: cases.map(c => ({ ...c, state: 'not_run' })) };
}

export function saveReport(report) {
  const path = join(report.output, 'report.json');
  writeFileSync(`${path}.tmp`, `${JSON.stringify(report, null, 2)}\n`);
  renameSync(`${path}.tmp`, path);
  const rows = [...report.checks, ...report.cases].map(c => `| ${c.id} | ${c.state} | ${(c.title ?? '').replaceAll('|', '/')} |`);
  writeFileSync(join(report.output, 'summary.md'), `# ${report.mode} baseline\n\nState: **${report.state}**. Source: ${report.source.commit} (dirty: ${report.source.dirty}).\n\nOnly this run's selected scope is evaluated; manual and other-client acceptance remain separate.\n\n| ID | State | Check |\n| --- | --- | --- |\n${rows.join('\n')}\n`);
}

export async function runCommand(output, id, command, args, { env = process.env, timeout = 1_200_000 } = {}) {
  const stdoutPath = join(output, `${id}.stdout`), stderrPath = join(output, `${id}.stderr`);
  const invocation = { command, args, startedAt: new Date().toISOString(), timeout };
  // This intent remains inspectable even if the runner or CLI dies mid-input.
  writeFileSync(join(output, `${id}.command.json`), `${JSON.stringify(invocation, null, 2)}\n`);
  const stdout = openSync(stdoutPath, 'wx'), stderr = openSync(stderrPath, 'wx');
  const started = Date.now();
  let outcome;
  try {
    outcome = await new Promise(resolveRun => {
      const child = spawn(command, args, { cwd: repository, env, windowsHide: true, shell: false,
        stdio: ['ignore', stdout, stderr], timeout });
      let error = null;
      child.once('error', value => { error = value.message; });
      child.once('close', (code, signal) => resolveRun({ code, signal, error }));
    });
  } finally { closeSync(stdout); closeSync(stderr); }
  const result = { ...invocation, ...outcome, milliseconds: Date.now() - started, stdout: stdoutPath, stderr: stderrPath };
  writeFileSync(join(output, `${id}.process.json`), `${JSON.stringify(result, null, 2)}\n`);
  return result;
}

export function parseGoEvents(text) {
  const tests = new Map(), packages = new Map();
  let events = 0;
  for (const line of text.split(/\r?\n/).filter(Boolean)) {
    const event = JSON.parse(line); events++;
    if (!event.Package || !event.Action) throw new Error('invalid go test JSON event');
    if (['pass', 'fail', 'skip'].includes(event.Action)) {
      if (event.Test) tests.set(`${event.Package}:${event.Test}`, event.Action);
      else packages.set(event.Package, event.Action);
    }
  }
  if (!events || !tests.size || !packages.size) throw new Error('go test executed no completed tests');
  return { tests, packages };
}

export function goCaseResult(spec, evidence) {
  const findings = [];
  for (const test of spec.tests) {
    const key = `${modulePath}${spec.package}:${test}`;
    const result = evidence.tests.get(key);
    if (result !== 'pass') findings.push(`${test}: ${result ?? 'missing'}`);
    for (const [name, state] of evidence.tests) {
      if (name.startsWith(`${key}/`) && state !== 'pass') findings.push(`${name}: ${state}`);
    }
  }
  return { ...spec, state: findings.some(f => f.endsWith(': fail') || f.endsWith(': missing')) ? 'failed' : findings.length ? 'blocked' : 'passed', findings };
}

export function parseTAP(text) {
  const count = name => Number([...text.matchAll(new RegExp(`^# ${name} (\\d+)\\s*$`, 'gm'))].at(-1)?.[1] ?? NaN);
  const counts = Object.fromEntries(['tests', 'pass', 'fail', 'cancelled', 'skipped', 'todo'].map(k => [k, count(k)]));
  if (!Number.isFinite(counts.tests) || counts.tests < 1 || Object.values(counts).some(v => !Number.isFinite(v))) throw new Error('Node test summary missing or empty');
  return { ...counts, state: counts.fail || counts.cancelled ? 'failed' : counts.skipped || counts.todo ? 'blocked' : 'passed' };
}

export function assessLive(run, envelope, spec) {
  const r = envelope?.result;
  if (run.error || run.signal || run.code === null) return { state: 'blocked', reason: 'process interrupted; input outcome unknown' };
  if (envelope?.schema !== 'lycheedev.result.v1') return { state: 'failed', reason: 'missing result envelope' };
  if (run.code === 6) return { state: 'blocked', reason: 'pending; recover the recorded operation before another case' };
  if (run.code === 3) return { state: 'blocked', reason: 'client or environment capability unavailable' };
  const failures = [];
  const expect = (value, message) => { if (!value) failures.push(message); };
  expect(run.code === (spec.expectedFailure ? 5 : 0), `unexpected exit ${run.code}`);
  expect(envelope.ok === !spec.expectedFailure, 'unexpected envelope.ok');
  expect(/^OP-/.test(envelope.operationId ?? '') && r?.operationId === envelope.operationId, 'operation identity missing or mismatched');
  expect(r?.goal === 'finished' && r?.complete === true && r?.cleanup === 'complete', 'incomplete cleanup');
  expect(r?.report?.state === 'verified' && r.report.bodyCapture && r.report.receiptCapture && r.report.sha256, 'missing verified report evidence');
  expect(r?.display?.state === 'cleared' && r.display.capture, 'missing display clear proof');
  expect(r?.business?.state === (spec.expectedFailure ? 'failed' : 'passed'), 'unexpected business outcome');
  const content = r?.report?.content;
  if (spec.expectedFailure) expect(JSON.stringify(content ?? {}).includes('BASELINE_EXPECTED_FAILURE'), 'expected failure marker absent');
  else {
    expect(content?.result?.passed === true && content.result.baseline === spec.id, 'wrong baseline result');
    expect(content?.acceptedBudgetSeconds === spec.budget, 'budget changed');
    if (spec.id === 'LIVE-01') expect(content?.result?.sum === 55, 'incorrect sum');
    if (spec.id === 'LIVE-04') {
      expect(JSON.stringify(content?.result?.pages) === JSON.stringify(['runner', 'objects', 'events', 'trace', 'diagnostics', 'exports', 'automation', 'about', 'settings']), 'page coverage incomplete');
      expect(content?.result?.unobstructed === true && content.result.scope === 'character-v1', 'overlay or report scope regression');
    }
  }
  return { state: failures.length ? 'failed' : 'passed', findings: failures };
}

export function sameLiveEvidence(first, repeated) {
  const identity = e => [e?.operationId, e?.result?.report?.bodyCapture, e?.result?.report?.receiptCapture,
    e?.result?.report?.sha256, e?.result?.display?.capture];
  return identity(first).every(Boolean) && JSON.stringify(identity(first)) === JSON.stringify(identity(repeated));
}
