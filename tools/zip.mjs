// Deterministic ZIP writer/reader used by tools/addon-package.mjs and
// tools/release.mjs. Zero dependencies; fixed entry metadata so the same input
// bytes produce the same archive digest. Read support covers the subset npm and
// this writer produce (no Zip64, no encryption).
import { readFile, writeFile } from 'node:fs/promises';
import { inflateRawSync, deflateRawSync } from 'node:zlib';

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

export function crc32(buffer) {
  let crc = 0xffffffff;
  for (const byte of buffer) crc = CRC_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

// Fixed DOS timestamp (1980-01-01 00:00:00) keeps archives byte-stable.
const DOS_DATE = (0 << 9) | (1 << 5) | 1;
const DOS_TIME = 0;

/**
 * Write a ZIP archive.
 * @param {{name: string, bytes: Buffer, mode?: number}[]|Map<string, Buffer>} entries
 *   Optional `mode` is the POSIX permission set stored in the external
 *   attributes (default 0o644); native Unix binaries pass 0o755 so the
 *   unpacked file is executable.
 * @param {string} outPath
 */
export async function writeZip(entries, outPath) {
  const pairs = entries instanceof Map
    ? [...entries.entries()].map(([name, value]) => [name, value, undefined])
    : [...entries].map(entry => (Array.isArray(entry) ? [entry[0], entry[1], entry[2]?.mode] : [entry.name, entry.bytes, entry.mode]));
  const list = pairs
    .map(([name, value, mode]) => ({
      name,
      bytes: Buffer.isBuffer(value) ? value : Buffer.from(value),
      mode: mode ?? 0o644,
    }))
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  const local = [];
  const central = [];
  let offset = 0;
  for (const entry of list) {
    if (Buffer.byteLength(entry.name, 'utf8') > 200) throw new Error(`zip.name_too_long: ${entry.name}`);
    const nameBytes = Buffer.from(entry.name, 'utf8');
    const crc = crc32(entry.bytes);
    let method = 8;
    let data = deflateRawSync(entry.bytes, { level: 9 });
    if (data.length >= entry.bytes.length) { method = 0; data = entry.bytes; }
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(0x0800, 6); // UTF-8 names
    header.writeUInt16LE(method, 8);
    header.writeUInt16LE(DOS_TIME, 10);
    header.writeUInt16LE(DOS_DATE, 12);
    header.writeUInt32LE(crc, 14);
    header.writeUInt32LE(data.length, 18);
    header.writeUInt32LE(entry.bytes.length, 22);
    header.writeUInt16LE(nameBytes.length, 26);
    header.writeUInt16LE(0, 28);
    local.push(header, nameBytes, data);
    const record = Buffer.alloc(46);
    record.writeUInt32LE(0x02014b50, 0);
    record.writeUInt16LE(0x0314, 4); // made by: UNIX, spec 3.0 (exec bits below)
    record.writeUInt16LE(20, 6);
    record.writeUInt16LE(0x0800, 8);
    record.writeUInt16LE(method, 10);
    record.writeUInt16LE(DOS_TIME, 12);
    record.writeUInt16LE(DOS_DATE, 14);
    record.writeUInt32LE(crc, 16);
    record.writeUInt32LE(data.length, 20);
    record.writeUInt32LE(entry.bytes.length, 24);
    record.writeUInt16LE(nameBytes.length, 28);
    // External attributes: UNIX made-by, regular file with the entry's
    // permission bits (shifted unsigned: the high bit overflows int32).
    record.writeUInt32LE((((0o100000 | entry.mode) << 16) >>> 0), 38);
    record.writeUInt32LE(offset, 42);
    central.push(record, nameBytes);
    offset += header.length + nameBytes.length + data.length;
  }
  const centralBytes = Buffer.concat(central);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(list.length, 8);
  eocd.writeUInt16LE(list.length, 10);
  eocd.writeUInt32LE(centralBytes.length, 12);
  eocd.writeUInt32LE(offset, 16);
  await writeFile(outPath, Buffer.concat([...local, centralBytes, eocd]));
  return outPath;
}

/**
 * Read a ZIP archive into an ordered name->bytes map.
 * @param {Buffer} buffer
 * @returns {Map<string, Buffer>}
 */
export function readZip(buffer) {
  let eocd = -1;
  for (let i = buffer.length - 22; i >= 0 && i >= buffer.length - 22 - 65558; i--) {
    if (buffer.readUInt32LE(i) === 0x06054b50) { eocd = i; break; }
  }
  if (eocd < 0) throw new Error('zip.end_of_central_directory_missing');
  const count = buffer.readUInt16LE(eocd + 10);
  const centralOffset = buffer.readUInt32LE(eocd + 16);
  const result = new Map();
  let cursor = centralOffset;
  for (let index = 0; index < count; index++) {
    if (buffer.readUInt32LE(cursor) !== 0x02014b50) throw new Error('zip.central_directory_corrupt');
    const method = buffer.readUInt16LE(cursor + 10);
    const compressedSize = buffer.readUInt32LE(cursor + 20);
    const nameLength = buffer.readUInt16LE(cursor + 28);
    const extraLength = buffer.readUInt16LE(cursor + 30);
    const commentLength = buffer.readUInt16LE(cursor + 32);
    const localOffset = buffer.readUInt32LE(cursor + 42);
    const name = buffer.subarray(cursor + 46, cursor + 46 + nameLength).toString('utf8');
    cursor += 46 + nameLength + extraLength + commentLength;
    if (buffer.readUInt32LE(localOffset) !== 0x04034b50) throw new Error(`zip.local_header_corrupt: ${name}`);
    const localNameLength = buffer.readUInt16LE(localOffset + 26);
    const localExtraLength = buffer.readUInt16LE(localOffset + 28);
    const dataStart = localOffset + 30 + localNameLength + localExtraLength;
    const raw = buffer.subarray(dataStart, dataStart + compressedSize);
    const bytes = method === 0 ? Buffer.from(raw) : method === 8 ? inflateRawSync(raw) : (() => { throw new Error(`zip.unsupported_method: ${name}`); })();
    if (result.has(name)) throw new Error(`zip.duplicate_entry: ${name}`);
    result.set(name, bytes);
  }
  return result;
}

export async function readZipFile(path) {
  return readZip(await readFile(path));
}
