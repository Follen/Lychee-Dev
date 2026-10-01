import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

const [cliArg,outputArg='.tmp/mailbox-live/titan-current-20261001/core-13']=process.argv.slice(2);
const cli=path.resolve(cliArg), project=path.resolve('.tmp/mailbox-live/titan-current-20261001/project');
const output=path.resolve(outputArg);
await fs.mkdir(output,{recursive:true});
const report={schema:'lycheedev.mailbox.acceptance.v1',startedAt:new Date().toISOString(),cli,project,pid:43756,installation:'D:/Game/World of Warcraft/_classic_titan_',character:'Qingtianjiuz',realm:'时光I',guid:'Player-6379-01045F6F',complete:false,steps:[]};
const reportPath=path.join(output,'report.json');
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
let session;
const digest=b=>createHash('sha256').update(b).digest('hex');
const journal=()=>path.join(project,'.lycheedev/live/connections',session+'.jsonl');
async function call(name,command){
  const args=[...command,'--project',project,'--wait-seconds','120','--format','json'];
  const started=Date.now();
  const response=await new Promise((resolve,reject)=>{
    const p=spawn(cli,args,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
    let stdout='',stderr='';
    const timeout=setTimeout(()=>{p.kill();reject(new Error(`host timeout; retain ${session}`));},145000);
    p.stdout.setEncoding('utf8');p.stderr.setEncoding('utf8');
    p.stdout.on('data',b=>{stdout+=b;if(stdout.length>4*1024*1024)p.kill();});
    p.stderr.on('data',b=>{if(stderr.length<65536)stderr+=b;});
    p.on('error',e=>{clearTimeout(timeout);reject(e);});
    p.on('close',code=>{clearTimeout(timeout);resolve({code,stdout,stderr});});
  });
  await fs.writeFile(path.join(output,name+'.json'),response.stdout,{flag:'wx'});
  const envelope=JSON.parse(response.stdout.replace(/^\uFEFF/,''));
  session=envelope.result?.session??session;
  report.session=session;
  const entry={name,args,exitCode:response.code,elapsedMs:Date.now()-started,session,operation:envelope.result?.operation,complete:envelope.result?.complete,cleanup:envelope.result?.cleanup,error:envelope.error};
  report.steps.push(entry);
  await fs.writeFile(reportPath,JSON.stringify(report,null,2));
  console.log(JSON.stringify(entry));
  assert.equal(response.code,0,`${name}: ${JSON.stringify(envelope.error)} ${response.stderr}`);
  assert.equal(envelope.ok,true,name);
  assert.equal(envelope.result.complete,true,name);
  if(envelope.result.identity) assert.equal(envelope.result.identity.guid,report.guid,'selected actor changed');
  return envelope.result;
}
async function execute(name,fixture,noCache=false,request=name){
  const result=await call(name,['live','execute','--session',session,'--request',request,'--file',path.resolve('tests/channel-live/fixtures',fixture),'--budget-seconds','5','--policy','observation',...(noCache?['--no-cache']:[])]);
  assert.equal(result.reportState,'verified');assert.equal(result.cleanup,'complete');assert.equal(result.report.ok,true);
  if(fixture==='payload.lua'){
    assert.equal(result.report.result.marker,'native-large-payload');
    assert.equal(result.report.result.text,'荔枝<&>\n'.repeat(8192));
  }else assert.equal(result.report.result.marker,'native-slot-baseline');
  return result;
}
try{
  const connected=JSON.parse((await fs.readFile('.tmp/mailbox-live/titan-current-20261001/connect.json','utf8')).replace(/^\uFEFF/,'')).result;
  session=connected.session;report.session=session;report.initialConnectEvidence='.tmp/mailbox-live/titan-current-20261001/connect.json';report.steps.push(JSON.parse((await fs.readFile('.tmp/mailbox-live/titan-current-20261001/connect-step.json','utf8')).replace(/^\uFEFF/,'')));
  assert.equal(connected.bound,true);assert.equal(connected.identity.slots,200);assert.equal(connected.identity.schema,'lycheedev.slot.identity.v2');
  const activated=await call('activate-current-bytes',['live','reload','--session',session,'--request','activate-wake-candidate','--no-cache']);
  assert.notEqual(activated.identity.runtime,connected.identity.runtime);
  const actor=await call('actor-independent',['live','execute','--session',session,'--request','mailbox-actor-independent','--file',path.resolve('.tmp/mailbox-live/actor.lua'),'--budget-seconds','5','--policy','observation','--no-cache']);
  assert.equal(actor.reportState,'verified');assert.equal(actor.cleanup,'complete');assert.equal(actor.report.ok,true);
  assert.equal(actor.report.result.marker,'mailbox-actor');assert.equal(actor.report.result.guid,report.guid);assert.equal(actor.report.result.character,report.character);assert.equal(actor.report.result.realm,report.realm);assert.equal(actor.report.result.interface,38002);
  await execute('normal','normal.lua');
  await execute('large','payload.lua');
  const normal=await execute('normal-no-cache','normal.lua',true);
  await execute('large-no-cache','payload.lua',true);
  const before=await fs.readFile(journal());
  const retry=await execute('normal-readonly-retry','normal.lua',true,'normal-no-cache');
  assert.equal(retry.operation,normal.operation);assert.deepEqual(await fs.readFile(journal()),before);
  report.readonlyRetry={journalSHA256:digest(before),unchanged:true};
  const reload=await call('reload',['live','reload','--session',session,'--request','mailbox-reload','--no-cache']);
  assert.notEqual(reload.identity.runtime,connected.identity.runtime);
  const reloadBefore=await fs.readFile(journal());
  await call('reload-readonly-retry',['live','reload','--session',session,'--request','mailbox-reload','--no-cache']);
  assert.deepEqual(await fs.readFile(journal()),reloadBefore);
  report.reloadReadonlyRetry={journalSHA256:digest(reloadBefore),unchanged:true};
  await execute('after-reload-no-cache','normal.lua',true);
  const closed=await call('disconnect',['live','disconnect',session,'--no-cache']);
  assert.equal(closed.closed,true);
  const closeBefore=await fs.readFile(journal());
  const again=await call('disconnect-readonly-retry',['live','disconnect',session,'--no-cache']);
  assert.equal(again.closed,true);assert.deepEqual(await fs.readFile(journal()),closeBefore);
  report.disconnectReadonlyRetry={journalSHA256:digest(closeBefore),unchanged:true};
  report.complete=true;
}catch(error){report.failure={message:error.message,session};process.exitCode=1;}
finally{report.finishedAt=new Date().toISOString();await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({report:reportPath,complete:report.complete,failure:report.failure}));}





