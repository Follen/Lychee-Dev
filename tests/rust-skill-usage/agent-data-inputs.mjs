// Readers for the two explicit synthetic CASC inputs. No historical paths.
import assert from 'node:assert/strict';
import {existsSync, lstatSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, join, resolve, win32, posix} from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import {hashFile} from '../../tools/rust-build.mjs';
import {contained} from '../../tools/rust-disk.mjs';
import {inventory, inventoryHash, jsonFile, within} from './assertions.mjs';
import {bounded, decodeFixtureBLTE, digest, goJSON} from '../rust-performance-paired/data-inputs.mjs';
import {EXPORTER_ARGS, EXPORTER_TARGETS} from './agent-data-exporters.mjs';
import {parseAgentJson} from '../../tools/rust-agent-client.mjs';
const parse = parseAgentJson;
export {bounded, digest, goJSON, parse};
export const pathStyle = path => /^[A-Za-z]:[\\/]/.test(path) || path.startsWith('\\\\') ? win32 : posix;
export function ordinaryPath(path) {
  if (path.startsWith('\\\\?\\UNC\\')) return '\\\\' + path.slice(8);
  return path.startsWith('\\\\?\\') ? path.slice(4) : path;
}
export const FIXED_PIN = {product: 'retail', region: 'cn', fullBuild: '12.1.0.69933', language: 'zhCN', definitionCommit: 'b'.repeat(40)};
export const SAMPLE_KEY_ID = '1234567890abcdef', SAMPLE_KEY = '000102030405060708090a0b0c0d0e0f';
export const ENCRYPTED_CHUNK = '4508efcdab9078563412041122334453c6ab63b7daa05cd383';
export const MISSING_KEYS = '0000000000000001 ' + SAMPLE_KEY + '\n';
export const RESTORED_KEYS = SAMPLE_KEY_ID + ' ' + SAMPLE_KEY + '\n';
export function npmOfflineFlags(scratch) {
  const p = pathStyle(scratch);
  return ['--offline', '--ignore-scripts', '--no-audit', '--no-fund', '--no-update-notifier', '--userconfig', p.join(scratch, 'user.npmrc'), '--globalconfig', p.join(scratch, 'global.npmrc'), '--cache', p.join(scratch, 'npm-cache')];
}
export function loadExporters(root, receiptPath, sourceTree) {
  const receipt = jsonFile(contained(root, resolve(receiptPath))), evidence = dirname(receiptPath);
  assert.equal(receipt.schema, 'lycheedev.disk-owner.v1'); assert.equal(receipt.owner, 'skill-data-exporters');
  assert(['succeeded', 'retired'].includes(receipt.state)); assert.equal(receipt.handlesReaped, true);
  assert.equal(receipt.inputHashes?.sourceTree, sourceTree);
  function sealed(name) {
    const file = contained(evidence, join(evidence, name)), row = receipt.sealedLogs.find(row => row.path === file);
    assert(row, `missing sealed exporter ${name}`); assert.equal(hashFile(file), row.sha256); return file;
  }
  const command = jsonFile(sealed('command.json'));
  assert.equal(command.cwd, root); assert.equal(command.channel, join(root, 'target')); assert.equal(command.target, join(root, 'target'));
  assert.deepEqual(receipt.command, ['cargo', ...EXPORTER_ARGS]);
  assert.equal(command.command, 'cargo'); assert.deepEqual(command.args, EXPORTER_ARGS); assert.equal(command.exit, 0);
  assert.equal(command.error, undefined); assert.equal(command.oneShot, false); assert.equal(command.inputHashes.sourceTree, sourceTree);
  const manifestPath = contained(evidence, command.skillDataExporters.path);
  assert.equal(hashFile(manifestPath), command.skillDataExporters.sha256);
  const manifest = jsonFile(manifestPath);
  assert.equal(manifest.schema, 'lycheedev.skill-agent-data-exporters.v1'); assert.equal(manifest.buildEvidence, receiptPath);
  assert.equal(manifest.sourceTree, sourceTree); assert.equal(manifest.target, command.target);
  const artifacts = bounded(sealed('stdout.bin'), 8 << 20).toString().split(/\r?\n/).filter(Boolean).map(line => JSON.parse(line));
  assert.deepEqual(manifest.executables.map(row => row.name), EXPORTER_TARGETS);
  for (const row of manifest.executables) {
    const compiled = artifacts.filter(item => item.reason === 'compiler-artifact' && item.target?.name === row.name && item.profile?.test && item.executable);
    assert.equal(compiled.length, 1); assert.deepEqual(compiled[0].target.kind, ['test']);
    assert.equal(row.source, join(root, 'crates/lycheedev-data/tests', row.name + '.rs'));
    assert.equal(resolve(compiled[0].target.src_path), row.source); assert.equal(resolve(compiled[0].executable), row.path);
    contained(command.target, row.path); assert.equal(lstatSync(row.path).size, row.bytes); assert.equal(hashFile(row.path, 128 << 20), row.sha256);
  }
  return {receiptPath, receiptSHA256: hashFile(receiptPath), manifestPath, manifestSHA256: hashFile(manifestPath), ...manifest};
}

