import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

// Explicit real-machine test. Reuses the two exact owned connections from an
// earlier discovery report; never selects another online actor or edits state.
const [cliArg,hostArg,sourceArg,outArg]=process.argv.slice(2);
const cli=path.resolve(cliArg),host=path.resolve(hostArg),out=path.resolve(outArg);
const {targets}=JSON.parse(await fs.readFile(sourceArg,'utf8'));
assert.equal(targets.length,2);
assert.equal(targets[0].installation,targets[1].installation);
assert.notEqual(targets[0].pid,targets[1].pid);
await fs.mkdir(out,{recursive:true});
const report={schema:'lycheedev.shared-installation-baseline.v1',targets,steps:[],complete:false};
await fs.writeFile(path.join(out,'report.json'),JSON.stringify(report),{flag:'wx'});
function launch(exe,args) {
  const child=spawn(exe,args,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
  child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
  let stdout='',stderr='';
  const done=new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>{child.kill();reject(new Error('test child deadline'));},150000);
    child.stdout.on('data',b=>stdout+=b);child.stderr.on('data',b=>stderr+=b);
    child.on('error',e=>{clearTimeout(timer);reject(e);});
    child.on('close',code=>{clearTimeout(timer);resolve({code,stdout,stderr});});
  });
  return {child,done};
}
async function call(t,name,args,expected=0,wait=120) {
  const start=Date.now();
  const result=await launch(cli,[...args,'--project',t.project,'--wait-seconds',String(wait),'--format','json']).done;
  await fs.writeFile(path.join(out,`${t.name}-${name}.json`),result.stdout||JSON.stringify(result),{flag:'wx'});
  const step={target:t.name,name,exitCode:result.code,elapsedMs:Date.now()-start};
  report.steps.push(step);console.log(JSON.stringify(step));
  assert.equal(result.code,expected,result.stdout.slice(0,1400)+result.stderr);
  return JSON.parse(result.stdout);
}
async function pair(fn) {
  const results=await Promise.allSettled(targets.map(fn));
  for(const r of results)if(r.status==='rejected')throw r.reason;
  return results.map(r=>r.value);
}
function verify(t,e,marker) {
  assert.equal(e.result.complete,true);assert.equal(e.result.reportState,'verified');
  assert.equal(e.result.cleanup,'complete');assert.equal(e.result.report.ok,true);
  assert.equal(e.result.identity.guid,t.guid);
  if(marker){assert.equal(e.result.report.result.marker,marker);assert.equal(e.result.report.result.guid,t.guid);}
}
async function execute(t,key,file,wait=120,expected=0,budget=10) {
  return call(t,key,['live','execute','--session',t.session,'--request',key,'--file',file,'--budget-seconds',String(budget),'--policy','observation'],expected,wait);
}
async function fixture(t,key,async=false) {
  const file=path.join(out,`${t.name}-${key}.lua`);
  const value=`{marker=${JSON.stringify(t.name+'-'+key)},guid=UnitGUID("player")}`;
  await fs.writeFile(file,async?`local probe=...\nassert(probe:Async(50))\nlocal timer=C_Timer.NewTimer(35,assert(probe:Callback(function() probe:Finish(${value}) end)))\nassert(probe:OnCleanup(function() timer:Cancel() end))\n`:`return ${value}\n`,{flag:'wx'});
  return file;
}
try {
  const aligned=await pair(async t=>call(t,'align',['live','reload','--session',t.session,'--request','race-align']));
  for(let i=0;i<2;i++){assert.equal(aligned[i].result.identity.guid,targets[i].guid);targets[i].initialRuntime=aligned[i].result.identity.runtime;}
  const a=targets[0],b=targets[1];
  const held=launch(host,['-mode','hold-publication','-installation',a.installation,'-pid',String(a.pid),'-project',a.project,'-connection',a.session,'-timeout','6']);
  await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>reject(new Error('holder did not become ready')),8000);
    held.child.stdout.once('data',data=>{clearTimeout(timer);try{assert.ok(data.includes('publicationHeld'));resolve();}catch(e){reject(e);}});
    held.done.then(r=>{if(r.code!==0)reject(new Error(r.stderr));},reject);
  });
  const waiting=await pair(async t=>{
    t.waitFile=await fixture(t,'wait-resume');
    const e=await execute(t,'wait-resume',t.waitFile,1,6);
    assert.equal(e.result.waiting,'shared_publication');return e;
  });
  const holder=await held.done;assert.equal(holder.code,0,holder.stderr);
  await fs.writeFile(path.join(out,'holder.json'),JSON.stringify(holder));
  await pair(async t=>{const e=await call(t,'wait-resumed',['live','resume',t.session]);verify(t,e,t.name+'-wait-resume');assert.equal(e.result.operation,waiting[targets.indexOf(t)].result.operation);});
  // Repeated paired commands cross the 49-slot admission threshold in both
  // runtimes; all returned payloads must belong to the selected actor/request.
  await pair(async t=>{
    for(let i=0;i<13;i++){
      const key='paired-'+i,file=await fixture(t,key),e=await execute(t,key,file);
      verify(t,e,t.name+'-'+key);t.lastRuntime=e.result.identity.runtime;
    }
    assert.notEqual(t.lastRuntime,t.initialRuntime,'automatic capacity reload was not exercised');
  });
  const running=await execute(b,'peer-async',await fixture(b,'peer-async',true),18,6,55);
  assert.equal(running.result.operationState,'running');
  const peerRuntime=running.result.identity.runtime;
  const [reloaded,finished]=await Promise.all([
    call(a,'peer-reload',['live','reload','--session',a.session,'--request','peer-isolation']),
    call(b,'peer-finish',['live','resume',b.session])
  ]);
  assert.equal(reloaded.result.identity.guid,a.guid);verify(b,finished,b.name+'-peer-async');
  assert.equal(finished.result.identity.runtime,peerRuntime,'peer reload leaked across processes');
  await pair(async t=>{const e=await call(t,'disconnect',['live','disconnect',t.session]);assert.equal(e.result.closed,true);});
  report.complete=true;
}catch(error){report.failure={message:error.message};process.exitCode=1;}
finally{await fs.writeFile(path.join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({complete:report.complete,failure:report.failure}));}
