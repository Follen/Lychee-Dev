import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {dirname, join} from 'node:path';
import {tmpdir} from 'node:os';
import {sha256} from '../../tools/rust-disk.mjs';
import {hashFile} from '../../tools/rust-build.mjs';
import {splitIndexEvidence} from '../../tools/rust-source-pack-evidence.mjs';
import {readFactDelta, rustFactPath} from './read-fact-delta.mjs';
import {captureGoCustody, goFactRelative, openGoCustody} from './source-pack-custody.mjs';
const stable = value => Array.isArray(value) ? value.map(stable) : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(k => [k, stable(value[k])])) : value;
const save = (path, value) => { mkdirSync(dirname(path), {recursive: true}); writeFileSync(path, typeof value === 'string' || Buffer.isBuffer(value) ? value : JSON.stringify(value)); };
async function fixture(t, large = false) {
  const root = mkdtempSync(join(tmpdir(), 'pack2-custody-')); t.after(() => { if (t.passed) rmSync(root, {recursive: true}); else t.diagnostic(`retained ${root}`); });
  const go = join(root, 'go'), rust = join(root, 'rust'), evidence = join(root, 'evidence'); for (const p of [go, rust, evidence]) mkdirSync(p);
  const document = {kind: 'document', path: 'Interface/AddOns/a.lua', sha256: sha256('abc'), bytes: 3};
  const facts = {declarations: [{name: 'A', line: 1}, {name: 'A', line: 1}], relationships: [{from: 'A', to: 'B', line: 1}], loads: [], headers: large ? [{key: 'text', value: 'x'.repeat((8 << 20) + 10)}] : [], diagnostics: []};
  const summary = {schema: 'lycheedev.source-map.v2', storage: 'file-cache', parser: 'source-v1', repository: 'wow-ui-source', product: 'retail', commit: 'c'.repeat(40), documents: 1, declarations: 2, relationships: 1, assets: 0, diagnostics: 0, complete: true};
  const recordPath = join(go, 'records/records.jsonl'), rawRecords = Buffer.from(JSON.stringify(document) + '\n'); save(recordPath, rawRecords);
  const manifest = {summary, recordsHash: sha256(rawRecords), recordBytes: rawRecords.length}; save(join(dirname(recordPath), 'manifest.json'), manifest);
  const file = goFactRelative(document, summary.parser), raw = JSON.stringify({schema: 'lycheedev.source-file-facts.v2', inputSHA256: document.sha256, mode: file.mode, parser: summary.parser, factsSHA256: sha256(JSON.stringify(facts)), facts}); save(join(go, file.relative), raw);
  const rustFile = rustFactPath(rust, document, 'v11'); save(rustFile, {schema: 'lycheedev.source-facts.rust.v11', inputSHA256: document.sha256, factsSHA256: sha256(JSON.stringify(stable(facts))), facts});
  const indexPath = join(rust, 'index.json'); save(indexPath, {summary, documents: [document]});
  const source = {path: recordPath, bytes: rawRecords.length, sha256: sha256(rawRecords)}; splitIndexEvidence(root, source, join(evidence, 'go-index-parts'));
  const reference = await captureGoCustody(go, {...source, manifest}, evidence, root);
  return {root, go, rust, evidence, document, facts, manifest, reference, indexPath, file: join(go, file.relative), records: recordPath, rustFile};
}
function editCatalog(f, fn) { const catalog = JSON.parse(readFileSync(f.reference.catalog)); fn(catalog); save(f.reference.catalog, catalog); f.reference.catalogSHA256 = hashFile(f.reference.catalog); return catalog; }
async function compare(f) { const custody = openGoCustody(f.reference); const r = await readFactDelta(custody.home, f.rust, custody.recordsPath, 'v11-pack2', f.indexPath, hashFile(f.indexPath), custody); custody.verify(); return r; }
test('full five arrays and duplicates remain equal after only the own Go scratch is removed', async t => { const f = await fixture(t); rmSync(f.go, {recursive: true}); const r = await compare(f); assert.equal(r.differentFiles.length, 0); assert.equal(r.documents, 1); assert.equal(r.totals.go.declarations, 2); assert.equal(r.inputs.rustLayout, 'v11-pack2'); });
test('large raw Go Facts stream into ordered 8 MiB custody parts and compare without materialized cache', async t => { const f = await fixture(t, true); const catalog = JSON.parse(readFileSync(f.reference.catalog)); const m = JSON.parse(readFileSync(join(f.evidence, catalog.files[0].manifest))); assert.equal(m.parts.length, 2); assert(m.parts.every(p => p.bytes <= 8 << 20)); rmSync(f.go, {recursive: true}); assert.equal((await compare(f)).differentFiles.length, 0); });
test('full field and array order differences survive sequential custody', async t => { const f = await fixture(t); const payload = JSON.parse(readFileSync(f.rustFile)); payload.facts.declarations = [{name: 'A', line: 2}, {name: 'A', line: 1}]; payload.factsSHA256 = sha256(JSON.stringify(stable(payload.facts))); save(f.rustFile, payload); const r = await compare(f); assert.equal(r.differentFiles.length, 1); assert.equal(r.differentFiles[0].exactRowsDiffer[0].section, 'declarations'); });
test('catalog raw-byte modification is rejected before opening streams', async t => { const f = await fixture(t); save(f.reference.catalog, readFileSync(f.reference.catalog, 'utf8') + ' '); assert.throws(() => openGoCustody(f.reference), /catalog changed/); });
test('missing and tampered parts are rejected without consulting the removed original home', async t => { for (const remove of [false, true]) { const f = await fixture(t); const part = join(f.evidence, 'go-fact-00000/index.part000.bin'); if (remove) rmSync(part); else save(part, 'tampered'); rmSync(f.go, {recursive: true}); assert.throws(() => openGoCustody(f.reference)); } });
test('duplicate paths, escaping paths and foreign original paths are rejected', async t => {
  for (const mutate of [c => c.files.push(c.files[0]), c => c.files[0].relative = '../foreign', c => c.files[0].manifest = '../manifest.json', c => c.home += '-foreign']) { const f = await fixture(t); editCatalog(f, mutate); assert.throws(() => openGoCustody(f.reference)); }
});
test('parts range/order/bytes/hash changes cannot pass a resealed catalog', async t => {
  for (const mutate of [m => m.parts[0].offset++, m => m.parts[0].bytes++, m => m.parts[0].sha256 = '0'.repeat(64), m => m.parts.pop(), m => m.source.sha256 = '0'.repeat(64)]) {
    const f = await fixture(t), partManifest = join(f.evidence, 'go-fact-00000/manifest.json'), m = JSON.parse(readFileSync(partManifest)); mutate(m); save(partManifest, m); editCatalog(f, c => c.files[0].manifestSHA256 = hashFile(partManifest)); assert.throws(() => openGoCustody(f.reference));
  }
});
test('Facts total, per-file, document and records budgets are strict', async t => {
  for (const mutate of [c => c.factsBytes++, c => c.factsBytes = (512 << 20) + 1, c => c.files[0].bytes = (32 << 20) + 1, c => c.documents = 100001, c => c.records.bytes = (128 << 20) + 1, c => c.extra = true]) { const f = await fixture(t); editCatalog(f, mutate); assert.throws(() => openGoCustody(f.reference)); }
});
test('post-open fact or catalog mutation is rejected at use and final verification', async t => {
  for (const target of ['catalog', 'fact']) {
    const f = await fixture(t), custody = openGoCustody(f.reference); custody.recordsStream.destroy();
    const path = target === 'catalog' ? f.reference.catalog : join(f.evidence, 'go-fact-00000/index.part000.bin'); save(path, readFileSync(path, 'utf8').replace('source', 'Source'));
    assert.throws(() => custody.readFact(f.file)); assert.throws(() => custody.verify());
  }
});
test('source manifest hashes and missing expected Fact identities remain authenticated', async t => {
  const f = await fixture(t); editCatalog(f, c => { c.files = []; c.factsBytes = 0; }); const custody = openGoCustody(f.reference); await assert.rejects(readFactDelta(custody.home, f.rust, custody.recordsPath, 'v11-pack2', f.indexPath, hashFile(f.indexPath), custody), /missing or foreign/);
});
test('fact keys retain generated-mode distinction and reject unsafe identity', () => { const doc = {path: 'Interface/AddOns/Blizzard_APIDocumentationGenerated/a.lua', sha256: 'a'.repeat(64)}; assert.equal(goFactRelative(doc, 'source-v1').mode, '.lua:generated-api'); for (const path of ['../a.lua', 'a/../a.lua', '/a.lua', 'a\\b.lua']) assert.throws(() => goFactRelative({...doc, path}, 'source-v1')); assert.throws(() => goFactRelative(doc, 'wrong')); });

test('shared reader rejects an unrecognized sixth Facts field from either engine', async t => {
  for (const engine of ['go', 'rust']) {
    const f = await fixture(t), path = engine === 'go' ? f.file : f.rustFile, value = JSON.parse(readFileSync(path)); value.facts.extra = [];
    value.factsSHA256 = sha256(JSON.stringify(engine === 'rust' ? stable(value.facts) : value.facts)); save(path, value);
    if (engine === 'rust') await assert.rejects(compare(f), /exact five Facts/);
    else await assert.rejects(readFactDelta(f.go, f.rust, f.records, 'v11-pack2', f.indexPath, hashFile(f.indexPath)), /exact five Facts/);
  }
});

test('final custody verification reauthenticates every original and parts manifest after opening', async t => {
  for (const name of ['go-original-manifest.json', 'go-index-parts/manifest.json', 'go-fact-00000/manifest.json']) {
    const f = await fixture(t), custody = openGoCustody(f.reference); custody.recordsStream.destroy(); const path = join(f.evidence, name); save(path, readFileSync(path, 'utf8') + ' '); assert.throws(() => custody.verify(), /referenced manifest changed/);
  }
});
