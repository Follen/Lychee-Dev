import test from 'node:test';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
import {mkdtempSync,readFileSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {ownedSpawnSync,auxiliaryRequest} from './rust-owned-process.mjs';

function fixture(t){const directory=mkdtempSync(join(process.cwd(),'.tmp/rust-process-test-'));t.diagnostic(`Preserved process evidence: ${directory}`);return {cwd:process.cwd(),env:process.env,evidence:directory,timeout:15000,orphanMs:200};}
test('Windows Job preserves binary output and exact argv; all handles and tree are reaped',{skip:process.platform!=='win32'},t=>{
  const options=fixture(t),args=['a b','quoted"value','trailing slash \\','中文'];
  const result=ownedSpawnSync(process.execPath,['-e','process.stdout.write(Buffer.from([0,255,128]));process.stderr.write(JSON.stringify(process.argv.slice(1)));',...args],options);
  assert.equal(result.error,undefined);assert.equal(result.status,0);assert.equal(result.handlesReaped,true);
  assert.deepEqual(result.stdout,Buffer.from([0,255,128]));assert.deepEqual(JSON.parse(result.stderr),args);
  assert.equal(result.proof.forced,false);assert.equal(result.proof.treeReaped,true);assert.equal(result.proof.handlesClosed,true);
});
test('parent exit with a real detached child does not count as tree completion',{skip:process.platform!=='win32'},t=>{
  const options=fixture(t),pidFile=join(options.evidence,'owned-child.json');
  const code=`const {spawn}=require('child_process');const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});require('fs').writeFileSync(process.argv[1],JSON.stringify({pid:child.pid}));child.unref();`;
  const result=ownedSpawnSync(process.execPath,['-e',code,pidFile],options);
  const child=JSON.parse(readFileSync(pidFile));assert.ok(child.pid>0);assert.equal(result.status,0);
  assert.match(result.error.message,/forced_tree_termination/);assert.equal(result.proof.parentExitedBeforeChildren,true);assert.equal(result.proof.forced,true);
  assert.equal(result.proof.treeReaped,true);assert.equal(result.proof.activeProcesses,0);assert.equal(result.handlesReaped,true);
  assert.throws(()=>process.kill(child.pid,0),error=>error.code==='ESRCH');
});
test('deadline terminates the exact owned Job and verifies kernel tree exit',{skip:process.platform!=='win32'},t=>{
  const options={...fixture(t),timeout:200};
  const result=ownedSpawnSync(process.execPath,['-e','setInterval(()=>{},1000)'],options);
  assert.ok(result.error);assert.equal(result.proof.deadlineHit,true);assert.equal(result.proof.forced,true);assert.equal(result.handlesReaped,true);assert.equal(result.proof.activeProcesses,0);
});
test('a parent may exit first while its finite child finishes normally within the drain budget',{skip:process.platform!=='win32'},t=>{
  const options={...fixture(t),orphanMs:1500};
  const code=`const child=require('child_process').spawn(process.execPath,['-e','setTimeout(()=>{},200)'],{detached:true,stdio:'ignore'});child.unref();`;
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  assert.equal(result.error,undefined);assert.equal(result.status,0);assert.equal(result.handlesReaped,true);assert.equal(result.proof.parentExitedBeforeChildren,true);assert.equal(result.proof.forced,false);
});
test('unbounded child output is capped and its owned tree is reaped before return',{skip:process.platform!=='win32'},t=>{
  const options=fixture(t),code=`const bytes=Buffer.alloc(65536,65);for(;;)require('fs').writeSync(1,bytes);`;
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  assert.match(result.error.message,/output_budget/);assert.equal(result.handlesReaped,true);assert.equal(result.proof.outputLimit,true);assert.ok(result.stdout.length<=16*1024**2);assert.equal(result.proof.outputReadersComplete,true);
});
test('a caller flag cannot grant the Cargo helper exception to another executable',{skip:process.platform!=='win32'},t=>{
  const options={...fixture(t),cargoBuild:true};
  const code=`const child=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});child.unref();`;
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  assert.equal(result.proof.helperAdmission.state,'disabled');assert.equal(result.proof.helperAdmission.reason,'owned.not_cargo_build');
  assert.equal(result.proof.knownAuxiliaryCleanup,false);assert.match(result.error.message,/forced_tree_termination/);assert.equal(result.handlesReaped,true);
});

test('FILETIME evidence preserves decimal strings for root and actual owned descendants',{skip:process.platform!=='win32'},t=>{
  const options=fixture(t),code=`const child=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});child.unref();`;
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  assert.match(result.error.message,/forced_tree_termination/);assert.equal(result.handlesReaped,true);
  const rows=result.proof.atDrainDeadline;assert.ok(rows.length>0);
  const raw=readFileSync(result.logs.find(path=>path.endsWith('.receipt.json')),'utf8');
  const rawTimes=[...raw.matchAll(/"creationFileTime"\s*:\s*"(\d+)"/g)].map(match=>match[1]);
  const times=[result.proof.creationFileTime,...rows.map(row=>row.creationFileTime)];
  assert.deepEqual(rawTimes.sort(),[...times].sort());
  for(const value of times){assert.equal(typeof value,'string');assert.match(value,/^[1-9]\d+$/);assert.ok(BigInt(value)>BigInt(Number.MAX_SAFE_INTEGER));assert.equal(BigInt(JSON.parse(JSON.stringify(value))).toString(),value);}
  for(const row of rows){assert.equal(row.error,null);assert.equal(row.jobMember,true);assert.ok(BigInt(row.creationFileTime)>=BigInt(result.proof.creationFileTime));}
});


test('Cargo and exact MSVC admission requests are explicit, typed, and mutually exclusive',()=>{
  assert.deepEqual(auxiliaryRequest(),{cargoBuild:false,msvcCompile:false});
  assert.deepEqual(auxiliaryRequest({cargoBuild:true}),{cargoBuild:true,msvcCompile:false});
  assert.deepEqual(auxiliaryRequest({msvcCompile:true}),{cargoBuild:false,msvcCompile:true});
  assert.throws(()=>auxiliaryRequest({cargoBuild:true,msvcCompile:true}),/conflicting_auxiliary_request/);
  for(const options of [{msvcCompile:'true'},{cargoBuild:1},{msvcCompile:null}])assert.throws(()=>auxiliaryRequest(options),/invalid_auxiliary_request/);
});
test('MSVC policy binds exact Enterprise and Community tool identities and same-directory helpers',()=>{
  const policy=JSON.parse(readFileSync(new URL('./rust-build-auxiliary.json',import.meta.url)));
  assert.equal(policy.msvcCompilers.length,2);
  assert.deepEqual(policy.msvcCompilers.map(row=>row.programFilesPath.split('/')[2]),['Enterprise','Community']);
  for(const compiler of policy.msvcCompilers){
  const cl=compiler,link=cl.linker,helper=policy.helpers.find(row=>row.programFilesPath===cl.helperProgramFilesPath);
  assert(helper);assert.equal(cl.kind,'msvc-cl');assert.equal(link.kind,'msvc-link');assert.equal(helper.kind,'msvc-vctip');
  const directory=path=>path.slice(0,path.lastIndexOf('/'));
  assert.equal(directory(cl.programFilesPath),directory(link.programFilesPath));assert.equal(directory(cl.programFilesPath),directory(helper.programFilesPath));
  assert.equal(cl.sha256,'b0a1818ebae6da7f18230a84e76ce839212584ac4c2ab55b97a0d5268085da57');
  assert.equal(link.sha256,'a8c42e25c91ca2453e1110540ac08f0fc12d7539454a11b9af8baeecb469837d');
  for(const entry of [cl,link,helper]){assert.match(entry.sha256,/^[a-f0-9]{64}$/);assert.ok(entry.bytes>0);assert.match(entry.fileVersion,/^\d+\.\d+\.\d+\.\d+$/);assert.match(entry.signerThumbprint,/^[A-F0-9]{40}$/);assert.equal(entry.signerSubject,'CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US');}
  }
  const enterprise=policy.msvcCompilers[0],community=policy.msvcCompilers[1];
  const identity=entry=>Object.fromEntries(['kind','bytes','sha256','fileVersion','signerSubject','signerThumbprint'].map(key=>[key,entry[key]]));
  assert.deepEqual(identity(community),identity(enterprise));
  assert.deepEqual(identity(community.linker),identity(enterprise.linker));
  const helper=compiler=>policy.helpers.find(row=>row.programFilesPath===compiler.helperProgramFilesPath);
  assert.deepEqual(identity(helper(community)),identity(helper(enterprise)));
  assert.equal(community.observation.comment,'https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6214810996');
});
test('a caller flag cannot grant MSVC helper approval to another executable',{skip:process.platform!=='win32'},t=>{
  const options={...fixture(t),msvcCompile:true};
  const code=`const child=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'});child.unref();`;
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  assert.equal(result.proof.helperAdmission.state,'disabled');assert.equal(result.proof.helperAdmission.reason,'owned.not_msvc_compile');
  assert.equal(result.proof.knownAuxiliaryCleanup,false);assert.match(result.error.message,/forced_tree_termination/);assert.equal(result.handlesReaped,true);
});
test('PowerShell MSVC argv admission rejects replacement tools, response files, and injected flags',{skip:process.platform!=='win32'},t=>{
  const options=fixture(t),wrapper=readFileSync(new URL('./rust-owned-process.ps1',import.meta.url),'utf8');
  const start=wrapper.indexOf('function Assert-MsvcCompileArguments('),end=wrapper.indexOf('function Open-ReviewedMsvcTool(',start);
  assert(start>=0&&end>start);
  writeFileSync(join(options.evidence,'sample.c'),'int main(void) { return 0; }\n');
  const script=join(options.evidence,'argv-admission.ps1');
  writeFileSync(script,`$ErrorActionPreference='Stop'
${wrapper.slice(start,end)}
$cwd=$PSScriptRoot
$good=@('/nologo','/Bv','/O2','/MT','/Brepro','/TC',('/Fe:'+(Join-Path $cwd 'sample.exe')),'sample.c','/link','/Brepro','/INCREMENTAL:NO','/OPT:REF','/OPT:ICF')
foreach($name in @('CL','_CL_','LINK','_LINK_')){[Environment]::SetEnvironmentVariable($name,$null)}
Assert-MsvcCompileArguments $good $cwd
$count=1
foreach($badSource in @('@arguments.rsp','/B1:foreign.exe','/B2:foreign.exe','../sample.c','missing.c')){
 $bad=$good.Clone();$bad[7]=$badSource;$rejected=$false
 try{Assert-MsvcCompileArguments $bad $cwd}catch{if($_.Exception.Message -ne 'owned.not_msvc_c_compile'){throw};$rejected=$true}
 if(-not $rejected){throw 'invalid compiler source accepted'};$count++
}
foreach($name in @('CL','_CL_','LINK','_LINK_')){
 [Environment]::SetEnvironmentVariable($name,'/DUNREVIEWED');$rejected=$false
 try{Assert-MsvcCompileArguments $good $cwd}catch{if($_.Exception.Message -ne 'owned.msvc_environment_override'){throw};$rejected=$true}
 [Environment]::SetEnvironmentVariable($name,$null)
 if(-not $rejected){throw 'injected compiler options accepted'};$count++
}
@{passed=$true;cases=$count}|ConvertTo-Json -Compress
`);
  const result=ownedSpawnSync('powershell.exe',['-NoProfile','-NonInteractive','-File',script],{...options,timeout:30000});
  assert.equal(result.error,undefined);assert.equal(result.status,0);assert.equal(result.handlesReaped,true);assert.deepEqual(JSON.parse(result.stdout),{passed:true,cases:10});
});

// These scheduled fixtures execute the actual C#/JavaScript-compatible decision
// blocks with fake kernel calls. They do not execute PowerShell, Windows APIs,
// signature checks, or real processes; unchanged strict retirement is pinned
// separately below. A Windows run remains necessary for the actual Job race.
const ownedWrapper=readFileSync(new URL('./rust-owned-process.ps1',import.meta.url),'utf8');
function sourceBlock(marker){
  const start=ownedWrapper.indexOf(marker);assert.ok(start>=0,`missing ${marker}`);
  assert.equal(ownedWrapper.indexOf(marker,start+marker.length),-1,`duplicate ${marker}`);
  const open=ownedWrapper.indexOf('{',start);let depth=0;
  for(let index=open;index<ownedWrapper.length;index++){
    if(ownedWrapper[index]==='{')depth++;
    if(ownedWrapper[index]==='}'&&--depth===0)return ownedWrapper.slice(start,index+1);
  }
  assert.fail(`unterminated ${marker}`);
}
const rootDrainBlock=sourceBlock('if(result.rootReaped&&Active(job)>0)');
const deadlineDrainBlock=sourceBlock('if(remaining<=0)');
function executeDrainBlock(source,state){
  // No translation or duplicated decision body: these two source blocks use a
  // shared syntax subset. Math.Min is supplied with the same integer behavior.
  const run=new Function('state',`
    const {job,process,reviewedAuxiliary,result,clock,Active,Observe,
      RetireReviewedAuxiliary,TerminateJobObject,Error}=state;
    const Math={Min:globalThis.Math.min};
    let {orphanDeadline,timeoutMs,orphanMs,readerError,remaining}=state;
    do { ${source} } while(false);
    state.orphanDeadline=orphanDeadline;
  `);
  run(state);
}
function drainFixture(){
  const state={job:{owned:true},process:{process:{originalRoot:true}},
    reviewedAuxiliary:['authenticated-helper'],clock:{ElapsedMilliseconds:0},
    timeoutMs:1000,orphanMs:100,orphanDeadline:Infinity,readerError:null,remaining:1,
    rootExitCode:0,members:[{pid:2,reviewed:false},{pid:3,reviewed:true}],events:[],
    result:{rootReaped:false,creationFileTime:'134364964460103438',outputLimit:false,
      error:null,forced:false,knownAuxiliaryCleanup:false,drainCompletedByAccounting:false}};
  state.Active=job=>{assert.equal(job,state.job);return state.members.length;};
  state.Observe=job=>{assert.equal(job,state.job);state.events.push('observe');state.onObserve?.();return state.members.map(row=>({...row}));};
  state.RetireReviewedAuxiliary=(job,root,creation,reviewed,result)=>{
    assert.deepEqual([job,root,creation,reviewed,result],[state.job,state.process.process,state.result.creationFileTime,state.reviewedAuxiliary,state.result]);
    state.events.push('retire');
    if(state.onRetire)return state.onRetire();
    // Stub only the existing strict function's outcome for known/unknown sets.
    // The source identity test below forbids weakening its actual implementation.
    if(!state.result.rootReaped||state.rootExitCode!==0||state.members.length===0||state.members.some(row=>!row.reviewed))return false;
    state.result.auxiliaryProcesses=state.members.map(row=>({...row}));
    state.members=[];state.result.forced=true;return true;
  };
  state.TerminateJobObject=(job,exit)=>{assert.equal(job,state.job);assert.equal(exit,8);state.events.push('force-job');state.members=[];return true;};
  state.Error=step=>new Error(step);
  return state;
}
function rootExit(state,at=10){state.clock.ElapsedMilliseconds=at;state.result.rootReaped=true;executeDrainBlock(rootDrainBlock,state);}
function drainDeadline(state,at=state.orphanDeadline){
  state.clock.ElapsedMilliseconds=at;
  state.remaining=Math.min(state.timeoutMs-at,state.orphanDeadline-at);
  executeDrainBlock(deadlineDrainBlock,state);
}

test('strict auxiliary handle authentication and one-shot checked closes remain byte-exact',()=>{
  const start=ownedWrapper.indexOf(' static bool RetireReviewedAuxiliary('),end=ownedWrapper.indexOf(' static string Quote(',start);
  assert.ok(start>=0&&end>start);
  assert.equal(createHash('sha256').update(ownedWrapper.slice(start,end)).digest('hex'),'b842ca8f89abc26f3928ae2c01e8cc92779c4bf660e70b46a21e80bef86ede2a');
});
test('scheduled root exit then ordinary-child exit retries strict helper retirement at the original drain deadline',()=>{
  const state=drainFixture();rootExit(state);
  assert.deepEqual(state.events,['retire']);assert.equal(state.orphanDeadline,110);
  assert.equal(state.result.knownAuxiliaryCleanup,false);
  state.clock.ElapsedMilliseconds=40;state.members=state.members.filter(row=>row.reviewed);
  drainDeadline(state);
  assert.deepEqual(state.events,['retire','observe','retire']);
  assert.equal(state.result.knownAuxiliaryCleanup,true);assert.equal(state.result.forced,true);
  assert.equal(state.members.length,0);assert.equal(state.result.deadlineHit,undefined);
  assert.equal(state.orphanDeadline,110);assert.equal(state.timeoutMs,1000);
  assert.deepEqual(state.result.atDrainDeadline,[{pid:3,reviewed:true}]);
});
test('an unknown descendant still alive at the drain deadline cannot become reviewed cleanup',()=>{
  const state=drainFixture();rootExit(state);drainDeadline(state);
  assert.deepEqual(state.events,['retire','observe','retire','force-job']);
  assert.equal(state.result.knownAuxiliaryCleanup,false);assert.equal(state.result.forced,true);
  assert.equal(state.result.deadlineHit,false);
});
test('hard deadline, output limit, reader failure, prior error, and a live root prohibit the deadline retry',()=>{
  for(const [name,change] of [
    ['hard deadline',state=>{state.timeoutMs=110;}],
    ['output limit',state=>{state.result.outputLimit=true;}],
    ['reader failure',state=>{state.readerError='fixture.reader_failure';}],
    ['prior error',state=>{state.result.error='fixture.prior_error';}],
    ['live root',state=>{state.result.rootReaped=false;}],
  ]){
    const state=drainFixture();rootExit(state);state.members=state.members.filter(row=>row.reviewed);change(state);drainDeadline(state);
    assert.deepEqual(state.events,['retire','observe','force-job'],name);
    assert.equal(state.result.knownAuxiliaryCleanup,false,name);assert.equal(state.result.forced,true,name);
    assert.equal(state.result.deadlineHit,name==='hard deadline',name);
  }
});
test('the hard deadline is rechecked after drain observation consumes the remaining budget',()=>{
  const state=drainFixture();rootExit(state);state.members=state.members.filter(row=>row.reviewed);
  state.onObserve=()=>{state.clock.ElapsedMilliseconds=state.timeoutMs;};drainDeadline(state);
  assert.deepEqual(state.events,['retire','observe','force-job']);
  assert.equal(state.result.deadlineHit,true);assert.equal(state.result.knownAuxiliaryCleanup,false);
});
test('a nonzero root exit cannot acquire successful helper cleanup at either decision point',()=>{
  const state=drainFixture();state.rootExitCode=1;rootExit(state);state.members=state.members.filter(row=>row.reviewed);drainDeadline(state);
  assert.deepEqual(state.events,['retire','observe','retire','force-job']);assert.equal(state.result.knownAuxiliaryCleanup,false);
});
test('accounting is rechecked after a refused retirement whose last member exited concurrently',()=>{
  const state=drainFixture();rootExit(state);
  state.onRetire=()=>{state.members=[];return false;};drainDeadline(state);
  assert.deepEqual(state.events,['retire','observe','retire']);
  assert.equal(state.result.drainCompletedByAccounting,true);assert.equal(state.result.forced,false);
  assert.equal(state.result.knownAuxiliaryCleanup,false);
});
test('accounting completion before or during observation needs no retry or forced termination',()=>{
  for(const phase of ['before','during']){
    const state=drainFixture();rootExit(state);
    if(phase==='before')state.members=[];else state.onObserve=()=>{state.members=[];};
    drainDeadline(state);assert.equal(state.result.drainCompletedByAccounting,true);
    assert.deepEqual(state.events,phase==='before'?['retire']:['retire','observe']);assert.equal(state.result.forced,false);
  }
});
test('duplicate root notifications cannot restart either original deadline',()=>{
  const state=drainFixture();rootExit(state);rootExit(state,50);
  assert.equal(state.orphanDeadline,110);assert.equal(state.timeoutMs,1000);
  state.members=state.members.filter(row=>row.reviewed);drainDeadline(state);
  assert.deepEqual(state.events,['retire','retire','observe','retire']);assert.equal(state.result.knownAuxiliaryCleanup,true);
});
test('root notification retirement also rejects existing reader errors without waiting for the drain retry',()=>{
  for(const field of ['readerError','error']){
    const state=drainFixture();state.members=state.members.filter(row=>row.reviewed);
    if(field==='readerError')state.readerError='fixture.reader_failure';else state.result.error='fixture.prior_error';
    rootExit(state);drainDeadline(state);
    assert.deepEqual(state.events,['observe','force-job']);assert.equal(state.result.knownAuxiliaryCleanup,false);
  }
});
test('a strict-retirement close failure propagates rather than retrying the uncertain handle',()=>{
  const state=drainFixture();state.onRetire=()=>{throw new Error('owned.close_auxiliary_handle');};
  assert.throws(()=>rootExit(state),/owned.close_auxiliary_handle/);
  assert.deepEqual(state.events,['retire']);assert.equal(state.result.knownAuxiliaryCleanup,false);
});
test('an ordinary-child exit before the drain deadline does not trigger an early deadline retry',()=>{
  const state=drainFixture();rootExit(state);state.members=state.members.filter(row=>row.reviewed);
  drainDeadline(state,50);
  assert.deepEqual(state.events,['retire']);assert.equal(state.result.knownAuxiliaryCleanup,false);
  assert.equal(state.orphanDeadline,110);assert.equal(state.timeoutMs,1000);
  drainDeadline(state);assert.equal(state.result.knownAuxiliaryCleanup,true);
});

test('native command clock excludes wrapper startup and includes finite descendant drain', {skip:process.platform!=='win32'}, t => {
  const options={...fixture(t),orphanMs:3000};
  const code=`const child=require('child_process').spawn(process.execPath,['-e','setTimeout(()=>{},750)'],{detached:true,stdio:'ignore'});child.unref();`;
  const wall=performance.now();
  const result=ownedSpawnSync(process.execPath,['-e',code],options);
  const wallMillis=performance.now()-wall;
  assert.equal(result.error,undefined);assert.equal(result.handlesReaped,true);
  const timing=result.proof.timing;assert.equal(timing.schema,'lycheedev.owned-timing.v1');
  for(const key of ['startTicks','rootExitTicks','treeReapedTicks'])assert.match(timing[key],/^[1-9]\d*$/);
  const start=BigInt(timing.startTicks),exit=BigInt(timing.rootExitTicks),end=BigInt(timing.treeReapedTicks);
  assert.ok(start<=exit&&exit<=end);
  const millis=Number(end-start)*1000/timing.frequency;
  assert.ok(millis>=700,'finite worker lifetime must remain in measured command');
  assert.ok(millis<wallMillis,'PowerShell startup is outside native command interval');
  assert.equal(result.proof.forced,false);
});

test('startup diagnostics timestamp durable ownership and keep CPU outside the original interval', () => {
  const owner = readFileSync(new URL('./rust-owned-process.ps1', import.meta.url), 'utf8');
  const marker = text => { const at = owner.indexOf(text); assert(at >= 0, text); return at; };
  const order = [
    'result.commandStartTicks=Stopwatch.GetTimestamp();', 'if(!CreateProcessW(binary,command',
    'if(startupDiagnostics)result.createReturnTicks=', 'if(startupDiagnostics)result.rootPersistBeginTicks=',
    'Persist(rootIdentity,rootJson);',
    'if(startupDiagnostics)result.rootPersistEndTicks=', 'memory=new ToolkitMemory.Observer(',
    'if(startupDiagnostics)result.observerSetupEndTicks=', 'if(startupDiagnostics)result.resumeBeforeTicks=',
    'if(ResumeThread(process.thread)', 'if(startupDiagnostics)result.resumeAfterTicks=',
    'result.treeReapedTicks=Stopwatch.GetTimestamp();', 'if(startupDiagnostics&&result.rootReaped)try{',
    'GetProcessTimes(process.process,out cpuCreated,out cpuExit,out cpuKernel,out cpuUser)',
    'if(memory!=null)try{result.memory=memory.Stop();}',
  ].map(marker);
  for (let i = 1; i < order.length; i++) assert(order[i] > order[i - 1]);
  assert(owner.includes('catch(Exception diagnostic){result.diagnosticError=diagnostic.ToString();}'));
  assert(owner.includes("if($requestData.startupDiagnostics -eq $true){\n $receipt.diagnostics="));
});
test('startup diagnostics use six ordered native timestamps and the held root CPU handle', {skip:process.platform!=='win32'}, t => {
  const options = {...fixture(t), startupDiagnostics: true};
  const result = ownedSpawnSync(process.execPath, ['-e', 'process.stdout.write("startup-diagnostic");'], options);
  assert.equal(result.status, 0); assert.equal(result.error, undefined); assert.equal(result.handlesReaped, true);
  assert.equal(result.stdout.toString(), 'startup-diagnostic');
  const d = result.proof.diagnostics, timing = result.proof.timing;
  assert.equal(d.schema, 'lycheedev.owned-startup-diagnostics.v1'); assert.equal(d.error, null); assert.equal(d.rootCpuObserved, true);
  const ticks = [timing.startTicks, ...['createReturnTicks', 'rootPersistBeginTicks', 'rootPersistEndTicks', 'observerSetupEndTicks', 'resumeBeforeTicks', 'resumeAfterTicks'].map(name => d[name]), timing.rootExitTicks, timing.treeReapedTicks].map(BigInt);
  for (let i = 1; i < ticks.length; i++) assert(ticks[i] >= ticks[i - 1]);
  for (const name of ['rootKernel100ns', 'rootUser100ns']) assert.match(d[name], /^(0|[1-9]\d*)$/);
});
