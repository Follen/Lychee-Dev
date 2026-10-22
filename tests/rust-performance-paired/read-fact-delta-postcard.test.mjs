import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import {join, dirname, relative} from 'node:path';
import {sha256, readFactDelta, readRustFact} from './read-fact-delta.mjs';
import {sourceDigest, hashFile} from '../../tools/rust-build.mjs';
import {defaults} from '../../tools/rust-disk.mjs';
import {splitIndexEvidence, PART_BYTES} from '../../tools/rust-source-pack-evidence.mjs';
import {captureGoCustody, openGoCustody} from './source-pack-custody.mjs';
import {comparePostcardFacts, validatePostcardRequest} from './source-pack-scenarios-postcard.mjs';
import {verifyOwnerEvidence} from './owned-proof.mjs';
import {POSTCARD_OWNER_RECEIPT_BYTES, POSTCARD_LAYOUT, EXPORT_TEST, EXPORT_ARGS, EXPORT_BUILD_ARGS, LIMITS, postcardFactKey, postcardFactPath, postcardIndexIdentity, verifyPostcardFrame, verifyPostcardExporter, preparePostcardExport, verifyPreparedPostcard, runPostcardExport, capturePostcardExport, verifyPostcardCustody, postcardExportSummary} from './read-fact-delta-postcard.mjs';
import {POSTCARD_PREPARATION_POLICY} from './read-fact-delta-postcard.mjs';
import {prepareFreshPostcardExporter} from './source-pack-scenarios-postcard-fresh.mjs';

import {fixture, proof, put, putJSON, emptyFrame} from './read-fact-delta-postcard.fixtures.mjs';

