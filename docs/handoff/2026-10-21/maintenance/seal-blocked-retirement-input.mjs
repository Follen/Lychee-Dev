// Read existing evidence and create NEW blocked receipts only; never cleans cache.
import {readFileSync,writeFileSync,readdirSync,lstatSync,realpathSync,existsSync,mkdirSync,openSync,closeSync,readSync,fsyncSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {join,resolve,dirname,relative,isAbsolute} from 'node:path';
const ROOT=process.argv[3];
function check(x,m){if(!x)throw new Error(m);}
check(process.platform==='win32'&&process.argv.length===4&&process.argv[2]==='--worktree'&&typeof ROOT==='string'&&isAbsolute(ROOT),'Usage: node script --worktree ABSOLUTE_PATH');
check(realpathSync(ROOT).toLowerCase()===resolve(ROOT).toLowerCase(),'Worktree alias/reparse not allowed');
const root=realpathSync(ROOT), inv=join(root,'.tmp/agent-unified-cfdef-inventory');
const git=spawnSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'});check(git.status===0&&git.stdout.trim()==='cfdefd46a11ec8dce470d296b424a3eaa1372b8b','Maintenance HEAD changed');
function safe(p){const a=resolve(p),r=relative(root,a);check(r&&!r.startsWith('..')&&!isAbsolute(r),'Outside root');let c=root;for(const n of r.split(/[\\/]/)){c=join(c,n);if(existsSync(c))check(!lstatSync(c).isSymbolicLink(),'Reparse');}check(!/^(target|target-symbols)([\\/]|$)/i.test(r),'Evidence in cache');return a;}
function hash(p){safe(p);const h=createHash('sha256'),fd=openSync(p,'r'),b=Buffer.alloc(1024*1024);try{for(;;){const n=readSync(fd,b,0,b.length,null);if(!n)break;h.update(b.subarray(0,n));}return h.digest('hex');}finally{closeSync(fd);}}
const protectedMap=new Map();function protect(p){p=safe(p);check(lstatSync(p).isFile(),'Nonfile evidence');const row={path:relative(root,p),bytes:lstatSync(p).size,sha256:hash(p)};protectedMap.set(p.toLowerCase(),row);return row;}
function referenced(name,container){check(typeof name==='string'&&name.length>0,'Invalid reference');const candidates=isAbsolute(name)?[name]:[resolve(root,name),resolve(dirname(container),name)];const unique=[...new Map(candidates.filter(existsSync).map(p=>[realpathSync(safe(p)).toLowerCase(),safe(p)])).values()];check(unique.length===1,'Missing or ambiguous manifest reference');return unique[0];}
const manifests=[join(root,'.tmp/agent-unified-cfdef/default-check.evidence-manifest.json'),join(root,'.tmp/agent-unified-cfdef/oracle-check.evidence-manifest.json'),join(root,'.tmp/agent-unified-cfdef-metadata/evidence-manifest.json'),...readdirSync(inv).filter(n=>n.endsWith('.evidence-manifest.json')).sort().map(n=>join(inv,n))];check(manifests.length===112,'Manifest count changed');
let references=0;for(const p of manifests){const m=JSON.parse(readFileSync(safe(p))), sealPath=p.endsWith('evidence-manifest.json')?p.slice(0,-'evidence-manifest.json'.length)+'seal-verification.json':null;const seal=JSON.parse(readFileSync(safe(sealPath)));check(seal.state==='sealed'&&seal.manifestSHA256===hash(p),'Unsealed/corrupt manifest');check(referenced(seal.manifest,sealPath).toLowerCase()===p.toLowerCase(),'Seal bound to other manifest');const arrays=Array.isArray(m)?[m]:[m.files,m.entries,m.artifacts].filter(Array.isArray);check(arrays.length===1,'Unsupported/ambiguous manifest layout');protect(p);protect(sealPath);for(const x of arrays[0]){const f=referenced(x.file,p),r=protect(f);check(r.bytes===x.bytes&&r.sha256===x.sha256,'Evidence mismatch');references++;}}
check(references===2135,'Reference count changed');
const failedStageFiles=['launch.json','queue-before.json','queue-after.json','terminal.json','wrapper.stdout.log','wrapper.stderr.log'].map(s=>relative(root,join(inv,'core-lib-lycheedev_core-ignored-list.'+s)));for(const p of failedStageFiles)protect(join(root,p));
const stagePrefix=join(inv,'core-lib-lycheedev_core-ignored-list.');
// Windows PowerShell writes UTF-8 JSON and UTF-16LE redirected logs, both with BOMs.
// Decode only for inspection; hashes and protected inventories retain the original bytes.
function powerShellText(bytes){return bytes.toString(bytes[0]===0xFF&&bytes[1]===0xFE?'utf16le':'utf8').replace(/^\uFEFF/,'');}
function readPowerShellJson(path){return JSON.parse(powerShellText(readFileSync(path)));}
const terminal=readPowerShellJson(stagePrefix+'terminal.json'),launch=readPowerShellJson(stagePrefix+'launch.json');
check(terminal.id==='core-lib-lycheedev_core-ignored-list'&&terminal.wrapperExit===1&&terminal.endedAtUtc==='2026-10-20T23:55:03.9064499Z'&&terminal.collection==='direct invocation and LASTEXITCODE','Wrong quota terminal');
check(launch.id===terminal.id&&launch.owner==='ucfdef-core-lib-lycheedev_core-ignored-list'&&launch.startedAtUtc==='2026-10-20T23:55:02.5006081Z'&&launch.timeoutMs===600000&&launch.stack==='default'&&launch.oneShot===false,'Wrong quota invocation');
for(const obj of [terminal,launch])for(const key of ['cargoStarted','cargoExit','buildEvidence','receipt'])check(!(key in obj),'Unexpected Cargo lifecycle evidence');
const stdout=readFileSync(stagePrefix+'wrapper.stdout.log'),stderr=readFileSync(stagePrefix+'wrapper.stderr.log');
const stderrText=powerShellText(stderr);
check(stdout.length===0&&stderr.length===2598&&stderrText.includes('disk.cache_quota')&&!/buildEvidence|receipt/i.test(stderrText),'Not the verified pre-Cargo quota failure');
const runner=join(root,'tools/rust-build.mjs'),disk=join(root,'tools/rust-disk.mjs');
check(hash(disk)==='54122b03b7b474f1f2119a31caa85d2217aeb5ea97e65f28a7e0161bebc203e5','Admission implementation changed');
check(hash(runner)==='8dc9f54f295bbf586e78098a01127ef461723235a78b4c2c21070fedd4b4a46c','Runner ordering changed');protect(disk);protect(runner);

for(const n of readdirSync(inv).filter(n=>n==='targets.json'||n.endsWith('.inventory.json')||n.endsWith('.listing.json')))protect(join(inv,n));
// Do not overwrite or claim completion of the missing inventory-root final artifacts.
for(const n of ['inventory.json','batch-result.json','evidence-manifest.json','seal-verification.json'])check(!existsSync(join(inv,n)),'Historical batch state changed; review first');
const out=safe(join(root,'.tmp/rust-disk/evidence','blocked-before-incremental-'+Date.now()));mkdirSync(out,{recursive:false});
function durable(name,value){const p=join(out,name),fd=openSync(p,'wx');try{writeFileSync(fd,JSON.stringify(value,null,2)+'\n');fsyncSync(fd);}finally{closeSync(fd);}return p;}
const stage=durable('quota-stage.blocked.json',{schema:'lycheedev.blocked-stage.v1',state:'blocked',reason:'disk.cache_quota',cargoStarted:false,cargoOwnerCreated:false,wrapperExit:1,cargoExit:null,lifecycleBasis:'Verified quota rejection in published beginScratch admission before UUID creation; launch.owner is only planned label',actualBusinessCases:0,files:failedStageFiles.map(p=>protectedMap.get(join(root,p).toLowerCase()))});protect(stage);
const batch=durable('batch.blocked.json',{schema:'lycheedev.blocked-batch.v1',state:'blocked',commit:'cfdefd46a11ec8dce470d296b424a3eaa1372b8b',actualBusinessCases:0,completedSealedManifests:112,verifiedReferences:references,allCaseEnumerationComplete:false,failedStage:relative(root,stage),qualification:'list_only_no_tests_executed'});protect(batch);
const input=durable('retirement-input.json',{schema:'lycheedev.incremental-retirement-input.v1',root,commit:'cfdefd46a11ec8dce470d296b424a3eaa1372b8b',protectedFiles:[...protectedMap.values()],failedStageFiles,blockedBatch:{state:'blocked',actualBusinessCases:0,reason:'disk.cache_quota before Cargo',stageSealPath:relative(root,stage),batchSummaryPath:relative(root,batch)}});
console.log(JSON.stringify({input,sha256:hash(input),manifests:112,references,authorization:'not_granted_by_this_script'}));
