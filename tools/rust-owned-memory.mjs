// Evidence-only Windows Job memory observation. This module never launches,
// discovers, kills, or changes processes. All membership comes from the owner.
import {existsSync, lstatSync, readFileSync, renameSync, writeFileSync} from 'node:fs';
import {isAbsolute, relative, resolve} from 'node:path';
import {createHash, randomUUID} from 'node:crypto';

const schema = 'lycheedev.owned-memory-sample.v1';
const marker = value => `memory-phase-v1\t${value.id}\t${value.phase}\t${value.caseId}\tactive`;
function publish(value, text) {
  const path = process.env.LYCHEEDEV_OWNED_MEMORY_PHASE;
  if (!path) { value.observer = 'not_enabled'; return; }
  try {
    const temporary = `${path}.${process.pid}.next`;
    writeFileSync(temporary, text, {flag: 'w'}); renameSync(temporary, path);
  } catch (error) { value.markerError = error.message; }
}
export function beginMemoryPhase(phase, caseId) {
  const value = {id: randomUUID(), phase, caseId, startedAt: new Date().toISOString()};
  if (![phase, caseId].every(text => /^[A-Za-z0-9_.:-]{1,160}$/.test(text))) { value.markerError = 'memory.invalid_phase'; return value; }
  publish(value, marker(value)); return value;
}
export function endMemoryPhase(value) {
  value.endedAt = new Date().toISOString();
  // End markers are distinct, so a sweep straddling a command boundary is
  // rejected instead of silently attributing it to either command.
  if (!value.markerError) publish(value, `memory-phase-v1\t${value.id}\t${value.phase}\t${value.caseId}\tended`);
  return value;
}
export function staticMemoryPhase(caseId) {
  if (caseId === 'index-cold') return 'cold-index';
  if (caseId === 'index-warm') return 'warm-index';
  if (/^(query-|page-)/.test(caseId)) return 'warm-query';
  if (caseId === 'sync') return 'input-preparation';
  return 'other-static';
}
export function nativeMemoryPhase(caseId) {
  if (/^(ordinary-cold-refs|ordinary-warm-context|api-native-type)$/.test(caseId)) return 'full-native-semantic';
  if (caseId === 'session-cold-refs') return 'retained-session-cold';
  if (caseId === 'session-warm-context') return 'retained-session-warm';
  if (caseId.startsWith('session-')) return 'retained-session-lifecycle';
  return 'other-native';
}
const exactImage = value => typeof value === 'string' ? value.replace(/^\\\\\?\\/, '').replaceAll('/', '\\').toLowerCase() : '';
const identity = row => `${row.pid}:${row.creationFileTime}`;
const integer = value => Number.isSafeInteger(value) && value >= 0;
function requireValue(test, reason) { if (!test) throw new Error(`memory.${reason}`); }
function peak(rows, field) {
  if (!rows.length) return null;
  const row = rows.reduce((best, item) => item[field] > best[field] ? item : best);
  return {bytes: row[field], sampleIndex: row.index, startedTicks: row.startedTicks, endedTicks: row.endedTicks};
}
function counters(rows) { return {sampledPeakWorkingSet: peak(rows, 'workingSetBytes'), sampledPeakPrivateBytes: peak(rows, 'privateBytes')}; }
function median(values) { if (!values.length) return null; const sorted = [...values].sort((a, b) => a - b), half = Math.floor(sorted.length / 2); return sorted.length % 2 ? sorted[half] : (sorted[half - 1] + sorted[half]) / 2; }
export function summarizeMemorySamples(samples, status, {
  rootIdentity, commands = [], windows = [], resources = [], sourceBinary, controllerImage,
  expectedPhases = [], runState = 'unknown', processSucceeded = false,
} = {}) {
  requireValue(status?.schema === 'lycheedev.owned-memory.v1', 'status_schema');
  requireValue(integer(status.frequency) && status.frequency > 0, 'clock_frequency');
  requireValue(integer(status.intervalMs) && status.intervalMs >= 10 && integer(status.totalAssignedProcesses), 'status_counters');
  requireValue(samples.length <= 15000 && status.samples === samples.length, 'sample_count');
  const phaseTokens = new Map([...commands, ...windows].filter(row => row.memoryPhase).map(row => [marker(row.memoryPhase), row]));
  const retained = new Set(resources.filter(row => Number.isSafeInteger(row.workerPid) && typeof row.workerCreated === 'string').map(row => `${row.workerPid}:${row.workerCreated}`));
  const processes = new Map(), phases = new Map(expectedPhases.map(name => [name, {phase: name, windows: [], rows: [], rejectedSamples: 0}]));
  for (const row of [...commands, ...windows]) if (row.memoryPhase) {
    const name = row.memoryPhase.phase;
    if (!phases.has(name)) phases.set(name, {phase: name, windows: [], rows: [], rejectedSamples: 0});
    phases.get(name).windows.push({caseId: row.caseId ?? row.memoryPhase.caseId, pid: row.pid ?? null, expectedOwner: row.expectedOwner ?? null, exit: row.exit ?? null, signal: row.signal ?? null, error: row.error ?? null, ...row.memoryPhase});
  }
  let previous, previousEnd, maxStartGapMillis = 0, maxSweepMillis = 0, pollingGaps = 0, rejectedSamples = 0;
  const accepted = [];
  for (const [index, sample] of samples.entries()) {
    requireValue(sample.schema === schema && sample.index === index, 'sample_order');
    requireValue(/^\d+$/.test(sample.startedTicks) && /^\d+$/.test(sample.endedTicks), 'sample_clock');
    const start = BigInt(sample.startedTicks), end = BigInt(sample.endedTicks);
    requireValue(end >= start && (previous === undefined || start >= previousEnd), 'clock_order');
    const sweep = Number(end - start) * 1000 / status.frequency;
    maxSweepMillis = Math.max(maxSweepMillis, sweep);
    if (previous !== undefined) { const gap = Number(start - previous) * 1000 / status.frequency; maxStartGapMillis = Math.max(maxStartGapMillis, gap); if (gap > status.intervalMs * 2) pollingGaps++; }
    previous = start; previousEnd = end;
    requireValue(Array.isArray(sample.members) && sample.members.length <= 128, 'member_budget');
    const row = phaseTokens.get(sample.phaseBefore), bucket = row ? phases.get(row.memoryPhase.phase) : null;
    const seen = new Set(), pids = new Set(); let workingSetBytes = 0, privateBytes = 0;
    for (const member of sample.members) {
      // A failed read is retained in raw evidence, never fabricated as zero.
      if (member.error || !member.jobMember) { requireValue(!sample.complete, 'false_complete_member'); continue; }
      requireValue(Number.isSafeInteger(member.pid) && member.pid > 0 && typeof member.creationFileTime === 'string' && /^[1-9]\d*$/.test(member.creationFileTime), 'member_identity');
      requireValue(typeof member.image === 'string' && member.image.length > 0, 'member_image');
      const key = identity(member); requireValue(!seen.has(key) && !pids.has(member.pid), 'duplicate_member'); seen.add(key); pids.add(member.pid);
      for (const field of ['workingSetBytes', 'privateBytes', 'kernelPeakWorkingSetBytes', 'kernelPeakPrivateCommitBytes']) requireValue(integer(member[field]), 'counter_range');
      workingSetBytes += member.workingSetBytes; privateBytes += member.privateBytes;
      let process = processes.get(key);
      if (!process) { process = {pid: member.pid, creationFileTime: member.creationFileTime, image: member.image, role: 'other_owned_member', roleBasis: 'authenticated Job member; role not established', samples: 0, sampledPeakWorkingSetBytes: 0, sampledPeakPrivateBytes: 0, kernelPeakWorkingSetBytes: 0, kernelPeakPrivateCommitBytes: 0}; processes.set(key, process); }
      requireValue(exactImage(process.image) === exactImage(member.image), 'image_changed');
      if (rootIdentity && key === identity(rootIdentity)) { process.role = 'controller'; process.roleBasis = 'outer owner PID and creation identity'; }
      else if (retained.has(key)) { process.role = 'retained_native_owner'; process.roleBasis = 'exact existing native diagnostic PID and creation identity'; }
      else if (row?.pid === member.pid && sample.phaseBefore === sample.phaseAfter) { process.role = 'command_root'; process.roleBasis = 'spawnSync PID observed within that command phase; creation identity from held Job-member handle'; }
      else if (process.role === 'other_owned_member' && exactImage(member.image) === exactImage(controllerImage)) { process.role = 'controller_image'; process.roleBasis = 'exact controller executable image; no command-line role inference'; }
      else if (process.role === 'other_owned_member' && exactImage(member.image) === exactImage(sourceBinary)) { process.role = 'source_worker_candidate'; process.roleBasis = 'exact source executable image; one-shot worker role is not independently authenticated'; }
      process.samples++; process.lastSampleIndex = index;
      process.sampledPeakWorkingSetBytes = Math.max(process.sampledPeakWorkingSetBytes, member.workingSetBytes);
      process.sampledPeakPrivateBytes = Math.max(process.sampledPeakPrivateBytes, member.privateBytes);
      process.kernelPeakWorkingSetBytes = Math.max(process.kernelPeakWorkingSetBytes, member.kernelPeakWorkingSetBytes);
      process.kernelPeakPrivateCommitBytes = Math.max(process.kernelPeakPrivateCommitBytes, member.kernelPeakPrivateCommitBytes);
    }
    if (sample.complete) {
      requireValue(!sample.error && sample.phaseBefore === sample.phaseAfter && integer(workingSetBytes) && integer(privateBytes), 'false_complete_sweep');
      requireValue(workingSetBytes === sample.workingSetBytes && privateBytes === sample.privateBytes, 'aggregate_mismatch');
      // A sweep is synchronized, not atomic: only current values from this one
      // bounded sweep are summed. Per-process lifetime peaks are never summed.
      const value = {...sample, sweepMillis: sweep}; accepted.push(value); if (bucket) bucket.rows.push(value);
    } else { rejectedSamples++; if (bucket) bucket.rejectedSamples++; }
  }
  const phaseReports = [...phases.values()].map(({rows, windows, ...phase}) => {
    const sourceRows = rows.filter(row => row.members.some(member => exactImage(member.image) === exactImage(sourceBinary))).map(row => ({...row, workingSetBytes: row.members.filter(member => exactImage(member.image) === exactImage(sourceBinary)).reduce((sum, member) => sum + member.workingSetBytes, 0), privateBytes: row.members.filter(member => exactImage(member.image) === exactImage(sourceBinary)).reduce((sum, member) => sum + member.privateBytes, 0)}));
    const windowReports = windows.map(window => {
      const matching = rows.filter(row => row.phaseBefore === marker(window));
      const expected = window.expectedOwner && `${window.expectedOwner.pid}:${window.expectedOwner.created}`;
      const missingOwnerSamples = expected ? matching.filter(row => !row.members.some(member => identity(member) === expected)).length : 0;
      return {...window, completeSamples: matching.length, missingOwnerSamples,
        state: !matching.length ? 'not_measured' : window.markerError || !window.endedAt || missingOwnerSamples ? 'incomplete' : 'sampled'};
    });
    const steady = phase.phase === 'retained-session-steady' ? rows.map(row => ({index: row.index, startedTicks: row.startedTicks, endedTicks: row.endedTicks, workingSetBytes: row.workingSetBytes, privateBytes: row.privateBytes, retainedOwners: row.members.filter(member => retained.has(identity(member))).map(({pid, creationFileTime, workingSetBytes, privateBytes}) => ({pid, creationFileTime, workingSetBytes, privateBytes}))})) : undefined;
    return {...phase, state: !rows.length ? 'not_measured' : windowReports.some(row => row.state !== 'sampled') || (steady && steady.length < 3) ? 'incomplete' : 'sampled', samples: rows.length, windows: windowReports,
      ownedJob: counters(rows), sourceExecutableOnly: {samples: sourceRows.length, ...counters(sourceRows)},
      ...(steady ? {steadyWindow: {meaning: 'explicit 1-second idle window after warm reuse; at least 3 samples required, not convergence or leak proof', samples: steady, medianWorkingSetBytes: median(steady.map(row => row.workingSetBytes)), medianPrivateBytes: median(steady.map(row => row.privateBytes))}} : {})};
  });
  const unseenProcessCountLowerBound = Math.max(0, status.totalAssignedProcesses - processes.size);
  // Required phases detect omitted work; every recorded phase also governs
  // completeness, including optional windows whose marker publication failed.
  const complete = status.threadStopped === true && status.handlesClosed === true && !status.error && !status.budgetExceeded && rejectedSamples === 0 && pollingGaps === 0 && unseenProcessCountLowerBound === 0 && phaseReports.every(row => row.state === 'sampled') && expectedPhases.every(name => phaseReports.find(row => row.phase === name)?.state === 'sampled');
  return {schema: 'lycheedev.owned-memory-report.v1', state: !accepted.length ? 'not_measured' : !complete ? 'sampled_with_gaps' : processSucceeded && runState.startsWith('passed_') ? 'sampled_successful_run' : 'sampled_failed_or_incomplete_run',
    correctnessRunState: runState, runOutcome: processSucceeded && runState.startsWith('passed_') ? 'successful_selected_correctness_run' : 'failed_or_incomplete_run', processSucceeded, observationComplete: complete, platform: 'windows', unit: 'bytes', scope: 'existing owned Job, including controller, CLI, native workers and other descendants; wrapper/sampler itself is outside the Job',
    definitions: {workingSet: 'Windows WorkingSetSize; not a Linux RSS measurement. Summed working sets may count shared pages more than once.', privateBytes: 'Windows PrivateUsage committed private memory; not resident memory.', sampledPeak: 'Maximum synchronized current-counter sum among complete sampled sweeps; lower bound, not an atomic snapshot or kernel group high-water mark.', kernelPeaks: 'Per-process lifetime counters observed through authenticated handles; may predate a phase, never summed into group peaks.'},
    sampling: {...status, chunks: undefined, samples: samples.length, acceptedSamples: accepted.length, rejectedSamples, maxStartGapMillis, maxSweepMillis, pollingGapsOverTwiceInterval: pollingGaps, observedProcessIdentities: processes.size, unseenProcessCountLowerBound, missedBetweenPollsPossible: true},
    ownedJob: counters(accepted), phases: phaseReports, processes: [...processes.values()]};
}
export function collectOwnedMemory(run, {evidence, ...options}) {
  try {
    const status = run.proof?.memory;
    if (!status) return {schema: 'lycheedev.owned-memory-report.v1', state: 'not_measured', reason: 'No terminal owned memory receipt', correctnessRunState: options.runState ?? 'unknown'};
    requireValue(Array.isArray(status.chunks) && status.chunks.length <= 16, 'chunk_budget');
    const samples = [], chunks = []; let total = 0;
    for (const path of status.chunks) {
      const rel = relative(resolve(evidence), resolve(path));
      requireValue(rel !== '' && !rel.startsWith('..') && !isAbsolute(rel), 'chunk_outside_evidence');
      requireValue(existsSync(path) && lstatSync(path).isFile() && !lstatSync(path).isSymbolicLink(), 'chunk_not_regular');
      const size = lstatSync(path).size; total += size; requireValue(size <= 8 << 20 && total <= 64 << 20, 'chunk_size');
      const raw = readFileSync(path); requireValue(!raw.length || raw.at(-1) === 10, 'truncated_chunk');
      chunks.push({path: rel, bytes: raw.length, sha256: createHash('sha256').update(raw).digest('hex')});
      for (const line of raw.toString('utf8').split('\n').filter(Boolean)) { requireValue(samples.length < 15000 && Buffer.byteLength(line) <= 1 << 20, 'raw_budget'); samples.push(JSON.parse(line)); }
    }
    return {...summarizeMemorySamples(samples, status, {...options, rootIdentity: {pid: run.proof.pid, creationFileTime: run.proof.creationFileTime}, processSucceeded: !run.error && run.status === 0 && run.handlesReaped === true}), chunks};
  } catch (error) { return {schema: 'lycheedev.owned-memory-report.v1', state: 'incomplete', error: error.message, correctnessRunState: options.runState ?? 'unknown', rawEvidenceRetained: true}; }
}
