import test from 'node:test';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync} from 'node:fs';
import {createHash,randomUUID} from 'node:crypto';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';
import {acquireChannel} from './rust-disk.mjs';
import {runCargo} from './rust-build.mjs';
import {capture,recover} from './rust-owned-recovery.mjs';

const windows = {skip: process.platform !== 'win32', timeout: 120000};
const tools = fileURLToPath(new URL('.', import.meta.url));
const json = path => JSON.parse(readFileSync(path, 'utf8').replace(/^\uFEFF/, ''));
const alive = pid => {try {process.kill(pid, 0); return true;} catch (error) {if (error.code === 'ESRCH') return false; throw error;}};
async function until(description, read, timeout = 30000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {const value = read(); if (value) return value; await delay(40);}
  throw new Error(`recovery fixture timed out: ${description}`);
}
function child(command, args, options) {
  const process = spawn(command, args, {...options, windowsHide: true, shell: false, stdio: ['ignore', 'pipe', 'pipe']});
  const out = [], err = [];
  process.stdout.on('data', bytes => out.push(bytes)); process.stderr.on('data', bytes => err.push(bytes));
  const done = new Promise(resolve => {
    process.once('error', error => resolve({error, stdout: Buffer.concat(out).toString(), stderr: Buffer.concat(err).toString()}));
    process.once('close', (code, signal) => resolve({code, signal, stdout: Buffer.concat(out).toString(), stderr: Buffer.concat(err).toString()}));
  });
  return {process, done};
}

function fixture(t, label) {
  const root = resolve('.tmp', `owned-recovery-${label}-${randomUUID()}`);
  mkdirSync(root, {recursive: true});
  t.diagnostic(`Preserved isolated recovery fixture: ${root}`);
  const localTools = join(root, 'runner'); mkdirSync(localTools);
  // This override runs the very same interruption test against a historical
  // wrapper, while importing the current recovery implementation. For a red
  // check, extract the old PS1 blob to an ignored .tmp file and pass its path.
  // Production runner code has no test-only override.
  copyFileSync(join(tools, 'rust-owned-process.mjs'), join(localTools, 'rust-owned-process.mjs'));
  copyFileSync(process.env.LYCHEEDEV_RECOVERY_TEST_WRAPPER || join(tools, 'rust-owned-process.ps1'), join(localTools, 'rust-owned-process.ps1'));
  const ready = join(root, 'owner-ready.json'), marker = join(root, 'root-and-child.json');
  const rootScript = join(root, 'owned-root.cjs'), controller = join(root, 'controller.mjs');
  writeFileSync(rootScript, `const fs=require('node:fs'),{spawn}=require('node:child_process');
if(process.argv[2]==='normal'){process.stdout.write('normal-owned-output');process.exit(0);}
const child=spawn(process.execPath,['-e',"process.stdout.write('child-started'+String.fromCharCode(10)+'c'.repeat(8192));setInterval(()=>{},1000)"],{stdio:['ignore','inherit','inherit'],windowsHide:true});
child.once('spawn',()=>{fs.writeFileSync(${JSON.stringify(marker)},JSON.stringify({root:process.pid,child:child.pid}));process.stdout.write('interrupted-root-output\\n'+'r'.repeat(8192));});
setInterval(()=>{},1000);
`);
  writeFileSync(controller, `import{writeFileSync}from'node:fs';
import{beginScratch,acquireChannel}from ${JSON.stringify(pathToFileURL(join(tools, 'rust-disk.mjs')).href)};
import{ownedUserEnvironment}from ${JSON.stringify(pathToFileURL(join(tools, 'rust-build.mjs')).href)};
import{ownedSpawnSync}from ${JSON.stringify(pathToFileURL(join(localTools, 'rust-owned-process.mjs')).href)};
const root=${JSON.stringify(root)},normal=process.argv[2]==='normal',args=[${JSON.stringify(rootScript)},...(normal?['normal']:[])];
const owner='recovery-regression',job=beginScratch(root,owner,{command:[process.execPath,...args]}),channel=acquireChannel(root,'ordinary',owner);
writeFileSync(${JSON.stringify(ready)},JSON.stringify({id:job.id,directory:job.directory,evidence:job.evidence,receipt:job.receipt,lease:channel.path,target:channel.target}));
const result=ownedSpawnSync(process.execPath,args,{cwd:root,evidence:job.evidence,env:ownedUserEnvironment(job.directory),timeout:90000,orphanMs:2000});
job.seal({state:result.handlesReaped?(result.status===0?'succeeded':'failed'):'interrupted',handlesReaped:result.handlesReaped,logs:result.logs});
if(result.handlesReaped){job.cleanup();channel.release();}
writeFileSync(root+'/controller-result.json',JSON.stringify({status:result.status,handlesReaped:result.handlesReaped,error:result.error?.message}));
process.exitCode=result.error||result.status!==0?1:0;
`);
  return {root, ready, marker, controller};
}

