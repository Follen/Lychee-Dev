// Shared timing and static Go reference identity for current Data/generation/Live runners.
import assert from 'node:assert/strict';

export const GO_COMMIT = '29bdbdaff5f064ec212ff2064380b1ccac652eda';
export const GO_VERSION = '3.2.4';
export const REPETITIONS = 20;
export const TARGET_RATIO = 0.50;
export const orderFor = iteration => iteration % 2 === 0 ? ['go', 'rust'] : ['rust', 'go'];
export function elapsed(proof) {
  assert.equal(proof?.schema, 'lycheedev.owned-process.v1');
  for (const key of ['rootReaped', 'treeReaped', 'handlesClosed', 'toolIdentityLocksClosed', 'outputReadersComplete']) assert.equal(proof[key], true, key);
  assert.equal(proof.activeProcesses, 0); assert.equal(proof.forced, false);
  assert(!proof.error && !proof.deadlineHit && !proof.outputLimit, 'failed process cannot yield accepted timing');
  const t = proof.timing;
  assert.equal(t?.schema, 'lycheedev.owned-timing.v1', 'actual CLI clock required');
  assert(Number.isSafeInteger(t.frequency) && t.frequency > 0);
  for (const key of ['startTicks', 'rootExitTicks', 'treeReapedTicks']) assert.match(t[key], /^[1-9]\d*$/);
  const start = BigInt(t.startTicks), root = BigInt(t.rootExitTicks), end = BigInt(t.treeReapedTicks);
  assert(end >= root && root >= start && end > start, 'ordered monotonic timing required');
  const millis = Number(end - start) * 1000 / t.frequency;
  assert(Number.isFinite(millis) && millis >= 1000 / t.frequency * 100, 'timing window under 100 clock ticks; enlarge equivalent work before acceptance');
  return {controllableMillis: millis, rootObservedMillis: Number(root - start) * 1000 / t.frequency, reapAndDrainMillis: Number(end - root) * 1000 / t.frequency, resolutionMillis: 1000 / t.frequency};
}


// Optional attribution only. Old receipts remain valid for elapsed()/acceptance.
// Never subtract these phases or root CPU from the original end-to-end clock.
export function startupBreakdown(proof, {required = false} = {}) {
  const d = proof?.diagnostics;
  if (d === undefined) { assert(!required, 'startup diagnostics missing'); return null; }
  const total = elapsed(proof), t = proof.timing;
  assert.equal(d?.schema, 'lycheedev.owned-startup-diagnostics.v1');
  assert(!d.error, 'startup diagnostic observation failed');
  const names = ['createReturnTicks', 'rootPersistBeginTicks', 'rootPersistEndTicks', 'observerSetupEndTicks', 'resumeBeforeTicks', 'resumeAfterTicks'];
  for (const name of names) assert.match(d[name], /^[1-9]\d*$/, name);
  const ticks = [BigInt(t.startTicks), ...names.map(name => BigInt(d[name])), BigInt(t.rootExitTicks), BigInt(t.treeReapedTicks)];
  for (let i = 1; i < ticks.length; i++) assert(ticks[i] >= ticks[i - 1], 'startup diagnostic clock order');
  assert.equal(d.rootCpuObserved, true, 'root process CPU observation missing');
  for (const name of ['rootKernel100ns', 'rootUser100ns']) assert.match(d[name], /^(0|[1-9]\d*)$/, name);
  const millis = (start, end) => Number(end - start) * 1000 / t.frequency;
  const phaseNames = ['createProcessMillis', 'identityBeforePersistenceMillis', 'rootIdentityDurabilityMillis', 'outputAndObserverSetupMillis', 'beforeResumeGapMillis', 'resumeCallMillis', 'resumedToObservedRootExitMillis', 'rootExitToReapAndDrainMillis'];
  return {
    schema: 'lycheedev.startup-breakdown.v1', ...total,
    preResumeMillis: millis(ticks[0], ticks[5]),
    resumeThroughReapAndDrainMillis: millis(ticks[5], ticks[8]),
    phases: Object.fromEntries(phaseNames.map((name, i) => [name, millis(ticks[i], ticks[i + 1])])),
    rootCpu: {kernelMillis: Number(BigInt(d.rootKernel100ns)) / 10000, userMillis: Number(BigInt(d.rootUser100ns)) / 10000,
      scope: 'GetProcessTimes on the original reaped root handle only; not descendants or owner/observer CPU'},
    qualification: 'Diagnostic attribution only; all original elapsed time remains included. Resume-through-reap contains scheduling, loader/runtime, product work, cleanup and output drain; it is not pure CPU or a performance pass.',
  };
}

export function statistics(values) {
  assert(values.length > 0 && values.every(v => Number.isFinite(v) && v > 0));
  const sorted = [...values].sort((a, b) => a - b), middle = Math.floor(sorted.length / 2);
  return {count: sorted.length, median: sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2, p95: sorted[Math.ceil(sorted.length * .95) - 1], min: sorted[0], max: sorted.at(-1)};
}
