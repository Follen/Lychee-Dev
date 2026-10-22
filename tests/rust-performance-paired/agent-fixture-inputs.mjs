import {preparationEnvironmentPolicy} from './agent-environment.mjs';
// Bounded readers for the existing synthetic fixture families only.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {dirname, isAbsolute, join, relative, resolve} from 'node:path';
import {createHash} from 'node:crypto';
import {inflateSync} from 'node:zlib';
import {DatabaseSync} from 'node:sqlite';
import {hashFile} from '../../tools/rust-build.mjs';
import {inventory, safeRelative} from './inputs.mjs';
import {authenticate} from './agent-custody.mjs';
import {CONTENT_BYTES, SQL_ROWS, auditCache, semanticData} from './data-contract.mjs';
export const digest = (bytes, algorithm = 'sha256') => createHash(algorithm).update(bytes).digest('hex');
import {parse} from './agent-json.mjs';
export {parse};
const ordered = (object, keys) => Object.fromEntries(keys.filter(key => Object.hasOwn(object, key)).map(key => [key, object[key]]));
export const goJSON = value => JSON.stringify(value).replace(/[<>&\u2028\u2029]/g, char => '\\u' + char.charCodeAt(0).toString(16).padStart(4, '0'));
export function bounded(path, maximum = 64 << 20) {
  const stat = lstatSync(path); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, 'bounded regular fixture file');
  const bytes = readFileSync(path); assert.equal(bytes.length, stat.size); return bytes;
}
function contained(base, path) { const suffix = relative(resolve(base), resolve(path)); assert(suffix && !suffix.startsWith('..') && !isAbsolute(suffix), 'fixture path outside root'); return path; }
const blobPath = (home, ref) => { assert.match(ref.sha256, /^[0-9a-f]{64}$/); return join(home, 'blobs', ref.sha256.slice(0, 2), ref.sha256.slice(2)); };
export function blob(home, ref) {
  assert(Number.isSafeInteger(ref.bytes) && ref.bytes >= 0 && ref.bytes <= 64 << 20);
  const bytes = bounded(blobPath(home, ref)); assert.equal(bytes.length, ref.bytes); assert.equal(digest(bytes), ref.sha256); return bytes;
}

export function decodeFixtureBLTE(frame) {
  assert(frame.length >= 9 && frame.subarray(0, 4).toString() === 'BLTE');
  const header = frame.readUInt32BE(4);
  if (!header) { assert.equal(frame[8], 78); return {raw: frame.subarray(9), ekey: digest(frame, 'md5'), chunks: 1}; }
  assert(header >= 12 && header <= frame.length && frame[8] === 15);
  const count = frame.readUIntBE(9, 3); assert(count > 0 && count <= 64 && header === 12 + count * 24);
  let offset = header, total = 0; const blocks = [];
  for (let index = 0; index < count; index++) {
    const at = 12 + index * 24, encoded = frame.readUInt32BE(at), decoded = frame.readUInt32BE(at + 4);
    assert(encoded > 0 && decoded <= 8 << 20 && offset + encoded <= frame.length && (total += decoded) <= 64 << 20);
    const chunk = frame.subarray(offset, offset + encoded); offset += encoded;
    assert.equal(digest(chunk, 'md5'), frame.subarray(at + 8, at + 24).toString('hex'));
    assert(chunk[0] === 78 || chunk[0] === 90);
    const raw = chunk[0] === 78 ? chunk.subarray(1) : inflateSync(chunk.subarray(1), {maxOutputLength: decoded + 1});
    assert.equal(raw.length, decoded); blocks.push(raw);
  }
  assert.equal(offset, frame.length);
  return {raw: Buffer.concat(blocks, total), ekey: digest(frame.subarray(0, header), 'md5'), chunks: count};
}

