import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

// Explicit real-client acceptance, never run by CI. Concurrent input below is
// intentional: the production desktop mutex must serialize the two drivers.
const [cliArg,hostArg,rootArg,targetsArg,resumeArg]=process.argv.slice(2);
if(!targetsArg) throw new Error('usage: cross-client-baseline.mjs <cli> <lab-host> <new-project-root> <targets.json>');
const cli=path.resolve(cliArg),host=path.resolve(hostArg),root=path.resolve(rootArg);
assert.ok(!resumeArg||resumeArg==='--resume');
const previous=resumeArg?JSON.parse(await fs.readFile(path.join(root,'report.json'),'utf8')):null;
const attempt=previous?`resumed-${Date.now()}`:'';
const targets=JSON.parse(await fs.readFile(targetsArg,'utf8'));
assert.equal(targets.length,2);
for(const t of targets) {
  assert.match(t.name,/^[a-z]+$/);
  assert.ok(Number.isInteger(t.pid)&&t.pid>0&&path.isAbsolute(t.installation));
  t.project=path.join(root,t.name);
  await fs.mkdir(t.project,{recursive:true});
}
assert.notEqual(targets[0].pid,targets[1].pid);
const report={schema:'lycheedev.cross-client-baseline.v1',targets,complete:false,steps:[]};
if(previous)report.resumes=path.join(root,'report.json');
const reportPath=path.join(root,previous?`report-${attempt}.json`:'report.json');
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
async function run(exe,argv,name,t,expected=0) {
  const start=Date.now();
  const value=await new Promise((resolve,reject)=>{
    const child=spawn(exe,argv,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
    child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
    let out='',err='';
    const timer=setTimeout(()=>{child.kill();reject(new Error(`deadline ${t.name}/${name}`));},145000);
    child.stdout.on('data',b=>{out+=b;if(out.length>4*1024*1024)child.kill();});
    child.stderr.on('data',b=>{if(err.length<65536)err+=b;});
    child.on('error',e=>{clearTimeout(timer);reject(e);});
    child.on('close',code=>{clearTimeout(timer);resolve({code,out,err});});
  });
  await fs.writeFile(path.join(t.project,(attempt?attempt+'-':'')+name+'.json'),value.out||JSON.stringify({exitCode:value.code,stderr:value.err}));
  report.steps.push({target:t.name,name,exitCode:value.code,elapsedMs:Date.now()-start,argv});
  // Per-step immutable files are primary evidence; the final report is written
  // after both concurrent children settle, avoiding concurrent manifest writes.
  console.log(JSON.stringify(report.steps.at(-1)));
  assert.equal(value.code,expected,`${t.name}/${name}: ${value.err} ${value.out.slice(0,1000)}`);
  return exe===cli?JSON.parse(value.out.replace(/^\uFEFF/,'')):null;
}
async function call(t,name,args,expected=0,wait=120) {
  return run(cli,[...args,'--project',t.project,'--wait-seconds',String(wait),'--format','json'],name,t,expected);
}
async function execute(t,key,fixture,budget,policy='observation',expected=0,wait=120,extra=[]) {
  return call(t,key,['live','execute','--session',t.session,'--request',key,'--file',path.resolve('tests/channel-live/fixtures',fixture),'--budget-seconds',String(budget),'--policy',policy,...extra],expected,wait);
}
function verified(t,e) {
  assert.equal(e.ok,true);
  assert.equal(e.result.complete,true);
  assert.equal(e.result.cleanup,'complete');
  assert.equal(e.result.reportState,'verified');
  assert.equal(e.result.identity.guid,t.guid);
  assert.equal(e.result.identity.product,t.product);
}
async function interrupt(t,opaque) {
  await run(host,['-mode',opaque?'interrupt-opaque-fixture':'interrupt-observation','-installation',t.installation,'-pid',String(t.pid),'-project',t.project,'-connection',t.session,'-reload-hint=false','-timeout','30'],opaque?'opaque-reload':'observation-reload',t);
}
async function awaitRunning(t,e,name) {
  for(let attempt=0;e.result.operationState!=='running'&&attempt<8;attempt++) {
    assert.equal(e.result.complete,false,'probe ended before interruption');
    e=await call(t,`${name}-wait-${attempt}`,['live','resume',t.session],6,5);
  }
  assert.equal(e.result.operationState,'running');
  return e;
}
try {
  for(const t of targets) {
    if(previous) {
      const old=previous.targets.find(x=>x.name===t.name);
      assert.ok(old&&old.pid===t.pid&&old.installation===t.installation&&old.session);
      Object.assign(t,{session:old.session,guid:old.guid,product:old.product});
      const status=await run(cli,['live','status',t.session,'--project',t.project,'--format','json'],'resume-status',t);
      if(status.result.closed) { assert.equal(status.result.operationState,'execution_unknown'); t.alreadyClosed=true; }
      continue;
    }
    const e=await call(t,'connect',['live','connect','--installation',t.installation,'--pid',String(t.pid)]);
    assert.equal(e.result.bound,true);
    t.session=e.result.session;t.guid=e.result.identity.guid;t.product=e.result.identity.product;
  }
  const active=targets.filter(t=>!t.alreadyClosed);
  const pending=active.map(t=>execute(t,'concurrent-async','async.lua',40));
  const settledPromise=Promise.allSettled(pending);
  await new Promise(resolve=>setTimeout(resolve,12000));
  for(const t of active) {
    await run(host,['-mode','capture','-installation',t.installation,'-pid',String(t.pid),'-project',t.project,'-timeout','30'],'running-capture',t);
  }
  const settled=await settledPromise;
  for(let i=0;i<active.length;i++) {
    assert.equal(settled[i].status,'fulfilled',String(settled[i].reason));
    const e=settled[i].value;
    verified(active[i],e);assert.equal(e.result.report.result.inputReleased,true);
  }
  for(const t of active) {
    const large=await execute(t,'large-cache-off','payload.lua',5,'observation',0,120,['--no-cache']);
    verified(t,large);assert.equal(large.result.report.result.text,'荔枝<&>\n'.repeat(8192));
    const timeout=await execute(t,'timeout','timeout.lua',5);
    verified(t,timeout);assert.equal(timeout.result.report.ok,false);
    assert.ok(JSON.stringify(timeout.result.report.error).includes('probe_timeout'));
    const interrupted=await awaitRunning(t,await execute(t,'observation-interrupted','async_reload.lua',90,'observation',6,12),'observation');
    await interrupt(t,false);
    const resumed=await call(t,'observation-resumed',['live','resume',t.session]);
    verified(t,resumed);assert.equal(resumed.result.operation,interrupted.result.operation);
    assert.notEqual(resumed.result.identity.runtime,interrupted.result.identity.runtime);
    assert.equal(resumed.result.report.result.marker,'observation-survives-reload');
    const opaque=await awaitRunning(t,await execute(t,'opaque-interrupted','async_reload.lua',90,'opaque',6,12),'opaque');
    await interrupt(t,true);
    for(const name of ['opaque-resumed','opaque-retry']) {
      const unknown=await call(t,name,['live','resume',t.session],5);
      assert.equal(unknown.result.operation,opaque.result.operation);
      assert.equal(unknown.result.operationState,'execution_unknown');
      assert.equal(unknown.result.complete,false);
      assert.equal(unknown.result.reportState,'unavailable');
    }
    const closed=await call(t,'disconnect',['live','disconnect',t.session]);
    assert.equal(closed.result.closed,true);assert.equal(closed.result.complete,false);
    await run(host,['-mode','capture','-installation',t.installation,'-pid',String(t.pid),'-project',t.project,'-timeout','30'],'closed-capture',t);
  }
  report.complete=true;
} catch(error) {
  report.failure={message:error.message};process.exitCode=1;
} finally {
  await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify({report:reportPath,complete:report.complete,failure:report.failure}));
}