async function start(t, f, normal = false) {
  const execution = child(process.execPath, [f.controller, ...(normal ? ['normal'] : [])], {cwd: f.root});
  t.after(async () => {
    // Each PID came from this fixture. Native Job cleanup remains the wrapper's
    // responsibility; this only bounds a failed test's controller lifetime.
    if (execution.process.exitCode === null) execution.process.kill();
    if (f.start && alive(f.start.wrapperPid)) process.kill(f.start.wrapperPid);
    await execution.done;
  });
  await until('controller owner', () => existsSync(f.ready));
  f.owner = json(f.ready);
  await until('owned request', () => {
    const name = readdirSync(f.owner.evidence).find(name => name.endsWith('.request.json'));
    if (name) {f.requestPath = join(f.owner.evidence, name); return true;}
  });
  f.request = json(f.requestPath);
  if (!normal) {
    await until('root and child started', () => existsSync(f.marker) && existsSync(f.request.rootIdentity));
    f.start = json(f.request.identity); f.members = json(f.marker);
    await until('root and child stdout drained', () => existsSync(f.request.stdout) &&
      /interrupted-root-output/.test(readFileSync(f.request.stdout, 'utf8')) && /child-started/.test(readFileSync(f.request.stdout, 'utf8')));
    assert.equal(alive(f.members.root), true); assert.equal(alive(f.members.child), true);
  }
  return execution;
}

function snapshot(f) {
  return new Map([f.owner.receipt, ...readdirSync(f.owner.evidence).map(name => join(f.owner.evidence, name))]
    .filter(path => !path.endsWith('.writing')).map(path => [path, readFileSync(path)]));
}
function unchanged(before) {for (const [path, bytes] of before) assert.deepEqual(readFileSync(path), bytes, `original evidence preserved: ${path}`);}
async function interrupt(f, execution) {
  // TerminateProcess semantics through Node's Windows kill: no finally block,
  // no synthetic receipt and no graceful signal handler are involved.
  process.kill(f.start.wrapperPid);
  await until('wrapper exited', () => !alive(f.start.wrapperPid));
  if (execution.process.exitCode === null) execution.process.kill();
  await execution.done;
  assert.equal(existsSync(f.owner.lease), true);
  assert.equal(existsSync(f.owner.directory), true);
}