test('frozen Rust empty Postcard golden authenticates framing; no Node schema decoder', () => {
  const input = sha256('empty'), raw = emptyFrame(input); verifyPostcardFrame(raw, input); assert.deepEqual([...raw.subarray(76)], [0, 0, 0, 0, 0]);
  for (const offset of [0, 8, 12, 44, 76]) { const bad = Buffer.from(raw); bad[offset] ^= 1; assert.throws(() => verifyPostcardFrame(bad, input)); }
  assert.throws(() => verifyPostcardFrame(Buffer.concat([raw, Buffer.from([0])]), input));
});
test('explicit v13 identity matches the current Rust key, pack domain and v19 index', () => {
  const source = fs.readFileSync(new URL('../../crates/lycheedev-source/src/index.rs', import.meta.url), 'utf8');
  assert.match(source, /rust-index-v19\\0assets-v2-go323-metadata\\0facts-pack-v2-path-radix512\\0facts-layout-v13-postcard-1\.1\.3/);
  const key = /fn facts_cache_key[\s\S]*?"([^"\n]+)"/.exec(source)?.[1], doc = {path: 'A.lua', sha256: sha256('source')};
  assert.equal(sha256(key.replaceAll('\\0', '\0').replace('{path}', doc.path).replace('{sha256}', doc.sha256)), postcardFactKey(doc));
  const pack = fs.readFileSync(new URL('../../crates/lycheedev-source/src/index/facts_cache.rs', import.meta.url), 'utf8'); assert.match(pack, /facts-pack-v2-path-radix512\\0facts-layout-v13-postcard-1\.1\.3/);
});
test('one authenticated same-source invocation compares all five complete arrays and preserves duplicates', t => fixture(t, async f => {
  const prepared = f.prepare(), token = f.run(prepared), delta = await f.read(token); assert.equal(f.calls(), 1); assert.deepEqual(delta.differentFiles, []); assert.equal(delta.totals.rust.loads, 9); assert.equal(delta.documents, 3);
  const custody = capturePostcardExport(token, f.evidence); assert.equal(custody.references.length, 3); verifyPostcardCustody(custody); assert(custody.references.every(ref => ref.manifest.partBytes === PART_BYTES));
  const part = join(custody.references[0].directory, custody.references[0].manifest.parts[0].path); fs.appendFileSync(part, 'changed'); assert.throws(() => verifyPostcardCustody(custody));
}));
test('executable full comparison port binds independent Go custody and v13 byte custody', t => fixture(t, async f => {
  const go = await f.custody(); const result = await comparePostcardFacts({root: f.root, candidate: f.build.candidate, exporter: f.build.expected, rustHome: f.home, index: f.index, go, scratch: f.scratch, evidence: f.evidence}, {execute: f.execute});
  assert.equal(result.fullFieldsAndOrderEqual, true); assert.equal(result.documents, 3); assert.equal(f.calls(), 1); assert(fs.existsSync(result.custody));
}));
test('wrong source, wrong binary, wrong build recipe and changed owner logs reject before launch', async t => {
  for (const mutation of [f => { f.build.candidate.sourceTreeSHA256 = '0'.repeat(64); }, f => put(f.build.sourceBinary, 'replaced'), f => { f.build.artifact.command = ['cargo', 'build']; putJSON(f.build.expected.artifact, f.build.artifact); f.build.expected.artifactSHA256 = hashFile(f.build.expected.artifact); }, f => put(join(f.build.evidence, 'stdout.bin'), 'changed')]) await fixture(t, f => { const prepared = f.prepare(); mutation(f); assert.throws(() => f.run(prepared)); assert.equal(f.calls(), 0); });
});
test('exporter requires separately sealed actual compiler observations and rejects missing or altered identity', async t => {
  for (const change of ['missing', 'record-SHA', 'owner-SHA', 'compiler-binary', 'policy', 'unreaped', 'duplicate-observation']) await fixture(t, f => {
    const ref = f.build.artifact.toolchain;
    if (change === 'missing') delete f.build.artifact.toolchain;
    else if (change === 'record-SHA') ref.recordSHA256 = '0'.repeat(64);
    else if (change === 'owner-SHA') ref.receiptSHA256 = '0'.repeat(64);
    else if (change === 'compiler-binary') put(f.build.compiler, 'changed compiler');
    else if (change === 'policy') putJSON(join(f.root, 'release/tools/rust.json'), {rustc: 'stable'});
    else if (change === 'unreaped') {
      const file = fs.readdirSync(f.build.compilerEvidence).find(n => n.startsWith('process-') && n.endsWith('.receipt.json'));
      const value = JSON.parse(fs.readFileSync(join(f.build.compilerEvidence, file))); value.treeReaped = false; putJSON(join(f.build.compilerEvidence, file), value);
      f.build.sealCompiler(); ref.receiptSHA256 = hashFile(ref.receipt);
    } else {
      const record = JSON.parse(fs.readFileSync(ref.record)); record.observations[1] = structuredClone(record.observations[0]); record.observations[1].label = 'after-build'; putJSON(ref.record, record);
      f.build.sealCompiler(); ref.recordSHA256 = hashFile(ref.record); ref.receiptSHA256 = hashFile(ref.receipt);
    }
    putJSON(f.build.expected.artifact, f.build.artifact); f.build.expected.artifactSHA256 = hashFile(f.build.expected.artifact);
    assert.throws(() => f.run(f.prepare())); assert.equal(f.calls(), 0);
  });
});
test('wrong output SHA, incomplete/cancelled/limited/reap failure never yields an export', async t => {
  await fixture(t, f => { f.mutateSummary = s => { s.outputSHA256 = '0'.repeat(64); }; assert.throws(() => f.run(f.prepare()), /output SHA/); });
  for (const flags of [{cancelled: true}, {canceled: true}, {deadlineHit: true}, {outputLimit: true}, {rootReaped: false}, {forced: true}]) await fixture(t, f => { f.proofOverrides = flags; assert.throws(() => f.run(f.prepare()), /incomplete/); assert.equal(f.calls(), 1); });
  assert.throws(() => postcardExportSummary({status: 0, handlesReaped: true, proof: proof(), stdout: Buffer.from('running 1 test'), stderr: Buffer.alloc(0)}, {}), /completion/);
  await fixture(t, f => { f.proofOverrides = {binary: join(f.root, 'different.exe')}; assert.throws(() => f.run(f.prepare()), /executable differs/); });
});
test('missing, extra, duplicate and reordered exported document records reject', async t => {
  for (const mutate of [rows => rows.pop(), rows => rows.push(rows[0]), rows => { rows[1] = structuredClone(rows[0]); }, rows => rows.reverse()]) await fixture(t, async f => { f.mutateRows = mutate; await assert.rejects(f.read(f.run(f.prepare())), /missing|extra|order/); });
});
test('full-array field, duplicate multiplicity and row-order mismatches remain observable', async t => {
  for (const [mutate, differentOrder] of [[facts => facts.loads.reverse(), true], [facts => facts.loads.pop(), false], [facts => { facts.declarations[0].scope = 'new'; }, false], [facts => { facts.diagnostics[0].line++; }, false]]) await fixture(t, async f => { f.mutateRows = rows => mutate(rows[0].facts); const delta = await f.read(f.run(f.prepare())); assert.equal(delta.differentFiles.length, 1); assert.equal(delta.firstDifferences[0].sameMultisetButDifferentOrder, differentOrder); });
});
test('missing array, unsafe integer and caller-invented export reader cannot bypass authentication', async t => {
  for (const mutate of [facts => { delete facts.headers; }, facts => { facts.loads[0].line = Number.MAX_SAFE_INTEGER + 1; }]) await fixture(t, async f => { f.mutateRows = rows => mutate(rows[0].facts); await assert.rejects(f.read(f.run(f.prepare()))); });
  await fixture(t, async f => { await assert.rejects(f.read({summary: {}, output: 'caller chosen'}), /authenticated same-source/); assert.throws(() => readRustFact(f.home, f.documents[0], POSTCARD_LAYOUT), /batch exporter/); });
});
test('changed pack, index, manifest and exporter binary are rejected at custody boundaries', async t => {
  for (const mutate of [f => fs.appendFileSync(postcardFactPath(f.home, f.documents[0]), 'bad'), f => fs.appendFileSync(f.index.path, 'bad'), (f, p) => fs.appendFileSync(p.manifest, 'bad')]) await fixture(t, f => { const p = f.prepare(); mutate(f, p); assert.throws(() => verifyPreparedPostcard(p)); assert.throws(() => f.run(p)); assert.equal(f.calls(), 0); });
  await fixture(t, f => { f.afterExecute = () => put(f.build.sourceBinary, 'replaced during execution'); assert.throws(() => f.run(f.prepare())); });
});
test('wrong v17 index recipe and overlapping pack record ranges are refused', async t => {
  await fixture(t, f => { f.index.manifest.identity = '0'.repeat(64); assert.throws(() => f.prepare(), /v17/); });
  await fixture(t, f => { const raw = JSON.parse(fs.readFileSync(f.index.path)); raw.documents[1].factsLocator.offset = raw.documents[0].factsLocator.offset; putJSON(f.index.path, raw); f.index.bytes = fs.statSync(f.index.path).size; f.index.sha256 = hashFile(f.index.path); f.index.manifest.blob = {bytes: f.index.bytes, sha256: f.index.sha256}; assert.throws(() => f.prepare()); });
});
test('explicit configuration and historical reader defaults cannot select v13 accidentally', t => fixture(t, async f => {
  assert.throws(() => validatePostcardRequest({schema: 'lycheedev.source-postcard-comparison-config.v1', layout: 'v11-pack2'}));
  await assert.rejects(readFactDelta(f.goHome, f.home, f.records), /ENOENT/);
  await assert.rejects(readFactDelta(f.goHome, f.home, f.records, 'v11-pack2', f.index.path, f.index.sha256, undefined, {postcardExport: {}}), /Historical Facts layout/);
  assert.equal(LIMITS.total, 128 << 20); assert.equal(LIMITS.manifest, 8 << 20); assert.equal(PART_BYTES, 8 << 20);
}));

