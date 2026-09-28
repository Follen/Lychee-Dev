import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

const [cliArg,hostArg,projectArg,installation,pid]=process.argv.slice(2);
assert.ok(cliArg&&hostArg&&projectArg&&path.isAbsolute(installation)&&/^\d+$/.test(pid??''),
  'usage: automation-history-baseline.mjs <cli> <host> <new-project> <installation> <pid>');
const cli=path.resolve(cliArg),host=path.resolve(hostArg),project=path.resolve(projectArg);
await fs.mkdir(project,{recursive:true});
const report={complete:false,installation,pid:Number(pid),steps:[]};
const reportPath=path.join(project,'automation-history.json');
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
async function run(exe,args,name) {
  const started=Date.now();
  const value=await new Promise((resolve,reject)=>{
    const child=spawn(exe,args,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
    let out='',err='';
    const timer=setTimeout(()=>{child.kill();reject(new Error(`${name} timed out; retain ${report.session}`));},145000);
    child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
    child.stdout.on('data',b=>{out+=b;if(out.length>4*1024*1024)child.kill();});
    child.stderr.on('data',b=>{if(err.length<65536)err+=b;});
    child.on('error',e=>{clearTimeout(timer);reject(e);});
    child.on('close',code=>{clearTimeout(timer);resolve({code,out,err});});
  });
  await fs.writeFile(path.join(project,name+'.json'),value.out||JSON.stringify(value));
  report.steps.push({name,args,exitCode:value.code,elapsedMs:Date.now()-started});
  await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify(report.steps.at(-1)));
  assert.equal(value.code,0,`${name}: ${value.err} ${value.out.slice(0,1000)}`);
  return exe===cli?JSON.parse(value.out.replace(/^\uFEFF/,'')).result:null;
}
const call=(name,args)=>run(cli,[...args,'--project',project,'--wait-seconds','120','--format','json'],name);
async function execute(name,code) {
  const file=path.join(project,name+'.lua');await fs.writeFile(file,code);
  const r=await call(name,['live','execute','--session',report.session,'--request',name,'--file',file,
    '--budget-seconds','5','--policy','observation']);
  assert.equal(r.complete,true);assert.equal(r.cleanup,'complete');assert.equal(r.reportState,'verified');
  return r;
}
const capture=name=>run(host,['-mode','capture','-installation',installation,'-pid',pid,
  '-project',path.join(project,name),'-timeout','20'],name);
const inspect=`
local ns=LycheeDevInternal
assert(ns.Workbench.ShowPage("automation"))
local found,failed
for _,record in ipairs(ns.AutomationHistory.List()) do
  if record.reportBody and record.reportBody:find('"marker":"automation-history-seed"',1,true) then found=record end
  if record.reportBody and record.reportBody:find("automation-history-expected-error",1,true) then failed=record end
end
assert(found and found.status=="acknowledged" and found.probeStatus=="completed")
assert(failed and failed.status=="acknowledged" and failed.probeStatus=="failed")
local page=assert(ns.Workbench.GetPage("automation"))
page.SelectRecord(found.requestId)
assert(not page.executeButton:IsEnabled())
assert(ns.AutomationView.GetReportText(found)==found.reportBody)
page.SelectRecord(failed.requestId)
assert(page.statusValue:GetText()==ns.L.AUTO_STATUS_ACKNOWLEDGED)
assert(not page.showNoticeButton and not page.hideNoticeButton)
local scroll=page.rows[1]:GetParent():GetParent()
local range=scroll.verticalRange
if range>0 then
  assert(scroll.scrollbar:IsShown())
  assert(scroll.scrollbar.thumb:GetWidth()==3)
  assert(scroll.scrollbar.thumb:GetHeight()>=24 and scroll.scrollbar.thumb:GetHeight()<=48)
  scroll.scrollbar:SetValue(range)
  assert(math.abs(scroll:GetVerticalScroll()-range)<1)
end
scroll.scrollbar:SetValue(0)
assert(scroll:GetVerticalScroll()==0)
local visibleFailed=false
for _,row in ipairs(page.rows) do
  if row.requestId==failed.requestId then
    assert(row.status:GetText()==ns.L.AUTO_STATUS_FAILED)
    visibleFailed=true
  end
end
assert(visibleFailed,"expected failure must be visible in the newest rows")
return {id=found.requestId,runtime=found.runtime,body=found.reportBody,failedId=failed.requestId,
  count=ns.AutomationView.GetCount(),character=found.character,realm=found.realm,build=found.build,
  scrollRange=range,failedLabel=ns.L.AUTO_STATUS_FAILED}
`;
try {
  const c=await call('connect',['live','connect','--installation',installation,'--pid',pid]);
  report.session=c.session;assert.equal(c.bound,true);
  const seed=await execute('seed','return {marker="automation-history-seed",message="自动化报告跨 reload 保留"}');
  assert.equal(seed.report.ok,true);
  const error=await execute('error','error("automation-history-expected-error")');assert.equal(error.report.ok,false);
  const before=await execute('inspect-before',inspect);assert.equal(before.report.ok,true);
  await capture('before-reload');
  const reload=await call('reload',['live','reload','--session',report.session,'--request','history-reload']);
  assert.equal(reload.complete,true);
  const after=await execute('inspect-after',inspect);assert.equal(after.report.ok,true);
  assert.equal(after.report.result.id,before.report.result.id);
  assert.equal(after.report.result.body,before.report.result.body);
  assert.equal(after.report.result.failedId,before.report.result.failedId);
  assert.equal(after.report.result.character,before.report.result.character);
  assert.notEqual(after.identity.runtime,before.identity.runtime);
  await capture('after-reload');
  const close=await call('disconnect',['live','disconnect',report.session]);assert.equal(close.closed,true);
  report.complete=true;
} catch(error) {report.failure={message:error.message};process.exitCode=1;}
finally {await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({complete:report.complete,report:reportPath,failure:report.failure}));}
