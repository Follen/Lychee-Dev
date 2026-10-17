import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {sha256} from '../../tools/rust-disk.mjs';
import {PHASE_NAMES, PHASE_TIMING, LOCK_TIMING, sourcePhaseProfile} from './source-candidates.mjs';
import {parsePackProfile, verifyPackProfile, verifyRetainedPackObservation} from './source-pack-resume.mjs';
const runId = '43ceb1ad-4e62-49cf-8058-505343a07f4f';
function observation(mode = 'warm', documents = 4036) {
  const calls = Object.fromEntries(PHASE_NAMES.map(name => [name, 0])); calls.metadata = 1;
  const io = {cachedFacts: documents, parsedDocuments: 0, gitTreeProcesses: 0, gitBlobProcesses: 0, elapsedMs: 400, deadlineMs: 180000};
  if (mode === 'warm') { calls.cached_index = 1; Object.assign(io, {cachedIndex: true, indexBytesRead: 82228560, sourceBytesRead: 0}); }
  else {
    for (const name of ['index_lock', 'git_tree', 'git_blobs_inclusive', 'index_serialize', 'index_publish', 'manifest']) calls[name] = 1;
    const parsed = mode === 'cold' ? documents : 64;
    Object.assign(io, {cachedFacts: documents - parsed, parsedDocuments: parsed, gitTreeProcesses: 1, gitBlobProcesses: 64, readBytes: 50000});
    Object.assign(calls, {facts_read: documents + Math.ceil(documents / 64), facts_parse: parsed, facts_parse_sequential: parsed, facts_serialize: parsed, facts_publish: parsed + 3 * Math.ceil(documents / 64), symbols: documents, original_publish: documents});
  }
  const profile = {schema: 'lycheedev.source-index-phases.v1', timing: PHASE_TIMING, lockTiming: LOCK_TIMING, elapsedMicros: 100000, phases: PHASE_NAMES.map(phase => ({phase, calls: calls[phase], elapsedMicros: calls[phase]}))};
  const envelope = {schema: 'lycheedev.result.v1', ok: true, error: null, result: {schema: 'lycheedev.source-map.v2', documents, complete: true, skippedDocuments: 0, io}};
  return {profile, envelope, raw: () => Buffer.from(JSON.stringify(profile) + '\n')};
}
const set = (value, name, calls) => Object.assign(value.profile.phases.find(row => row.phase === name), {calls, elapsedMicros: calls});

test('pack profile accepts the warm metadata/cache-only path while historical cold validator remains strict', () => {
  const value = observation(), before = value.raw();
  const report = verifyPackProfile(value.raw(), value.envelope, {mode: 'warm', documents: 4036});
  assert.equal(report.mode, 'warm'); assert.equal(report.fullFactsEquivalent, 'not_evaluated'); assert.equal(report.performanceAcceptanceEvaluated, false);
  assert.throws(() => sourcePhaseProfile(value.raw()), /cold-index phase git_tree/);
  assert.deepEqual(value.raw(), before);
});

test('pack profile validates cold grouped counters rather than old per-file read/publication counts', () => {
  const value = observation('cold');
  assert.equal(value.profile.phases.find(p => p.phase === 'facts_read').calls, 4100);
  assert.equal(value.profile.phases.find(p => p.phase === 'facts_publish').calls, 4228);
  verifyPackProfile(value.raw(), value.envelope, {mode: 'cold', documents: 4036});
  set(value, 'facts_read', 4036); assert.throws(() => verifyPackProfile(value.raw(), value.envelope, {mode: 'cold', documents: 4036}), /group\/document/);
});

test('pack profile allows changed cohorts and speculative cache-hit workers without claiming only one parse', () => {
  const value = observation('changed'); set(value, 'facts_worker_consumed', 4000); set(value, 'facts_worker_wait', 4000); set(value, 'facts_worker_abandoned', 23);
  verifyPackProfile(value.raw(), value.envelope, {mode: 'changed', documents: 4036});
  set(value, 'facts_parse', 1); assert.throws(() => verifyPackProfile(value.raw(), value.envelope, {mode: 'changed', documents: 4036}));
  set(value, 'facts_parse', 64); set(value, 'facts_worker_wait', 3999); assert.throws(() => verifyPackProfile(value.raw(), value.envelope, {mode: 'changed', documents: 4036}));
});

test('pack profile rejects malformed duplicate extra reordered noninteger and nonzero zero-call fields', () => {
  const base = observation();
  for (const raw of [Buffer.alloc(0), Buffer.alloc(16385), Buffer.from([0xff]), Buffer.from('\ufeff' + base.raw()), Buffer.from(base.raw().toString() + '{}\n'), Buffer.from(base.raw().toString().replace('"elapsedMicros":100000', '"elapsedMicros":1,"elapsedMicros":100000')), Buffer.from(JSON.stringify(base.profile, null, 2))]) assert.throws(() => parsePackProfile(raw));
  for (const change of [v => v.extra = true, v => v.phases.reverse(), v => v.phases.pop(), v => v.phases[0].calls = -1, v => v.phases[0].calls = 0.5, v => v.phases[0].calls = Number.MAX_SAFE_INTEGER + 1, v => v.phases[3].elapsedMicros = 1, v => v.lockTiming = 'additive']) {
    const copy = structuredClone(base.profile); change(copy); assert.throws(() => parsePackProfile(Buffer.from(JSON.stringify(copy))));
  }
});

