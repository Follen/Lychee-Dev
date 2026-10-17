// Validation of retained pack-index observations. No process runs or cache writes.
import assert from 'node:assert/strict';
import {lstatSync, readFileSync} from 'node:fs';
import {basename, dirname, join, resolve} from 'node:path';
import {PHASE_NAMES, PHASE_TIMING, LOCK_TIMING} from './source-candidates.mjs';
import {contained, sha256} from '../../tools/rust-disk.mjs';
import {hashFile, verifyArtifact} from '../../tools/rust-build.mjs';
import {elapsed} from './contract.mjs';
const exact = (value, names) => assert.deepEqual(Object.keys(value).sort(), [...names].sort(), 'exact Source profile fields required');
const integer = value => assert(Number.isSafeInteger(value) && value >= 0, 'nonnegative exact integer required');

export function parsePackProfile(raw) {
  assert(Buffer.isBuffer(raw) && raw.length > 0 && raw.length <= 16384, 'missing or oversized Source profile');
  const text = new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(raw);
  const line = text.endsWith('\r\n') ? text.slice(0, -2) : text.endsWith('\n') ? text.slice(0, -1) : text;
  assert(line.startsWith('{') && line.endsWith('}') && !/[\r\n]/.test(line), 'exactly one compact Source record required');
  const value = JSON.parse(line); assert.equal(JSON.stringify(value), line, 'duplicate keys and noncanonical JSON rejected');
  exact(value, ['schema', 'timing', 'lockTiming', 'elapsedMicros', 'phases']);
  assert.equal(value.schema, 'lycheedev.source-index-phases.v1'); assert.equal(value.timing, PHASE_TIMING); assert.equal(value.lockTiming, LOCK_TIMING); integer(value.elapsedMicros);
  assert(Array.isArray(value.phases)); assert.equal(value.phases.length, PHASE_NAMES.length);
  value.phases.forEach((phase, i) => { exact(phase, ['phase', 'calls', 'elapsedMicros']); assert.equal(phase.phase, PHASE_NAMES[i]); integer(phase.calls); integer(phase.elapsedMicros); if (!phase.calls) assert.equal(phase.elapsedMicros, 0); });
  return value;
}

export function verifyPackProfile(raw, envelope, {mode, documents}) {
  assert(['cold', 'warm', 'changed'].includes(mode), 'explicit cold/warm/changed mode required'); integer(documents); assert(documents > 0 && documents <= 100000);
  const profile = parsePackProfile(raw), result = envelope?.result;
  assert.equal(envelope?.schema, 'lycheedev.result.v1'); assert.equal(envelope.ok, true); assert.equal(envelope.error, null); assert.equal(result?.schema, 'lycheedev.source-map.v2');
  assert.equal(result.documents, documents); assert.equal(result.complete, true); assert.equal(result.skippedDocuments ?? 0, 0);
  const io = result.io; assert(io && typeof io === 'object');
  for (const key of ['cachedFacts', 'parsedDocuments', 'gitTreeProcesses', 'gitBlobProcesses', 'elapsedMs']) integer(io[key]);
  assert.equal(io.deadlineMs, 180000); assert.equal(io.cachedFacts + io.parsedDocuments, documents);
  const calls = Object.fromEntries(profile.phases.map(row => [row.phase, row.calls]));
  assert.equal(calls.metadata, 1); assert.equal(calls.facts_parse, io.parsedDocuments);
  assert.equal(calls.facts_worker_wait, calls.facts_worker_consumed); assert(calls.facts_worker_abandoned <= calls.facts_worker_consumed);
  if (mode === 'warm') {
    assert.equal(io.cachedIndex, true); assert.equal(io.cachedFacts, documents); assert.equal(io.parsedDocuments, 0);
    assert.equal(io.gitTreeProcesses, 0); assert.equal(io.gitBlobProcesses, 0); assert.equal(io.sourceBytesRead, 0);
    integer(io.indexBytesRead); assert(io.indexBytesRead > 0 && io.indexBytesRead <= (128 << 20));
    assert.equal(calls.cached_index, 1);
    for (const name of PHASE_NAMES) if (!['metadata', 'cached_index'].includes(name)) assert.equal(calls[name], 0, `warm cache hit cannot run ${name}`);
  } else {
    assert(io.cachedIndex === undefined || io.cachedIndex === false); assert.equal(calls.cached_index, 0);
    assert.equal(io.gitTreeProcesses, 1); assert(io.gitBlobProcesses > 0); integer(io.readBytes);
    for (const name of ['index_lock', 'git_tree', 'git_blobs_inclusive', 'index_serialize', 'index_publish', 'manifest']) assert.equal(calls[name], 1, `successful uncached index ${name}`);
    // Packs observe one group-open plus one per-document read, and may also
    // close existing readers. Publishing counts frames and group lifecycle,
    // not documents; repairs can serialize cached backfill as well.
    assert(calls.facts_read >= documents + Math.ceil(documents / 64), 'complete group/document reads required');
    assert(calls.facts_serialize >= io.parsedDocuments); assert(calls.facts_publish >= calls.facts_serialize);
    if (mode === 'cold') {
      assert.equal(io.cachedFacts, 0); assert.equal(io.parsedDocuments, documents);
      assert.equal(calls.facts_serialize, documents); assert.equal(calls.symbols, documents);
      assert.equal(calls.facts_parse_sequential + calls.facts_worker_consumed - calls.facts_worker_abandoned, documents, 'cold parse denominator');
    }
    // Cache hits may discard prepared speculative worker results; the cold
    // worker denominator is intentionally not imposed on changed-index work.
  }
  return {mode, documents, profile, io, profileSHA256: sha256(raw), profileBytes: raw.length, performanceAcceptanceEvaluated: false, fullFactsEquivalent: 'not_evaluated'};
}

