// Production static-SQL generation evidence. No timing subtraction or narrowed rows.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
export const GO_COMMIT = '3766f9cacb52e93a803a626187e2a1d54a445468';
export const REPETITIONS = 20;
export const TARGET_RATIO = 0.30;
export const PHASES = Object.freeze(['fresh-wdc', 'prepare', 'first-after-prepare', 'subsequent-hit']);
export const orderFor = i => i % 2 === 0 ? ['go', 'rust'] : ['rust', 'go'];
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const hash = value => assert.match(value, /^[0-9a-f]{64}$/);
const unsigned = value => assert(Number.isSafeInteger(value) && value >= 0, 'exact nonnegative counter');
// Compare decimal values rather than exponent spelling: 1e-7 and 1E-07 agree,
// but 1.5000000000000001 cannot silently become 1.5. This deliberately rejects
// noncanonical higher-precision fractions instead of silently rounding them.
function decimalIdentity(token) {
  const match = /^(-?)(\d+)(?:\.(\d+))?(?:[eE]([+-]?\d+))?$/.exec(token);
  assert(match, 'JSON numeric token');
  let digits = (match[2] + (match[3] ?? '')).replace(/^0+/, '');
  let exponent = Number(match[4] ?? 0) - (match[3]?.length ?? 0);
  if (!digits) return '0'; // Sign of zero is retained by JSON.parse itself.
  while (digits.endsWith('0')) { digits = digits.slice(0,-1); exponent++; }
  return `${match[1]}${digits}e${exponent}`;
}
// Node 22+ source context is mandatory: never accept a previously rounded value.
// Integer tokens outside the safe range become bigint, including nested arrays.
export function parseLossless(bytes) {
  const text = typeof bytes === 'string' ? bytes : new TextDecoder('utf-8', {fatal: true}).decode(bytes);
  return JSON.parse(text, (_key, value, context) => {
    if (typeof value !== 'number') return value;
    assert.equal(typeof context?.source, 'string', 'lossless JSON requires reviver source context');
    assert(Number.isFinite(value), 'nonfinite JSON number');
    if (/^-?\d+$/.test(context.source)) {
      const integer = BigInt(context.source);
      if (integer > BigInt(Number.MAX_SAFE_INTEGER) || integer < BigInt(Number.MIN_SAFE_INTEGER)) return integer;
    } else {
      assert.equal(decimalIdentity(context.source), decimalIdentity(String(value)), 'lossy decimal JSON number');
      if (Number.isInteger(value) && !Number.isSafeInteger(value)) {
        const [digits, exponent] = decimalIdentity(context.source).split('e');
        assert(Number(exponent) >= 0, 'exact integral decimal exponent');
        return BigInt(digits) * 10n ** BigInt(exponent);
      }
    }
    return value;
  });
}
export function stringifyLossless(value) {
  return JSON.stringify(value, (_key, item) => {
    if (typeof item === 'bigint') return JSON.rawJSON(item.toString());
    if (typeof item === 'number') assert(Number.isFinite(item), 'nonfinite JSON number');
    return item;
  });
}
export function auditSqlSemantics(envelope, expected) {
  assert.equal(envelope.schema, 'lycheedev.result.v1'); assert.equal(envelope.ok, true); assert.equal(envelope.error, null);
  assert(Array.isArray(envelope.warnings)); assert(Array.isArray(envelope.captures));
  assert.equal(envelope.context.snapshot, expected.snapshot);
  const result = envelope.result;
  assert.equal(result.query.sql, expected.sql); assert.equal(result.complete, true); assert.equal(result.partial, false);
  assert.deepEqual(result.result.columns, expected.columns, 'all ordered SQL columns');
  assert.deepEqual(result.result.rows, expected.rows, 'all ordered SQL rows and exact nested cells');
  assert(result.sources.length > 0, 'source provenance required');
  for (const source of result.sources) {
    assert.equal(source.complete, true); assert.equal(source.truncated, false);
    assert.equal(source.file.contentVerified, true); assert.equal(source.file.source, 'installation');
    assert.deepEqual(source.file.pin, expected.pin);
  }
  return true;
}
// Normalizations must be independently audited by the caller. Do not project a
// whitelist: compare every stable field, including unknown fields and coverage.
export function compareSqlSemantics(go, rust) { assert.deepEqual(rust, go, 'full stable SQL envelope differs'); return true; }
export function auditSqlCapture(envelope, {capture, canonicalBytes, blobBytes, storedCapture}) {
  assert.equal(envelope.captures.length, 1); assert.deepEqual(envelope.captures[0], capture);
  assert.deepEqual(storedCapture, capture, 'persisted CAP metadata');
  assert.equal(capture.id, 'CAP-' + sha256(canonicalBytes));
  const canonical = parseLossless(canonicalBytes); assert.deepEqual(canonical, {...capture, id: ''}, 'canonical CAP bytes must bind every field');
  assert.equal(capture.mediaType, 'application/json'); assert.equal(capture.provenance.kind, 'data-query');
  assert.equal(capture.provenance.snapshot, envelope.context.snapshot);
  assert.equal(capture.complete, true); assert.equal(capture.truncated, false); assert(Number.isFinite(Date.parse(capture.capturedAt)));
  hash(capture.blob.sha256); unsigned(capture.blob.bytes); assert.equal(blobBytes.length, capture.blob.bytes);
  assert.equal(sha256(blobBytes), capture.blob.sha256, 'full CAP body hash');
  assert.deepEqual(parseLossless(blobBytes), envelope.result, 'CAP includes every exact SQL field and cell');
  return {identityVerified: true, metadataVerified: true, completeBytesVerified: true};
}
export function auditGenerationProof(observation, {phase, generation, object}) {
  assert(PHASES.includes(phase));
  if (phase === 'fresh-wdc') { assert.equal(observation, undefined, 'fresh WDC cannot reuse or publish generation'); return {verified: true, reused: false}; }
  assert(phase === 'first-after-prepare' || phase === 'subsequent-hit', 'prepare receipt must be audited separately');
  hash(generation); hash(object.sha256); unsigned(object.bytes); assert(object.bytes > 0);
  assert.deepEqual(observation, {status: 'hit', generation, object, reused: true, publicationThisCommand: false}, 'exact authenticated generation hit required');
  return {verified: true, reused: true, generation, object};
}
const median = values => { const sorted = [...values].sort((a,b) => a-b); const i = sorted.length >> 1; return sorted.length % 2 ? sorted[i] : (sorted[i-1] + sorted[i])/2; };
const timed = sample => Number.isFinite(sample?.controllableMillis) && sample.controllableMillis > 0 && sample.valid === true && sample.terminalVerified === true;
// Fresh WDC uses separate independent homes. Prepare+first is compared with a
// fresh Go first query in a second paired home. Subsequent reuses only that home.
export function summarizeGeneration(pairs, {acceptance = false} = {}) {
  const requiredPairs = acceptance ? REPETITIONS : 1;
  const tasks = ['fresh-wdc', 'prepare-inclusive-first', 'first-after-prepare', 'subsequent-hit'];
  const homes = pairs.flatMap(p => [p.go?.fresh?.home,p.rust?.fresh?.home,p.go?.first?.home,p.rust?.first?.home]);
  const independent = homes.every(h => typeof h === 'string' && h.length > 0) && new Set(homes).size === requiredPairs * 4;
  const correct = pairs.length === requiredPairs && independent && pairs.every((p,i) => {
    if (p.iteration !== i || p.valid !== true || p.ownerRetired !== true || p.finalIntegrityVerified !== true || p.fixtureVerified !== true || p.goCommit !== GO_COMMIT) return false;
    if (JSON.stringify(p.order) !== JSON.stringify(orderFor(i))) return false;
    if (!timed(p.rust?.prepare) || p.rust.prepare.receiptVerified !== true || p.go?.prepare !== undefined) return false;
    for (const engine of ['go','rust']) {
      const e = p[engine];
      if (e.first.home !== e.subsequent?.home || engine === 'rust' && e.prepare.home !== e.first.home) return false;
      const samples = [e.fresh,e.first,e.subsequent,...(engine === 'rust' ? [e.prepare] : [])];
      if (!samples.every(s => timed(s) && s.binary && s.snapshot && s.binary === e.first.binary && s.snapshot === e.first.snapshot && s.binary === pairs[0][engine].first.binary && s.snapshot === pairs[0][engine].first.snapshot)) return false;
      if (![e.fresh,e.first,e.subsequent].every(s => s.captureVerified === true && s.resultEquivalent === true)) return false;
    }
    if (p.go.first.snapshot !== p.rust.first.snapshot) return false;
    const firstProof = p.rust.first.generationProof, nextProof = p.rust.subsequent.generationProof;
    if (!/^[0-9a-f]{64}$/.test(firstProof?.generation ?? '') || !/^[0-9a-f]{64}$/.test(firstProof?.object?.sha256 ?? '') || !Number.isSafeInteger(firstProof?.object?.bytes) || firstProof.object.bytes <= 0) return false;
    if (nextProof?.generation !== firstProof.generation || nextProof?.object?.sha256 !== firstProof.object.sha256 || nextProof?.object?.bytes !== firstProof.object.bytes) return false;
    return ['fresh','first','subsequent'].every(phase => p.rust[phase].generationProof?.verified === true && p.rust[phase].generationProof.reused === (phase !== 'fresh'));
  });
  return tasks.map(task => {
    const field = {'fresh-wdc':'fresh','first-after-prepare':'first','subsequent-hit':'subsequent'}[task];
    const samples = pairs.map(p => task === 'prepare-inclusive-first'
      ? {go:p.go?.first?.controllableMillis,rust:p.rust?.prepare?.controllableMillis+p.rust?.first?.controllableMillis}
      : {go:p.go?.[field]?.controllableMillis,rust:p.rust?.[field]?.controllableMillis});
    const validTimes = samples.length > 0 && samples.every(p => ['go','rust'].every(e => Number.isFinite(p[e]) && p[e] > 0));
    const timings = validTimes ? Object.fromEntries(['go','rust'].map(e => [e,{median:median(samples.map(s => s[e])),samples:samples.map(s => s[e])}])) : null;
    const ratio = timings ? timings.rust.median/timings.go.median : null;
    const evaluated = acceptance && task !== 'first-after-prepare';
    return {task, requiredPairs, attemptedPairs:pairs.length, correctAndComplete:correct, timings, targetRatio:TARGET_RATIO, medianRatio:ratio, performanceAcceptanceEvaluated:evaluated,
      qualification:task === 'first-after-prepare' ? 'conditional prepared-cache diagnostic only; excludes preparation and is not cold acceptance' : 'full controllable elapsed including process ownership and output; no subtraction',
      state:!pairs.length?'not_run':!correct?'failed_correctness_or_missing_samples':!evaluated?'equivalent_observation_only':ratio !== null && ratio <= TARGET_RATIO?'passed_selected_path':'failed_performance'};
  });
}
export const PROOF = Object.freeze({encodingFramingEKey: true, encodingRequestedPagesMD5: true, encodingWholeDecodedCKey: false, rootWholeDecodedCKey: true, contentWholeDecodedCKey: true});
const usageKeys = ['decodeWork', 'metadataBytes', 'networkBytes', 'networkRequests', 'retainedBytes'];
function unsignedCounter(value, label) { assert(Number.isSafeInteger(value) && value >= 0, label); }
export function usage(value) {
  assert.deepEqual(Object.keys(value).sort(), usageKeys, 'resource fields');
  for (const key of usageKeys) unsignedCounter(value[key], key);
  assert.equal(value.networkBytes, 0); assert.equal(value.networkRequests, 0);
  assert(value.decodeWork <= 100000000, 'unchanged work limit');
  return value;
}

