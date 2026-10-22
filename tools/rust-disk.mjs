// Project-local disk admission and explicit ownership; no age-based cleanup.
import {closeSync,copyFileSync,existsSync,lstatSync,mkdirSync,openSync,readFileSync,readdirSync,realpathSync,renameSync,rmSync,statfsSync,writeFileSync} from 'node:fs';
import {createHash,randomUUID} from 'node:crypto';
import {dirname,isAbsolute,join,relative,resolve,sep} from 'node:path';
const GiB=1024**3;
export const defaults=Object.freeze({cacheBytes:50*GiB,scratchBytes:5*GiB,freeReserveBytes:2*GiB,maxEntries:500000});
export const sha256=bytes=>createHash('sha256').update(bytes).digest('hex');
export function policy(root,env=process.env){let limits={...defaults};const path=join(root,'release/rust-disk-policy.json');if(existsSync(path)){if(lstatSync(path).size>16384)throw new Error('disk.policy_budget');const document=JSON.parse(readFileSync(path));if(document.schema!=='lycheedev.disk-policy.v1')throw new Error('disk.unknown_policy');for(const key of Object.keys(defaults)){if(document[key]!==undefined)limits[key]=document[key];}}
  for(const [name,key]of [['LYCHEEDEV_DISK_CACHE_GIB','cacheBytes'],['LYCHEEDEV_DISK_SCRATCH_GIB','scratchBytes'],['LYCHEEDEV_DISK_FREE_GIB','freeReserveBytes']])if(env[name]!==undefined){if(!/^\d+(\.\d+)?$/.test(env[name]))throw new Error('disk.invalid_policy');limits[key]=Number(env[name])*GiB;}
  for(const[key,value]of Object.entries(limits))if(!Number.isSafeInteger(value)||value<0||(key==='maxEntries'&&value===0))throw new Error('disk.invalid_policy');return limits;
}
export function contained(root,path){const base=realpathSync(root),absolute=resolve(path),rel=relative(base,absolute);if(!rel||rel==='..'||rel.startsWith(`..${sep}`)||isAbsolute(rel))throw new Error('disk.path_outside_owned_root');let cursor=base;for(const part of rel.split(sep)){cursor=join(cursor,part);if(existsSync(cursor)&&lstatSync(cursor).isSymbolicLink())throw new Error('disk.reparse_boundary');}return absolute;}
export function usage(path,maxEntries=defaults.maxEntries,{skipReparse=false}={}){let bytes=0,entries=0,reparseEntries=0;const stack=[path];while(stack.length){const current=stack.pop();if(!existsSync(current))continue;const info=lstatSync(current);if(info.isSymbolicLink()){if(!skipReparse)throw new Error('disk.reparse_boundary');reparseEntries++;continue;}if(++entries>maxEntries)throw new Error('disk.inventory_budget');if(info.isDirectory()){for(const child of readdirSync(current))stack.push(join(current,child));}else if(info.isFile()){bytes+=info.size;if(!Number.isSafeInteger(bytes))throw new Error('disk.inventory_budget');}else throw new Error('disk.non_regular_entry');}return {bytes,entries,reparseEntries};}
export function admission(root,{limits,requiredBytes=0,cacheReserveBytes=0,scratchReserveBytes=0,inventory=usage,freeBytes,legacyInventory=false}={}){
  limits??=policy(root);
  root=realpathSync(root);const caches=['target','target-symbols'].map(name=>({path:join(root,name),...inventory(join(root,name),limits.maxEntries)}));
  const scratch=inventory(join(root,'.tmp/rust-scratch'),limits.maxEntries);const cacheBytes=caches.reduce((n,x)=>n+x.bytes,0);
  let legacy={state:'not_measured',scope:'Legacy tmp inventory is opt-in and is never treated as managed scratch or automatically reclaimable.'};
  if(legacyInventory){const allTmp=inventory(join(root,'.tmp'),limits.maxEntries,{skipReparse:true}),evidence=inventory(join(root,'.tmp/rust-disk/evidence'),limits.maxEntries);legacy={state:'snapshot',observedAt:new Date().toISOString(),tmpLogicalBytes:allTmp.bytes,unclassifiedLogicalBytes:Math.max(0,allTmp.bytes-scratch.bytes-evidence.bytes),reparseEntries:allTmp.reparseEntries||0,scope:'Unregistered tmp includes protected artifacts/evidence and legacy caches; it is not admitted as scratch or eligible for automatic cleanup.'};}
  if(freeBytes===undefined){const fs=statfsSync(root,{bigint:true});freeBytes=Number(fs.bavail*fs.bsize);}
  for(const value of [cacheReserveBytes,scratchReserveBytes])if(!Number.isSafeInteger(value)||value<0)throw new Error('disk.invalid_reservation');
  const report={schema:'lycheedev.disk-admission.v1',observedAt:new Date().toISOString(),cacheBytes,scratchBytes:scratch.bytes,freeBytes,requiredBytes,cacheReserveBytes,scratchReserveBytes,limits,caches,legacy};
  if(cacheBytes+cacheReserveBytes>limits.cacheBytes)throw Object.assign(new Error('disk.cache_quota'),{report});
  if(scratch.bytes+scratchReserveBytes>limits.scratchBytes)throw Object.assign(new Error('disk.scratch_quota'),{report});
  if(requiredBytes<0||!Number.isSafeInteger(requiredBytes)||freeBytes-requiredBytes<limits.freeReserveBytes)throw Object.assign(new Error('disk.insufficient_space'),{report});
  return report;
}
function durable(path,value){const temp=`${path}.${randomUUID()}.tmp`;writeFileSync(temp,JSON.stringify(value,null,2)+'\n',{flag:'wx'});renameSync(temp,path);}
export function acquireChannel(root,kind='ordinary',owner='rust-build'){
  root=realpathSync(root);if(!['ordinary','symbols'].includes(kind))throw new Error('disk.unknown_channel');
  const dir=join(root,'.tmp/rust-disk/channels');mkdirSync(dir,{recursive:true});const path=join(dir,`${kind}.lease`);
  contained(root,path);
  const fd=openSync(path,'wx');const token=randomUUID();writeFileSync(fd,JSON.stringify({schema:'lycheedev.build-lease.v1',owner,token,pid:process.pid,state:'active',channel:kind}));
  let closed=false;return {target:join(root,kind==='ordinary'?'target':'target-symbols'),token,path,release(){if(closed)return;const record=JSON.parse(readFileSync(path));if(record.token!==token||record.pid!==process.pid)throw new Error('disk.foreign_lease');closeSync(fd);rmSync(path);closed=true;}};
}

