// Immutable, comparison-only Go output custody for sequential engine diagnostics.
// This is not a cache or alternate product index; never move/adopt a home here.
import assert from 'node:assert/strict';
import {createReadStream, lstatSync, readFileSync, writeFileSync} from 'node:fs';
import {join, relative, resolve, sep} from 'node:path';
import {Readable} from 'node:stream';
import readline from 'node:readline';
import {contained, sha256} from '../../tools/rust-disk.mjs';
import {hashFile} from '../../tools/rust-build.mjs';
import {splitIndexEvidence, verifyParts, INDEX_BYTES} from '../../tools/rust-source-pack-evidence.mjs';
const MAX_FACT_BYTES = 32 << 20, MAX_FACTS_BYTES = 512 << 20, MAX_FILES = 100000;
const exact = (value, fields) => assert.deepEqual(Object.keys(value ?? {}).sort(), [...fields].sort(), 'exact custody fields');
const digest = value => assert.match(value, /^[0-9a-f]{64}$/);
function readBounded(path, maximum) { const stat = lstatSync(path); assert(stat.isFile() && !stat.isSymbolicLink() && stat.size <= maximum, 'custody file budget/type'); return readFileSync(path); }
export function goFactRelative(document, parser) {
  assert.equal(parser, 'source-v1'); assert(typeof document.path === 'string' && document.path && !document.path.includes('\\') && !document.path.split('/').some(p => !p || p === '.' || p === '..')); digest(document.sha256);
  const extension = document.path.slice(document.path.lastIndexOf('.')).toLowerCase(); assert(['.lua', '.xml', '.toc'].includes(extension));
  const mode = extension + (extension === '.lua' && document.path.includes('/Blizzard_APIDocumentationGenerated/') ? ':generated-api' : '');
  return {relative: `source/v1/facts/files/${parser}/${sha256(`${parser}\0${mode}\0${document.sha256}`)}.json`, mode};
}
function authenticateFact(raw, document, parser, mode) {
  assert(raw.length <= MAX_FACT_BYTES); const text = new TextDecoder('utf-8', {fatal: true}).decode(raw), value = JSON.parse(text);
  assert.equal(value.schema, 'lycheedev.source-file-facts.v2'); assert.equal(value.inputSHA256, document.sha256); assert.equal(value.parser, parser); assert.equal(value.mode, mode);
  const marker = ',"facts":', offset = text.indexOf(marker); assert(offset > 0); assert.equal(sha256(text.slice(offset + marker.length, -1)), value.factsSHA256);
  exact(value.facts, ['declarations', 'relationships', 'loads', 'headers', 'diagnostics']); for (const rows of Object.values(value.facts)) assert(Array.isArray(rows));
}
export async function captureGoCustody(home, index, directory, root) {
  assert.equal(hashFile(index.path, INDEX_BYTES), index.sha256); assert.equal(index.manifest.recordsHash, index.sha256); assert.equal(index.manifest.recordBytes, index.bytes);
  const manifestPath = join(directory, 'go-original-manifest.json');
  const originalManifest = readBounded(join(resolve(index.path, '..'), 'manifest.json'), 1 << 20); assert.deepEqual(JSON.parse(originalManifest), index.manifest); writeFileSync(manifestPath, originalManifest, {flag: 'wx'});
  const recordsManifestPath = join(directory, 'go-index-parts/manifest.json');
  const recordsManifest = JSON.parse(readBounded(recordsManifestPath, 65536)); verifyParts(resolve(recordsManifestPath, '..'), recordsManifest);
  const files = [], known = new Set(), documents = new Set(); let total = 0;
  for await (const line of readline.createInterface({input: createReadStream(index.path), crlfDelay: Infinity})) {
    if (!line) continue; const doc = JSON.parse(line); if (doc.kind !== 'document') continue;
    assert(!documents.has(doc.path) && documents.size < MAX_FILES, 'duplicate/budget Go document'); documents.add(doc.path);
    const item = goFactRelative(doc, index.manifest.summary.parser), path = contained(home, join(home, item.relative));
    const raw = readBounded(path, MAX_FACT_BYTES); authenticateFact(raw, doc, index.manifest.summary.parser, item.mode);
    if (known.has(item.relative)) continue; known.add(item.relative);
    total += raw.length; assert(total <= MAX_FACTS_BYTES, 'custody total Facts budget');
    const source = {path, bytes: raw.length, sha256: sha256(raw)}, name = `go-fact-${String(files.length).padStart(5, '0')}`;
    splitIndexEvidence(root, source, join(directory, name));
    files.push({relative: item.relative, bytes: raw.length, sha256: source.sha256, manifest: `${name}/manifest.json`, manifestSHA256: hashFile(join(directory, name, 'manifest.json'), 65536)});
  }
  assert.equal(documents.size, index.manifest.summary.documents); assert.equal(hashFile(index.path, INDEX_BYTES), index.sha256, 'Go records changed while capturing Facts');
  const catalog = {schema: 'lycheedev.source-go-facts-custody.v1', home: resolve(home), documents: documents.size, factsBytes: total,
    records: {manifest: 'go-index-parts/manifest.json', manifestSHA256: hashFile(recordsManifestPath, 65536), bytes: index.bytes, sha256: index.sha256},
    manifest: {path: 'go-original-manifest.json', bytes: originalManifest.length, sha256: sha256(originalManifest)}, files};
  const raw = Buffer.from(JSON.stringify(catalog, null, 2) + '\n'); assert(raw.length <= 16 << 20); const path = join(directory, 'go-custody.json'); writeFileSync(path, raw, {flag: 'wx'});
  return {directory, catalog: path, catalogSHA256: sha256(raw)};
}
export function openGoCustody(reference) {
  exact(reference, ['directory', 'catalog', 'catalogSHA256']); digest(reference.catalogSHA256);
  const directory = resolve(reference.directory), catalogPath = contained(directory, reference.catalog);
  const raw = readBounded(catalogPath, 16 << 20); assert.equal(sha256(raw), reference.catalogSHA256, 'Go custody catalog changed');
  const catalog = JSON.parse(raw); exact(catalog, ['schema', 'home', 'documents', 'factsBytes', 'records', 'manifest', 'files']); assert.equal(catalog.schema, 'lycheedev.source-go-facts-custody.v1');
  assert(typeof catalog.home === 'string' && resolve(catalog.home) === catalog.home); assert(Number.isSafeInteger(catalog.documents) && catalog.documents > 0 && catalog.documents <= MAX_FILES);
  assert(Array.isArray(catalog.files) && catalog.files.length <= catalog.documents); assert(Number.isSafeInteger(catalog.factsBytes) && catalog.factsBytes >= 0 && catalog.factsBytes <= MAX_FACTS_BYTES);
  exact(catalog.manifest, ['path', 'bytes', 'sha256']); assert.equal(catalog.manifest.path, 'go-original-manifest.json'); digest(catalog.manifest.sha256);
  const manifestFile = contained(directory, join(directory, catalog.manifest.path)), manifestFiles = [{path: manifestFile, sha256: catalog.manifest.sha256, maximum: 1 << 20}];
  const manifestRaw = readBounded(manifestFile, 1 << 20); assert.equal(manifestRaw.length, catalog.manifest.bytes); assert.equal(sha256(manifestRaw), catalog.manifest.sha256);
  const manifest = JSON.parse(manifestRaw); assert.equal(manifest.summary.documents, catalog.documents); assert.equal(manifest.summary.parser, 'source-v1');
  function parts(row, name, maximum) {
    assert.equal(row.manifest, name); digest(row.manifestSHA256); digest(row.sha256); assert(Number.isSafeInteger(row.bytes) && row.bytes >= 0 && row.bytes <= maximum);
    const path = contained(directory, join(directory, row.manifest)), bytes = readBounded(path, 65536); assert.equal(sha256(bytes), row.manifestSHA256, 'custody parts manifest changed');
    manifestFiles.push({path, sha256: row.manifestSHA256, maximum: 65536});
    const value = JSON.parse(bytes); assert.equal(value.source.bytes, row.bytes); assert.equal(value.source.sha256, row.sha256); verifyParts(resolve(path, '..'), value);
    return {directory: resolve(path, '..'), manifest: value};
  }
  exact(catalog.records, ['manifest', 'manifestSHA256', 'bytes', 'sha256']);
  const records = parts(catalog.records, 'go-index-parts/manifest.json', INDEX_BYTES);
  assert.equal(manifest.recordsHash, catalog.records.sha256); assert.equal(manifest.recordBytes, catalog.records.bytes);
  const byPath = new Map(); let total = 0;
  for (const [i, row] of catalog.files.entries()) {
    exact(row, ['relative', 'bytes', 'sha256', 'manifest', 'manifestSHA256']); assert.match(row.relative, /^source\/v1\/facts\/files\/source-v1\/[0-9a-f]{64}\.json$/); assert(!byPath.has(row.relative), 'duplicate custody fact path');
    const value = parts(row, `go-fact-${String(i).padStart(5, '0')}/manifest.json`, MAX_FACT_BYTES); total += row.bytes; assert(total <= MAX_FACTS_BYTES);
    assert.equal(resolve(value.manifest.source.path), resolve(catalog.home, row.relative), 'custody original fact path mismatch'); byPath.set(row.relative, {row, ...value});
  }
  assert.equal(total, catalog.factsBytes);
  function unchangedCatalog() { assert.equal(hashFile(catalogPath, 16 << 20), reference.catalogSHA256, 'Go custody changed during comparison'); }
  function unchangedManifests() { for (const file of manifestFiles) assert.equal(hashFile(file.path, file.maximum), file.sha256, 'Go custody referenced manifest changed during comparison'); }
  async function* streamRecords() {
    unchangedCatalog();
    for (const part of records.manifest.parts) for await (const chunk of createReadStream(join(records.directory, part.path), {highWaterMark: 65536})) yield chunk;
    verifyParts(records.directory, records.manifest); unchangedCatalog();
  }
  const readFact = path => {
    unchangedCatalog(); const name = relative(catalog.home, resolve(path)).split(sep).join('/'), value = byPath.get(name); assert(value, 'missing or foreign custody fact');
    verifyParts(value.directory, value.manifest);
    const raw = Buffer.concat(value.manifest.parts.map(part => readBounded(join(value.directory, part.path), 8 << 20))); assert.equal(raw.length, value.row.bytes); assert.equal(sha256(raw), value.row.sha256, 'custody fact changed while reading'); return raw;
  };
  return {home: catalog.home, recordsPath: records.manifest.source.path, manifestRaw, recordsStream: Readable.from(streamRecords()), readFact, verify() { unchangedCatalog(); unchangedManifests(); verifyParts(records.directory, records.manifest); for (const value of byPath.values()) verifyParts(value.directory, value.manifest); unchangedManifests(); unchangedCatalog(); }};
}
