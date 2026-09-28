import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

const [cliArg,hostArg,projectArg,installation,pid]=process.argv.slice(2);
assert.ok(cliArg&&hostArg&&projectArg&&path.isAbsolute(installation)&&/^\d+$/.test(pid??''),
  'usage: input-policy-baseline.mjs <cli> <host> <new-project> <installation> <pid>');
const cli=path.resolve(cliArg),host=path.resolve(hostArg),project=path.resolve(projectArg);
await fs.mkdir(project,{recursive:true});
const report={complete:false,installation,pid:Number(pid),steps:[]};
const reportPath=path.join(project,'input-policy.json');
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
async function run(exe,args,name) {
  const start=Date.now();
  const value=await new Promise((resolve,reject)=>{
    const child=spawn(exe,args,{shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
    let out='',err='';
    const timer=setTimeout(()=>{child.kill();reject(new Error(`${name} exceeded host deadline`));},145000);
    child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
    child.stdout.on('data',b=>{out+=b;if(out.length>4*1024*1024)child.kill();});
    child.stderr.on('data',b=>{if(err.length<65536)err+=b;});
    child.on('error',e=>{clearTimeout(timer);reject(e);});
    child.on('close',code=>{clearTimeout(timer);resolve({code,out,err});});
  });
  await fs.writeFile(path.join(project,name+'.json'),value.out||JSON.stringify(value));
  report.steps.push({name,args,exitCode:value.code,elapsedMs:Date.now()-start});
  assert.equal(value.code,0,`${name}: ${value.err} ${value.out.slice(0,1000)}`);
  return exe===cli?JSON.parse(value.out.replace(/^\uFEFF/,'')):null;
}
async function call(args,name) {
  return run(cli,[...args,'--project',project,'--wait-seconds','120','--format','json'],name);
}
async function execute(fixture,budget) {
  const e=await call(['live','execute','--session',report.session,'--request',fixture,'--file',
    path.resolve('tests/channel-live/fixtures',fixture+'.lua'),'--budget-seconds',String(budget),'--policy','observation'],fixture);
  assert.equal(e.result.complete,true);assert.equal(e.result.cleanup,'complete');
  assert.equal(e.result.reportState,'verified');
  return e.result.report;
}
async function capture(name) {
  return run(host,['-mode','capture','-installation',installation,'-pid',pid,'-project',path.join(project,name),'-timeout','20'],name);
}
try {
  const c=await call(['live','connect','--installation',installation,'--pid',pid],'connect');
  report.session=c.result.session;assert.equal(c.result.bound,true);
  const pending=Promise.allSettled([execute('input_policy',20)]);
  for(let i=1;i<=4;i++) {
    await new Promise(resolve=>setTimeout(resolve,4000));
    await capture('phase-'+i);
  }
  const [settled]=await pending;
  assert.equal(settled.status,'fulfilled',String(settled.reason));
  assert.equal(settled.value.ok,true);
  assert.deepEqual(settled.value.result,{protected:true,during:'connecting',released:true,after:'probe'});
  for(const [fixture,budget,reason] of [['input_timeout',10,'input_protection_timeout'],
    ['input_error',15,'input_protection_fixture_error'],['scene_guard',10,'scene_changed']]) {
    const r=await execute(fixture,budget);
    assert.equal(r.ok,false);assert.ok(JSON.stringify(r.error).includes(reason));
    assert.equal(r.resourcesReleased,true);
  }
  const released=await execute('input_released',5);assert.equal(released.result.released,true);
  const close=await call(['live','disconnect',report.session],'disconnect');assert.equal(close.result.closed,true);
  await capture('closed');
  report.complete=true;
} catch(error) {
  report.failure={message:error.message};process.exitCode=1;
} finally {
  await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify({report:reportPath,complete:report.complete,failure:report.failure}));
}