test('current preparation requires new bounded identity while historical one-shot receipts remain readable', t => fixture(t, f => {
  const {build} = f, read = path => JSON.parse(fs.readFileSync(path));
  verifyPostcardExporter(f.root, build.expected, build.candidate);
  const request = {ownerRoot: f.root, candidate: build.candidate, exporter: {mode: 'existing', ...build.expected}};
  assert.throws(() => prepareFreshPostcardExporter(request), /current exporter preparation policy required/);
  build.artifact.preparationPolicy = POSTCARD_PREPARATION_POLICY;
  const saveArtifact = () => {
    build.artifact.toolchain.receiptSHA256 = hashFile(build.artifact.toolchain.receipt);
    putJSON(build.expected.artifact, build.artifact); build.expected.artifactSHA256 = hashFile(build.expected.artifact);
    request.exporter = {mode: 'existing', ...build.expected};
  };
  saveArtifact();
  assert.throws(() => verifyPostcardExporter(f.root, build.expected, build.candidate), 'a marker cannot relabel one-shot history');
  const commandPath = join(build.evidence, 'command.json'), command = read(commandPath);
  const nativePath = fs.readdirSync(build.evidence).find(name => name.endsWith('.request.json'));
  const nativeFile = join(build.evidence, nativePath), native = read(nativeFile);
  Object.assign(command, {oneShot: false, timeoutMs: 600000}); native.timeoutMs = 600000;
  putJSON(commandPath, command); putJSON(nativeFile, native); build.seal();
  const cargoReceipt = read(build.artifact.buildEvidence); cargoReceipt.admission = {limits: defaults}; putJSON(build.artifact.buildEvidence, cargoReceipt);
  const compilerReceipt = read(build.artifact.toolchain.receipt); compilerReceipt.admission = {limits: defaults}; putJSON(build.artifact.toolchain.receipt, compilerReceipt); saveArtifact();
  const prepared = prepareFreshPostcardExporter(request);
  assert.equal(prepared.preparation.built, false);
  assert.equal(prepared.preparation.identity.artifact.preparationPolicy, POSTCARD_PREPARATION_POLICY);
  native.timeoutMs = 900000; putJSON(nativeFile, native);
  const resealCargo = () => {
    cargoReceipt.sealedLogs = cargoReceipt.sealedLogs.map(row => ({...row, sha256: hashFile(row.path)}));
    cargoReceipt.logHashes = Object.fromEntries(cargoReceipt.sealedLogs.map(row => [relative(f.root, row.path), row.sha256]));
    putJSON(build.artifact.buildEvidence, cargoReceipt);
  };
  resealCargo(); assert.throws(() => prepareFreshPostcardExporter(request), /current exporter owned deadline/);
  native.timeoutMs = 600000; putJSON(nativeFile, native); resealCargo();
  cargoReceipt.admission.limits = {...defaults, scratchBytes: 6 * 1024 ** 3}; putJSON(build.artifact.buildEvidence, cargoReceipt);
  assert.throws(() => prepareFreshPostcardExporter(request), /Cargo admission differs/, 'explicit 6 GiB exporter cannot be reused by default 5 GiB preparation');
}));


