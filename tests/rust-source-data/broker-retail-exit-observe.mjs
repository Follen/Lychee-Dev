import {spawn} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,existsSync,lstatSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
const release=resolve(process.env.LYCHEEDEV_RELEASE_ROOT||'.tmp/rust-full-tools-npm-stage-10/installed/node_modules/lycheedev');
const bin=resolve(process.env.LYCHEEDEV_RUST_BIN||join(release,'native/windows-amd64/lycheedev.exe'));
const home=resolve(process.env.LYCHEEDEV_ENVIRONMENT_HOME||'.tmp/rust-source-matrix-four-baseline-01/home');
const pin=process.env.LYCHEEDEV_ENVIRONMENT_PIN||'PIN-f5ca7507c049580ad68831744678771cc49525719fdc901a91c739db2871b38b';
const out=resolve(process.argv[2]||'.tmp/rust-source-broker-retail-exit-observe-01');if(existsSync(out))throw new Error('Fresh observation directory required');mkdirSync(out,{recursive:true});
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const before={binary:hash(bin),release:hash(join(release,'release.json'))};
const records=[];let failure,owned=false,observer,observerTerminal,debuggerTerminal;
// Preserve the optional new control-process admission diagnostic even when a
// first request fails before LuaLS is spawned. This file never grants input,
// process ownership or test success: the original kernel handle still does.
function resourceDiagnostic(phase){
 const path=join(home,'tmp/source-broker-resource-last.json');
 if(!existsSync(path)){records.push({name:'resource-diagnostic',phase,state:'absent'});return;}
 try{
  const info=lstatSync(path);
  if(!info.isFile()||info.isSymbolicLink()||info.size>(64<<10))throw new Error('Invalid bounded resource diagnostic');
  const raw=readFileSync(path),value=JSON.parse(raw);
  writeFileSync(join(out,`resource-${phase}.json`),raw);
  records.push({name:'resource-diagnostic',phase,state:'observed',bytes:raw.length,sha256:createHash('sha256').update(raw).digest('hex'),value,scope:'optional non-authoritative phase telemetry; identity and freshness require independent binding'});
 }catch(error){records.push({name:'resource-diagnostic',phase,state:'unavailable',error:String(error)});}
}
async function run(name,args){
 const started=performance.now();
 const child=spawn(bin,[...args,'--home',home,'--format','json'],{windowsHide:true,stdio:['ignore','pipe','pipe']});
 let stdout='',stderr='',error;
 const append=(kind,chunk)=>{if(kind==='stdout')stdout+=chunk;else stderr+=chunk;if(Buffer.byteLength(stdout)+Buffer.byteLength(stderr)>(16<<20)){error='CLI output budget exceeded';child.kill();}};
 child.stdout.on('data',chunk=>append('stdout',chunk));child.stderr.on('data',chunk=>append('stderr',chunk));
 const timer=setTimeout(()=>{error='CLI invocation deadline exceeded';child.kill();},100000);
 const exit=await new Promise(resolve=>{child.once('error',e=>{error=e.message;resolve(null);});child.once('close',resolve);});clearTimeout(timer);
 writeFileSync(join(out,`${name}.stdout.json`),stdout);writeFileSync(join(out,`${name}.stderr.txt`),stderr);
 let value;try{value=JSON.parse(stdout);}catch{}
 records.push({name,exit,elapsedMs:performance.now()-started,envelope:value,error});return {exit,envelope:value};
}
try {
 resourceDiagnostic('before');
 const absent=await run('absent',['source','session','status','--release',release]);
 if(absent.exit!==0||absent.envelope?.result?.state!=='closed')throw new Error('fixture requires absent broker');
 owned=true;
 const first=await run('first',['source','refs','--snapshot',pin,'--symbol','CallbackRegistryMixin:RegisterCallback','--direction','incoming','--limit','10','--session-reuse','--release',release]);
 resourceDiagnostic('after-first');
 if(first.exit!==0)throw new Error('first semantic analysis failed');
 if(first.envelope?.result?.semanticAnalysis?.cacheHit!==false)throw new Error('first request did not execute an actual semantic batch');
 const identity=JSON.parse(readFileSync(join(home,'tmp/source-broker-last-run.json'),'utf8'));
 let raw='',stderr='';
 observer=spawn('powershell',['-NoProfile','-NonInteractive','-File',resolve('tests/rust-source-data/observe-broker-process.ps1'),'-TargetPid',String(identity.pid),'-ExpectedCreated',identity.created],{windowsHide:true,stdio:['ignore','pipe','pipe']});
 const terminal=new Promise((resolve,reject)=>{observer.once('error',reject);observer.once('close',code=>resolve(code));});
 const ready=new Promise((resolve,reject)=>{observer.stdout.on('data',chunk=>{raw+=chunk;if(raw.length>8192){reject(new Error('observer output budget'));observer.kill();return;}writeFileSync(join(out,'observer.stdout.jsonl'),raw);if(raw.includes('\n'))resolve();});observer.once('error',reject);observer.once('exit',()=>{if(!raw.includes('\n'))reject(new Error('observer exited before holding original process'));});});
 observer.stderr.on('data',chunk=>{stderr+=chunk;if(stderr.length>8192)observer.kill();else writeFileSync(join(out,'observer.stderr.txt'),stderr);});
 observerTerminal=terminal.then(exit=>{records.push({name:'original-process-exit',exit,frames:raw.trim().split(/\r?\n/).map(s=>JSON.parse(s))});return exit;});
 await ready;
 records.push({name:'original-process-held',value:JSON.parse(raw.split(/\r?\n/)[0])});
 if(process.env.LYCHEEDEV_BROKER_DEBUGGER){
  const symbolRoot=resolve(process.env.LYCHEEDEV_BROKER_SYMBOLS||join(release,'native/windows-amd64'));
  if(process.env.LYCHEEDEV_BROKER_SYMBOLS){
   const symbols=JSON.parse(readFileSync(join(symbolRoot,'artifact.json'),'utf8'));
   if(symbols.sameInvocation!==true||symbols.binarySHA256!==before.binary||hash(join(symbolRoot,'lycheedev.exe'))!==before.binary||hash(join(symbolRoot,'lycheedev.pdb'))!==symbols.pdbSHA256)throw new Error('Symbol artifact does not match this exact executable/PDB pair');
   records.push({name:'matched-symbol-artifact',binarySHA256:before.binary,pdbSHA256:symbols.pdbSHA256});
  }
  const commandFile=join(out,'debugger-commands.txt');
  const onException='".echo LYCHEEDEV_SECOND_CHANCE; .lastevent; .exr -1; .ecxr; kv 256; ~* k 64; lmvm lycheedev; .dump /m broker.dmp; .detach; q"';
  // Microsoft sx documentation: sxd leaves first chance unhandled and -c2
  // runs only when that exception reaches its second chance. No system-wide
  // WER/AeDebug configuration or remote symbol server is used.
  writeFileSync(commandFile,`sxd -c2 ${onException} av\nsxd -c2 ${onException} 0xc0000409\nsxd -c2 ${onException} 0xe06d7363\n.echo LYCHEEDEV_DEBUG_READY\ng\n`);
  const debuggerChild=spawn(process.env.LYCHEEDEV_BROKER_DEBUGGER,['-p',String(identity.pid),'-cf',commandFile,'-y',symbolRoot,'-logo',join(out,'debugger.log'),'-G'],{cwd:out,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,_NT_SYMBOL_PATH:symbolRoot,_NT_ALT_SYMBOL_PATH:''}});
  let debugRaw='',debugError='';
  debuggerTerminal=new Promise((resolve,reject)=>{debuggerChild.once('error',reject);debuggerChild.once('close',exit=>{records.push({name:'owned-debugger-exit',exit});resolve(exit);});});
  await new Promise((resolve,reject)=>{
   debuggerChild.stdout.on('data',chunk=>{debugRaw+=chunk;if(debugRaw.length>(1<<20)){reject(new Error('debugger output budget exceeded'));debuggerChild.kill();return;}writeFileSync(join(out,'debugger.stdout.txt'),debugRaw);if(/(^|[\r\n])LYCHEEDEV_DEBUG_READY[\r\n]/.test(debugRaw))resolve();});
   debuggerChild.once('error',reject);debuggerChild.once('exit',()=>{if(!/(^|[\r\n])LYCHEEDEV_DEBUG_READY[\r\n]/.test(debugRaw))reject(new Error('debugger exited before attaching to exact original process'));});
  });
  debuggerChild.stderr.on('data',chunk=>{debugError+=chunk;if(debugError.length>(1<<20))debuggerChild.kill();else writeFileSync(join(out,'debugger.stderr.txt'),debugError);});
  records.push({name:'original-process-debugger-attached',pid:identity.pid,created:identity.created,scope:'only authenticated task broker; local symbols; dump remains private task-local evidence'});
 }
 if(process.env.LYCHEEDEV_BROKER_OBSERVE_SECOND==='1'){
  const second=await run('second',['source','refs','--snapshot',pin,'--symbol','CallbackRegistryMixin:RegisterCallback','--direction','incoming','--limit','10','--session-reuse','--release',release]);
  resourceDiagnostic('after-second');
  if(second.exit!==0)failure='Second actual semantic request failed; original process exit is separately observed';
  if(second.envelope?.result?.semanticAnalysis?.cacheHit!==false)failure='Second request did not execute an actual semantic batch';
 }
 const status=await run('status',['source','session','status','--release',release]);
 const afterStatus=JSON.parse(readFileSync(join(home,'tmp/source-broker-last-run.json'),'utf8'));
 records.push({name:'lifecycle-after-status',value:afterStatus});
 if(afterStatus.pid!==identity.pid||afterStatus.created!==identity.created)failure='Original broker PID/creation changed: automatic replacement is not retained-session success';
 if(status.envelope?.result?.state!=='ready')failure='Original broker is not ready after semantic requests';
 if(process.env.LYCHEEDEV_BROKER_OBSERVE_SECOND==='1'&&status.envelope?.result?.batches!==2)failure='Original broker did not retain both actual semantic batches';
 const close=await run('owned-close',['source','session','close','--release',release]);owned=false;
 if(close.exit!==0)throw new Error('owned close failed');
 const observedExit=await observerTerminal;
 writeFileSync(join(out,'observer.stdout.jsonl'),raw);writeFileSync(join(out,'observer.stderr.txt'),stderr);
 if(observedExit!==0)throw new Error('held process observer failed');
 const originalExit=records.find(r=>r.name==='original-process-exit')?.frames.find(f=>f.phase==='exited');
 if(!originalExit||originalExit.pid!==identity.pid||originalExit.created!==identity.created||originalExit.exitCode!==0)failure='Original held broker did not retire with kernel exit 0';
 if(status.exit!==0)failure='Actual status request failed; original process exit is separately observed, without inferring its cause';
}catch(error){failure=error.stack||String(error);}finally{
 if(owned)await run('owned-cleanup',['source','session','close','--release',release]);
 if(observerTerminal){try{await observerTerminal;}catch(error){records.push({name:'original-process-observer-failure',error:String(error)});}}
 if(debuggerTerminal){try{await debuggerTerminal;}catch(error){records.push({name:'owned-debugger-failure',error:String(error)});}}
 resourceDiagnostic('after-cleanup');
}
const after={binary:hash(bin),release:hash(join(release,'release.json'))};
const stable=JSON.stringify(before)===JSON.stringify(after);
const report={passed:!failure&&stable,artifactStable:stable,before,after,release,home,pin,records,failure,scope:'test-only actual packaged partial Retail request then status, original broker kernel handle held before second request and exact exit code; no process is killed by the observer; not complete lifecycle certification'};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));if(!report.passed)process.exitCode=1;
