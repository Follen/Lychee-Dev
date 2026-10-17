import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {sha256, rustFactPath} from './read-fact-delta.mjs';
import {PAIRS, OUTER_TIMEOUT_MS, verifyObservedReport, COMMITS, CANDIDATE_IDENTITIES, PROFILE_SOURCE_SHA256, PHASE_NAMES, PHASE_TIMING, LOCK_TIMING, RUNNER_FILES, orderForCandidatePair, validateRequest, validateConditions, sourcePhaseEnvironment, sourcePhaseProfile, verifyColdProfile, verifyCandidateRecord, compareCandidateFacts, compareCandidateSummaries, runCandidateController, runnerIdentity} from './source-candidates.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const oracle = JSON.parse(fs.readFileSync(path.join(root, 'tests/rust-source-full-corpus/retail-oracle.json')));
const sides = ['baseline', 'candidate'];
const sections = ['declarations', 'relationships', 'loads', 'headers', 'diagnostics'];
const sorted = value => Array.isArray(value) ? value.map(sorted) : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(key => [key, sorted(value[key])])) : value;
const conditions = {machine: 'fixture', powerAndThermal: 'unknown fixture', competingLoad: 'none simulated', disk: 'temporary fixture', operator: 'Node fixture only'};
function temporary(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'source-candidates-test-'));
  t.after(() => { if (t.passed) fs.rmSync(directory, {recursive: true}); else t.diagnostic(`Fixture retained: ${directory}`); });
  return directory;
}
function requestAt(directory) {
  const request = {schema: 'lycheedev.source-candidate-diagnostics-config.v1', ownerRoot: directory};
  const artifactRoot = path.join(directory, 'historical-artifact-owner'); fs.mkdirSync(artifactRoot);
  for (const side of sides) {
    const sourceRoot = path.join(directory, `${side}-source`); fs.mkdirSync(sourceRoot);
    const artifact = path.join(artifactRoot, `${side}.json`); fs.writeFileSync(artifact, '{}');
    request[side] = {sourceRoot, artifactRoot, artifact, commit: COMMITS[side], artifactSHA256: sha256('{}'), ...CANDIDATE_IDENTITIES[side]};
  }
  return request;
}
function envelope() {
  return {schema: 'lycheedev.result.v1', ok: true, error: null, result: {schema: 'lycheedev.source-map.v2', storage: 'file-cache', parser: 'source-v1', repository: oracle.repository, product: oracle.product, commit: oracle.commit, documents: 4036, declarations: 65320, relationships: 227786, assets: 4, diagnostics: 0, complete: true, io: {parsedDocuments: 4036, cachedFacts: 0, deadlineMs: 180000, elapsedMs: 1, readBytes: 32269051, gitBlobProcesses: 1, gitTreeProcesses: 1}}};
}
function profile() {
  const calls = {metadata: 1, git_tree: 1, git_blobs_inclusive: 1, facts_read: 4036, facts_parse: 4036, facts_serialize: 4036, facts_publish: 4036, original_publish: 1, symbols: 1, index_serialize: 1, index_publish: 1, manifest: 1, facts_worker_consumed: 1000, facts_worker_wait: 1000, facts_parse_sequential: 3040, facts_worker_abandoned: 4};
  return {schema: 'lycheedev.source-index-phases.v1', elapsedMicros: 100000, timing: PHASE_TIMING, lockTiming: LOCK_TIMING, phases: PHASE_NAMES.map(phase => ({phase, calls: calls[phase] ?? 0, elapsedMicros: calls[phase] ?? 0}))};
}
const bytes = value => Buffer.from(JSON.stringify(value) + '\n');
function proof(millis = 10) {
  return {schema: 'lycheedev.owned-process.v1', rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0, forced: false, timing: {schema: 'lycheedev.owned-timing.v1', frequency: 1000000, startTicks: '1000000', rootExitTicks: String(1000000 + millis * 1000 - 100), treeReapedTicks: String(1000000 + millis * 1000)}};
}
function factsPayload() {
  return {declarations: [{name: 'F', category: 'function', line: 1, endLine: 2, scope: '<file>'}], relationships: [{from: 'F', to: 'Raw<&>Target', category: 'call', confidence: 'inferred', line: 2}, {from: 'F', to: 'Raw<&>Target', category: 'call', confidence: 'inferred', line: 2}], loads: [{path: 'a.lua', line: 1, kind: 'script'}], headers: [{key: 'Interface', value: '120100', line: 1}], diagnostics: [{code: 'fixture', message: 'fixture', line: 3}]};
}
function writeFact(home, document, facts) {
  const file = rustFactPath(home, document, 'v10'); fs.mkdirSync(path.dirname(file), {recursive: true});
  fs.writeFileSync(file, JSON.stringify({schema: 'lycheedev.source-facts.rust.v10', inputSHA256: document.sha256, factsSHA256: sha256(JSON.stringify(sorted(facts))), facts}));
}
function factsFixture(t, count = 4) {
  const directory = temporary(t), homes = Object.fromEntries(sides.map(side => [side, path.join(directory, side)]));
  const names = ['a.lua', 'Blizzard_APIDocumentationGenerated/a.lua', 'a.xml', 'a.toc'];
  const documents = Array.from({length: count}, (_, i) => ({path: `Interface/AddOns/${i}/${names[i % 4]}`, sha256: sha256(`source-${i}`)}));
  for (const document of documents) for (const side of sides) writeFact(homes[side], document, factsPayload());
  const summaries = Object.fromEntries(sides.map(side => [side, {declarations: count, relationships: count * 2, diagnostics: count}]));
  return {homes, documents, summaries, oracle: {documents: count}, compare() { return compareCandidateFacts(documents, homes, summaries, this.oracle); }};
}

