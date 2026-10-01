import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import assert from 'node:assert/strict';

// Explicit target only. This is an interactive acceptance runner, never a unit
// test or an npm hook. A failed run retains its connection for manual/resume work.
const [cliArg, projectArg, installation, pid, resumeArg] = process.argv.slice(2);
if (!cliArg || !projectArg || !installation || !/^\d+$/.test(pid ?? '')) {
  throw new Error('usage: public-baseline.mjs <cli.exe> <project> <installation> <pid> [--resume]');
}
assert.ok(!resumeArg || resumeArg==='--resume','only --resume is accepted');
const cli=path.resolve(cliArg), project=path.resolve(projectArg);
await fs.mkdir(project,{recursive:true});
const originalReportPath=path.join(project,'baseline.json');
const readJSON=async file=>JSON.parse((await fs.readFile(file,'utf8')).replace(/^\uFEFF/,''));
const previous=resumeArg?await readJSON(originalReportPath):null;
const originalConnection=previous?await readJSON(path.join(project,'connect.json')):null;
const attempt=previous?`resumed-${Date.now()}-${process.pid}`:'';
if(previous){
  assert.equal(previous.complete,false,'a passed baseline does not need resuming');
  assert.equal(path.resolve(previous.project),project);
  assert.equal(path.resolve(previous.installation).toLowerCase(),path.resolve(installation).toLowerCase());
  assert.equal(previous.pid,Number(pid),'resume must retain the original PID');
  assert.ok(originalConnection.result?.bound,'resume requires the original successful connection');
  assert.match(originalConnection.result.session,/^CON-[0-9a-f]{32}$/);
  if(previous.session)assert.equal(previous.session,originalConnection.result.session);
}
const reportPath=previous?path.join(project,`baseline-${attempt}.json`):originalReportPath;
const report={schema:'lycheedev.native-live-baseline.v1',project,installation,pid:Number(pid),complete:false,steps:[]};
if(previous){report.resumes=originalReportPath;report.previousFailure=previous.failure;report.session=originalConnection.result.session;}
await fs.writeFile(reportPath,JSON.stringify(report,null,2),{flag:'wx'});
let session=report.session;
async function call(name,args) {
  const argv=[...args,'--project',project,...(args[1]==='status'?[]:['--wait-seconds','120']),'--format','json'];
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
  await fs.writeFile(path.join(project,(attempt?attempt+'-':'')+name+'.json'),result.stdout,{flag:'wx'});
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
  let connected;
  if(previous){
    connected=originalConnection.result;
    const status=await call('resume-status',['live','status',session]);
    assert.equal(status.session,session);
    assert.equal(status.closed,false,'closed connections cannot resume this baseline');
    assert.equal(status.bound,true,'recover the original binding before resuming the baseline');
    assert.equal(status.identity.guid,connected.identity.guid);
    assert.equal(status.identity.build,connected.identity.build);
    assert.equal(status.identity.product,connected.identity.product);
    assert.ok(!['budget_exhausted','wait_active_driver','needs_decision'].includes(status.continuation?.kind),
      'resolve the original continuation before resuming the baseline');
    assert.ok(status.continuation?.remainingBudgetMs==null || status.continuation.remainingBudgetMs>0,
      'the original goal budget is exhausted');
    assert.ok(!['execution_unknown','cancelled'].includes(status.operationState),'unfinished business outcome cannot be replayed');
    if(!status.complete){
      assert.notEqual(status.continuation?.kind,'wait_external','resolve the recorded external blocker first');
      const recovered=await call('resume-original',['live','resume',session]);
      assert.equal(recovered.complete,true,'original goal must complete before further baseline work');
      assert.equal(recovered.session,session);
    }
  }else{
    connected=await call('connect',['live','connect','--installation',installation,'--pid',pid,'--character','Qingtianjiuz','--realm','时光I','--no-cache']);
  }
  session=connected.session;
  report.session=session;
  assert.equal(connected.bound,true);
  assert.equal(connected.identity.guid,'Player-6379-01045F6F');
  assert.equal(connected.identity.product,'titan');
  assert.equal(connected.identity.slots,200);
  assert.equal(connected.identity.inventory,200);
  const capacityCommands=Math.floor((connected.identity.slots-16)/4)+1;
  report.slotCount=connected.identity.slots;
  const runtimes=new Set([connected.identity.runtime]);
  let first;
  for(let n=1;n<=capacityCommands;n++) {
    const r=await execute(`normal-${n}`,'normal.lua');
    assert.equal(r.report.ok,true);
    assert.equal(r.report.result.marker,'native-slot-baseline');
    assert.equal(r.identity.guid,'Player-6379-01045F6F');
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
