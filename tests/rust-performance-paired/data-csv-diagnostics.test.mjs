// Cloud Node regressions with real files, SQLite CAP/blob/output evidence and
// the actual controller/auditor. Windows processes and disk admission are seams;
// none of these tests claims native CLI execution or a performance result.
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';
import {tmpdir} from 'node:os';
import {DatabaseSync} from 'node:sqlite';
import {CSV_CARGO_LOCK_SHA256, NUMERIC_LAYOUT_SOURCE_SHA256, CSV_BYTES, CSV_COLUMNS, CSV_ROWS, CSV_SHA256, CSV_SQL, DBD_SHA256, WDC_BYTES, WDC_SHA256, auditNumericCSVFixture, removedNumericLayoutCost, captureCanonical, digest, expectedNumericCSV, goJSON, numericRow, parse} from './data-inputs.mjs';
import {PROOF} from './data-contract.mjs';
import {fixtureArgs, fixturePreparers, numericCSVPreparer, selectedFixturePreparers} from './prepare-data.mjs';
import {CSV_BASELINE, CSV_COMMITS, CSV_PAIRS, CSV_OUTER_TIMEOUT_MS, assertCSVOutputPath, auditCSVQuery, compareCSVOutputs, csvEnvironment, csvOrder, loadCSVPreparation, recheckCSVRetained, runCSVController, validateCSVOutput, validateCSVRequest, verifyCSVArtifactRecord, verifyCSVBuildIdentity, verifyCSVObserved} from './data-csv-diagnostics.mjs';