function verifyRecovery(f, result, before, observation) {
  assert.equal(result.state, 'channel_released');
  assert.equal(result.ownerId, f.owner.id);
  assert.equal(result.jobObservation, observation);
  assert.equal(result.activeProcesses, 0);
  assert.equal(result.originalOwnerReceiptUnchanged, true);
  assert.equal(result.originalTerminalReceiptCreated, false);
  unchanged(before);
  assert.equal(existsSync(f.owner.lease), false);
  assert.notEqual(json(f.owner.receipt).state, 'succeeded');
  assert.equal(existsSync(f.request.receipt), before.has(f.request.receipt), 'recovery must not manufacture a successful native receipt');
  const channel = acquireChannel(f.root, 'ordinary', 'after-recovery');
  try {
    assert.equal(channel.target, f.owner.target);
    const newLease = readFileSync(channel.path), observed = recover(f.root, f.owner.id);
    assert.equal(observed.state, 'commit_observed');
    assert.equal(observed.currentLease, 'present_do_not_modify');
    assert.deepEqual(readFileSync(channel.path), newLease);
    unchanged(before);
  } finally {channel.release();}
}

test('Windows normal completion retains the owned runner receipt and channel lifecycle', windows, async t => {
  const f = fixture(t, 'normal'), execution = await start(t, f, true), result = await execution.done;
  assert.equal(result.code, 0, result.stderr);
  assert.deepEqual(json(join(f.root, 'controller-result.json')), {status: 0, handlesReaped: true});
  const proof = json(f.request.receipt);
  for (const key of ['rootReaped', 'treeReaped', 'handlesClosed', 'outputReadersComplete']) assert.equal(proof[key], true);
  assert.equal(readFileSync(f.request.stdout, 'utf8'), 'normal-owned-output');
  assert.equal(json(f.owner.receipt).state, 'retired');
  assert.equal(existsSync(f.owner.directory), false); assert.equal(existsSync(f.owner.lease), false);
});

test('Windows hard termination recovers a destroyed Job on this boot and runs Cargo through the same channel', windows, async t => {
  const f = fixture(t, 'job-gone'), execution = await start(t, f);
  await interrupt(f, execution);
  await until('kill-on-close reaped both owned processes', () => !alive(f.members.root) && !alive(f.members.child));
  const before = snapshot(f);
  const result = recover(f.root, f.owner.id);
  verifyRecovery(f, result, before, 'absent');
  // A dependency-free package exercises the actual runCargo acquisition,
  // Windows runner, stdout receipt, scratch retirement and release. Metadata
  // invokes the installed Cargo without compiling product code or dependencies.
  mkdirSync(join(f.root, 'src')); mkdirSync(join(f.root, 'crates')); mkdirSync(join(f.root, '.cargo'));
  writeFileSync(join(f.root, 'src/lib.rs'), 'pub fn fixture() {}\n');
  writeFileSync(join(f.root, 'Cargo.toml'), '[workspace]\n\n[package]\nname="owned-recovery-fixture"\nversion="0.0.0"\nedition="2021"\n');
  writeFileSync(join(f.root, 'Cargo.lock'), 'version = 4\n\n[[package]]\nname = "owned-recovery-fixture"\nversion = "0.0.0"\n');
  const cargo = runCargo(f.root, ['metadata', '--locked', '--offline', '--format-version=1', '--no-deps'], {owner: 'recovery-cargo-proof', isolateUser: true});
  assert.equal(cargo.target, f.owner.target);
  assert.equal(JSON.parse(cargo.stdout).packages[0].name, 'owned-recovery-fixture');
  assert.equal(json(cargo.buildEvidence).state, 'retired');
  assert.equal(existsSync(f.owner.lease), false);
  unchanged(before);
});