// Authenticate already-produced bytes and native timing. This does not infer
// full-Facts equivalence from a cache-hit flag or import performance samples.
export function verifyRetainedPackObservation(config) {
  exact(config, ['schema', 'root', 'mode', 'documents', 'runId', 'artifact', 'files']);
  assert.equal(config.schema, 'lycheedev.source-pack-observation-config.v1');
  assert.match(config.runId, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/);
  exact(config.artifact, ['root', 'path', 'sha256']); assert.match(config.artifact.sha256, /^[0-9a-f]{64}$/);
  assert.equal(hashFile(contained(config.artifact.root, config.artifact.path), 65536), config.artifact.sha256, 'artifact record changed');
  const artifact = verifyArtifact(config.artifact.root, config.artifact.path);
  assert.equal(artifact.sameInvocation, true); assert(['release', 'symbols'].includes(artifact.profile));
  exact(config.files, ['stdout', 'stderr', 'receipt']);
  const observed = {}, raw = {};
  let directory;
  for (const [name, maximum] of [['stdout', 1 << 20], ['stderr', 16384], ['receipt', 1 << 20]]) {
    const row = config.files[name]; exact(row, ['path', 'bytes', 'sha256']); assert.match(row.sha256, /^[0-9a-f]{64}$/);
    integer(row.bytes); assert(row.bytes > 0 && row.bytes <= maximum);
    const path = contained(config.root, row.path), info = lstatSync(path);
    assert(info.isFile() && info.size === row.bytes, 'original observation type/size changed');
    assert.equal(basename(path), `process-${config.runId}.${name === 'receipt' ? 'receipt.json' : name + '.bin'}`);
    directory ??= dirname(path); assert.equal(dirname(path), directory, 'same original run evidence required');
    assert.equal(hashFile(path, maximum), row.sha256, 'original observation hash changed');
    const body = readFileSync(path); assert.equal(body.length, row.bytes); assert.equal(sha256(body), row.sha256);
    raw[name] = body; observed[name] = {...row, path};
  }
  assert.equal(dirname(directory), join(resolve(config.root), '.tmp/rust-disk/evidence'), 'original owner evidence directory required');
  const proof = JSON.parse(raw.receipt);
  assert.equal(proof.runId, config.runId); assert.equal(proof.exitCode, 0);
  assert.equal(resolve(proof.binary), resolve(artifact.sourceBinary), 'native executable/artifact mismatch');
  const timing = elapsed(proof);
  const validated = verifyPackProfile(raw.stderr, JSON.parse(raw.stdout), config);
  assert.equal(hashFile(config.artifact.path, 65536), config.artifact.sha256); verifyArtifact(config.artifact.root, config.artifact.path);
  for (const row of Object.values(observed)) assert.equal(hashFile(row.path, row.bytes), row.sha256, 'original observation changed during validation');
  return {schema: 'lycheedev.source-pack-retained-observation.v1', ...validated, runId: config.runId, artifact: {path: config.artifact.path, sha256: config.artifact.sha256, binarySHA256: artifact.binarySHA256, sourceTreeSHA256: artifact.inputHashes?.sourceTree}, files: observed, timing, productCalls: 0, originalFailurePreserved: true, importedSamples: 0, verificationScope: 'pinned raw observations, current sealed artifact bytes, and native receipt timing only', historicalLaunchIdentity: 'not_independently_reauthenticated', wrapperOutcome: 'not_evaluated', completeInvocationVerified: false};
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const [configPath, ...extra] = process.argv.slice(2);
  assert(configPath && !extra.length, 'usage: node tests/rust-performance-paired/source-pack-resume.mjs <retained-observation-config.json>');
  const info = lstatSync(configPath); assert(info.isFile() && !info.isSymbolicLink() && info.size <= 65536, 'bounded JSON configuration required');
  console.log(JSON.stringify(verifyRetainedPackObservation(JSON.parse(readFileSync(configPath))), null, 2));
}
