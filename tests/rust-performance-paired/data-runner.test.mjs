// Synthetic harness tests only. No Cargo, product CLI, game or Windows claim.
import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {deflateSync} from 'node:zlib';
import {DatabaseSync} from 'node:sqlite';
import {CONTENT_BYTES, GO_COMMIT, PROOF, REPETITIONS, SQL, SQL_ROWS, auditCache, compareData, dataCases, orderFor, semanticData, summarizeData, usage} from './data-contract.mjs';
import {blob, decodeFixtureBLTE, decodedIdentity, digest, goJSON, parse, seedDataHome, validateDataOutput} from './data-inputs.mjs';
import {fixtureArgs, fixturePreparers} from './prepare-data.mjs';

function images() { return {schema: 'lycheedev.large-image-paired-fixture.v1', encodings: ['png', 'webp'], channels: ['rgba'], cases: ['palette', 'bgra8', 'bc1', 'bc2', 'bc3'].map((storage, index) => ({fileDataID: 701 + index, mipmap: 0, pixelsPath: `pixels/${701 + index}.rgba`, pixelsBytes: 1031 * 519 * 4, pixelsSHA256: 'a'.repeat(64), texture: {format: 'BLP2', storage, width: 1031, height: 519, mipmap: 0, levels: 1, alphaBits: 8}}))}; }
function cache(mode = 'cold') { return {hits: mode === 'warm' ? 2 : 0, reusedBytes: mode === 'warm' ? 100 : 0, contentHashCalls: 2, contentHashBytes: 120, contentDecodeCalls: mode === 'warm' ? 0 : 2, extractedBytes: 100, blobReads: 2, blobReadBytes: 100, fragmentLoads: 2, blockCacheHits: 0}; }
function resources() { return {retainedBytes: 100000000, metadataBytes: 10000, decodeWork: 123, networkRequests: 0, networkBytes: 0}; }
function envelope(kind = 'sql', engine = 'go', mode = 'cold') {
  const file = {contentVerified: true, source: 'installation', decodedCache: cache(mode), pin: {fullBuild: 'fixture'}, content: {sha256: 'a'.repeat(64), bytes: 1}, entry: {fileDataId: 403, contentKey: 'b'.repeat(32)}};
  if (engine === 'rust') Object.assign(file, {verification: structuredClone(PROOF), encodingIO: {bytes: 123, reads: 2, wholeEncoded: false}});
  const result = kind === 'sql' ? {complete: true, partial: false, query: {sql: SQL}, result: {rows: structuredClone(SQL_ROWS), columns: ['ID', 'Name_lang', 'ModelFileDataID'], complete: true}, sources: [{file, schema: {name: 'HouseDecor'}}],
    resources: resources(), memoryBudget: {mode: 'windows_adaptive', concurrentQueries: 4, ceilingBytes: 2 ** 30, retainedBytes: 512 << 20, metadataBytes: 128 << 20, peakScratchBytes: 20000, growths: 0},
    timings: {decodedSourceBytes: 200, definitionsMs: 1, preparationMs: 2, executionMs: 3}} : file;
  if (engine === 'rust') result.resourceAccounting = kind === 'sql'
    ? {scope: 'admitted allocation and work across local CASC/Encoding/decoded cache/definitions/WDC/SQL', strategy: 'conservative cumulative admission', wholeQueryAccountingComplete: false, logicalProviderParityComplete: false, used: resources(), remainingGaps: ['physical/effective provider logical counters not audited against Go 3.2', 'SQL plan/order-container transient allocations', 'uninterruptible single codec call']}
    : {wholeQueryAccountingComplete: false, used: resources()};
  return {schema: 'lycheedev.result.v1', ok: true, operationId: '', context: {snapshot: 'PIN-fixture'}, result, captures: [], warnings: [], error: null};
}
const sqlCase = dataCases(images())[0], contentCase = dataCases(images())[1];
function pairs(entry = contentCase, count = 20) { return Array.from({length: count}, (_, iteration) => ['cold', 'warm'].map(cacheMode => ({caseId: entry.id, iteration, cacheMode, resultEquivalent: true, order: orderFor(iteration),
  ...Object.fromEntries(['go', 'rust'].map(engine => [engine, {valid: true, home: `${iteration}/${engine}`, binary: engine, snapshot: 'PIN-input', reuse: {verified: true}, controllableMillis: engine === 'go' ? 100 : 30}]))}))).flat(); }