// Exact EKey selection is important: the ordinary framed Encoding object ALSO
// has header length 60, so frame shape alone cannot select encrypted Sample.
export function decodePartialFrame(frame, identity, raw) {
  assert.equal(frame.subarray(0, 4).toString(), 'BLTE'); assert.equal(frame.readUInt32BE(4), 60);
  assert.equal(digest(frame.subarray(0, 60), 'md5'), identity.encodingKey);
  assert.equal(digest(frame), identity.encodedSHA256); assert.equal(frame.length, identity.encodedBytes);
  assert.deepEqual([...frame.subarray(8, 12)], [15, 0, 0, 2]);
  const blocks = []; let offset = 60;
  for (let i = 0; i < 2; i++) {
    const at = 12 + i * 24, length = frame.readUInt32BE(at), decoded = frame.readUInt32BE(at + 4);
    assert(length > 0 && offset + length <= frame.length); const block = frame.subarray(offset, offset + length); offset += length;
    assert.equal(digest(block, 'md5'), frame.subarray(at + 8, at + 24).toString('hex')); blocks.push({block, decoded});
  }
  assert.equal(offset, frame.length); assert.equal(blocks[0].block[0], 78);
  assert(blocks[0].block.subarray(1).equals(raw.subarray(0, blocks[0].decoded)));
  assert.equal(blocks[1].decoded, 8); assert.equal(blocks[0].decoded + 8, raw.length);
  assert.equal(identity.encryptedChunkHex, ENCRYPTED_CHUNK);
  assert.equal(blocks[1].block.toString('hex'), ENCRYPTED_CHUNK);
  // The exporter descriptor cannot choose a different key or plaintext.
  assert.equal(identity.keyID, SAMPLE_KEY_ID); assert.equal(identity.key, SAMPLE_KEY);
  assert.equal(blocks[1].block[0], 69); assert.equal(blocks[1].block[1], 8);
  assert.equal(blocks[1].block.subarray(2, 10).toString('hex'), 'efcdab9078563412');
  assert.equal(raw.readUInt32LE(raw.length - 8), 2); assert.equal(raw.readUInt32LE(raw.length - 4), 99);
  const partial = Buffer.concat([raw.subarray(0, blocks[0].decoded), Buffer.alloc(8)]);
  return {raw: partial, ekey: identity.encodingKey, chunks: 2, encodedBytes: frame.length};
}
function fixtureParts({kind, originalRoot, files, read, databasePath, dataDatabasePath}) {
  const identityBytes = read('input-identity.json'), identity = parse(identityBytes), p = pathStyle(originalRoot);
  assert.equal(identity.schema, 'lycheedev.catalog-navigation-fixture.v1'); assert.equal(identity.scope, 'synthetic authenticated local input; not real-client evidence');
  assert.equal(identity.home, p.join(originalRoot, 'home'));
  assert.equal(identity.installation, p.join(originalRoot, 'game', '不是身份 client'));
  for (const [key, value] of Object.entries(FIXED_PIN)) assert.equal(identity.pin[key], value);
  const expectedTables = kind === 'catalog' ? ['Item', 'ItemSparse', 'HouseDecor'] : ['Sample'];
  assert.deepEqual(identity.tables.map(row => row.name), expectedTables);
  assert.deepEqual(files.filter(row => row.path.startsWith('home/state/')).map(row => row.path), ['home/state/data/cache-v1.sqlite', 'home/state/toolkit.sqlite']);
  const db = new DatabaseSync(databasePath, {readOnly: true}), dataDb = new DatabaseSync(dataDatabasePath, {readOnly: true}), documents = [], blobs = [];
  try {
    for(const [key,expected]of [['application_id',0x4c444331],['user_version',1],['page_size',65536],['auto_vacuum',0],['journal_mode','delete']])assert.equal(Object.values(dataDb.prepare('PRAGMA '+key).get())[0],expected);
    assert.deepEqual(dataDb.prepare("SELECT name,type,tbl_name,sql FROM sqlite_schema ORDER BY name").all().map(row=>({...row})),[{name:'documents',type:'table',tbl_name:'documents',sql:'CREATE TABLE documents (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID'}]);
    const sharedRows = db.prepare('SELECT key,generation,value FROM documents ORDER BY key').all();
    const dataRows = dataDb.prepare('SELECT key,generation,value FROM documents ORDER BY key').all();
    assert.equal(sharedRows.length, 1);assert.equal(sharedRows[0].key,'pin/'+identity.snapshot); assert.equal(dataRows.length, identity.tables.length + 1);assert(dataRows.every(row=>row.key.startsWith('definition/')));
    const rows = [...sharedRows, ...dataRows];
    const pin = rows.find(row => row.key === 'pin/' + identity.snapshot); assert(pin);
    const selection = parse(Buffer.from(pin.value)); assert.deepEqual(selection.data, identity.pin);
    const canonical = {schema: 'lycheedev.selection.v1', id: '', data: Object.fromEntries(['product', 'region', 'fullBuild', 'buildConfig', 'cdnConfig', 'language', 'definitionCommit'].map(key => [key, identity.pin[key]]))};
    assert.deepEqual({...selection, id: ''}, canonical); assert.equal(identity.snapshot, 'PIN-' + digest(goJSON(canonical))); assert.equal(selection.id, identity.snapshot);
    const base = `https://raw.githubusercontent.com/wowdev/WoWDBDefs/${FIXED_PIN.definitionCommit}/`;
    const expected = new Map(['manifest.json', ...expectedTables.map(name => `definitions/${name}.dbd`)].map(name => ['definition/' + digest(base + name), base + name]));
    for (const row of rows) {
      assert.equal(row.generation, 1); const bytes = Buffer.from(row.value); assert(bytes.length <= 1 << 20);
      documents.push({key: row.key, bytes, sha256: digest(bytes)}); if (row === pin) continue;
      const value = parse(bytes); assert(expected.has(row.key)); assert.equal(value.url, expected.get(row.key));
      const raw = read(`home/blobs/${value.blob.sha256.slice(0, 2)}/${value.blob.sha256.slice(2)}`);
      assert.equal(raw.length, value.blob.bytes); assert.equal(digest(raw), value.blob.sha256); blobs.push({...value.blob, raw});
      const table = identity.tables.find(table => value.url.endsWith(`/definitions/${table.name}.dbd`));
      if (table) assert.equal(table.dbdSHA256, value.blob.sha256);
      else {const manifest=parse(raw);assert.deepEqual(manifest.map(row=>[row.tableName,row.db2FileDataID]),identity.tables.map(row=>[row.name,row.fileDataID]));for(const row of manifest){const table=identity.tables.find(table=>table.name===row.tableName);table.manifestTableHash=row.tableHash;}}
    }
    if (db.prepare("SELECT name FROM sqlite_master WHERE name='deleted_generations'").get()) assert.equal(db.prepare('SELECT count(*) AS n FROM deleted_generations').get().n, 0);
      if (dataDb.prepare("SELECT name FROM sqlite_master WHERE name='deleted_generations'").get()) assert.equal(dataDb.prepare('SELECT count(*) AS n FROM deleted_generations').get().n, 0);
  } finally { dataDb.close(); db.close(); }
  let partial, full;
  if (kind === 'partial') {
    partial = parse(read('partial-identity.json')); full = read('sample.wdc');
    assert.equal(partial.schema, 'lycheedev.skill-partial-fixture.v2'); assert.equal(full.length, partial.rawBytes); assert.equal(digest(full), partial.rawSHA256);
    assert.equal(full.subarray(0, 4).toString(), 'WDC3'); assert.equal(full.readUInt32LE(4), 2);
    assert.equal(full.readUInt32LE(68), 2); assert.equal(full.subarray(112, 120).toString('hex'), 'efcdab9078563412');
    assert.deepEqual([0, 4, 8, 12].map(offset => full.readUInt32LE(full.length - 16 + offset)), [1, 42, 2, 99]);
    assert.deepEqual(partial.expectedRows, [[1, 42], [2, 99]]);
    assert.equal(identity.tables[0].fileDataID, 201); assert.equal(identity.tables[0].sha256, partial.rawSHA256); assert.equal(identity.tables[0].bytes, full.length);
  }
  const archive = read('game/Data/data/data.000'), index = read('game/Data/data/0000000001.idx'), objects = {}, rawObjects = {};
  assert(index.length >= 40 && (index.length - 40) % 18 === 0);
  let selected = 0;
  for (let at = 40; at < index.length; at += 18) {
    assert.equal(index[at + 9], 0); const offset = index.readUInt32BE(at + 10), size = index.readUInt32LE(at + 14);
    assert(size >= 39 && offset + size <= archive.length); const frame = archive.subarray(offset + 30, offset + size), header = frame.readUInt32BE(4);
    const ekey = digest(header ? frame.subarray(0, header) : frame, 'md5');
    assert.equal(index.subarray(at, at + 9).toString('hex'), ekey.slice(0, 18));
    const encrypted = partial && ekey === partial.encodingKey;
    const decoded = encrypted ? decodePartialFrame(frame, partial, full) : decodeFixtureBLTE(frame);
    if (encrypted) { selected++; partial.partialContent = {sha256: digest(decoded.raw), bytes: decoded.raw.length}; }
    const raw = encrypted ? full : decoded.raw, sha256 = digest(raw);
    objects[sha256] = {decodedBytes: raw.length, encodedBytes: frame.length, ckey: digest(raw, 'md5'), ekey}; rawObjects[sha256] = raw;
    if (encrypted) { objects[partial.partialContent.sha256] = {...objects[sha256]}; rawObjects[partial.partialContent.sha256] = decoded.raw; }
  }
  if (partial) assert.equal(selected, 1);
  for (const table of identity.tables) {assert.equal(objects[table.sha256]?.decodedBytes, table.bytes);assert.equal(table.manifestTableHash,rawObjects[table.sha256].readUInt32LE(20).toString(16).toUpperCase().padStart(8,'0'));}
  const configs = [identity.pin.buildConfig, identity.pin.cdnConfig].map(key => {
    assert.match(key, /^[a-f0-9]{32}$/); const raw = read(`game/Data/config/${key.slice(0, 2)}/${key.slice(2, 4)}/${key}`);
    assert.equal(digest(raw, 'md5'), key); return {sha256: digest(raw), bytes: raw.length, raw};
  });
  // Independently bind config -> Encoding pages -> Root -> selected CKeys.
  const build = configs[0].raw.toString(), rootKey = /^root = ([a-f0-9]{32})$/m.exec(build)?.[1];
  const encoded = /^encoding = ([a-f0-9]{32}) ([a-f0-9]{32})$/m.exec(build);
  assert(rootKey && encoded);
  const rootEntry = Object.entries(objects).find(([, value]) => value.ckey === rootKey);
  const encodingEntry = Object.entries(objects).find(([, value]) => value.ckey === encoded[1] && value.ekey === encoded[2]);
  assert(rootEntry && encodingEntry); const rootRaw = rawObjects[rootEntry[0]], encodingRaw = rawObjects[encodingEntry[0]];
  assert.equal(rootRaw.subarray(0, 4).toString(), 'TSFM'); const count = identity.tables.length;
  assert.equal(rootRaw.length, 24 + count * 20); assert.equal(rootRaw.readUInt32LE(4), count); assert.equal(rootRaw.readUInt32LE(12), count); assert.equal(rootRaw.readUInt32LE(20), 0x40);
  let next = 0;
  for (const [i, table] of [...identity.tables].sort((a, b) => a.fileDataID - b.fileDataID).entries()) {
    const id = next + rootRaw.readUInt32LE(24 + 4 * i); assert.equal(id, table.fileDataID); next = id + 1;
    assert.equal(rootRaw.subarray(24 + count * 4 + i * 16, 40 + count * 4 + i * 16).toString('hex'), objects[table.sha256].ckey);
  }
  assert.equal(encodingRaw.length, 2134); assert.equal(encodingRaw.subarray(0, 5).toString('hex'), '454e011010');
  assert.equal(encodingRaw.readUInt16BE(5), 1); assert.equal(encodingRaw.readUInt16BE(7), 1); assert.equal(encodingRaw.readUInt32BE(9), 1); assert.equal(encodingRaw.readUInt32BE(13), 1);
  const cp = encodingRaw.subarray(54, 1078), ep = encodingRaw.subarray(1110);
  assert.equal(digest(cp, 'md5'), encodingRaw.subarray(38, 54).toString('hex')); assert.equal(digest(ep, 'md5'), encodingRaw.subarray(1094, 1110).toString('hex'));
  const expectedObjects = [...identity.tables.map(table => objects[table.sha256]), rootEntry[1]];
  const ckRows = [...expectedObjects].sort((a, b) => a.ckey.localeCompare(b.ckey)), ekRows = [...expectedObjects].sort((a, b) => a.ekey.localeCompare(b.ekey));
  assert.equal(encodingRaw.subarray(22, 38).toString('hex'), ckRows[0].ckey); assert.equal(encodingRaw.subarray(1078, 1094).toString('hex'), ekRows[0].ekey);
  for (const [i, object] of ckRows.entries()) {
    const at = i * 38; assert.equal(cp[at], 1); assert.equal(cp.readUIntBE(at + 1, 5), object.decodedBytes);
    assert.equal(cp.subarray(at + 6, at + 22).toString('hex'), object.ckey); assert.equal(cp.subarray(at + 22, at + 38).toString('hex'), object.ekey);
  }
  for (const [i, object] of ekRows.entries()) { const at = i * 25; assert.equal(ep.subarray(at, at + 16).toString('hex'), object.ekey); assert.equal(ep.readUIntBE(at + 20, 5), object.encodedBytes); }
  assert(cp.subarray(ckRows.length * 38).every(byte => byte === 0)); assert(ep.subarray(ekRows.length * 25).every(byte => byte === 0));
  return {identity, documents, blobs, objects, rawObjects, rootRef: {sha256: rootEntry[0], bytes: rootRaw.length}, partial, configs, catalog: read('game/.build.info'), receipt: {kind, descriptorSHA256: digest(identityBytes), snapshot: identity.snapshot, pin: identity.pin, objects}};
}
export function loadFixture(directory, kind) {
  const files = inventory(directory, {maxFiles: 256, maxBytes: 64 << 20});
  const fixture = fixtureParts({kind, originalRoot: directory, files, read: name => bounded(within(directory, name)), databasePath: within(directory, 'home/state/toolkit.sqlite'), dataDatabasePath: within(directory, 'home/state/data/cache-v1.sqlite')});
  assert.deepEqual(inventory(directory, {maxFiles: 256, maxBytes: 64 << 20}), files);
  return {...fixture, directory, files};
}
export function archiveFixture(fixture, kind, evidence) {
  const files = fixture.files.map((row, i) => {
    const name = `fixture-${kind}-${String(i).padStart(3, '0')}.bin`, bytes = bounded(within(fixture.directory, row.path));
    assert.equal(digest(bytes), row.sha256); writeFileSync(join(evidence, name), bytes, {flag: 'wx'}); return {...row, evidence: name};
  });
  const manifest = {schema: 'lycheedev.skill-agent-fixture-archive.v1', kind, originalRoot: fixture.directory, inventorySHA256: inventoryHash(fixture.files), files};
  const name = `fixture-${kind}.json`, raw = Buffer.from(JSON.stringify(manifest, null, 2) + '\n'); writeFileSync(join(evidence, name), raw, {flag: 'wx'});
  return {file: name, bytes: raw.length, sha256: digest(raw)};
}
export function loadArchivedFixture(evidence, reference, kind) {
  const raw = bounded(within(evidence, reference.file)); assert.equal(raw.length, reference.bytes); assert.equal(digest(raw), reference.sha256);
  const manifest = parse(raw); assert.equal(manifest.schema, 'lycheedev.skill-agent-fixture-archive.v1'); assert.equal(manifest.kind, kind);
  assert(manifest.files.length <= 256); assert.equal(new Set(manifest.files.map(row => row.path)).size, manifest.files.length);
  let total = 0; const content = new Map();
  for (const row of manifest.files) {
    assert(row.path && row.path.split('/').every(part => part && part !== '.' && part !== '..') && !/[\\:\0]/.test(row.path));
    const bytes = bounded(within(evidence, row.evidence)); total += bytes.length; assert(total <= 64 << 20);
    assert.equal(bytes.length, row.bytes); assert.equal(digest(bytes), row.sha256); content.set(row.path, bytes);
  }
  assert.equal(inventoryHash(manifest.files.map(({evidence, ...row}) => row)), manifest.inventorySHA256);
  const read = name => { assert(content.has(name), `missing fixture ${name}`); return content.get(name); };
  const database = manifest.files.find(row => row.path === 'home/state/toolkit.sqlite'); assert(database);
  const dataDatabase = manifest.files.find(row => row.path === 'home/state/data/cache-v1.sqlite'); assert(dataDatabase);
  return {...fixtureParts({kind, originalRoot: manifest.originalRoot, files: manifest.files, read, databasePath: within(evidence, database.evidence), dataDatabasePath: within(evidence, dataDatabase.evidence)}), directory: manifest.originalRoot, manifest};
}