export function loadAgentDataFixture(directory, kind) {
  directory = resolve(directory);
  const before = inventory(directory, {maxFiles: 256, maxBytes: 64 << 20});
  const descriptor = bounded(join(directory, 'input-identity.json'), 1 << 20), identity = parse(descriptor);
  assert.equal(identity.schema, 'lycheedev.catalog-navigation-fixture.v1');
  assert.equal(identity.scope, 'synthetic authenticated local input; not real-client evidence');
  contained(directory, identity.home); contained(join(directory, 'game'), identity.installation);
  assert.deepEqual(ordered(identity.pin, ['product', 'region', 'fullBuild', 'language', 'definitionCommit']), {product: 'retail', region: 'cn', fullBuild: '12.1.0.69933', language: 'zhCN', definitionCommit: 'b'.repeat(40)});
  assert.deepEqual(readdirSync(join(identity.home, 'state')).sort(), ['data','toolkit.sqlite'], 'current private Data fixture required');
  const database = new DatabaseSync(join(identity.home, 'state/toolkit.sqlite'), {readOnly: true}), documents = [], blobs = [];
  try {
    const pins=database.prepare('SELECT key,generation,value FROM documents ORDER BY key').all();assert.equal(pins.length,1);
    const cache=new DatabaseSync(join(identity.home,'state/data/cache-v1.sqlite'),{readOnly:true});let data;try{data=cache.prepare('SELECT key,generation,value FROM documents ORDER BY key').all();}finally{cache.close();}assert(data.every(row=>row.key.startsWith('definition/')));const rows=[...pins,...data].sort((a,b)=>a.key.localeCompare(b.key));
    assert.equal(rows.length, identity.tables.length + 2, 'pin and exact definitions only; no decoded/capture state');
    const pin = rows.find(row => row.key === `pin/${identity.snapshot}`); assert(pin);
    const value = parse(Buffer.from(pin.value)), canonical = {schema: value.schema, id: '', data: ordered(value.data, ['product', 'region', 'fullBuild', 'buildConfig', 'cdnConfig', 'language', 'definitionCommit'])};
    assert.equal(value.schema, 'lycheedev.selection.v1'); assert.deepEqual(value.data, identity.pin);
    assert.deepEqual({...value, id: ''}, canonical); assert.equal(identity.snapshot, 'PIN-' + digest(goJSON(canonical))); assert.equal(value.id, identity.snapshot);
    const base = `https://raw.githubusercontent.com/wowdev/WoWDBDefs/${identity.pin.definitionCommit}/`;
    const expected = new Map(['manifest.json', ...identity.tables.map(table => `definitions/${table.name}.dbd`)].map(path => ['definition/' + digest(base + path), base + path]));
    for (const row of rows) {
      assert.equal(row.generation, 1); const bytes = Buffer.from(row.value); assert(bytes.length <= 1 << 20);
      documents.push({key: row.key, bytes, sha256: digest(bytes)});
      if (row === pin) continue;
      assert(expected.has(row.key), 'unexpected seed document'); const value = parse(bytes); assert.equal(value.url, expected.get(row.key));
      const raw = blob(identity.home, value.blob); blobs.push({...value.blob, raw});
      const table = identity.tables.find(table => value.url.endsWith(`/definitions/${table.name}.dbd`));
      if (table) assert.equal(value.blob.sha256, table.dbdSHA256);
      else assert.deepEqual(parse(raw).map(row => [row.tableName, row.db2FileDataID]), identity.tables.map(row => [row.name, row.fileDataID]));
    }
    const deleted = database.prepare("SELECT name FROM sqlite_master WHERE name='deleted_generations'").get();
    if (deleted) assert.equal(database.prepare('SELECT count(*) AS count FROM deleted_generations').get().count, 0);
  } finally { database.close(); }
  const archive = bounded(join(directory, 'game/Data/data/data.000')), index = bounded(join(directory, 'game/Data/data/0000000001.idx'));
  assert(index.length >= 40 && (index.length - 40) % 18 === 0);
  const payloads = new Map();
  for (let at = 40; at < index.length; at += 18) {
    assert.equal(index[at + 9], 0); const offset = index.readUInt32BE(at + 10), bytes = index.readUInt32LE(at + 14);
    assert(bytes >= 39 && offset + bytes <= archive.length);
    const decoded = decodeFixtureBLTE(archive.subarray(offset + 30, offset + bytes));
    assert.equal(index.subarray(at, at + 9).toString('hex'), decoded.ekey.slice(0, 18));
    payloads.set(digest(decoded.raw), {...decoded, encodedBytes: bytes - 30});
  }
  for (const table of identity.tables) { const body = payloads.get(table.sha256); assert(body && body.raw.length === table.bytes, 'authenticated selected bytes'); }
  let images;
  if (kind === 'content') {
    const content = identity.content, body = payloads.get(content?.sha256);
    assert.equal(content.fileDataID, 501); assert.equal(content.decodedBytes, CONTENT_BYTES); assert.equal(content.chunks, 3); assert.equal(content.fillByte, 120);
    assert.equal(body.chunks, 3); assert.equal(body.encodedBytes, content.encodedBytes); assert.equal(body.ekey, content.encodingKey);
    assert.equal(digest(body.raw, 'md5'), content.contentKey); assert(body.raw.every(byte => byte === 120));
  } else if (kind === 'images') {
    images = parse(bounded(join(directory, 'image-cases.json'), 1 << 20));
    for (const test of images.cases) {
      safeRelative(test.pixelsPath); const raw = bounded(join(directory, test.pixelsPath), 4 << 20);
      assert.equal(raw.length, test.pixelsBytes); assert.equal(digest(raw), test.pixelsSHA256);
    }
  } else if (kind === 'csv-numeric') { auditNumericCSVFixture(identity, payloads, blobs);
  } else { assert.equal(kind, 'catalog'); assert.deepEqual(identity.tables.map(table => table.name), ['Item', 'ItemSparse', 'HouseDecor']); }
  assert.equal(inventory(directory, {maxFiles: 256, maxBytes: 64 << 20}).sha256, before.sha256);
  const objects = Object.fromEntries([...payloads].map(([sha256, body]) => [sha256, {decodedBytes: body.raw.length, encodedBytes: body.encodedBytes, ckey: digest(body.raw, 'md5'), ekey: body.ekey}]));
  return {directory, identity, images, documents, blobs, objects, receipt: {kind, descriptorSHA256: digest(descriptor), inventory: before, snapshot: identity.snapshot, pin: identity.pin, objects}};
}

