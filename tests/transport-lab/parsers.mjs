import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const HEAD = 'LYCHEE_MEMORY_LAB2_HEAD|';
const BODY = 'LYCHEE_MEMORY_LAB2_BODY|';
const cli = join(process.env.APPDATA ?? '', 'npm', 'node_modules', 'wowdump', 'dist', 'cli.js');
const ids = ['REQ-LAB2-A', 'REQ-LAB2-B', 'REQ-LAB2-C', 'REQ-LAB2-D', 'REQ-LAB2-E'];
const plans = [
  ['loaded', 'running', 'reported', 'none', 'none'],
  ['running', 'reported', 'reported', 'none', 'none'],
  ['reported', 'acknowledged', 'reported', 'none', 'none'],
  ['acknowledged', 'acknowledged', 'acknowledged', 'loaded', 'running'],
  ['acknowledged', 'acknowledged', 'acknowledged', 'reported', 'reported'],
  ['acknowledged', 'acknowledged', 'acknowledged', 'acknowledged', 'acknowledged'],
];
const sizes = [1024, 65536, 131072, 524288, 4096];
const kinds = ['ascii', 'utf8', 'binary', 'binary', 'duplicate'];
const born = [2, 1, 0, 4, 4];

function adler32(bytes) {
  let a = 1, b = 0;
  for (const value of bytes) { a = (a + value) % 65521; b = (b + a) % 65521; }
  return ((b * 65536 + a) >>> 0).toString(16).padStart(8, '0');
}
function expectedPayload(index, epoch) {
  let block;
  if (index === 0) block = Buffer.from(`probeStatus=completed;result=hello;ticket=A;generation=${epoch};`);
  else if (index === 1) block = Buffer.from(`探针结果：法术、目标、秘密值边界；generation=${epoch};`);
  else if (index === 4) block = Buffer.from('same-content-across-generations;');
  else {
    block = Buffer.alloc(256);
    for (let i = 0; i < 256; i++) block[i] = (i * 73 + (index + 1) * 19 + epoch * 11) % 256;
  }
  return Buffer.alloc(sizes[index], 0).map((_, i) => block[i % block.length]);
}
function parseHead(bytes) {
  const text = bytes.toString('utf8');
  const match = /^LYCHEE_MEMORY_LAB2_HEAD\|([0-9a-f]{16})\|([0-5])\|([^|]+)\|END\|\1\|\2/.exec(text);
  if (!match) return null;
  const [full, run, epochText, rowText] = match;
  const epoch = Number(epochText);
  const rows = rowText.split(';').map((row, index) => {
    const fields = row.split(',');
    if (fields.length !== 6 || fields[0] !== ids[index] || fields[1] !== plans[epoch][index] ||
      fields[2] !== kinds[index] || !/^\d+$/.test(fields[3]) || !/^[0-9a-f]{8}$/.test(fields[4]) ||
      fields[5] !== (fields[1] === 'reported' ? String(born[index]) : '-')) return null;
    return { id: fields[0], state: fields[1], kind: fields[2], bytes: Number(fields[3]), checksum: fields[4],
      reportEpoch: fields[5] === '-' ? null : Number(fields[5]) };
  });
  if (rows.length !== ids.length || rows.includes(null) || rows.some((r, i) =>
    r.bytes !== (r.state === 'reported' ? sizes[i] : 0) ||
    r.checksum !== (r.state === 'reported' ? adler32(expectedPayload(i, born[i])) : '00000000'))) return null;
  return { run, epoch, rows, byteLength: Buffer.byteLength(full) };
}
function parseBody(bytes) {
  const opening = bytes.toString('latin1', 0, Math.min(180, bytes.length));
  const match = /^LYCHEE_MEMORY_LAB2_BODY\|([0-9a-f]{16})\|(REQ-LAB2-[A-E])\|([0-5])\|(ascii|utf8|binary|duplicate)\|(\d{1,6})\|([0-9a-f]{8})\|/.exec(opening);
  if (!match) return null;
  const [, run, id, epochText, kind, sizeText, checksum] = match;
  const epoch = Number(epochText), index = ids.indexOf(id), size = Number(sizeText);
  if (kind !== kinds[index] || size !== sizes[index]) return null;
  const trailer = Buffer.from(`|END|${run}|${id}|${epoch}`, 'ascii');
  const total = Buffer.byteLength(match[0]) + size + trailer.length;
  if (bytes.length < total || !bytes.subarray(total - trailer.length, total).equals(trailer)) return null;
  const payload = bytes.subarray(Buffer.byteLength(match[0]), Buffer.byteLength(match[0]) + size);
  if (adler32(payload) !== checksum || !payload.equals(expectedPayload(index, epoch))) return null;
  return { run, id, epoch, kind, size, checksum, total,
    sha256: createHash('sha256').update(payload).digest('hex') };
}
function command(args, timeout = 180_000) {
  const run = spawnSync(process.execPath, [cli, ...args], { encoding: 'utf8', timeout, maxBuffer: 32 * 1024 * 1024 });
  if (run.error || run.status !== 0) throw new Error((run.stderr || run.stdout || String(run.error)).slice(0, 1000));
  return JSON.parse(run.stdout);
}
function readAt(pid, address, size) {
  const result = command(['memory', 'read', '--pid', String(pid), '--address', address, '--size', String(size)]);
  if (!result.ok || !result.complete || result.bytesRead !== size) return null;
  return Buffer.from(result.dataHex, 'hex');
}
function find(pid, text, range) {
  const args = ['memory', 'find', '--pid', String(pid), '--text', text,
    '--max-hits', '256', '--context', '64', '--read-budget', String(16 * 1024 ** 3)];
  if (range) args.push('--from', range[0], '--to', range[1]);
  else args.push('--confirm');
  return command(args, 240_000);
}
function regionFor(pid, address) {
  const regions = command(['memory', 'regions', '--pid', String(pid), '--max-regions', '100000']);
  const value = BigInt(address);
  const region = regions.regions.find(r => value >= BigInt(r.baseAddress) &&
    value < BigInt(r.baseAddress) + BigInt(r.regionSize));
  if (!region) throw new Error('head region unavailable');
  return { range: [region.baseAddress,
    `0x${(BigInt(region.baseAddress) + BigInt(region.regionSize)).toString(16)}`],
    size: region.regionSize, type: region.type, protect: region.protect };
}
function fixture(dir) {
  const files = readdirSync(dir);
  const results = [];
  for (let epoch = 0; epoch <= 5; epoch++) {
    const head = parseHead(readFileSync(join(dir, `head-${epoch}.bin`)));
    if (!head || head.epoch !== epoch) throw new Error(`bad fixture head ${epoch}`);
    const reports = [];
    for (const row of head.rows.filter(row => row.state === 'reported')) {
      const file = `${row.id}-${epoch}.bin`;
      if (!files.includes(file)) throw new Error(`missing ${file}`);
      const body = parseBody(readFileSync(join(dir, file)));
      if (!body || body.id !== row.id || body.run !== head.run || body.epoch !== row.reportEpoch ||
        body.size !== row.bytes || body.checksum !== row.checksum) throw new Error(`bad ${file}`);
      reports.push({ id: body.id, bytes: body.size, sha256: body.sha256 });
    }
    results.push({ epoch, reported: reports, states: head.rows.map(r => r.state) });
  }
  console.log(JSON.stringify({ ok: true, offline: true, runs: results }, null, 2));
}


export { parseHead, parseBody, readAt, regionFor, command };
