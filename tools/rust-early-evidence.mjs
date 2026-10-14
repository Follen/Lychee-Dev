// Snapshot only completed, hash-sealed project CI evidence. Never mutate its source.
import {existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync} from 'node:fs';
import {dirname, isAbsolute, join, relative, resolve, sep} from 'node:path';
import {contained, sha256} from './rust-disk.mjs';
export function snapshotEarlyEvidence(root, output, limits = {files: 2000, bytes: 64 << 20, fileBytes: 16 << 20}) {
  root = resolve(root);
  const source = contained(root, join(root, '.tmp/rust-disk/evidence'));
  const destination = contained(root, resolve(root, output));
  const nested = relative(source, destination);
  if (!nested || (!isAbsolute(nested) && nested !== '..' && !nested.startsWith(`..${sep}`))) throw new Error('early-evidence.destination_within_source');
  if (existsSync(destination)) throw new Error('early-evidence.destination_exists');
  if (!existsSync(source)) throw new Error('early-evidence.source_missing');
  const manifest = {schema: 'lycheedev.early-ci-evidence.v1', files: [], skipped: [], receipts: 0};
  const copies = new Map(); let bytes = 0, entries = 0;
  function readRegular(path, maximum) {
    const file = contained(source, path), stat = lstatSync(file);
    if (!stat.isFile() || stat.size > maximum) throw new Error('early-evidence.file_budget');
    const data = readFileSync(file);
    if (data.length > maximum) throw new Error('early-evidence.file_changed');
    return data;
  }
  function retain(path, data, expected) {
    const name = relative(source, path), hash = sha256(data);
    if (expected !== undefined && hash !== expected) throw new Error('early-evidence.hash_mismatch');
    if (copies.has(name)) {
      if (copies.get(name).sha256 !== hash) throw new Error('early-evidence.conflicting_seal');
      return;
    }
    bytes += data.length;
    if (copies.size + 1 > limits.files || bytes > limits.bytes) throw new Error('early-evidence.total_budget');
    copies.set(name, {data, sha256: hash});
  }
  for (const entry of readdirSync(source, {withFileTypes: true})) {
    if (++entries > 2000) throw new Error('early-evidence.entry_budget');
    if (!entry.isDirectory()) continue;
    const path = contained(source, join(source, entry.name, 'receipt.json'));
    if (!existsSync(path)) continue;
    const data = readRegular(path, 1 << 20), receipt = JSON.parse(data);
    if (!receipt.sealedAt || receipt.handlesReaped !== true || !Array.isArray(receipt.sealedLogs)) {
      manifest.skipped.push({receipt: relative(source, path), reason: 'not_sealed_and_reaped'}); continue;
    }
    if (receipt.sealedLogs.length > limits.files) throw new Error('early-evidence.log_budget');
    for (const row of receipt.sealedLogs) {
      if (typeof row.path !== 'string' || !/^[0-9a-f]{64}$/.test(row.sha256)) throw new Error('early-evidence.invalid_seal');
      const file = contained(dirname(path), resolve(row.path));
      retain(file, readRegular(file, limits.fileBytes), row.sha256);
    }
    retain(path, data); manifest.receipts++;
  }
  // Materialize verified immutable byte snapshots only after every input passes.
  mkdirSync(destination);
  for (const [name, value] of copies) {
    const path = contained(destination, join(destination, name));
    mkdirSync(dirname(path), {recursive: true}); writeFileSync(path, value.data, {flag: 'wx'});
    manifest.files.push({path: name.split('\\').join('/'), bytes: value.data.length, sha256: value.sha256});
  }
  manifest.bytes = bytes;
  writeFileSync(join(destination, 'index.json'), JSON.stringify(manifest, null, 2) + '\n', {flag: 'wx'});
  return manifest;
}
if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  if (process.argv.length !== 2) throw new Error('usage: node tools/rust-early-evidence.mjs');
  console.log(JSON.stringify(snapshotEarlyEvidence(process.cwd(), '.tmp/ci-early-evidence'), null, 2));
}
