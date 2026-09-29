import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';
import {verifySkip,verifyCommit} from './slot-skip-evidence.mjs';

// Interactive acceptance only. Failure keeps original CONs and all evidence.
const [cliArg,labArg,rootArg,targetsArg]=process.argv.slice(2);
assert.ok(cliArg&&labArg&&rootArg&&targetsArg,'usage: slot-skip-baseline.mjs <cli> <lab> <new-root> <targets.json>');
const cli=path.resolve(cliArg),lab=path.resolve(labArg),root=path.resolve(rootArg);
const read=async p=>JSON.parse((await fs.readFile(p,'utf8')).replace(/^\uFEFF/,''));
const targets=await read(targetsArg);
assert.ok(Array.isArray(targets)&&targets.length===2,'two targets required');
for(const t of targets){assert.ok(Number.isSafeInteger(t.pid)&&t.pid>0);assert.equal(typeof t.installation,'string');}
assert.notEqual(targets[0].pid,targets[1].pid);
assert.equal(path.resolve(targets[0].installation).toLowerCase(),path.resolve(targets[1].installation).toLowerCase(),'same installation required');
await fs.mkdir(root); // Existing evidence must never be overwritten.
const fixture=path.resolve('tests/channel-live/fixtures/slot_skip.lua');
const report={schema:'lycheedev.slot-skip-baseline.v1',complete:false,targets,steps:[],scope:'one commit input/receipt chain and read-only retry; no claim about invisible internal execution counts'};
const save=()=>fs.writeFile(path.join(root,'report.json'),JSON.stringify(report,null,2));
const peers=targets.map((t,i)=>({...t,project:path.join(root,i?'b':'a')}));
async function call(name,exe,argv){
 const started=Date.now();
 const raw=await new Promise(resolve=>{
  let stdout='',stderr='',timedOut=false,overflow=false;
  const child=spawn(exe,argv,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
  const timer=setTimeout(()=>{timedOut=true;child.kill();},145000);
  child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
  child.stdout.on('data',b=>{if(stdout.length<8*1024*1024)stdout+=b;else{overflow=true;child.kill();}});
  child.stderr.on('data',b=>{if(stderr.length<65536)stderr+=b;});
  child.on('error',e=>{clearTimeout(timer);resolve({stdout,stderr,error:String(e),exitCode:null});});
  child.on('close',exitCode=>{clearTimeout(timer);resolve({stdout,stderr,exitCode,timedOut,overflow});});
 });
 const record={exe,argv,elapsedMs:Date.now()-started,...raw};
 await fs.writeFile(path.join(root,name+'.json'),JSON.stringify(record,null,2),{flag:'wx'});
 report.steps.push({name,argv,exitCode:raw.exitCode,elapsedMs:record.elapsedMs});await save();
 assert.equal(raw.exitCode,0,`${name}: ${raw.stderr} ${raw.stdout}`);
 assert.ok(!raw.timedOut&&!raw.overflow,name);
 return JSON.parse(raw.stdout.replace(/^\uFEFF/,''));
}
async function pub(p,name,args){
 const r=await call(name,cli,[...args,'--project',p.project,...(args[1]==='status'?[]:['--wait-seconds','120']),'--format','json']);
 assert.equal(r.ok,true,name);return r.result;
}
const log=p=>path.join(p.project,'.lycheedev','live','connections',p.session+'.jsonl');
const events=async p=>(await fs.readFile(log(p),'utf8')).trim().split(/\r?\n/).map(JSON.parse);
function complete(r){assert.equal(r.complete,true);assert.equal(r.reportState,'verified');assert.equal(r.cleanup,'complete');assert.equal(r.report.ok,true);assert.equal(r.report.result.marker,'slot-skip-readonly');}
try{
 for(const [i,p] of peers.entries()){
  await fs.mkdir(p.project);
  p.connected=await pub(p,`connect-${i}`,['live','connect','--installation',p.installation,'--pid',String(p.pid)]);
  p.session=p.connected.session;assert.match(p.session,/^CON-[a-f0-9]{32}$/);assert.equal(p.connected.bound,true);assert.equal(p.connected.identity.inventory,200);await save();
 }
 if(peers[0].connected.identity.nextSlot!==peers[1].connected.identity.nextSlot){
  for(const [i,p] of peers.entries())p.connected=await pub(p,`align-${i}`,['live','reload','--session',p.session,'--request','slot-skip-align']);
 }
 assert.equal(peers[0].connected.identity.nextSlot,peers[1].connected.identity.nextSlot,'must align before staging');
 const [a,b]=peers;
 const staged=await call('stage-a',lab,['-mode','stage-observation','-installation',a.installation,'-pid',String(a.pid),'-project',a.project,'-connection',a.session,'-timeout','30']);
 assert.equal(staged.staged,true);assert.equal(staged.inputSent,false);assert.equal(staged.slot,b.connected.identity.nextSlot,'exact same next-slot contention required');
 const parent=path.join(a.installation,'Interface','AddOns');
 const payload=path.join(parent,`Lychee Dev Slot ${String(staged.slot).padStart(2,'0')}`,'Payload.lua');
 const poolPath=path.join(parent,'.lycheedev-slots.json');
 const before=await fs.readFile(payload),reservation=(await read(poolPath)).files[staged.slot-1];
 assert.equal(reservation.nonce,staged.nonce);assert.equal(reservation.consumed,false);
 await fs.writeFile(path.join(root,'a-reserved-payload.lua'),before,{flag:'wx'});
 await fs.writeFile(path.join(root,'a-reservation.json'),JSON.stringify(reservation,null,2),{flag:'wx'});
 const br=await pub(b,'execute-b',['live','execute','--session',b.session,'--request','slot-skip-b','--file',fixture,'--budget-seconds','5','--policy','observation']);complete(br);assert.equal(br.report.result.guid,b.connected.identity.guid);
 const be=await events(b);
 report.skippedTo=verifySkip(be,'slot-skip-b',staged.slot);
 assert.deepEqual(await fs.readFile(payload),before,'B changed A payload');
 assert.deepEqual((await read(poolPath)).files[staged.slot-1],reservation,'B changed A reservation');
 const ar=await pub(a,'resume-a',['live','resume',a.session]);complete(ar);assert.equal(ar.operation,staged.operation);assert.equal(ar.report.result.guid,a.connected.identity.guid);
 const ae=await events(a);
 report.commitNonce=verifyCommit(ae,staged.operation);
 const beforeRetry=await fs.readFile(log(a));
 const retry=await pub(a,'retry-a',['live','execute','--session',a.session,'--request','slot-skip-a','--file',fixture,'--budget-seconds','5','--policy','observation']);complete(retry);assert.equal(retry.operation,staged.operation);
 assert.deepEqual(await fs.readFile(log(a)),beforeRetry,'same request must be read-only');
 for(const [i,p] of peers.entries()){const r=await pub(p,`disconnect-${i}`,['live','disconnect',p.session]);assert.equal(r.closed,true);assert.equal(r.complete,true);}
 report.complete=true;
}catch(e){report.failure=String(e);process.exitCode=1;}finally{report.connections=peers.map(({pid,project,session})=>({pid,project,session}));await save();}
