import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync,existsSync,statSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {createHash} from 'node:crypto';
const root=resolve(process.cwd());
const release=resolve(process.env.LYCHEEDEV_RELEASE_ROOT||'.tmp/rust-full-tools-npm-stage-04/installed/node_modules/lycheedev');
const bin=resolve(process.env.LYCHEEDEV_RUST_BIN||join(release,'native/windows-amd64/lycheedev.exe'));
const home=resolve(process.env.LYCHEEDEV_ENVIRONMENT_HOME||'.tmp/rust-source-matrix-four-baseline-01/home');
const pin=process.env.LYCHEEDEV_ENVIRONMENT_PIN||'PIN-f5ca7507c049580ad68831744678771cc49525719fdc901a91c739db2871b38b';
const out=resolve(process.argv[2]||'.tmp/rust-source-broker-public-01');mkdirSync(out,{recursive:true});
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const initial={binary:hash(bin),release:hash(join(release,'release.json'))};
const records=[];let started=false;let failure;
function lifecycle(){const path=join(home,'tmp/source-broker-last-run.json');if(!existsSync(path))return {available:false};if(statSync(path).size>4096)throw new Error('Lifecycle diagnostic exceeds budget');const value=JSON.parse(readFileSync(path,'utf8'));if(!Number.isInteger(value.pid)||value.pid<=0)throw new Error('Invalid lifecycle PID');const checked=spawnSync('powershell',['-NoProfile','-NonInteractive','-Command',`$p=Get-Process -Id ${value.pid} -ErrorAction SilentlyContinue; if($p){ [string]$p.StartTime.ToUniversalTime().ToFileTimeUtc() }; exit 0`],{windowsHide:true,encoding:'utf8',timeout:5000});return {...value,osObserved:checked.status===0,osCreated:checked.stdout?.trim()||null};}
function run(name,args){const time=performance.now();const result=spawnSync(bin,[...args,'--home',home,'--format','json'],{cwd:root,windowsHide:true,encoding:'utf8',shell:false,timeout:100000,maxBuffer:16<<20});writeFileSync(join(out,`${name}.stdout.json`),result.stdout||'');writeFileSync(join(out,`${name}.stderr.txt`),result.stderr||'');let envelope;try{envelope=JSON.parse(result.stdout);}catch{};const record={name,args,exit:result.status,elapsedMs:performance.now()-time,error:result.error?.message,envelope};records.push(record);if(result.error||result.status!==0||!envelope?.ok)throw new Error(`${name}: ${result.error?.message||result.status} ${envelope?.error?.message||result.stderr}`);return envelope;}
function status(name){return run(name,['source','session','status','--release',release]).result;}
function verify(name,envelope){for(const capture of envelope.captures||[]){const checked=run(`${name}-${capture.id}`,['evidence','verify',capture.id]);if(checked.result?.verified!==true)throw new Error(`capture ${capture.id} was not verified`);}}
try{
 const absent=status('absent');if(absent.state!=='closed')throw new Error('Existing broker is owned; this fixture will not retire it');
 const args=['source','refs','--snapshot',pin,'--symbol','CallbackRegistryMixin:RegisterCallback','--direction','incoming','--limit','10','--session-reuse','--release',release];
 started=true;
 const first=run('first-analysis',args);records.push({name:'first-broker-lifecycle',lifecycle:lifecycle()});verify('first-proof',first);
 const firstStatus=status('first-status');records.push({name:'status-broker-lifecycle',lifecycle:lifecycle()});if(firstStatus.state!=='ready'||!(firstStatus.workerPid>0)||firstStatus.batches!==1)throw new Error('First analysis did not retain one initialized worker');
 const second=run('second-analysis',args);verify('second-proof',second);
 const secondStatus=status('second-status');if(secondStatus.state!=='ready'||secondStatus.workerPid!==firstStatus.workerPid||secondStatus.batches!==2)throw new Error('Second independent CLI did not reuse exact worker PID');
 if(first.result.semanticAnalysis?.cacheHit===true||second.result.semanticAnalysis?.cacheHit===true)throw new Error('Partial Retail catalog incorrectly admitted a complete semantic cache entry');
 const closed=run('close',['source','session','close','--release',release]).result;if(JSON.stringify(closed)!==JSON.stringify({state:'closed'}))throw new Error('close public legacy shape differs');
 started=false;
 if(status('closed-status').state!=='closed')throw new Error('Broker remained available after authenticated retirement');
 if(run('idempotent-close',['source','session','close','--release',release]).result.state!=='closed')throw new Error('close is not idempotent');
 const processCheck=spawnSync('powershell',['-NoProfile','-NonInteractive','-Command',`if (Get-Process -Id ${firstStatus.workerPid} -ErrorAction SilentlyContinue) { exit 1 } else { exit 0 }`],{windowsHide:true,encoding:'utf8',timeout:5000});
 if(processCheck.status!==0)throw new Error('Observed retained LuaLS PID survives close');
 if(!first.result?.semanticAnalysis||!second.result?.semanticAnalysis)throw new Error('Public refs omitted actual semantic analysis');
}catch(error){failure=error.stack||String(error);}finally{
 if(started){try{run('owned-cleanup',['source','session','close','--release',release]);}catch(error){records.push({name:'owned-cleanup-failure',error:String(error)});}}
}
const final={binary:hash(bin),release:hash(join(release,'release.json'))};
const stable=initial.binary===final.binary&&initial.release===final.release;
const report={schema:'lycheedev.source-broker-public-test.v1',passed:!failure&&stable,binary:bin,release,home,pin,initial,final,artifactStable:stable,records,failure,scope:'actual packaged CLI, opt-in startup single-use intent, two separate CLI semantic requests retained PID, status/close/idempotence, actual capture verification; caller cancellation/replay/foreign peer/performance matrices remain pending'};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({passed:report.passed,failure,records:records.map(({name,exit,elapsedMs})=>({name,exit,elapsedMs})),artifactStable:stable}));if(!report.passed)process.exitCode=1;
