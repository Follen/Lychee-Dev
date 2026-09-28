import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';

const MAGIC = process.argv[4] === 'all' ? 'LYCHEE_MEMORY_LAB2_' : process.argv[4] === 'body'
  ? 'LYCHEE_MEMORY_LAB2_BODY|' : 'LYCHEE_MEMORY_LAB2_HEAD|';
const cli = join(process.env.APPDATA ?? '', 'npm', 'node_modules', 'wowdump', 'dist', 'cli.js');
const pid = Number(process.argv[2]);
const workers = Number(process.argv[3] ?? 4);
if (!Number.isSafeInteger(pid) || pid < 1 || !Number.isSafeInteger(workers) || workers < 2 || workers > 8)
  throw new Error('Usage: node parallel-find.mjs <Wow PID> [workers: 2..8]');
if (!existsSync(cli)) throw new Error(`wowdump CLI missing: ${cli}`);

function command(args, timeout = 30_000) {
  const run = spawnSync(process.execPath, [cli, ...args], {
    encoding: 'utf8', timeout, maxBuffer: 24 * 1024 * 1024,
  });
  if (run.error) throw run.error;
  if (run.status !== 0) throw new Error((run.stderr || run.stdout).slice(0, 1200));
  return JSON.parse(run.stdout);
}

function asyncCommand(args, timeout = 240_000) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [cli, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '', stderr = '';
    const timer = setTimeout(() => child.kill(), timeout);
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', chunk => { stdout += chunk; if (stdout.length > 8 * 1024 * 1024) child.kill(); });
    child.stderr.on('data', chunk => { stderr += chunk; if (stderr.length > 1024 * 1024) child.kill(); });
    child.on('error', error => { clearTimeout(timer); reject(error); });
    child.on('close', code => {
      clearTimeout(timer);
      if (code !== 0) reject(new Error((stderr || stdout || `exit ${code}`).slice(0, 1200)));
      else { try { resolve(JSON.parse(stdout)); } catch (error) { reject(error); } }
    });
  });
}

const target = command(['targets', '--pid', String(pid)]).selected;
if (!target || target.pid !== pid || !/^Wow(?:B|T|Classic.*)?\.exe$/i.test(target.name))
  throw new Error('PID is not the selected WoW client');
const regionsResult = command(['memory', 'regions', '--pid', String(pid), '--max-regions', '100000']);
if (regionsResult.truncated) throw new Error('region enumeration truncated');
const regions = regionsResult.regions
  .filter(region => region.committed &&
    [2, 4, 8, 0x20, 0x40, 0x80].includes(Number(region.protect) & 0xff) &&
    !(Number(region.protect) & 0x100))
  .map(region => ({ start: BigInt(region.baseAddress),
    end: BigInt(region.baseAddress) + BigInt(region.regionSize),
    bytes: Number(region.regionSize) }))
  .sort((a, b) => a.start < b.start ? -1 : a.start > b.start ? 1 : 0);
const total = regions.reduce((sum, region) => sum + region.bytes, 0);
if (!regions.length || total > 32 * 1024 ** 3) throw new Error('unexpected memory coverage');
const groups = [];
let offset = 0;
for (let worker = 0; worker < workers; worker++) {
  const remainingWorkers = workers - worker;
  const remainingBytes = regions.slice(offset).reduce((sum, region) => sum + region.bytes, 0);
  const targetBytes = remainingBytes / remainingWorkers;
  const first = offset;
  let groupBytes = 0;
  do { groupBytes += regions[offset++].bytes; }
  while (offset < regions.length && (worker === workers - 1 ||
    (offset < regions.length - (remainingWorkers - 1) && groupBytes < targetBytes)));
  groups.push({ start: regions[first].start, end: regions[offset - 1].end,
    plannedBytes: groupBytes, regions: offset - first });
}

const startedAt = Date.now();
const results = await Promise.all(groups.map(async (group, index) => {
  const begun = Date.now();
  const result = await asyncCommand([
    'memory', 'find', '--pid', String(pid), '--text', MAGIC,
    '--from', `0x${group.start.toString(16)}`, '--to', `0x${group.end.toString(16)}`,
    '--max-hits', '64', '--context', '80', '--read-budget', String(32 * 1024 ** 3),
  ]);
  return { worker: index + 1, plannedBytes: group.plannedBytes,
    scannedBytes: result.bytesScanned, regions: group.regions,
    elapsedMs: Date.now() - begun, complete: result.complete,
    truncated: result.truncated, hits: result.hits.map(hit => hit.address) };
}));
console.log(JSON.stringify({ ok: true, pid, buildKey: target.buildKey, workers,
  plannedBytes: total, scannedBytes: results.reduce((sum, result) => sum + result.scannedBytes, 0),
  elapsedMs: Date.now() - startedAt, results }, null, 2));
