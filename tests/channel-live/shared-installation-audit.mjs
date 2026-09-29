import fs from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';

// Read-only post-run audit across rotated journal segments. It verifies the
// actual effect ledger, not just successful command envelopes.
const reportPath=path.resolve(process.argv[2]);
const report=JSON.parse(await fs.readFile(reportPath,'utf8'));
assert.equal(report.complete,true);
const audit={complete:false,targets:[]};
for(const t of report.targets){
  const log=path.join(t.project,'.lycheedev/live/connections',t.session+'.jsonl');
  const segments=[log];
  try{for(const file of await fs.readdir(log+'.history'))if(file.endsWith('.jsonl'))segments.push(path.join(log+'.history',file));}
  catch(e){if(e.code!=='ENOENT')throw e;}
  const rows=[];
  for(const file of segments)for(const line of (await fs.readFile(file,'utf8')).split('\n'))if(line.trim())rows.push(JSON.parse(line));
  const current=JSON.parse((await fs.readFile(log,'utf8')).trim().split('\n').at(-1)).data;
  assert.equal(current.closed,true);
  const prepareNonces=new Set(rows.filter(r=>r.data?.operation?.request==='wait-resume'&&r.data?.transaction?.envelope?.action==='prepare').map(r=>r.data.transaction.envelope.nonce));
  assert.equal(prepareNonces.size,1,'pending/resume allocated a replacement prepare nonce');
  const effects=new Map();
  for(const r of rows)if(r.kind==='input_observed')effects.set(r.data.input.id,r.data.input);
  const submitted=new Set();
  let zero=0;
  for(const input of effects.values()){
    if(input.outcome.disposition==='not_sent'){zero++;assert.equal(input.outcome.messagesQueued,0);continue;}
    // Esc recovery may legitimately require several separately observed keys.
    if(input.kind==='escape')continue;
    const key=input.runtime+'/'+input.exchange+'/'+input.kind;
    assert.ok(!submitted.has(key),'repeated an already submitted/uncertain effect');
    submitted.add(key);
  }
  const capacity=new Set(rows.filter(r=>r.kind==='automatic_reload_intent'&&r.data.reload?.request?.startsWith('capacity-')).map(r=>r.data.reload.request));
  assert.ok(capacity.size>0,'no actual capacity reload intent');
  const commands=report.steps.filter(s=>s.target===t.name&&/^paired-\d+$/.test(s.name));
  const slots=report.slotCount??rows.find(r=>r.data?.identity?.slots)?.data.identity.slots;
  assert.ok(slots===64||slots===200,'unsupported evidence slot count');
  assert.equal(commands.length,Math.floor((slots-16)/4)+1);assert.ok(commands.every(s=>s.exitCode===0));
  audit.targets.push({name:t.name,closed:true,prepareNonces:[...prepareNonces],capacityReloads:capacity.size,submittedEffects:submitted.size,zeroSendAttempts:zero,pairedCommands:commands.length});
}
const parent=path.join(report.targets[0].installation,'Interface/AddOns');
const pool=JSON.parse(await fs.readFile(path.join(parent,'.lycheedev-slots.json'),'utf8'));
assert.ok(pool.files.length===64||pool.files.length===200,'unsupported slot pool');
if(report.slotCount!==undefined)assert.equal(pool.files.length,report.slotCount);
audit.slotCount=pool.files.length;
audit.pendingSlots=pool.files.filter(s=>s.pendingHash||(s.nonce&&!s.consumed&&!s.retiredRuntime&&!s.retiredProcess)).length;
assert.equal(audit.pendingSlots,0);
audit.claims=(await fs.readdir(path.join(parent,'.lycheedev-window-owners'))).filter(f=>f.endsWith('.json')).length;
assert.equal(audit.claims,0);
audit.complete=true;
await fs.writeFile(path.join(path.dirname(reportPath),'journal-audit.json'),JSON.stringify(audit,null,2),{flag:'wx'});
console.log(JSON.stringify(audit));
