// Exporter boundary fixtures are synthetic, never native encoder/Windows proof.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {join, dirname} from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import {fixture, put, putJSON, proof} from './read-fact-delta-postcard.fixtures.mjs';
import {sha256} from './read-fact-delta.mjs';
import {hashFile} from '../../tools/rust-build.mjs';
import {splitIndexEvidence} from '../../tools/rust-source-pack-evidence.mjs';
import {captureGoCustody, openGoCustody} from './source-pack-custody.mjs';
import {POSTCARD_LAYOUT, POSTCARD_INDEX_LAYOUT, INDEX_EXPORT_TEST, INDEX_EXPORT_ARGS, EXPORT_ARGS, LIMITS, postcardIndexIdentity,
  preparePostcardIndexExport, runPostcardIndexExport, comparePostcardIndex, capturePostcardIndexExport, postcardIndexView, verifyPostcardIndexCustody,
  verifyPostcardIndexFrame, postcardIndexExportSummary, assertPostcardOutputBudget} from './read-fact-delta-postcard.mjs';
import {comparePostcardFacts} from './source-pack-scenarios-postcard.mjs';
import {comparePostcardScenario, verifyFreshPostcardFacts, postcardComparisonFootprint} from './source-pack-scenarios-postcard-fresh.mjs';

