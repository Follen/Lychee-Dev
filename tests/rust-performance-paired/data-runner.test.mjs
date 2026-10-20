// Synthetic harness tests only. No Cargo, product CLI, game or Windows claim.
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';
import {spawnSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {tmpdir} from 'node:os';
import {deflateSync} from 'node:zlib';
import {DatabaseSync} from 'node:sqlite';
import {CONTENT_BYTES, GO_COMMIT, PROOF, REPETITIONS, SQL, SQL_ROWS, auditCache, compareData, dataCases, orderFor, semanticData, summarizeData, usage} from './data-contract.mjs';
import {blob, decodeDataImage, decodeFixtureBLTE, decodedIdentity, digest, goJSON, parse, seedDataHome, validateDataOutput} from './data-inputs.mjs';
import {fixtureArgs, fixturePreparers} from './prepare-data.mjs';
import {DATA_CAPTURE_TIMING, DATA_DETAIL_NAMES, DATA_PUBLISHER_NAMES, DATA_PUBLISHER_NAMES_V3, DATA_PHASE_NAMES, DATA_PHASE_TIMING, dataPhaseEnvironment, dataPhaseProfile} from './data-phase-profile.mjs';

function phaseProfile() {
  return {schema: 'lycheedev.data-query-phases.v1', elapsedMicros: 1000, timing: DATA_PHASE_TIMING, captureTiming: DATA_CAPTURE_TIMING,
    phases: DATA_PHASE_NAMES.map(phase => ({phase, calls: 1, elapsedMicros: 500}))};
}
test('Data phase contract matches the actual recorder in the source checkout', {skip: !existsSync(new URL('../../crates/lycheedev-data/src/phase_profile.rs', import.meta.url))}, () => {
  const source = readFileSync(new URL('../../crates/lycheedev-data/src/phase_profile.rs', import.meta.url), 'utf8');
  assert.deepEqual([...source.match(/const NAMES:.*?= \[([\s\S]*?)\];/)[1].matchAll(/"([^"]+)"/g)].map(match => match[1]), DATA_PHASE_NAMES);
  assert.deepEqual([...source.match(/const DETAIL_NAMES:.*?= \[([\s\S]*?)\];/)[1].matchAll(/"([^"]+)"/g)].map(match => match[1]), DATA_DETAIL_NAMES);
  assert.deepEqual([...source.match(/const PUBLISHER_NAMES:.*?= \[([\s\S]*?)\];/)[1].matchAll(/"([^"]+)"/g)].map(match => match[1]), DATA_PUBLISHER_NAMES);
  assert(source.includes(DATA_PHASE_TIMING)); assert(source.includes(DATA_CAPTURE_TIMING)); assert(source.includes('lycheedev.data-query-phases.v5'));
});
test('Data v3 preserves v1 and v2 bytes and validates fixed nested publisher counters', () => {
  assert.deepEqual(DATA_PUBLISHER_NAMES_V3, [
    'source_to_spool_inclusive', 'spool_to_shard_inclusive', 'final_verify_inclusive',
    'stage_create', 'stage_seal', 'digest_lock', 'shard_open', 'rename', 'winner_open', 'checked_close',
  ]);
  const v1 = phaseProfile();
  const v2 = {...v1, schema: 'lycheedev.data-query-phases.v2', details: DATA_DETAIL_NAMES.map(phase => ({phase, calls: 1, elapsedMicros: 700}))};
  const v3 = {...v2, schema: 'lycheedev.data-query-phases.v3', publisher: DATA_PUBLISHER_NAMES_V3.map(phase => ({phase, calls: 2, elapsedMicros: 900}))};
  for (const value of [v1, v2, v3]) {
    const raw = Buffer.from(JSON.stringify(value) + '\n');
    assert.deepEqual(dataPhaseProfile(raw), value);
    assert.equal(JSON.stringify(dataPhaseProfile(raw)) + '\n', raw.toString());
  }
  assert.deepEqual(v3.phases, v1.phases); assert.deepEqual(v3.details, v2.details);
  assert(v3.publisher.reduce((n, p) => n + p.elapsedMicros, 0) > v3.elapsedMicros, 'nested diagnostics are never treated as additive');
  for (const change of [x => delete x.publisher, x => x.publisher.reverse(), x => x.publisher.pop(), x => x.publisher.push(x.publisher[0]),
    x => x.publisher[0].phase = 'unknown', x => x.publisher[0].extra = 0, x => x.publisher[0].calls = -1,
    x => x.publisher[0].calls = 0, x => x.publisher[0].elapsedMicros = 0.5, x => x.schema = v2.schema,
    x => delete x.details, x => x.schema = v1.schema]) {
    const invalid = structuredClone(v3); change(invalid);
    assert.throws(() => dataPhaseProfile(Buffer.from(JSON.stringify(invalid))));
  }
  const empty = structuredClone(v3);
  empty.publisher = DATA_PUBLISHER_NAMES_V3.map(phase => ({phase, calls: 0, elapsedMicros: 0}));
  assert.deepEqual(dataPhaseProfile(Buffer.from(JSON.stringify(empty))), empty);
});
test('Data v4 keeps private read readiness and final seal separate without rewriting historical bytes', () => {
  assert.deepEqual(DATA_PUBLISHER_NAMES, [
    'source_to_spool_inclusive', 'spool_to_shard_inclusive', 'final_verify_inclusive',
    'stage_create', 'private_spool_read_ready', 'final_stage_seal', 'digest_lock',
    'shard_open', 'rename', 'winner_open', 'checked_close',
  ]);
  const v1 = phaseProfile();
  const v2 = {...v1, schema: 'lycheedev.data-query-phases.v2', details: DATA_DETAIL_NAMES.map(phase => ({phase, calls: 1, elapsedMicros: 700}))};
  const v3 = {...v2, schema: 'lycheedev.data-query-phases.v3', publisher: DATA_PUBLISHER_NAMES_V3.map(phase => ({phase, calls: 2, elapsedMicros: 900}))};
  const v4 = {...v2, schema: 'lycheedev.data-query-phases.v4', publisher: DATA_PUBLISHER_NAMES.map(phase => ({phase, calls: 0, elapsedMicros: 0}))};
  Object.assign(v4.publisher[4], {calls: 2, elapsedMicros: 8});
  Object.assign(v4.publisher[5], {calls: 1, elapsedMicros: 29});
  for (const value of [v1, v2, v3, v4]) for (const ending of ['', '\n', '\r\n']) {
    const raw = Buffer.from(JSON.stringify(value) + ending), original = Buffer.from(raw);
    const parsed = dataPhaseProfile(raw);
    assert.deepEqual(parsed, value);
    assert.deepEqual(raw, original, 'reader must leave original stderr bytes untouched');
    assert.equal(JSON.stringify(parsed) + ending, original.toString(), 'reader must preserve each schema and its counters');
  }
  const parsed = dataPhaseProfile(Buffer.from(JSON.stringify(v4)));
  assert.deepEqual(parsed.phases, v1.phases); assert.deepEqual(parsed.details, v2.details);
  assert.equal(parsed.elapsedMicros, v1.elapsedMicros); assert.equal(parsed.timing, v1.timing);
  assert.deepEqual(parsed.publisher[4], {phase: 'private_spool_read_ready', calls: 2, elapsedMicros: 8});
  assert.deepEqual(parsed.publisher[5], {phase: 'final_stage_seal', calls: 1, elapsedMicros: 29});
  for (const change of [x => delete x.publisher, x => delete x.details, x => x.publisher.reverse(), x => x.publisher.pop(),
    x => x.publisher.splice(4, 1), x => x.publisher.splice(5, 1), x => x.publisher.push(x.publisher[0]),
    x => x.publisher[4].phase = 'stage_seal', x => x.publisher[5].phase = 'stage_seal',
    x => x.publisher[4] = structuredClone(v3.publisher[4]), x => x.publisher[5] = structuredClone(x.publisher[4]),
    x => [x.publisher[4], x.publisher[5]] = [x.publisher[5], x.publisher[4]], x => x.publisher = structuredClone(v3.publisher),
    x => x.publisher[4].extra = 0, x => x.publisher[4].calls = -1, x => x.publisher[4].calls = 0,
    x => x.publisher[4].elapsedMicros = 0.5, x => x.publisher[4].elapsedMicros = Number.MAX_SAFE_INTEGER + 1]) {
    const invalid = structuredClone(v4); change(invalid);
    assert.throws(() => dataPhaseProfile(Buffer.from(JSON.stringify(invalid))));
  }
  // A schema label is not an upgrade or downgrade operation. Reject every
  // mismatched historical/current shape, including v3/v4 mixed seal vectors.
  for (const value of [v1, v2, v3, v4]) for (const {schema} of [v1, v2, v3, v4]) {
    if (value.schema !== schema) assert.throws(() => dataPhaseProfile(Buffer.from(JSON.stringify({...value, schema}))));
  }
  const mixedV3 = structuredClone(v3); mixedV3.publisher[4] = structuredClone(v4.publisher[4]);
  assert.throws(() => dataPhaseProfile(Buffer.from(JSON.stringify(mixedV3))));
  mixedV3.publisher.splice(4, 1, ...structuredClone(v4.publisher.slice(4, 6)));
  assert.throws(() => dataPhaseProfile(Buffer.from(JSON.stringify(mixedV3))));
  const empty = structuredClone(v4);
  empty.publisher.forEach(phase => Object.assign(phase, {calls: 0, elapsedMicros: 0}));
  assert.deepEqual(dataPhaseProfile(Buffer.from(JSON.stringify(empty))), empty);
});
test('Data v2 keeps the original eleven phases and validates separate inclusive details without rewriting v1', () => {
  const old = phaseProfile();
  const value = {...old, schema: 'lycheedev.data-query-phases.v2', details: DATA_DETAIL_NAMES.map(phase => ({phase, calls: 1, elapsedMicros: 700}))};
  assert.deepEqual(dataPhaseProfile(Buffer.from(JSON.stringify(old))), old);
  assert.deepEqual(dataPhaseProfile(Buffer.from(JSON.stringify(value))), value);
  assert.deepEqual(value.phases, old.phases);
  for (const change of [x => delete x.details, x => x.details.reverse(), x => x.details.pop(), x => x.details.push(x.details[0]),
    x => x.details[0].phase = 'unknown', x => x.details[0].extra = 0, x => x.details[0].calls = -1,
    x => x.details[0].calls = 0, x => x.details[0].elapsedMicros = 0.5, x => x.schema = old.schema]) {
    const invalid = structuredClone(value); change(invalid);
    assert.throws(() => dataPhaseProfile(Buffer.from(JSON.stringify(invalid))));
  }
});
test('Data profile retains overlapping durations without treating their sum as a total', () => {
  const value = phaseProfile();
  for (const ending of ['', '\n', '\r\n']) assert.deepEqual(dataPhaseProfile(Buffer.from(JSON.stringify(value) + ending)), value);
  assert(value.phases.reduce((sum, phase) => sum + phase.elapsedMicros, 0) > value.elapsedMicros, 'overlap must not be rejected as a supposed total');
});
test('Data profile parser requires one exact record and rejects unknown stderr, schema, keys, phases and numbers', () => {
  for (const change of [x => x.schema += '.unknown', x => x.extra = 1, x => delete x.captureTiming, x => x.timing = 'cpu', x => x.captureTiming = 'exclusive',
    x => x.phases.reverse(), x => x.phases.pop(), x => x.phases.push(x.phases[0]), x => x.phases[0].phase = 'unknown', x => x.phases[0].extra = 0,
    x => x.phases[0].calls = -1, x => x.phases[0].calls = 0.5, x => x.phases[0].calls = 0, x => x.phases[0].elapsedMicros = 0.5,
    x => x.elapsedMicros = -1, x => x.elapsedMicros = Number.MAX_SAFE_INTEGER + 1, x => x.elapsedMicros = null, x => x.phases[0] = null]) {
    const value = phaseProfile(); change(value); assert.throws(() => dataPhaseProfile(Buffer.from(JSON.stringify(value))));
  }
  const good = JSON.stringify(phaseProfile());
  for (const text of ['', 'warning\n', good + '\n' + good, good + '\nwarning\n', 'warning\n' + good, good + '\n\n', '\ufeff' + good,
    '{"schema":"unknown",' + good.slice(1), good.replace('"elapsedMicros":1000', '"elapsedMicros":1e3'), 'x'.repeat(16385)]) assert.throws(() => dataPhaseProfile(Buffer.from(text)));
  assert.throws(() => dataPhaseProfile(Buffer.concat([Buffer.from(good), Buffer.from([255])])));
  assert.throws(() => dataPhaseProfile(good));
  const untouched = phaseProfile(); untouched.phases[0] = {...untouched.phases[0], calls: 0, elapsedMicros: 0};
  assert.deepEqual(dataPhaseProfile(Buffer.from(JSON.stringify(untouched))), untouched);
});
test('profile environment is opt-in only for measured Rust; Go, preparation and inherited values stay disabled', () => {
  const original = {KEEP: 'unchanged', LYCHEEDEV_DATA_PHASE_PROFILE: '1', lycheedev_data_phase_profile: '1'};
  for (const enabled of [false, true]) for (const measured of [false, true]) for (const engine of [null, 'go', 'rust']) {
    const actual = dataPhaseEnvironment(original, {enabled, measured, engine});
    assert.deepEqual(actual, enabled && measured && engine === 'rust' ? {KEEP: 'unchanged', LYCHEEDEV_DATA_PHASE_PROFILE: '1'} : {KEEP: 'unchanged'});
  }
  assert.deepEqual(original, {KEEP: 'unchanged', LYCHEEDEV_DATA_PHASE_PROFILE: '1', lycheedev_data_phase_profile: '1'});
});

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
test('known successful fixture paths require executed business scopes; early failures and unknown paths do not invent them', () => {
  for (const entry of dataCases(images())) {
    assert.deepEqual(dataPhaseProfile(Buffer.from(JSON.stringify(phaseProfile())), {entry, successful: true}), phaseProfile());
    const required = entry.kind === 'image' ? DATA_PHASE_NAMES : ['admission', 'prepare_inclusive', 'payload_inclusive', 'source_capture_metadata_inclusive', 'source_close'];
    for (const name of required) {
      const value = phaseProfile(); Object.assign(value.phases.find(phase => phase.phase === name), {calls: 0, elapsedMicros: 0});
      const bytes = Buffer.from(JSON.stringify(value));
      assert.throws(() => dataPhaseProfile(bytes, {entry, successful: true}), /required successful fixture scope/);
      assert.deepEqual(dataPhaseProfile(bytes, {entry, successful: false}), value);
    }
  }
  const zero = phaseProfile(); zero.phases.forEach(phase => Object.assign(phase, {calls: 0, elapsedMicros: 0}));
  assert.deepEqual(dataPhaseProfile(Buffer.from(JSON.stringify(zero)), {entry: {kind: 'sql', id: 'unknown-path'}, successful: true}), zero);
  const sql = phaseProfile(); for (const name of ['texture_decode', 'pixel_transform_and_hash', 'encode', 'artifact_capture_metadata_inclusive', 'manifest_capture_metadata_inclusive', 'output']) {
    Object.assign(sql.phases.find(phase => phase.phase === name), {calls: 0, elapsedMicros: 0});
  }
  assert.deepEqual(dataPhaseProfile(Buffer.from(JSON.stringify(sql)), {entry: sqlCase, successful: true}), sql);
});
function pairs(entry = contentCase, count = 20) { return Array.from({length: count}, (_, iteration) => ['cold', 'warm'].map(cacheMode => ({caseId: entry.id, iteration, cacheMode, resultEquivalent: true, pairTerminal: {state: 'verified'}, order: orderFor(iteration),
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
test('final pair integrity is required independently of equivalent product rows, including SQL and the final iteration', () => {
  for (const [entry, count, failedIteration] of [[sqlCase, 1, 0], [contentCase, 20, 19], [contentCase, 20, 9]]) {
    const value = pairs(entry, count);
    for (const row of value.filter(row => row.iteration === failedIteration)) row.pairTerminal = {state: 'failed_final_integrity', failure: {phase: 'final_integrity', message: 'fixture changed'}};
    assert(value.every(row => row.resultEquivalent));
    const groups = summarizeData([entry], value, true);
    assert(groups.every(group => group.state === 'failed_final_integrity' && !group.correctAndComplete && !group.pairTerminalVerified));
  }
  const legacy = pairs(sqlCase, 1); for (const row of legacy) delete row.pairTerminal;
  assert(summarizeData([sqlCase], legacy, true).every(group => !group.correctAndComplete));
  assert(summarizeData([sqlCase], legacy, false, {allowLegacyTerminal: true}).every(group => group.state === 'equivalent_observation_only'));
  legacy[0].pairTerminal = {state: 'failed_final_integrity'};
  assert(summarizeData([sqlCase], legacy, false, {allowLegacyTerminal: true}).every(group => group.state === 'failed_final_integrity'));
});
test('real controller preserves product results and raw timings across final-integrity, startup and Data phase failures', t => {
  const directory = mkdtempSync(join(tmpdir(), 'paired-data-terminal-')); t.after(() => rmSync(directory, {recursive: true, force: true}));
  const controller = readFileSync(new URL('./data-run.mjs', import.meta.url));
  const real = file => JSON.stringify(new URL(file, import.meta.url).href);
  for (const [name, entry, failAt, expectedOwners, diagnostics = false, missingDiagnostics = false, profileCase = 'none'] of [
    ['sql-only', sqlCase, 2, 1], ['last-performance', contentCase, 42, 21], ['middle-performance', contentCase, 22, 11], ['complete-performance', contentCase, 0, 21],
    ['startup-complete', contentCase, 0, 1, true], ['startup-missing', contentCase, 0, 1, true, true],
    ['profile-all-twelve', dataCases(images()), 0, 12, true, false, 'complete'],
    ['profile-missing', contentCase, 0, 1, true, false, 'missing'], ['profile-unknown', contentCase, 0, 1, true, false, 'unknown'],
    ['profile-zero', contentCase, 0, 1, true, false, 'zero'],
    ['profile-malformed', contentCase, 0, 1, true, false, 'malformed'], ['profile-product-failure', contentCase, 0, 1, true, false, 'product-failure'],
    ['profile-product-failure-zero', contentCase, 0, 1, true, false, 'product-failure-zero'],
    ['profile-both-failed', contentCase, 0, 1, true, true, 'product-failure-missing'], ['ordinary-stderr', contentCase, 0, 1, false, false, 'ordinary-stderr'],
  ]) {
    const profileEnabled = profileCase !== 'none' && profileCase !== 'ordinary-stderr';
    const productFailed = profileCase.startsWith('product-failure') || profileCase === 'ordinary-stderr';
    const profileFailed = ['missing', 'unknown', 'malformed', 'zero', 'product-failure-missing'].includes(profileCase);
    const profile = phaseProfile(); if (profileCase === 'malformed') profile.phases[0].calls = 0.5;
    if (['zero', 'product-failure-zero'].includes(profileCase)) profile.phases.forEach(phase => Object.assign(phase, {calls: 0, elapsedMicros: 0}));
    const stderr = ['complete', 'product-failure', 'product-failure-zero', 'malformed', 'zero'].includes(profileCase) ? JSON.stringify(profile) + '\n'
      : profileCase === 'unknown' ? JSON.stringify(profile) + '\nunknown stderr\n' : profileCase === 'ordinary-stderr' ? 'unexpected ordinary stderr\n' : '';
    const root = join(directory, name), out = join(root, 'evidence'), scratch = join(root, 'scratch'); mkdirSync(out, {recursive: true}); mkdirSync(scratch);
    const put = (file, body) => { const path = join(root, file); mkdirSync(dirname(path), {recursive: true}); writeFileSync(path, body); };
    put('tests/rust-performance-paired/data-run.mjs', controller);
    put('tests/rust-performance-paired/contract.mjs', `export * from ${real('./contract.mjs')};`);
    put('tests/rust-performance-paired/data-phase-profile.mjs', `export * from ${real('./data-phase-profile.mjs')};`);
    put('tests/rust-performance-paired/data-contract.mjs', `export * from ${real('./data-contract.mjs')}; export const dataCases=()=>${JSON.stringify(Array.isArray(entry) ? entry : [entry])};`);
    put('tests/rust-performance-paired/data-resume.mjs', `export function verifyDataContinuation(){throw Error('acceptance cannot resume');}`);
    put('tests/rust-performance-paired/inputs.mjs', `import {mkdirSync} from 'node:fs';
      let inventories=0; export function inventory(path){ return {sha256:++inventories===${failAt}?'changed-fixture':'fixture'}; }
      export const cloneSeed=(_seed,home)=>mkdirSync(home,{recursive:true}); export const verifyGoMetadata=()=>({}); export const verifyGoPackage=()=>({packageSHA256:'go'});`);
    put('tests/rust-performance-paired/data-inputs.mjs', `export {digest,parse} from ${real('./data-inputs.mjs')};
      export const loadDataFixture=directory=>({directory,images:{},receipt:{inventory:{sha256:'fixture'}},identity:{snapshot:'PIN-fixed',installation:'installation'}});
      export const seedDataHome=()=>({sha256:'seed'}); export const decodedIdentity=home=>globalThis.decoded?.has(home)?['root','payload']:[];
      export const validateDataOutput=async()=>({value:{complete:true},observations:{reuse:{verified:true}},proofs:[]}); export const decodeDataImage=()=>{throw Error('not an image test');};`);
    put('tools/rust-build.mjs', `import {readFileSync} from 'node:fs'; import {createHash} from 'node:crypto';
      export const hashFile=path=>createHash('sha256').update(readFileSync(path)).digest('hex'); export const sourceDigest=()=> 'source';
      export const verifyArtifact=()=>({profile:'release',binarySHA256:'rust',inputHashes:{sourceTree:'source',emmy:{patchedTreeSHA256:'emmy'}}});`);
    put('tools/rust-emmy-identity.mjs', `export const inspectEmmy=()=>({identity:'public-upstream-unmodified-v1',patchedTreeSHA256:'emmy'});`);
    put('tools/rust-owned-memory.mjs', `export const collectOwnedMemory=()=>({state:'not_measured',synthetic:true});`);
    put('tools/rust-disk.mjs', `import {mkdirSync,writeFileSync} from 'node:fs'; import {join} from 'node:path'; let id=0;
      export function beginScratch(root){ const directory=join(root,'pairs',String(++id)), evidence=join(root,'pair-evidence',String(id)); mkdirSync(directory,{recursive:true}); mkdirSync(evidence,{recursive:true});
        return {directory,evidence,receipt:join(evidence,'receipt.json'),seal:value=>writeFileSync(join(evidence,'receipt.json'),JSON.stringify(value)),cleanup:()=>writeFileSync(join(evidence,'cleaned'),'yes')}; }`);
    put('tools/rust-owned-process.mjs', `import assert from 'node:assert/strict'; import {appendFileSync,writeFileSync} from 'node:fs'; import {join} from 'node:path'; let invocation=0;
      export function ownedSpawnSync(binary,args,options){
      const measured=options.memoryObservation, rust=binary==='rust.exe', expectedProfile=${profileEnabled}&&measured&&rust;
      const profileKeys=Object.keys(options.env).filter(key=>key.toUpperCase()==='LYCHEEDEV_DATA_PHASE_PROFILE');
      assert.deepEqual(profileKeys,expectedProfile?['LYCHEEDEV_DATA_PHASE_PROFILE']:[]);
      if(expectedProfile) assert.equal(options.env.LYCHEEDEV_DATA_PHASE_PROFILE,'1');
      appendFileSync(${JSON.stringify(join(out, 'invocations.jsonl'))},JSON.stringify({binary,args,measured,startupDiagnostics:options.startupDiagnostics,profile:options.env.LYCHEEDEV_DATA_PHASE_PROFILE})+'\\n');
      if(options.startupDiagnostics&&!options.memoryObservation) throw Error('preparation must not request startup diagnostics');
      let output=''; if(binary==='git'&&args.includes('rev-parse')) output=${JSON.stringify(GO_COMMIT)};
      else if(binary==='rustc') output='rustc 1.99.0 synthetic'; else if(binary==='powercfg.exe') output='power';
      else if(args[0]==='init') output=JSON.stringify({ok:true}); else if(options.memoryObservation){output='{}';(globalThis.decoded??=new Set()).add(args[args.indexOf('--home')+1]);}
      const end=binary==='rust.exe'?'400':'1100';
      const stderr=Buffer.from(measured&&rust?${JSON.stringify(stderr)}:''); const log=join(options.evidence,'raw-'+(++invocation)+'.stderr.bin'); writeFileSync(log,stderr);
      return {status:measured&&rust&&${profileCase.startsWith('product-failure')}?17:0,error:measured&&rust&&${profileCase.startsWith('product-failure')}?new Error('original product error'):undefined,
        handlesReaped:true,stdout:Buffer.from(output),stderr,logs:[log],proof:{schema:'lycheedev.owned-process.v1',rootReaped:true,treeReaped:true,handlesClosed:true,toolIdentityLocksClosed:true,outputReadersComplete:true,activeProcesses:0,forced:false,
        ...(options.startupDiagnostics&&${!missingDiagnostics}?{diagnostics:{schema:'lycheedev.owned-startup-diagnostics.v1',createReturnTicks:'110',rootPersistBeginTicks:'120',rootPersistEndTicks:'130',observerSetupEndTicks:'140',resumeBeforeTicks:'150',resumeAfterTicks:'160',rootCpuObserved:true,rootKernel100ns:'10000',rootUser100ns:'20000'}}:{}),
        timing:{schema:'lycheedev.owned-timing.v1',frequency:10000,startTicks:'100',rootExitTicks:end,treeReapedTicks:end}}}; }`);
    put('release/npm-channel.json', JSON.stringify({version: '3.5.0', distTag: 'beta'}));
    put('sharp.cjs', `module.exports=Object.assign(()=>{}, {versions:{sharp:'synthetic',vips:'synthetic'},cache:()=>({memory:{max:0},files:{max:0},items:{max:0}})});`);
    const config = {root, runnerRoot: root, evidence: out, scratch, acceptance: !diagnostics, resumeFrom: null, startupDiagnostics: diagnostics, dataPhaseProfile: profileEnabled, inputs: {}, conditions: {},
      rustArtifact: 'synthetic', rust: {sourceBinary: 'rust.exe', binarySHA256: 'rust'}, go: {root: 'go', binary: 'go.exe', binarySHA256: 'go', packageSHA256: 'go'},
      goSource: 'go-source', luaLSArchive: 'luals', sharpModule: join(root, 'sharp.cjs'), fixtures: {catalog: 'catalog', content: 'content', images: 'images'}};
    put('configuration.json', JSON.stringify(config));
    const script = join(root, 'tests/rust-performance-paired/data-run.mjs');
    const bootstrap = `Object.defineProperty(process,'platform',{value:'win32'}); process.argv=[process.execPath,${JSON.stringify(script)},${JSON.stringify(join(root, 'configuration.json'))}]; await import(${JSON.stringify(pathToFileURL(script).href)});`;
    const run = spawnSync(process.execPath, ['--input-type=module', '--eval', bootstrap], {cwd: root, encoding: 'utf8', timeout: 15000, env: {...process.env, RUSTFLAGS: '', CARGO_ENCODED_RUSTFLAGS: '', LYCHEEDEV_DATA_PHASE_PROFILE: '1', lycheedev_data_phase_profile: '1'}});
    assert.equal(run.error, undefined); assert.equal(run.status, failAt || missingDiagnostics || profileFailed || productFailed ? 1 : 0, run.stderr);
    const report = JSON.parse(readFileSync(join(out, 'report.json'))); assert.equal(report.pairOwners.length, expectedOwners, JSON.stringify(report));
    const measuredRows = [...report.preflightPairs, ...report.pairs];
    assert(measuredRows.every(row => productFailed ? !row.resultEquivalent && row.go.valid && !row.rust.valid : row.resultEquivalent && row.go.valid && row.rust.valid));
    assert.equal(report.dataPhaseProfile, profileEnabled);
    assert.equal(report.startupDiagnostics, diagnostics); assert.equal(report.performanceAcceptanceEvaluated, !diagnostics);
    if (diagnostics) assert.match(report.metrics.cpu, /original-root GetProcessTimes requested.*startupFailure/);
    else assert.equal(report.metrics.cpu, 'not_measured');
    for (const row of measuredRows) for (const engine of ['go', 'rust']) {
      assert.equal(row[engine].controllableMillis, engine === 'go' ? 100 : 30, 'original total never reduced');
      assert.equal(row[engine].handlesReaped, true);
      assert.equal(row[engine].stderrSHA256, digest(engine === 'rust' ? stderr : '')); assert.equal(row[engine].stderrBytes, Buffer.byteLength(engine === 'rust' ? stderr : ''));
      assert.equal(readFileSync(row[engine].logs[0], 'utf8'), engine === 'rust' ? stderr : '');
      assert.equal(row[engine].exit, engine === 'rust' && profileCase.startsWith('product-failure') ? 17 : 0);
      assert.equal(row[engine].error, engine === 'rust' && profileCase.startsWith('product-failure') ? 'original product error' : undefined);
      if (engine === 'rust' && profileEnabled) {
        assert.equal(Boolean(row[engine].dataPhaseProfileFailure), profileFailed);
        if (!profileFailed) assert.deepEqual(row[engine].dataPhaseProfile, profile);
      } else { assert.equal(row[engine].dataPhaseProfile, undefined); assert.equal(row[engine].dataPhaseProfileFailure, undefined); }
      if (diagnostics && !missingDiagnostics) { assert.equal(row[engine].startup.controllableMillis, row[engine].controllableMillis); assert.equal(row[engine].startup.preResumeMillis, 5); }
      else { assert.equal(row[engine].startup, undefined); assert.equal(Boolean(row[engine].startupFailure), missingDiagnostics); }
    }
    if (failAt) {
      assert.equal(report.state, 'failed'); assert(report.groups.every(group => group.state === 'failed_final_integrity' && !group.correctAndComplete));
      const terminal = report.pairOwners.at(-1); assert.equal(terminal.pairTerminal.state, 'failed_final_integrity');
      const pair = JSON.parse(readFileSync(join(dirname(terminal.receipt), 'pair.json'))); assert.equal(pair.valid, false); assert.equal(pair.rows.length, 2);
      assert(pair.rows.every(row => row.resultEquivalent && row.pairTerminal.state === 'failed_final_integrity'));
      assert.equal(JSON.parse(readFileSync(terminal.receipt)).state, 'failed');
    } else if (productFailed) {
      assert.equal(report.state, 'failed'); assert.equal(measuredRows.length, 1, 'failed cold has no warm replacement');
      assert.equal(report.pairOwners[0].pairTerminal.state, 'failed_pair'); assert(measuredRows[0].rust.failure);
    } else if (missingDiagnostics || profileFailed) {
      assert.equal(report.state, 'failed'); assert(report.groups.every(group => group.state === 'failed_diagnostics' && !group.correctAndComplete));
      const terminal = report.pairOwners.at(-1); assert.equal(terminal.pairTerminal.state, 'failed_diagnostics');
      assert.equal(JSON.parse(readFileSync(terminal.receipt)).handlesReaped, true); assert.equal(JSON.parse(readFileSync(terminal.receipt)).state, 'failed');
    } else { assert.equal(report.state, diagnostics ? 'equivalent_data_observation_only' : 'passed_selected_data_paths_full_acceptance_open'); assert(report.groups.every(group => group.correctAndComplete && group.pairTerminalVerified)); }
    const invocations = readFileSync(join(out, 'invocations.jsonl'), 'utf8').trim().split('\n').map(text => JSON.parse(text));
    assert.equal(invocations.filter(row => !row.measured).length, 15, 'preparation stays outside measurement');
    assert(invocations.filter(row => !row.measured).every(row => !row.profile && row.startupDiagnostics === false));
    const measured = invocations.filter(row => row.measured);
    assert.equal(measured.length, (report.pairs.length + report.preflightPairs.length) * 2);
    if (name === 'profile-all-twelve') { assert.equal(measured.length, 48); assert.equal(report.pairs.length, 24); assert.equal(report.groups.length, 24); }
  }
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
test('independent image decoder receives only authenticated bounded encoded bytes', async t => {
  const sample = materializedImage(t), original = sample.decodeImage;
  sample.decodeImage = async bytes => { assert(Buffer.isBuffer(bytes)); assert(bytes.equals(readFileSync(sample.output))); return original(bytes); };
  await validateDataOutput(sample);
});
test('buffer decoder destroys its pipeline after success and failure, preserving both errors', async () => {
  const bytes = Buffer.from('encoded');
  for (const fail of [false, true]) {
    const events = [], error = new Error('decode failed'), decoder = {ensureAlpha() { return this; }, raw() { return this; }, async toBuffer() { events.push('decode'); if (fail) throw error; return 'pixels'; }, destroy() { events.push('destroy'); }};
    const work = decodeDataImage((input, options) => { assert.equal(input, bytes); assert.equal(options.limitInputPixels, 1031 * 519); return decoder; }, bytes);
    if (fail) await assert.rejects(work, actual => actual === error); else assert.equal(await work, 'pixels');
    assert.deepEqual(events, ['decode', 'destroy']);
  }
  const first = new Error('decode first'), second = new Error('destroy second');
  const decoder = {ensureAlpha() { return this; }, raw() { return this; }, async toBuffer() { throw first; }, destroy() { throw second; }};
  await assert.rejects(decodeDataImage(() => decoder, bytes), error => error instanceof AggregateError && error.cause === first && error.errors[1] === second);
  await assert.rejects(decodeDataImage(() => { throw Error('should not run'); }, 'scratch.webp'), /Buffer required/);
});
test('runner preserves original owned clock and stops failed paths before warm/replacement', () => {
  const source = readFileSync(new URL('./data-run.mjs', import.meta.url), 'utf8');
  assert(source.includes('elapsed(run.proof)')); assert(source.includes('memoryObservation: measured')); assert(source.includes('if (!row.resultEquivalent) break'));
  assert(source.includes('if (!verified || !config.acceptance || !entry.performance) continue')); assert(source.includes('if (!fatal && valid) {'));
  assert(source.includes("state: 'failed_retained'")); assert(source.includes('job.cleanup();')); assert(!source.includes('maxRetries'));
  assert(source.includes('sharp.cache(false)')); assert(!source.includes('sharp(path)'));
  assert(!source.includes('rust-wdc-performance')); assert(!source.includes('target/release/lycheedev')); assert(!source.includes('spawnSync('));
});