// The real controller's exact scheduler and owner/failure branches are exercised;
// only process, disk-owner and input preparation boundaries are fixtures.
function controllerFixture(t, faults = {}) {
  const directory = temporary(t), request = requestAt(directory), evidence = path.join(directory, 'evidence'), scratch = path.join(directory, 'scratch');
  fs.mkdirSync(evidence); fs.mkdirSync(scratch);
  const events = [], owners = [], invocations = [], inputs = {'fixture-runner': 'fixed'};
  let identityCalls = 0, measured = 0;
  const services = {
    runnerIdentity() { events.push('identity'); return inputs; },
    verifyCandidate(input) {
      identityCalls++;
      if (faults.identityAt === identityCalls) throw new Error('late source identity drift');
      return {...input, artifact: {sourceBinary: path.join(input.artifactRoot, `${input.commit}.exe`)}};
    },
    prepareSeeds() { events.push('prepare-seeds'); return {seeds: {baseline: 'baseline-seed', candidate: 'candidate-seed'}, identities: {baseline: {}, candidate: {}}, pins: {baseline: {id: 'PIN-fixture'}, candidate: {id: 'PIN-fixture'}}, documents: []}; },
    cloneSeed(seed, home) { events.push(`clone:${seed}`); assert(!fs.existsSync(home)); fs.mkdirSync(home); },
    beginScratch(_root, name) {
      events.push(`owner:${name}`); const directory = path.join(scratch, name), evidence = path.join(directory, 'evidence'); fs.mkdirSync(evidence, {recursive: true});
      const owner = {directory, evidence, receipt: {name}, sealed: null, cleaned: false, seal(value) { this.sealed = value; events.push('seal'); }, cleanup() { assert.equal(this.sealed.state, 'succeeded'); this.cleaned = true; events.push('cleanup'); }}; owners.push(owner); return owner;
    },
    ownedSpawnSync(binary, args, options) {
      if (binary === 'powercfg.exe') { events.push('power'); return {status: 0, handlesReaped: true, stdout: Buffer.from('fixture-power'), stderr: Buffer.alloc(0), proof: proof(), logs: []}; }
      const side = binary.includes(COMMITS.baseline) ? 'baseline' : 'candidate', index = measured++;
      events.push(`measure:${side}`); invocations.push({side, binary, args, options});
      const result = {status: 0, handlesReaped: true, stdout: bytes(envelope()), stderr: bytes(profile()), proof: proof(side === 'baseline' ? 10 : 8)};
      faults.mutate?.(result, index, side);
      const stdout = path.join(options.evidence, `${index}.stdout.bin`), stderr = path.join(options.evidence, `${index}.stderr.bin`);
      fs.writeFileSync(stdout, result.stdout); fs.writeFileSync(stderr, result.stderr); result.logs = [stdout, stderr];
      return result;
    },
    collectOwnedMemory() { events.push('memory'); return {scope: 'fixture sampled lower bound'}; },
    compareCandidateFacts(documents, homes) {
      events.push('facts'); for (const home of Object.values(homes)) assert(fs.existsSync(home));
      if (faults.facts) throw new Error('complete Facts fields/order differ: fixture');
      return {documents: 4036, totals: {}, rows: []};
    },
  };
  const report = runCandidateController({request, runnerRoot: root, inputs, conditions, scratch, evidence}, services);
  return {directory, request, report, events, owners, invocations};
}