function binaryIndex(f, configureAssets = () => {}) {
  const diagnostics = f.documents.flatMap((doc, i) => f.facts[i].diagnostics.map(d => ({path: doc.path, ...d})));
  const summary = {...JSON.parse(fs.readFileSync(f.index.path)).summary, schema: 'lycheedev.source-map.v2', storage: 'file-cache', assets: 4, diagnostics: diagnostics.length, complete: diagnostics.length === 0, ...(diagnostics.length ? {diagnosticSample: diagnostics} : {})};
  const assets = [{path: 'A.png', contentHash: sha256('image'), bytes: 64 << 20, extension: '.png', mime: 'image/png', format: 'png', width: 0, height: 0},
    {path: 'B.png', contentHash: sha256(''), bytes: 0, extension: '.png', mime: 'image/png', format: 'png', width: 0, height: 0},
    {path: 'Larger.blp', contentHash: '', bytes: (64 << 20) + 2, extension: '.blp', mime: 'image/x-blp', format: 'blp', width: 0, height: 0},
    {path: 'Large.blp', contentHash: '', bytes: (64 << 20) + 1, extension: '.blp', mime: 'image/x-blp', format: 'blp', width: 0, height: 0}];
  // Deliberately interleaved groups exercise the historical stable partition;
  // exact 64 MiB and empty input stay ordinary and retain their SHA values.
  [assets[1], assets[2]] = [assets[2], assets[1]]; configureAssets(assets);
  const symbols = [], records = [];
  for (const [i, doc] of f.documents.entries()) {
    const {factsLocator, ...source} = doc; records.push({kind: 'document', ...source});
    const facts = f.facts[i], rows = [];
    for (const [ordinal, d] of facts.declarations.entries()) {
      const symbol = {kind: 'declaration', name: d.name, category: d.category, confidence: 'exact', path: doc.path, line: d.line, endLine: d.endLine,
        ...(d.scope ? {scope: d.scope} : {}), ...(ordinal ? {ordinal} : {}), ...(d.signature ? {signature: d.signature} : {})};
      symbol.id = 'SYM-' + sha256([summary.repository, summary.commit, doc.path, String(d.line), String(d.endLine), d.name, d.category, d.scope ?? '', String(ordinal)].join('\0')).slice(0, 32); rows.push(symbol);
    }
    for (const r of facts.relationships) rows.push({kind: 'relationship', name: r.from, target: r.to, category: r.category, confidence: r.confidence, path: doc.path, line: r.line, endLine: r.line});
    for (const l of facts.loads) rows.push({kind: 'load', name: doc.path, target: l.path, category: l.kind, confidence: 'exact', path: doc.path, line: l.line, endLine: l.line});
    for (const h of facts.headers) rows.push({kind: 'header', name: h.key, target: h.value, category: 'toc-field', confidence: 'exact', path: doc.path, line: h.line, endLine: h.line});
    symbols.push(...rows); records.push(...rows.map(symbol => ({kind: 'symbol', path: doc.path, symbol}))); records.push(...facts.diagnostics.map(d => ({kind: 'diagnostic', path: doc.path, diagnostic: {path: doc.path, ...d}})));
  }
  records.push(...assets.map(asset => ({kind: 'asset', path: asset.path, bytes: asset.bytes, asset})));
  f.publicIndex = {documents: structuredClone(f.documents), symbols, assets: [...assets.filter(a => !a.contentHash), ...assets.filter(a => a.contentHash)].map(asset => ({...asset, local: asset.contentHash ? {sha256: asset.contentHash, bytes: asset.bytes} : null})),
    summary: {...summary, astFiles: f.documents.filter(d => !diagnostics.some(n => n.path === d.path)).length, skippedDocuments: 0}, compatibilityGaps: diagnostics.length ? ['frame-type', 'interface', 'mixin', 'template'] : []};
  const payload = Buffer.from('opaque synthetic boundary payload, not a Postcard fixture'), raw = Buffer.alloc(44 + payload.length), identity = postcardIndexIdentity(summary, POSTCARD_INDEX_LAYOUT);
  raw.write('LYCIDX01'); raw.writeUInt32LE(payload.length, 8); Buffer.from(identity, 'hex').copy(raw, 12); payload.copy(raw, 44);
  const sha = sha256(raw), path = join(f.home, 'cache/objects', sha.slice(0, 2), sha.slice(2)); put(path, raw);
  f.index = {path, bytes: raw.length, sha256: sha, manifest: {schema: 'lycheedev.source-index.rust.v7', identity, blob: {sha256: sha, bytes: raw.length}}};
  const goRaw = Buffer.from(records.map(row => JSON.stringify(row).replace(/\u2028/g, '\\u2028').replace(/\u2029/g, '\\u2029')).join('\n') + '\n'); put(f.records, goRaw);
  const manifest = {recordsHash: sha256(goRaw), recordBytes: goRaw.length, summary}; putJSON(join(dirname(f.records), 'manifest.json'), manifest);
  const factsExecute = f.execute; f.indexCalls = 0;
  f.execute = (binary, args, options) => {
    if (args[1] !== INDEX_EXPORT_TEST) return factsExecute(binary, args, options);
    f.indexCalls++; assert.deepEqual(args, INDEX_EXPORT_ARGS); assert.equal(binary, f.build.sourceBinary); assert.equal(options.cwd, f.build.candidate.sourceRoot);
    const manifest = JSON.parse(fs.readFileSync(options.env.LYCHEEDEV_SOURCE_INDEX_EXPORT_INPUT));
    const source = manifest.records[0], rows = [{schema: 'lycheedev.source-index-export-record.v1', ordinal: 0, ...source, index: structuredClone(f.publicIndex)}];
    f.mutateIndexRows?.(rows); const output = Buffer.from(rows.map(row => JSON.stringify(row)).join('\n') + '\n');
    put(options.env.LYCHEEDEV_SOURCE_INDEX_EXPORT_OUTPUT, output);
    const summary = {schema: 'lycheedev.source-index-export-summary.v1', codec: 'postcard-1.1.3', layout: 'LYCIDX01', manifestSHA256: options.env.LYCHEEDEV_SOURCE_INDEX_EXPORT_SHA256,
      manifestBytes: Number(options.env.LYCHEEDEV_SOURCE_INDEX_EXPORT_BYTES), records: 1, recordBytes: source.fileBytes, outputBytes: output.length, outputSHA256: sha256(output)};
    f.mutateIndexSummary?.(summary); f.afterIndexExecute?.();
    return {status: f.indexExit ?? 0, handlesReaped: true, proof: {...proof(), binary, ...f.indexProof}, stdout: Buffer.from(`running 1 test\ntest ${INDEX_EXPORT_TEST} ... ${JSON.stringify(summary)}\nok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n`), stderr: Buffer.alloc(0)};
  };
  f.capture = async () => { const directory = join(f.root, 'go-full-index'); fs.mkdirSync(directory); const goIndex = {path: f.records, bytes: goRaw.length, sha256: sha256(goRaw), manifest}; splitIndexEvidence(f.root, {path: f.records, bytes: goRaw.length, sha256: sha256(goRaw)}, join(directory, 'go-index-parts')); return captureGoCustody(f.goHome, goIndex, directory, f.root); };
  f.prepareIndex = () => preparePostcardIndexExport(f.root, f.home, f.index, join(f.scratch, 'index-input'));
  f.exportIndex = preparation => runPostcardIndexExport(f.root, f.build.expected, f.build.candidate, preparation, join(f.scratch, 'index-output.jsonl'), f.evidence, {execute: f.execute});
  return f;
}
const boundary = (t, callback, configureAssets) => fixture(t, f => callback(binaryIndex(f, configureAssets)));