test('fixed Go323, 17MiB/3-chunk family, five full-size storage modes and no SQL performance claim', () => {
  assert.equal(GO_COMMIT, '3766f9cacb52e93a803a626187e2a1d54a445468'); assert.equal(REPETITIONS, 20); assert.equal(CONTENT_BYTES, 17825792);
  const cases = dataCases(images()); assert.equal(cases.length, 12); assert.equal(cases[0].performance, false); assert(cases.slice(1).every(row => row.performance));
  assert.equal(new Set(cases.map(row => row.id)).size, 12); assert.equal(cases.filter(row => row.encoding === 'webp').length, 5);
});
test('three existing fixture targets have exact serial controlled-Cargo filters', () => {
  assert.deepEqual(fixturePreparers.map(entry => [entry.kind, entry.target, entry.test]), [
    ['catalog', 'catalog_navigation_parity', 'prepare_catalog_public_fixture'],
    ['content', 'content_budget_parity', 'prepare_compressed_content_public_fixture'],
    ['images', 'image_export_business', 'prepare_large_image_export_public_fixture'],
  ]);
  for (const entry of fixturePreparers) assert.deepEqual(fixtureArgs(entry), ['test', '--locked', '--offline', '-j', '1', '-p', 'lycheedev-data', '--test', entry.target, entry.test, '--', '--ignored', '--exact', '--nocapture', '--test-threads=1']);
  const source = readFileSync(new URL('./prepare-data.mjs', import.meta.url), 'utf8');
  assert(source.includes("retention: 'evidence'")); assert(source.includes('runCargo(root, row.args')); assert(source.includes('ownedSpawnSync(binary, args, options)'));
  assert(source.includes('row.cargoReceipt.handlesReaped !== true')); assert(!source.includes('.cleanup(')); assert(!source.includes('Promise.all'));
});
test('small, reordered or incomplete image inputs cannot become the parallel benchmark', () => {
  for (const mutate of [x => x.cases.pop(), x => x.cases.reverse(), x => x.cases[0].texture.width = 4, x => x.cases[0].pixelsBytes--, x => x.channels.push('a'), x => x.cases[0].pixelsPath = '../other']) { const value = images(); mutate(value); assert.throws(() => dataCases(value)); }
});
test('cold and warm require real hash/decode/cache evidence', () => {
  assert.equal(auditCache(cache(), 'cold').verified, false);
  assert(auditCache(cache(), 'cold', {encodedBytes: 120, decodedBytes: 100}).verified);
  const warm = cache('warm'); warm.extractedBytes = 0;
  assert(auditCache(warm, 'warm', {encodedBytes: 120, decodedBytes: 100}).verified);
  for (const mutate of [x => x.hits = 0, x => x.contentDecodeCalls = 1, x => x.contentHashCalls = 0, x => x.contentHashBytes = 0, x => delete x.blobReads, x => x.newCounter = 1]) { const value = cache('warm'); mutate(value); assert.throws(() => auditCache(value, 'warm')); }
  assert.throws(() => auditCache(cache('warm'), 'cold'));
  for (const key of ['contentHashBytes', 'reusedBytes', 'extractedBytes']) {
    const value = {...warm}; value[key] = 1;
    assert.throws(() => auditCache(value, 'warm', {encodedBytes: 120, decodedBytes: 100}));
  }
});
test('separate audited counters retain full stable Go SQL result/schema/warnings and unknown fields', () => {
  const go = semanticData(sqlCase, envelope(), 'go', 'cold'), rust = semanticData(sqlCase, envelope('sql', 'rust'), 'rust', 'cold');
  compareData(go.value, rust.value); assert.deepEqual(rust.observations.accounting.used, resources());
  for (const mutate of [x => x.result.result.rows.reverse(), x => x.result.sources[0].schema.name = 'Other', x => x.warnings.push('partial'), x => x.result.newPublicField = true, x => x.context.snapshot = 'other', x => x.result.timings.decodedSourceBytes++]) {
    const value = envelope('sql', 'rust'); mutate(value); assert.throws(() => compareData(go.value, semanticData(sqlCase, value, 'rust', 'cold').value));
  }
});
test('Rust encoding IO has the actual public Boolean shape and rejects unknown fields', () => {
  const value = envelope('content', 'rust'); assert.deepEqual(semanticData(contentCase, value, 'rust', 'cold').observations.encodingIO, {bytes: 123, reads: 2, wholeEncoded: false});
  for (const io of [{bytesRead: 123}, {bytes: 123, reads: 2, wholeEncoded: true}, {bytes: 123, reads: 2, wholeEncoded: false, extra: 1}, {bytes: -1, reads: 2, wholeEncoded: false}]) {
    const value = envelope('content', 'rust'); value.result.encodingIO = io; assert.throws(() => semanticData(contentCase, value, 'rust', 'cold'));
  }
});
test('adaptive policy and original quotas remain enforced while observed ceiling is reported separately', () => {
  const value = envelope('sql', 'rust'); value.result.memoryBudget.ceilingBytes += 4096; value.result.resources.decodeWork++;
  const normalized = semanticData(sqlCase, value, 'rust', 'cold'); compareData(semanticData(sqlCase, envelope(), 'go', 'cold').value, normalized.value);
  assert.equal(normalized.observations.memoryBudget.ceilingBytes, 2 ** 30 + 4096);
  for (const mutate of [x => x.result.memoryBudget.retainedBytes++, x => x.result.memoryBudget.growths++, x => x.result.memoryBudget.mode = 'fixed', x => x.result.memoryBudget.ceilingBytes = 1000, x => x.result.resources.networkBytes = 1, x => x.result.resourceAccounting.wholeQueryAccountingComplete = true, x => x.result.sources[0].file.verification.encodingWholeDecodedCKey = true]) {
    const value = envelope('sql', 'rust'); mutate(value); assert.throws(() => semanticData(sqlCase, value, 'rust', 'cold'));
  }
  assert.throws(() => usage({...resources(), decodeWork: 100000001}));
});
test('20 pairs enforce .30 separately and one shot/tiny SQL never pass performance', () => {
  assert(summarizeData([contentCase], pairs(), true).every(group => group.state === 'passed_selected_path'));
  const slow = pairs(); slow.filter(row => row.cacheMode === 'warm').forEach(row => row.rust.controllableMillis = 30.01);
  assert.equal(summarizeData([contentCase], slow, true)[1].state, 'failed_performance');
  for (const [entry, acceptance] of [[contentCase, false], [sqlCase, true]]) for (const group of summarizeData([entry], pairs(entry, 1), acceptance)) {
    assert.equal(group.state, 'equivalent_observation_only'); assert.equal(group.performanceAcceptanceEvaluated, false); assert.equal(group.medianRatio, null);
  }
});
test('missing/corrupt/intermediate/nonalternating/reused-home samples cannot pass', () => {
  for (const mutate of [x => x.pop(), x => x[4].resultEquivalent = false, x => x[5].rust.valid = false, x => x[2].iteration = 0, x => x[2].order = ['go', 'rust'], x => x[2].rust.home = x[0].rust.home, x => x[1].rust.snapshot = 'other', x => x[0].go.home = x[0].rust.home, x => x[4].rust.controllableMillis = null]) {
    const value = pairs(); mutate(value); assert(summarizeData([contentCase], value, true).some(group => !['passed_selected_path'].includes(group.state)));
  }
});
test('failed cold also invalidates warm lineage, without replacing samples', () => {
  const value = pairs(); value[0].resultEquivalent = false; assert(summarizeData([contentCase], value, true).every(group => !group.correctAndComplete));
});
function framed(blocks) {
  const header = Buffer.alloc(12 + blocks.length * 24); header.write('BLTE'); header.writeUInt32BE(header.length, 4); header[8] = 15; header.writeUIntBE(blocks.length, 9, 3);
  const payloads = blocks.map((raw, index) => { const encoded = Buffer.concat([Buffer.from('Z'), deflateSync(raw)]), at = 12 + index * 24; header.writeUInt32BE(encoded.length, at); header.writeUInt32BE(raw.length, at + 4); Buffer.from(digest(encoded, 'md5'), 'hex').copy(header, at + 8); return encoded; });
  return Buffer.concat([header, ...payloads]);
}
test('bounded fixture BLTE validates every chunk hash, length, whole frame and raw bytes', () => {
  const blocks = [Buffer.from('abc'), Buffer.from('def'), Buffer.from('ghi')], frame = framed(blocks), decoded = decodeFixtureBLTE(frame);
  assert.deepEqual(decoded.raw, Buffer.concat(blocks)); assert.equal(decoded.chunks, 3); assert.equal(decoded.ekey, digest(frame.subarray(0, 84), 'md5'));
  assert.deepEqual(decodeFixtureBLTE(Buffer.from('424c5445000000004e0102', 'hex')).raw, Buffer.from([1, 2]));
  for (const bad of [frame.subarray(0, frame.length - 1), Buffer.concat([frame, Buffer.from([0])])]) assert.throws(() => decodeFixtureBLTE(bad));
  for (const offset of [12, 16, 20, 84]) { const bad = Buffer.from(frame); bad[offset] ^= 1; assert.throws(() => decodeFixtureBLTE(bad)); }
});
test('unsafe-width numbers and lossy integer parsing are rejected', () => { assert.equal(parse('123'), 123); assert.throws(() => parse('9007199254740993')); });
test('independent seed imports only explicit documents/blobs and proves no decoded cache', t => {
  const root = mkdtempSync(join(tmpdir(), 'paired-data-seed-')); t.after(() => rmSync(root, {recursive: true, force: true}));
  mkdirSync(join(root, 'state')); mkdirSync(join(root, 'blobs')); writeFileSync(join(root, 'workspace.json'), '{}');
  const raw = Buffer.from('fixed-dbd'), ref = {sha256: digest(raw), bytes: raw.length};
  const fixture = {documents: [{key: 'pin/fixed', bytes: Buffer.from('{}')}], blobs: [{...ref, raw}]};
  seedDataHome(fixture, root); assert.deepEqual(decodedIdentity(root), []); assert.deepEqual(blob(root, ref), raw);
  assert.throws(() => seedDataHome(fixture, root), /0/);
});
function materialized(t, kind = 'content') {
  const root = mkdtempSync(join(tmpdir(), 'paired-data-capture-')); t.after(() => rmSync(root, {recursive: true, force: true}));
  mkdirSync(join(root, 'state')); mkdirSync(join(root, 'blobs')); mkdirSync(join(root, 'evidence'));
  const raw = Buffer.from('known-source'), ref = {sha256: digest(raw), bytes: raw.length}; mkdirSync(join(root, 'blobs', ref.sha256.slice(0, 2))); writeFileSync(join(root, 'blobs', ref.sha256.slice(0, 2), ref.sha256.slice(2)), raw);
  const value = envelope(kind, 'go'); value.result.content = ref;
  value.result.root = ref; value.result.payloadEncodingKey = 'c'.repeat(32);
  value.result.decodedCache.contentHashBytes = 2 * (raw.length + 9); value.result.decodedCache.extractedBytes = 2 * raw.length;
  const header = {schema: 'lycheedev.capture.v1', id: '', blob: ref, mediaType: 'application/octet-stream', provenance: {kind: 'asset', locator: `casc:fdid:501:ckey:${value.result.entry.contentKey}`, snapshot: 'PIN-fixture', operationId: '', dataBuild: 'fixture'}, complete: true, truncated: false, capturedAt: '2026-10-17T00:00:00Z'};
  header.id = 'CAP-' + digest(goJSON(header)); value.captures = [header];
  const database = new DatabaseSync(join(root, 'state/toolkit.sqlite')); database.exec('CREATE TABLE documents(key TEXT PRIMARY KEY,value BLOB)'); database.prepare('INSERT INTO documents VALUES(?,?)').run('capture/' + header.id, Buffer.from(JSON.stringify(header))); database.close();
  return {entry: contentCase, fixture: {objects: {[ref.sha256]: {encodedBytes: raw.length + 9, decodedBytes: raw.length, ckey: value.result.entry.contentKey, ekey: value.result.payloadEncodingKey}}, identity: {snapshot: 'PIN-fixture', pin: {fullBuild: 'fixture'}, tables: [{fileDataID: 501, ...ref}]}}, engine: 'go', cacheMode: 'cold', envelope: value, home: root, evidence: join(root, 'evidence'), root};
}
test('full source capture digest/metadata/bytes are independently verified', async t => {
  const sample = materialized(t); const result = await validateDataOutput(sample); assert.equal(result.proofs.length, 1); assert.equal(result.proofs[0].completeBytesVerified, true);
});
test('capture field/byte/provenance corruption fails rather than normalization hiding it', async t => {
  for (const mutate of [x => x.envelope.captures[0].complete = false, x => x.envelope.captures[0].provenance.locator = 'other', x => x.envelope.captures[0].id = 'CAP-' + '0'.repeat(64), x => x.envelope.result.content.sha256 = '0'.repeat(64)]) {
    const sample = materialized(t); mutate(sample); await assert.rejects(validateDataOutput(sample));
  }
});
function materializedImage(t, engine = 'go', stamp = '2026-10-17T00:00:00Z', alterDependencies) {
  const sample = materialized(t), root = sample.root, entry = structuredClone(dataCases(images())[2]);
  const pixels = Buffer.alloc(1031 * 519 * 4), encoded = Buffer.from('independently-decoded-fixture-image');
  entry.image.pixelsSHA256 = digest(pixels); mkdirSync(join(root, 'pixels')); writeFileSync(join(root, entry.image.pixelsPath), pixels);
  const putBlob = raw => { const ref = {sha256: digest(raw), bytes: raw.length}; mkdirSync(join(root, 'blobs', ref.sha256.slice(0, 2)), {recursive: true}); writeFileSync(join(root, 'blobs', ref.sha256.slice(0, 2), ref.sha256.slice(2)), raw); return ref; };
  const capture = (ref, kind, locator, dependencies) => {
    const value = {schema: 'lycheedev.capture.v1', id: '', blob: ref, mediaType: kind === 'asset' ? 'application/octet-stream' : kind === 'asset-image' ? 'image/png' : 'application/json',
      provenance: {kind, locator, snapshot: 'PIN-fixture', operationId: '', dataBuild: 'fixture'}, complete: true, truncated: false, capturedAt: stamp, ...(dependencies === undefined ? {} : {dependencies})};
    value.id = 'CAP-' + digest(goJSON(value)); return value;
  };
  const source = sample.envelope.result; source.entry.fileDataId = 701;
  if (engine === 'rust') Object.assign(source, {verification: structuredClone(PROOF), encodingIO: {bytes: 123, reads: 2, wholeEncoded: false}});
  const original = capture(source.content, 'asset', `casc:fdid:701:ckey:${source.entry.contentKey}`);
  const artifact = capture(putBlob(encoded), 'asset-image', original.id, alterDependencies ? alterDependencies('artifact', [original.id]) : [original.id]);
  const output = join(root, 'image.png'); writeFileSync(output, encoded);
  const value = {schema: 'lycheedev.asset-export.v1', snapshot: 'PIN-fixture', encoding: 'png', output, content: artifact.blob, source, sourceCapture: original,
    image: {texture: entry.image.texture, channels: 'rgba', pixelsSHA256: entry.image.pixelsSHA256}, artifactCapture: artifact};
  const manifest = capture(putBlob(Buffer.from(JSON.stringify(value))), 'asset-export-manifest', original.id, alterDependencies ? alterDependencies('manifest', [original.id, artifact.id]) : [original.id, artifact.id]);
  sample.envelope.result = value; sample.envelope.captures = [original, manifest, artifact];
  const database = new DatabaseSync(join(root, 'state/toolkit.sqlite')); database.exec('DELETE FROM documents');
  for (const item of sample.envelope.captures) database.prepare('INSERT INTO documents VALUES(?,?)').run('capture/' + item.id, Buffer.from(JSON.stringify(item))); database.close();
  sample.fixture.identity.tables[0].fileDataID = 701; sample.fixture.directory = root;
  return {...sample, entry, engine, output, decodeImage: async () => ({info: {width: 1031, height: 519, channels: 4}, data: pixels})};
}
test('real Go323 three-capture dependency shapes hash, verify and normalize across differing IDs', async t => {
  const go = await validateDataOutput(materializedImage(t)), rust = await validateDataOutput(materializedImage(t, 'rust', '2026-10-17T00:00:01Z'));
  compareData(go.value, rust.value);
  assert.deepEqual(go.value.captures[1].dependencies, ['<verified-capture-0>', '<verified-capture-2>']);
  assert.deepEqual(go.value.captures[2].dependencies, ['<verified-capture-0>']);
});
test('missing/reordered/foreign ownership edges fail even with internally consistent capture hashes', async t => {
  for (const change of [(_kind, _edges) => undefined, (kind, edges) => kind === 'manifest' ? edges.reverse() : edges, (_kind, edges) => [...edges, 'CAP-' + 'f'.repeat(64)]]) {
    await assert.rejects(validateDataOutput(materializedImage(t, 'go', '2026-10-17T00:00:00Z', change)));
  }
});
test('image validation checks complete independently decoded RGBA bytes', async t => {
  const sample = materializedImage(t); sample.decodeImage = async () => ({info: {width: 1031, height: 519, channels: 4}, data: Buffer.alloc(1031 * 519 * 4, 1)});
  await assert.rejects(validateDataOutput(sample));
});
test('runner preserves original owned clock and stops failed paths before warm/replacement', () => {
  const source = readFileSync(new URL('./data-run.mjs', import.meta.url), 'utf8');
  assert(source.includes('elapsed(run.proof)')); assert(source.includes('memoryObservation: measured')); assert(source.includes('if (!row.resultEquivalent) break'));
  assert(source.includes('if (!verified || !config.acceptance || !entry.performance) continue')); assert(source.includes('if (!fatal && valid) job.cleanup()'));
  assert(!source.includes('rust-wdc-performance')); assert(!source.includes('target/release/lycheedev')); assert(!source.includes('spawnSync('));
});