// Each engine first runs its own real init. Import only the exact synthetic pin
// and authenticated definition blobs; never import caches or another DB image.
export function seedDataHome(fixture, home) {
  assert(existsSync(join(home, 'workspace.json')));
  const database = new DatabaseSync(join(home, 'state/toolkit.sqlite'));
  try {
    database.exec('PRAGMA synchronous=FULL;BEGIN IMMEDIATE;CREATE TABLE IF NOT EXISTS documents(key TEXT PRIMARY KEY NOT NULL,generation INTEGER NOT NULL CHECK(generation>0),value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID;CREATE TABLE IF NOT EXISTS deleted_generations(key TEXT PRIMARY KEY NOT NULL,generation INTEGER NOT NULL) WITHOUT ROWID;PRAGMA user_version=1;');
    assert.equal(database.prepare('SELECT count(*) AS count FROM documents').get().count, 0);
    const insert = database.prepare('INSERT INTO documents(key,generation,value) VALUES(?,1,?)');
    for (const doc of fixture.documents) insert.run(doc.key, doc.bytes);
    database.exec('COMMIT');
  } finally { database.close(); }
  for (const item of fixture.blobs) { const path = blobPath(home, item); mkdirSync(dirname(path), {recursive: true}); if (existsSync(path)) assert.equal(hashFile(path), item.sha256); else writeFileSync(path, item.raw, {flag: 'wx'}); }
  assert.deepEqual(readdirSync(join(home, 'state')).sort(), ['toolkit.sqlite']);
  return inventory(home);
}

