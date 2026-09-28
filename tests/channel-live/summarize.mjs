import fs from 'node:fs/promises';
import path from 'node:path';

const root = path.resolve(process.argv[2] ?? '.tmp/channel-live/retail-a');
const evidence = path.join(root, 'evidence');
const runs = [];
for (const entry of (await fs.readdir(evidence, { withFileTypes: true })).sort((a,b)=>a.name.localeCompare(b.name))) {
  if (!entry.isDirectory()) continue;
  const directory = path.join(evidence, entry.name);
  const read = async name => {
    try {
      const file = path.join(directory, name);
      if ((await fs.stat(file)).size > 32 * 1024 * 1024) throw new Error('evidence JSON size limit');
      return JSON.parse(await fs.readFile(file, 'utf8'));
    } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
  };
  const state = await read('state.json');
  const scans = await read('scans.json');
  const discovery = await read('discovery.json');
  const operation = state?.operation;
  const body = operation?.resultBytes ? JSON.parse(Buffer.from(operation.resultBytes, 'base64').toString('utf8')) : operation?.result;
  runs.push({
    run: entry.name, connection: state?.id, bound: state?.bound, runtime: state?.identity.runtime,
    nextSlot: state?.identity.nextSlot,
    operation: operation && { id: operation.id, stage: operation.stage, bytes: operation.reportBytes, ok: body?.ok, error: body?.error, resourcesReleased: body?.resourcesReleased },
    reloadHint: await read('reload-hint.json'),
    discovery: discovery && { candidates: discovery.candidates.length, ...coverage(discovery.coverage) },
    scans: scans?.map(s=>({ path:s.path,elapsedMillis:s.elapsedMillis,...coverage(s.coverage) })),
    captures: (await fs.readdir(directory)).filter(name=>name.endsWith('.png')),
  });
}
function coverage(c) {
  return c && {plannedBytes:c.plannedBytes,scannedBytes:c.scannedBytes,complete:c.complete,truncated:c.truncated,gaps:c.gaps?.length,workers:c.workers?.length};
}
await fs.writeFile(path.join(root,'summary.json'),JSON.stringify({schema:'lycheedev.channel-acceptance-summary.v1',root,runs},null,2)+'\n');
console.log(JSON.stringify({root,runs:runs.length,summary:path.join(root,'summary.json')}));
