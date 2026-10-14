import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, existsSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import test from 'node:test';
import {sha256} from './rust-disk.mjs';
import {snapshotEarlyEvidence} from './rust-early-evidence.mjs';
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'lycheedev-early-evidence-'));
  t.after(() => rmSync(root, {recursive: true, force: true}));
  const evidence = join(root, '.tmp/rust-disk/evidence'), dir = join(evidence, 'owned');
  mkdirSync(dir, {recursive: true});
  const file = join(dir, 'stderr.bin'), receipt = join(dir, 'receipt.json');
  const data = Buffer.from('expected project compile error\n'); writeFileSync(file, data);
  const value = {state: 'failed', sealedAt: '2026-10-14T00:00:00Z', handlesReaped: true, sealedLogs: [{path: file, sha256: sha256(data)}]};
  const save = () => writeFileSync(receipt, JSON.stringify(value)); save();
  return {root, dir, file, receipt, value, save, output: join(root, '.tmp/ci-early-evidence')};
}
test('copies failed but reaped sealed evidence with exact hashes without changing source', t => {
  const f = fixture(t), before = readFileSync(f.receipt);
  const report = snapshotEarlyEvidence(f.root, '.tmp/ci-early-evidence');
  assert.equal(report.receipts, 1); assert.equal(report.files.length, 2);
  assert.deepEqual(readFileSync(f.receipt), before);
  assert.deepEqual(readFileSync(join(f.output, 'owned/stderr.bin')), readFileSync(f.file));
  assert.equal(report.files[0].sha256, sha256(readFileSync(f.file)));
});
test('does not inspect log paths of active or unsealed evidence', t => {
  const f = fixture(t); f.value.handlesReaped = false; f.value.sealedLogs[0].path = join(f.root, 'missing-private'); f.save();
  const report = snapshotEarlyEvidence(f.root, '.tmp/ci-early-evidence');
  assert.equal(report.receipts, 0); assert.equal(report.files.length, 0); assert.equal(report.skipped.length, 1);
});
test('rejects changed sealed output before creating snapshot', t => {
  const f = fixture(t); writeFileSync(f.file, 'changed');
  assert.throws(() => snapshotEarlyEvidence(f.root, '.tmp/ci-early-evidence'), /hash_mismatch/); assert(!existsSync(f.output));
});
test('rejects paths outside evidence even with a matching known hash', t => {
  const f = fixture(t), other = join(f.root, 'outside'); writeFileSync(other, 'outside');
  f.value.sealedLogs[0] = {path: other, sha256: sha256(Buffer.from('outside'))}; f.save();
  assert.throws(() => snapshotEarlyEvidence(f.root, '.tmp/ci-early-evidence'), /outside_owned_root/); assert(!existsSync(f.output));
});
test('never adopts an existing destination', t => {
  const f = fixture(t); mkdirSync(f.output);
  assert.throws(() => snapshotEarlyEvidence(f.root, '.tmp/ci-early-evidence'), /destination_exists/);
});
test('enforces aggregate admission before materializing output', t => {
  const f = fixture(t);
  assert.throws(() => snapshotEarlyEvidence(f.root, '.tmp/ci-early-evidence', {files: 10, bytes: 1, fileBytes: 1000}), /total_budget/);
  assert(!existsSync(f.output));
});

test('rejects a completed receipt borrowing an active sibling log', t => {
  const f = fixture(t), active = join(f.dir, '..', 'active'); mkdirSync(active);
  const live = join(active, 'live.log'); writeFileSync(live, 'still active');
  writeFileSync(join(active, 'receipt.json'), JSON.stringify({handlesReaped: false, sealedLogs: []}));
  f.value.sealedLogs[0] = {path: live, sha256: sha256(Buffer.from('still active'))}; f.save();
  assert.throws(() => snapshotEarlyEvidence(f.root, '.tmp/ci-early-evidence'), /outside_owned_root/); assert(!existsSync(f.output));
});
test('never places a snapshot inside the original evidence tree', t => {
  const f = fixture(t), output = '.tmp/rust-disk/evidence/snapshot';
  assert.throws(() => snapshotEarlyEvidence(f.root, output), /destination_within_source/);
  assert(!existsSync(join(f.root, output)));
});