test('fixed three AB/BA/AB pairs and original sealed commit/binary/source identities', t => {
  assert.equal(PAIRS, 3); assert.deepEqual([0, 1, 2].map(orderForCandidatePair), [['baseline', 'candidate'], ['candidate', 'baseline'], ['baseline', 'candidate']]);
  for (const commit of Object.values(COMMITS)) assert.equal(sha256(execFileSync('git', ['show', `${commit}:crates/lycheedev-source/src/index/profile.rs`], {cwd: root})), PROFILE_SOURCE_SHA256);
  assert.equal(Object.keys(runnerIdentity(root)).length, RUNNER_FILES.length); assert.equal(RUNNER_FILES.length, 18);
  
});

test('request separates original artifact owner from independent clean-source witnesses and rejects configurable pairs', t => {
  const request = requestAt(temporary(t)), result = validateRequest(request);
  assert.equal(result.baseline.artifactRoot, result.candidate.artifactRoot); assert.notEqual(result.baseline.sourceRoot, result.baseline.artifactRoot);
  for (const field of ['pairs', 'repetitions']) assert.throws(() => validateRequest({...request, [field]: 3}), /exact fields/);
  for (const field of ['commit', 'artifactSHA256', 'binarySHA256', 'sourceTreeSHA256']) { const copy = structuredClone(request); copy.baseline[field] = '0'.repeat(field === 'commit' ? 40 : 64); assert.throws(() => validateRequest(copy)); }
  const alias = structuredClone(request); alias.candidate.sourceRoot = alias.baseline.sourceRoot; assert.throws(() => validateRequest(alias), /independent fixed/);
  const outside = structuredClone(request); outside.baseline.artifact = path.join(request.ownerRoot, 'outside.json'); assert.throws(() => validateRequest(outside));
  assert.equal(validateConditions(conditions), conditions); assert.throws(() => validateConditions({...conditions, disk: ''}));
  
});

test('sealed record validation preserves historical sourceRoot and rejects changed binary/source/profile/invocation', t => {
  const request = requestAt(temporary(t));
  for (const side of sides) {
    const input = request[side], record = {sourceRoot: request.ownerRoot, sourceBinary: path.join(input.artifactRoot, 'original.exe'), profile: 'symbols', sameInvocation: true, pdb: 'original.pdb', binarySHA256: input.binarySHA256, inputHashes: {sourceTree: input.sourceTreeSHA256}};
    fs.writeFileSync(input.artifact, JSON.stringify(record)); input.artifactSHA256 = sha256(fs.readFileSync(input.artifact));
    const before = fs.readFileSync(input.artifact); verifyCandidateRecord(input, JSON.parse(before)); assert.deepEqual(fs.readFileSync(input.artifact), before);
    for (const change of [r => r.profile = 'debug', r => r.sameInvocation = false, r => delete r.pdb, r => r.binarySHA256 = 'a'.repeat(64), r => r.inputHashes.sourceTree = 'a'.repeat(64), r => r.sourceRoot = 'relative']) { const bad = structuredClone(record); change(bad); assert.throws(() => verifyCandidateRecord(input, bad)); }
  }
  
});