// Keep a second, non-inherited Job handle open so wrapper termination cannot
// itself destroy the Job. Recovery must open the named Job, terminate the live
// root + child, and wait for activeProcesses=0 before releasing the lease.
async function holdJob(t, f) {
  const script = join(f.root, 'hold-job.ps1'), held = join(f.root, 'job-held.json'), stop = join(f.root, 'release-job-handle'), final = join(f.root, 'job-holder-final.json');
  writeFileSync(script, `param([string]$Name,[string]$Held,[string]$Stop,[string]$Final)
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;using System.ComponentModel;using System.Runtime.InteropServices;
public static class RecoveryJobFixture {
 [StructLayout(LayoutKind.Sequential)] public struct Accounting { public long user,kernel,periodUser,periodKernel;public uint faults,total,active,terminated; }
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr OpenJobObjectW(uint access,bool inherit,string name);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool QueryInformationJobObject(IntPtr job,int kind,out Accounting info,uint size,IntPtr returned);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool TerminateJobObject(IntPtr job,uint code);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool CloseHandle(IntPtr handle);
 public static uint Active(IntPtr job){Accounting info;if(!QueryInformationJobObject(job,1,out info,(uint)Marshal.SizeOf(typeof(Accounting)),IntPtr.Zero))throw new Win32Exception();return info.active;}
}
'@
$job=[RecoveryJobFixture]::OpenJobObjectW(12,$false,$Name)
if($job -eq [IntPtr]::Zero){throw 'fixture.open_named_job'}
try{
 [IO.File]::WriteAllText($Held,(@{activeProcesses=[RecoveryJobFixture]::Active($job)}|ConvertTo-Json -Compress))
 $deadline=[DateTime]::UtcNow.AddSeconds(90)
 while(-not [IO.File]::Exists($Stop)){if([DateTime]::UtcNow -gt $deadline){throw 'fixture.holder_timeout'};Start-Sleep -Milliseconds 40}
 [IO.File]::WriteAllText($Final,(@{activeProcesses=[RecoveryJobFixture]::Active($job)}|ConvertTo-Json -Compress))
}finally{[void][RecoveryJobFixture]::TerminateJobObject($job,197);[void][RecoveryJobFixture]::CloseHandle($job)}
`);
  const holder = child('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script,
    '-Name', f.start.jobName, '-Held', held, '-Stop', stop, '-Final', final], {cwd: f.root});
  const release = async () => {writeFileSync(stop, 'release'); return holder.done;};
  t.after(release);
  await until('second named Job handle held', () => existsSync(held));
  assert.ok(json(held).activeProcesses >= 2);
  return {release, final};
}

test('Windows recovery refuses live wrappers and foreign leases, then terminates a held named Job', windows, async t => {
  const f = fixture(t, 'held-job'), execution = await start(t, f);
  const initial = snapshot(f), originalLease = readFileSync(f.owner.lease);
  assert.throws(() => recover(f.root, f.owner.id), /wrapper.*(?:live|running|active)|(?:live|running|active).*wrapper|lock_original/i);
  unchanged(initial); assert.deepEqual(readFileSync(f.owner.lease), originalLease);
  const holder = await holdJob(t, f);
  await interrupt(f, execution);
  assert.equal(alive(f.members.root), true); assert.equal(alive(f.members.child), true);
  const before = snapshot(f), foreign = Buffer.from(JSON.stringify({...JSON.parse(originalLease), owner: 'another-owner', token: randomUUID(), pid: process.pid}));
  writeFileSync(f.owner.lease, foreign);
  assert.throws(() => recover(f.root, f.owner.id), /lease|owner_mismatch/i);
  assert.deepEqual(readFileSync(f.owner.lease), foreign); unchanged(before);
  writeFileSync(f.owner.lease, originalLease);
  const result = recover(f.root, f.owner.id);
  verifyRecovery(f, result, before, 'terminated_active_zero');
  await until('recovery reaped held Job members', () => !alive(f.members.root) && !alive(f.members.child));
  const released = await holder.release();
  assert.equal(released.code, 0, released.stderr);
  assert.equal(json(holder.final).activeProcesses, 0);
});