test('Postcard compiler and build owner receipts use the explicit 16 MiB ceiling while historical defaults stay 2 MiB', t => fixture(t, f => {
  for (const receipt of [f.build.artifact.toolchain.receipt, f.build.artifact.buildEvidence]) {
    fs.appendFileSync(receipt, ' '.repeat(2 << 20));
    assert(fs.statSync(receipt).size > 2 << 20);
    assert.throws(() => verifyOwnerEvidence(f.root, receipt), /owned-proof.invalid_file/);
  }
  f.build.artifact.toolchain.receiptSHA256 = hashFile(f.build.artifact.toolchain.receipt);
  putJSON(f.build.expected.artifact, f.build.artifact); f.build.expected.artifactSHA256 = hashFile(f.build.expected.artifact);
  assert.equal(verifyPostcardExporter(f.root, f.build.expected, f.build.candidate).buildReceiptSHA256, hashFile(f.build.artifact.buildEvidence));
  fs.truncateSync(f.build.artifact.buildEvidence, POSTCARD_OWNER_RECEIPT_BYTES + 1);
  assert.throws(() => verifyPostcardExporter(f.root, f.build.expected, f.build.candidate), /owned-proof.invalid_file/);
  for (const receiptMaximumBytes of [0, -1, NaN, Infinity, 0.5, POSTCARD_OWNER_RECEIPT_BYTES + 1]) assert.throws(() => verifyOwnerEvidence(f.root, f.build.artifact.toolchain.receipt, undefined, {receiptMaximumBytes}), /owner_receipt_budget/);
}));