function temporary(t) { const root = mkdtempSync(join(tmpdir(), 'lychee-csv-diagnostic-')); t.after(() => rmSync(root, {recursive: true, force: true})); return root; }
const ref = bytes => ({sha256: digest(bytes), bytes: bytes.length});
function put(home, raw) { const value = ref(raw), path = join(home, 'blobs', value.sha256.slice(0, 2), value.sha256.slice(2)); mkdirSync(dirname(path), {recursive: true}); writeFileSync(path, raw); return value; }
function dbAt(home) { mkdirSync(join(home, 'state'), {recursive: true}); const db = new DatabaseSync(join(home, 'state/toolkit.sqlite')); db.exec('CREATE TABLE IF NOT EXISTS documents(key TEXT PRIMARY KEY NOT NULL,generation INTEGER NOT NULL,value BLOB NOT NULL) WITHOUT ROWID;'); return db; }
function store(db, key, value) { db.prepare('INSERT OR REPLACE INTO documents(key,generation,value) VALUES(?,1,?)').run(key, Buffer.from(JSON.stringify(value))); }
// Test-only independent byte assembly for auditing the fixed WDC layout. This
// is never a CLI fixture generator or replacement for the real Rust exporter.
function wdc() {
  const raw = Buffer.alloc(WDC_BYTES); raw.write('WDC3');
  for (const [at, n] of [[4, 16384], [8, 5], [12, 20], [16, 1], [20, 0xf00d0194], [24, 0xabcd0194], [28, 1], [32, 16384], [44, 5], [56, 120], [68, 1], [80, 252], [84, 16384], [88, 1]]) raw.writeUInt32LE(n, at);
  for (let n = 0; n < 5; n++) { raw.writeUInt16LE(n * 4, 114 + n * 4); raw.writeUInt16LE(n * 32, 132 + n * 24); raw.writeUInt16LE(32, 134 + n * 24); }
  for (let id = 1; id <= 16384; id++) { const at = 252 + (id - 1) * 20; raw.writeUInt32LE(id, at); [id + 0.5, -id - 0.5, 0.25, -0.25].forEach((n, i) => raw.writeFloatLE(n, at + 4 + i * 4)); }
  assert.equal(digest(raw), WDC_SHA256); return raw;
}
function fixture(root) {
  const raw = wdc(), rootRaw = Buffer.from('independent simulated Root object'), rootRef = ref(rootRaw);
  const table = {name: 'NumericSample', fileDataID: 404, layout: 0xabcd0194, bytes: raw.length, sha256: digest(raw), dbdSHA256: DBD_SHA256};
  return {directory: root, identity: {numericCsvABI: {usizeNumberLayoutBytes: 56, stringNumberLayoutBytes: 72}, snapshot: 'PIN-fixture', installation: join(root, 'synthetic-game'), pin: {product: 'retail', region: 'cn', fullBuild: '12.1.0.69933', language: 'zhCN', definitionCommit: 'b'.repeat(40)}, tables: [table]},
    rootRaw, raw, rootRef, documents: [], blobs: [], receipt: {inventory: {sha256: 'f'.repeat(64)}},
    objects: {[table.sha256]: {decodedBytes: raw.length, encodedBytes: raw.length + 9, ckey: digest(raw, 'md5'), ekey: 'a'.repeat(32)}, [rootRef.sha256]: {decodedBytes: rootRaw.length, encodedBytes: rootRaw.length + 9}}};
}
function query(fx, side = 'baseline', cacheMode = 'cold') {
  const table = fx.identity.tables[0], total = table.bytes + fx.rootRaw.length, usage = {retainedBytes: 100000, metadataBytes: 1000, decodeWork: 10000, networkBytes: 0, networkRequests: 0};
  return {complete: true, partial: false, query: {sql: CSV_SQL}, result: {columns: [...CSV_COLUMNS], rows: Array.from({length: CSV_ROWS}, (_, i) => numericRow(i))},
    sources: [{complete: true, truncated: false, schema: {name: 'NumericSample', extraStable: 'must survive'}, file: {source: 'installation', contentVerified: true, pin: fx.identity.pin,
      content: {sha256: table.sha256, bytes: table.bytes}, root: fx.rootRef, entry: {fileDataID: 404, contentKey: fx.objects[table.sha256].ckey}, payloadEncodingKey: 'a'.repeat(32), verification: PROOF,
      decodedCache: {hits: cacheMode === 'warm' ? 2 : 0, reusedBytes: cacheMode === 'warm' ? total : 0, contentHashCalls: 2, contentHashBytes: total + 18, contentDecodeCalls: cacheMode === 'warm' ? 0 : 2,
        extractedBytes: cacheMode === 'cold' ? total : 0, blobReads: 2, blobReadBytes: total, fragmentLoads: 0, blockCacheHits: 0}}}],
    resources: usage, memoryBudget: {mode: 'windows_adaptive', ceilingBytes: (side === 'baseline' ? 1024 : 900) * 1024 * 1024, retainedBytes: 512 << 20, metadataBytes: 128 << 20, concurrentQueries: 1, peakScratchBytes: 1024, growths: 0},
    timings: {definitionsMs: side === 'baseline' ? 5 : 4, preparationMs: 2, executionMs: 30, decodedSourceBytes: table.bytes},
    resourceAccounting: {scope: 'admitted allocation and work across local CASC/Encoding/decoded cache/definitions/WDC/SQL', strategy: 'conservative cumulative admission', wholeQueryAccountingComplete: false, logicalProviderParityComplete: false,
      used: {...usage, retainedBytes: side === 'baseline' ? 10000000 + 53700992 : 10000000, decodeWork: side === 'baseline' ? 81920 + 10000 : 10000}, remainingGaps: ['physical/effective provider logical counters not audited against Go 3.2', 'SQL plan/order-container transient allocations', 'uninterruptible single codec call']}};
}
let serial = 0;
function emit(fx, home, output, side, cacheMode, mutate) {
  const db = dbAt(home), body = query(fx, side, cacheMode); if (mutate) mutate(body);
  function capture(raw, provenance, mediaType, dependencies) {
    const cap = {schema: 'lycheedev.capture.v1', id: '', blob: put(home, raw), mediaType, provenance: {...provenance, operationId: ''}, complete: true, truncated: false,
      capturedAt: new Date(Date.UTC(2026, 9, 18, 0, 0, serial++)).toISOString(), ...(dependencies ? {dependencies} : {})};
    cap.id = 'CAP-' + digest(goJSON(captureCanonical(cap))); store(db, 'capture/' + cap.id, cap); return cap;
  }
  put(home, fx.raw); put(home, fx.rootRaw);
  store(db, 'decoded-v1/root', {blob: fx.rootRef}); store(db, 'decoded-v1/table', {blob: ref(fx.raw)});
  const one = capture(Buffer.from(JSON.stringify(body)), {kind: 'data-query', locator: 'sql:sha256:' + digest(goJSON({sql: CSV_SQL})), snapshot: fx.identity.snapshot, dataBuild: fx.identity.pin.fullBuild}, 'application/json');
  const csv = expectedNumericCSV(), two = capture(csv, {kind: 'data-query-csv', locator: one.id, snapshot: fx.identity.snapshot}, 'text/csv; charset=utf-8', [one.id]); db.close();
  const result = {schema: 'lycheedev.query-csv.v1', snapshot: fx.identity.snapshot, output, ...ref(csv), columns: [...CSV_COLUMNS], rows: CSV_ROWS, queryCapture: one.id, csvCapture: two.id, complete: true};
  writeFileSync(output, csv); writeFileSync(output + '.manifest.json', JSON.stringify(result));
  return {schema: 'lycheedev.result.v1', ok: true, error: null, warnings: [], context: {snapshot: fx.identity.snapshot}, result, captures: [one, two]};
}
function auditPair(t, change) {
  const root = temporary(t), fx = fixture(root), values = {}, records = {};
  for (const side of ['baseline', 'candidate']) {
    const home = join(root, side + '-home'), output = join(root, side + '.csv'), evidence = join(root, side + '-evidence'); mkdirSync(evidence);
    const envelope = emit(fx, home, output, side, 'cold', side === 'candidate' ? change : undefined);
    records[side] = {fixture: fx, envelope, home, output, evidence, label: side, cacheMode: 'cold'};
    values[side] = validateCSVOutput(records[side]);
  }
  return {root, fx, values, records};
}
function proof(ms = 1000) { return {schema: 'lycheedev.owned-process.v1', rootReaped: true, treeReaped: true, handlesClosed: true, toolIdentityLocksClosed: true, outputReadersComplete: true, activeProcesses: 0, forced: false, deadlineHit: false,
  timing: {schema: 'lycheedev.owned-timing.v1', frequency: 10000, startTicks: '1000', rootExitTicks: String(1000 + ms * 10 - 1), treeReapedTicks: String(1000 + ms * 10)}}; }
