import test from 'node:test';
import assert from 'node:assert/strict';
import {closeSync, existsSync, lstatSync, mkdirSync, mkdtempSync, openSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync, writeSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {beginScratch, sha256} from './rust-disk.mjs';
import {hashFile} from './rust-build.mjs';
import {authenticatePrior, INDEX_BYTES, PART_BYTES, retainIndexEvidence, splitIndexEvidence, verifyParts} from './rust-source-pack-evidence.mjs';
const id = '6d9d5ad5-01ec-4a9c-a248-85020e1e62cb';
function fixture(t, length = 100) {
  const root = mkdtempSync(join(tmpdir(), 'source-index-custody-'));
  t.after(() => { if (!t.passed) console.error(`retained fixture: ${root}`); else rmSync(root, {recursive: true}); });
  const directory = join(root, '.tmp/rust-scratch', id), evidence = join(root, '.tmp/rust-disk/evidence', id);
  mkdirSync(directory, {recursive: true}); mkdirSync(evidence, {recursive: true});
  const source = join(evidence, 'cold-index.json'), fd = openSync(source, 'wx'), chunk = Buffer.alloc(65536, 0x61);
  try { for (let n = 0; n < length; n += chunk.length) writeSync(fd, chunk, 0, Math.min(chunk.length, length - n)); } finally { closeSync(fd); }
  const receipt = join(evidence, 'receipt.json');
  writeFileSync(receipt, JSON.stringify({schema: 'lycheedev.disk-owner.v1', id, owner: 'source-pack', directory, state: 'active', activePid: 1234}));
  return {root, directory, evidence, source, receipt, config: {schema: 'lycheedev.source-index-custody-config.v1', root, source: {path: source, bytes: length, sha256: hashFile(source)}, priorOwner: {id, receiptSHA256: hashFile(receipt)}}};
}

test('source index custody streams the real 82228560-byte size into ten bounded ordered parts without changing the original', t => {
  const f = fixture(t, 82228560), out = join(f.root, 'parts'), before = readFileSync(f.receipt);
  const manifest = splitIndexEvidence(f.root, f.config.source, out);
  assert.equal(manifest.parts.length, 10); assert(manifest.parts.every(p => p.bytes <= PART_BYTES && p.bytes <= (16 << 20)));
  assert.deepEqual(verifyParts(out, manifest), {bytes: 82228560, sha256: f.config.source.sha256, parts: 10});
  assert.equal(hashFile(f.source), f.config.source.sha256); assert.deepEqual(readFileSync(f.receipt), before);
  assert.equal(JSON.parse(readFileSync(join(out, 'manifest.json'))).source.sha256, f.config.source.sha256);
  assert.throws(() => splitIndexEvidence(f.root, f.config.source, out), /new parts directory/);
});

test('source index parts cover empty exact-boundary and boundary-plus-one files', t => {
  for (const length of [0, PART_BYTES, PART_BYTES + 1]) {
    const f = fixture(t, length), out = join(f.root, 'parts');
    const manifest = splitIndexEvidence(f.root, f.config.source, out);
    assert.equal(manifest.parts.length, Math.max(1, Math.ceil(length / PART_BYTES))); assert.equal(verifyParts(out, manifest).bytes, length);
  }
});

test('source index parts reject ordering offsets missing parts unexpected fields and whole or per-part corruption', t => {
  const f = fixture(t, PART_BYTES + 1), out = join(f.root, 'parts'), manifest = splitIndexEvidence(f.root, f.config.source, out);
  for (const change of [m => m.parts.reverse(), m => m.parts[1].offset++, m => m.parts.pop(), m => m.parts[0].path = '../cold-index.json', m => m.source.sha256 = '0'.repeat(64), m => m.parts[0].sha256 = '0'.repeat(64), m => m.parts[0].extra = 1, m => m.partBytes *= 2]) {
    const copy = structuredClone(manifest); change(copy); assert.throws(() => verifyParts(out, copy));
  }
  writeFileSync(join(out, manifest.parts[1].path), 'b'); assert.throws(() => verifyParts(out, manifest), /part hash/);
  writeFileSync(join(out, manifest.parts[1].path), 'bb'); assert.throws(() => verifyParts(out, manifest), /type\/size/);
  rmSync(join(out, manifest.parts[1].path)); assert.throws(() => verifyParts(out, manifest), /ENOENT/);
});

test('source index custody rejects input hash size owner receipt and outside paths before creating a new owner', t => {
  const f = fixture(t), ownerIds = readdirSync(join(f.root, '.tmp/rust-disk/evidence'));
  for (const change of [c => c.source.sha256 = '0'.repeat(64), c => c.source.bytes++, c => c.source.bytes = INDEX_BYTES + 1, c => c.priorOwner.receiptSHA256 = '0'.repeat(64), c => c.source.extra = true, c => c.priorOwner.id = '../escape']) {
    const config = structuredClone(f.config); change(config); assert.throws(() => retainIndexEvidence(config));
  }
  const outside = join(f.root, 'outside'); writeFileSync(outside, readFileSync(f.source));
  assert.throws(() => authenticatePrior({...f.config, source: {...f.config.source, path: outside}}), /original owner/);
  assert.deepEqual(readdirSync(join(f.root, '.tmp/rust-disk/evidence')), ownerIds);
});

test('source index custody rejects directory reparse boundaries and non-regular source files', t => {
  const f = fixture(t), link = join(f.evidence, 'link'); symlinkSync(f.evidence, link, 'junction');
  assert.throws(() => authenticatePrior({...f.config, source: {...f.config.source, path: join(link, 'cold-index.json')}}), /reparse/);
  assert.throws(() => splitIndexEvidence(f.root, {...f.config.source, path: f.directory}, join(f.root, 'parts')), /type\/size/);
});

test('source index custody seals only its new owner and leaves the failed-to-seal original receipt byte-identical', t => {
  const f = fixture(t, 82228560), before = readFileSync(f.receipt), sourceSHA = hashFile(f.source);
  const report = retainIndexEvidence(f.config), receipt = JSON.parse(readFileSync(report.receipt));
  assert.equal(report.state, 'authenticated_copy'); assert.equal(report.prior.observedState, 'active'); assert.equal(report.prior.observedHandlesReaped, null);
  assert.equal(report.originalOwnerModified, false); assert.equal(report.originalOwnerRetired, false); assert.equal(report.productCalls, 0);
  assert.notEqual(receipt.id, id); assert.equal(receipt.state, 'succeeded'); assert.equal(receipt.retention, 'preserve'); assert.equal(receipt.handlesReaped, true);
  assert(receipt.sealedLogs.length >= 4); assert(receipt.sealedLogs.every(log => log.original !== f.source && lstatSync(log.path).size <= (16 << 20)));
  for (const log of receipt.sealedLogs) assert.equal(sha256(readFileSync(log.path)), log.sha256);
  assert.deepEqual(readFileSync(f.receipt), before); assert.equal(hashFile(f.source), sourceSHA); assert(existsSync(f.directory));
});


test('source index custody preserves partial new evidence and seals failure without touching original state', t => {
  const f = fixture(t), original = readFileSync(f.receipt); let job;
  assert.throws(() => retainIndexEvidence(f.config, {
    begin: (...args) => (job = beginScratch(...args)),
    split: (_root, _source, output) => { mkdirSync(output); writeFileSync(join(output, 'index.part000.bin'), 'partial'); throw new Error('fixture interrupted write'); },
  }), /fixture interrupted write/);
  const receipt = JSON.parse(readFileSync(job.receipt)); assert.equal(receipt.state, 'failed'); assert.equal(receipt.handlesReaped, true);
  assert.equal(readFileSync(join(job.evidence, 'index-parts/index.part000.bin'), 'utf8'), 'partial');
  assert(!existsSync(join(job.evidence, 'index-parts/manifest.json'))); assert(!existsSync(join(job.evidence, 'custody.json')));
  assert.match(JSON.parse(readFileSync(join(job.evidence, 'custody-failure.json'))).error, /interrupted write/);
  assert.deepEqual(readFileSync(f.receipt), original);
});

test('source index custody propagates a new-owner seal failure and never fabricates a terminal receipt', t => {
  const f = fixture(t), original = readFileSync(f.receipt); let job;
  assert.throws(() => retainIndexEvidence(f.config, {begin: (...args) => { job = beginScratch(...args); return {...job, seal() { throw new Error('fixture seal rejected'); }}; }}), /fixture seal rejected/);
  assert.equal(JSON.parse(readFileSync(job.receipt)).state, 'active'); assert.equal(JSON.parse(readFileSync(job.receipt)).handlesReaped, undefined);
  assert(existsSync(join(job.evidence, 'index-parts/manifest.json'))); assert.deepEqual(readFileSync(f.receipt), original);
});
