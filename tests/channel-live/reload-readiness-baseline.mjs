import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

// Real-client acceptance: only the public CLI sends input. The journal checks
// distinguish an observed reload with zero Esc from a fixed bootstrap burst.
const [cliArg,projectArg,installation,pid,resumeSession]=process.argv.slice(2);
assert.ok(cliArg&&projectArg&&path.isAbsolute(installation)&&/^\d+$/.test(pid??''));
const cli=path.resolve(cliArg),project=path.resolve(projectArg);
await fs.mkdir(project,{recursive:true});
const report={complete:false,installation,pid:Number(pid),steps:[]};
if(resumeSession)assert.match(resumeSession,/^CON-[0-9a-f]{32}$/);
const reportPath=path.join(project,resumeSession?'reload-readiness-resumed.json':'reload-readiness.json');
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
async function call(name,args){
  args=[...args,'--project',project,'--wait-seconds','120','--format','json'];
  const start=Date.now();
  const r=await new Promise((resolve,reject)=>{
    const child=spawn(cli,args,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
    let out='',err='';
    const timer=setTimeout(()=>{child.kill();reject(new Error('timeout; preserve '+report.session));},145000);
    child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
    child.stdout.on('data',b=>{out+=b;if(out.length>4*1024*1024)child.kill();});
    child.stderr.on('data',b=>{if(err.length<65536)err+=b;});
    child.on('error',e=>{clearTimeout(timer);reject(e);});
    child.on('close',code=>{clearTimeout(timer);resolve({code,out,err});});
  });
  await fs.writeFile(path.join(project,name+'.json'),r.out||JSON.stringify(r));
  const envelope=JSON.parse(r.out.replace(/^\uFEFF/,''));
  report.session=envelope.result?.session??report.session;
  report.steps.push({name,exitCode:r.code,elapsedMs:Date.now()-start,session:report.session,stage:envelope.result?.stage});
  await fs.writeFile(reportPath,JSON.stringify(report,null,2));
  console.log(JSON.stringify(report.steps.at(-1)));
  assert.equal(r.code,0,JSON.stringify(envelope.error));
  assert.equal(envelope.result.complete,true);
  return envelope.result;
}
const journal=s=>path.join(project,'.lycheedev','live','connections',s+'.jsonl');
async function observedReload(session,key){
  const events=(await fs.readFile(journal(session),'utf8')).trim().split('\n').map(JSON.parse);
  const attempts=events.filter(e=>e.kind==='input_intent'&&e.data.input?.exchange==='reload:'+key).map(e=>e.data.input);
  assert.ok(attempts.length>0);
  assert.ok(attempts.every(a=>a.kind==='reload'&&a.observation.inputBlocked===false),'ready reload must send no Escape');
  const outcomes=events.filter(e=>e.kind==='input_observed'&&e.data.input?.exchange==='reload:'+key);
  assert.equal(outcomes.length,attempts.length,'all test attempts have known outcomes');
  const sent=outcomes.filter(e=>e.data.input.outcome.disposition!=='not_sent');
  assert.equal(sent.length,1,'exactly one physically submitted reload');
  assert.equal(sent[0].data.input.outcome.disposition,'submitted');
  assert.equal(sent[0].data.input.outcome.messagesQueued,11,'no fixed Escape prefix');
  assert.ok(outcomes.filter(e=>e.data.input.outcome.disposition==='not_sent').every(e=>e.data.input.outcome.messagesQueued===0));
  return {actions:sent.map(e=>e.data.input.kind),zeroSendRetries:outcomes.length-sent.length,messagesQueued:sent[0].data.input.outcome.messagesQueued};
}
try{
  const c=resumeSession?JSON.parse((await fs.readFile(path.join(project,'connect.json'),'utf8')).replace(/^\uFEFF/,'')).result:await call('connect',['live','connect','--installation',installation,'--pid',pid]);
  if(resumeSession)assert.equal(c.session,resumeSession);
  assert.equal(c.identity.inputState,'lycheedev.input.hybrid.v2');
  const r=await call(resumeSession?'reload-readonly-resume':'reload-no-cache',['live','reload','--session',c.session,'--request','ready-reload','--no-cache']);
  assert.notEqual(r.identity.runtime,c.identity.runtime);
  report.connected=await observedReload(c.session,'ready-reload');
  const before=await fs.readFile(journal(c.session));
  await call('reload-readonly-retry',['live','reload','--session',c.session,'--request','ready-reload']);
  assert.deepEqual(await fs.readFile(journal(c.session)),before);
  await call('disconnect',['live','disconnect',c.session]);
  const f=await call('fallback-observed',['live','reload','fallback','--installation',installation,'--pid',pid,'--request','ready-fallback']);
  assert.notEqual(f.identity.runtime,r.identity.runtime);
  report.fallback=await observedReload(f.session,'ready-fallback');
  await call('fallback-disconnect',['live','disconnect',f.session]);
  report.complete=true;
}catch(error){report.failure={message:error.message,session:report.session};process.exitCode=1;}
finally{await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({complete:report.complete,report:reportPath,failure:report.failure}));}
