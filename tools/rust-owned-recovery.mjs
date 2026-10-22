// Recover one interrupted owned run. Preserve its failed outcome and all logs.
import {spawnSync} from 'node:child_process';
import {createHash,randomUUID} from 'node:crypto';
import {closeSync,existsSync,fsyncSync,lstatSync,mkdirSync,openSync,readFileSync,readdirSync,realpathSync,renameSync,writeFileSync} from 'node:fs';
import {basename,join,resolve} from 'node:path';
import {contained} from './rust-disk.mjs';
const UUID=/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
function need(ok,code){if(!ok)throw new Error(`owned_recovery.${code}`);}
function read(root,path){path=contained(root,path);const info=lstatSync(path);need(info.isFile()&&info.size<=1<<20,'file_budget');const bytes=readFileSync(path);return {path,sha256:hash(bytes),value:JSON.parse(bytes)};}
function same(a,b){return typeof a==='string'&&typeof b==='string'&&resolve(a).toLowerCase()===resolve(b).toLowerCase();}
function writeNew(path,bytes){const temporary=`${path}.${randomUUID()}.writing`,fd=openSync(temporary,'wx');try{writeFileSync(fd,bytes);fsyncSync(fd);}finally{closeSync(fd);}renameSync(temporary,path);}

// No process decisions occur here: the native boundary locks these exact bytes
// before observing the wrapper and reopening only the recorded named Job.
export function capture(root,ownerId,{terminalReaped=false}={}){
  root=realpathSync(root);need(UUID.test(ownerId),'invalid_owner_id');
  const evidence=contained(root,join(root,'.tmp/rust-disk/evidence',ownerId));
  const names=readdirSync(evidence).filter(name=>/^process-[0-9a-f-]{36}\.request\.json$/.test(name));
  need(names.length===1,'exactly_one_request_required');
  const request=read(root,join(evidence,names[0])),runId=names[0].slice(8,-13);
  need(UUID.test(runId),'invalid_run_id');
  const base=join(evidence,`process-${runId}`),owner=read(root,join(evidence,'receipt.json'));
  const channel=owner.value.channel??'ordinary';
  need(['ordinary','symbols'].includes(channel),'unknown_channel');
  const lease=read(root,join(root,'.tmp/rust-disk/channels',`${channel}.lease`));
  const o=owner.value,l=lease.value,q=request.value;
  need(l.schema==='lycheedev.build-lease.v1'&&l.state==='active'&&l.channel===channel&&UUID.test(l.token)&&Number.isSafeInteger(l.pid)&&l.pid>0,'invalid_lease');
  need(o.schema==='lycheedev.disk-owner.v1'&&o.id===ownerId&&o.owner===l.owner&&['active','interrupted'].includes(o.state)&&o.handlesReaped!==true&&(o.state==='active'?o.activePid===l.pid:o.activePid===null),'owner_mismatch');
  need(same(o.directory,join(root,'.tmp/rust-scratch',ownerId)),'owner_directory');
  need(Array.isArray(o.command)&&o.commandSHA256===hash(Buffer.from(JSON.stringify(o.command)))&&o.command[0]===q.command&&JSON.stringify(o.command.slice(1))===JSON.stringify(q.args),'command_mismatch');
  need(q.schema==='lycheedev.owned-request.v2'&&q.runId===runId&&q.coordinatorPid===l.pid,'request_owner_mismatch');
  for(const [field,suffix]of [['identity','.identity.json'],['rootIdentity','.root.json'],['stdout','.stdout.bin'],['stderr','.stderr.bin'],['receipt','.receipt.json']])need(same(q[field],base+suffix),'request_path');
  const start=read(root,base+'.identity.json'),s=start.value,jobName=`Local\\LycheeDev.Owned.${runId}`;
  need(s.schema==='lycheedev.owned-start.v1'&&s.runId===runId&&s.requestSHA256===request.sha256&&s.coordinatorPid===l.pid&&same(s.requestPath,request.path)&&same(s.rootIdentity,q.rootIdentity)&&same(s.jobIdentity,base+'.job.json')&&s.jobName===jobName&&s.rootAssignment==='PROC_THREAD_ATTRIBUTE_JOB_LIST','startup_identity');
  need(Number.isSafeInteger(s.wrapperPid)&&s.wrapperPid>0&&typeof s.wrapperCreationFileTime==='string'&&/^[1-9]\d*$/.test(s.wrapperCreationFileTime)&&Number.isSafeInteger(s.wrapperSessionId)&&s.wrapperSessionId>=0,'wrapper_identity');
  const files=[lease,owner,request,start],job=existsSync(base+'.job.json')?read(root,base+'.job.json'):null,rootRow=existsSync(base+'.root.json')?read(root,base+'.root.json'):null;
  const bound=value=>value.runId===runId&&value.jobName===jobName&&value.requestSHA256===request.sha256&&(terminalReaped?value.wrapperPid===0&&value.wrapperCreationFileTime==='':value.wrapperPid===s.wrapperPid&&value.wrapperCreationFileTime===s.wrapperCreationFileTime);
  if(job){need(job.value.schema==='lycheedev.owned-job.v1'&&bound(job.value)&&job.value.createdNew===true&&job.value.limitFlags===8192&&job.value.jobHandleInherited===false,'job_identity');files.push(job);}
  if(rootRow){need(job&&rootRow.value.schema==='lycheedev.owned-root.v1'&&bound(rootRow.value)&&rootRow.value.jobAssignedAtCreation===true&&rootRow.value.rootAssignment==='PROC_THREAD_ATTRIBUTE_JOB_LIST'&&Number.isSafeInteger(rootRow.value.pid)&&rootRow.value.pid>0&&/^[1-9]\d*$/.test(rootRow.value.creationFileTime),'root_identity');files.push(rootRow);}
  const receipt=existsSync(q.receipt)?read(root,q.receipt):null;if(receipt)files.push(receipt);
  let terminal;
  if(terminalReaped){
    need(job&&rootRow&&receipt,'terminal_reaped_evidence_required');
    const t=receipt.value,r=rootRow.value;
    need(t.schema==='lycheedev.owned-process.v1'&&t.identitySource==='request-v2'&&t.runId===runId&&same(t.startupIdentity,start.path)&&same(t.rootIdentity,rootRow.path)&&t.pid===r.pid&&t.creationFileTime===r.creationFileTime,'terminal_identity');
    need(['rootReaped','treeReaped','handlesClosed','toolIdentityLocksClosed','outputReadersComplete'].every(key=>t[key]===true)&&t.activeProcesses===0&&t.exitCode===0&&t.error===null&&t.deadlineHit===false&&t.outputLimit===false,'terminal_reap_unverified');
    // This explicit route is solely for the observed Cargo-helper clobber. Its
    // original Job accounting and checked close, not PID absence, prove reap.
    const policy=read(root,join(root,'tools/rust-build-auxiliary.json')),a=t.helperAdmission,programFiles=Object.entries(process.env).find(([key])=>key.toLowerCase()==='programfiles')?.[1];
    need(q.cargoBuild===true&&q.msvcCompile!==true&&['build','check','test','clippy'].includes(q.args[0])&&t.forced===true&&t.knownAuxiliaryCleanup===true&&policy.value.schema==='lycheedev.build-auxiliary-policy.v1'&&a?.state==='enabled'&&a.cargo?.sha256===policy.value.cargoSHA256&&same(t.binary,a.cargo.path)&&basename(t.binary).toLowerCase()==='cargo.exe'&&programFiles,'terminal_helper_admission');
    need(Array.isArray(a.helpers)&&a.helpers.length>=1&&a.helpers.length<=8,'terminal_helper_admission');
    for(const helper of a.helpers){
      const expected=policy.value.helpers.filter(entry=>same(join(programFiles,entry.programFilesPath),helper.path));
      need(expected.length===1&&['bytes','sha256','kind','fileVersion','signerSubject','signerThumbprint'].every(key=>helper[key]===expected[0][key])&&helper.signatureStatus==='WinVerifyTrust:0','terminal_helper_admission');
    }
    const ids=new Set();need(Array.isArray(t.auxiliaryProcesses)&&t.auxiliaryProcesses.length>=1&&t.auxiliaryProcesses.length<=16,'terminal_helper_admission');
    for(const row of t.auxiliaryProcesses){need(row.jobMember===true&&row.reviewedAuxiliary===true&&row.error===null&&Number.isSafeInteger(row.pid)&&row.pid>0&&!ids.has(row.pid)&&/^[1-9]\d*$/.test(row.creationFileTime)&&BigInt(row.creationFileTime)>=BigInt(r.creationFileTime)&&a.helpers.some(helper=>same(helper.path,row.image)),'terminal_helper_admission');ids.add(row.pid);}
    files.push(policy);terminal={recoveryMode:'terminal-reaped',rootPid:r.pid,rootCreationFileTime:r.creationFileTime};
  }
  return {schema:'lycheedev.owned-recovery-plan.v1',...terminal,root,ownerId,runId,channel,jobName,wrapperPid:s.wrapperPid,wrapperCreationFileTime:s.wrapperCreationFileTime,wrapperSessionId:s.wrapperSessionId,jobCreated:!!job,rootRecorded:!!rootRow,originalTerminalReceiptPresent:!!receipt,files:files.map(({path,sha256})=>({path,sha256}))};
}