test('default stack is enforced and phase environment is case-insensitively confined to measured calls', () => {
  assert.deepEqual(sourcePhaseEnvironment({Path: 'fixture', lycheedev_source_phase_profile: 'old'}), {Path: 'fixture'});
  assert.deepEqual(sourcePhaseEnvironment({Path: 'fixture', lycheedev_source_phase_profile: 'old'}, true), {Path: 'fixture', LYCHEEDEV_SOURCE_PHASE_PROFILE: '1'});
  for (const key of ['RUST_MIN_STACK', 'rust_min_stack', 'RUSTFLAGS', 'Cargo_Encoded_RustFlags']) assert.throws(() => sourcePhaseEnvironment({[key]: 'override'}), /default stack/);
});

test('strict 31-phase parser accepts exact compact LF/CRLF profiles and no normalization', () => {
  for (const ending of ['', '\n', '\r\n']) assert.deepEqual(sourcePhaseProfile(Buffer.from(JSON.stringify(profile()) + ending)), profile());
  const base = JSON.stringify(profile());
  for (const raw of ['', '\ufeff' + base, base + '\nextra', base + '\n\n', ' ' + base, base.replace('"elapsedMicros":100000', '"elapsedMicros":1e5'), base.replace('"elapsedMicros":100000', '"elapsedMicros":100000,"elapsedMicros":100000')]) assert.throws(() => sourcePhaseProfile(Buffer.from(raw)));
  assert.throws(() => sourcePhaseProfile(Buffer.from([0xff]))); assert.throws(() => sourcePhaseProfile(Buffer.alloc(16385)));
  for (const mutate of [v => v.extra = 1, v => v.timing = 'different', v => v.lockTiming = 'different', v => v.phases.pop(), v => v.phases.reverse(), v => v.phases[0].phase = 'unknown', v => v.phases[0].calls = -1, v => v.phases[0].elapsedMicros = 1.5, v => v.phases[0].extra = 1, v => v.phases[1].elapsedMicros = 1, v => v.phases[0].calls = Number.MAX_SAFE_INTEGER + 1]) { const bad = profile(); mutate(bad); assert.throws(() => sourcePhaseProfile(bytes(bad))); }
});

test('cold profiles bind 4036 public parsed documents to read/parse/serialize/publish and worker accounting', () => {
  verifyColdProfile(sourcePhaseProfile(bytes(profile())), envelope(), oracle);
  for (const name of ['facts_read', 'facts_parse', 'facts_serialize', 'facts_publish', 'facts_worker_wait', 'facts_parse_sequential', 'facts_worker_abandoned']) { const bad = profile(); bad.phases.find(p => p.phase === name).calls++; assert.throws(() => verifyColdProfile(sourcePhaseProfile(bytes(bad)), envelope(), oracle)); }
  for (const cachedIndex of [true, null, 0, 'false']) { const out = envelope(); out.result.io.cachedIndex = cachedIndex; assert.throws(() => verifyColdProfile(profile(), out, oracle)); }
  for (const workers of [0, 4036]) { const value = profile(); for (const p of value.phases) { if (['facts_worker_wait', 'facts_worker_consumed'].includes(p.phase)) p.calls = workers; if (p.phase === 'facts_worker_abandoned') p.calls = 0; if (p.phase === 'facts_parse_sequential') p.calls = 4036 - workers; p.elapsedMicros = p.calls; } verifyColdProfile(sourcePhaseProfile(bytes(value)), envelope(), oracle); }
});

test('public summary accepts actual omitted cachedIndex, rejects warm/missing documents and retains all semantic fields', () => {
  const outputs = {baseline: envelope(), candidate: envelope()}; compareCandidateSummaries(outputs, oracle, 'PIN-fixture');
  outputs.baseline.result.io.cachedIndex = false; outputs.candidate.result.io.cachedIndex = false; compareCandidateSummaries(outputs, oracle, 'PIN-fixture');
  outputs.candidate.result.io.elapsedMs = 500; compareCandidateSummaries(outputs, oracle, 'PIN-fixture');
  for (const mutate of [v => v.io.cachedIndex = true, v => v.io.cachedIndex = null, v => v.io.cachedFacts = 1, v => v.io.parsedDocuments--, v => v.assets++, v => v.astFiles = 5, v => v.io.readBytes++, v => v.io.deadlineMs++, v => v.unexpected = true, v => v.diagnosticSample = [{code: 'unexpected'}], v => delete v.declarations]) { const copy = structuredClone(outputs); mutate(copy.candidate.result); assert.throws(() => compareCandidateSummaries(copy, oracle, 'PIN-fixture')); }
});

