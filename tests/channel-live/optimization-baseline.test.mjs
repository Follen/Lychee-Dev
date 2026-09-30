import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { assessEnvelope, assertActor, assertReport, main, requireMetrics, selectTarget } from './optimization-baseline.mjs';

const session = 'CON-' + 'a'.repeat(32);
const identity = { guid: 'Player-1-1', character: 'Fixture', realm: 'Test', build: '120100.69933',
  product: 'retail', release: '3.0.2', runtime: 'r1', slots: 200 };
const candidate = root => ({ state: 'unobserved', client: { directory: path.join(root, 'game'), interface: 120100 },
  window: { processId: 42, processStartedAt: '134037001234567890', executable: path.join(root, 'game', 'Wow.exe') } });
const metrics = () => ({ scope: 'invocation', pid: 42, processCreated: '134037001234567890', total: {
  mappingQueries: 6, rpmCalls: 5, readCalls: 5, requestedBytes: 1024, actualBytes: 1000, verifyCalls: 1, regionCalls: 1,
  candidates: 2, learningCandidates: 1, validated: 1, rejected: 1, budgetExhausted: false, cancelled: false } });
const envelope = result => ({ schema: 'lycheedev.result.v1', ok: true, result });

test('passive selection refuses ambiguity, occupied/unreadable targets and excluded builds', () => {
  const c = candidate(os.tmpdir());
  assert.equal(selectTarget({ candidates: [c] }).pid, 42);
  assert.throws(() => selectTarget({ candidates: [c, c] }), /exactly one/);
  assert.throws(() => selectTarget({ candidates: [] }), /exactly one/);
  for (const state of ['busy', 'unreadable']) assert.throws(() => selectTarget({ candidates: [{ ...c, state }] }));
  assert.throws(() => selectTarget({ candidates: [{ ...c, client: { ...c.client, interface: 16001 } }] }));
  assert.throws(() => selectTarget({ candidates: [c] }, c.client.directory, '99'));
  const other = { ...c, window: { ...c.window, processId: 43 } };
  assert.equal(selectTarget({ candidates: [c, other] }, c.client.directory, '43').pid, 43);
});

test('pending, interrupted and failed business outcomes never count as acceptance', () => {
  for (const run of [{ code: 6 }, { code: 0, signal: 'SIGTERM' }, { code: null }, { code: 0, error: 'launch failed' }]) {
    assert.throws(() => assessEnvelope(run, envelope({ continuation: { kind: 'wait_external' } })));
  }
  assert.throws(() => assessEnvelope({ code: 0 }, { ok: true }));
  const r = { complete: true, reportState: 'verified', cleanup: 'complete', operation: 'one', report: { ok: false } };
  assert.throws(() => assertReport(r), /business success/);
  assert.throws(() => assertReport({ ...r, report: { ok: true }, cleanup: 'pending' }));
  assert.throws(() => assertActor({ session, identity: { ...identity, guid: 'other' } }, identity));
});

test('metrics require actual bytes and exact creation identity instead of fabricated zero/unsafe precision', () => {
  const target = { pid: 42, processStartedAt: '134037001234567890' };
  assert.ok(requireMetrics({ observation: metrics() }, target));
  const fallback = metrics(); fallback.total.localStopped = true;
  assert.ok(requireMetrics({ observation: fallback }, target));
  assert.throws(() => requireMetrics({}));
  for (const change of [m => delete m.total.actualBytes, m => m.total.actualBytes = 0,
    m => m.total.actualBytes = 2048, m => m.total.readCalls = -1,
    m => m.total.budgetExhausted = true, m => m.total.cancelled = true, m => m.scope = 'operation',
    m => m.pid = 43, m => m.processCreated = Number(m.processCreated)]) {
    const m = metrics(); change(m); assert.throws(() => requireMetrics({ observation: m }, target));
  }
});

test('runner completes fixed-target cases; pending stops without another execute, reload or disconnect', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'lychee-live-optimization-'));
  try {
    const cli = path.join(root, 'candidate.exe'); await fs.writeFile(cli, 'fixture CLI identity');
    for (const stop of [null, 'normal-warm-2', 'connect-cache-off']) {
      const project = path.join(root, stop ?? 'success'), calls = [], reports = new Map();
      const current = { ...identity };
      const invoke = async (output, name, command, args) => {
        calls.push(name);
        assert.ok(await fs.readFile(path.join(output, name + '.intent.json')));
        let r = { session, identity: { ...current }, complete: true, observation: metrics() }, code = 0;
        if (name === 'passive-inventory') r = { candidates: [candidate(root)] };
        else if (name === 'managed-status') r = { installation: { state: 'managed', receipt: { version: '3.0.2' } }, slots: { state: 'managed', count: 200, pending: 0 } };
        else if (name === 'connect-cache-off') {
          r.bound = true;
          const dir = path.join(project, '.lycheedev', 'live', 'connections');
          await fs.mkdir(dir, { recursive: true }); await fs.writeFile(path.join(dir, session + '.jsonl'), 'fixed journal bytes');
        } else if (name === 'reload') { current.runtime = 'r2'; r.identity.runtime = 'r2'; }
        else if (name.startsWith('disconnect')) r.closed = true;
        else {
          const key = args[args.indexOf('--request') + 1];
          if (reports.has(key)) r = reports.get(key);
          else {
            const payload = name.startsWith('payload');
            Object.assign(r, { operation: key, reportState: 'verified', cleanup: 'complete', report: { ok: true,
              result: payload ? { marker: 'native-large-payload', text: '荔枝<&>\n'.repeat(8192) } : { marker: 'native-slot-baseline' } } });
            reports.set(key, r);
          }
        }
        if (name === stop) { code = 6; r.complete = false; r.continuation = { kind: 'wait_external' }; }
        const stdout = path.join(output, name + '.stdout');
        await fs.writeFile(stdout, JSON.stringify(envelope(r)));
        return { code, stdout, milliseconds: 5 };
      };
      const code = await main(['--cli', cli, '--project', project], { invoke });
      const report = JSON.parse(await fs.readFile(path.join(project, 'optimization.json')));
      assert.equal(report.complete, stop === null);
      assert.equal(code, stop === null ? 0 : 2);
      if (stop) {
        assert.equal(calls.at(-1), stop);
        assert.equal(report.failure.session, session);
        assert.ok(report.failure.recovery.includes(session));
      } else {
        assert.equal(calls.filter(c => c.startsWith('normal-')).length, 11);
        assert.ok(calls.includes('payload-cache-off') && calls.includes('historical-readonly'));
        assert.equal(calls.at(-1), 'disconnect-readonly');
        assert.equal(report.target.processStartedAt, '134037001234567890');
      }
      await assert.rejects(() => main(['--cli', cli, '--project', project], { invoke }), /EEXIST/);
    }
  } finally {
    // Exact mkdtemp directory owned by this test, never an input path.
    await fs.rm(root, { recursive: true, force: true });
  }
});
