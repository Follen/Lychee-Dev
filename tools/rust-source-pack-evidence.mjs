// Read-only custody of an existing Source index. Never adopts or retires its owner.
import assert from 'node:assert/strict';
import {closeSync, existsSync, fstatSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, readSync, realpathSync, readdirSync, writeFileSync, writeSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join, resolve, sep} from 'node:path';
import {beginScratch, contained} from './rust-disk.mjs';
import {hashFile} from './rust-build.mjs';

export const PART_BYTES = 8 << 20; // Existing native-acceptance evidence part size; seal cap remains 16 MiB.
export const INDEX_BYTES = 128 << 20; // Existing product aggregate-index limit.
const hash = () => createHash('sha256');
const keys = (value, expected) => assert.deepEqual(Object.keys(value).sort(), [...expected].sort(), 'exact evidence fields required');
const digest = value => assert.match(value, /^[0-9a-f]{64}$/);
const bytes = value => assert(Number.isSafeInteger(value) && value >= 0 && value <= INDEX_BYTES, 'index byte budget');
const partName = i => `index.part${String(i).padStart(3, '0')}.bin`;
function regular(path, size) {
  const info = lstatSync(path);
  assert(info.isFile() && !info.isSymbolicLink() && info.size === size, 'source file type/size changed');
  return info;
}
function unchanged(before, after) {
  for (const key of ['dev', 'ino', 'size', 'mtimeMs', 'ctimeMs']) assert.equal(after[key], before[key], `source file changed: ${key}`);
}
function writeAll(fd, data) {
  let offset = 0;
  while (offset < data.length) { const count = writeSync(fd, data, offset, data.length - offset); assert(count > 0, 'short evidence write'); offset += count; }
}

export function verifyParts(directory, manifest) {
  keys(manifest, ['schema', 'source', 'partBytes', 'parts']);
  assert.equal(manifest.schema, 'lycheedev.source-index-parts.v1');
  keys(manifest.source, ['path', 'bytes', 'sha256']); bytes(manifest.source.bytes); digest(manifest.source.sha256);
  assert.equal(typeof manifest.source.path, 'string'); assert.equal(manifest.partBytes, PART_BYTES);
  assert(Array.isArray(manifest.parts));
  assert.equal(manifest.parts.length, Math.max(1, Math.ceil(manifest.source.bytes / PART_BYTES)), 'complete ordered parts required');
  const whole = hash(), buffer = Buffer.alloc(65536); let total = 0;
  for (const [index, part] of manifest.parts.entries()) {
    keys(part, ['path', 'offset', 'bytes', 'sha256']); digest(part.sha256);
    assert.equal(part.path, partName(index), 'fixed consecutive part paths required'); assert.equal(part.offset, total);
    assert.equal(part.bytes, Math.min(PART_BYTES, manifest.source.bytes - total));
    const path = contained(directory, join(directory, part.path)), before = regular(path, part.bytes);
    const fd = openSync(path, 'r'), one = hash(); let count = 0;
    try {
      unchanged(before, fstatSync(fd));
      for (;;) {
        const n = readSync(fd, buffer, 0, buffer.length, null); if (!n) break;
        count += n; assert(count <= part.bytes, 'part grew'); one.update(buffer.subarray(0, n)); whole.update(buffer.subarray(0, n));
      }
      assert.equal(count, part.bytes, 'part truncated'); unchanged(before, fstatSync(fd));
    } finally { closeSync(fd); }
    unchanged(before, regular(path, part.bytes)); assert.equal(one.digest('hex'), part.sha256, 'part hash changed'); total += count;
  }
  assert.equal(total, manifest.source.bytes); assert.equal(whole.digest('hex'), manifest.source.sha256, 'whole ordered index hash changed');
  return {bytes: total, sha256: manifest.source.sha256, parts: manifest.parts.length};
}

