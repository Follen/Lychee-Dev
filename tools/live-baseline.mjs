// Live runtime baseline: the standard acceptance flow to run against a real
// client after every addon or CLI modification. One command drives the whole
// journaled chain and prints a per-step verdict; exit 0 only when every step
// passes. Game input happens only through the CLI's own guarded commands.
//
// Usage:
//   node tools/live-baseline.mjs --session <session-id> [--account <acct>] [--probe <name>] [--cli <path>] [--skip-hide]
//   node tools/live-baseline.mjs --connect "--pid <pid> --installation <dir> --snapshot <pin>" [...]
// With --connect the tool first runs `live connect` with the given extra args,
// then continues the chain on the returned session.
//
// The default probe is the transport smoke (1..10 sum = 55, no hooks, no
// game-state mutation). Request keys carry a timestamp so repeated runs never
// collide with a previous run's journaled operations.

import { spawnSync } from 'node:child_process';

const args = process.argv.slice(2);
const option = (name) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
};
const flag = (name) => args.includes(name);

const cli = option('--cli') ?? process.env.LYCHEEDEV_CLI ?? 'lycheedev';
const session = option('--session');
const connectArgs = option('--connect');
const account = option('--account');
const probe = option('--probe') ?? 'retail-atomic-smoke-20260924';
const skipHide = flag('--skip-hide');
const expectSum = Number(option('--expect-sum') ?? 55);

function fail(message) {
  console.error(`live-baseline: ${message}`);
  process.exit(2);
}

if (!session && !connectArgs) {
  fail('provide --session <id> or --connect "<live connect extra args>"');
}

function run(extraArgs, timeoutMs) {
  const result = spawnSync(cli, [...extraArgs, '--format', 'json'], {
    encoding: 'utf8',
    timeout: timeoutMs,
    windowsHide: true,
  });
  let envelope = null;
  const firstBrace = result.stdout?.indexOf('{');
  if (firstBrace !== undefined && firstBrace >= 0) {
    try { envelope = JSON.parse(result.stdout.slice(firstBrace)); } catch { /* keep null */ }
  }
  return { envelope, status: result.status, stderr: result.stderr ?? '' };
}

const steps = [];
let sessionId = session ?? null;
let operationId = null;

function step(name, fn) {
  const startedAt = Date.now();
  try {
    const detail = fn();
    const ms = Date.now() - startedAt;
    steps.push({ name, ok: true, ms, detail });
    console.log(`PASS  ${name}  (${ms} ms)${detail ? `  ${detail}` : ''}`);
  } catch (error) {
    const ms = Date.now() - startedAt;
    steps.push({ name, ok: false, ms, detail: String(error.message ?? error) });
    console.error(`FAIL  ${name}  (${ms} ms)  ${error.message ?? error}`);
    report();
    process.exit(1);
  }
}

function expectOk(envelope, label) {
  if (!envelope || envelope.ok !== true) {
    const code = envelope?.error?.code ?? 'no-envelope';
    const message = envelope?.error?.message ?? '';
    throw new Error(`${label}: ${code} ${message}`.trimEnd());
  }
}

function report() {
  const passed = steps.filter((s) => s.ok).length;
  console.log(`\nbaseline: ${passed}/${steps.length} steps passed`);
}

// --- connect (optional) -----------------------------------------------------
if (connectArgs) {
  step('connect', () => {
    const extra = connectArgs.split(/\s+/).filter(Boolean);
    const { envelope } = run(['live', 'connect', ...extra], 240_000);
    expectOk(envelope, 'live connect');
    sessionId = envelope.context?.session ?? envelope.result?.id;
    if (!sessionId) throw new Error('connect returned no session id');
    return `${envelope.result?.character}@${envelope.result?.target?.client?.fullBuild ?? ''}`;
  });
}

// --- load -------------------------------------------------------------------
step('load', () => {
  const request = `R-baseline-${Date.now()}`;
  const extra = account ? ['--account', account] : [];
  const { envelope } = run(['live', 'probe', 'load', '--session', sessionId, '--probe', probe, '--request', request, ...extra], 240_000);
  expectOk(envelope, 'live probe load');
  if (envelope.context?.stage !== 'loaded') {
    throw new Error(`stage ${envelope.context?.stage}, want loaded`);
  }
  operationId = envelope.operationId;
  return operationId;
});

// --- run --------------------------------------------------------------------
let reportSequence = null;
step('run', () => {
  const { envelope } = run(['live', 'run', operationId], 300_000);
  expectOk(envelope, 'live run');
  if (envelope.context?.stage !== 'verified' || envelope.result?.report?.state !== 'verified') {
    throw new Error(`stage ${envelope.context?.stage}, report ${envelope.result?.report?.state}`);
  }
  if (envelope.result?.cleanup !== 'pending') {
    throw new Error(`cleanup ${envelope.result?.cleanup}, want pending`);
  }
  const content = envelope.result.report.content;
  const sum = content?.result?.sum;
  if (sum !== expectSum) {
    throw new Error(`sum ${sum}, want ${expectSum}`);
  }
  reportSequence = envelope.result.report.receiptCapture ?? null;
  return `sum=${sum}`;
});

// --- ack --------------------------------------------------------------------
step('ack', () => {
  const { envelope } = run(['live', 'ack', operationId], 240_000);
  expectOk(envelope, 'live ack');
  if (envelope.context?.stage !== 'cleaned' || envelope.result?.status !== 'completed' || envelope.result?.cleanup !== 'complete') {
    throw new Error(`stage=${envelope.context?.stage} status=${envelope.result?.status} cleanup=${envelope.result?.cleanup}`);
  }
  return 'cleaned/complete';
});

// --- hide -------------------------------------------------------------------
if (!skipHide) {
  step('hide', () => {
    const { envelope } = run(['live', 'hide', '--session', sessionId], 240_000);
    expectOk(envelope, 'live hide');
    if (envelope.result?.cleared !== true) {
      throw new Error('receipt not cleared');
    }
    return 'cleared';
  });
}

report();
const failed = steps.filter((s) => !s.ok).length;
process.exit(failed === 0 ? 0 : 1);