// Only the formal offline gate requests this fixed subdirectory. Its parent
// channel is held throughout both compilation and receipt-driven retirement.
// An existing directory is never adopted, even if its recorded PID is gone.
export function beginFreshCache(root,channel,job){
  root=realpathSync(root);
  const directory=contained(root,join(channel.target,'.corresponding-source'));
  const receipt=contained(root,join(job.evidence,'fresh-cache.json'));
  function ownedLease(){const observed=JSON.parse(readFileSync(channel.path));if(observed.token!==channel.token||observed.pid!==process.pid||observed.state!=='active')throw new Error('disk.foreign_lease');}
  ownedLease();
  if(existsSync(directory))throw new Error('disk.fresh_cache_not_empty');
  const record={schema:'lycheedev.fresh-cache.v1',ownerId:job.id,category:'build-cache',retention:'rebuildable',directory,channel:channel.path,leaseToken:channel.token,state:'active',initiallyAbsent:true,artifacts:[]};
  durable(receipt,record);mkdirSync(directory,{recursive:true});
  if(readdirSync(directory).length!==0)throw new Error('disk.fresh_cache_not_empty');
  function verifyArtifacts(artifacts){for(const artifact of artifacts){const path=contained(root,artifact.path);if(path===directory||path.startsWith(directory+sep)||path===job.directory||path.startsWith(job.directory+sep))throw new Error('disk.artifact_inside_scratch');if(!lstatSync(path).isFile()||lstatSync(path).size>512<<20||sha256(readFileSync(path))!==artifact.sha256)throw new Error('disk.artifact_integrity');}}
  return {directory,receipt,sealArtifacts(artifacts){if(!Array.isArray(artifacts)||artifacts.length===0)throw new Error('disk.missing_artifact');ownedLease();verifyArtifacts(artifacts);record.artifacts=artifacts;durable(receipt,record);},cleanup(){
    ownedLease();const terminal=JSON.parse(readFileSync(job.receipt)),observed=JSON.parse(readFileSync(receipt));
    if(terminal.id!==job.id||!terminal.handlesReaped||!['succeeded','failed'].includes(terminal.state)||observed.ownerId!==job.id||observed.directory!==directory||observed.leaseToken!==channel.token||observed.state!=='active')throw new Error('disk.cleanup_not_admitted');
    if(!terminal.sealedLogs?.length)throw new Error('disk.missing_terminal_evidence');
    for(const log of terminal.sealedLogs){const path=contained(job.evidence,log.path);if(sha256(readFileSync(path))!==log.sha256)throw new Error('disk.sealed_log_corrupt');}
    if(terminal.state==='succeeded'&&!record.artifacts.length)throw new Error('disk.missing_artifact');
    verifyArtifacts(record.artifacts);contained(root,directory);usage(directory);
    rmSync(directory,{recursive:true});record.state='retired';record.terminalReceipt=job.receipt;durable(receipt,record);
  }};
}
export function beginScratch(root,owner,{command=[],inputs={},retention='rebuildable',limits,admissionOptions={}}={}){
  root=realpathSync(root);const admitted=admission(root,{...admissionOptions,limits});if(!owner||owner.length>128)throw new Error('disk.invalid_owner');
  const id=randomUUID(),base=join(root,'.tmp/rust-scratch'),directory=contained(root,join(base,id));mkdirSync(directory,{recursive:true});
  const evidence=contained(root,join(root,'.tmp/rust-disk/evidence',id));mkdirSync(evidence,{recursive:true});const receipt=join(evidence,'receipt.json');
  const record={schema:'lycheedev.disk-owner.v1',id,owner,category:'scratch',directory,retention,state:'active',activePid:process.pid,command,inputHashes:inputs,commandSHA256:sha256(Buffer.from(JSON.stringify(command))),admission:admitted,createdAt:new Date().toISOString()};durable(receipt,record);
  let sealed=false;
  return {id,directory,evidence,receipt,seal({state,handlesReaped,logs=[]}={}){if(sealed)throw new Error('disk.already_sealed');if(!['succeeded','failed','interrupted'].includes(state)||typeof handlesReaped!=='boolean')throw new Error('disk.invalid_terminal');const logHashes={},sealedLogs=[];for(const [index,path]of logs.entries()){const checked=contained(root,path);if(!lstatSync(checked).isFile()||lstatSync(checked).size>16<<20)throw new Error('disk.log_budget');const digest=sha256(readFileSync(checked));let preserved=checked;if(!checked.startsWith(evidence+sep)){preserved=join(evidence,`log-${index}.bin`);copyFileSync(checked,preserved);}if(sha256(readFileSync(preserved))!==digest||sha256(readFileSync(checked))!==digest)throw new Error('disk.log_changed');logHashes[relative(root,preserved)]=digest;sealedLogs.push({original:checked,path:preserved,sha256:digest});}Object.assign(record,{state,handlesReaped,activePid:null,logHashes,sealedLogs,sealedAt:new Date().toISOString()});durable(receipt,record);sealed=true;return {...record};},cleanup(){if(!sealed||!record.handlesReaped||record.retention!=='rebuildable'||!['succeeded','failed'].includes(record.state))throw new Error('disk.cleanup_not_admitted');const checked=contained(base,directory);const observed=JSON.parse(readFileSync(receipt));if(observed.id!==id||observed.owner!==owner||observed.directory!==directory||observed.state!==record.state||!observed.handlesReaped)throw new Error('disk.foreign_owner');for(const log of observed.sealedLogs||[]){const preserved=contained(evidence,log.path);if(sha256(readFileSync(preserved))!==log.sha256)throw new Error('disk.sealed_log_corrupt');}rmSync(checked,{recursive:true});record.state='retired';durable(receipt,record);return {...record};}};
}

if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){
  const command=process.argv[2]||'inspect',root=resolve(process.argv[3]||'.');if(command!=='inspect')throw new Error('usage: node tools/rust-disk.mjs inspect [workspace] [--legacy]');
  try{console.log(JSON.stringify(admission(root,{legacyInventory:process.argv.includes('--legacy')}),null,2));}catch(error){console.log(JSON.stringify({state:'rejected',code:error.message,...error.report},null,2));process.exitCode=3;}
}