export function decodedIdentity(home) {
  const database = new DatabaseSync(join(home, 'state/toolkit.sqlite'), {readOnly: true});
  try {
    return database.prepare("SELECT key,value FROM documents WHERE key LIKE 'decoded-v1/%' ORDER BY key").all().map(row => {
      const value = parse(Buffer.from(row.value)); blob(home, value.blob); return {key: row.key, value};
    });
  } finally { database.close(); }
}

export function captureCanonical(capture) {
  return {schema: capture.schema, id: '', blob: ordered(capture.blob, ['sha256', 'bytes']), mediaType: capture.mediaType,
    provenance: ordered(capture.provenance, ['kind', 'locator', 'snapshot', 'operationId', 'sourceCommit', 'baseSnapshot', 'baseSourceCommit', 'dataBuild', 'session']), complete: capture.complete, truncated: capture.truncated, capturedAt: capture.capturedAt,
    ...(Object.hasOwn(capture, 'dependencies') ? {dependencies: capture.dependencies} : {})};
}

// Only authenticated, bounded encoded bytes reach libvips. Passing a scratch
// filename can let its operation cache retain an open file after toBuffer().
// The caller disables that cache; destroy always follows the completed decode.
export async function decodeDataImage(sharp, bytes) {
  assert(Buffer.isBuffer(bytes) && bytes.length <= 64 << 20, 'bounded encoded image Buffer required');
  const decoder = sharp(bytes, {limitInputPixels: 1031 * 519});
  let failure;
  try { return await decoder.ensureAlpha().raw().toBuffer({resolveWithObject: true}); }
  catch (error) { failure = error; throw error; }
  finally {
    try { decoder.destroy(); }
    catch (error) { if (failure) throw new AggregateError([failure, error], failure.message, {cause: failure}); throw error; }
  }
}

