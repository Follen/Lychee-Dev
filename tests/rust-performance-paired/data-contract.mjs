// Narrow B04 contracts. No Go312 driver, product fallback or timing subtraction.
import assert from 'node:assert/strict';
import {isDeepStrictEqual} from 'node:util';
import {GO_COMMIT, REPETITIONS, TARGET_RATIO, orderFor, statistics} from './contract.mjs';
export {GO_COMMIT, REPETITIONS, TARGET_RATIO, orderFor};
export const SQL = 'SELECT ID, Name_lang, ModelFileDataID FROM HouseDecor ORDER BY ID';
export const SQL_ROWS = [[10, '无模型🙂', 0], [20, '第二', 501], [30, '第三', 501]];
export const CONTENT_BYTES = 17 << 20;
export const PROOF = Object.freeze({encodingFramingEKey: true, encodingRequestedPagesMD5: true, encodingWholeDecodedCKey: false, rootWholeDecodedCKey: true, contentWholeDecodedCKey: true});
const usageKeys = ['decodeWork', 'metadataBytes', 'networkBytes', 'networkRequests', 'retainedBytes'];
export function unsigned(value, label) { assert(Number.isSafeInteger(value) && value >= 0, label); }
export function usage(value) {
  assert.deepEqual(Object.keys(value).sort(), usageKeys, 'resource fields');
  for (const key of usageKeys) unsigned(value[key], key);
  assert.equal(value.networkBytes, 0); assert.equal(value.networkRequests, 0);
  assert(value.decodeWork <= 100000000, 'unchanged work limit');
  return value;
}
export function dataCases(images) {
  assert.equal(images.schema, 'lycheedev.large-image-paired-fixture.v1');
  assert.deepEqual(images.encodings, ['png', 'webp']); assert.deepEqual(images.channels, ['rgba']);
  assert.equal(images.cases.length, 5);
  const storage = ['palette', 'bgra8', 'bc1', 'bc2', 'bc3'];
  return [
    {id: 'B04-sql-correctness', kind: 'sql', fixture: 'catalog', performance: false, args: ['data', 'sql', '--sql', SQL]},
    {id: 'B04-blte-17MiB', kind: 'content', fixture: 'content', performance: true, fileDataID: 501, args: ['asset', 'inspect', '--file-id', '501', '--max-bytes', String(CONTENT_BYTES)]},
    ...images.cases.flatMap((image, index) => {
      assert.equal(image.fileDataID, 701 + index); assert.equal(image.mipmap, 0);
      assert.equal(image.texture.width, 1031); assert.equal(image.texture.height, 519);
      assert.equal(image.texture.storage, storage[index]); assert.equal(image.texture.format, 'BLP2');
      assert.equal(image.texture.mipmap, 0); assert.equal(image.texture.levels, 1);
      assert.equal(image.pixelsBytes, 1031 * 519 * 4); assert.match(image.pixelsSHA256, /^[0-9a-f]{64}$/);
      assert.equal(image.pixelsPath, `pixels/${image.fileDataID}.rgba`);
      return images.encodings.map(encoding => ({id: `B04-${storage[index]}-${encoding}`, kind: 'image', fixture: 'images', performance: true, fileDataID: image.fileDataID, encoding, image,
        args: ['asset', 'export', '--file-id', String(image.fileDataID), '--encoding', encoding, '--mipmap', '0', '--channels', 'rgba']}));
    }),
  ];
}