function terminalCaptureFixture(t){
 const root=mkdtempSync(join(tmpdir(),'owned-terminal-capture-'));t.after(()=>rmSync(root,{recursive:true,force:true}));
 const ownerId=randomUUID(),runId=randomUUID(),evidence=join(root,'.tmp/rust-disk/evidence',ownerId),base=join(evidence,'process-'+runId),lease=join(root,'.tmp/rust-disk/channels/ordinary.lease');
 mkdirSync(evidence,{recursive:true});mkdirSync(join(root,'.tmp/rust-disk/channels'),{recursive:true});mkdirSync(join(root,'tools'));
 const hash=b=>createHash('sha256').update(b).digest('hex'),save=(path,value)=>writeFileSync(path,JSON.stringify(value));
 const programFiles=Object.entries(process.env).find(([key])=>key.toLowerCase()==='programfiles')?.[1];
 const envKey=Object.keys(process.env).find(key=>key.toLowerCase()==='programfiles')??'ProgramFiles';
 if(!programFiles){process.env[envKey]=root;t.after(()=>delete process.env[envKey]);}
 const helper={path:join(programFiles||root,'fixture-vctip.exe'),bytes:64,sha256:'b'.repeat(64),kind:'msvc-vctip',fileVersion:'test',signerSubject:'test signer',signerThumbprint:'test thumbprint',signatureStatus:'WinVerifyTrust:0'},cargo={path:join(root,'cargo.exe'),bytes:128,sha256:'c'.repeat(64)};
 const q={schema:'lycheedev.owned-request.v2',runId,coordinatorPid:123,command:'cargo',args:['test','--help'],cargoBuild:true,msvcCompile:false,identity:base+'.identity.json',rootIdentity:base+'.root.json',stdout:base+'.stdout.bin',stderr:base+'.stderr.bin',receipt:base+'.receipt.json'};
 save(base+'.request.json',q);const binding={runId,jobName:'Local\\LycheeDev.Owned.'+runId,requestSHA256:hash(readFileSync(base+'.request.json'))};
 const start={schema:'lycheedev.owned-start.v1',...binding,coordinatorPid:123,requestPath:base+'.request.json',rootIdentity:q.rootIdentity,jobIdentity:base+'.job.json',rootAssignment:'PROC_THREAD_ATTRIBUTE_JOB_LIST',wrapperPid:456,wrapperCreationFileTime:'100',wrapperSessionId:1};
 const bad={...binding,wrapperPid:0,wrapperCreationFileTime:''},job={schema:'lycheedev.owned-job.v1',...bad,createdNew:true,limitFlags:8192,jobHandleInherited:false},native={schema:'lycheedev.owned-root.v1',...bad,pid:789,creationFileTime:'101',jobAssignedAtCreation:true,rootAssignment:'PROC_THREAD_ATTRIBUTE_JOB_LIST'};
 const receipt={schema:'lycheedev.owned-process.v1',identitySource:'request-v2',runId,startupIdentity:q.identity,rootIdentity:q.rootIdentity,pid:789,creationFileTime:'101',binary:cargo.path,rootReaped:true,treeReaped:true,handlesClosed:true,toolIdentityLocksClosed:true,outputReadersComplete:true,activeProcesses:0,exitCode:0,error:null,deadlineHit:false,outputLimit:false,forced:true,knownAuxiliaryCleanup:true,helperAdmission:{state:'enabled',cargo,helpers:[helper]},auxiliaryProcesses:[{pid:790,creationFileTime:'102',jobMember:true,reviewedAuxiliary:true,error:null,image:helper.path}]};
 const command=[q.command,...q.args],owner={schema:'lycheedev.disk-owner.v1',id:ownerId,owner:'terminal-fixture',directory:join(root,'.tmp/rust-scratch',ownerId),state:'interrupted',activePid:null,handlesReaped:false,command,commandSHA256:hash(Buffer.from(JSON.stringify(command)))};
 const policy={schema:'lycheedev.build-auxiliary-policy.v1',cargoSHA256:cargo.sha256,helpers:[{...helper,programFilesPath:'fixture-vctip.exe'}]};
 const documents=new Map([[lease,{schema:'lycheedev.build-lease.v1',state:'active',channel:'ordinary',token:randomUUID(),pid:123,owner:owner.owner}],[join(evidence,'receipt.json'),owner],[q.identity,start],[base+'.job.json',job],[q.rootIdentity,native],[q.receipt,receipt],[join(root,'tools/rust-build-auxiliary.json'),policy]]);
 for(const [path,value]of documents)save(path,value);writeFileSync(q.stdout,'original stdout');writeFileSync(q.stderr,'original stderr');
 return {root,ownerId,runId,base,lease,q,receipt,start,job,native,policy,documents,save};
}
test('terminal-reaped capture preserves strict default and authenticates the known zero-binding outcome',t=>{
 const f=terminalCaptureFixture(t),before=new Map([...f.documents.keys(),f.base+'.request.json',f.q.stdout,f.q.stderr].map(path=>[path,readFileSync(path)]));
 assert.throws(()=>capture(f.root,f.ownerId),/job_identity/);
 const plan=capture(f.root,f.ownerId,{terminalReaped:true});
 assert.equal(plan.recoveryMode,'terminal-reaped');assert.equal(plan.wrapperPid,456);assert.equal(plan.rootPid,789);assert.equal(plan.rootCreationFileTime,'101');assert.equal(plan.files.length,8);assert.equal(plan.originalTerminalReceiptPresent,true);
 for(const[path,bytes]of before)assert.deepEqual(readFileSync(path),bytes);
 for(const [name,mutate]of Object.entries({root:p=>p.rootReaped=false,tree:p=>p.treeReaped=false,handles:p=>p.handlesClosed=false,tools:p=>p.toolIdentityLocksClosed=false,output:p=>p.outputReadersComplete=false,active:p=>p.activeProcesses=1,pid:p=>p.pid++,creation:p=>p.creationFileTime='102',run:p=>p.runId=randomUUID(),startup:p=>p.startupIdentity+='other',error:p=>p.error='failed',deadline:p=>p.deadlineHit=true,outputLimit:p=>p.outputLimit=true,exit:p=>p.exitCode=1,forced:p=>p.forced=false,cleanup:p=>p.knownAuxiliaryCleanup=false,unreviewed:p=>p.auxiliaryProcesses[0].reviewedAuxiliary=false,foreign:p=>p.auxiliaryProcesses[0].image+='other',policy:p=>p.helperAdmission.helpers[0].sha256='d'.repeat(64)})){
  const value=structuredClone(f.receipt);mutate(value);f.save(f.q.receipt,value);assert.throws(()=>capture(f.root,f.ownerId,{terminalReaped:true}),undefined,name);f.save(f.q.receipt,f.receipt);
 }
 for(const [path,value]of [[f.base+'.job.json',{...f.job,wrapperPid:456}],[f.q.rootIdentity,{...f.native,wrapperCreationFileTime:'100'}],[f.lease,{...f.documents.get(f.lease),owner:'foreign',pid:999}]]){
  f.save(path,value);assert.throws(()=>capture(f.root,f.ownerId,{terminalReaped:true}));f.save(path,f.documents.get(path));
 }
 for(const[path,bytes]of before)assert.deepEqual(readFileSync(path),bytes,'no original evidence changed after rejection');
});
test('terminal-reaped native observation checks exact exited identities without opening a Job',()=>{
 const source=readFileSync(new URL('./rust-owned-recovery.ps1',import.meta.url),'utf8'),terminal=source.slice(source.indexOf(' public static string ObserveTerminal('),source.indexOf(' public static string Observe('));
 assert.match(terminal,/ObserveExited\(wrapperPid,wrapperCreated,"wrapper"\)/);assert.match(terminal,/ObserveExited\(rootPid,rootCreated,"root"\)/);assert.doesNotMatch(terminal,/OpenJobObjectW|TerminateJobObject/);
 assert.match(source,/actual==created&&WaitForSingleObject\(process,0\)!=0/);
});
