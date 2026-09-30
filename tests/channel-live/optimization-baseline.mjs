import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { options, runCommand } from '../../tools/baseline-common.mjs';

const fixtures = fileURLToPath(new URL('./fixtures/', import.meta.url));
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const connectionPattern = /^CON-[0-9a-f]{32}$/;

// Passive discovery is only target selection. Fresh binding proves the actor.
export function selectTarget(report, installation, pid) {
  assert.ok(Array.isArray(report?.candidates), 'missing passive candidate inventory');
  const matches = report.candidates.filter(c =>
    (!pid || c.window?.processId === Number(pid)) &&
    (!installation || path.resolve(c.client?.directory ?? '').toLowerCase() === path.resolve(installation).toLowerCase()));
  assert.equal(matches.length, 1, 'select exactly one client; multiple/absent candidates require a target');
  const c = matches[0];
  assert.equal(c.state, 'unobserved', 'target is occupied or unreadable; preserve its existing owner');
  assert.ok([120100, 50504, 38002].includes(c.client?.interface), 'target is outside the accepted client matrix');
  assert.ok(Number.isSafeInteger(c.window?.processId) && c.window.processId > 0);
  assert.match(c.window.processStartedAt ?? '', /^[1-9][0-9]*$/);
  assert.ok(path.isAbsolute(c.client.directory), 'installation must be absolute');
  return { installation: c.client.directory, pid: c.window.processId,
    processStartedAt: c.window.processStartedAt, executable: c.window.executable, client: c.client };
}

export function assessEnvelope(run, envelope) {
  assert.equal(envelope?.schema, 'lycheedev.result.v1', 'missing CLI result envelope');
  if (run.error || run.signal || run.code === null) throw new Error('CLI interrupted; retain the original request, input outcome unknown');
  assert.equal(run.code, 0, `CLI exit ${run.code}: ${JSON.stringify(envelope.error ?? envelope.result?.continuation)}`);
  assert.equal(envelope.ok, true, 'CLI did not complete successfully');
  return envelope.result;
}

export function requireMetrics(result, target) {
  const m = result?.observation;
  assert.equal(m?.scope, 'invocation', 'missing measured invocation scope');
  const t = m.total;
  if (target) {
    assert.equal(m.pid, target.pid, 'measurement changed process');
    assert.equal(m.processCreated, target.processStartedAt, 'measurement changed process creation identity');
  }
  for (const field of ['mappingQueries', 'rpmCalls', 'readCalls', 'requestedBytes', 'actualBytes', 'verifyCalls', 'regionCalls', 'candidates', 'learningCandidates', 'validated', 'rejected']) {
    assert.ok(Number.isSafeInteger(t?.[field]) && t[field] >= 0, `missing/invalid metric ${field}`);
  }
  assert.ok(t.readCalls > 0 && t.actualBytes > 0 && t.verifyCalls > 0 && t.rpmCalls > 0 && t.mappingQueries > 0, 'test did not observe real process bytes');
  assert.ok(t.actualBytes <= t.requestedBytes, 'actual bytes exceed requested bytes');
  assert.equal(t.budgetExhausted, false, 'physical read budget exhausted');
  // Nearby stops have their own diagnostic and can precede full fallback.
  assert.equal(t.cancelled, false, 'invocation cancelled');
  return m;
}

export function assertActor(result, identity) {
  assert.match(result?.session ?? '', connectionPattern);
  for (const key of ['guid', 'character', 'realm', 'build', 'product', 'release']) {
    assert.ok(identity?.[key], `missing original actor field ${key}`);
    assert.equal(result.identity?.[key], identity[key], `target changed: ${key}`);
  }
}

export function assertReport(result) {
  assert.equal(result.complete, true);
  assert.equal(result.reportState, 'verified');
  assert.equal(result.cleanup, 'complete');
  assert.equal(result.report?.ok, true, 'transport completion is not business success');
  assert.ok(result.operation, 'missing original operation');
}