export async function validateDataOutput({entry, fixture, engine, cacheMode, envelope, home, output, decodeImage, evidence,readBlob=ref=>blob(home,ref),metadata,readOutput=path=>bounded(path)}) {
  const audited = semanticData(entry, envelope, engine, cacheMode), value = envelope.result;
  assert.equal(envelope.captures.length, entry.kind === 'image' ? 3 : 1);
  assert.equal(envelope.context.snapshot, fixture.identity.snapshot);
  const file = entry.kind === 'sql' ? value.sources[0].file : entry.kind === 'image' ? value.source : value;
  assert.deepEqual(file.pin, fixture.identity.pin);
  const table = fixture.identity.tables.find(row => row.fileDataID === (entry.fileDataID ?? 403)); assert(table);
  assert.deepEqual(file.content, {sha256: table.sha256, bytes: table.bytes});
  readBlob(file.content);
  const rootObject = fixture.objects[file.root?.sha256], payloadObject = fixture.objects[file.content.sha256];
  assert(rootObject && payloadObject, 'Root and selected object must belong to authenticated fixture');
  assert.equal(file.root.bytes, rootObject.decodedBytes); assert.equal(file.payloadEncodingKey, payloadObject.ekey); assert.equal(file.entry.contentKey, payloadObject.ckey);
  readBlob(file.root);
  audited.observations.reuse = auditCache(file.decodedCache, cacheMode, {encodedBytes: rootObject.encodedBytes + payloadObject.encodedBytes, decodedBytes: rootObject.decodedBytes + payloadObject.decodedBytes});
  const proofs = [], captured = [];
  const database = metadata?null:new DatabaseSync(join(home,engine==='go'?'state/toolkit.sqlite':'state/data/evidence-v2.sqlite'),{readOnly:true});
  try {
    for (const [index, capture] of envelope.captures.entries()) {
      const canonical = captureCanonical(capture);
      assert.deepEqual({...capture, id: ''}, canonical); assert.equal(capture.id, 'CAP-' + digest(goJSON(canonical)));
      const dependencies = entry.kind === 'image' && index > 0 ? index === 1 ? [envelope.captures[0].id, envelope.captures[2].id] : [envelope.captures[0].id] : undefined;
      assert.deepEqual(capture.dependencies, dependencies, 'exact ordered Go323 capture ownership edges');
      assert(Number.isFinite(Date.parse(capture.capturedAt))); assert.equal(capture.complete, true); assert.equal(capture.truncated, false);
      assert.equal(capture.provenance.snapshot, fixture.identity.snapshot); assert.equal(capture.provenance.dataBuild, fixture.identity.pin.fullBuild);
      const stored=metadata?metadata(capture.id):parse(Buffer.from(database.prepare('SELECT value FROM documents WHERE key=?').get('capture/'+capture.id).value));assert.deepEqual(stored,capture);
      const bytes = readBlob(capture.blob); captured.push(bytes);
      // Preserve complete manifest/result blobs even when successful homes are
      // reclaimed. Large content is fixed by immutable fixture and exact hash.
      if (evidence && capture.mediaType === 'application/json') writeFileSync(join(evidence, `${engine}-${cacheMode}-capture-${index}.json`), bytes, {flag: 'wx'});
      proofs.push({id: capture.id, blob: capture.blob, identityVerified: true, metadataVerified: true, completeBytesVerified: true});
    }
  } finally { database?.close(); }
  if (entry.kind === 'sql') {
    assert.deepEqual(value.result.rows, SQL_ROWS); assert.equal(envelope.captures[0].provenance.kind, 'data-query');
    assert.equal(envelope.captures[0].mediaType, 'application/json'); assert.deepEqual(parse(captured[0]), value);
  } else {
    assert.deepEqual(envelope.captures[0].blob, file.content); assert.equal(envelope.captures[0].mediaType, 'application/octet-stream');
    assert.equal(envelope.captures[0].provenance.kind, 'asset');
    assert.equal(envelope.captures[0].provenance.locator, `casc:fdid:${entry.fileDataID}:ckey:${file.entry.contentKey}`);
    if (entry.kind === 'image') {
      assert.equal(value.output, output); assert.equal(value.encoding, entry.encoding); assert.equal(value.image.channels, 'rgba');
      assert.deepEqual(value.image.texture, entry.image.texture); assert.equal(value.image.pixelsSHA256, entry.image.pixelsSHA256);
      assert.deepEqual(value.sourceCapture, envelope.captures[0]); assert.deepEqual(value.artifactCapture, envelope.captures[2]);
      assert.deepEqual(value.content, envelope.captures[2].blob); assert(readOutput(output).equals(captured[2]), 'complete artifact bytes differ');
      assert.equal(envelope.captures[1].mediaType, 'application/json'); assert.deepEqual(parse(captured[1]), value);
      assert.equal(envelope.captures[2].mediaType, 'image/' + entry.encoding);
      for (const index of [1, 2]) { assert.equal(envelope.captures[index].provenance.kind, index === 1 ? 'asset-export-manifest' : 'asset-image'); assert.equal(envelope.captures[index].provenance.locator, envelope.captures[0].id); }
      const image = await decodeImage(captured[2]);
      assert.equal(image.info.width, 1031); assert.equal(image.info.height, 519); assert.equal(image.info.channels, 4);
      const expected = bounded(join(fixture.directory, entry.image.pixelsPath), 4 << 20); assert(Buffer.isBuffer(image.data) && image.data.equals(expected), 'complete RGBA bytes differ'); assert.equal(digest(image.data), value.image.pixelsSHA256);
      assert.equal(digest(captured[2]), value.content.sha256, 'encoded buffer changed during independent decode');
      if(evidence)writeFileSync(join(evidence, `${engine}-${cacheMode}.${entry.encoding}`), captured[2], {flag: 'wx'});
      audited.value.result.output = '<verified-output>'; audited.value.result.content = {sha256: '<verified-codec-artifact>', bytes: '<verified-codec-bytes>'};
    }
  }
  const normalized = new Map(envelope.captures.map((capture, index) => {
    const result = structuredClone(capture); result.id = `<verified-capture-${index}>`; result.capturedAt = '<verified-time>';
    if (index > 0) result.provenance.locator = '<verified-capture-0>';
    if (capture.dependencies) result.dependencies = capture.dependencies.map(id => `<verified-capture-${envelope.captures.findIndex(capture => capture.id === id)}>`);
    if (entry.kind === 'sql' || index > 0) result.blob = {sha256: index === 2 ? '<verified-codec-artifact>' : '<verified-manifest>', bytes: index === 2 ? '<verified-codec-bytes>' : '<verified-manifest-bytes>'};
    return [capture.id, result];
  }));
  audited.value.captures = envelope.captures.map(capture => normalized.get(capture.id));
  if (entry.kind === 'image') { audited.value.result.sourceCapture = normalized.get(value.sourceCapture.id); audited.value.result.artifactCapture = normalized.get(value.artifactCapture.id); }
  return {...audited, proofs};
}