test('binary Index frame authentication stays framing-only and rejects length/magic/identity/old JSON', t => boundary(t, f => {
  const raw = fs.readFileSync(f.index.path); verifyPostcardIndexFrame(raw, f.index.manifest.identity);
  for (const change of [b => b[0]++, b => b.writeUInt32LE(0, 8), b => b[12]++]) { const bad = Buffer.from(raw); change(bad); assert.throws(() => verifyPostcardIndexFrame(bad, f.index.manifest.identity)); }
  assert.throws(() => verifyPostcardIndexFrame(Buffer.from('{}'), f.index.manifest.identity));
  assert.throws(() => postcardIndexView({}, f.index), /independently compared/);
}));
test('same-source Index and Facts export compare every public field independently and archive before staging release', t => boundary(t, async f => {
  const reference = await f.capture(), go = openGoCustody(reference); go.reference = reference;
  const result = await comparePostcardFacts({root: f.root, candidate: f.build.candidate, exporter: f.build.expected, rustHome: f.home, index: f.index, go, scratch: f.scratch, evidence: f.evidence, layout: POSTCARD_INDEX_LAYOUT}, {execute: f.execute});
  assert.equal(f.indexCalls, 1); assert.equal(f.calls(), 1); assert.equal(result.documents, 3); assert.equal(result.indexExport.oracle.symbols, 18); assert.equal(result.indexExport.oracle.assets, 4);
  assert.deepEqual(result.indexExport.oracle.fields, ['documents', 'assets', 'symbols', 'summary', 'compatibilityGaps']);
  assert(!fs.existsSync(join(f.scratch, 'postcard-index-input'))); assert(!fs.existsSync(join(f.scratch, 'postcard-index-output.jsonl')));
  const custody = JSON.parse(fs.readFileSync(result.indexExport.custody)); fs.rmSync(f.home, {recursive: true}); verifyPostcardIndexCustody(custody);
}));
test('Index export authenticates wrong source/binary/output SHA and rejects incomplete/cancel/limit/unreaped results', async t => {
  for (const change of ['source', 'binary', 'output', 'cancelled', 'outputLimit', 'treeReaped', 'observed-binary']) await boundary(t, f => {
    if (change === 'source') f.build.expected.sourceTreeSHA256 = '0'.repeat(64);
    else if (change === 'binary') put(f.build.sourceBinary, 'wrong bytes');
    else if (change === 'output') f.mutateIndexSummary = s => s.outputSHA256 = '0'.repeat(64);
    else f.indexProof = change === 'observed-binary' ? {binary: join(f.root, 'wrong.exe')} : {[change]: change !== 'treeReaped'};
    assert.throws(() => f.exportIndex(f.prepareIndex())); assert.equal(f.indexCalls, ['source', 'binary'].includes(change) ? 0 : 1);
  });
  assert.throws(() => postcardIndexExportSummary({status: 0, handlesReaped: true, proof: proof(), stdout: Buffer.from('running 1 test'), stderr: Buffer.alloc(0)}, {}));
});
test('missing/extra/duplicate Index rows, wrong row identity/ordinal and unsafe public integers reject', async t => {
  for (const mutate of [rows => rows.pop(), rows => rows.push(rows[0]), rows => rows[0].ordinal++, rows => rows[0].identity = '0'.repeat(64), rows => rows[0].fileSHA256 = '0'.repeat(64), rows => rows[0].index.documents[0].bytes = Number.MAX_SAFE_INTEGER + 1]) await boundary(t, f => {
    f.mutateIndexRows = mutate; assert.throws(() => f.exportIndex(f.prepareIndex()));
  });
});
test('independent Go oracle catches full public fields, array order, duplicates and Rust-only old mappings', async t => {
  const mutations = [i => i.symbols.reverse(), i => i.symbols.pop(), i => i.symbols.push(i.symbols[0]), i => i.symbols[0].signature = 'changed', i => i.symbols[0].id = 'wrong', i => i.assets.reverse(), i => { [i.assets[0], i.assets[1]] = [i.assets[1], i.assets[0]]; }, i => { [i.assets[2], i.assets[3]] = [i.assets[3], i.assets[2]]; }, i => i.assets.push(i.assets[0]), i => i.assets[0].width++, i => i.assets[2].local.bytes++, i => i.assets[0].local = {}, i => i.documents.reverse(), i => i.documents[0].sha256 = '0'.repeat(64), i => i.summary.astFiles++, i => i.summary.skippedDocuments++, i => i.summary.diagnosticSample.pop(), i => i.compatibilityGaps.pop(), i => i.compatibilityGaps.reverse(), i => i.summary.io = {}];
  for (const mutate of mutations) await boundary(t, async f => {
    const reference = await f.capture(); f.mutateIndexRows = rows => mutate(rows[0].index); const token = f.exportIndex(f.prepareIndex());
    await assert.rejects(comparePostcardIndex(token, reference)); assert.throws(() => postcardIndexView(token, f.index));
  });
});
test('current binary Index SHA is rechecked for warm/view, changed staging/manifest/exporter also reject', async t => {
  for (const changed of ['binary', 'staging', 'manifest', 'exporter']) await boundary(t, f => {
    const preparation = f.prepareIndex(); const file = changed === 'binary' ? f.index.path : changed === 'staging' ? join(dirname(preparation.manifest), 'index.bin') : changed === 'manifest' ? preparation.manifest : f.build.sourceBinary;
    fs.appendFileSync(file, 'changed'); assert.throws(() => f.exportIndex(preparation)); assert.equal(f.indexCalls, 0);
  });
  await boundary(t, async f => { const reference = await f.capture(), token = f.exportIndex(f.prepareIndex()); await comparePostcardIndex(token, reference); capturePostcardIndexExport(token, f.evidence); fs.appendFileSync(f.index.path, 'changed'); assert.throws(() => postcardIndexView(token, f.index), /current binary Index SHA/); });
});
test('Index output custody uses existing split parts and rejects changed output/input/report bindings', async t => {
  for (const field of ['output', 'manifest', 'index', 'summary', 'oracle']) await boundary(t, async f => {
    const reference = await f.capture(), token = f.exportIndex(f.prepareIndex()); await comparePostcardIndex(token, reference); const custody = capturePostcardIndexExport(token, f.evidence);
    if (field === 'output') fs.appendFileSync(join(custody.output.directory, custody.output.manifest.parts[0].path), 'changed');
    if (field === 'manifest') fs.appendFileSync(custody.manifestArchive.path, 'changed');
    if (field === 'index') custody.index.sha256 = '0'.repeat(64);
    if (field === 'summary') custody.summary = {...custody.summary, outputSHA256: '0'.repeat(64)};
    if (field === 'oracle') custody.oracle.complete = false;
    assert.throws(() => verifyPostcardIndexCustody(custody));
  });
});
test('two full JSONL outputs share the original total cap, never drop fields or expand owner reservations', () => {
  assertPostcardOutputBudget(64 << 20, 64 << 20); assert.throws(() => assertPostcardOutputBudget(64 << 20, (64 << 20) + 1), /128 MiB/);
  assert.equal(LIMITS.total, 128 << 20); assert.equal(postcardComparisonFootprint().scratchReserveBytes, (512 + 128 + 8) * 2 ** 20);
});

