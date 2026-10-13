// Real-client acceptance for Lycheedev mailbox protocol v1. All input uses CLI.
import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

const [cliArg,projectArg,installation,pid]=process.argv.slice(2);
assert.ok(cliArg&&projectArg&&path.isAbsolute(installation)&&/^\d+$/.test(pid??''));
const cli=path.resolve(cliArg),project=path.resolve(projectArg);
await fs.mkdir(project,{recursive:true});
const report={protocol:'Lycheedev mailbox protocol v1',complete:false,installation,pid:Number(pid),steps:[]};
const reportPath=path.join(project,'mailbox-protocol.json');
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
async function call(name,args,expected=0){
  const start=Date.now();
  const r=await new Promise((resolve,reject)=>{
    const child=spawn(cli,[...args,'--project',project,'--wait-seconds','60','--format','json'],{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
    let out='',err='';
    const timer=setTimeout(()=>{child.kill();reject(new Error('timeout; retain '+report.session));},75000);
    child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
    child.stdout.on('data',b=>{out+=b;if(out.length>4*1024*1024)child.kill();});
    child.stderr.on('data',b=>{if(err.length<65536)err+=b;});
    child.on('error',e=>{clearTimeout(timer);reject(e);});
    child.on('close',code=>{clearTimeout(timer);resolve({code,out,err});});
  });
  await fs.writeFile(path.join(project,name+'.json'),r.out||JSON.stringify(r));
  report.steps.push({name,exitCode:r.code,elapsedMs:Date.now()-start});
  await fs.writeFile(reportPath,JSON.stringify(report,null,2));
  console.log(name,r.code);
  assert.equal(r.code,expected,r.out.slice(0,1500));
  return JSON.parse(r.out.replace(/^\uFEFF/,'')).result;
}
function verified(r){assert.equal(r.complete,true);assert.equal(r.cleanup,'complete');assert.equal(r.report.ok,true,JSON.stringify(r.report));return r;}
async function execute(name,code,expected=0){
  const file=path.join(project,name+'.lua');await fs.writeFile(file,code);
  const r=await call(name,['live','execute','--session',report.session,'--request',name,'--file',file,'--budget-seconds','5','--policy','observation'],expected);
  if(expected===0)verified(r);
  return r;
}
async function inputEvents(){
  const file=path.join(project,'.lycheedev','live','connections',report.session+'.jsonl');
  return (await fs.readFile(file,'utf8')).trim().split('\n').map(JSON.parse).filter(e=>e.kind==='input_observed').map(e=>e.data.input);
}
function focusCode(clearAfter){return `
local ns=LycheeDevInternal
assert(not GetCurrentKeyBoardFocus(),"existing user editor must be preserved")
assert(not LycheeMailboxFocusFixture)
local f=CreateFrame("EditBox",nil,UIParent)
f:SetAutoFocus(false);f:SetMultiLine(${clearAfter!==1});f:SetFont(STANDARD_TEXT_FONT,16,"")
f:SetSize(360,80);f:SetPoint("TOP",0,-90)
f:SetText("mailbox-focus-sentinel");f:SetCursorPosition(5)
local fixture={frame=f,escapes=0};LycheeMailboxFocusFixture=fixture
f:SetScript("OnEscapePressed",function(self)
  fixture.escapes=fixture.escapes+1
  if ${clearAfter}>0 and fixture.escapes>=${clearAfter} then self:ClearFocus() end
end)
fixture.timer=C_Timer.NewTimer(${clearAfter===0?30:60},function()fixture.expired=true;f:ClearFocus();f:Hide()end)
f:Show();f:SetFocus();assert(GetCurrentKeyBoardFocus()==f)
ns.InputState.Refresh()
assert(ns.Mailbox.input:find('"escapeLimit":3',1,true))
return {focused=true,clearAfter=${clearAfter}}`;}
function verifyFocus(count,expired){return `
local fixture=assert(LycheeMailboxFocusFixture)
local f=fixture.frame
local focused=GetCurrentKeyBoardFocus()==f
local text,cursor=f:GetText(),f:GetCursorPosition()
fixture.timer:Cancel();f:ClearFocus();f:Hide();LycheeMailboxFocusFixture=nil
assert(not focused and fixture.escapes==${count},"incorrect number of Esc presses")
assert((fixture.expired==true)==${expired},"unexpected watchdog outcome")
assert(text=="mailbox-focus-sentinel" and cursor==5,"transport changed editor content")
return {preserved=true,escapes=fixture.escapes,expired=fixture.expired==true}`;}

try{
  const c=await call('connect',['live','connect','--installation',installation,'--pid',pid]);
  report.session=c.session;assert.equal(c.bound,true);
  assert.equal(c.identity.schema,'Lycheedev mailbox protocol v1');assert.equal(c.identity.inputState,'Lycheedev mailbox protocol v1');
  report.initialRuntime=c.identity.runtime;
  await execute('protocol',`
local ns=LycheeDevInternal
assert(ns.Mailbox.schema=="Lycheedev mailbox protocol v1")
assert(ns.InputSignal==nil and ns.StartupBeacon==nil,"optical runtime still loaded")
ns.InputState.Refresh()
assert(ns.Mailbox.input:find('"schema":"Lycheedev mailbox protocol v1"',1,true))
assert(ns.Mailbox.input:find('"escapeLimit":0',1,true))
for i=1,500 do
  local name=string.format("Lychee Dev Slot %02d",i)
  assert(C_AddOns.GetAddOnMetadata(name,"X-Lychee-Slot")==tostring(i),name)
  assert(C_AddOns.GetAddOnMetadata(name,"X-Lychee-Transport")=="Lycheedev mailbox protocol v1",name)
end
return {protocol="Lycheedev mailbox protocol v1",slots=500,build=select(2,GetBuildInfo())}`);
  for(const count of [1,3]){
    const before=(await inputEvents()).length;
    await execute('focus-'+count,focusCode(count));
    await execute('verify-'+count,verifyFocus(count,false));
    const escapes=(await inputEvents()).slice(before).filter(e=>e.kind==='escape'&&e.outcome.disposition!=='not_sent');
    assert.equal(escapes.length,count);
    assert.deepEqual(escapes.map(e=>e.escapeAttempts),Array.from({length:count},(_,i)=>i+1));
  }
  const started=Date.now();
  const blocked=await execute('focus-exhausted',focusCode(0),6);
  assert.equal(blocked.waiting,'input_focus_recovery_exhausted');
  assert.equal(blocked.input.escapeAttempts,3);
  assert.equal(blocked.continuation.kind,'wait_external');
  const before=await inputEvents();
  const retry=await call('resume-still-focused',['live','resume',report.session],6);
  assert.equal(retry.waiting,'input_focus_recovery_exhausted');assert.equal(retry.input.escapeAttempts,3);
  assert.deepEqual(await inputEvents(),before,'resume issued more input');
  // Only the fixture's own watchdog clears focus. Host does not send a fourth Esc.
  await new Promise(resolve=>setTimeout(resolve,Math.max(0,started+45000-Date.now())));
  verified(await call('resume-after-focus-cleared',['live','resume',report.session]));
  await execute('verify-exhausted',verifyFocus(3,true));
  const r=await call('reload',['live','reload','--session',report.session,'--request','mailbox-ready','--no-cache']);
  assert.notEqual(r.identity.runtime,c.identity.runtime);assert.equal(r.identity.inputState,'Lycheedev mailbox protocol v1');
  const reload=(await inputEvents()).filter(e=>e.exchange==='reload:mailbox-ready'&&e.outcome.disposition!=='not_sent');
  assert.equal(reload.length,1);assert.equal(reload[0].kind,'reload');assert.equal(reload[0].outcome.messagesQueued,11);
  report.reloadedRuntime=r.identity.runtime;
  await execute('post-reload','return {protocol=LycheeDevInternal.Mailbox.schema,guid=UnitGUID("player")}');
  const d=await call('disconnect',['live','disconnect',report.session]);assert.equal(d.closed,true);
  report.complete=true;
}catch(error){report.failure={message:error.message,session:report.session};process.exitCode=1;}
finally{await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({complete:report.complete,report:reportPath,failure:report.failure}));}