export function auditCache(stats, cacheMode, expected) {
  const keys = ['hits', 'reusedBytes', 'contentHashCalls', 'contentHashBytes', 'contentDecodeCalls', 'extractedBytes', 'blobReads', 'blobReadBytes', 'fragmentLoads', 'blockCacheHits'];
  assert.deepEqual(Object.keys(stats).sort(), keys.sort(), 'complete cache counters required');
  for (const key of keys) unsigned(stats[key], `cache.${key}`);
  assert.equal(stats.contentHashCalls, 2, 'Root and payload remain hashed'); assert(stats.contentHashBytes > 0);
  if (cacheMode === 'warm') { assert.equal(stats.hits, 2); assert.equal(stats.contentDecodeCalls, 0); assert(stats.reusedBytes > 0); }
  else { assert.equal(cacheMode, 'cold'); assert.equal(stats.hits, 0); assert.equal(stats.contentDecodeCalls, 2); }
  if (expected) {
    unsigned(expected.encodedBytes, 'expected encoded bytes'); unsigned(expected.decodedBytes, 'expected decoded bytes');
    assert(expected.encodedBytes > 0 && expected.decodedBytes > 0);
    assert.equal(stats.contentHashBytes, expected.encodedBytes, 'entire authenticated Root and payload hashed');
    assert.equal(stats.reusedBytes, cacheMode === 'warm' ? expected.decodedBytes : 0, 'entire decoded reuse');
    assert.equal(stats.extractedBytes, cacheMode === 'cold' ? expected.decodedBytes : 0, 'entire cold extraction');
  }
  return {verified: !!expected, basis: expected ? 'authenticated full Root/payload encoded and decoded byte totals plus exact hash/decode/hit counts' : 'counter shape only; full fixture byte checks still required', expected, counters: stats};
}

function accounting(value, sql = false) {
  assert.equal(value.wholeQueryAccountingComplete, false, 'do not overstate accounting completeness'); usage(value.used);
  if (sql) {
    assert.deepEqual(Object.keys(value).sort(), ['logicalProviderParityComplete', 'remainingGaps', 'scope', 'strategy', 'used', 'wholeQueryAccountingComplete'].sort());
    assert.equal(value.scope, 'admitted allocation and work across local CASC/Encoding/decoded cache/definitions/WDC/SQL');
    assert.equal(value.strategy, 'conservative cumulative admission'); assert.equal(value.logicalProviderParityComplete, false);
    assert.deepEqual(value.remainingGaps, ['physical/effective provider logical counters not audited against Go 3.2', 'SQL plan/order-container transient allocations', 'uninterruptible single codec call']);
  } else assert.deepEqual(Object.keys(value).sort(), ['used', 'wholeQueryAccountingComplete']);
}

// Preserve every public field except these explicit, individually audited
// observations. New/unknown fields are never silently projected away.
export function semanticData(entry, envelope, engine, cacheMode) {
  assert.equal(envelope.schema, 'lycheedev.result.v1'); assert.equal(envelope.ok, true); assert.equal(envelope.error, null);
  assert(Array.isArray(envelope.warnings)); assert(Array.isArray(envelope.captures));
  const copy = structuredClone(envelope), result = copy.result, observations = {};
  const file = entry.kind === 'sql' ? result.sources[0]?.file : entry.kind === 'image' ? result.source : result;
  assert(file && file.contentVerified === true && file.source === 'installation');
  observations.cache = structuredClone(file.decodedCache); observations.reuse = auditCache(file.decodedCache, cacheMode);
  file.decodedCache = '<audited-cache-counters>';
  if (engine === 'rust') {
    assert.deepEqual(file.verification, PROOF); observations.verification = file.verification; delete file.verification;
    assert.deepEqual(Object.keys(file.encodingIO).sort(), ['bytes', 'reads', 'wholeEncoded']);
    unsigned(file.encodingIO.bytes, 'encoding bytes'); unsigned(file.encodingIO.reads, 'encoding reads'); assert.equal(file.encodingIO.wholeEncoded, false);
    observations.encodingIO = file.encodingIO; delete file.encodingIO;
    if (entry.kind === 'content') { accounting(result.resourceAccounting); observations.accounting = result.resourceAccounting; delete result.resourceAccounting; }
  } else { assert.equal(engine, 'go'); assert(!Object.hasOwn(file, 'verification') && !Object.hasOwn(file, 'encodingIO')); }
  if (entry.kind === 'sql') {
    assert.equal(result.complete, true); assert.equal(result.partial, false); assert.equal(result.sources.length, 1);
    assert.deepEqual(result.result.rows, SQL_ROWS); assert.equal(result.query.sql, SQL);
    observations.resources = usage(result.resources); result.resources = '<audited-resource-usage>';
    const memory = result.memoryBudget;
    assert.deepEqual(Object.keys(memory).sort(), ['mode', 'concurrentQueries', 'ceilingBytes', 'retainedBytes', 'metadataBytes', 'peakScratchBytes', 'growths'].sort());
    assert.equal(memory.mode, 'windows_adaptive');
    for (const key of Object.keys(memory).filter(key => key !== 'mode')) unsigned(memory[key], `memory.${key}`);
    assert(memory.concurrentQueries >= 1); assert(memory.ceilingBytes >= 512 << 20, 'require unconstrained initial budget for selected tiny SQL input');
    assert.equal(memory.retainedBytes, 512 << 20); assert.equal(memory.metadataBytes, 128 << 20); assert.equal(memory.growths, 0);
    assert(observations.resources.retainedBytes + memory.peakScratchBytes <= memory.retainedBytes);
    assert(observations.resources.metadataBytes <= memory.metadataBytes);
    observations.memoryBudget = structuredClone(memory); memory.ceilingBytes = '<observed-available-memory>'; memory.peakScratchBytes = '<audited-peak-scratch>';
    assert.deepEqual(Object.keys(result.timings).sort(), ['decodedSourceBytes', 'definitionsMs', 'executionMs', 'preparationMs']);
    for (const key of ['definitionsMs', 'executionMs', 'preparationMs']) { assert(Number.isFinite(result.timings[key]) && result.timings[key] >= 0); }
    unsigned(result.timings.decodedSourceBytes, 'decoded source bytes'); observations.timings = structuredClone(result.timings);
    for (const key of ['definitionsMs', 'executionMs', 'preparationMs']) result.timings[key] = '<observed-time>';
    if (engine === 'rust') { accounting(result.resourceAccounting, true); observations.accounting = result.resourceAccounting; delete result.resourceAccounting; }
  }
  return {value: copy, observations};
}

