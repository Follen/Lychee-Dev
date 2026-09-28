import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';

// Explicit target only. This is an interactive acceptance runner, never a unit
// test or an npm hook. A failed run retains its connection for manual/resume work.
const [cliArg, projectArg, installation, pid] = process.argv.slice(2);
if (!cliArg || !projectArg || !installation || !/^\d+$/.test(pid ?? '')) {
  throw new Error('usage: public-baseline.mjs <cli.exe> <new-project> <installation> <pid>');
}
const cli=path.resolve(cliArg), project=path.resolve(projectArg);
await fs.mkdir(project,{recursive:true});
const reportPath=path.join(project,'baseline.json');
const report={schema:'lycheedev.native-live-baseline.v1',project,installation,pid:Number(pid),complete:false,steps:[]};
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
let session;
async function call(name,args) {
  const argv=[...args,'--project',project,'--wait-seconds','120','--format','json'];
  const started=Date.now();
  const result=await new Promise((resolve,reject)=>{
    const child=spawn(cli,argv,{windowsHide:true,shell:false,stdio:['ignore','pipe','pipe']});
    let stdout='',stderr='';
    const timer=setTimeout(()=>{child.kill();reject(new Error(`timeout: ${name}; retain ${session}`));},145000);
    child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
    child.stdout.on('data',b=>{stdout+=b;if(stdout.length>4*1024*1024)child.kill();});
    child.stderr.on('data',b=>{if(stderr.length<65536)stderr+=b;});
    child.on('error',e=>{clearTimeout(timer);reject(e);});
    child.on('close',code=>{clearTimeout(timer);resolve({code,stdout,stderr});});
  });
  await fs.writeFile(path.join(project,name+'.json'),result.stdout);
  const envelope=JSON.parse(result.stdout.replace(/^\uFEFF/,''));
  report.steps.push({name,argv,exitCode:result.code,elapsedMs:Date.now()-started,session:envelope.result?.session,operation:envelope.result?.operation,complete:envelope.result?.complete,cleanup:envelope.result?.cleanup,error:envelope.error});
  await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify(report.steps.at(-1)));
  assert.equal(result.code,0,`${name}: ${result.stderr} ${JSON.stringify(envelope.error)}`);
  assert.equal(envelope.ok,true,name);
  return envelope.result;
}
function complete(result) {
  assert.equal(result.complete,true);
  assert.equal(result.reportState,'verified');
  assert.equal(result.cleanup,'complete');
}
async function execute(key,fixture,name=key) {
  const result=await call(name,['live','execute','--session',session,'--request',key,'--file',path.resolve('tests/channel-live/fixtures',fixture),'--budget-seconds','5','--policy','observation']);
  complete(result);
  return result;
}
try {
  const connected=await call('connect',['live','connect','--installation',installation,'--pid',pid,'--no-cache']);
  session=connected.session;
  report.session=session;
  assert.equal(connected.bound,true);
  assert.equal(connected.identity.inventory,64);
  const runtimes=new Set([connected.identity.runtime]);
  let first;
  for(let n=1;n<=13;n++) {
    const r=await execute(`normal-${n}`,'normal.lua');
    assert.equal(r.report.ok,true);
    assert.equal(r.report.result.marker,'native-slot-baseline');
    runtimes.add(r.identity.runtime);
    first ??= r.operation;
  }
  assert.ok(runtimes.size>=2,'capacity must trigger a verified runtime transition');
  const before=await fs.readFile(path.join(project,'.lycheedev','live','connections',session+'.jsonl'));
  const retry=await execute('normal-1','normal.lua','normal-1-retry');
  assert.equal(retry.operation,first);
  const after=await fs.readFile(path.join(project,'.lycheedev','live','connections',session+'.jsonl'));
  assert.deepEqual(before,after,'historical retry must be read-only');
  const segments=await fs.readdir(path.join(project,'.lycheedev','live','connections',session+'.jsonl.history'));
  assert.ok(segments.length>0,'connection history should rotate');
  const syntax=await execute('compile-error','compile_error.lua');
  assert.equal(syntax.report.ok,false);
  assert.equal(syntax.report.error.kind,'compile_error');
  const cleanup=await execute('cleanup-error','cleanup_error.lua');
  assert.equal(cleanup.report.resourcesReleased,false);
  assert.equal(cleanup.cleanupMethod,'runtime_destroyed');
  assert.equal(cleanup.reload.phase,'complete');
  const bugs=await call('bugs',['live','bugs','--session',session,'--request','bugs','--count','20']);
  complete(bugs);
  assert.equal(bugs.report.result.complete,bugs.report.result.snapshot?.complete===true);
  const closed=await call('disconnect',['live','disconnect',session]);
  assert.equal(closed.closed,true);
  const again=await call('disconnect-retry',['live','disconnect',session]);
  assert.equal(again.closed,true);
  report.complete=true;
} catch(error) {
  report.failure={message:error.message,session,resume:session?`live resume ${session} --project ${project}`:undefined};
  process.exitCode=1;
} finally {
  await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify({report:reportPath,complete:report.complete,failure:report.failure}));
}