export function splitIndexEvidence(root, source, directory) {
  keys(source, ['path', 'bytes', 'sha256']); bytes(source.bytes); digest(source.sha256);
  const path = contained(root, resolve(source.path)), before = regular(path, source.bytes);
  assert.equal(hashFile(path, source.bytes), source.sha256, 'original index hash mismatch');
  directory = contained(root, resolve(directory)); assert(!existsSync(directory), 'new parts directory required'); mkdirSync(directory);
  const manifest = {schema: 'lycheedev.source-index-parts.v1', source: {...source, path}, partBytes: PART_BYTES, parts: []};
  const whole = hash(), buffer = Buffer.alloc(65536), input = openSync(path, 'r'); let total = 0;
  try {
    unchanged(before, fstatSync(input));
    do {
      const name = partName(manifest.parts.length), length = Math.min(PART_BYTES, source.bytes - total), one = hash();
      const output = openSync(contained(directory, join(directory, name)), 'wx'); let count = 0;
      try {
        while (count < length) {
          const n = readSync(input, buffer, 0, Math.min(buffer.length, length - count), null); assert(n > 0, 'source truncated');
          const chunk = buffer.subarray(0, n); writeAll(output, chunk); one.update(chunk); whole.update(chunk); count += n;
        }
        fsyncSync(output);
      } finally { closeSync(output); }
      manifest.parts.push({path: name, offset: total, bytes: count, sha256: one.digest('hex')}); total += count;
    } while (total < source.bytes);
    assert.equal(readSync(input, buffer, 0, 1, null), 0, 'source grew'); unchanged(before, fstatSync(input));
  } finally { closeSync(input); }
  unchanged(before, regular(path, source.bytes)); assert.equal(whole.digest('hex'), source.sha256, 'source changed while copying');
  verifyParts(directory, manifest);
  assert.equal(hashFile(path, source.bytes), source.sha256, 'original index changed after copy');
  writeFileSync(join(directory, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', {flag: 'wx'});
  return manifest;
}

export function authenticatePrior(config) {
  keys(config, ['schema', 'root', 'source', 'priorOwner']); assert.equal(config.schema, 'lycheedev.source-index-custody-config.v1');
  keys(config.priorOwner, ['id', 'receiptSHA256']); assert.match(config.priorOwner.id, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/); digest(config.priorOwner.receiptSHA256);
  const root = realpathSync(config.root), id = config.priorOwner.id;
  const receipt = contained(root, join(root, '.tmp/rust-disk/evidence', id, 'receipt.json'));
  const info = lstatSync(receipt); assert(info.isFile() && info.size <= (1 << 20), 'bounded original receipt required');
  assert.equal(hashFile(receipt, 1 << 20), config.priorOwner.receiptSHA256, 'original receipt changed');
  const owner = JSON.parse(readFileSync(receipt)); assert.equal(owner.schema, 'lycheedev.disk-owner.v1'); assert.equal(owner.id, id);
  assert.equal(resolve(owner.directory), join(root, '.tmp/rust-scratch', id));
  const source = contained(root, resolve(config.source.path));
  assert([resolve(owner.directory), join(root, '.tmp/rust-disk/evidence', id)].some(base => source.startsWith(base + sep)), 'source must belong to the original owner');
  keys(config.source, ['path', 'bytes', 'sha256']); bytes(config.source.bytes); digest(config.source.sha256);
  regular(source, config.source.bytes); assert.equal(hashFile(source, config.source.bytes), config.source.sha256, 'original index hash mismatch');
  return {root, receipt, receiptSHA256: config.priorOwner.receiptSHA256, ownerId: id, observedState: owner.state, observedHandlesReaped: owner.handlesReaped ?? null, source: {...config.source, path: source}};
}

export function retainIndexEvidence(config, {begin = beginScratch, split = splitIndexEvidence} = {}) {
  const prior = authenticatePrior(config);
  const job = begin(prior.root, 'source-index-evidence-custody', {command: ['source-index-evidence-custody'], retention: 'preserve',
    inputs: {priorReceipt: prior.receiptSHA256, index: prior.source.sha256, helper: hashFile(import.meta.filename)},
    admissionOptions: {requiredBytes: prior.source.bytes + (1 << 20), scratchReserveBytes: prior.source.bytes + (1 << 20)}});
  let state = 'failed', failure;
  try {
    const directory = join(job.evidence, 'index-parts');
    const manifest = split(prior.root, prior.source, directory);
    assert.deepEqual(authenticatePrior(config), prior, 'original owner or index changed');
    const report = {schema: 'lycheedev.source-index-custody.v1', state: 'authenticated_copy', prior, manifest: join(directory, 'manifest.json'), manifestSHA256: hashFile(join(directory, 'manifest.json'), 65536), verified: verifyParts(directory, manifest), originalOwnerModified: false, originalOwnerRetired: false, productCalls: 0};
    writeFileSync(join(job.evidence, 'custody.json'), JSON.stringify(report, null, 2) + '\n', {flag: 'wx'}); state = 'succeeded';
    return {receipt: job.receipt, report: join(job.evidence, 'custody.json'), ...report};
  } catch (error) {
    failure = error;
    writeFileSync(join(job.evidence, 'custody-failure.json'), JSON.stringify({state, error: error.message, prior, productCalls: 0, originalOwnerModified: false}, null, 2) + '\n', {flag: 'wx'});
    throw error;
  } finally {
    const files = [];
    for (const name of readdirSync(job.evidence)) {
      const path = join(job.evidence, name);
      if (name === 'index-parts') { for (const part of readdirSync(path)) files.push(join(path, part)); }
      else if (name !== 'receipt.json') files.push(path);
    }
    // This new synchronous copy operation creates no child process. This says
    // nothing about the previous owner's process or scratch terminal state.
    try { job.seal({state, handlesReaped: true, logs: files}); }
    catch (error) { if (failure) error.cause = failure; throw error; }
  }
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  const [operation, input, ...extra] = process.argv.slice(2);
  assert(input && !extra.length && ['capture', 'verify'].includes(operation), 'usage: node tools/rust-source-pack-evidence.mjs capture <custody-config.json> OR verify <manifest.json>');
  const info = lstatSync(input); assert(info.isFile() && !info.isSymbolicLink() && info.size <= 65536, 'bounded JSON input required');
  const value = JSON.parse(readFileSync(input));
  if (operation === 'capture') console.log(JSON.stringify(retainIndexEvidence(value), null, 2));
  else console.log(JSON.stringify(verifyParts(resolve(input, '..'), value), null, 2));
}