export function compareData(go, rust) {
  assert(isDeepStrictEqual(go, rust), 'full stable Go323 Data envelopes differ; raw outputs retained');
  return true;
}
export function summarizeData(entries, pairs, acceptance) {
  return entries.flatMap(entry => ['cold', 'warm'].map(cacheMode => {
    const needed = acceptance && entry.performance ? REPETITIONS : 1;
    const rows = pairs.filter(row => row.caseId === entry.id && row.cacheMode === cacheMode);
    const homes = new Set(rows.flatMap(row => ['go', 'rust'].map(engine => row[engine]?.home)));
    const correct = rows.length === needed && homes.size === needed * 2 && !homes.has(undefined) && new Set(rows.map(row => row.iteration)).size === needed && rows.every((row, index) => {
      const cold = pairs.find(other => other.caseId === row.caseId && other.iteration === row.iteration && other.cacheMode === 'cold');
      return row.iteration === index && row.resultEquivalent === true && cold?.resultEquivalent === true && row.order?.join() === orderFor(index).join() && ['go', 'rust'].every(engine => row[engine]?.valid === true && row[engine]?.reuse?.verified === true && cold?.[engine]?.valid === true && row[engine]?.home === cold?.[engine]?.home && row[engine]?.binary === cold?.[engine]?.binary && row[engine]?.snapshot === cold?.[engine]?.snapshot);
    });
    const timed = rows.length > 0 && rows.every(row => ['go', 'rust'].every(engine => Number.isFinite(row[engine]?.controllableMillis) && row[engine].controllableMillis > 0));
    const timings = timed ? Object.fromEntries(['go', 'rust'].map(engine => [engine, statistics(rows.map(row => row[engine].controllableMillis))])) : null;
    const ratio = timings ? timings.rust.median / timings.go.median : null;
    const evaluated = acceptance && entry.performance;
    return {caseId: entry.id, cacheMode, requiredPairs: needed, attemptedPairs: rows.length, correctAndComplete: correct, timings,
      performanceAcceptanceEvaluated: evaluated, medianRatio: evaluated ? ratio : null, targetRatio: evaluated ? TARGET_RATIO : null,
      state: !correct ? 'failed_correctness_or_missing_samples' : !evaluated ? 'equivalent_observation_only' : ratio !== null && ratio <= TARGET_RATIO ? 'passed_selected_path' : 'failed_performance'};
  }));
}