// Independent optional NumericSample CSV oracle; the existing Rust exporter
// constructs the actual CASC/WDC fixture. Default B04 readers are unchanged.
export const CSV_ROWS = 16384;
export const CSV_COLUMNS = Object.freeze(['ID', 'F0', 'F1', 'F2', 'F3']);
export const CSV_SQL = 'SELECT ID, F0, F1, F2, F3 FROM NumericSample ORDER BY ID';
export const CSV_SHA256 = '83abe748a19c1f63626cc5702843ecd346a98c9b77d03ba6b3960125750d05ac';
export const WDC_SHA256 = '3b914d5c1dbe818a91aae00bf37c32489c0c6f1ffeb5ee16c6e2b8cfbc45d68c';
export const DBD_SHA256 = '3246db1f30be88aa590c0c2c5ec8c89d832764ab01fe8450f647bad9e4da6450';
export const CSV_BYTES = 523753;
export const WDC_BYTES = 327933;

export function numericRow(i) { assert(Number.isInteger(i) && i >= 0 && i < CSV_ROWS); return [i + 1, i + 1.5, -i - 1.5, 0.25, -0.25]; }
export function expectedNumericCSV() {
  // Decimal strings are independent of product float formatting. Every chosen
  // fraction is exactly representable in f32 and has this unique CSV spelling.
  const rows = ['ID,F0,F1,F2,F3\n'];
  for (let i = 0; i < CSV_ROWS; i++) rows.push(`${i + 1},${i + 1}.5,-${i + 1}.5,0.25,-0.25\n`);
  const bytes = Buffer.from(rows.join(''));
  assert.equal(bytes.length, CSV_BYTES); assert.equal(digest(bytes), CSV_SHA256); return bytes;
}
function validateNumericABI(abi) {
  assert.deepEqual(Object.keys(abi ?? {}).sort(), ['stringNumberLayoutBytes', 'usizeNumberLayoutBytes']);
  for (const bytes of Object.values(abi)) assert(Number.isSafeInteger(bytes) && bytes >= 16 && bytes <= 256 && bytes % 8 === 0, 'bounded Windows x64 tuple ABI');
  assert.equal(abi.stringNumberLayoutBytes - abi.usizeNumberLayoutBytes, 16, 'Windows x64 String versus usize tuple');
}
export function auditNumericCSVFixture(identity, payloads, blobs) {
  validateNumericABI(identity.numericCsvABI);
  assert.deepEqual(identity.tables, [{name: 'NumericSample', fileDataID: 404, layout: 0xabcd0194, bytes: WDC_BYTES, sha256: WDC_SHA256, dbdSHA256: DBD_SHA256}]);
  const body = payloads.get(WDC_SHA256); assert(body, 'numeric WDC object missing');
  const raw = body.raw; assert.equal(raw.length, WDC_BYTES); assert.equal(digest(raw), WDC_SHA256);
  assert.equal(raw.subarray(0, 4).toString(), 'WDC3');
  for (const [at, value] of [[4, CSV_ROWS], [8, 5], [12, 20], [16, 1], [20, 0xf00d0194], [24, 0xabcd0194], [28, 1], [32, CSV_ROWS], [44, 5], [56, 120], [68, 1], [80, 252], [84, CSV_ROWS], [88, 1]]) assert.equal(raw.readUInt32LE(at), value);
  for (let i = 0; i < CSV_ROWS; i++) {
    const at = 252 + i * 20, expected = numericRow(i);
    assert.equal(raw.readUInt32LE(at), expected[0]);
    for (let column = 1; column < 5; column++) assert.equal(raw.readFloatLE(at + column * 4), expected[column], 'exact f32 fixture cell');
  }
  const definition = blobs.find(ref => ref.sha256 === DBD_SHA256); assert(definition);
  assert.equal(definition.raw.toString(), 'COLUMNS\nint ID\nfloat F0\nfloat F1\nfloat F2\nfloat F3\n\nBUILD 12.1.0.69933\nLAYOUT ABCD0194\n$id$ID<u32>\nF0<32>\nF1<32>\nF2<32>\nF3<32>\n');
}