test('complete 4036-document comparison authenticates each file and retains hashes without retaining Facts bodies', t => {
  const f = factsFixture(t, 4036), result = f.compare();
  assert.equal(result.documents, 4036); assert.equal(result.rows.length, 4036); assert.equal(result.fullFieldsAndOrderEqual, true);
  assert.equal(result.totals.baseline.relationships, 8072); assert(!JSON.stringify(result).includes('Raw<&>Target'));
  
});

for (const section of sections) test(`complete Facts rejects ${section} field changes, same-multiset order and duplicate multiplicity`, t => {
  const f = factsFixture(t), document = f.documents[0], original = factsPayload();
  for (const mutate of [facts => facts[section][0].extra = 'unexpected', facts => delete facts[section][0][Object.keys(facts[section][0])[0]], facts => facts[section].push(facts[section][0]), facts => { facts[section].push({...facts[section][0], line: 99}); facts[section].reverse(); }]) {
    const expected = structuredClone(original); if (mutate.toString().includes('reverse')) expected[section].push({...expected[section][0], line: 99});
    writeFact(f.homes.baseline, document, expected); const changed = structuredClone(original); mutate(changed); writeFact(f.homes.candidate, document, changed); assert.throws(() => f.compare(), /Facts fields\/order differ/);
  }
  
});

test('complete Facts rejects missing/extra sections, missing/duplicate manifest paths and index-total mismatch', t => {
  const f = factsFixture(t), document = f.documents[0];
  for (const mutate of [facts => delete facts.loads, facts => facts.extra = []]) { const changed = factsPayload(); mutate(changed); writeFact(f.homes.candidate, document, changed); assert.throws(() => f.compare()); }
  writeFact(f.homes.candidate, document, factsPayload()); f.summaries.candidate.declarations++; assert.throws(() => f.compare(), /sum differs/); f.summaries.candidate.declarations--;
  f.documents[1] = {...document}; assert.throws(() => f.compare(), /duplicate document/); f.documents.pop(); assert.throws(() => f.compare());
  
});

test('real controller schedules exactly six adjacent owned invocations, six cold homes and validation-before-cleanup', t => {
  const f = controllerFixture(t); assert.equal(f.report.state, 'source_candidates_observed'); assert.equal(f.report.performanceAcceptanceEvaluated, false); assert.equal(f.report.fullB01B07Acceptance, false);
  assert.deepEqual(f.invocations.map(v => v.side), ['baseline', 'candidate', 'candidate', 'baseline', 'baseline', 'candidate']);
  const homes = f.invocations.map(v => v.args[v.args.indexOf('--home') + 1]); assert.equal(new Set(homes).size, 6);
  for (const call of f.invocations) { assert.equal(call.options.cwd, f.request[call.side].sourceRoot); assert.equal(call.options.timeout, 210000); assert.equal(call.options.orphanMs, 30000); assert.equal(call.options.memoryObservation, true); assert.equal(call.options.env.LYCHEEDEV_SOURCE_PHASE_PROFILE, '1'); assert.deepEqual(call.args.slice(0, 2), ['source', 'index']); assert(!call.args.includes('--budget')); }
  for (let i = 0; i < f.events.length; i++) if (f.events[i].startsWith('measure:') && !f.events[i - 1]?.startsWith('measure:')) assert(f.events[i + 1].startsWith('measure:'), 'measured pair stays adjacent');
  assert.equal(f.events.filter(v => v === 'facts').length, 3); assert(f.owners.every(v => v.cleaned && v.sealed.state === 'succeeded'));
  assert(f.report.pairs.every(p => p.factsEquivalent && p.profilesVerified && p.resultEquivalent && p.baseline.valid && p.candidate.valid)); assert.equal(f.report.observation.ratioOfMedians, 0.8); assert.equal(f.report.finalIntegrityVerified, true);
  
});