export function recover(root,ownerId,{terminalReaped=false}={}){
  need(process.platform==='win32','windows_required');root=realpathSync(root);need(UUID.test(ownerId),'invalid_owner_id');
  // Native commit is also idempotent after a lost response. It recognizes its
  // exact archive before examining any new lease, and never modifies that lease.
  const evidence=contained(root,join(root,'.tmp/rust-disk/evidence',ownerId)),names=readdirSync(evidence).filter(name=>/^process-[0-9a-f-]{36}\.request\.json$/.test(name));
  need(names.length===1,'exactly_one_request_required');const runId=names[0].slice(8,-13);need(UUID.test(runId),'invalid_run_id');
  const directory=contained(root,join(root,'.tmp/rust-disk/recovery',ownerId,runId,'same-boot')),archive=join(directory,'ordinary.lease.original');
  let bytes,planPath;
  if(existsSync(archive)){
    const fence=read(root,join(directory,'fence.json')).value;need(fence.ownerId===ownerId&&fence.runId===runId,'fence_owner');planPath=contained(root,fence.plan);need(same(planPath,join(directory,`plan-${fence.planSHA256}.json`)),'fence_plan');bytes=readFileSync(planPath);need(bytes.length<=1<<20&&hash(bytes)===fence.planSHA256,'plan_digest');
  }else{
    const plan=capture(root,ownerId,{terminalReaped});need(plan.channel==='ordinary','ordinary_channel_required');bytes=Buffer.from(JSON.stringify(plan,null,2)+'\n');mkdirSync(directory,{recursive:true});planPath=join(directory,`plan-${hash(bytes)}.json`);if(existsSync(planPath))need(hash(readFileSync(planPath))===hash(bytes),'original_evidence_changed');else writeNew(planPath,bytes);
  }
  const result=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',join(import.meta.dirname,'rust-owned-recovery.ps1'),'-Plan',planPath,'-PlanSHA256',hash(bytes)],{encoding:'utf8',windowsHide:true,shell:false,timeout:45000,maxBuffer:1<<20});
  need(!result.error&&result.status===0,`native_failed:${result.error?.message||result.stderr||result.status}`);
  return JSON.parse(result.stdout.replace(/^\uFEFF/,''));
}
if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){
  try{const [root,ownerId,...extra]=process.argv.slice(2);need(root&&ownerId&&(extra.length===0||extra.length===1&&extra[0]==='--terminal-reaped'),'usage: node tools/rust-owned-recovery.mjs <repo> <owner UUID> [--terminal-reaped]');console.log(JSON.stringify(recover(root,ownerId,{terminalReaped:extra.length===1}),null,2));}
  catch(error){console.error(JSON.stringify({error:error.message,leaseRelease:'not_claimed',next:'Inspect this exact owner and recovery archive; preserve its failed report and retry the recovery after its wrapper exits.'}));process.exitCode=1;}
}
