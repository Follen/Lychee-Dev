import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

const [cliArg,projectArg,installation,pid]=process.argv.slice(2);
assert.ok(cliArg&&projectArg&&path.isAbsolute(installation)&&/^\d+$/.test(pid??''));
const cli=path.resolve(cliArg),project=path.resolve(projectArg);
await fs.mkdir(project,{recursive:true});
const report={complete:false,installation,pid:Number(pid),steps:[]};
const reportPath=path.join(project,'focus.json');
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
async function call(name,args){
  args=[...args,'--project',project,'--wait-seconds','60','--format','json'];
  const start=Date.now();
  const r=await new Promise((resolve,reject)=>{
    const child=spawn(cli,args,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
    let out='',err='';
    const timer=setTimeout(()=>{child.kill();reject(new Error('timeout; retain '+report.session));},75000);
    child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
    child.stdout.on('data',b=>{out+=b;if(out.length>4*1024*1024)child.kill();});
    child.stderr.on('data',b=>{if(err.length<65536)err+=b;});
    child.on('error',e=>{clearTimeout(timer);reject(e);});
    child.on('close',code=>{clearTimeout(timer);resolve({code,out,err});});
  });
  await fs.writeFile(path.join(project,name+'.json'),r.out||JSON.stringify(r));
  report.steps.push({name,args,exitCode:r.code,elapsedMs:Date.now()-start});
  await fs.writeFile(reportPath,JSON.stringify(report,null,2));
  console.log(name,r.code);
  assert.equal(r.code,0,r.out.slice(0,1500));
  return JSON.parse(r.out.replace(/^\uFEFF/,'')).result;
}
async function execute(name,code){
  const file=path.join(project,name+'.lua');await fs.writeFile(file,code);
  const r=await call(name,['live','execute','--session',report.session,'--request',name,
    '--file',file,'--budget-seconds','5','--policy','observation']);
  assert.equal(r.complete,true);assert.equal(r.cleanup,'complete');assert.equal(r.report.ok,true,JSON.stringify(r.report));
}
try{
  const c=await call('connect',['live','connect','--installation',installation,'--pid',pid]);
  report.session=c.session;assert.equal(c.bound,true);
  for(const multiline of [false,true]){
    const kind=multiline?'multiline':'singleline';
    await execute('focus-'+kind,`
local ns=LycheeDevInternal
local profile=assert(ns.ReceiverBindings.Current())
assert((profile.wake=="ALT-CTRL-F12" and profile.submit=="ALT-CTRL-SHIFT-F12")
  or (profile.wake=="ALT-CTRL-F11" and profile.submit=="ALT-CTRL-SHIFT-F11"))
assert(profile.close=="ALT-CTRL-[")
assert(not GetCurrentKeyBoardFocus(),"existing user editor must be preserved")
assert(not LycheeFocusFixture)
local f=CreateFrame("EditBox",nil,UIParent)
f:SetAutoFocus(false);f:SetMultiLine(${multiline});f:SetFont(STANDARD_TEXT_FONT,16,"")
f:SetSize(360,80);f:SetPoint("TOP",0,-90)
f:SetText("focus-sentinel-123");f:SetCursorPosition(5)
local fixture={frame=f,escapes=0};LycheeFocusFixture=fixture
f:SetScript("OnEscapePressed",function(self)
  fixture.escapes=fixture.escapes+1
  if fixture.escapes>=3 then self:ClearFocus() end
end)
fixture.timer=C_Timer.NewTimer(50,function()fixture.expired=true;f:ClearFocus();f:Hide()end)
f:Show();f:SetFocus()
assert(GetCurrentKeyBoardFocus()==f)
return {focused=true,multiline=${multiline}}`);
    await execute('verify-'+kind,`
local fixture=assert(LycheeFocusFixture)
local f=fixture.frame
local focused=GetCurrentKeyBoardFocus()==f
local text,cursor=f:GetText(),f:GetCursorPosition()
fixture.timer:Cancel();f:ClearFocus();f:Hide();LycheeFocusFixture=nil
assert(not fixture.expired,"watchdog released focus before confirmation")
assert(not focused and fixture.escapes==3,"inputBlocked recovery did not stop at exactly three Esc presses")
assert(text=="focus-sentinel-123" and cursor==5,"transport changed editor contents/cursor")
return {preserved=true,escapes=fixture.escapes}`);
  }
  const d=await call('disconnect',['live','disconnect',report.session]);assert.equal(d.closed,true);
  report.complete=true;
}catch(error){report.failure={message:error.message};process.exitCode=1;}
finally{await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({complete:report.complete,report:reportPath,failure:report.failure}));}