for (const [name, mutate] of Object.entries({product_exit: r => r.status = 1, malformed_profile: r => r.stderr = Buffer.from('not a profile\n'), forced_owner: r => r.proof.forced = true, deadline: r => r.proof.deadlineHit = true, output_limit: r => r.proof.outputLimit = true, proof_error: r => r.proof.error = 'failure', missing_native_clock: r => delete r.proof.timing, warm_result: r => { const out = envelope(); out.result.io.cachedFacts = 1; r.stdout = bytes(out); }})) {
  test(`real controller retains ${name} pair and raw outputs without replacement or acceptance`, t => {
    const f = controllerFixture(t, {mutate: (run, index) => { if (index === 0) mutate(run); }});
    assert.equal(f.report.state, 'source_candidates_failed'); assert.equal(f.invocations.length, 2); assert.equal(f.report.pairs.length, 1); assert.equal(f.owners[0].cleaned, false); assert.equal(f.owners[0].sealed.state, 'failed');
    assert(f.report.pairs[0].baseline.validationFailure); assert.equal(f.report.pairs[0].candidate.valid, true); assert.equal(f.report.observation, undefined);
    for (const side of sides) for (const log of f.report.pairs[0][side].logs) assert(fs.existsSync(log));
    
  });
}

test('real controller stops before paired launch when retirement is unproved and retains outer owner', t => {
  const f = controllerFixture(t, {mutate: (run, index) => { if (index === 0) run.handlesReaped = false; }});
  assert.equal(f.invocations.length, 1); assert.equal(f.report.requiresOuterRetirement, true); assert.equal(f.owners[0].sealed.state, 'interrupted'); assert.equal(f.owners[0].cleaned, false); assert.equal(f.report.finalIntegrityVerified, false); 
});

test('real controller retains complete Facts mismatch and late source drift before cleanup', t => {
  for (const faults of [{facts: true}, {identityAt: 5}]) { const f = controllerFixture(t, faults); assert.equal(f.report.state, 'source_candidates_failed'); assert.equal(f.invocations.length, 2); assert.equal(f.owners[0].cleaned, false); assert.equal(f.owners[0].sealed.state, 'failed'); assert(f.report.pairs[0].failure); }
  
});

test('real controller cannot promote otherwise verified pairs after final identity drift', t => {
  const f = controllerFixture(t, {identityAt: 15}); assert.equal(f.invocations.length, 6); assert.equal(f.report.state, 'source_candidates_failed'); assert.equal(f.report.finalIntegrityVerified, false); assert.equal(f.report.observation, undefined); 
});


test('outer completion requires the original native owner proof, three verified pairs and 60-minute cap', t => {
  assert.equal(OUTER_TIMEOUT_MS, 3600000);
  const wrapper = fs.readFileSync(path.join(root, 'tools/rust-source-candidate-diagnostics.mjs'), 'utf8');
  assert.match(wrapper, /timeout: OUTER_TIMEOUT_MS/);
  const f = controllerFixture(t), run = {status: 0, handlesReaped: true, proof: proof()};
  assert.equal(verifyObservedReport(run, f.report), true);
  for (const mutate of [r => r.proof.forced = true, r => r.proof.deadlineHit = true, r => r.proof.outputLimit = true, r => r.proof.error = 'owner fault', r => r.status = 1, r => r.handlesReaped = false]) { const bad = structuredClone(run); mutate(bad); assert.throws(() => verifyObservedReport(bad, f.report)); }
  for (const mutate of [r => r.pairs.pop(), r => r.pairs[0].facts.documents--, r => r.pairs[1].order.reverse(), r => r.pairs[1].baseline.home = r.pairs[0].baseline.home, r => r.pairs[0].candidate.valid = false, r => r.requiresOuterRetirement = true, r => r.finalIntegrityVerified = false, r => r.performanceAcceptanceEvaluated = true]) { const bad = structuredClone(f.report); mutate(bad); assert.throws(() => verifyObservedReport(run, bad)); }
});