// An explicit live runner, never imported by CI as an automatic game action.
// Tests inject the CLI adapter; production invocations always use public CLI.
export async function main(argv = process.argv.slice(2), { invoke = runCommand } = {}) {
  const opts = options(argv, ['--cli', '--project', '--installation', '--pid', '--home', '--help']);
  if (opts['--help']) {
    console.log('optimization-baseline.mjs --cli <candidate.exe> --project <new-directory> [--installation <client> --pid <pid>] [--home <workspace>]');
    return 0;
  }
  assert.ok(opts['--cli'] && opts['--project'], '--cli and fresh --project are required');
  assert.equal(Boolean(opts['--installation']), Boolean(opts['--pid']), 'installation and PID must be supplied together');
  if (opts['--pid']) assert.match(opts['--pid'], /^[1-9][0-9]*$/);
  const cli = path.resolve(opts['--cli']), project = path.resolve(opts['--project']);
  const cliHash = digest(await readFile(cli));
  await mkdir(path.dirname(project), { recursive: true });
  await mkdir(project); // Refuse existing evidence, including interrupted runs.
  const report = { schema: 'lycheedev.live-optimization-baseline.v1', startedAt: new Date().toISOString(),
    project, cli, cliSha256: cliHash, state: 'running', complete: false, steps: [],
    notRun: ['relogin', 'combat', 'other-clients', 'dual-instance', 'physical-IME', 'subjective-frame-experience'],
    scope: 'one fixed process/actor; functional and invocation I/O measurements, not full acceptance' };
  const reportPath = path.join(project, 'optimization.json');
  const save = () => writeFile(reportPath, JSON.stringify(report, null, 2) + '\n');
  await writeFile(reportPath, JSON.stringify(report, null, 2) + '\n', { flag: 'wx' });
  let session, identity;
  async function call(name, args, live = true) {
    assert.equal(digest(await readFile(cli)), cliHash, 'candidate CLI changed during the run');
    const argv = [...args, ...(live ? ['--project', project, '--wait-seconds', '120'] : []), '--format', 'json'];
    const intent = { command: cli, args: argv, session, startedAt: new Date().toISOString() };
    await writeFile(path.join(project, name + '.intent.json'), JSON.stringify(intent, null, 2), { flag: 'wx' });
    const run = await invoke(project, name, cli, argv, { timeout: 145000 });
    let envelope;
    try {
      assert.ok((await stat(run.stdout)).size <= 4 * 1024 * 1024, 'CLI result exceeds runner budget');
      envelope = JSON.parse((await readFile(run.stdout, 'utf8')).replace(/^\uFEFF/, ''));
    } finally {
      const r = envelope?.result;
      session ??= r?.session;
      report.session = session;
      report.steps.push({ name, run, session: r?.session, operation: r?.operation,
        complete: r?.complete, continuation: r?.continuation, observation: r?.observation });
      await save();
    }
    const result = assessEnvelope(run, envelope);
    if (live && session) assert.equal(result.session, session, 'connection changed');
    if (live && identity) assertActor(result, identity);
    return result;
  }
  async function execute(name, fixture, noCache = false, key = name) {
    const copied = path.join(project, fixture);
    assert.equal(digest(await readFile(copied)), report.fixtures[fixture], 'immutable probe bytes changed');
    const result = await call(name, ['live', 'execute', '--session', session, '--request', key,
      '--file', copied, '--budget-seconds', '10', '--policy', 'observation', ...(noCache ? ['--no-cache'] : [])]);
    assertReport(result);
    return result;
  }
  try {
    for (const fixture of ['normal.lua', 'payload.lua']) {
      const bytes = await readFile(path.join(fixtures, fixture));
      await writeFile(path.join(project, fixture), bytes, { flag: 'wx' });
      (report.fixtures ??= {})[fixture] = digest(bytes);
    }
    const inventory = await call('passive-inventory', ['live', 'instances', '--passive',
      ...(opts['--home'] ? ['--home', path.resolve(opts['--home'])] : [])], false);
    report.target = selectTarget(inventory, opts['--installation'], opts['--pid']);
    await save();
    const target = report.target;
    const status = await call('managed-status', ['addon', 'status', '--installation', target.installation], false);
    assert.equal(status.installation?.state, 'managed', 'prepare a clean managed candidate before running');
    assert.equal(status.slots?.state, 'managed');
    assert.equal(status.slots.count, 200);
    assert.equal(status.slots.pending, 0, 'existing slot work must be resolved first');
    report.installationReceipt = status.installation.receipt;
    const connected = await call('connect-cache-off', ['live', 'connect', '--installation', target.installation,
      '--pid', String(target.pid), '--no-cache']);
    assert.equal(connected.bound, true);
    assert.equal(connected.complete, true);
    assert.equal(connected.identity.slots, 200);
    identity = connected.identity;
    assertActor(connected, identity);
    requireMetrics(connected, target);
    report.identity = identity;
    let first;
    for (const noCache of [false, true]) {
      for (let n = 1; n <= 5; n++) {
        const name = `normal-${noCache ? 'cache-off' : 'warm'}-${n}`;
        const result = await execute(name, 'normal.lua', noCache);
        assert.equal(result.report.result?.marker, 'native-slot-baseline');
        requireMetrics(result, target);
        first ??= result;
      }
    }
    for (const noCache of [false, true]) {
      const result = await execute(`payload-${noCache ? 'cache-off' : 'warm'}`, 'payload.lua', noCache);
      assert.equal(result.report.result?.marker, 'native-large-payload');
      assert.equal(result.report.result.text, '荔枝<&>\n'.repeat(8192));
      requireMetrics(result, target);
    }
    const journal = path.join(project, '.lycheedev', 'live', 'connections', session + '.jsonl');
    const before = await readFile(journal);
    const repeated = await execute('historical-readonly', 'normal.lua', false, 'normal-warm-1');
    assert.equal(repeated.operation, first.operation);
    assert.deepEqual(repeated.report, first.report);
    assert.deepEqual(await readFile(journal), before, 'historical request caused a new journal event');
    const reloaded = await call('reload', ['live', 'reload', '--session', session, '--request', 'optimization-reload']);
    assert.equal(reloaded.complete, true);
    assert.notEqual(reloaded.identity.runtime, identity.runtime, 'reload has no verified runtime transition');
    const afterReload = await execute('normal-after-reload', 'normal.lua');
    assert.equal(afterReload.report.result?.marker, 'native-slot-baseline');
    requireMetrics(afterReload, target);
    const closed = await call('disconnect', ['live', 'disconnect', session]);
    assert.equal(closed.closed, true);
    const closedJournal = await readFile(journal);
    assert.equal((await call('disconnect-readonly', ['live', 'disconnect', session])).closed, true);
    assert.deepEqual(await readFile(journal), closedJournal);
    report.state = 'passed';
    report.complete = true;
  } catch (error) {
    report.state = 'blocked';
    report.failure = { message: error.message, session, project,
      recovery: session ? `live status ${session} --project "${project}" --format json` : 'inspect retained command intents before any new connection' };
  } finally {
    report.finishedAt = new Date().toISOString();
    await save();
  }
  console.log(JSON.stringify({ report: reportPath, state: report.state, session }));
  return report.complete ? 0 : 2;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().then(code => { process.exitCode = code; }).catch(error => { console.error(error.message); process.exitCode = 2; });
}
