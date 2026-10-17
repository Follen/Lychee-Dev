import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync} from 'node:fs';
import {join, relative, resolve} from 'node:path';
import {hostname, tmpdir} from 'node:os';
import {DatabaseSync} from 'node:sqlite';
import {hashFile} from '../../tools/rust-build.mjs';
import {fixturePin} from '../rust-source-native-acceptance/assertions.mjs';
import {PHASE_NAMES, PHASE_TIMING, LOCK_TIMING} from './source-candidates.mjs';
import {verifyPackProfile} from './source-pack-resume.mjs';
import {elapsed, semantic} from './contract.mjs';
import {beginScratch, sha256} from '../../tools/rust-disk.mjs';
import {cloneSeed, inventory, proveWarmReuse} from './inputs.mjs';
import {RETAINED_WARM, REMAINING_ALLOWANCE, authenticateRemaining, validateRemainingReference, claimRemaining, RETAINED_COLD, authenticateRetainedCold, inspectRetainedCold, validateColdReference, claimColdBorrow, captureColdFactFiles, metadataIdentity, captureMetadata, indexEvidence, assertDiskPolicy, CASES, BASE_COMMIT, CANDIDATE_COMMIT, CANDIDATE_IDENTITY, INSERT_TEXT, FIRST_PATH, RENAME_PATH, RUNNER_FILES, evidenceFiles, footprint, mutationRows, orderFor, runnerIdentity, runController, validateRequest, variantOracle, verifyObservedReport} from './source-pack-scenarios.mjs';
const root = resolve(import.meta.dirname, '../..');
const conditions = {machine: 'fixture', powerAndThermal: 'unknown', competingLoad: 'unknown', disk: 'fixture only', operator: 'Node test'};
function temp(t) { const d = mkdtempSync(join(tmpdir(), 'pack2-runner-')); t.after(() => { if (t.passed) rmSync(d, {recursive: true}); else t.diagnostic(`retained fixture ${d}`); }); return d; }
function proof(ms = 10) { return {schema: 'lycheedev.owned-process.v1', rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0, forced: false, timing: {schema: 'lycheedev.owned-timing.v1', frequency: 1000000, startTicks: '1000000', rootExitTicks: String(1000000 + ms * 1000 - 100), treeReapedTicks: String(1000000 + ms * 1000)}}; }
function requestAt(d) { const sourceRoot = join(d, 'product'), artifactRoot = join(d, 'artifacts'), goRoot = join(d, 'go'); for (const x of [sourceRoot, artifactRoot, goRoot]) mkdirSync(x); const artifact = join(artifactRoot, 'artifact.json'); writeFileSync(artifact, '{}'); return {schema: 'lycheedev.source-pack-scenarios-config.v1', ownerRoot: d, candidate: {sourceRoot, artifactRoot, artifact, commit: CANDIDATE_COMMIT, ...CANDIDATE_IDENTITY}, go: {sourceRoot: goRoot, packageRoot: goRoot, luaLSArchive: join(d, 'luals.zip'), packageSHA256: 'd'.repeat(64)}}; }
async function fixture(t, fault = {}) {
  const d = temp(t), request = requestAt(d), out = join(d, 'outer-evidence'), scratch = join(d, 'outer-scratch'); mkdirSync(out); mkdirSync(scratch);
  const inputs = {runner: 'fixed'}, owners = [], events = [], samples = [], modes = []; let calls = 0, identityChecks = 0; const admissions = [];
  const basePin = {id: 'PIN-fixture', source: {exactCommit: BASE_COMMIT}}, baseOracle = JSON.parse(readFileSync(join(root, 'tests/rust-source-full-corpus/retail-oracle.json')));
  const svc = {
    runnerIdentity() { events.push('identity'); if (fault.identityAt === ++identityChecks) return {runner: 'changed'}; return inputs; },
    verifyCandidateRecord() { return {sourceBinary: 'rust.exe'}; }, verifyGoPackage() { return {binary: 'go.exe', packageSHA256: request.go.packageSHA256}; },
    admission(_root, options) { events.push('admission'); admissions.push(options); },
    beginScratch(_root, name, options) {
      events.push(`begin:${name}`); if (fault.admission && name.includes('single-content-change')) throw Object.assign(new Error('disk.scratch_quota'), {report: {scratchBytes: 4612057341, limits: {scratchBytes: 5368709120}}});
      const directory = join(d, name), evidence = join(d, 'evidence-' + name); mkdirSync(directory); mkdirSync(evidence);
      const owner = {directory, evidence, receipt: join(evidence, 'receipt.json'), options, sealed: null, cleaned: false, seal(value) { if (fault.seal && name.endsWith('go')) throw new Error('seal fault'); this.sealed = value; events.push(`seal:${name}:${value.state}`); }, cleanup() { assert.equal(this.sealed.state, 'succeeded'); assert.equal(this.sealed.handlesReaped, true); this.cleaned = true; events.push(`cleanup:${name}`); rmSync(directory, {recursive: true}); }}; owners.push(owner); return owner;
    },
    prepareSeeds(homes) { for (const p of Object.values(homes)) mkdirSync(p); events.push('sync-seeds'); return {pin: basePin, rows: []}; },
    inventory(home) { if (fault.inventory && home.includes('root-insert') && calls >= 7) return {rows: [], bytes: 2 ** 40, sha256: 'healthy'}; return {rows: [], bytes: 100, sha256: 'healthy'}; },
    cloneSeed(source, dest) { assert(existsSync(source)); assert(!existsSync(dest)); mkdirSync(dest); events.push(`clone:${dest}`); },
    prepareVariant({homes, directory, id}) { const entry = CASES.find(c => c.id === id); events.push(`prepare:${id}:${Object.keys(homes)[0]}`); const pin = {id: 'PIN-' + id, source: {exactCommit: id}}; return {pin, oracle: {...baseOracle, documents: entry.documents, commit: id}, project: join(directory, 'project'), mutation: {id}, rows: []}; },
    ownedSpawnSync(binary, args, options) {
      if (binary === 'powercfg.exe') return {status: 0, handlesReaped: true, stdout: Buffer.from(fault.power && calls > 0 ? 'changed-power' : 'power'), stderr: Buffer.alloc(0), proof: proof(), logs: []};
      const index = calls++, engine = binary.startsWith('rust') ? 'rust' : 'go', entry = CASES[Math.floor(index / 2)];
      events.push(`sample:${entry.id}:${engine}`); samples.push({entry, engine, options, args});
      if (fault.throwAt === index) throw new Error('launch exception');
      const pin = entry.mode === 'changed' ? 'PIN-' + entry.id : basePin.id;
      const result = {schema: 'lycheedev.source-map.v2', storage: 'file-cache', parser: 'source-v1', repository: baseOracle.repository, product: baseOracle.product, commit: entry.mode === 'changed' ? entry.id : BASE_COMMIT, documents: entry.documents, declarations: 1, relationships: 0, diagnostics: 0, assets: 0, complete: true, io: {parsedDocuments: entry.mode === 'cold' ? 4036 : entry.mode === 'warm' ? 0 : 1}};
      const run = {status: 0, handlesReaped: true, stdout: Buffer.from(JSON.stringify({schema: 'lycheedev.result.v1', ok: true, error: null, result, context: {snapshot: pin}})), stderr: Buffer.from('profile'), proof: proof(), logs: []};
      if (fault.failAt === index) run.status = 4;
      if (fault.unreapedAt === index) { run.handlesReaped = false; run.proof.treeReaped = false; }
      if (fault.badPinAt === index) run.stdout = Buffer.from(run.stdout.toString().replace(pin, 'PIN-wrong'));
      return run;
    },
    collectOwnedMemory() { return {state: 'sampled_lower_bound', gaps: ['fixture gap']}; },
    verifyPackProfile(_raw, envelope, options) { modes.push(options.mode); if (fault.profile === options.mode) throw new Error('profile mismatch'); return {mode: options.mode, profile: {phases: Array(31).fill({})}, io: envelope.result.io}; },
    derivedIdentity() { return {sha256: 'same', files: [1]}; }, proveWarmReuse() { if (fault.warm) throw new Error('warm identity changed'); return {verified: true}; },
    indexEvidence() { events.push('capture-index'); return {path: 'records', bytes: 100, sha256: 'a'.repeat(64)}; },
    captureGoCustody(home, index, evidence) { events.push('capture-full-go'); if (fault.custody) throw new Error('custody incomplete'); return {directory: evidence, catalog: join(evidence, 'catalog.json'), catalogSHA256: 'b'.repeat(64)}; }, openGoCustody() {},
    compareFacts(homes, outputs, pin, oracle, evidence, _root, retained) { events.push('facts'); if (fault.facts && (fault.facts === 'cold' ? !retained : retained)) throw new Error('complete Facts differ'); return {documents: oracle.documents, fullFieldsAndOrderEqual: true, totals: {}, indexes: {go: {bytes: 100}, rust: {bytes: 100}}}; },
  };
  let resumeCold, resumeRemaining;
  if (fault.resume || fault.remaining) {
    const homes = Object.fromEntries(['go', 'rust'].map(e => [e, join(d, `retained-${e}-home`)])); for (const home of Object.values(homes)) mkdirSync(home);
    const old = {id: 'cold', mode: 'cold', order: ['go', 'rust'], state: 'failed', factsEquivalent: false, failure: 'closed checkpointed metadata required', pin: basePin, lineage: {state: 'authenticated-own-sync-only'}}, outputs = {};
    for (const engine of ['go', 'rust']) { const run = svc.ownedSpawnSync(`${engine}.exe`, [], {}); run.proof.runId = RETAINED_COLD[engine]; const row = {binary: `${engine}.exe`, cwd: request[engine === 'rust' ? 'candidate' : 'go'].sourceRoot, home: homes[engine], proof: run.proof, valid: true, ...elapsed(run.proof)}; const envelope = JSON.parse(run.stdout); if (engine === 'rust') row.sourceProfile = svc.verifyPackProfile(run.stderr, envelope, {mode: 'cold'}); old[engine] = row; outputs[engine] = {row, envelope, stderr: run.stderr}; }
    samples.length = 0; events.length = 0; modes.length = 0;
    resumeCold = {schema: 'lycheedev.source-pack2-retained-cold.v1', outerReceiptSHA256: 'a'.repeat(64), baseReceiptSHA256: 'b'.repeat(64), homeInventorySHA256: {go: 'c'.repeat(64), rust: 'd'.repeat(64)}};
    svc.assertCorpus = () => {};
    svc.authenticateRetainedCold = () => { events.push('authenticate-cold'); if (fault.authentication) throw new Error('prior sealed log changed'); return {paths: {homes, outer: 'original-outer', base: 'original-base'}, pin: basePin, rows: [], cold: old, outputs, oldReport: {machine: {hostname: fault.otherMachine ? 'other-host' : hostname(), platform: process.platform, powerBefore: 'power'}, inputs: {}}, inventories: Object.fromEntries(['go', 'rust'].map(e => [e, svc.inventory(homes[e])])), verifyHomes() { events.push('verify-old-homes'); }, verifyEvidence() { events.push('verify-old-evidence'); if (fault.originalChanged && calls > 2) throw new Error('original failed evidence changed'); }}; };
    svc.captureColdFactFiles = () => { events.push('cold-facts-custody'); if (fault.coldCustody) throw new Error('cold custody incomplete'); };
    svc.claimColdBorrow = (_root, _reference, owner) => { events.push('claim-borrow'); if (fault.claimed) throw new Error('already claimed'); const path = join(owner.evidence, 'claim.json'); writeFileSync(path, '{}'); return {path, sha256: hashFile(path)}; };
  }
  if (fault.remaining) {
    const previous = svc.authenticateRetainedCold(), cold = previous.cold; cold.state = 'verified'; cold.factsEquivalent = true; delete cold.failure; cold.facts = svc.compareFacts({}, {}, basePin, baseOracle, out); const warm = {id: 'warm', mode: 'warm', order: ['rust', 'go'], state: 'verified', factsEquivalent: true, facts: cold.facts, pin: basePin};
    const outputs = Object.fromEntries(Object.entries(previous.outputs).map(([e, value]) => [`cold-${e}`, value]));
    for (const engine of ['rust', 'go']) { const run = svc.ownedSpawnSync(`${engine}.exe`, [], {}), row = {binary: `${engine}.exe`, cwd: request[engine === 'rust' ? 'candidate' : 'go'].sourceRoot, home: previous.paths.homes[engine], valid: true, proof: run.proof, ...elapsed(run.proof), warmReuse: {verified: true}}; const envelope = JSON.parse(run.stdout); if (engine === 'rust') row.sourceProfile = svc.verifyPackProfile(run.stderr, envelope, {mode: 'warm'}); warm[engine] = row; outputs[`warm-${engine}`] = {row, envelope, stderr: run.stderr}; }
    resumeRemaining = {...resumeCold, schema: 'lycheedev.source-pack2-retained-warm.v1'}; resumeCold = undefined; samples.length = 0; events.length = 0; modes.length = 0;
    svc.authenticateRemaining = () => { events.push('authenticate-warm'); if (fault.remainingAuthentication) throw new Error('sealed warm changed'); return {paths: {...previous.paths, custody: 'original-custody'}, reference: resumeRemaining, prior: previous.oldReport, cold, warm, pending: {id: 'single-content-change', state: 'running'}, pin: basePin, rows: [], outputs, baseIdentity: previous.inventories, verifyEvidence() { events.push('verify-completed-evidence'); if (fault.remainingIntegrity && calls > 4) throw new Error('prior completed evidence changed'); }, verifyHomes() { events.push('verify-read-only-bases'); }}; };
    svc.claimRemaining = (_root, _reference, owner) => { events.push('claim-remaining'); if (fault.remainingClaimed) throw new Error('already claimed remaining'); const path = join(owner.evidence, 'remaining-claim.json'); writeFileSync(path, '{}'); return {path, sha256: hashFile(path)}; };
  }
  const report = await runController({request, runnerRoot: root, inputs, conditions, scratch, evidence: out, ...(resumeCold ? {resumeCold} : {}), ...(resumeRemaining ? {resumeRemaining} : {}), ...(fault.allowance ? {allowance: fault.allowance} : {})}, svc);

  return {report, request, inputs, owners, events, samples, modes, resumeCold, resumeRemaining, admissions, directory: d};
}
test('fixed pack2 candidate and exact six cases keep the old historical runner untouched', () => {
  assert.equal(CANDIDATE_COMMIT, '00d215a937513609e482c1ef65f3b43f0288beee'); assert.deepEqual(CASES.map(c => c.id), ['cold', 'warm', 'single-content-change', 'root-insert', 'first-delete', 'cross-prefix-rename']); assert.deepEqual(CASES.slice(2).map(c => c.maxParsed), [64, 513, 512, 1025]);
  assert.equal(Buffer.byteLength(INSERT_TEXT), 34); assert.equal(sha256(INSERT_TEXT), 'a1fb32c0408f44f38dec357b31f1268ea84a12f1c33c01ffda14948ec3d9e330'); assert.equal(RENAME_PATH, 'ZZZ_LycheePackRenamed.lua'); assert(readFileSync(join(root, 'tests/rust-performance-paired/source-candidates.mjs'), 'utf8').includes("'v10'"));
});
test('configuration rejects unknown policies, old candidates and malformed input identity', t => {
  const request = requestAt(temp(t)); assert.equal(validateRequest(request).candidate.commit, CANDIDATE_COMMIT);
  for (const mutate of [r => r.pairs = 20, r => r.candidate.commit = BASE_COMMIT, r => r.candidate.binarySHA256 = 'x', r => r.go.packageSHA256 = 'x', r => r.go.extra = true, r => r.ownerRoot = '.', r => r.candidate.artifact = join(r.ownerRoot, 'foreign.json')]) { const copy = structuredClone(request); mutate(copy); assert.throws(() => validateRequest(copy)); }
});
test('runner manifest closes the new imports without needing any build executable', () => { assert.equal(Object.keys(runnerIdentity(root)).length, RUNNER_FILES.length); assert(RUNNER_FILES.includes('tests/rust-performance-paired/source-pack-custody.mjs')); const source = readFileSync(join(root, 'tools/rust-source-pack-diagnostics.mjs'), 'utf8'); assert(!/buildRust\(|runCargo\(|rustc/.test(source)); });
test('admission estimate is explicit positive and rejects malformed quantities', () => { const cold = footprint({cold: true}); assert.equal(cold.scratchReserveBytes, 257058598 + 131416885 + (64 << 20)); assert(cold.requiredBytes > cold.scratchReserveBytes); assert.throws(() => footprint({cold: false})); });
test('mutation authenticity rejects foreign bytes and nonfixed first document', () => { assert.throws(() => mutationRows([], 'root-insert', Buffer.alloc(2533))); assert.throws(() => mutationRows([{path: FIRST_PATH}], 'root-insert', Buffer.alloc(2533))); assert.throws(() => mutationRows([], 'unknown', Buffer.alloc(2533))); });
test('variant oracle covers the entire path byte hash and extension inventory', () => { const rows = [{path: 'a.lua', bytes: 3, sha256: sha256('abc')}, {path: 'b.xml', bytes: 2, sha256: sha256('xy')}, {path: 'n.txt', bytes: 1, sha256: sha256('x')}]; const v = variantOracle({}, rows, 'c'); assert.equal(v.documents, 2); assert.equal(v.allBytes, 6); assert.equal(v.documentBytes, 5); assert.deepEqual(v.counts, {'.lua': 1, '.xml': 1, '.toc': 0}); assert.notEqual(v.allPathSHA256SizeDigest, v.documentPathSHA256SizeDigest); });
test('six complete scenarios seal before cleanup, clone healthy base and sequence engine owners', async t => {
  const f = await fixture(t); assert.equal(f.report.state, 'source_pack_scenarios_observed', f.report.blocker); assert.equal(f.samples.length, 12); assert.equal(f.owners.length, 9); assert(f.owners.every(o => o.cleaned)); assert.deepEqual(f.modes, ['cold', 'warm', 'changed', 'changed', 'changed', 'changed']);
  for (let i = 2; i < CASES.length; i++) { const id = CASES[i].id; assert(f.events.indexOf(`cleanup:source-pack2-${id}-go`) < f.events.indexOf(`begin:source-pack2-${id}-rust`)); }
  assert(f.samples.every(s => s.options.timeout === 210000 && s.options.memoryObservation)); assert(f.samples.filter(s => s.engine === 'go').every(s => !s.options.env.LYCHEEDEV_SOURCE_PHASE_PROFILE)); assert(f.samples.filter(s => s.engine === 'rust').every(s => s.options.env.LYCHEEDEV_SOURCE_PHASE_PROFILE === '1'));
  verifyObservedReport({status: 0, handlesReaped: true, proof: proof()}, f.report, f.request, f.inputs);
});
for (const [name, fault, calls] of [['cold failure', {failAt: 0}, 2], ['cold semantic failure', {badPinAt: 1}, 2], ['warm reuse failure', {warm: true}, 4], ['changed Go failure', {failAt: 4}, 5], ['changed Rust failure', {failAt: 5}, 6], ['custody failure', {custody: true}, 5], ['complete Facts mismatch', {facts: 'changed'}, 6], ['profile failure', {profile: 'changed'}, 6]]) test(`${name} preserves failed owners and never substitutes samples`, async t => { const f = await fixture(t, fault); assert.equal(f.report.state, 'source_pack_scenarios_failed'); assert.equal(f.samples.length, calls); assert.equal(f.owners[0].cleaned, false); assert(f.owners.some(o => o.sealed?.state === 'failed')); });
for (const fault of [{unreapedAt: 0}, {throwAt: 0}, {unreapedAt: 4}, {throwAt: 4}]) test(`unreaped/throw path stops launches and retains interrupted ownership ${JSON.stringify(fault)}`, async t => { const f = await fixture(t, fault); assert.equal(f.report.requiresOuterRetirement, true); assert.equal(f.report.state, 'source_pack_scenarios_failed'); assert(f.owners.filter(o => !o.cleaned).every(o => o.sealed?.state === 'interrupted')); assert.equal(f.samples.length, (fault.unreapedAt ?? fault.throwAt) + 1); });
test('quota rejection retains healthy base and publishes the actual blocker', async t => { const f = await fixture(t, {admission: true}); assert.equal(f.report.blocker, 'disk.scratch_quota'); assert.equal(f.report.admissionFailure.scratchBytes, 4612057341); assert.equal(f.samples.length, 4); assert.equal(f.owners.length, 1); assert.equal(f.owners[0].cleaned, false); });
test('final identity and power failures cannot qualify otherwise completed samples', async t => { for (const fault of [{identityAt: 2}, {power: true}]) { const f = await fixture(t, fault); assert.equal(f.report.state, 'source_pack_scenarios_failed'); assert.equal(f.owners[0].cleaned, false); } });
test('actual footprint above admitted estimate fails and retains the own fixture', async t => { const f = await fixture(t, {inventory: true}); assert.equal(f.report.state, 'source_pack_scenarios_failed'); assert.match(f.report.blocker, /footprint/); assert(f.owners.some(o => !o.cleaned && o.sealed?.state === 'failed')); });
test('outer report validation refuses missing cases, imported samples, homes reused and lost clocks', async t => { const f = await fixture(t); for (const mutate of [r => r.scenarios.pop(), r => r.importedSamples = 1, r => r.scenarios[2].go.home = r.scenarios[0].go.home, r => r.scenarios[1].go.warmReuse.verified = false, r => r.scenarios[4].rust.proof.forced = true, r => r.layout = 'v11-pack1']) { const copy = structuredClone(f.report); mutate(copy); assert.throws(() => verifyObservedReport({status: 0, handlesReaped: true, proof: proof()}, copy, f.request, f.inputs)); } });
test('nested bounded evidence is all sealed, oversized files cannot silently disappear', t => { const d = temp(t); mkdirSync(join(d, 'parts')); writeFileSync(join(d, 'receipt.json'), '{}'); writeFileSync(join(d, 'parts/index.part000.bin'), 'a'); assert.equal(evidenceFiles(d).length, 1); writeFileSync(join(d, 'oversize'), Buffer.alloc((16 << 20) + 1)); assert.throws(() => evidenceFiles(d), /bounded evidence/); });

test('quota overrides and policy weakening reject before any scratch owner is created', async t => {
  const d = temp(t); assert.equal(assertDiskPolicy(d, {}).scratchBytes, 5 * 1024 ** 3);
  for (const key of ['LYCHEEDEV_DISK_SCRATCH_GIB', 'LYCHEEDEV_DISK_CACHE_GIB', 'LYCHEEDEV_DISK_FREE_GIB', 'lycheedev_disk_scratch_gib']) assert.throws(() => assertDiskPolicy(d, {[key]: '10'}), /overrides/);
  mkdirSync(join(d, 'release')); writeFileSync(join(d, 'release/rust-disk-policy.json'), JSON.stringify({schema: 'lycheedev.disk-policy.v1', scratchBytes: 10 * 1024 ** 3})); assert.throws(() => assertDiskPolicy(d, {}), /fixed original/);
  let created = false; await assert.rejects(runController({request: {ownerRoot: d}}, {beginScratch() { created = true; }}), /fixed original/); assert.equal(created, false);
  const wrapper = readFileSync(join(root, 'tools/rust-source-pack-diagnostics.mjs'), 'utf8'); assert(wrapper.indexOf('assertDiskPolicy(request.ownerRoot,') < wrapper.indexOf('const job = beginScratch'));
});

function metadataFixture(t) {
  const d = temp(t), home = join(d, 'home'), evidence = join(d, 'evidence'); mkdirSync(join(home, 'state'), {recursive: true}); mkdirSync(evidence);
  const pin = {schema: 'lycheedev.selection.v1', id: 'PIN-fixture', source: {repository: 'wow-ui-source', product: 'retail', requestedRef: BASE_COMMIT, exactCommit: BASE_COMMIT, parserRevision: 'source-v1'}};
  const path = join(home, 'state/toolkit.sqlite'), db = new DatabaseSync(path); db.exec('PRAGMA user_version = 1; CREATE TABLE documents(key TEXT PRIMARY KEY, generation INTEGER, value BLOB); CREATE TABLE deleted_generations(key TEXT PRIMARY KEY, generation INTEGER);'); db.prepare('INSERT INTO documents VALUES (?,1,?)').run(`pin/${pin.id}`, Buffer.from(JSON.stringify(pin))); db.close();
  return {d, home, evidence, pin, path};
}
test('same-run metadata capture checks both exact schema and complete persisted selection', t => {
  const f = metadataFixture(t); assert.equal(captureMetadata(f.home, f.pin, f.evidence, 'go').sha256.length, 64);
  assert.throws(() => captureMetadata(f.home, {...f.pin, source: {...f.pin.source, product: 'classic'}}, f.evidence, 'rust'));
  writeFileSync(f.path + '-wal', 'pending'); assert.throws(() => captureMetadata(f.home, f.pin, f.evidence, 'rust'), /nonempty WAL/); rmSync(f.path + '-wal');
  const db = new DatabaseSync(f.path); db.exec('CREATE TABLE unexpected(value TEXT)'); db.close(); assert.throws(() => captureMetadata(f.home, f.pin, f.evidence, 'rust'));
});
test('pack2 index discovery follows only its exact metadata fingerprint and streams the bound bytes', t => {
  const f = metadataFixture(t), p = f.pin.source;
  const identity = sha256(['rust-index-v14', 'facts-pack-v2-path-radix512', 'facts-layout-v11-go323-generated-origins', 'TS0.26.3', 'Lua0.5.0', 'XML0.38', 'generator-v6', p.repository, p.product, p.exactCommit, p.parserRevision].join('\0'));
  const bytes = Buffer.from('{"documents":[]}'), hash = sha256(bytes), blob = join(f.home, 'cache/objects', hash.slice(0, 2), hash.slice(2)); mkdirSync(resolve(blob, '..'), {recursive: true}); writeFileSync(blob, bytes);
  let db = new DatabaseSync(f.path); db.prepare('INSERT INTO documents VALUES (?,1,?)').run(`source/rust-index/${identity}`, Buffer.from(JSON.stringify({schema: 'lycheedev.source-index.rust.v6', identity, blob: {sha256: hash, bytes: bytes.length}}))); db.close();
  const index = indexEvidence(f.home, 'rust', f.pin, f.evidence, f.d); assert.equal(index.sha256, hash); assert.equal(index.parts.parts.length, 1);
  db = new DatabaseSync(f.path); db.prepare('DELETE FROM documents WHERE key = ?').run(`source/rust-index/${identity}`); db.close(); const other = join(f.d, 'other'); mkdirSync(other); assert.throws(() => indexEvidence(f.home, 'rust', f.pin, other, f.d), /exact pack2 index manifest/);
});

test('closed metadata with empty WAL and retained SHM is copied read-only without changing original bytes or identities', t => {
  const f = metadataFixture(t); let db = new DatabaseSync(f.path); db.exec('PRAGMA journal_mode = WAL'); db.close();
  writeFileSync(f.path + '-wal', Buffer.alloc(0)); writeFileSync(f.path + '-shm', Buffer.alloc(32768));
  const before = metadataIdentity(f.home), result = captureMetadata(f.home, f.pin, f.evidence, 'go');
  assert.deepEqual(metadataIdentity(f.home), before); assert.deepEqual(result.original, before); assert.equal(result.sha256, before.database.sha256);
  assert.equal(before['-wal'].bytes, 0); assert.equal(before['-shm'].bytes, 32768);
  const clone = join(f.d, 'clone'); cloneSeed(f.home, clone, inventory(f.home)); const other = join(f.d, 'clone-evidence'); mkdirSync(other); captureMetadata(clone, f.pin, other, 'go');
});
test('nonempty WAL and rollback journal still fail without deleting sidecars', t => {
  for (const suffix of ['-wal', '-journal']) { const f = metadataFixture(t); writeFileSync(f.path + suffix, suffix === '-wal' ? 'frame' : ''); assert.throws(() => captureMetadata(f.home, f.pin, f.evidence, 'go'), /WAL|journal/); assert(existsSync(f.path + suffix)); }
});
test('metadata sidecar content/identity changes during copy are rejected', t => {
  for (const mutate of [f => writeFileSync(f.path + '-wal', 'x'), f => writeFileSync(f.path + '-shm', Buffer.alloc(32768, 1)), f => rmSync(f.path + '-shm')]) {
    const f = metadataFixture(t); writeFileSync(f.path + '-wal', ''); writeFileSync(f.path + '-shm', Buffer.alloc(32768)); assert.throws(() => captureMetadata(f.home, f.pin, f.evidence, 'go', {afterCopy: () => mutate(f)}), /WAL|changed/);
  }
});

const original7d8Inputs = {
  "tools/rust-source-candidate-diagnostics.mjs": "5579c2c6c3361f72fa6758d6746641437aa30754d19cb3ea80c44043cccd21b7",
  "tests/rust-performance-paired/source-candidates.mjs": "a922cf48ef09ca138a88855787b4855c78deb0c110781ea5f2fde71fb87230f0",
  "tests/rust-performance-paired/read-fact-delta.mjs": "7a2808850291606e286befe14bb154ca394d799585f39c5af279135990313450",
  "tests/rust-performance-paired/inputs.mjs": "9e8039a6b91d8b78d0ad6af8adb5ae0f290e80823d4a8118a59aefb86d3654d5",
  "tests/rust-performance-paired/contract.mjs": "987dc40840d518e2e61094d2120bf0e7380d6ad04797f7d959aad3d3505a0b5f",
  "tests/rust-source-full-corpus/assertions.mjs": "6a77312d8eeeba8727d35df1a941d852af409d829277cf120cf03e4184ede713",
  "tests/rust-source-full-corpus/retail-oracle.json": "5ab0df2063ba65834106c28e0d0bd4743d1071dd270774d66da9168bfbd21598",
  "tests/rust-parity/describe-contract.mjs": "76f717e786ae2cf8993e6933245c5e3fd8d3a752abf0e6c22fa9e34136d4dd21",
  "tests/rust-parity/oracle.mjs": "0490c98394b8125c4de4feb1834c25f3399027a772039780d9efb80a5506ba60",
  "tools/rust-build.mjs": "427673dc44a4ff6cdb662a77a25d5b0089ad9fc6b51720f71f5dffac5a279856",
  "tools/rust-disk.mjs": "54122b03b7b474f1f2119a31caa85d2217aeb5ea97e65f28a7e0161bebc203e5",
  "tools/rust-emmy-identity.mjs": "200421e639365e9c410467df04f7d4880bc72aa1b636168e5309045fc5514d97",
  "tools/rust-owned-process.mjs": "e63e8a9f09b6d00279f48ae7b4431623ec2f0d0cdca9b82cea48f14c345d2b34",
  "tools/rust-owned-process.ps1": "eb30686906592bc78f6c3783afc848bcad3a81f382d8761b05e043e8e21d9079",
  "tools/rust-owned-memory.mjs": "5ea696d6d65a380a90cc6f589c2086632dbb89560c3c2235ccb579b4d9745841",
  "tools/rust-build-auxiliary.json": "60fd549add2f19797f1a72632372d43a383eb1f9245b207a6c74fa4329173637",
  "tools/zip.mjs": "d392eb4c677b680642d88eea083e0635ee67f8dc0b30516b00217904227e9888",
  "release/rust-disk-policy.json": "672e33453bcb1333e6e7ab0cbe267249ae93e0d53ff0800d560f723543290b6d",
  "tools/rust-source-pack-diagnostics.mjs": "6fca635b0fd331d18e40947cc40ce954c3a23da492e64875644f371304e7c14d",
  "tools/rust-source-pack-evidence.mjs": "b7e9d4e0c3c7f1ea566a0f18a9b957d6ccc895cfbd903231a1fdd2ab3928da72",
  "tests/rust-performance-paired/source-pack-scenarios.mjs": "3b4cef989bb07323edcb315e3e03de2dc26fd178683b1ae293d400818e4c0d8f",
  "tests/rust-performance-paired/source-pack-resume.mjs": "82d2a82221bb1db1063ad5b6d99a8b3316e3d5621de5fe0038e10ac55aa6eadb",
  "tests/rust-performance-paired/source-pack-custody.mjs": "546c1034561c80d13e59f5cd3ae1e27984eb97ee9348a3ed2db4d239c2b05a39",
  "tests/rust-source-native-acceptance/assertions.mjs": "1f0f0dad45b321481f121df852cbd741f9d2e9dd59447f4fc3be34c279662768",
  "tests/rust-skill-usage/assertions.mjs": "19872afbfdf1da43654c4cdfb9305853a97c348eca3acef97f5e224d7aa31834"
};
function retainedFixture(t) {
  const d = temp(t), request = requestAt(d), dirs = Object.fromEntries(['outer', 'base'].map(role => [role, join(d, '.tmp/rust-disk/evidence', RETAINED_COLD[role])])), homes = Object.fromEntries(['go', 'rust'].map(e => [e, join(d, '.tmp/rust-scratch', RETAINED_COLD.base, `${e}-home`)]));
  for (const p of [...Object.values(dirs), ...Object.values(homes).map(p => join(p, 'state')), join(dirs.base, 'cold')]) mkdirSync(p, {recursive: true});
  const put = (p, value) => writeFileSync(p, JSON.stringify(value, null, 2) + '\n');
  const pin = fixturePin({repository: 'wow-ui-source', product: 'retail', requestedRef: BASE_COMMIT, exactCommit: BASE_COMMIT, parserRevision: 'source-v1'});
  const cold = {id: 'cold', mode: 'cold', order: ['go', 'rust'], state: 'failed', factsEquivalent: false, failure: 'closed checkpointed metadata required', pin, lineage: {state: 'authenticated-own-sync-only'}}, preparation = [];
  for (const engine of ['go', 'rust']) {
    writeFileSync(join(homes[engine], 'state/toolkit.sqlite'), 'metadata');
    const stem = join(dirs.base, 'cold', `process-${RETAINED_COLD[engine]}`), binary = join(request[engine === 'rust' ? 'candidate' : 'go'].sourceRoot, `${engine}.exe`), cwd = request[engine === 'rust' ? 'candidate' : 'go'].sourceRoot;
    const req = {schema: 'lycheedev.owned-request.v2', runId: RETAINED_COLD[engine], command: binary, args: ['source', 'index', '--snapshot', pin.id, '--home', homes[engine], '--format', 'json'], cwd, timeoutMs: 210000, orphanMs: 30000, cargoBuild: false, msvcCompile: false};
    for (const [key, suffix] of [['stdout', '.stdout.bin'], ['stderr', '.stderr.bin'], ['receipt', '.receipt.json'], ['identity', '.identity.json'], ['rootIdentity', '.root.json'], ['memoryPrefix', '.memory'], ['memoryMarker', '.phase']]) req[key] = stem + suffix;
    put(stem + '.request.json', req); put(req.stdout, {context: {snapshot: pin.id}}); writeFileSync(req.stderr, 'profile');
    const p = {...proof(), runId: req.runId, binary, exitCode: 0, pid: 10, creationFileTime: '100', startupIdentity: req.identity, rootIdentity: req.rootIdentity}; put(req.receipt, p);
    put(req.identity, {schema: 'lycheedev.owned-start.v1', runId: req.runId, requestSHA256: hashFile(stem + '.request.json'), wrapperObservedSHA256: original7d8Inputs['tools/rust-owned-process.ps1'], rootAssignment: 'PROC_THREAD_ATTRIBUTE_JOB_LIST', historicalReapProof: false});
    put(req.rootIdentity, {schema: 'lycheedev.owned-root.v1', runId: req.runId, pid: p.pid, creationFileTime: p.creationFileTime, jobAssignedAtCreation: true, historicalReapProof: false}); put(stem + '.request.json.wrapper.json', {status: 0});
    cold[engine] = {home: homes[engine], exit: 0, valid: true, handlesReaped: true, binary, cwd, args: req.args, proof: p, ...elapsed(p), stdoutSHA256: hashFile(req.stdout), stderrSHA256: hashFile(req.stderr), memory: {state: 'sampled_with_gaps', gaps: ['fixture gap']}};
    put(join(dirs.base, `${engine}-verified-corpus-manifest.json`), {commit: BASE_COMMIT, files: []});
    const metadata = {checkpointed: true, documents: 1, deletedGenerations: 0, documentKey: `pin/${pin.id}`, evidenceCopy: join(dirs.base, `${engine}-seed-metadata.sqlite`), sha256: sha256('seed')}; writeFileSync(metadata.evidenceCopy, 'seed');
    preparation.push({label: `${engine}-authenticated-sync-only`, home: homes[engine], pin, metadata, identity: {rows: [], sha256: sha256('[]')}});
  }
  const oldReport = {schema: 'lycheedev.source-pack-scenarios.v1', state: 'source_pack_scenarios_failed', blocker: 'closed checkpointed metadata required', finalIntegrityVerified: false, requiresOuterRetirement: false, importedSamples: 0, request, inputs: original7d8Inputs, scenarios: [cold], preparation};
  put(join(dirs.outer, 'report.json'), oldReport); put(join(dirs.outer, 'configuration.json'), {request, inputs: original7d8Inputs, evidence: dirs.outer, scratch: join(d, '.tmp/rust-scratch', RETAINED_COLD.outer)});
  put(join(dirs.outer, 'owned-report.json'), {schema: 'lycheedev.source-pack-scenarios-owned.v1', state: 'failed', process: {exit: 1, handlesReaped: true, proof: proof()}, request, inputs: original7d8Inputs});
  const seal = role => {
    const logs = evidenceFiles(dirs[role]).map(path => ({path, original: path, sha256: hashFile(path)}));
    put(join(dirs[role], 'receipt.json'), {schema: 'lycheedev.disk-owner.v1', id: RETAINED_COLD[role], owner: role === 'outer' ? 'source-pack2-diagnostics' : 'source-pack2-healthy-base', category: 'scratch', retention: 'rebuildable', state: 'failed', handlesReaped: true, activePid: null, sealedAt: 'fixture', directory: join(d, '.tmp/rust-scratch', RETAINED_COLD[role]), inputHashes: role === 'outer' ? {runner: original7d8Inputs} : original7d8Inputs, sealedLogs: logs, logHashes: Object.fromEntries(logs.map(log => [relative(d, log.path), log.sha256]))});
  };
  seal('outer'); seal('base'); const reference = {schema: 'lycheedev.source-pack2-retained-cold.v1', outerReceiptSHA256: hashFile(join(dirs.outer, 'receipt.json')), baseReceiptSHA256: hashFile(join(dirs.base, 'receipt.json')), homeInventorySHA256: Object.fromEntries(['go', 'rust'].map(e => [e, inventory(homes[e]).sha256]))};
  const reseal = role => { seal(role); reference[`${role}ReceiptSHA256`] = hashFile(join(dirs[role], 'receipt.json')); };
  return {d, request, dirs, homes, reference, reseal, put};
}
test('fixed cold references authenticate all original evidence and current homes without adopting the old owners', t => {
  const f = retainedFixture(t), auth = authenticateRetainedCold(f.request, f.reference); assert.deepEqual(inspectRetainedCold(f.request), f.reference); auth.verifyHomes(); auth.verifyEvidence(); assert.equal(auth.cold.state, 'failed'); assert.equal(auth.outputs.go.row.memory.gaps.length, 1);
  assert.throws(() => validateColdReference({...f.reference, arbitraryHome: '/tmp/foreign'}));
  writeFileSync(join(f.dirs.outer, 'report.json'), '{}'); assert.throws(auth.verifyEvidence, /original failed evidence changed/);
});
for (const fault of ['receipt', 'unsealed', 'raw', 'home', 'active', 'unreaped', 'foreign-home', 'wrong-run', 'request-command', 'runner', 'extra-scenario', 'seeded', 'empty-wal', 'nonempty-wal']) test(`retained cold authentication ${fault === 'empty-wal' ? 'accepts' : 'rejects'} ${fault}`, t => {
  const f = retainedFixture(t), outer = join(f.dirs.outer, 'receipt.json'), reportPath = join(f.dirs.outer, 'report.json'), reqPath = join(f.dirs.base, 'cold', `process-${RETAINED_COLD.go}.request.json`);
  const edit = (path, fn) => { const value = JSON.parse(readFileSync(path)); fn(value); f.put(path, value); };
  if (fault === 'receipt') writeFileSync(outer, '{}');
  if (fault === 'unsealed') writeFileSync(join(f.dirs.base, 'unsealed'), 'x');
  if (fault === 'raw') writeFileSync(reqPath.replace('.request.json', '.stdout.bin'), 'x');
  if (fault === 'home') writeFileSync(join(f.homes.go, 'extra'), 'x');
  if (['active', 'unreaped'].includes(fault)) { edit(outer, r => { if (fault === 'active') r.state = 'active'; else r.handlesReaped = false; }); f.reference.outerReceiptSHA256 = hashFile(outer); }
  if (fault === 'foreign-home') { edit(reportPath, r => r.scenarios[0].go.home = f.homes.rust); f.reseal('outer'); }
  if (fault === 'wrong-run' || fault === 'request-command') { edit(reqPath, r => { if (fault === 'wrong-run') r.runId = RETAINED_COLD.rust; else r.args = ['source', 'sync']; }); f.reseal('base'); }
  if (fault === 'runner') { edit(join(f.dirs.outer, 'configuration.json'), r => r.inputs['tools/rust-source-pack-diagnostics.mjs'] = 'f'.repeat(64)); f.reseal('outer'); }
  if (fault === 'extra-scenario') { edit(reportPath, r => r.scenarios.push({id: 'warm'})); f.reseal('outer'); }
  if (fault === 'seeded') { edit(reportPath, r => { const seed = r.preparation[0]; seed.identity.rows = [{path: 'source/derived', bytes: 1, sha256: sha256('x')}]; seed.identity.sha256 = sha256(JSON.stringify(seed.identity.rows)); }); f.reseal('outer'); }
  if (fault.endsWith('-wal')) { writeFileSync(join(f.homes.go, 'state/toolkit.sqlite-wal'), fault === 'empty-wal' ? '' : 'frames'); f.reference.homeInventorySHA256.go = inventory(f.homes.go).sha256; }
  if (fault === 'empty-wal') authenticateRetainedCold(f.request, f.reference); else assert.throws(() => authenticateRetainedCold(f.request, f.reference));
});
test('continuation reuses exactly the two original cold samples, seals full cold custody before warm and never cleans old homes', async t => {
  const f = await fixture(t, {resume: true}); assert.equal(f.report.state, 'source_pack_scenarios_observed', f.report.blocker); assert.equal(f.samples.length, 10); assert.equal(f.samples[0].entry.id, 'warm'); assert.equal(f.report.continuation.freshColdCalls, 0); assert(!f.events.includes('sync-seeds')); assert(!f.events.includes('begin:source-pack2-healthy-base'));
  assert(f.events.indexOf('seal:source-pack2-cold-revalidation:succeeded') < f.events.indexOf('claim-borrow')); assert(f.events.indexOf('claim-borrow') < f.events.indexOf('sample:warm:rust')); assert(f.owners.every(o => o.cleaned));
  for (const engine of ['go', 'rust']) assert(existsSync(join(f.directory, `retained-${engine}-home`)));
  verifyObservedReport({status: 0, handlesReaped: true, proof: proof()}, f.report, f.request, f.inputs, f.resumeCold); assert.throws(() => verifyObservedReport({status: 0, handlesReaped: true, proof: proof()}, f.report, f.request, f.inputs));
});
for (const [fault, calls] of [[{otherMachine: true}, 0], [{authentication: true}, 0], [{coldCustody: true}, 0], [{claimed: true}, 0], [{facts: 'cold'}, 0], [{unreapedAt: 2}, 1], [{originalChanged: true}, 2], [{admission: true}, 2]]) test(`continuation preserves evidence and does not replace calls after ${JSON.stringify(fault)}`, async t => {
  const f = await fixture(t, {resume: true, ...fault}); assert.equal(f.report.state, 'source_pack_scenarios_failed'); assert.equal(f.samples.length, calls); for (const engine of ['go', 'rust']) assert(existsSync(join(f.directory, `retained-${engine}-home`)));
});

test('dangling metadata sidecar symlinks are not mistaken for missing files', t => {
  for (const suffix of ['-wal', '-shm']) { const f = metadataFixture(t); symlinkSync(join(f.d, 'missing-target'), f.path + suffix, 'junction'); assert.throws(() => metadataIdentity(f.home), /WAL|file\/type/); }
});

function borrowFixture(t) {
  const f = retainedFixture(t), before = Object.fromEntries(['go', 'rust'].map(e => [e, inventory(f.homes[e])])), options = {requiredBytes: 0, scratchReserveBytes: 0, freeBytes: 10 * 1024 ** 3};
  const custody = beginScratch(f.d, 'source-pack2-cold-revalidation', {inputs: {retainedCold: f.reference}, admissionOptions: options});
  f.put(join(custody.evidence, 'cold-revalidated.json'), {state: 'verified', factsEquivalent: true, facts: {fullFieldsAndOrderEqual: true, documents: 4036}, lineage: {reference: f.reference}});
  f.put(join(custody.evidence, 'cold-facts-custody.json'), {files: ['fixture']}); custody.seal({state: 'succeeded', handlesReaped: true, logs: evidenceFiles(custody.evidence)}); custody.cleanup();
  const owner = beginScratch(f.d, 'source-pack2-cold-continuation', {inputs: {retainedCold: f.reference, borrowedHomes: f.homes, beforeInventory: f.reference.homeInventorySHA256}, admissionOptions: options});
  return {...f, before, custody, owner};
}
test('single-use borrow claim binds new active owner, exact homes and already sealed complete cold custody', t => {
  const f = borrowFixture(t), old = hashFile(join(f.dirs.base, 'receipt.json')), claim = claimColdBorrow(f.d, f.reference, f.owner, f.before, f.custody.receipt);
  assert.equal(hashFile(claim.path), claim.sha256); assert.equal(hashFile(join(f.dirs.base, 'receipt.json')), old); const value = JSON.parse(readFileSync(claim.path)); assert.deepEqual(value.homes, f.homes); assert.equal(value.cleanupScope, f.owner.directory);
  assert.throws(() => claimColdBorrow(f.d, f.reference, f.owner, f.before, f.custody.receipt), /EEXIST/); assert.throws(() => authenticateRetainedCold(f.request, f.reference), /already claimed/);
  f.owner.seal({state: 'succeeded', handlesReaped: true, logs: evidenceFiles(f.owner.evidence)}); f.owner.cleanup(); for (const home of Object.values(f.homes)) assert(existsSync(home));
});
for (const fault of ['foreign-owner', 'active-custody', 'changed-custody', 'changed-home', 'wrong-before']) test(`borrow claim rejects ${fault} before creating the marker`, t => {
  const f = borrowFixture(t), edit = (path, fn) => { const value = JSON.parse(readFileSync(path)); fn(value); f.put(path, value); };
  if (fault === 'foreign-owner') edit(f.owner.receipt, r => r.owner = 'unrelated');
  if (fault === 'active-custody') edit(f.custody.receipt, r => r.state = 'active');
  if (fault === 'changed-custody') writeFileSync(join(f.custody.evidence, 'cold-facts-custody.json'), '{}');
  if (fault === 'changed-home') writeFileSync(join(f.homes.go, 'extra'), 'x');
  if (fault === 'wrong-before') f.before.go.sha256 = 'f'.repeat(64);
  assert.throws(() => claimColdBorrow(f.d, f.reference, f.owner, f.before, f.custody.receipt)); assert(!existsSync(join(f.d, '.tmp/rust-disk', `source-pack2-cold-${RETAINED_COLD.base}.json`)));
});
test('cold custody streams only complete Facts files and leaves original homes unchanged', t => {
  const d = temp(t), homes = {go: join(d, 'go'), rust: join(d, 'rust')}, out = join(d, 'out'); mkdirSync(out);
  for (const [e, path] of [['go', 'source/v1/facts/files/source-v1/a.json'], ['rust', 'cache/source-facts-rust/a.json']]) { mkdirSync(resolve(homes[e], path, '..'), {recursive: true}); writeFileSync(join(homes[e], path), Buffer.alloc((8 << 20) + 1, 1)); writeFileSync(join(homes[e], 'unrelated'), 'not copied'); }
  const before = Object.fromEntries(Object.entries(homes).map(([e, home]) => [e, inventory(home)])); assert.equal(captureColdFactFiles(homes, before, out, d), 2);
  const catalog = JSON.parse(readFileSync(join(out, 'cold-facts-custody.json'))); assert(catalog.files.every(f => !f.relative.includes('unrelated'))); for (const f of catalog.files) { assert.equal(hashFile(f.manifest), f.manifestSHA256); const parts = JSON.parse(readFileSync(f.manifest)); assert.equal(parts.parts.length, 2); }
  for (const [e, home] of Object.entries(homes)) assert.equal(inventory(home).sha256, before[e].sha256);
});

test('remaining-only continuation makes eight calls without repeating cold warm sync or base copy', async t => {
  const f = await fixture(t, {remaining: true, allowance: REMAINING_ALLOWANCE}); assert.equal(f.report.state, 'source_pack_scenarios_observed', f.report.blocker); assert.equal(f.samples.length, 8); assert.equal(f.samples[0].entry.id, 'single-content-change'); assert.equal(f.report.continuation.originalCallsReused, 4); assert.equal(f.report.continuation.freshWarmCalls, 0); assert(!f.events.includes('sync-seeds')); assert(f.owners.every(o => o.cleaned)); assert.equal(f.owners.length, 9);
  assert(f.owners.every(o => o.options.limits.scratchBytes === 6 * 1024 ** 3)); assert(f.admissions.every(o => o.limits.scratchBytes === 6 * 1024 ** 3 && o.limits.cacheBytes === 20 * 1024 ** 3 && o.limits.freeReserveBytes === 2 * 1024 ** 3));
  assert(f.events.indexOf('claim-remaining') < f.events.indexOf('sample:single-content-change:go')); for (const e of ['go', 'rust']) assert(existsSync(join(f.directory, `retained-${e}-home`)));
  verifyObservedReport({status: 0, handlesReaped: true, proof: proof()}, f.report, f.request, f.inputs, undefined, f.resumeRemaining); assert.throws(() => verifyObservedReport({status: 0, handlesReaped: true, proof: proof()}, f.report, f.request, f.inputs));
});
test('remaining without explicit allowance keeps every admission at 5 GiB', async t => { const f = await fixture(t, {remaining: true}); assert.equal(f.report.state, 'source_pack_scenarios_observed', f.report.blocker); assert(f.owners.every(o => o.options.limits.scratchBytes === 5 * 1024 ** 3)); assert(f.admissions.every(o => o.limits.scratchBytes === 5 * 1024 ** 3)); });
test('fixed 6 GiB allowance rejects other scope amount authorization extra keys and old cold mode', async t => {
  const d = temp(t), reference = {schema: 'lycheedev.source-pack2-retained-warm.v1', outerReceiptSHA256: 'a'.repeat(64), baseReceiptSHA256: 'b'.repeat(64), homeInventorySHA256: {go: 'c'.repeat(64), rust: 'd'.repeat(64)}};
  assert.equal(assertDiskPolicy(d, {}, REMAINING_ALLOWANCE, reference).scratchBytes, 6 * 1024 ** 3); assert.equal(assertDiskPolicy(d, {}).scratchBytes, 5 * 1024 ** 3);
  for (const change of [v => v.scratchBytes++, v => v.scope = RETAINED_COLD.outer, v => v.authorizationReference = 'unapproved', v => v.freeReserveBytes = 0]) { const value = structuredClone(REMAINING_ALLOWANCE); change(value); assert.throws(() => assertDiskPolicy(d, {}, value, reference)); }
  assert.throws(() => assertDiskPolicy(d, {}, REMAINING_ALLOWANCE)); assert.throws(() => assertDiskPolicy(d, {LYCHEEDEV_DISK_SCRATCH_GIB: '6'}, REMAINING_ALLOWANCE, reference));
  let started = false; await assert.rejects(runController({request: {ownerRoot: d}, allowance: REMAINING_ALLOWANCE}, {beginScratch() { started = true; }})); assert.equal(started, false);
});
for (const [fault, calls] of [[{remainingAuthentication: true}, 0], [{remainingClaimed: true}, 0], [{admission: true}, 0], [{failAt: 4}, 1], [{unreapedAt: 4}, 1], [{facts: 'changed'}, 2], [{remainingIntegrity: true}, 8]]) test(`remaining continuation stops without replacement calls for ${JSON.stringify(fault)}`, async t => { const f = await fixture(t, {remaining: true, allowance: REMAINING_ALLOWANCE, ...fault}); assert.equal(f.report.state, 'source_pack_scenarios_failed'); assert.equal(f.samples.length, calls); for (const e of ['go', 'rust']) assert(existsSync(join(f.directory, `retained-${e}-home`))); if (fault.admission) { assert.equal(f.report.scenarios[2].state, 'not_run'); assert.equal(f.report.scenarios[2].blockedBeforeOwner, 'go'); assert(!f.events.includes('claim-remaining')); } });

const be168Inputs = {
  "tools/rust-source-candidate-diagnostics.mjs": "5579c2c6c3361f72fa6758d6746641437aa30754d19cb3ea80c44043cccd21b7",
  "tests/rust-performance-paired/source-candidates.mjs": "a922cf48ef09ca138a88855787b4855c78deb0c110781ea5f2fde71fb87230f0",
  "tests/rust-performance-paired/read-fact-delta.mjs": "7a2808850291606e286befe14bb154ca394d799585f39c5af279135990313450",
  "tests/rust-performance-paired/inputs.mjs": "9e8039a6b91d8b78d0ad6af8adb5ae0f290e80823d4a8118a59aefb86d3654d5",
  "tests/rust-performance-paired/contract.mjs": "987dc40840d518e2e61094d2120bf0e7380d6ad04797f7d959aad3d3505a0b5f",
  "tests/rust-source-full-corpus/assertions.mjs": "6a77312d8eeeba8727d35df1a941d852af409d829277cf120cf03e4184ede713",
  "tests/rust-source-full-corpus/retail-oracle.json": "5ab0df2063ba65834106c28e0d0bd4743d1071dd270774d66da9168bfbd21598",
  "tests/rust-parity/describe-contract.mjs": "76f717e786ae2cf8993e6933245c5e3fd8d3a752abf0e6c22fa9e34136d4dd21",
  "tests/rust-parity/oracle.mjs": "0490c98394b8125c4de4feb1834c25f3399027a772039780d9efb80a5506ba60",
  "tools/rust-build.mjs": "427673dc44a4ff6cdb662a77a25d5b0089ad9fc6b51720f71f5dffac5a279856",
  "tools/rust-disk.mjs": "54122b03b7b474f1f2119a31caa85d2217aeb5ea97e65f28a7e0161bebc203e5",
  "tools/rust-emmy-identity.mjs": "200421e639365e9c410467df04f7d4880bc72aa1b636168e5309045fc5514d97",
  "tools/rust-owned-process.mjs": "e63e8a9f09b6d00279f48ae7b4431623ec2f0d0cdca9b82cea48f14c345d2b34",
  "tools/rust-owned-process.ps1": "eb30686906592bc78f6c3783afc848bcad3a81f382d8761b05e043e8e21d9079",
  "tools/rust-owned-memory.mjs": "5ea696d6d65a380a90cc6f589c2086632dbb89560c3c2235ccb579b4d9745841",
  "tools/rust-build-auxiliary.json": "60fd549add2f19797f1a72632372d43a383eb1f9245b207a6c74fa4329173637",
  "tools/zip.mjs": "d392eb4c677b680642d88eea083e0635ee67f8dc0b30516b00217904227e9888",
  "release/rust-disk-policy.json": "672e33453bcb1333e6e7ab0cbe267249ae93e0d53ff0800d560f723543290b6d",
  "tools/rust-source-pack-diagnostics.mjs": "5ee07391735b8dc02382ef17deb9e97ff0803cc670172d7b3d70038676bf2810",
  "tools/rust-source-pack-evidence.mjs": "b7e9d4e0c3c7f1ea566a0f18a9b957d6ccc895cfbd903231a1fdd2ab3928da72",
  "tests/rust-performance-paired/source-pack-scenarios.mjs": "325551c68a9a7eb9af2ef9b3382fa3dd11aca50b685a1272d4eb56dd263c833b",
  "tests/rust-performance-paired/source-pack-resume.mjs": "82d2a82221bb1db1063ad5b6d99a8b3316e3d5621de5fe0038e10ac55aa6eadb",
  "tests/rust-performance-paired/source-pack-custody.mjs": "546c1034561c80d13e59f5cd3ae1e27984eb97ee9348a3ed2db4d239c2b05a39",
  "tests/rust-source-native-acceptance/assertions.mjs": "1f0f0dad45b321481f121df852cbd741f9d2e9dd59447f4fc3be34c279662768",
  "tests/rust-skill-usage/assertions.mjs": "19872afbfdf1da43654c4cdfb9305853a97c348eca3acef97f5e224d7aa31834"
};
function remainingFixture(t) {
  const f = retainedFixture(t), paths = {...f.dirs, outer: join(f.d, '.tmp/rust-disk/evidence', RETAINED_WARM.outer), base: join(f.d, '.tmp/rust-disk/evidence', RETAINED_WARM.base), custody: join(f.d, '.tmp/rust-disk/evidence', RETAINED_WARM.custody), claim: join(f.d, '.tmp/rust-disk', `source-pack2-cold-${RETAINED_COLD.base}.json`)};
  for (const p of [paths.outer, paths.base, paths.custody, join(paths.base, 'warm')]) mkdirSync(p, {recursive: true});
  const original = JSON.parse(readFileSync(join(f.dirs.outer, 'report.json'))), cold = structuredClone(original.scenarios[0]), warm = {id: 'warm', mode: 'warm', order: ['rust', 'go'], pin: cold.pin, state: 'verified', factsEquivalent: true};
  const rows = Array.from({length: 4036}, (_, i) => ({path: `p${String(i).padStart(4, '0')}.lua`, bytes: 0, sha256: sha256(''), gitSHA1: 'a'.repeat(40)})), oracle = variantOracle({repository: 'wow-ui-source', product: 'retail'}, rows, BASE_COMMIT), indexes = {};
  for (const e of ['go', 'rust']) { const path = join(f.homes[e], 'index-fixture'); writeFileSync(path, `${e}-index`); indexes[e] = {path, bytes: readFileSync(path).length, sha256: hashFile(path)}; }
  function calls(row, engine, directory, mode, id) {
    const stem = join(directory, `process-${id}`), previous = join(f.dirs.base, 'cold', `process-${RETAINED_COLD[engine]}`), req = JSON.parse(readFileSync(previous + '.request.json'));
    req.runId = id; for (const [key, suffix] of [['stdout', '.stdout.bin'], ['stderr', '.stderr.bin'], ['receipt', '.receipt.json'], ['identity', '.identity.json'], ['rootIdentity', '.root.json'], ['memoryPrefix', '.memory'], ['memoryMarker', '.phase']]) req[key] = stem + suffix; f.put(stem + '.request.json', req);
    const phases = Object.fromEntries(PHASE_NAMES.map(p => [p, 0])); phases.metadata = 1;
    const io = {cachedFacts: mode === 'cold' ? 0 : 4036, parsedDocuments: mode === 'cold' ? 4036 : 0, gitTreeProcesses: mode === 'cold' ? 1 : 0, gitBlobProcesses: mode === 'cold' ? 1 : 0, elapsedMs: 10, deadlineMs: 180000};
    if (mode === 'warm') { phases.cached_index = 1; Object.assign(io, {cachedIndex: true, indexBytesRead: 10, sourceBytesRead: 0}); }
    else { for (const name of ['index_lock', 'git_tree', 'git_blobs_inclusive', 'index_serialize', 'index_publish', 'manifest']) phases[name] = 1; Object.assign(phases, {facts_parse: 4036, facts_parse_sequential: 4036, facts_read: 4100, facts_serialize: 4036, facts_publish: 4036, symbols: 4036}); io.readBytes = 10; }
    const profile = {schema: 'lycheedev.source-index-phases.v1', timing: PHASE_TIMING, lockTiming: LOCK_TIMING, elapsedMicros: 100000, phases: PHASE_NAMES.map(phase => ({phase, calls: phases[phase], elapsedMicros: phases[phase]}))};
    const envelope = {schema: 'lycheedev.result.v1', ok: true, error: null, result: {schema: 'lycheedev.source-map.v2', storage: 'file-cache', parser: 'source-v1', repository: oracle.repository, product: oracle.product, commit: BASE_COMMIT, documents: 4036, declarations: 1, relationships: 0, assets: 0, diagnostics: 0, complete: true, io}, context: {snapshot: cold.pin.id}};
    f.put(req.stdout, envelope); writeFileSync(req.stderr, engine === 'rust' ? JSON.stringify(profile) + '\n' : '');
    const proofValue = {...proof(), runId: id, binary: req.command, exitCode: 0, pid: 10, creationFileTime: '100', startupIdentity: req.identity, rootIdentity: req.rootIdentity}; f.put(req.receipt, proofValue);
    f.put(req.identity, {schema: 'lycheedev.owned-start.v1', runId: id, requestSHA256: hashFile(stem + '.request.json'), wrapperObservedSHA256: be168Inputs['tools/rust-owned-process.ps1'], rootAssignment: 'PROC_THREAD_ATTRIBUTE_JOB_LIST', historicalReapProof: false}); f.put(req.rootIdentity, {schema: 'lycheedev.owned-root.v1', runId: id, pid: 10, creationFileTime: '100', jobAssignedAtCreation: true, historicalReapProof: false}); f.put(stem + '.request.json.wrapper.json', {status: 0});
    row[engine] = {home: f.homes[engine], binary: req.command, cwd: req.cwd, args: req.args, exit: 0, valid: true, handlesReaped: true, proof: proofValue, ...elapsed(proofValue), memory: {state: 'sampled_with_gaps'}, stdoutSHA256: hashFile(req.stdout), stderrSHA256: hashFile(req.stderr)};
    if (engine === 'rust') row.rust.sourceProfile = verifyPackProfile(readFileSync(req.stderr), envelope, {mode, documents: 4036});
    if (mode === 'warm') { const derived = {directory: f.homes[engine], files: [{path: 'fixture'}], sha256: 'd'.repeat(64)}; f.put(join(directory, `${engine}-warm-before.json`), derived); f.put(join(directory, `${engine}-warm-after.json`), derived); row[engine].warmReuse = JSON.parse(JSON.stringify(proveWarmReuse(engine, derived, derived, envelope))); assert(!Object.hasOwn(row[engine].warmReuse.before, 'evidence')); }
    return semantic('B02-retail-index', envelope, engine, oracle, cold.pin.id).value;
  }
  for (const row of [cold, warm]) {
    const mode = row.id, directory = mode === 'cold' ? join(f.dirs.base, 'cold') : join(paths.base, 'warm'), evidence = mode === 'cold' ? paths.custody : directory; let summary;
    for (const e of ['go', 'rust']) summary = calls(row, e, directory, mode, mode === 'cold' ? RETAINED_COLD[e] : e === 'go' ? 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa' : 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb');
    const totals = {go: {declarations: 1, relationships: 0, loads: 0, headers: 0, diagnostics: 0}, rust: {declarations: 1, relationships: 0, loads: 0, headers: 0, diagnostics: 0}}, comparison = join(evidence, 'facts-comparison.json'); f.put(comparison, {documents: 4036, differentFiles: [], firstDifferences: [], totals, inputs: {rustLayout: 'v11-pack2', goRecords: indexes.go.path, rustIndexFile: indexes.rust.path, rustIndexSHA256: indexes.rust.sha256, rustIndexBytes: indexes.rust.bytes}});
    row.facts = {documents: 4036, fullFieldsAndOrderEqual: true, totals, indexes, summary, evidence: comparison}; row.state = 'verified'; row.factsEquivalent = true; delete row.failure;
  }
  original.scenarios[0] = {...cold, state: 'failed', factsEquivalent: false, failure: 'closed checkpointed metadata required'}; delete original.scenarios[0].facts; f.put(join(f.dirs.outer, 'report.json'), original); f.reseal('outer'); f.reseal('base');
  cold.lineage = {...cold.lineage, reference: f.reference}; f.put(join(paths.custody, 'cold-revalidated.json'), cold); f.put(join(paths.base, 'warm/warm-scenario.json'), warm);
  for (const e of ['go', 'rust']) f.put(join(paths.custody, `${e}-retained-verified-corpus-manifest.json`), {commit: BASE_COMMIT, files: rows});
  const baseIdentity = Object.fromEntries(['go', 'rust'].map(e => [e, inventory(f.homes[e])])); f.reference.homeInventorySHA256 = Object.fromEntries(['go', 'rust'].map(e => [e, baseIdentity[e].sha256])); f.put(join(paths.custody, 'borrow-before-inventories.json'), baseIdentity); f.put(join(paths.custody, 'cold-revalidated.json'), cold); f.put(join(paths.base, 'healthy-base-inventories.json'), baseIdentity); f.put(join(paths.base, 'borrow-after-inventories.json'), baseIdentity);
  const claim = {schema: 'lycheedev.source-pack2-cold-borrow.v1', owner: RETAINED_WARM.base, originalOwner: RETAINED_COLD.base, reference: f.reference, homes: f.homes, before: baseIdentity, custodyReceipt: join(paths.custody, 'receipt.json')}; f.put(paths.claim, claim); f.put(join(paths.base, 'borrow-claim.json'), claim);
  const continuation = {originalCallsReused: 2, freshColdCalls: 0, reference: f.reference, custodyReceipt: claim.custodyReceipt, borrow: {path: paths.claim, sha256: hashFile(paths.claim)}};
  const prior = {schema: 'lycheedev.source-pack-scenarios.v1', state: 'source_pack_scenarios_failed', blocker: 'disk.scratch_quota', requiresOuterRetirement: false, finalIntegrityVerified: false, importedSamples: 0, layout: 'v11-pack2', productCommit: CANDIDATE_COMMIT, request: f.request, inputs: be168Inputs, continuation, scenarios: [cold, warm, {id: 'single-content-change', state: 'running', mode: 'changed', factsEquivalent: false}], owners: [{role: 'read-only-cold-custody', receipt: claim.custodyReceipt}, {role: 'borrowed-healthy-base', receipt: join(paths.base, 'receipt.json')}], admissionFailure: {scratchBytes: 5005736596, scratchReserveBytes: 380939478, limits: {scratchBytes: 5 * 1024 ** 3}}};
  f.put(join(paths.outer, 'report.json'), prior); f.put(join(paths.outer, 'configuration.json'), {request: f.request, inputs: be168Inputs, resumeCold: f.reference, evidence: paths.outer, scratch: join(f.d, '.tmp/rust-scratch', RETAINED_WARM.outer)}); f.put(join(paths.outer, 'owned-report.json'), {schema: 'lycheedev.source-pack-scenarios-owned.v1', request: f.request, inputs: be168Inputs, state: 'failed', process: {exit: 1, handlesReaped: true, proof: proof()}});
  function seal(role) {
    const logs = evidenceFiles(paths[role]).map(path => ({path, original: path, sha256: hashFile(path)})), inputHashes = role === 'outer' ? {runner: be168Inputs, retainedCold: f.reference} : role === 'base' ? {...be168Inputs, retainedCold: f.reference, borrowedHomes: f.homes, beforeInventory: f.reference.homeInventorySHA256, coldCustodyReceipt: claim.custodyReceipt} : {...be168Inputs, retainedCold: f.reference};
    f.put(join(paths[role], 'receipt.json'), {schema: 'lycheedev.disk-owner.v1', id: RETAINED_WARM[role], owner: role === 'outer' ? 'source-pack2-diagnostics' : role === 'base' ? 'source-pack2-cold-continuation' : 'source-pack2-cold-revalidation', category: 'scratch', retention: 'rebuildable', state: role === 'custody' ? 'retired' : 'failed', handlesReaped: true, activePid: null, sealedAt: 'fixture', directory: join(f.d, '.tmp/rust-scratch', RETAINED_WARM[role]), inputHashes, sealedLogs: logs, logHashes: Object.fromEntries(logs.map(log => [relative(f.d, log.path), log.sha256]))});
  }
  for (const role of ['outer', 'base', 'custody']) seal(role);
  const reference = {schema: 'lycheedev.source-pack2-retained-warm.v1', outerReceiptSHA256: hashFile(join(paths.outer, 'receipt.json')), baseReceiptSHA256: hashFile(join(paths.base, 'receipt.json')), homeInventorySHA256: Object.fromEntries(['go', 'rust'].map(e => [e, baseIdentity[e].sha256]))}, bindings = {...RETAINED_WARM, oracle, custodyReceiptSHA256: hashFile(claim.custodyReceipt), coldSHA256: hashFile(join(paths.custody, 'cold-revalidated.json')), claimSHA256: hashFile(paths.claim)};
  const reseal = role => { seal(role); if (role === 'custody') bindings.custodyReceiptSHA256 = hashFile(claim.custodyReceipt); else reference[`${role}ReceiptSHA256`] = hashFile(join(paths[role], 'receipt.json')); };
  return {...f, paths, reference, bindings, reseal, baseIdentity, prior};
}
test('real remaining authenticator accepts saved/reloaded warm reuse and full fixed evidence chain', t => {
  const f = remainingFixture(t), value = authenticateRemaining(f.request, f.reference, f.bindings); assert.equal(value.warm.state, 'verified'); assert.equal(Object.keys(value.outputs).length, 4); value.verifyHomes(); value.verifyEvidence();
  assert.throws(() => authenticateRemaining(f.request, f.reference), /prior owner receipt changed/); // Synthetic fixtures cannot masquerade as published evidence pins.
  writeFileSync(f.paths.claim, '{}'); assert.throws(value.verifyEvidence, /prior completed evidence changed/);
});
for (const fault of ['old-claim', 'cold-custody', 'receipt', 'home', 'warm-reuse', 'warm-call', 'variant-owner', 'variant-call', 'runner', 'active-owner', 'unsealed', 'replayed', 'index-sha']) test(`real remaining authenticator rejects ${fault}`, t => {
  const f = remainingFixture(t), edit = (path, change) => { const value = JSON.parse(readFileSync(path)); change(value); f.put(path, value); };
  if (fault === 'old-claim') writeFileSync(f.paths.claim, '{}');
  if (fault === 'cold-custody') writeFileSync(join(f.paths.custody, 'facts-comparison.json'), '{}');
  if (fault === 'receipt') writeFileSync(join(f.paths.base, 'receipt.json'), '{}');
  if (fault === 'home') writeFileSync(join(f.homes.go, 'extra'), 'x');
  if (fault === 'warm-reuse') { edit(join(f.paths.base, 'warm/go-warm-after.json'), v => v.sha256 = 'f'.repeat(64)); f.reseal('base'); }
  if (fault === 'warm-call') { const path = join(f.paths.base, 'warm/process-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.stdout.bin'); edit(path, v => v.context.snapshot = 'wrong'); f.reseal('base'); }
  if (['variant-owner', 'variant-call', 'runner'].includes(fault)) { edit(join(f.paths.outer, fault === 'runner' ? 'configuration.json' : 'report.json'), v => { if (fault === 'runner') v.inputs['tools/rust-source-pack-diagnostics.mjs'] = 'f'.repeat(64); else if (fault === 'variant-owner') v.owners.push({role: 'single-content-change'}); else v.scenarios[2].go = {valid: true}; }); f.reseal('outer'); }
  if (fault === 'active-owner') { edit(join(f.paths.base, 'receipt.json'), v => v.state = 'active'); f.reference.baseReceiptSHA256 = hashFile(join(f.paths.base, 'receipt.json')); }
  if (fault === 'unsealed') writeFileSync(join(f.paths.custody, 'unsealed'), 'x');
  if (fault === 'replayed') writeFileSync(join(f.d, '.tmp/rust-disk', `source-pack2-remaining-${RETAINED_WARM.outer}.json`), '{}');
  if (fault === 'index-sha') { edit(join(f.paths.base, 'warm/facts-comparison.json'), v => v.inputs.rustIndexSHA256 = 'f'.repeat(64)); f.reseal('base'); }
  assert.throws(() => authenticateRemaining(f.request, f.reference, f.bindings));
});
test('remaining claim uses only the new owner, preserves the old claim and refuses replay or foreign directories', t => {
  const f = remainingFixture(t), options = {inputs: {retainedWarm: f.reference, readOnlyHomes: f.homes}, admissionOptions: {freeBytes: 10 * 1024 ** 3}}, owner = beginScratch(f.d, 'source-pack2-remaining-cases', options), before = hashFile(f.paths.claim);
  const receipt = JSON.parse(readFileSync(owner.receipt)); f.put(owner.receipt, {...receipt, directory: f.homes.go}); assert.throws(() => claimRemaining(f.d, f.reference, owner, f.baseIdentity)); f.put(owner.receipt, receipt);
  const claim = claimRemaining(f.d, f.reference, owner, f.baseIdentity); assert.equal(hashFile(claim.path), claim.sha256); assert.equal(hashFile(f.paths.claim), before); assert.throws(() => claimRemaining(f.d, f.reference, owner, f.baseIdentity), /EEXIST/); assert.throws(() => authenticateRemaining(f.request, f.reference, f.bindings), /already claimed/);
  owner.seal({state: 'succeeded', handlesReaped: true, logs: evidenceFiles(owner.evidence)}); owner.cleanup(); for (const home of Object.values(f.homes)) assert(existsSync(home));
});
test('complete cold custody accepts its bounded larger sealed-log inventory', t => {
  const f = remainingFixture(t); for (let i = 0; i < 2100; i++) writeFileSync(join(f.paths.custody, `extra-evidence-${i}`), ''); f.reseal('custody'); assert(JSON.parse(readFileSync(join(f.paths.custody, 'receipt.json'))).sealedLogs.length > 2000); authenticateRemaining(f.request, f.reference, f.bindings);
});
test('quota stop explicitly lists all unstarted remaining calls and no attempted calls', async t => { const f = await fixture(t, {remaining: true, admission: true}); assert.deepEqual(f.report.notRunCases, CASES.slice(2).map(c => c.id)); assert.equal(f.report.notRunInvocations.length, 8); assert.deepEqual(f.report.invocationAttempts, []); });
