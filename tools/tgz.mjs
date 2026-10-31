// Minimal dependency-free reader for npm pack tarballs (.tgz = gzip + ustar).
// Handles the tar subset npm-packlist produces: ustar headers, PAX extended
// headers (long/unicode paths) and GNU long names.
import { gunzipSync } from 'node:zlib';

function parseOctal(buffer, offset, length) {
  const text = buffer.subarray(offset, offset + length).toString('ascii').replace(/\0.*$/, '').trim();
  if (text === '') return 0;
  const value = Number.parseInt(text, 8);
  if (!Number.isInteger(value)) throw new Error(`tar.invalid_size_field`);
  return value;
}

/**
 * @param {Buffer} buffer raw .tgz bytes
 * @returns {{name: string, bytes: Buffer, mode: number, type: string}[]}
 */
export function readTgz(buffer) {
  const tar = gunzipSync(buffer);
  const entries = [];
  let offset = 0;
  let longName = null;
  let paxOverrides = null;
  while (offset + 512 <= tar.length) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every(byte => byte === 0)) break; // end-of-archive padding
    const rawName = header.subarray(0, 100).toString('utf8').replace(/\0.*$/, '');
    const prefix = header.subarray(345, 500).toString('utf8').replace(/\0.*$/, '');
    const size = parseOctal(header, 124, 12);
    const mode = parseOctal(header, 100, 8);
    const type = String.fromCharCode(header[156]);
    const dataStart = offset + 512;
    const dataEnd = dataStart + size;
    if (dataEnd > tar.length) throw new Error('tar.truncated_entry');
    const data = tar.subarray(dataStart, dataEnd);
    offset = dataStart + Math.ceil(size / 512) * 512;
    if (type === 'L') { longName = data.toString('utf8').replace(/\0.*$/, ''); continue; }
    if (type === 'x' || type === 'X') {
      paxOverrides = {};
      let cursor = 0;
      while (cursor < data.length) {
        const space = data.indexOf(0x20, cursor);
        if (space < 0) break;
        const recordLength = Number.parseInt(data.subarray(cursor, space).toString('ascii'), 10);
        if (!Number.isInteger(recordLength) || recordLength <= 0) throw new Error('tar.invalid_pax_record');
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
    if (entries.some(entry => entry.name === name)) throw new Error(`tar.duplicate_entry: ${name}`);
    entries.push({ name, bytes: Buffer.from(data), mode, type });
  }
  return entries;
}