export function auditCache(stats, cacheMode, expected) {
  const keys = ['hits', 'reusedBytes', 'contentHashCalls', 'contentHashBytes', 'contentDecodeCalls', 'extractedBytes', 'blobReads', 'blobReadBytes', 'fragmentLoads', 'blockCacheHits'];
  assert.deepEqual(Object.keys(stats).sort(), keys.sort(), 'complete cache counters required');
  for (const key of keys) unsignedCounter(stats[key], `cache.${key}`);
  assert.equal(stats.contentHashCalls, 2, 'Root and payload remain hashed'); assert(stats.contentHashBytes > 0);
  if (cacheMode === 'warm') { assert.equal(stats.hits, 2); assert.equal(stats.contentDecodeCalls, 0); assert(stats.reusedBytes > 0); }
  else { assert.equal(cacheMode, 'cold'); assert.equal(stats.hits, 0); assert.equal(stats.contentDecodeCalls, 2); }
  if (expected) {
    unsignedCounter(expected.encodedBytes, 'expected encoded bytes'); unsignedCounter(expected.decodedBytes, 'expected decoded bytes');
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


export function semanticGeneration(entry, envelope, engine, cacheMode) {
  assert.equal(entry.kind, 'sql');
  assert.equal(envelope.schema, 'lycheedev.result.v1'); assert.equal(envelope.ok, true); assert.equal(envelope.error, null);
  assert(Array.isArray(envelope.warnings)); assert(Array.isArray(envelope.captures));
  const copy = structuredClone(envelope), result = copy.result, observations = {};
  const file = result.sources[0]?.file;
  assert(file && file.contentVerified === true && file.source === 'installation');
  observations.cache = structuredClone(file.decodedCache); observations.reuse = auditCache(file.decodedCache, cacheMode);
  file.decodedCache = '<audited-cache-counters>';
  if (engine === 'rust') {
    assert.deepEqual(file.verification, PROOF); observations.verification = file.verification; delete file.verification;
    assert.deepEqual(Object.keys(file.encodingIO).sort(), ['bytes', 'reads', 'wholeEncoded']);
    unsignedCounter(file.encodingIO.bytes, 'encoding bytes'); unsignedCounter(file.encodingIO.reads, 'encoding reads'); assert.equal(file.encodingIO.wholeEncoded, false);
    observations.encodingIO = file.encodingIO; delete file.encodingIO;
  } else { assert.equal(engine, 'go'); assert(!Object.hasOwn(file, 'verification') && !Object.hasOwn(file, 'encodingIO')); }
  if (entry.kind === 'sql') {
    assert.equal(result.complete, true); assert.equal(result.partial, false); assert.equal(result.sources.length, 1);
    assert.deepEqual(result.result.columns, entry.columns, 'all ordered columns');
    assert.deepEqual(result.result.rows, entry.rows); assert.equal(result.query.sql, entry.sql);
    observations.generations = result.sources.map(source => {
      assert.equal(source.complete, true); assert.equal(source.truncated, false);
      const proof = auditGenerationProof(source.typedGenerationCache, {phase: engine === 'go' ? 'fresh-wdc' : entry.phase, generation: entry.generation, object: entry.object});
      if (source.typedGenerationCache) delete source.typedGenerationCache;
      return proof;
    });
    observations.resources = usage(result.resources); result.resources = '<audited-resource-usage>';
    const memory = result.memoryBudget;
    assert.deepEqual(Object.keys(memory).sort(), ['mode', 'concurrentQueries', 'ceilingBytes', 'retainedBytes', 'metadataBytes', 'peakScratchBytes', 'growths'].sort());
    assert.equal(memory.mode, 'windows_adaptive');
    for (const key of Object.keys(memory).filter(key => key !== 'mode')) unsignedCounter(memory[key], `memory.${key}`);
    assert(memory.concurrentQueries >= 1); assert(memory.ceilingBytes >= 512 << 20, 'require unconstrained initial budget for selected tiny SQL input');
    assert.equal(memory.retainedBytes, 512 << 20); assert.equal(memory.metadataBytes, 128 << 20); assert.equal(memory.growths, 0);
    assert(observations.resources.retainedBytes + memory.peakScratchBytes <= memory.retainedBytes);
    assert(observations.resources.metadataBytes <= memory.metadataBytes);
    observations.memoryBudget = structuredClone(memory); memory.ceilingBytes = '<observed-available-memory>'; memory.peakScratchBytes = '<audited-peak-scratch>';
    assert.deepEqual(Object.keys(result.timings).sort(), ['decodedSourceBytes', 'definitionsMs', 'executionMs', 'preparationMs']);
    for (const key of ['definitionsMs', 'executionMs', 'preparationMs']) { assert(Number.isFinite(result.timings[key]) && result.timings[key] >= 0); }
    unsignedCounter(result.timings.decodedSourceBytes, 'decoded source bytes'); observations.timings = structuredClone(result.timings);
    for (const key of ['definitionsMs', 'executionMs', 'preparationMs']) result.timings[key] = '<observed-time>';
    if (engine === 'rust') { accounting(result.resourceAccounting, true); observations.accounting = result.resourceAccounting; delete result.resourceAccounting; }
  }
  return {value: copy, observations};
}


// Fresh explicit prepare receipt, not reuse. Publication/body custody and disk
// reservation inventory remain independently authenticated by the runner.
export function auditPrepareEnvelope(envelope, {snapshot, table, rows, definitionSHA256, recordIdColumn = 5}) {
  assert.equal(envelope.schema, 'lycheedev.result.v1'); assert.equal(envelope.ok,true); assert.equal(envelope.error,null);
  assert.deepEqual(envelope.captures,[]); assert.deepEqual(envelope.warnings,[]);
  assert.deepEqual(envelope.context,{snapshot,coverage:{sourceKind:'static',complete:true,availableRows:rows,missing:[]}});
  const p=envelope.result;
  assert.deepEqual(Object.keys(p).sort(), ['generation','generationPublished','sourceKind','snapshot','table','definitionSHA256','typedSchemaSHA256','object','representation','derivationVersion','availableRows','batchCount','complete','missing','unavailablePartitions','recordIdColumn','recordIdOrder','resources','memoryBudget','diskReservation'].sort(), 'exact fresh prepare receipt fields');
  assert.equal(p.generationPublished,true); assert.equal(p.sourceKind,'static'); assert.equal(p.snapshot,snapshot); assert.equal(p.table,table);
  assert.equal(p.availableRows,rows); unsigned(rows); assert(rows>0); assert.equal(p.batchCount,Math.ceil(rows/1024));
  assert.equal(p.complete,true); assert.deepEqual(p.missing,[]); assert.deepEqual(p.unavailablePartitions,[]);
  hash(p.generation); hash(p.typedSchemaSHA256); hash(p.definitionSHA256); assert.equal(p.definitionSHA256,definitionSHA256);
  assert.deepEqual(Object.keys(p.object).sort(),['bytes','sha256']); hash(p.object.sha256); unsigned(p.object.bytes); assert(p.object.bytes>0);
  assert.equal(p.recordIdColumn,recordIdColumn); assert.equal(p.recordIdOrder,'ascending');
  assert.equal(p.representation,'arrow-ipc-file-v5/arrow-58.3.0/uncompressed'); assert.equal(p.derivationVersion,'lycheedev.table-arrow-import.v1');
  usage(p.resources);
  const m=p.memoryBudget;
  assert.deepEqual(Object.keys(m).sort(),['mode','concurrentQueries','ceilingBytes','retainedBytes','metadataBytes','peakScratchBytes','growths'].sort());
  assert.equal(m.mode,'windows_adaptive'); for(const [key,value] of Object.entries(m))if(key!=='mode')unsigned(value);
  assert(m.concurrentQueries>=1); assert(m.ceilingBytes>=512<<20); assert.equal(m.retainedBytes,512<<20); assert.equal(m.metadataBytes,128<<20); assert.equal(m.growths,0);
  assert(p.resources.retainedBytes+m.peakScratchBytes<=m.retainedBytes); assert(p.resources.metadataBytes<=m.metadataBytes);
  auditDiskReservation(p.diskReservation);
  return {receiptVerified:true,generation:p.generation,object:structuredClone(p.object),resources:structuredClone(p.resources),memoryBudget:structuredClone(m),diskReservation:structuredClone(p.diskReservation)};
}

// Pure record validation. Caller still hashes the actual EXE/PDB, source tree,
// artifact record and pinned upstream bytes under existing controlled readers.
export function verifyGenerationArtifact(artifact, {sourceRoot, sourceTree, binarySHA256, emmy, channel}) {
  assert(['lycheedev.rust-build-artifact.v1','lycheedev.optimized-symbol-artifact.v1'].includes(artifact.schema));
  assert(['release','symbols'].includes(artifact.profile)); assert.equal(artifact.sameInvocation,true);
  assert.equal(artifact.sourceRoot,sourceRoot); assert(typeof sourceRoot==='string' && sourceRoot.length>0);
  hash(sourceTree); hash(binarySHA256); hash(artifact.binarySHA256); hash(artifact.environmentSHA256);
  assert.equal(artifact.binarySHA256,binarySHA256); unsigned(artifact.bytes); assert(artifact.bytes>0 && artifact.bytes<=128<<20);
  assert(typeof artifact.sourceBinary==='string' && artifact.sourceBinary.length>0);
  assert(typeof artifact.buildEvidence==='string' && artifact.buildEvidence.length>0);
  assert(typeof artifact.channel==='string' && artifact.channel.length>0);
  assert.deepEqual(artifact.command,['cargo','build','--locked','--offline','-j','1','--profile',artifact.profile,'-p','lycheedev-cli']);
  assert.equal(artifact.inputHashes?.sourceTree,sourceTree);
  const recorded=artifact.inputHashes.emmy;
  assert.equal(recorded?.schema,'lycheedev.emmy-reconstruction.v1'); assert.equal(recorded.historicalPrivatePatchRecovered,false);
  for(const field of ['identitySHA256','recipeSHA256','patchedTreeSHA256'])hash(recorded[field]);
  assert.equal(artifact.inputHashes.emmyPatch,recorded.patchedTreeSHA256);
  if(artifact.profile==='symbols'){assert(typeof artifact.pdb==='string' && artifact.pdb.length>0);hash(artifact.pdbSHA256);}
  if(emmy){assert.equal(emmy.identity,'public-upstream-unmodified-v1');assert.equal(emmy.schema,recorded.schema);assert.equal(emmy.historicalPrivatePatchRecovered,false);for(const field of ['identitySHA256','recipeSHA256','patchedTreeSHA256'])assert.equal(recorded[field],emmy[field]);}
  if(channel){assert.equal(channel.version,'3.5.0');assert.equal(channel.distTag,'beta');}
  return true;
}

export const DISK_COVERAGE = Object.freeze({totalDataCoverageComplete:false,accountedScope:'versioned-derived-roots-private-metadata-registered-shared-blobs-and-pending-peaks',notYetAttributed:[
  'historical/unregistered Data shared blobs and writers not using shared-blob admission',
  'historical shared metadata database, sidecars and legacy raw-source references',
  'legacy Data staging/spill paths outside tmp/data-derived-v1',
  'export temporaries outside tmp/data-derived-v1/exports',
],automaticEvictionEnabled:false});
export function auditDiskReservation(r) {
  assert.deepEqual(Object.keys(r).sort(),['configuredMaxBytes','existing','additionalPeak','sharedBlobAdditionalPeak','privateMetadataAdditionalPeak','admittedPeakBytes','coverage'].sort());
  assert.equal(r.configuredMaxBytes,5000000000,'unchanged decimal 5GB Data quota');
  assert.deepEqual(Object.keys(r.existing).sort(),['objectNamespaceBytes','generationNamespaceBytes','temporaryNamespaceBytes','privateMetadataNamespaceBytes','sharedBlobBytes','retainedSharedPeakBytes','files'].sort());
  assert.deepEqual(Object.keys(r.additionalPeak).sort(),['objects','generations','staging','spill','exportTemporary'].sort());
  for(const value of [...Object.values(r.existing),...Object.values(r.additionalPeak),r.sharedBlobAdditionalPeak,r.privateMetadataAdditionalPeak,r.admittedPeakBytes])unsigned(value);
  assert.deepEqual(r.coverage,DISK_COVERAGE,'honest partial disk accounting');
  const total = Object.entries(r.existing).filter(([key])=>key!=='files').reduce((n,[,v])=>n+BigInt(v),0n)+Object.values(r.additionalPeak).reduce((n,v)=>n+BigInt(v),0n)+BigInt(r.sharedBlobAdditionalPeak)+BigInt(r.privateMetadataAdditionalPeak);
  assert.equal(BigInt(r.admittedPeakBytes),total,'all existing and simultaneous new peak bytes included');
  assert(r.admittedPeakBytes<=r.configuredMaxBytes,'admitted peak exceeds fixed quota');
  return true;
}