function request(root, pairs) {
  const input = {schema: 'lycheedev.data-csv-diagnostics-config.v1', ownerRoot: root, fixturePreparation: {report: join(root, 'fixture-report.json'), reportSHA256: 'f'.repeat(64), sourceTreeSHA256: 'e'.repeat(64)}, ...(pairs === undefined ? {} : {pairs})};
  for (const side of ['baseline', 'candidate']) { const dir = join(root, side); mkdirSync(dir); input[side] = {sourceRoot: dir, artifactRoot: dir, artifact: join(dir, 'artifact.json'), commit: CSV_COMMITS[side], artifactSHA256: 'a'.repeat(64), binarySHA256: side === 'baseline' ? CSV_BASELINE.binarySHA256 : 'b'.repeat(64), sourceTreeSHA256: side === 'baseline' ? CSV_BASELINE.sourceTreeSHA256 : 'c'.repeat(64)}; }
  return input;
}
function controller(t, options = {}) {
  const root = temporary(t), req = validateCSVRequest(request(root, options.pairs)), scratch = join(root, 'outer-scratch'), evidence = join(root, 'outer-evidence'); mkdirSync(scratch); mkdirSync(evidence);
  const fx = fixture(root), events = [], jobs = []; let identityChecks = 0, loads = 0;
  const config = {request: req, runnerRoot: root, inputs: {runner: 'fixed'}, scratch, evidence, conditions: {machine: 'synthetic'}};
  const services = {
    assertCSVPolicy() {},
    csvRunnerIdentity: () => ({runner: 'fixed'}),
    verifyCSVProduct(input) { identityChecks++; if (options.identityFailure && identityChecks >= options.identityFailure) throw Error('identity changed'); return {...input, artifact: {sourceBinary: input.sourceRoot.endsWith('baseline') ? 'baseline.exe' : 'candidate.exe'}}; },
    loadCSVPreparation() { loads++; if (options.fixtureFailure && loads >= options.fixtureFailure) throw Error('fixture changed'); return {fixture: fx, provenance: {fixture: 'immutable-third-producer'}}; },
    collectOwnedMemory() { return {state: 'not_measured', gaps: ['simulated OS boundary']}; },
    beginScratch(_root, _name, args) {
      const directory = join(root, `pair-${jobs.length}`), ev = join(root, `evidence-${jobs.length}`); mkdirSync(directory); mkdirSync(ev);
      const job = {directory, evidence: ev, receipt: join(ev, 'receipt.json'), args, sealed: null, cleanupCalled: false,
        seal(value) { job.sealed = value; writeFileSync(job.receipt, JSON.stringify(value)); }, cleanup() { job.cleanupCalled = true; if (options.cleanupFailure) throw Object.assign(Error('owned cleanup blocked'), {code: 'EPERM'}); rmSync(directory, {recursive: true}); }};
      jobs.push(job); return job;
    },
    ownedSpawnSync(binary, args, invocation) {
      events.push({binary, args, invocation}); const get = flag => args[args.indexOf(flag) + 1]; let value;
      const run = {status: 0, handlesReaped: true, stdout: Buffer.alloc(0), stderr: Buffer.alloc(0), proof: proof(binary === 'baseline.exe' ? 1000 : 900), logs: []};
      if (binary === 'powercfg.exe') value = 'same-power';
      else if (args[0] === 'init') { const home = get('--home'); mkdirSync(join(home, 'state'), {recursive: true}); writeFileSync(join(home, 'workspace.json'), '{}'); value = {schema: 'lycheedev.result.v1', ok: true, error: null}; }
      else if (args[0] === 'data') {
        const side = binary === 'baseline.exe' ? 'baseline' : 'candidate', output = get('--output'), mode = output.includes('-warm') ? 'warm' : 'cold';
        value = emit(fx, get('--home'), output, side, mode, side === 'candidate' ? options.changeQuery : undefined);
        if (options.changeCSV && side === 'candidate') writeFileSync(output, 'wrong');
        if (options.changeSidecar && side === 'candidate') writeFileSync(output + '.manifest.json', '{}');
        if (options.failProcess && side === 'candidate') { run.status = 7; run.proof.deadlineHit = true; }
        if (options.unreaped && side === 'baseline') { run.handlesReaped = false; run.proof.treeReaped = false; }
      } else if (args[0] === 'evidence') {
        const db = dbAt(get('--home')), raw = db.prepare('SELECT value FROM documents WHERE key=?').get('capture/' + args[2]); db.close();
        value = {schema: 'lycheedev.result.v1', ok: true, error: null, result: parse(Buffer.from(raw.value)), captures: []};
        if (options.failVerify) value.result = {...value.result, complete: false};
        if (options.lateCSV && binary === 'candidate.exe') { const data = [...events].reverse().find(e => e.binary === binary && e.args[0] === 'data'); writeFileSync(data.args[data.args.indexOf('--output') + 1], 'late change'); }
      } else throw Error('unexpected fixture command');
      run.stdout = Buffer.from(typeof value === 'string' ? value : JSON.stringify(value)); return run;
    },
  };
  const report = runCSVController(config, services); return {root, report, events, jobs, config};
}

