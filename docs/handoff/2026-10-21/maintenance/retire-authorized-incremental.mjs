// One-off, explicit-authority maintenance. Not a product cache policy.
import {createHash} from 'node:crypto';
import {openSync,closeSync,readSync,readFileSync,writeFileSync,lstatSync,readdirSync,realpathSync,unlinkSync,rmdirSync,mkdirSync,fsyncSync,existsSync} from 'node:fs';
import {resolve,join,relative,isAbsolute,dirname} from 'node:path';
import {spawnSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';
const ROOT=process.argv[3];
const HEAD='cfdefd46a11ec8dce470d296b424a3eaa1372b8b';
function assert(ok,message){if(!ok)throw new Error(message);}
assert(process.platform==='win32','Windows only');
assert(process.argv.length===6 && process.argv[2]==='--worktree' && process.argv[4]==='--execute-authorized' && typeof ROOT==='string' && isAbsolute(ROOT),'Usage after explicit authorization: node script --worktree ABSOLUTE_PATH --execute-authorized sealed-input.json');
assert(realpathSync(ROOT).toLowerCase()===resolve(ROOT).toLowerCase(),'Worktree alias/reparse not allowed');
const root=realpathSync(ROOT), target=join(root,'target/debug/incremental');
const specPath=resolve(process.argv[5]), spec=JSON.parse(readFileSync(specPath));
assert(spec.schema==='lycheedev.incremental-retirement-input.v1' && spec.root===root && spec.commit===HEAD,'Wrong input identity');
assert(Array.isArray(spec.protectedFiles)&&spec.protectedFiles.length>0,'Missing authorization/protected inventory');
assert(Array.isArray(spec.failedStageFiles)&&spec.failedStageFiles.length===6,'Exactly six failed-stage files required');
assert(spec.blockedBatch?.state==='blocked' && spec.blockedBatch?.actualBusinessCases===0,'Missing truthful blocked batch');
function under(p){const a=resolve(root,p),r=relative(root,a);assert(r && !r.startsWith('..')&&!isAbsolute(r),'Outside worktree');return a;}
function digest(p){const fd=openSync(p,'r'), h=createHash('sha256'),b=Buffer.alloc(1024*1024);try{for(;;){const n=readSync(fd,b,0,b.length,null);if(!n)break;h.update(b.subarray(0,n));}return h.digest('hex');}finally{closeSync(fd);}}
function ps(code){const r=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-Command',code],{encoding:'utf8',maxBuffer:16*1024*1024});assert(r.status===0,`PowerShell inspection failed: ${r.stderr}`);return r.stdout;}
// Native attribute inspection refuses every reparse type, not merely Node symlinks.
function noReparse(){ps(`$ErrorActionPreference='Stop'; $root='${ROOT.replaceAll("'","''")}'; $p=$root; while($p){$i=Get-Item -LiteralPath $p -Force;if($i.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'ancestor reparse'};$p=Split-Path -Parent $p}; $q=[Collections.Generic.Queue[string]]::new();$q.Enqueue($root);while($q.Count){$d=$q.Dequeue();foreach($i in Get-ChildItem -LiteralPath $d -Force){if($i.Attributes -band [IO.FileAttributes]::ReparsePoint){throw ('reparse '+$i.FullName)};if($i.PSIsContainer){$q.Enqueue($i.FullName)}}}`);}
function idle(){const raw=ps(`$ErrorActionPreference='Stop'; @(Get-CimInstance Win32_Process | Where-Object { $_.Name -match '^(cargo|rustc|rustdoc|lycheedev|lycheedev-cli|vctip|link|lld-link)[.]exe$' -or ($_.ExecutablePath -and $_.ExecutablePath.StartsWith('${join(ROOT,"target").replaceAll("'","''")}',[StringComparison]::OrdinalIgnoreCase)) -or ($_.Name -eq 'node.exe' -and $_.ProcessId -ne ${process.pid} -and $_.CommandLine -match 'rust-(cargo|build|owned|supervis)|agent_native|unified.*(test|check)') } | Select-Object ProcessId,Name,ExecutablePath,CommandLine) | ConvertTo-Json -Compress`);const parsed=raw.trim()?JSON.parse(raw):[];assert(Array.isArray(parsed)?parsed.length===0:false,'Producer/consumer still present');return [];}
function files(base){const out=[];function walk(p){const s=lstatSync(p);assert(!s.isSymbolicLink(),'Reparse');if(s.isDirectory()){for(const n of readdirSync(p).sort())walk(join(p,n));}else{assert(s.isFile(),'Nonregular entry');out.push({path:relative(root,p),bytes:s.size,sha256:digest(p)});}}walk(base);return out;}
const git=spawnSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'});assert(git.status===0&&git.stdout.trim()===HEAD,'HEAD changed');
noReparse();idle();
assert(digest(join(root,'tools/rust-disk.mjs'))==='54122b03b7b474f1f2119a31caa85d2217aeb5ea97e65f28a7e0161bebc203e5','Published lease implementation changed');
const {acquireChannel}=await import(pathToFileURL(join(root,'tools/rust-disk.mjs')).href);
let ordinary,symbols,receipt,record,save,successResult,sequence=0;
try{
 ordinary=acquireChannel(root,'ordinary','authorized-incremental-retirement');
 symbols=acquireChannel(root,'symbols','authorized-incremental-retirement');
 noReparse();idle();
 const evidence=join(root,'.tmp/rust-disk/evidence',`incremental-retirement-${Date.now()}`);mkdirSync(evidence);receipt=null;
 const protectedFiles=spec.protectedFiles.map(x=>{const p=under(x.path);assert(p.toLowerCase()!==target.toLowerCase()&&!p.toLowerCase().startsWith(target.toLowerCase()+'\\'),'Protected reference inside removal scope');assert(/^[a-f0-9]{64}$/.test(x.sha256)&&digest(p)===x.sha256,'Protected hash mismatch');return x;});
 for(const p of spec.failedStageFiles)assert(protectedFiles.some(x=>x.path===p),'Failed-stage file must be preserved');
 const inputRelative=relative(root,under(specPath));assert(!/^(target|target-symbols)([\\/]|$)/i.test(inputRelative),'Input inside cache');protectedFiles.push({path:inputRelative,sha256:digest(specPath)});
 for(const key of ['stageSealPath','batchSummaryPath']){const p=spec.blockedBatch[key];assert(typeof p==='string'&&protectedFiles.some(x=>x.path===p),'Blocked seal not protected');const sealed=JSON.parse(readFileSync(under(p)));assert(sealed.state==='blocked'&&sealed.actualBusinessCases===0,'Blocked seal mismatch');}
 // Cover all binaries, libraries, symbols and every other cache file outside the one allowed directory.
 const retained=[];for(const name of ['target','target-symbols']){const base=join(root,name);if(!existsSync(base))continue;function visit(p){if(p.toLowerCase()===target.toLowerCase())return;const s=lstatSync(p);assert(!s.isSymbolicLink(),'Reparse');if(s.isDirectory())for(const n of readdirSync(p))visit(join(p,n));else{assert(s.isFile(),'Nonregular');retained.push({path:relative(root,p),bytes:s.size,sha256:digest(p)});}}visit(base);}
 const inventory=files(target);assert(inventory.reduce((n,x)=>n+x.bytes,0)===6819246069,'Inventory no longer matches authorized snapshot');assert(inventory.every(x=>! /\.(exe|pdb)$/i.test(x.path)),'Unexpected executable/symbol in incremental');
 record={schema:'lycheedev.incremental-retirement.v1',state:'prepared',root,commit:HEAD,scope:relative(root,target),inputSHA256:digest(specPath),blockedBatch:spec.blockedBatch,failedStageFiles:spec.failedStageFiles,protectedFiles,retained,inventory,beforeBytes:inventory.reduce((n,x)=>n+x.bytes,0),deleted:[],startedAt:new Date().toISOString()};
 save=function(){const next=join(evidence,`retirement-${String(sequence++).padStart(4,'0')}.json`);const fd=openSync(next,'wx');try{writeFileSync(fd,JSON.stringify(record,null,2)+'\n');fsyncSync(fd);}finally{closeSync(fd);}receipt=next;};
 save(); // Durable failed-stage hashes and whole-batch blocked record BEFORE any removal.
 noReparse();idle();
 assert(JSON.stringify(files(target))===JSON.stringify(inventory),'Incremental changed during inventory');
 for(const x of inventory){const p=under(x.path);assert(p.startsWith(target+'\\')&&digest(p)===x.sha256,'Changed deletion target');unlinkSync(p);record.deleted.push(x.path);}
 function emptyDirs(p){for(const n of readdirSync(p)){const c=join(p,n);assert(lstatSync(c).isDirectory(),'Unexpected new file');emptyDirs(c);}rmdirSync(p);}
 emptyDirs(target);
 for(const x of [...protectedFiles,...retained])assert(digest(under(x.path))===x.sha256,'Protected file changed');
 idle();record.state='data_retired_pending_lease_release';record.completedAt=new Date().toISOString();record.remainingIncrementalBytes=0;save();successResult={state:'retired',receipt,removedLogicalBytes:record.beforeBytes,protectedFiles:protectedFiles.length,retainedFiles:retained.length};
}catch(e){if(record&&save){record.state='stopped';record.error=e.message;record.stoppedAt=new Date().toISOString();try{save();}catch(persistError){console.error(JSON.stringify({state:'receipt_persistence_failed',error:persistError.message,lastDurableReceipt:receipt}));}}console.error(JSON.stringify({state:'stopped',error:e.message,receipt,partialRemovalPossible:!!receipt}));process.exitCode=1;}
finally{for(const lease of [symbols,ordinary]){try{lease?.release();}catch(e){console.error(JSON.stringify({state:'lease_release_failed',lease:lease?.path,error:e.message,receipt}));process.exitCode=1;}}}

if(successResult&&process.exitCode!==1){record.state='retired';record.leasesReleased=true;try{save();successResult.receipt=receipt;console.log(JSON.stringify(successResult));}catch(e){console.error(JSON.stringify({state:'final_receipt_failed',error:e.message,lastDurableReceipt:receipt,leasesReleased:true}));process.exitCode=1;}}
