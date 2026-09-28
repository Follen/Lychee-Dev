import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

const [cliArg,hostArg,projectArg,installation,pid,mode='after']=process.argv.slice(2);
assert.ok(cliArg&&hostArg&&projectArg&&path.isAbsolute(installation)&&/^\d+$/.test(pid??'')&&['before','after'].includes(mode));
const cli=path.resolve(cliArg),host=path.resolve(hostArg),project=path.resolve(projectArg);
await fs.mkdir(project,{recursive:true});
const report={complete:false,installation,pid:Number(pid),mode,steps:[]};
const reportPath=path.join(project,'workbench.json');
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
async function run(exe,args,name) {
  const start=Date.now();
  const r=await new Promise((resolve,reject)=>{
    const child=spawn(exe,args,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
    let out='',err='';
    const timer=setTimeout(()=>{child.kill();reject(new Error(`timeout ${name}; retain ${report.session}`));},145000);
    child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
    child.stdout.on('data',b=>{out+=b;if(out.length>4*1024*1024)child.kill();});
    child.stderr.on('data',b=>{if(err.length<65536)err+=b;});
    child.on('error',e=>{clearTimeout(timer);reject(e);});
    child.on('close',code=>{clearTimeout(timer);resolve({code,out,err});});
  });
  await fs.writeFile(path.join(project,name+'.json'),r.out||JSON.stringify(r));
  report.steps.push({name,args,exitCode:r.code,elapsedMs:Date.now()-start});
  await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');
  assert.equal(r.code,0,`${name}: ${r.err} ${r.out.slice(0,1000)}`);
  console.log(name);
  return exe===cli?JSON.parse(r.out.replace(/^\uFEFF/,'')).result:null;
}
const call=(name,args)=>run(cli,[...args,'--project',project,'--wait-seconds','120','--format','json'],name);
async function execute(name,code) {
  const file=path.join(project,name+'.lua');await fs.writeFile(file,code);
  const r=await call(name,['live','execute','--session',report.session,'--request',name,'--file',file,'--budget-seconds','5','--policy','observation']);
  assert.equal(r.complete,true);assert.equal(r.cleanup,'complete');assert.equal(r.report.ok,true,JSON.stringify(r.report));
  return r;
}
try {
  const connected=await call('connect',['live','connect','--installation',installation,'--pid',pid]);
  report.session=connected.session;assert.equal(connected.bound,true);
  for(const key of ['runner','objects','events','trace','diagnostics','exports','automation','about']) {
    const code=`local ns=LycheeDevInternal
assert(ns.Workbench.ShowPage("${key}"))
local page=assert(ns.Workbench.GetPage("${key}"))
${mode==='before'?`local focused=GetCurrentKeyBoardFocus and GetCurrentKeyBoardFocus();if focused then focused:ClearFocus() end` : `assert(not (GetCurrentKeyBoardFocus and GetCurrentKeyBoardFocus()),"page captured keyboard focus")`}
${mode==='after'?`assert(not ns.SettingsPage and not ns.Workbench.ShowSettings and not LycheeToolkitWindow.settingsButton)` : ''}
if "${key}"=="automation" then
  ${mode==='after'?`assert(not page.showNoticeButton and not page.hideNoticeButton)` : ''}
  for _,id in ipairs(ns.AutomationView.GetOrder()) do
    local record=ns.AutomationView.GetRecord(id)
    if record.probeStatus=="failed" then page.SelectRecord(id);break end
  end
  ${mode==='after'?`for _,row in ipairs(page.rows) do
    local record=ns.AutomationView.GetRecord(row.requestId)
    if record and record.probeStatus=="failed" then assert(row.status:GetText()==ns.L.AUTO_STATUS_FAILED) end
  end` : ''}
end
return {page="${key}",shown=ns.Workbench.IsShown()}`;
    await execute(key,code);
    await run(host,['-mode','capture','-installation',installation,'-pid',pid,'-project',path.join(project,key),'-timeout','20'],key+'-capture');
  }
  await execute('return-automation','assert(LycheeDevInternal.Workbench.ShowPage("automation"));return true');
  const closed=await call('disconnect',['live','disconnect',report.session]);assert.equal(closed.closed,true);
  report.complete=true;
} catch(error) {report.failure={message:error.message};process.exitCode=1;}
finally {await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({complete:report.complete,report:reportPath,failure:report.failure}));}