test('pack profile rejects mode confusion incomplete output false cache claims changed deadline and denominators', () => {
  for (const change of [v => v.envelope.ok = false, v => v.envelope.schema = 'unknown', v => v.envelope.result.complete = false, v => v.envelope.result.skippedDocuments = 1, v => v.envelope.result.io.cachedFacts--, v => v.envelope.result.io.cachedIndex = false, v => v.envelope.result.io.deadlineMs++, v => v.envelope.result.io.gitTreeProcesses = 1, v => set(v, 'git_tree', 1), v => set(v, 'facts_worker_consumed', 1)]) {
    const value = observation(); change(value); assert.throws(() => verifyPackProfile(value.raw(), value.envelope, {mode: 'warm', documents: 4036}));
  }
  for (const mode of ['cold', 'changed', 'unknown']) { const v = observation(); assert.throws(() => verifyPackProfile(v.raw(), v.envelope, {mode, documents: 4036})); }
});

function retained(t) {
  const root = mkdtempSync(join(tmpdir(), 'source-pack-observation-')); t.after(() => { if (t.passed) rmSync(root, {recursive: true}); else console.error(`retained fixture: ${root}`); });
  const evidence = join(root, '.tmp/rust-disk/evidence/6d9d5ad5-01ec-4a9c-a248-85020e1e62cb'); mkdirSync(evidence, {recursive: true});
  const binary = join(root, 'sealed.exe'); writeFileSync(binary, 'synthetic fixture only');
  const artifact = {schema: 'lycheedev.rust-build-artifact.v1', profile: 'release', sameInvocation: true, sourceBinary: binary, binarySHA256: sha256(readFileSync(binary)), inputHashes: {sourceTree: 'a'.repeat(64)}};
  const artifactPath = join(root, 'artifact.json'); writeFileSync(artifactPath, JSON.stringify(artifact));
  const proof = {schema: 'lycheedev.owned-process.v1', runId, binary, exitCode: 0, rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0, forced: false, deadlineHit: false, outputLimit: false, error: null,
    timing: {schema: 'lycheedev.owned-timing.v1', frequency: 10000000, startTicks: '1000', rootExitTicks: '5096000', treeReapedTicks: '5096789'}};
  const value = observation(), raw = {stdout: Buffer.from(JSON.stringify(value.envelope)), stderr: value.raw(), receipt: Buffer.from(JSON.stringify(proof))};
  const config = {schema: 'lycheedev.source-pack-observation-config.v1', root, mode: 'warm', documents: 4036, runId, artifact: {root, path: artifactPath, sha256: sha256(readFileSync(artifactPath))}, files: {}};
  for (const [name, body] of Object.entries(raw)) { const path = join(evidence, `process-${runId}.${name === 'receipt' ? 'receipt.json' : name + '.bin'}`); writeFileSync(path, body); config.files[name] = {path, bytes: body.length, sha256: sha256(body)}; }
  return {config, proof};
}

test('retained pack observation authenticates original bytes executable and original native timing without product calls', t => {
  const {config} = retained(t), before = Object.values(config.files).map(row => readFileSync(row.path));
  const result = verifyRetainedPackObservation(config);
  assert.equal(result.timing.controllableMillis, 509.5789); assert.equal(result.productCalls, 0); assert.equal(result.importedSamples, 0); assert.equal(result.originalFailurePreserved, true); assert.equal(result.fullFactsEquivalent, 'not_evaluated'); assert.equal(result.completeInvocationVerified, false); assert.equal(result.historicalLaunchIdentity, 'not_independently_reauthenticated'); assert.equal(result.wrapperOutcome, 'not_evaluated');
  Object.values(config.files).forEach((row, i) => assert.deepEqual(readFileSync(row.path), before[i]));
});

test('retained pack observation rejects wrong raw identity mismatched native binary failed exit and unreaped proof', t => {
  const {config, proof} = retained(t);
  for (const change of [c => c.files.stderr.sha256 = '0'.repeat(64), c => c.files.stdout.bytes++, c => c.artifact.sha256 = '0'.repeat(64), c => c.runId = '00000000-0000-0000-0000-000000000000']) { const c = structuredClone(config); change(c); assert.throws(() => verifyRetainedPackObservation(c)); }
  for (const change of [p => p.binary += '.other', p => p.exitCode = 1, p => p.forced = true, p => p.deadlineHit = true, p => p.handlesClosed = false, p => p.activeProcesses = 1]) {
    const p = structuredClone(proof); change(p); const body = Buffer.from(JSON.stringify(p)), c = structuredClone(config); writeFileSync(c.files.receipt.path, body); c.files.receipt.bytes = body.length; c.files.receipt.sha256 = sha256(body); assert.throws(() => verifyRetainedPackObservation(c));
  }
});
