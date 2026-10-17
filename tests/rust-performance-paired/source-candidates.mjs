// Fixed, diagnostic-only bbfe/ab4 comparison. No builds or acceptance changes.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {existsSync, lstatSync, readFileSync, readdirSync, realpathSync, writeFileSync} from 'node:fs';
import {isAbsolute, join, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {performance} from 'node:perf_hooks';
import os from 'node:os';
import {hashFile, sourceDigest, verifyArtifact} from '../../tools/rust-build.mjs';
import {beginScratch, contained} from '../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';
import {collectOwnedMemory} from '../../tools/rust-owned-memory.mjs';
import {inspectEmmy} from '../../tools/rust-emmy-identity.mjs';
import {assertCorpus, assertEnvelope, sha256} from '../rust-source-full-corpus/assertions.mjs';
import {assertSeedCold, auditCorpus, cloneSeed, verifySeedMetadata} from './inputs.mjs';
import {elapsed, semantic, statistics} from './contract.mjs';
import {readRustFact} from './read-fact-delta.mjs';

export const PAIRS = 3;
export const OUTER_TIMEOUT_MS = 3600000;
export const COMMITS = Object.freeze({baseline: 'bbfe88265ee431b9ad925ab86dc52561742dd5ee', candidate: 'ab4fc03605a08b544476742d07a3af08e4cc7ce9'});
export const CANDIDATE_IDENTITIES = Object.freeze({
  baseline: Object.freeze({artifactSHA256: '43429c85cac9e0a3bb130c5de4d939e19a8219dca81001d426ae9a1c9ebd0d4e', binarySHA256: 'f25d5fbe4e43eb0ae2f28e310f28e4de51ab6be701d2919f94729132c76a39e6', sourceTreeSHA256: '76bea4b27798356c9f5194f56c97c3a1292b214166b84c875dfc0bb75dfc7b34'}),
  candidate: Object.freeze({artifactSHA256: '280c42204696a00d2a8e134347c726282c818e6353083c22a7d6737cf17a8450', binarySHA256: '3b124671486c476e55fabf8358d737ebe541005111585837f6ce03a609f95e47', sourceTreeSHA256: '9b45e843f8f4759e729a5574a6af4d5bca6a37bbfefaea9b7f6c03c6e65d71cc'}),
});
export const PROFILE_SOURCE_SHA256 = 'a47cd78ffc979b20ce4a9d94974dac5ee0022b2ce9302eb910066da781062989';
export const PHASE_NAMES = Object.freeze(['metadata', 'cached_index', 'index_lock', 'git_tree', 'git_blobs_inclusive', 'facts_read', 'facts_parse', 'facts_serialize', 'facts_publish', 'original_publish', 'symbols', 'index_serialize', 'index_publish', 'manifest', 'facts_worker_consumed', 'facts_worker_wait', 'facts_refill_dispatched', 'facts_publish_lock', 'facts_publish_stage', 'facts_publish_write', 'facts_publish_seal', 'facts_publish_rename', 'facts_publish_stage_close', 'facts_publish_lock_close', 'facts_parse_sequential', 'facts_worker_abandoned', 'facts_publish_lock_verify', 'facts_publish_lock_open', 'facts_publish_lock_kernel_attempt', 'facts_publish_lock_contention_backoff', 'facts_publish_lock_failure_close']);
export const PHASE_TIMING = 'wall-clock; worker phase sums consumed job durations; phases overlap; not additive';
export const LOCK_TIMING = 'verify/open/kernel_attempt/contention_backoff/failure_close are nested in facts_publish_lock; not additive with their parent. kernel_attempt counts every kernel try, including immediate success; contention_backoff measures actual sleep calls only after real contention, including scheduler delay. failure_close measures only failed-acquisition handle close; successful close remains facts_publish_lock_close. Verification includes the unchanged controlled ancestry checks; lock preparation and other checks remain in the parent.';
export const RUNNER_FILES = Object.freeze([
  'tools/rust-source-candidate-diagnostics.mjs', 'tests/rust-performance-paired/source-candidates.mjs',
  'tests/rust-performance-paired/read-fact-delta.mjs', 'tests/rust-performance-paired/inputs.mjs',
  'tests/rust-performance-paired/contract.mjs', 'tests/rust-source-full-corpus/assertions.mjs',
  'tests/rust-source-full-corpus/retail-oracle.json', 'tests/rust-parity/describe-contract.mjs', 'tests/rust-parity/oracle.mjs',
  'tools/rust-build.mjs', 'tools/rust-disk.mjs', 'tools/rust-emmy-identity.mjs',
  'tools/rust-owned-process.mjs', 'tools/rust-owned-process.ps1', 'tools/rust-owned-memory.mjs',
  'tools/rust-build-auxiliary.json', 'tools/zip.mjs', 'release/rust-disk-policy.json',
]);
const SIDES = Object.freeze(['baseline', 'candidate']);
const SECTIONS = Object.freeze(['declarations', 'relationships', 'loads', 'headers', 'diagnostics']);
export const orderForCandidatePair = iteration => iteration % 2 ? ['candidate', 'baseline'] : ['baseline', 'candidate'];
const keys = (value, expected, label) => {
  assert(value && typeof value === 'object' && !Array.isArray(value), label);
  assert.deepEqual(Object.keys(value).sort(), [...expected].sort(), `${label}: exact fields required`);
};
export function validateRequest(value) {
  keys(value, ['schema', 'ownerRoot', ...SIDES], 'configuration');
  assert.equal(value.schema, 'lycheedev.source-candidate-diagnostics-config.v1');
  assert(isAbsolute(value.ownerRoot), 'absolute ownerRoot required');
  const result = {schema: value.schema, ownerRoot: realpathSync(value.ownerRoot)};
  for (const side of SIDES) {
    const input = value[side];
    keys(input, ['sourceRoot', 'artifactRoot', 'artifact', 'commit', 'artifactSHA256', 'binarySHA256', 'sourceTreeSHA256'], side);
    assert.equal(input.commit, COMMITS[side], `${side}: fixed candidate commit required`);
    for (const key of ['sourceRoot', 'artifactRoot', 'artifact']) assert(isAbsolute(input[key]), `${side}.${key}: absolute path required`);
    for (const key of ['artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']) assert.match(input[key], /^[0-9a-f]{64}$/, `${side}.${key}`);
    for (const key of ['artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']) assert.equal(input[key], CANDIDATE_IDENTITIES[side][key], `${side}: fixed sealed ${key} required`);
    result[side] = {...input, sourceRoot: realpathSync(input.sourceRoot), artifactRoot: realpathSync(input.artifactRoot)};
    result[side].artifact = contained(result[side].artifactRoot, input.artifact);
  }
  assert.notEqual(result.baseline.sourceRoot, result.candidate.sourceRoot, 'independent fixed source roots required');
  assert.notEqual(result.baseline.binarySHA256, result.candidate.binarySHA256, 'distinct candidate binaries required');
  return result;
}
export function validateConditions(value) {
  for (const field of ['machine', 'powerAndThermal', 'competingLoad', 'disk', 'operator']) assert(typeof value?.[field] === 'string' && value[field].trim(), `conditions.${field}: disclose unknowns explicitly`);
  return value;
}
export function sourcePhaseEnvironment(environment, measured = false) {
  assert(!Object.keys(environment).some(key => ['RUST_MIN_STACK', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS'].includes(key.toUpperCase()) && environment[key]), 'default stack and unchanged compiler environment required');
  const result = Object.fromEntries(Object.entries(environment).filter(([key]) => key.toUpperCase() !== 'LYCHEEDEV_SOURCE_PHASE_PROFILE'));
  if (measured) result.LYCHEEDEV_SOURCE_PHASE_PROFILE = '1';
  return result;
}
export function sourcePhaseProfile(stderr) {
  assert(Buffer.isBuffer(stderr) && stderr.length > 0 && stderr.length <= 16384, 'missing or oversized Source profile');
  const text = new TextDecoder('utf-8', {fatal: true, ignoreBOM: true}).decode(stderr);
  const line = text.endsWith('\r\n') ? text.slice(0, -2) : text.endsWith('\n') ? text.slice(0, -1) : text;
  assert(line.startsWith('{') && line.endsWith('}') && !/[\r\n]/.test(line), 'exactly one Source JSON record required; unknown stderr rejected');
  const value = JSON.parse(line);
  assert.equal(JSON.stringify(value), line, 'exact compact Source JSON without duplicate keys required');
  keys(value, ['schema', 'timing', 'lockTiming', 'elapsedMicros', 'phases'], 'Source profile');
  assert.equal(value.schema, 'lycheedev.source-index-phases.v1');
  assert.equal(value.timing, PHASE_TIMING); assert.equal(value.lockTiming, LOCK_TIMING);
  const integer = value => assert(Number.isSafeInteger(value) && value >= 0, 'nonnegative exact integer required');
  integer(value.elapsedMicros);
  assert(Array.isArray(value.phases)); assert.equal(value.phases.length, PHASE_NAMES.length);
  value.phases.forEach((phase, index) => {
    keys(phase, ['phase', 'calls', 'elapsedMicros'], 'Source phase');
    assert.equal(phase.phase, PHASE_NAMES[index]); integer(phase.calls); integer(phase.elapsedMicros);
    if (phase.calls === 0) assert.equal(phase.elapsedMicros, 0);
  });
  for (const name of ['metadata', 'git_tree', 'git_blobs_inclusive', 'facts_read', 'facts_parse', 'facts_serialize', 'facts_publish', 'original_publish', 'symbols', 'index_serialize', 'index_publish', 'manifest']) {
    assert(value.phases.find(phase => phase.phase === name).calls > 0, `missing successful cold-index phase ${name}`);
  }
  return value;
}
export function verifyColdProfile(profile, envelope, oracle) {
  const io = envelope.result?.io;
  assert(io && (io.cachedIndex === undefined || io.cachedIndex === false) && io.cachedFacts === 0 && io.parsedDocuments === oracle.documents, 'actual cold parse required');
  const calls = Object.fromEntries(profile.phases.map(phase => [phase.phase, phase.calls]));
  for (const name of ['facts_read', 'facts_parse', 'facts_serialize', 'facts_publish']) assert.equal(calls[name], io.parsedDocuments, `${name}: complete cold denominator`);
  assert.equal(calls.facts_worker_wait, calls.facts_worker_consumed, 'worker wait/consumed calls');
  assert(calls.facts_worker_abandoned <= calls.facts_worker_consumed, 'abandoned workers are consumed jobs');
  assert.equal(calls.facts_parse_sequential + calls.facts_worker_consumed - calls.facts_worker_abandoned, io.parsedDocuments, 'sequential/worker/abandoned cold denominator');
  return profile;
}
export function verifyCandidateRecord(input, artifact) {
  const side = SIDES.find(side => input.commit === COMMITS[side]);
  assert(side, 'fixed candidate commit required');
  for (const key of ['binarySHA256', 'sourceTreeSHA256']) assert.equal(input[key], CANDIDATE_IDENTITIES[side][key], `fixed sealed ${key} required`);
  assert(['release', 'symbols'].includes(artifact.profile), 'sealed optimized artifact required');
  assert.equal(artifact.sameInvocation, true, 'same-invocation artifact required');
  if (artifact.profile === 'symbols') assert(artifact.pdb, 'symbols artifact requires matching PDB');
  assert.equal(artifact.binarySHA256, input.binarySHA256, 'candidate EXE identity');
  assert.equal(artifact.inputHashes?.sourceTree, input.sourceTreeSHA256, 'fixed sealed source identity');
  assert(isAbsolute(artifact.sourceRoot), 'original artifact sourceRoot provenance required');
}
export function runnerIdentity(root) {
  return Object.fromEntries(RUNNER_FILES.map(file => [file, hashFile(join(root, file))]));
}
export function verifyCandidate(input, git) {
  assert.equal(hashFile(input.artifact, 65536), input.artifactSHA256, 'artifact record changed');
  const artifact = verifyArtifact(input.artifactRoot, input.artifact);
  verifyCandidateRecord(input, artifact);
  assert.equal(realpathSync(git(['rev-parse', '--show-toplevel'], undefined, input.sourceRoot).toString().trim()), input.sourceRoot, 'source root is not checkout root');
  assert.equal(git(['rev-parse', 'HEAD'], undefined, input.sourceRoot).toString().trim(), input.commit, 'candidate checkout commit');
  assert.equal(git(['status', '--porcelain', '--untracked-files=all'], undefined, input.sourceRoot).toString().trim(), '', 'candidate checkout must be clean');
  assert.equal(sourceDigest(input.sourceRoot), artifact.inputHashes.sourceTree, 'source/build identity mismatch');
  const emmy = inspectEmmy(input.sourceRoot);
  assert.equal(emmy.identity, 'public-upstream-unmodified-v1');
  assert.equal(emmy.patchedTreeSHA256, artifact.inputHashes.emmy?.patchedTreeSHA256);
  const channel = JSON.parse(readFileSync(join(input.sourceRoot, 'release/npm-channel.json')));
  assert.equal(channel.version, '3.5.0'); assert.equal(channel.distTag, 'beta');
  assert.equal(hashFile(join(input.sourceRoot, 'crates/lycheedev-source/src/index/profile.rs')), PROFILE_SOURCE_SHA256, 'fixed 31-phase source identity');
  return {...input, artifact, emmy: {identity: emmy.identity, baseCommit: emmy.base.commit, treeSHA256: emmy.patchedTreeSHA256}};
}
export function compareCandidateFacts(documents, homes, summaries, oracle) {
  assert.equal(documents.length, oracle.documents);
  assert.equal(new Set(documents.map(doc => doc.path)).size, documents.length, 'duplicate document');
  const totals = Object.fromEntries(SIDES.map(side => [side, Object.fromEntries(SECTIONS.map(section => [section, 0]))]));
  const rows = [];
  for (const document of documents) {
    const facts = Object.fromEntries(SIDES.map(side => [side, readRustFact(homes[side], document, 'v10')]));
    for (const side of SIDES) {
      keys(facts[side].facts, SECTIONS, `${side} facts sections`);
      for (const section of SECTIONS) totals[side][section] += facts[side].facts[section].length;
    }
    assert.deepEqual(facts.candidate.facts, facts.baseline.facts, `complete Facts fields/order differ: ${document.path}`);
    rows.push({path: document.path, inputSHA256: document.sha256, baselineFactSHA256: facts.baseline.fileSHA256, candidateFactSHA256: facts.candidate.fileSHA256, factsSHA256: facts.baseline.factsSHA256});
  }
  for (const side of SIDES) for (const section of ['declarations', 'relationships', 'diagnostics']) assert.equal(totals[side][section], summaries[side][section], `${side} complete ${section} sum differs from index`);
  return {schema: 'lycheedev.source-candidate-facts.v1', layout: 'v10', documents: documents.length, fullFieldsAndOrderEqual: true, totals, rows};
}
export function compareCandidateSummaries(outputs, oracle, pin) {
  const values = SIDES.map(side => {
    semantic('B02-retail-index', outputs[side], 'rust', oracle, pin);
    const value = structuredClone(outputs[side].result);
    if (value.io) delete value.io.elapsedMs; // Only the measured duration is nondeterministic.
    return value;
  });
  assert.deepEqual(values[0], values[1], 'complete public index summary differs');
  for (const side of SIDES) {
    const io = outputs[side].result.io;
    assert.equal(io?.deadlineMs, 180000, `${side}: unchanged product index budget`);
    assert(io && (io.cachedIndex === undefined || io.cachedIndex === false) && io.parsedDocuments === oracle.documents && io.cachedFacts === 0, `${side}: actual cold parse required`);
  }
  return Object.fromEntries(SIDES.map((side, index) => [side, values[index]]));
}
export function verifyObservedReport(run, observed) {
  assert(run.handlesReaped === true && run.status === 0 && !run.error, 'outer owned invocation failed');
  elapsed(run.proof); // A killed or incomplete outer Job is never a successful diagnostic.
  assert.equal(observed?.state, 'source_candidates_observed');
  assert.equal(observed.performanceAcceptanceEvaluated, false);
  assert.equal(observed.fullB01B07Acceptance, false);
  assert.equal(observed.finalIntegrityVerified, true);
  assert.equal(observed.requiresOuterRetirement, false);
  assert.equal(observed.requiredPairs, PAIRS);
  assert.equal(observed.pairs?.length, PAIRS);
  const homes = new Set();
  for (const [iteration, row] of observed.pairs.entries()) {
    assert.equal(row.iteration, iteration); assert.deepEqual(row.order, orderForCandidatePair(iteration));
    assert.equal(row.cacheMode, 'cold'); assert.equal(row.pairTerminal, 'verified');
    for (const key of ['resultEquivalent', 'factsEquivalent', 'profilesVerified']) assert.equal(row[key], true);
    assert.equal(row.facts.documents, 4036);
    for (const side of SIDES) { assert.equal(row[side].valid, true); assert.equal(typeof row[side].home, 'string'); homes.add(row[side].home); elapsed(row[side].proof); }
  }
  assert.equal(homes.size, PAIRS * 2, 'six independent cold homes required');
  return true;
}
export const evidenceFiles = directory => readdirSync(directory).filter(name => name !== 'receipt.json' && lstatSync(join(directory, name)).isFile()).map(name => join(directory, name));

// Dependency injection is only for deterministic orchestration/fault fixtures;
// the CLI uses all real implementations and admits no override configuration.
export function runCandidateController(config, overrides = {}) {
  const services = {beginScratch, ownedSpawnSync, collectOwnedMemory, verifyCandidate, compareCandidateFacts, runnerIdentity, ...overrides};
  const {request, runnerRoot, inputs, conditions, scratch, evidence: out} = config;
  const oracle = JSON.parse(readFileSync(join(runnerRoot, 'tests/rust-source-full-corpus/retail-oracle.json')));
  assert.equal(oracle.commit, '31c7f7b9cc79e56c986b365c06a6afbcf3c9177b'); assert.equal(oracle.documents, 4036);
  let fatal = false, sequence = 0;
  const start = performance.now();
  const report = {schema: 'lycheedev.source-candidate-diagnostics.v1', state: 'running', performanceAcceptanceEvaluated: false, fullB01B07Acceptance: false, requiredPairs: PAIRS, startedAt: new Date().toISOString(), request, runnerRoot, inputs, conditions, oracle,
    machine: {hostname: os.hostname(), platform: os.platform(), arch: os.arch(), cpus: os.cpus(), totalMemory: os.totalmem(), node: process.version},
    timing: 'Original native CreateProcessW through all owned descendants reaped and output drained; no phase subtraction. Memory remains sampled lower-bound evidence, not complete RSS.',
    cacheDefinition: 'Independent application-derived cold homes from each candidate own authenticated init/sync-only seed; no OS cache flush or OS-cold claim.',
    preparation: [], pairs: [], pairOwners: [], identities: {}, finalIntegrityVerified: false};
  const save = () => writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2) + '\n');
  function controlled(binary, args, cwd, evidence, measured = false, timeout = 210000) {
    assert(!fatal, 'unverified process retirement; no further launch');
    fatal = true;
    const startedAt = new Date().toISOString(), at = performance.now();
    const run = services.ownedSpawnSync(binary, args, {cwd, env: sourcePhaseEnvironment(process.env, measured), evidence, timeout, orphanMs: 30000, memoryObservation: measured});
    fatal = run.handlesReaped !== true;
    const row = {binary, args, cwd, startedAt, endedAt: new Date().toISOString(), wrapperWallMillis: performance.now() - at, exit: run.status, error: run.error?.message, handlesReaped: run.handlesReaped, proof: run.proof, logs: run.logs, stdoutSHA256: sha256(run.stdout), stderrSHA256: sha256(run.stderr), valid: false};
    return {run, row};
  }
  function prepare(binary, args, cwd, label, timeout) {
    const {run, row} = controlled(binary, args, cwd, out, false, timeout);
    report.preparation.push({...row, label}); save();
    assert(run.handlesReaped && !run.error && run.status === 0, `${label}: ${run.error?.message ?? run.stderr.toString()}`);
    return run.stdout;
  }
  const gitEnv = Object.fromEntries(Object.entries(sourcePhaseEnvironment(process.env)).filter(([key]) => !key.toUpperCase().startsWith('GIT_') && key.toUpperCase() !== 'GCM_INTERACTIVE'));
  Object.assign(gitEnv, {GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: 'NUL', GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_NO_REPLACE_OBJECTS: '1'});
  function git(args, input, cwd = request.ownerRoot) {
    assert(!fatal);
    const run = spawnSync('git', ['--no-optional-locks', '-c', 'core.fsmonitor=false', ...args], {input, cwd, env: gitEnv, timeout: 60000, maxBuffer: 16 << 20, windowsHide: true, shell: false});
    const stdout = run.stdout ?? Buffer.alloc(0), stderr = run.stderr ?? Buffer.alloc(0), prefix = join(out, `git-audit-${sequence++}`);
    writeFileSync(`${prefix}.stdout.bin`, stdout); writeFileSync(`${prefix}.stderr.bin`, stderr);
    report.preparation.push({label: 'authenticated-git-read', cwd, args, inputSHA256: input === undefined ? null : sha256(Buffer.from(input)), exit: run.status, error: run.error?.message, logs: [`${prefix}.stdout.bin`, `${prefix}.stderr.bin`], stdoutSHA256: sha256(stdout), stderrSHA256: sha256(stderr)});
    if (run.error || run.signal) fatal = true;
    save(); assert(!run.error && run.status === 0, 'authenticated Git read failed'); return stdout;
  }
  function identities() {
    assert.deepEqual(services.runnerIdentity(runnerRoot), inputs, 'runner bytes changed');
    return Object.fromEntries(SIDES.map(side => [side, services.verifyCandidate(request[side], git)]));
  }
  function prepareSeeds(candidates) {
    const seeds = {}, pins = {}, identities = {}, manifests = {};
    for (const side of SIDES) {
      const home = join(scratch, `${side}-sync-only-seed`), candidate = candidates[side];
      assert(!existsSync(home));
      assertEnvelope({exit: 0, envelope: JSON.parse(prepare(candidate.artifact.sourceBinary, ['init', '--home', home, '--format', 'json'], candidate.sourceRoot, `${side}-init`))});
      const pin = assertEnvelope({exit: 0, envelope: JSON.parse(prepare(candidate.artifact.sourceBinary, ['source', 'sync', '--source', oracle.repository, '--product', oracle.product, '--ref', oracle.commit, '--home', home, '--format', 'json'], candidate.sourceRoot, `${side}-sync`, 240000))});
      assert.deepEqual({repository: pin.source.repository, product: pin.source.product, commit: pin.source.exactCommit, requestedRef: pin.source.requestedRef}, {repository: oracle.repository, product: oracle.product, commit: oracle.commit, requestedRef: oracle.commit});
      assert.match(pin.id, /^PIN-/);
      const corpus = auditCorpus(join(home, 'mirrors', `${oracle.repository}.git`), oracle, git, out, `${side}-`);
      const metadata = verifySeedMetadata(home, pin, join(out, `${side}-seed-metadata.sqlite`));
      seeds[side] = home; pins[side] = pin; identities[side] = assertSeedCold(home);
      manifests[side] = JSON.parse(readFileSync(join(out, `${side}-verified-corpus-manifest.json`))).files;
      report.preparation.push({label: `${side}-authenticated-seed`, home, pin, corpus, metadata, seedSHA256: identities[side].sha256}); save();
    }
    assert.deepEqual(pins.baseline, pins.candidate, 'independent public pins differ');
    assert.deepEqual(manifests.baseline, manifests.candidate, 'candidate corpus bytes differ');
    return {seeds, pins, identities, documents: assertCorpus(manifests.baseline, oracle)};
  }
  save();
  try {
    const candidates = identities(); report.identities = candidates; save();
    report.machine.powerBefore = prepare('powercfg.exe', ['/getactivescheme'], request.ownerRoot, 'power-before').toString().trim();
    const prepared = (services.prepareSeeds ?? prepareSeeds)(candidates, report);
    for (let iteration = 0; iteration < PAIRS; iteration++) {
      identities();
      const pair = services.beginScratch(request.ownerRoot, `source-candidates-${iteration}`, {command: ['source', 'index'], inputs: {baseline: request.baseline.binarySHA256, candidate: request.candidate.binarySHA256, corpus: oracle.documentPathSHA256SizeDigest}, admissionOptions: {requiredBytes: 512 << 20, scratchReserveBytes: 512 << 20}});
      const row = {iteration, cacheMode: 'cold', order: orderForCandidatePair(iteration), resultEquivalent: false, factsEquivalent: false, profilesVerified: false, pairTerminal: 'running'};
      report.pairs.push(row); report.pairOwners.push({iteration, receipt: pair.receipt}); save();
      let valid = false;
      try {
        const homes = Object.fromEntries(SIDES.map(side => [side, join(pair.directory, `${side}-home`)]));
        for (const side of SIDES) (services.cloneSeed ?? cloneSeed)(prepared.seeds[side], homes[side], prepared.identities[side]);
        const outputs = {};
        // No preparation, corpus/Facts audit or source identity scan occurs
        // between these two measured invocations. Both homes already exist.
        for (const side of row.order) {
          const candidate = candidates[side];
          const output = controlled(candidate.artifact.sourceBinary, ['source', 'index', '--snapshot', prepared.pins[side].id, '--home', homes[side], '--format', 'json'], candidate.sourceRoot, pair.evidence, true);
          Object.assign(output.row, {side, iteration, home: homes[side], snapshot: prepared.pins[side].id, cacheMode: 'cold'});
          row[side] = output.row; outputs[side] = output; save();
          assert(!fatal, 'unverified process retirement; paired invocation stopped');
        }
        const envelopes = {}, failures = [];
        for (const side of SIDES) {
          const {run, row: sample} = outputs[side];
          try {
            Object.assign(sample, elapsed(run.proof));
            sample.memory = services.collectOwnedMemory(run, {evidence: pair.evidence, sourceBinary: sample.binary, runState: 'passed_invocation'});
            assert(run.status === 0 && !run.error && run.handlesReaped, `${side}: product failure`);
            sample.sourceProfile = sourcePhaseProfile(run.stderr);
            envelopes[side] = JSON.parse(run.stdout);
            verifyColdProfile(sample.sourceProfile, envelopes[side], oracle);
            sample.valid = true;
          } catch (error) { sample.validationFailure = error.message.slice(0, 4096); failures.push(`${side}: ${sample.validationFailure}`); }
        }
        assert.equal(failures.length, 0, failures.join('; '));
        row.profilesVerified = true;
        const summaries = compareCandidateSummaries(envelopes, oracle, prepared.pins.baseline.id);
        row.resultEquivalent = true;
        const facts = services.compareCandidateFacts(prepared.documents, homes, summaries, oracle);
        const factsPath = join(pair.evidence, 'facts-comparison.json'); writeFileSync(factsPath, JSON.stringify(facts, null, 2) + '\n');
        row.facts = {path: factsPath, sha256: hashFile(factsPath), documents: facts.documents, totals: facts.totals}; row.factsEquivalent = true;
        identities(); // A late identity failure must retain this pair's homes.
        row.candidateToBaselineRatio = row.candidate.controllableMillis / row.baseline.controllableMillis;
        valid = true;
      } catch (error) { row.failure = error.message.slice(0, 4096); }
      finally {
        row.pairTerminal = fatal ? 'interrupted' : valid ? 'verified' : 'failed';
        writeFileSync(join(pair.evidence, 'pair.json'), JSON.stringify(row, null, 2) + '\n');
        pair.seal({state: fatal ? 'interrupted' : valid ? 'succeeded' : 'failed', handlesReaped: !fatal, logs: evidenceFiles(pair.evidence)});
        if (valid && !fatal) pair.cleanup();
        save();
      }
      if (!valid) break; // Never replace failed/missing/slow samples.
    }
    assert(!fatal, 'unverified process retirement; outer owner retained');
    identities();
    report.machine.powerAfter = prepare('powercfg.exe', ['/getactivescheme'], request.ownerRoot, 'power-after').toString().trim();
    assert.equal(report.machine.powerAfter, report.machine.powerBefore, 'power policy changed');
    report.finalIntegrityVerified = true;
    const complete = report.pairs.length === PAIRS && report.pairs.every(row => row.pairTerminal === 'verified');
    if (complete) {
      const timings = Object.fromEntries(SIDES.map(side => [side, statistics(report.pairs.map(row => row[side].controllableMillis))]));
      report.observation = {timings, ratioOfMedians: timings.candidate.median / timings.baseline.median, pairedRatios: statistics(report.pairs.map(row => row.candidateToBaselineRatio)), qualification: 'Three adjacent alternating diagnostic pairs only; not the 20-pair Go performance acceptance; all original command time remains included.'};
    }
    report.state = complete ? 'source_candidates_observed' : 'source_candidates_failed';
  } catch (error) { report.blocker = error.message.slice(0, 4096); report.state = 'source_candidates_failed'; }
  finally { report.requiresOuterRetirement = fatal; report.finishedAt = new Date().toISOString(); report.wallMillis = performance.now() - start; save(); }
  return report;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.platform, 'win32', 'owned Windows controller required');
  assert.equal(process.argv.length, 3);
  const config = JSON.parse(readFileSync(process.argv[2]));
  config.request = validateRequest(config.request); validateConditions(config.conditions);
  const report = runCandidateController(config);
  console.log(JSON.stringify({state: report.state, report: join(config.evidence, 'report.json'), observation: report.observation}));
  if (report.state !== 'source_candidates_observed') process.exitCode = 1;
}