async function scenarioBoundary(t, callback) {
  return fixture(t, async f => {
    for (const facts of f.facts) facts.diagnostics = []; f.saveGoFacts(); binaryIndex(f);
    Object.assign(f.build.candidate, {commit: '1'.repeat(40), artifactSHA256: f.build.expected.artifactSHA256, binarySHA256: f.build.expected.binarySHA256});
    const reference = await f.capture(), goManifest = JSON.parse(fs.readFileSync(join(dirname(f.records), 'manifest.json'))), summary = goManifest.summary;
    const retained = {reference, index: {path: f.records, bytes: fs.lstatSync(f.records).size, sha256: hashFile(f.records), manifest: goManifest}};
    const pin = {schema: 'lycheedev.selection.v1', id: 'PIN-binary', source: {repository: summary.repository, product: summary.product, exactCommit: summary.commit, requestedRef: summary.commit, parserRevision: summary.parser}};
    fs.mkdirSync(join(f.home, 'state')); const db = new DatabaseSync(join(f.home, 'state/toolkit.sqlite'));
    db.exec('PRAGMA user_version=1; CREATE TABLE documents(key TEXT PRIMARY KEY, generation INTEGER, value BLOB); CREATE TABLE deleted_generations(key TEXT PRIMARY KEY, generation INTEGER);');
    db.prepare('INSERT INTO documents VALUES (?,1,?)').run(`pin/${pin.id}`, Buffer.from(JSON.stringify(pin)));
    db.prepare('INSERT INTO documents VALUES (?,1,?)').run(`source/rust-index/${f.index.manifest.identity}`, Buffer.from(JSON.stringify(f.index.manifest))); db.close();
    const envelope = {schema: 'lycheedev.result.v1', ok: true, error: null, context: {snapshot: pin.id}, result: summary};
    const owners = [], spawnCalls = [];
    const beginScratch = (_root, owner, options) => {
      const directory = join(f.root, `owned-scratch-${owners.length}`), evidence = join(f.root, `owned-evidence-${owners.length}`), receipt = join(evidence, 'receipt.json'); fs.mkdirSync(directory); fs.mkdirSync(evidence);
      const value = {schema: 'lycheedev.disk-owner.v1', owner, state: 'active', activePid: null, directory}; putJSON(receipt, value);
      const job = {directory, evidence, receipt, options, cleaned: false, seal(record) { Object.assign(value, record); putJSON(receipt, value); this.sealed = record; }, cleanup() { assert.equal(value.state, 'succeeded'); assert.equal(value.handlesReaped, true); this.cleaned = true; fs.rmSync(directory, {recursive: true}); value.state = 'retired'; putJSON(receipt, value); }}; owners.push(job); return job;
    };
    f.runScenario = () => comparePostcardScenario({go: f.goHome, rust: f.home}, {go: envelope, rust: envelope}, pin, summary, f.evidence, f.root, retained,
      {candidate: f.build.candidate, limits: {}, layout: POSTCARD_INDEX_LAYOUT}, f.build.expected, {beginScratch, ownedSpawnSync(...args) { spawnCalls.push(args[1]); return f.execute(...args); }});
    Object.assign(f, {owners, spawnCalls}); await callback(f);
  });
}
test('fresh comparison port consumes real SQLite v18 manifest, archives both exports, and survives both homes cleanup', t => scenarioBoundary(t, async f => {
  fs.rmSync(f.goHome, {recursive: true}); const result = await f.runScenario();
  assert.deepEqual(f.spawnCalls, [INDEX_EXPORT_ARGS, EXPORT_ARGS]); assert(f.owners[0].cleaned); assert.equal(result.indexes.rust.sha256, f.index.sha256);
  assert(result.exporterTiming.exporterWrapperWallMillis >= 0); assert(result.indexExport.timing.exporterWrapperWallMillis >= 0); assert(result.validationWallMillis >= 0);
  fs.rmSync(f.home, {recursive: true}); verifyFreshPostcardFacts(result, f.build.candidate, f.build.expected, POSTCARD_INDEX_LAYOUT);
  assert.throws(() => verifyFreshPostcardFacts(result, f.build.candidate, f.build.expected, POSTCARD_LAYOUT));
}));
test('Index failure stops before Facts, retains raw/owned evidence, and unreaped Index remains interrupted', async t => {
  await scenarioBoundary(t, async f => { f.mutateIndexRows = rows => rows[0].index.symbols.reverse(); await assert.rejects(f.runScenario()); assert.equal(f.spawnCalls.length, 1); assert(!f.owners[0].cleaned); assert.equal(f.owners[0].sealed.state, 'failed'); assert(fs.existsSync(join(f.owners[0].evidence, 'postcard-index-failed-output-parts/manifest.json'))); });
  await scenarioBoundary(t, async f => { f.indexProof = {treeReaped: false}; await assert.rejects(f.runScenario(), e => e.handlesReaped === false); assert.equal(f.spawnCalls.length, 1); assert(!f.owners[0].cleaned); assert.equal(f.owners[0].sealed.state, 'interrupted'); assert(fs.existsSync(join(f.owners[0].directory, 'postcard-index-output.jsonl'))); });
  await scenarioBoundary(t, async f => { f.proofOverrides = {outputLimit: true}; await assert.rejects(f.runScenario()); assert.equal(f.spawnCalls.length, 2); assert(!f.owners[0].cleaned); assert(fs.existsSync(join(f.owners[0].evidence, 'postcard-index-custody.json'))); });
});
test('final fresh verifier binds exported public Index to original raw binary, independent Go and same exporter', async t => {
  for (const change of ['index-parts', 'index-JSONL', 'go-binding', 'exporter']) await scenarioBoundary(t, async f => {
    const result = await f.runScenario(), custody = JSON.parse(fs.readFileSync(result.indexExport.custody));
    if (change === 'index-parts') fs.appendFileSync(join(result.indexCustody.directory, 'index.part000.bin'), 'changed');
    if (change === 'index-JSONL') fs.appendFileSync(join(custody.output.directory, 'index.part000.bin'), 'changed');
    if (change === 'go-binding') result.indexExport.oracle.goRecordsSHA256 = '0'.repeat(64);
    if (change === 'exporter') result.exporter.artifact.binarySHA256 = '0'.repeat(64);
    assert.throws(() => verifyFreshPostcardFacts(result, f.build.candidate, f.build.expected, POSTCARD_INDEX_LAYOUT));
  });
});

test('historical asset partition uses strict >64MiB, keeps empty-input SHA, and rejects inconsistent Go metadata identity', async t => {
  await boundary(t, async f => {
    const reference = await f.capture(), token = f.exportIndex(f.prepareIndex()); const oracle = await comparePostcardIndex(token, reference);
    assert.equal(oracle.assetOrder, 'historical-f811-metadata-only-first-stable-partition');
    const assets = postcardIndexView(token, f.index).assets; assert.deepEqual(assets.map(a => a.path), ['Larger.blp', 'Large.blp', 'A.png', 'B.png']);
    assert.equal(assets[2].bytes, 64 << 20); assert.equal(assets[3].bytes, 0); assert.equal(assets[3].local.sha256, sha256(''));
  });
  for (const mutate of [assets => assets[0].contentHash = '', assets => assets[1].contentHash = sha256('unexpected'), assets => assets[2].contentHash = '']) await boundary(t, async f => {
    const reference = await f.capture(), token = f.exportIndex(f.prepareIndex()); await assert.rejects(comparePostcardIndex(token, reference), /metadata-only/);
  }, mutate);
});
