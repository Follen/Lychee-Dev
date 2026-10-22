// Deterministic tar.gz writer/reader. Zero dependencies; plain ustar headers
// with fixed metadata (mtime/uid/gid 0, sorted entries) plus gzipSync's zero
// MTIME so identical input always produces identical archive bytes. Read
// support covers regular files plus the name-override headers real tar
// variants emit (ustar prefix splits, GNU long names, PAX path overrides).
import { readFile, writeFile } from 'node:fs/promises';
import { gunzipSync, gzipSync } from 'node:zlib';

const BLOCK = 512;

// ustar numeric fields: zero-padded octal digits with a trailing NUL.
function octal(value, width) {
  return value.toString(8).padStart(width - 1, '0') + '\0';
}

function checkName(name) {
  if (name.startsWith('/') || name.includes('\\') || name.split('/').includes('..')) {
    throw new Error(`tar.invalid_entry_name: ${name}`);
  }
}

// ustar keeps at most 100 name bytes; overflow moves a leading path prefix
// (<= 155 bytes) into the prefix field, split at a '/' so prefix + '/' + name
// reconstructs exactly. Only ASCII '/' can split, so multibyte names are safe.
function splitName(name) {
  const bytes = Buffer.from(name, 'utf8');
  if (bytes.length <= 100) return [Buffer.alloc(0), bytes];
  for (let i = Math.min(155, bytes.length - 2); i >= Math.max(1, bytes.length - 101); i--) {
    if (bytes[i] === 0x2f) return [bytes.subarray(0, i), bytes.subarray(i + 1)];
  }
  throw new Error(`tar.name_too_long: ${name}`);
}

function ustarHeader(prefix, name, size, mode) {
  const header = Buffer.alloc(BLOCK);
  name.copy(header, 0);
  header.write(octal(mode & 0o7777, 8), 100, 'ascii');
  header.write(octal(0, 8), 108, 'ascii'); // uid
  header.write(octal(0, 8), 116, 'ascii'); // gid
  header.write(octal(size, 12), 124, 'ascii');
  header.write(octal(0, 12), 136, 'ascii'); // mtime
  header.write('0', 156, 'ascii'); // typeflag: regular file
  header.write('ustar\0', 257, 'ascii');
  header.write('00', 263, 'ascii');
  header.write(octal(0, 8), 329, 'ascii'); // devmajor
  header.write(octal(0, 8), 337, 'ascii'); // devminor
  prefix.copy(header, 345);
  header.fill(0x20, 148, 156); // checksum is defined over spaces in its own field
  let sum = 0;
  for (const byte of header) sum += byte;
  header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148, 'ascii');
  return header;
}

/**
 * Write a deterministic gzip-compressed ustar archive of regular files.
 * @param {Map<string, Buffer|string>|Array<[string, Buffer|string]|{name: string, bytes: Buffer|string, mode?: number}>} entries
 * @param {string} outPath
 * @returns {Promise<string>} outPath
 */
export async function writeTarGz(entries, outPath) {
  const pairs = entries instanceof Map
    ? [...entries.entries()]
    : [...entries].map(entry => (Array.isArray(entry) ? entry : [entry.name, entry.bytes, entry.mode]));
  const list = pairs.map(([name, value, mode]) => ({
    name,
    key: Buffer.from(name, 'utf8'),
    bytes: Buffer.isBuffer(value) ? value : Buffer.from(value),
    mode: mode ?? 0o644,
  }));
  for (const entry of list) checkName(entry.name);
  list.sort((a, b) => Buffer.compare(a.key, b.key));
  for (let i = 1; i < list.length; i++) {
    if (Buffer.compare(list[i - 1].key, list[i].key) === 0) throw new Error(`tar.duplicate_entry: ${list[i].name}`);
  }
  const blocks = [];
  for (const entry of list) {
    const [prefix, name] = splitName(entry.name);
    blocks.push(ustarHeader(prefix, name, entry.bytes.length, entry.mode), entry.bytes);
    const pad = (BLOCK - (entry.bytes.length % BLOCK)) % BLOCK;
    if (pad) blocks.push(Buffer.alloc(pad));
  }
  blocks.push(Buffer.alloc(BLOCK * 2)); // end-of-archive marker
  await writeFile(outPath, gzipSync(Buffer.concat(blocks), { level: 9 }));
  return outPath;
}

function parseOctal(buffer, offset, length) {
  const text = buffer.subarray(offset, offset + length).toString('ascii').replace(/\0.*$/, '').trim();
  if (text === '') return 0;
  const value = Number.parseInt(text, 8);
  if (!Number.isInteger(value)) throw new Error(`tar.invalid_size_field`);
  return value;
}

/**
 * Read a .tar.gz archive into an ordered name->bytes map of regular files.
 * Handles ustar prefix splits, GNU long names (type `L`) and PAX `x`/`X`
 * headers with a `path` override; skips directories and metadata links.
 * @param {Buffer} buffer raw .tar.gz bytes
 * @returns {Map<string, Buffer>}
 */
export function readTarGz(buffer) {
  const tar = gunzipSync(buffer);
  const result = new Map();
  let offset = 0;
  let longName = null;
  let paxOverrides = null;
  while (offset + BLOCK <= tar.length) {
    const header = tar.subarray(offset, offset + BLOCK);
    if (header.every(byte => byte === 0)) break; // end-of-archive padding
    const rawName = header.subarray(0, 100).toString('utf8').replace(/\0.*$/, '');
    const prefix = header.subarray(345, 500).toString('utf8').replace(/\0.*$/, '');
    const size = parseOctal(header, 124, 12);
    const type = String.fromCharCode(header[156]);
    const dataStart = offset + BLOCK;
    const dataEnd = dataStart + size;
    if (dataEnd > tar.length) throw new Error('tar.truncated_entry');
    const data = tar.subarray(dataStart, dataEnd);
    offset = dataStart + Math.ceil(size / BLOCK) * BLOCK;
    if (type === 'L') { longName = data.toString('utf8').replace(/\0.*$/, ''); continue; }
    if (type === 'x' || type === 'X') {
      paxOverrides = {};
      let cursor = 0;
      while (cursor < data.length) {
        const space = data.indexOf(0x20, cursor);
        if (space < 0) break;
        const recordLength = Number.parseInt(data.subarray(cursor, space).toString('ascii'), 10);
        if (!Number.isInteger(recordLength) || recordLength <= 0) throw new Error(`tar.invalid_pax_record`);
        const record = data.subarray(space + 1, cursor + recordLength - 1).toString('utf8');
        const equals = record.indexOf('=');
        if (equals > 0) paxOverrides[record.slice(0, equals)] = record.slice(equals + 1);
        cursor += recordLength;
      }
      continue;
    }
    let name = longName ?? (prefix ? `${prefix}/${rawName}` : rawName);
    if (paxOverrides?.path) name = paxOverrides.path;
    longName = null;
    paxOverrides = null;
    if (type === '5') continue; // directory entries
    if (type !== '0' && type !== '\0') continue; // skip metadata links
    if (result.has(name)) throw new Error(`tar.duplicate_entry: ${name}`);
    result.set(name, Buffer.from(data));
  }
  return result;
}

/**
 * @param {string} path
 * @returns {Promise<Map<string, Buffer>>}
 */
export async function readTarGzFile(path) {
  return readTarGz(await readFile(path));
}