// Explicit current fixture producers, shared by invocation and receipt replay.
export const fixturePreparers = Object.freeze([
  {kind: 'catalog', target: 'agent_fixture_prepare', test: 'prepare_catalog_agent_fixture', variable: 'LYCHEEDEV_CATALOG_FIXTURE'},
  {kind: 'content', target: 'content_budget', test: 'prepare_compressed_content_public_fixture', variable: 'LYCHEEDEV_CONTENT_BUDGET_FIXTURE'},
  {kind: 'images', target: 'image_export_business', test: 'prepare_large_image_export_public_fixture', variable: 'LYCHEEDEV_IMAGE_EXPORT_FIXTURE'},
  {kind: 'csv-numeric', target: 'agent_fixture_prepare', test: 'prepare_numeric_agent_fixture', variable: 'LYCHEEDEV_NUMERIC_CSV_FIXTURE'},
]);
export function fixturePreparationRecipe(kind) {
  const entry = fixturePreparers.find(entry => entry.kind === kind);
  assert(entry, `missing current fixture producer for ${kind}; prepare and connect the current independent fixture before measuring`);
  return entry;
}
export const fixtureArgs = entry => ['test', '--locked', '--offline', '-j', '1', '-p', 'lycheedev-data', '--test', entry.target, entry.test, '--', '--ignored', '--exact', '--nocapture', '--test-threads=1'];

