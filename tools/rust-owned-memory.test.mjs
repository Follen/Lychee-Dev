// Synthetic reducer/fault tests, plus opt-in Windows owned-process observations.
// No fixture result is a full Retail measurement or product acceptance.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {beginMemoryPhase, endMemoryPhase, staticMemoryPhase, nativeMemoryPhase, summarizeMemorySamples, collectOwnedMemory} from './rust-owned-memory.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
const phase = (name = 'cold-index', id = 'phase1') => ({id, phase: name, caseId: id, startedAt: '2026-01-01T00:00:00.000Z', endedAt: '2026-01-01T00:00:01.000Z'});
const token = p => `memory-phase-v1\t${p.id}\t${p.phase}\t${p.caseId}\tactive`;
const member = (pid = 1, workingSetBytes = 100, privateBytes = 200) => ({pid, creationFileTime: `13400000000000000${pid}`, image: pid === 1 ? 'C:\\node.exe' : 'C:\\source.exe', jobMember: true, error: null, workingSetBytes, privateBytes, kernelPeakWorkingSetBytes: 9000, kernelPeakPrivateCommitBytes: 18000});
function sample(index = 0, members = [member(), member(2)], p = phase()) { return {schema: 'lycheedev.owned-memory-sample.v1', index, startedTicks: String(1000 + index * 100), endedTicks: String(1005 + index * 100), phaseBefore: token(p), phaseAfter: token(p), complete: true, error: null, totalAssignedProcesses: 2, workingSetBytes: members.reduce((n, r) => n + r.workingSetBytes, 0), privateBytes: members.reduce((n, r) => n + r.privateBytes, 0), members}; }
function report(samples = [sample()], options = {}, status = {}) {
  return summarizeMemorySamples(samples, {schema: 'lycheedev.owned-memory.v1', frequency: 1000, intervalMs: 100, samples: samples.length, totalAssignedProcesses: 2, threadStopped: true, handlesClosed: true, budgetExceeded: false, error: null, ...status}, {rootIdentity: member(), sourceBinary: 'C:\\source.exe', controllerImage: 'C:\\node.exe', commands: [{caseId: 'index-cold', pid: 2, exit: 0, memoryPhase: phase()}], expectedPhases: ['cold-index'], processSucceeded: true, runState: 'passed_fixture', ...options});
}
test('group peaks sum concurrent CURRENT samples, never individual lifetime peaks', () => {
  const rows = [sample(0, [member(1, 100, 800), member(2, 800, 100)]), sample(1, [member(1, 900, 200), member(2, 100, 1000)])];
  const result = report(rows);
  assert.equal(result.ownedJob.sampledPeakWorkingSet.bytes, 1000);
  assert.equal(result.ownedJob.sampledPeakPrivateBytes.bytes, 1200);
  assert.equal(result.processes[0].kernelPeakWorkingSetBytes, 9000);
  assert.equal(result.processes[1].kernelPeakWorkingSetBytes, 9000);
  assert.equal(result.phases[0].sourceExecutableOnly.sampledPeakWorkingSet.bytes, 800);
  assert.equal(result.state, 'sampled_successful_run');
  assert.match(result.definitions.workingSet, /not a Linux RSS/);
  assert.match(result.definitions.privateBytes, /not resident/);
});
test('a compiler high-water outside measured Source phase is not its Source peak', () => {
  const compiler = member(3, 1000000, 2000000); compiler.image = 'C:\\cl.exe';
  const prep = sample(0, [member(), compiler]); prep.phaseBefore = prep.phaseAfter = null;
  const result = report([prep, sample(1)], {}, {totalAssignedProcesses: 3});
  assert.equal(result.ownedJob.sampledPeakWorkingSet.bytes, 1000100);
  assert.equal(result.phases[0].ownedJob.sampledPeakWorkingSet.bytes, 200);
  assert.equal(result.phases[0].sourceExecutableOnly.sampledPeakWorkingSet.bytes, 100);
  assert.equal(result.processes.find(p => p.pid === 3).role, 'other_owned_member');
});
test('exact held identity role evidence separates controller, CLI and retained owner', () => {
  const result = report([sample(0, [member(), member(2), member(3)])], {resources: [{workerPid: 3, workerCreated: member(3).creationFileTime}]}, {totalAssignedProcesses: 3});
  assert.deepEqual(result.processes.map(p => p.role), ['controller', 'command_root', 'retained_native_owner']);
  assert.equal(result.processes[2].creationFileTime, '134000000000000003');
});
test('no sampled Source executable means unknown rather than invented zero peak', () => {
  const result = report([sample(0, [member()])], {}, {totalAssignedProcesses: 1});
  assert.equal(result.phases[0].sourceExecutableOnly.samples, 0);
  assert.equal(result.phases[0].sourceExecutableOnly.sampledPeakWorkingSet, null);
});
test('failed or unfinished correctness run is never successful memory acceptance', () => {
  for (const options of [{runState: 'failed'}, {runState: 'running'}, {processSucceeded: false}]) {
    const result = report(undefined, options); assert.equal(result.state, 'sampled_failed_or_incomplete_run'); assert.equal(result.runOutcome, 'failed_or_incomplete_run');
  }
});
test('membership/read/phase-boundary failures retain evidence but do not lower peaks with zeros', () => {
  for (const mutate of [s => {s.members[1].error = 'exited';}, s => {s.phaseAfter = 'ended';}, s => {s.error = 'job_query_failed';}]) {
    const bad = sample(1); bad.complete = false; bad.workingSetBytes = null; bad.privateBytes = null; mutate(bad);
    const result = report([sample(), bad]); assert.equal(result.state, 'sampled_with_gaps'); assert.equal(result.sampling.rejectedSamples, 1); assert.equal(result.ownedJob.sampledPeakWorkingSet.bytes, 200);
  }
});
test('poll gaps, unseen children, observer budget and unjoined handles are explicit gaps', () => {
  const delayed = sample(1); delayed.startedTicks = '1500'; delayed.endedTicks = '1505';
  assert.equal(report([sample(), delayed]).sampling.pollingGapsOverTwiceInterval, 1);
  for (const status of [{totalAssignedProcesses: 3}, {threadStopped: false}, {handlesClosed: false}, {budgetExceeded: true}, {error: 'sample_failed'}]) assert.equal(report(undefined, {}, status).state, 'sampled_with_gaps');
  assert.equal(report(undefined, {}, {totalAssignedProcesses: 3}).sampling.unseenProcessCountLowerBound, 1);
});
test('a phase with an entirely missed command remains incomplete', () => {
  const result = report(undefined, {commands: [{memoryPhase: phase()}, {memoryPhase: phase('cold-index', 'missed')} ]});
  assert.equal(result.phases[0].state, 'incomplete'); assert.equal(result.phases[0].windows[1].state, 'not_measured');
});
test('warm-index marker EPERM cannot hide behind full PID coverage or a prior ended marker', () => {
  const cold = phase('cold-index', 'cold'), warm = {...phase('warm-index', 'warm'), markerError: 'EPERM: atomic marker rename failed'}, query = phase('warm-query', 'query');
  const commands = [{pid: 2, exit: 0, memoryPhase: cold}, {pid: 3, exit: 0, memoryPhase: warm}, {pid: 4, exit: 0, memoryPhase: query}];
  const rows = [sample(0, [member(), member(2)], cold), sample(1, [member(), member(3)], cold), sample(2, [member(), member(4)], query)];
  rows[1].phaseBefore = rows[1].phaseAfter = token(cold).replace(/active$/, 'ended');
  // Each separate CLI is sampled; the missing phase cannot be attributed to
  // an unseen short-lived child or an otherwise incomplete correctness run.
  for (const expectedPhases of [['cold-index', 'warm-index', 'warm-query'], ['cold-index', 'warm-query']]) {
    const result = report(rows, {commands, expectedPhases}, {totalAssignedProcesses: 4});
    assert.equal(result.sampling.observedProcessIdentities, 4);
    assert.equal(result.sampling.unseenProcessCountLowerBound, 0);
    assert.equal(result.sampling.rejectedSamples, 0);
    assert.equal(result.runOutcome, 'successful_selected_correctness_run');
    assert.equal(result.phases.find(p => p.phase === 'warm-index').state, 'not_measured');
    assert.equal(result.phases.find(p => p.phase === 'warm-index').windows[0].markerError, warm.markerError);
    assert.equal(result.observationComplete, false);
    assert.equal(result.state, 'sampled_with_gaps');
  }
});
test('both acceptance launchers require warm-index observation independently of cold index', () => {
  for (const name of ['rust-source-full-corpus.mjs', 'rust-source-native-acceptance.mjs']) {
    const text = readFileSync(new URL(name, import.meta.url), 'utf8');
    assert.match(text, /expectedPhases: [^\n]*\['cold-index', 'warm-index', 'warm-query'/, name);
  }
});
test('steady window requires repeated observations and the exact authenticated owner', () => {
  const p = phase('retained-session-steady');
  const options = {commands: [], windows: [{memoryPhase: p, expectedOwner: {pid: 2, created: member(2).creationFileTime}}], resources: [{workerPid: 2, workerCreated: member(2).creationFileTime}], expectedPhases: ['retained-session-steady']};
  const rows = [0, 1, 2].map(i => sample(i, [member(), member(2, 100 + i, 200 + i)], p));
  const good = report(rows, options); assert.equal(good.phases[0].state, 'sampled'); assert.equal(good.phases[0].steadyWindow.medianWorkingSetBytes, 201); assert.equal(good.phases[0].steadyWindow.samples[0].retainedOwners.length, 1);
  assert.equal(report(rows.slice(0, 1), options).phases[0].state, 'incomplete');
  options.windows[0].expectedOwner.created = '134000000000000099';
  assert.equal(report(rows, options).phases[0].windows[0].missingOwnerSamples, 3);
});
test('invalid/forged sums, identity, ordering and counters are rejected', () => {
  for (const mutate of [s => {s.workingSetBytes++;}, s => {s.members[0].privateBytes = -1;}, s => {s.members[0].creationFileTime = 134000000000000001;}, s => {s.members[1].pid = 1;}, s => {s.members[0].jobMember = false;}, s => {s.index = 1;}, s => {s.endedTicks = '999';}, s => {s.members[0].workingSetBytes = 2 ** 53;}]) { const row = sample(); mutate(row); assert.throws(() => report([row]), /memory\./); }
  const overlap = sample(1); overlap.startedTicks = '1002'; assert.throws(() => report([sample(), overlap]), /clock_order/);
});
test('missing expected phases and zero samples stay not measured', () => {
  assert.equal(report([], {}, {totalAssignedProcesses: 0}).state, 'not_measured');
  const result = report(undefined, {expectedPhases: ['cold-index', 'full-native-semantic']});
  assert.equal(result.phases[1].state, 'not_measured'); assert.equal(result.state, 'sampled_with_gaps');
});
test('raw collection preserves malformed/unclosed evidence and missing receipt honestly', () => {
  const evidence = mkdtempSync(join(tmpdir(), 'owned-memory-reducer-')), path = join(evidence, 'part0.jsonl');
  assert.equal(collectOwnedMemory({}, {evidence}).state, 'not_measured');
  writeFileSync(path, JSON.stringify(sample()));
  const run = {proof: {memory: {chunks: [path]}}};
  const incomplete = collectOwnedMemory(run, {evidence}); assert.equal(incomplete.state, 'incomplete'); assert.match(incomplete.error, /truncated_chunk/);
  run.proof.memory.chunks = [join(evidence, '..', 'foreign')]; assert.match(collectOwnedMemory(run, {evidence}).error, /chunk_outside_evidence/);
});
test('phase markers are bounded, atomically replaced and failures are evidence-only', () => {
  const previous = process.env.LYCHEEDEV_OWNED_MEMORY_PHASE;
  try {
    const dir = mkdtempSync(join(tmpdir(), 'owned-memory-phase-')); process.env.LYCHEEDEV_OWNED_MEMORY_PHASE = join(dir, 'phase');
    const p = beginMemoryPhase('cold-index', 'index-cold'); assert.equal(readFileSync(process.env.LYCHEEDEV_OWNED_MEMORY_PHASE, 'utf8'), token(p)); endMemoryPhase(p); assert.match(readFileSync(process.env.LYCHEEDEV_OWNED_MEMORY_PHASE, 'utf8'), /\tended$/);
    assert.equal(beginMemoryPhase('bad\nphase', 'x').markerError, 'memory.invalid_phase');
    process.env.LYCHEEDEV_OWNED_MEMORY_PHASE = join(dir, 'missing', 'phase'); assert(beginMemoryPhase('cold-index', 'x').markerError);
  } finally { if (previous === undefined) delete process.env.LYCHEEDEV_OWNED_MEMORY_PHASE; else process.env.LYCHEEDEV_OWNED_MEMORY_PHASE = previous; }
  assert.equal(staticMemoryPhase('index-cold'), 'cold-index'); assert.equal(staticMemoryPhase('query-x'), 'warm-query');
  assert.equal(nativeMemoryPhase('ordinary-cold-refs'), 'full-native-semantic'); assert.equal(nativeMemoryPhase('session-warm-context'), 'retained-session-warm');
});
test('native sampler is query-only, bounded and tied to the existing Job and held identities', () => {
  const owner = readFileSync(new URL('./rust-owned-process.ps1', import.meta.url), 'utf8'), sampler = owner.split('namespace ToolkitMemory {')[1].split("\n'@")[0];
  assert(sampler); assert.match(sampler, /QueryJob=4,QueryProcess=0x00101000/);
  for (const text of ['DuplicateHandle(', 'IsProcessInJob(handle,job', 'GetProcessTimes(handle', 'QueryFullProcessImageNameW(handle', 'K32GetProcessMemoryInfo(handle', 'identity<rootCreated', 'pid==rootPid&&identity!=rootCreated', 'before!=after', 'assigned!=Total()', 'thread.Join(5000)', 'MaxProcesses=128,MaxSamples=15000', 'MaxBytes=64<<20']) assert(sampler.includes(text), text);
  assert.doesNotMatch(sampler, /TerminateProcess\(|TerminateJobObject\(|EnumProcesses\(|GetProcessesByName\(|CreateToolhelp32Snapshot\(/);
  assert(owner.includes('if(memory!=null)try{result.memory=memory.Stop();}catch(Exception failure)'));
  assert(owner.includes('bool watcherClosed=rootWatcher==null||rootWatcher.Wait(10000);'));
  assert(!owner.includes('rust-owned-memory.cs'), 'copied legacy wrappers stay self-contained');
});
function windowsFixture(t) { mkdirSync('.tmp', {recursive: true}); const evidence = mkdtempSync(join(process.cwd(), '.tmp/rust-memory-test-')); t.diagnostic(`Preserved synthetic observation: ${evidence}`); return {cwd: process.cwd(), env: process.env, evidence, timeout: 15000, orphanMs: 5000, memoryObservation: true}; }
test('Windows observations include owned child, preserve FILETIME and synchronized sums', {skip: process.platform !== 'win32'}, t => {
  const options = windowsFixture(t);
  const code = `const {spawn}=require('child_process'); const child=spawn(process.execPath,['-e',"const b=Buffer.alloc(24<<20,1); setTimeout(()=>process.stdout.write(String(b.length)),1300)"],{stdio:'ignore'}); const b=Buffer.alloc(16<<20,1); child.once('exit',()=>console.log(JSON.stringify({pid:child.pid,bytes:b.length})));`;
  const run = ownedSpawnSync(process.execPath, ['-e', code], options); assert.equal(run.error, undefined); assert.equal(run.status, 0); assert.equal(run.handlesReaped, true);
  const child = JSON.parse(run.stdout), raw = run.proof.memory.chunks.flatMap(path => readFileSync(path, 'utf8').trim().split('\n').map(JSON.parse));
  const together = raw.find(s => s.complete && s.members.some(p => p.pid === child.pid) && s.members.some(p => p.pid === run.proof.pid)); assert(together);
  assert.equal(together.workingSetBytes, together.members.reduce((n, p) => n + p.workingSetBytes, 0));
  assert.equal(together.privateBytes, together.members.reduce((n, p) => n + p.privateBytes, 0));
  assert(together.members.every(p => typeof p.creationFileTime === 'string' && p.jobMember)); assert.equal(run.proof.memory.threadStopped, true); assert.equal(run.proof.memory.handlesClosed, true);
});
test('Windows observation read failure never changes successful business exit', {skip: process.platform !== 'win32'}, t => {
  const options = windowsFixture(t), code = `require('fs').writeFileSync(process.env.LYCHEEDEV_OWNED_MEMORY_PHASE,'x'.repeat(2048));setTimeout(()=>console.log('business-ok'),500);`;
  const run = ownedSpawnSync(process.execPath, ['-e', code], options); assert.equal(run.error, undefined); assert.equal(run.status, 0); assert.equal(run.stdout.toString().trim(), 'business-ok'); assert.equal(run.proof.memory.threadStopped, true);
  const raw = run.proof.memory.chunks.flatMap(path => readFileSync(path, 'utf8').trim().split('\n').map(JSON.parse)); assert(raw.some(s => !s.complete && /phase_budget/.test(s.error)));
});