test('exact bounded numeric input and independent CSV oracle retain all 16384 rows', () => {
  assert.equal(CSV_ROWS, 16384); assert.deepEqual(CSV_COLUMNS, ['ID', 'F0', 'F1', 'F2', 'F3']); assert.equal(CSV_BYTES, 523753);
  const bytes = expectedNumericCSV(); assert.equal(digest(bytes), CSV_SHA256); assert.equal(bytes.toString().split('\n').length, 16386);
  assert(bytes.toString().startsWith('ID,F0,F1,F2,F3\n1,1.5,-1.5,0.25,-0.25\n')); assert(bytes.toString().endsWith('16384,16384.5,-16384.5,0.25,-0.25\n'));
  const fx = fixture('/unused'), definition = Buffer.from('COLUMNS\nint ID\nfloat F0\nfloat F1\nfloat F2\nfloat F3\n\nBUILD 12.1.0.69933\nLAYOUT ABCD0194\n$id$ID<u32>\nF0<32>\nF1<32>\nF2<32>\nF3<32>\n'); assert.equal(digest(definition), DBD_SHA256);
  auditNumericCSVFixture(fx.identity, new Map([[WDC_SHA256, {raw: fx.raw}]]), [{sha256: DBD_SHA256, raw: definition}]);
  fx.raw[256] ^= 1; assert.throws(() => auditNumericCSVFixture(fx.identity, new Map([[WDC_SHA256, {raw: fx.raw}]]), [{sha256: DBD_SHA256, raw: definition}]));
});
test('only authenticated bounded x64 ABI facts determine the exact removed admission', () => {
  const abi = {usizeNumberLayoutBytes: 56, stringNumberLayoutBytes: 72};
  assert.deepEqual(removedNumericLayoutCost(abi), {retainedBytes: 53700992, decodeWork: 81920, abi});
  for (const invalid of [{}, {...abi, extra: 1}, {...abi, usizeNumberLayoutBytes: 57}, {...abi, stringNumberLayoutBytes: 80}, {usizeNumberLayoutBytes: 512, stringNumberLayoutBytes: 528}]) assert.throws(() => removedNumericLayoutCost(invalid));
});
test('actual lower adaptive memory is audited without adding a 512MiB eligibility gate', () => {
  const fx = fixture('/unused'), body = query(fx); body.memoryBudget.ceilingBytes = 256 << 20; body.memoryBudget.retainedBytes = 256 << 20;
  assert.equal(auditCSVQuery(body, fx, 'cold').availableMemory, 256 << 20);
});
test('existing three preparers remain default; numeric exporter is explicit and serial', () => {
  assert.equal(selectedFixturePreparers([]), fixturePreparers); assert.equal(fixturePreparers.length, 3);
  assert.deepEqual(selectedFixturePreparers(['--csv-numeric']), [numericCSVPreparer]);
  assert.deepEqual(fixtureArgs(numericCSVPreparer), ['test', '--locked', '--offline', '-j', '1', '-p', 'lycheedev-data', '--test', 'catalog_navigation_parity', 'prepare_numeric_csv_public_fixture', '--', '--ignored', '--exact', '--nocapture', '--test-threads=1']);
  for (const args of [['--acceptance'], ['--csv-numeric', '--csv-numeric'], ['--csv-numeric', '--rows', '999']]) assert.throws(() => selectedFixturePreparers(args));
});
test('fixed product identities are separate from fixture producer and only 1/3 diagnostic pairs are permitted', t => {
  const root = temporary(t), req = request(root); assert.equal(CSV_PAIRS, 1); assert.equal(CSV_OUTER_TIMEOUT_MS, 3600000); assert.equal(validateCSVRequest(req).pairs, 1);
  assert.equal(validateCSVRequest({...req, pairs: 3}).pairs, 3);
  for (const pairs of [0, 2, 20, 100, -1, '3']) assert.throws(() => validateCSVRequest({...req, pairs}));
  for (const change of [x => x.baseline.binarySHA256 = 'f'.repeat(64), x => x.baseline.commit = CSV_COMMITS.candidate, x => x.candidate.commit = 'f'.repeat(40), x => x.candidate.sourceRoot = x.baseline.sourceRoot, x => x.candidate.artifact = join(root, 'outside.json'), x => x.acceptance = true, x => x.fixturePreparation.sourceTreeSHA256 = 'no']) { const bad = structuredClone(req); change(bad); assert.throws(() => validateCSVRequest(bad)); }
  const artifact = {profile: 'release', sameInvocation: true, sourceRoot: root, bytes: 4096, command: ['cargo', 'build', '--locked', '--offline', '-j', '1', '--profile', 'release', '-p', 'lycheedev-cli'], binarySHA256: req.candidate.binarySHA256, inputHashes: {sourceTree: req.candidate.sourceTreeSHA256}};
  verifyCSVArtifactRecord(req.candidate, artifact);
  for (const change of [x => x.profile = 'dev', x => x.sameInvocation = false, x => x.binarySHA256 = 'f'.repeat(64), x => x.inputHashes.sourceTree = req.fixturePreparation.sourceTreeSHA256, x => x.profile = 'symbols', x => x.command.push('--features=unexpected'), x => x.bytes = -1]) { const bad = structuredClone(artifact); change(bad); assert.throws(() => verifyCSVArtifactRecord(req.candidate, bad)); }
});
test('optimized build recipe, full Emmy custody and product source digest stay mandatory', t => {
  const root = temporary(t), req = request(root).candidate;
  const emmy = {schema: 'emmy-fixture', identity: 'public-upstream-unmodified-v1', identitySHA256: '1'.repeat(64), recipeSHA256: '2'.repeat(64), patchedTreeSHA256: '3'.repeat(64)};
  const artifact = {profile: 'release', sameInvocation: true, sourceRoot: root, bytes: 4096, command: ['cargo', 'build', '--locked', '--offline', '-j', '1', '--profile', 'release', '-p', 'lycheedev-cli'], binarySHA256: req.binarySHA256,
    inputHashes: {sourceTree: req.sourceTreeSHA256, emmy: {...emmy, historicalPrivatePatchRecovered: false}}};
  const channel = {version: '3.5.0', distTag: 'beta'};
  verifyCSVBuildIdentity(req, artifact, req.sourceTreeSHA256, emmy, channel);
  for (const field of ['identitySHA256', 'recipeSHA256', 'patchedTreeSHA256', 'schema']) {
    const bad = structuredClone(artifact); bad.inputHashes.emmy[field] = field === 'schema' ? 'wrong' : 'f'.repeat(64);
    assert.throws(() => verifyCSVBuildIdentity(req, bad, req.sourceTreeSHA256, emmy, channel));
  }
  for (const change of [x => x.command[4] = '2', x => x.inputHashes.emmy.historicalPrivatePatchRecovered = true]) {
    const bad = structuredClone(artifact); change(bad); assert.throws(() => verifyCSVBuildIdentity(req, bad, req.sourceTreeSHA256, emmy, channel));
  }
  assert.throws(() => verifyCSVBuildIdentity(req, artifact, 'e'.repeat(64), emmy, channel));
  assert.throws(() => verifyCSVBuildIdentity(req, artifact, req.sourceTreeSHA256, {...emmy, identity: 'patched'}, channel));
  assert.throws(() => verifyCSVBuildIdentity(req, artifact, req.sourceTreeSHA256, emmy, {...channel, distTag: 'latest'}));
});
test('unsafe, preexisting, managed-home and symlink output paths are rejected before launch', t => {
  const root = temporary(t), home = join(root, 'home'); mkdirSync(home); assertCSVOutputPath(root, {home}, join(root, 'good.csv'));
  for (const path of [join(home, 'bad.csv'), join(root, '..', 'outside.csv'), join(root, 'nested', 'bad.csv')]) assert.throws(() => assertCSVOutputPath(root, {home}, path));
  writeFileSync(join(root, 'existing.csv'), 'foreign'); assert.throws(() => assertCSVOutputPath(root, {home}, join(root, 'existing.csv')));
  writeFileSync(join(root, 'sidecar.csv.manifest.json'), 'foreign'); assert.throws(() => assertCSVOutputPath(root, {home}, join(root, 'sidecar.csv')));
});
test('real file symlink output is rejected when creation is permitted', t => {
  const root = temporary(t), home = join(root, 'home'); mkdirSync(home); writeFileSync(join(root, 'existing.csv'), 'foreign');
  try { symlinkSync(join(root, 'existing.csv'), join(root, 'alias.csv')); }
  catch (error) { if (process.platform === 'win32' && ['EPERM', 'EACCES', 'ENOTSUP'].includes(error.code)) { t.skip(`actual file symlink creation ${error.code}`); return; } throw error; }
  assert.throws(() => assertCSVOutputPath(root, {home}, join(root, 'alias.csv'), false));
});
test('phase profiling never leaks in, and compiler/stack/target overrides remain forbidden', () => {
  assert.deepEqual(csvEnvironment({KEEP: 'x', lycheedev_data_phase_profile: '1', LYCHEEDEV_SOURCE_PHASE_PROFILE: '1'}), {KEEP: 'x'});
  for (const key of ['RUST_MIN_STACK', 'rustflags', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_DIR', 'LYCHEEDEV_DISK_SCRATCH_BYTES']) assert.throws(() => csvEnvironment({[key]: 'override'}));
});
test('both actual CAP identities, metadata, query bodies and CSV bytes are checked without ID equality', t => {
  const {values, records} = auditPair(t); assert.notEqual(records.baseline.envelope.captures[0].id, records.candidate.envelope.captures[0].id); assert.notEqual(records.baseline.envelope.captures[1].id, records.candidate.envelope.captures[1].id);
  const result = compareCSVOutputs(values.baseline, values.candidate); assert.equal(result.equivalent, true); assert.equal(result.differences.safetyUsed.decodeWork.delta, -81920);
  assert(result.differences.queryTimings.qualification.includes('cannot measure'));
});
test('unknown query and schema fields are preserved rather than projected away', t => {
  for (const change of [x => x.arbitrarySemanticField = 'changed', x => x.sources[0].schema.extraStable = 'changed', x => x.result.newResultField = true, x => x.sources[0].file.newSourceField = true]) {
    const {values} = auditPair(t, change); assert.throws(() => compareCSVOutputs(values.baseline, values.candidate), /semantics differ/);
  }
});
test('counter exceptions are individual; unexplained work, logical usage and arbitrary accounting changes fail', t => {
  for (const change of [x => x.resourceAccounting.used.decodeWork++, x => x.resourceAccounting.used.retainedBytes++, x => x.resourceAccounting.used.retainedBytes--, x => x.resourceAccounting.used.metadataBytes++, x => x.resources.retainedBytes++, x => x.resourceAccounting.remainingGaps = [], x => x.resourceAccounting.unknown = true]) {
    assert.throws(() => { const {values} = auditPair(t, change); compareCSVOutputs(values.baseline, values.candidate); });
  }
});
test('changed CAP or CSV bytes, rows, manifest, sidecar and dependencies cannot pass', t => {
  const mutations = [r => r.envelope.captures[0].id = r.envelope.captures[1].id, r => r.envelope.captures[1].dependencies = [], r => r.envelope.captures[0].complete = false,
    r => writeFileSync(r.output, 'changed'), r => writeFileSync(r.output + '.manifest.json', '{}'), r => r.envelope.result.rows--,
    r => { const cap = r.envelope.captures[0]; writeFileSync(join(r.home, 'blobs', cap.blob.sha256.slice(0, 2), cap.blob.sha256.slice(2)), '{}'); },
    r => { const db = dbAt(r.home); store(db, 'capture/' + r.envelope.captures[0].id, {}); db.close(); }];
  for (const [index, mutate] of mutations.entries()) { const {records} = auditPair(t), record = records.candidate; mutate(record); assert.throws(() => validateCSVOutput({...record, label: `again-${index}`})); }
  assert.throws(() => auditPair(t, value => value.result.rows[16383][1]++), /row 16383/);
});
test('one default pair executes four adjacent native samples and eight actual CAP verification calls before cleanup', t => {
  const {report, events, jobs} = controller(t); assert.equal(report.state, 'csv_candidates_observed');
  assert.equal(report.pairs.length, 2); assert.equal(jobs.length, 1); assert(jobs[0].cleanupCalled); assert(!existsSync(jobs[0].directory));
  const measured = events.filter(row => row.invocation.memoryObservation); assert.equal(measured.length, 4); assert(measured.every(row => row.invocation.timeout === 330000 && row.invocation.orphanMs === 30000));
  assert.equal(events.filter(row => row.args[0] === 'evidence').length, 8);
  for (const mode of ['cold', 'warm']) { const first = events.findIndex(row => row.args[0] === 'data' && row.args.includes(join(jobs[0].directory, `baseline-${mode}.csv`))); assert.equal(events[first + 1].args[0], 'data'); }
  verifyCSVObserved({status: 0, handlesReaped: true, proof: proof()}, report); assert.equal(report.performanceAcceptanceEvaluated, false); assert.equal(report.fullB04Acceptance, false);
});
test('explicit three-pair mode alternates engine order and uses six fresh independent homes', t => {
  const {report, events} = controller(t, {pairs: 3}); assert.equal(report.state, 'csv_candidates_observed'); assert.equal(events.filter(row => row.args[0] === 'data').length, 12);
  for (const row of report.pairs) assert.deepEqual(row.order, csvOrder(row.iteration)); verifyCSVObserved({status: 0, handlesReaped: true, proof: proof()}, report);
});
for (const [name, options] of [['CSV bytes', {changeCSV: true}], ['sidecar', {changeSidecar: true}], ['arbitrary semantic field', {changeQuery: query => query.ignoredArbitrarySemanticField = 1}], ['CAP evidence verify', {failVerify: true}], ['late CSV change', {lateCSV: true}], ['deadline/cancellation', {failProcess: true}], ['final identity', {identityFailure: 5}], ['final fixture', {fixtureFailure: 3}]]) {
  test(`controller retains owned inputs on ${name} failure and cannot produce accepted diagnostics`, t => {
    const {report, jobs} = controller(t, options); assert.equal(report.state, 'csv_candidates_failed'); assert(jobs.length === 1 && !jobs[0].cleanupCalled); assert(existsSync(jobs[0].directory));
    assert.throws(() => verifyCSVObserved({status: 0, handlesReaped: true, proof: proof()}, report));
  });
}
test('unreaped process blocks the second engine, retains scratch and seals interrupted', t => {
  const {report, jobs, events} = controller(t, {unreaped: true}); assert.equal(report.requiresOuterRetirement, true); assert.equal(report.state, 'csv_candidates_failed');
  assert.equal(events.filter(row => row.args[0] === 'data').length, 1); assert.equal(jobs[0].sealed.state, 'interrupted'); assert.equal(jobs[0].sealed.handlesReaped, false); assert(!jobs[0].cleanupCalled);
});
test('cleanup failure retains equivalent product facts but prevents successful run completion', t => {
  const {report, jobs} = controller(t, {cleanupFailure: true}); assert.equal(report.state, 'csv_candidates_failed'); assert(report.pairs.every(row => row.resultEquivalent && row.pairTerminal === 'verified'));
  assert.equal(report.pairOwners[0].cleanup.state, 'failed_retained'); assert.equal(jobs[0].sealed.state, 'succeeded'); assert(existsSync(jobs[0].directory)); assert.equal(report.finalIntegrityVerified, false);
});
test('outer verifier rejects timing, missing verify, duplicate home, acceptance and cleanup forgeries', t => {
  const {report} = controller(t); for (const change of [x => x.performanceAcceptanceEvaluated = true, x => x.fullB04Acceptance = true, x => x.pairOwners[0].cleanup.state = 'failed', x => x.pairs[1].candidate.home = 'other', x => x.pairs[0].candidate.controllableMillis++, x => x.pairs[0].candidate.evidenceVerify.pop(), x => x.finalIntegrityVerified = false]) {
    const bad = structuredClone(report); change(bad); assert.throws(() => verifyCSVObserved({status: 0, handlesReaped: true, proof: proof()}, bad));
  }
});
test('fixture producer mismatch and modified sealed report are rejected independently of product artifacts', t => {
  const root = temporary(t), path = join(root, 'fixture-report.json');
  const report = {schema: 'lycheedev.paired-data-preparation.v1', state: 'prepared_synthetic_inputs', handlesReaped: true, inputs: {sourceTree: 'a'.repeat(64)}};
  const raw = Buffer.from(JSON.stringify(report)); writeFileSync(path, raw);
  assert.throws(() => loadCSVPreparation({report: path, reportSHA256: digest(raw), sourceTreeSHA256: 'b'.repeat(64)}), /producer source identity/);
  assert.throws(() => loadCSVPreparation({report: path, reportSHA256: 'c'.repeat(64), sourceTreeSHA256: 'a'.repeat(64)}), /preparation identity/);
});

function producerFixture(t) {
  const root = temporary(t), evidence = join(root, 'evidence'), cargoDirectory = join(root, 'cargo-evidence'), directory = join(root, 'fixture-owner');
  for (const path of [evidence, cargoDirectory, directory]) mkdirSync(path);
  const destination = join(directory, 'csv-numeric'); mkdirSync(destination);
  const reportPath = join(evidence, 'fixture-preparation.json'), owner = join(evidence, 'receipt.json'), cargoPath = join(cargoDirectory, 'receipt.json');
  const producer = 'd'.repeat(64), inputs = {sourceTree: producer, numericLayout: {sourceSHA256: NUMERIC_LAYOUT_SOURCE_SHA256, cargoLockSHA256: CSV_CARGO_LOCK_SHA256}};
  const log = join(cargoDirectory, 'command.json'); writeFileSync(log, 'original controlled Cargo log');
  const cargo = {schema: 'lycheedev.disk-owner.v1', owner: 'paired-data-fixture-csv-numeric', state: 'retired', activePid: null, handlesReaped: true,
    command: ['cargo', ...fixtureArgs(numericCSVPreparer)], inputHashes: {sourceTree: producer}, sealedLogs: [{path: log, sha256: digest(readFileSync(log))}]};
  const fx = fixture(destination), report = {schema: 'lycheedev.paired-data-preparation.v1', state: 'prepared_synthetic_inputs', handlesReaped: true, owner, directory, inputs,
    fixtures: {'csv-numeric': destination}, fixtureIdentity: {'csv-numeric': fx.receipt}, calls: [{kind: 'csv-numeric', args: fixtureArgs(numericCSVPreparer), launched: true,
      process: {exit: 0, handlesReaped: true, proof: proof()}, invocation: {inputHashes: {sourceTree: producer}}, buildEvidence: cargoPath, cargoReceipt: cargo, destination}]};
  const receipt = {schema: 'lycheedev.disk-owner.v1', owner: 'paired-data-fixtures', state: 'succeeded', activePid: null, handlesReaped: true, retention: 'evidence', directory, inputHashes: inputs};
  let input;
  function seal() {
    writeFileSync(cargoPath, JSON.stringify(cargo)); report.calls[0].cargoReceiptSHA256 = digest(readFileSync(cargoPath));
    writeFileSync(reportPath, JSON.stringify(report)); input = {report: reportPath, reportSHA256: digest(readFileSync(reportPath)), sourceTreeSHA256: producer};
    receipt.sealedLogs = [{path: reportPath, sha256: input.reportSHA256}]; writeFileSync(owner, JSON.stringify(receipt)); return input;
  }
  return {report, receipt, cargo, fx, log, seal, verify: () => loadCSVPreparation(input, (actual, kind) => { assert.equal(actual, destination); assert.equal(kind, 'csv-numeric'); return fx; })};
}
test('separate fixture producer verifies original owner, Cargo identity, sealed logs and inventory', t => {
  const value = producerFixture(t); value.seal(); const result = value.verify(); assert.equal(result.provenance.sourceTreeSHA256, 'd'.repeat(64)); assert.equal(result.fixture, value.fx);
});
test('producer owner/Cargo/source/ABI/scope failures cannot be hidden by resaving report bytes', t => {
  for (const change of [x => x.receipt.owner = 'foreign', x => x.receipt.retention = 'rebuildable', x => x.receipt.directory += '-foreign', x => x.receipt.handlesReaped = false,
    x => x.cargo.owner = 'foreign', x => x.cargo.inputHashes.sourceTree = 'e'.repeat(64), x => x.cargo.command.push('another-test'), x => x.cargo.handlesReaped = false,
    x => x.report.inputs.numericLayout.sourceSHA256 = 'f'.repeat(64), x => x.report.calls[0].args.push('another-test'), x => x.report.calls[0].process.proof.deadlineHit = true,
    x => x.report.fixtureIdentity['csv-numeric'] = {inventory: {sha256: 'e'.repeat(64)}}]) {
    const value = producerFixture(t); change(value); value.seal(); assert.throws(value.verify);
  }
  const value = producerFixture(t); value.seal(); writeFileSync(value.log, 'changed after seal'); assert.throws(value.verify, /sealed fixture evidence changed/);
});