export function loadOwnedAgentFixture(spec,kind){
 const args=fixtureArgs(fixturePreparationRecipe(kind));
 assert.deepEqual(Object.keys(spec).sort(),['directory','preparation','preparationSHA256']);const raw=bounded(spec.preparation,4<<20);assert.equal(digest(raw),spec.preparationSHA256);const report=parse(raw);assert.equal(report.schema,'lycheedev.paired-agent-inputs.recovered.v1');assert.equal(report.inputs.environmentPolicy,preparationEnvironmentPolicy);assert.deepEqual(report.inputs.environmentOverrides,[]);assert.equal(report.state,'prepared_synthetic_inputs');assert.equal(report.handlesReaped,true);const owner=parse(bounded(report.owner,4<<20));assert.equal(owner.schema,'lycheedev.disk-owner.v1');assert.equal(owner.owner,'paired-data-fixtures');assert.equal(owner.retention,'evidence');assert(['succeeded','retired'].includes(owner.state));assert.equal(owner.handlesReaped,true);assert.equal(owner.activePid,null);assert.equal(owner.directory,report.directory);assert.deepEqual(owner.inputHashes,report.inputs);assert.equal(owner.sealedLogs.filter(row=>resolve(row.path)===resolve(spec.preparation)).length,1);assert.equal(owner.sealedLogs.find(row=>resolve(row.path)===resolve(spec.preparation)).sha256,spec.preparationSHA256);
 assert.equal(report.fixtures[kind],spec.directory);const calls=report.calls.filter(row=>row.kind===kind);assert.equal(calls.length,1);const call=calls[0];assert.equal(call.destination,spec.directory);assert(call.launched&&call.process.handlesReaped&&call.process.exit===0&&!call.process.error);assert.equal(digest(bounded(call.buildEvidence,4<<20)),call.cargoReceiptSHA256);assert.deepEqual(parse(bounded(call.buildEvidence,4<<20)),call.cargoReceipt);assert.equal(call.cargoReceipt.handlesReaped,true);assert(['succeeded','retired'].includes(call.cargoReceipt.state));assert.equal(call.cargoReceipt.inputHashes.sourceTree,report.inputs.sourceTree);assert.equal(call.invocation.inputHashes.sourceTree,report.inputs.sourceTree);assert(call.args.includes('--locked')&&call.args.includes('--offline')&&call.args.includes('--test-threads=1'));assert.equal(call.args[call.args.indexOf('-j')+1],'1');const proof=call.process.proof;for(const key of ['rootReaped','treeReaped','handlesClosed','toolIdentityLocksClosed','outputReadersComplete'])assert.equal(proof[key],true);assert.equal(proof.activeProcesses,0);assert(!proof.error&&!proof.deadlineHit&&!proof.outputLimit);
 assert.deepEqual(call.args,args);assert.deepEqual(call.cargoReceipt.command,['cargo',...args]);const evidence=dirname(call.buildEvidence),sealedBuild=name=>{const path=join(evidence,name),log=call.cargoReceipt.sealedLogs.find(l=>l.path===path);assert(log);assert.equal(digest(bounded(path,16<<20)),log.sha256);return path;},command=parse(bounded(sealedBuild('command.json'),4<<20));assert.deepEqual(command.args,args);assert.equal(command.command,'cargo');assert.equal(command.oneShot,false);assert.equal(command.exit,0);assert.equal(command.inputHashes.sourceTree,report.inputs.sourceTree);for(const [path,sha]of Object.entries(report.inputs.files))assert.equal(digest(bounded(join(command.cwd,path),16<<20)),sha,'preparation tool closure changed');for(const path of ['tests/rust-performance-paired/agent-custody.mjs','tests/rust-performance-paired/agent-environment.mjs','tests/rust-performance-paired/agent-tool-identity.mjs','tools/rust-owned-memory.mjs'])assert(report.inputs.files[path]);assert(Number.isSafeInteger(call.actualTimeoutMs)&&call.actualTimeoutMs>=1000&&call.actualTimeoutMs<=600000);const nativeRequest=join(evidence,'process-'+proof.runId+'.request.json'),native=parse(bounded(nativeRequest,1<<20));assert.equal(native.timeoutMs,call.actualTimeoutMs);const verified=authenticate({...call.process,binary:'cargo',args,stdoutSHA256:call.process.stdoutSHA256,stderrSHA256:call.process.stderrSHA256},{owner:call.cargoReceipt,receipt:call.buildEvidence,root:command.cwd,binary:'cargo',args,inputs:report.inputs.files,ids:new Set(),outer:{pid:native.coordinatorPid,timing:proof.timing},measured:false,maximumTimeout:call.actualTimeoutMs,cargoBuild:true,allowStderr:true,resolvedIdentity:report.inputs.cargoIdentity});assert.equal(digest(verified.stdout),call.process.stdoutSHA256);
 const fixture=loadAgentDataFixture(spec.directory,kind);assert.deepEqual(fixture.receipt,report.fixtureIdentity[kind]);return fixture;
}
