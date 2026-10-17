import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {join, resolve} from 'node:path';
import {tmpdir} from 'node:os';
import {DatabaseSync} from 'node:sqlite';
import {sha256} from '../../tools/rust-disk.mjs';
import {captureMetadata, indexEvidence, assertDiskPolicy, CASES, BASE_COMMIT, CANDIDATE_COMMIT, CANDIDATE_IDENTITY, INSERT_TEXT, FIRST_PATH, RENAME_PATH, RUNNER_FILES, evidenceFiles, footprint, mutationRows, orderFor, runnerIdentity, runController, validateRequest, variantOracle, verifyObservedReport} from './source-pack-scenarios.mjs';
const root = resolve(import.meta.dirname, '../..');
const conditions = {machine: 'fixture', powerAndThermal: 'unknown', competingLoad: 'unknown', disk: 'fixture only', operator: 'Node test'};
function temp(t) { const d = mkdtempSync(join(tmpdir(), 'pack2-runner-')); t.after(() => { if (t.passed) rmSync(d, {recursive: true}); else t.diagnostic(`retained fixture ${d}`); }); return d; }
function proof(ms = 10) { return {schema: 'lycheedev.owned-process.v1', rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0, forced: false, timing: {schema: 'lycheedev.owned-timing.v1', frequency: 1000000, startTicks: '1000000', rootExitTicks: String(1000000 + ms * 1000 - 100), treeReapedTicks: String(1000000 + ms * 1000)}}; }
function requestAt(d) { const sourceRoot = join(d, 'product'), artifactRoot = join(d, 'artifacts'), goRoot = join(d, 'go'); for (const x of [sourceRoot, artifactRoot, goRoot]) mkdirSync(x); const artifact = join(artifactRoot, 'artifact.json'); writeFileSync(artifact, '{}'); return {schema: 'lycheedev.source-pack-scenarios-config.v1', ownerRoot: d, candidate: {sourceRoot, artifactRoot, artifact, commit: CANDIDATE_COMMIT, ...CANDIDATE_IDENTITY}, go: {sourceRoot: goRoot, packageRoot: goRoot, luaLSArchive: join(d, 'luals.zip'), packageSHA256: 'd'.repeat(64)}}; }
async function fixture(t, fault = {}) {
  const d = temp(t), request = requestAt(d), out = join(d, 'outer-evidence'), scratch = join(d, 'outer-scratch'); mkdirSync(out); mkdirSync(scratch);
  const inputs = {runner: 'fixed'}, owners = [], events = [], samples = [], modes = []; let calls = 0, identityChecks = 0;
  const basePin = {id: 'PIN-fixture', source: {exactCommit: BASE_COMMIT}}, baseOracle = JSON.parse(readFileSync(join(root, 'tests/rust-source-full-corpus/retail-oracle.json')));
  const svc = {
    runnerIdentity() { events.push('identity'); if (fault.identityAt === ++identityChecks) return {runner: 'changed'}; return inputs; },
    verifyCandidateRecord() { return {sourceBinary: 'rust.exe'}; }, verifyGoPackage() { return {binary: 'go.exe', packageSHA256: request.go.packageSHA256}; },
    admission() { events.push('admission'); },
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
  const report = await runController({request, runnerRoot: root, inputs, conditions, scratch, evidence: out}, svc);
  return {report, request, inputs, owners, events, samples, modes};
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
  const wrapper = readFileSync(join(root, 'tools/rust-source-pack-diagnostics.mjs'), 'utf8'); assert(wrapper.indexOf('assertDiskPolicy(request.ownerRoot)') < wrapper.indexOf('const job = beginScratch'));
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
  writeFileSync(f.path + '-wal', 'pending'); assert.throws(() => captureMetadata(f.home, f.pin, f.evidence, 'rust'), /checkpointed/); rmSync(f.path + '-wal');
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
