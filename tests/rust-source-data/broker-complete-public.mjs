import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
const root=resolve(process.cwd());
const release=resolve(process.env.LYCHEEDEV_RELEASE_ROOT||'.tmp/rust-full-tools-npm-stage-04/installed/node_modules/lycheedev');
const bin=resolve(process.env.LYCHEEDEV_RUST_BIN||join(release,'native/windows-amd64/lycheedev.exe'));
const home=resolve(process.env.LYCHEEDEV_ENVIRONMENT_HOME||'.tmp/rust-source-broker-complete-input-01/home');
const pin=process.env.LYCHEEDEV_ENVIRONMENT_PIN||'PIN-9e101198396228da82d822287f9ada729e75e3669384bac22b8ceb750b04d281';
const out=resolve(process.argv[2]||'.tmp/rust-source-broker-complete-public-01');mkdirSync(out,{recursive:true});
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const initial={binary:hash(bin),release:hash(join(release,'release.json'))};
const leaseTool=resolve(process.env.LYCHEEDEV_BROKER_LEASE_TOOL||'target/debug/examples/check_broker_lease.exe');
const leaseToolSHA256=hash(leaseTool);
const records=[];let started=false;let failure;
function lease(name,expected){const result=spawnSync(leaseTool,[home,pin],{windowsHide:true,encoding:'utf8',timeout:10000,maxBuffer:1<<20});writeFileSync(join(out,`${name}.stdout.json`),result.stdout||'');writeFileSync(join(out,`${name}.stderr.txt`),result.stderr||'');records.push({name,exit:result.status,testOnlyProbe:true});if(result.status!==expected)throw new Error(`${name} lease probe exit ${result.status}, expected ${expected}`);}
function run(name,args){const time=performance.now();const result=spawnSync(bin,[...args,'--home',home,'--format','json'],{cwd:root,windowsHide:true,encoding:'utf8',shell:false,timeout:100000,maxBuffer:16<<20});writeFileSync(join(out,`${name}.stdout.json`),result.stdout||'');writeFileSync(join(out,`${name}.stderr.txt`),result.stderr||'');let envelope;try{envelope=JSON.parse(result.stdout);}catch{};const record={name,args,exit:result.status,elapsedMs:performance.now()-time,error:result.error?.message,envelope};records.push(record);if(result.error||result.status!==0||!envelope?.ok)throw new Error(`${name}: ${result.error?.message||result.status} ${envelope?.error?.message||result.stderr}`);return envelope;}
function status(name){return run(name,['source','session','status','--release',release]).result;}
function verify(name,envelope){for(const capture of envelope.captures||[]){const checked=run(`${name}-${capture.id}`,['evidence','verify',capture.id]);if(checked.result?.verified!==true)throw new Error(`capture ${capture.id} was not verified`);}}
try{
 const absent=status('absent');if(absent.state!=='closed')throw new Error('Existing broker is owned; this fixture will not retire it');
 let workerPid;
 const analyses=[];
 for(const [i,symbol] of ['First','Second','Third'].entries()){
  started=true;
  const envelope=run(`analysis-${symbol}`,['source','refs','--snapshot',pin,'--symbol',symbol,'--direction','both','--session-reuse','--release',release]);
  analyses.push(envelope);verify(`proof-${symbol}`,envelope);
  if(envelope.result.complete!==true||envelope.result.semanticAnalysis?.state!=='complete'||envelope.result.semanticAnalysis?.cacheHit===true)throw new Error(`${symbol} is not a fresh complete semantic request`);
  const retained=status(`status-${symbol}`);
  if(retained.state!=='ready'||retained.batches!==i+1||!(retained.workerPid>0))throw new Error(`${symbol} retained batch count differs`);
  if(i===0)workerPid=retained.workerPid;else if(retained.workerPid!==workerPid)throw new Error('Distinct CLI requests changed worker PID');
 }
 lease('active-lease-refused',6);
 const cached=run('complete-cache-hit',['source','refs','--snapshot',pin,'--symbol','First','--direction','both','--session-reuse','--release',release]);verify('cache-proof',cached);
 if(cached.result.complete!==true||cached.result.semanticAnalysis?.cacheHit!==true)throw new Error('Complete semantic cache was not authenticated and reused');
 const cachedStatus=status('cache-status');if(cachedStatus.workerPid!==workerPid||cachedStatus.batches!==3)throw new Error('Cache hit ran another semantic batch');
 const pruned=run('prune',['source','prune','--target-bytes','0']);if(pruned.result.complete!==true)throw new Error('Authenticated broker retirement did not permit complete prune');
 started=false;
 if(status('pruned-status').state!=='closed')throw new Error('Prune did not retire broker');
 lease('pruned-lease-acquired',0);
 if(run('idempotent-close',['source','session','close','--release',release]).result.state!=='closed')throw new Error('close is not idempotent');
 const processCheck=spawnSync('powershell',['-NoProfile','-NonInteractive','-Command',`if (Get-Process -Id ${workerPid} -ErrorAction SilentlyContinue) { exit 1 } else { exit 0 }`],{windowsHide:true,encoding:'utf8',timeout:5000});
 if(processCheck.status!==0)throw new Error('Observed retained LuaLS PID survives close');

}catch(error){failure=error.stack||String(error);}finally{
 if(started){try{run('owned-cleanup',['source','session','close','--release',release]);}catch(error){records.push({name:'owned-cleanup-failure',error:String(error)});}}
}
const final={binary:hash(bin),release:hash(join(release,'release.json'))};
const stable=initial.binary===final.binary&&initial.release===final.release;
const leaseToolStable=leaseToolSHA256===hash(leaseTool);
const report={schema:'lycheedev.source-broker-public-test.v1',passed:!failure&&stable&&leaseToolStable,binary:bin,release,home,pin,initial,final,artifactStable:stable,leaseTool,leaseToolSHA256,leaseToolStable,records,failure,scope:'actual packaged CLI with complete fixed synthetic source/catalog: First/Second/Third retain exact PID/batches3, authenticated complete semantic cache hit keeps batches3, prune retires/reaps worker and releases lease; caller cancellation/replay/foreign peer/performance matrices remain pending'};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({passed:report.passed,failure,records:records.map(({name,exit,elapsedMs})=>({name,exit,elapsedMs})),artifactStable:stable}));if(!report.passed)process.exitCode=1;
