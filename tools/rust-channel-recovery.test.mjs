import test from 'node:test';
import {spawnSync} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,renameSync,rmSync,writeFileSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import {bootKey,capture,inspect,sha256,validateCurrent,validateFence,validatePreparation,validateSnapshot,validateStable,validateTransition} from './rust-channel-recovery.mjs';
const ownerId='9f053173-8472-4860-be36-19a455a32e62',runId='00000000-0000-4000-8000-000000000001',token='00000000-0000-4000-8000-000000000002',fenceId='00000000-0000-4000-8000-000000000003';
function boot(uptime100ns='12345678901234567',machineSHA256='a'.repeat(64)){return {schema:'lycheedev.windows-boot-counter.v1',machineSHA256,uptime100ns};}
function row(path,value){const bytes=Buffer.from(JSON.stringify(value));return {path,bytes:bytes.length,base64:bytes.toString('base64'),sha256:sha256(bytes)};}
function fixture(t){const root=mkdtempSync(join(tmpdir(),'rust-channel-fence-model-'));t.after(()=>rmSync(root,{recursive:true,force:true}));const evidence=join(root,'.tmp/rust-disk/evidence',ownerId),base=join(evidence,`process-${runId}`);mkdirSync(evidence,{recursive:true});mkdirSync(join(root,'.tmp/rust-disk/channels'),{recursive:true});const command=['cargo','check','--locked','--offline'];const snapshot={ownerId,channel:'ordinary',files:[row('.tmp/rust-disk/channels/ordinary.lease',{schema:'lycheedev.build-lease.v1',owner:'local-check',token,pid:17,state:'active',channel:'ordinary'}),row(`.tmp/rust-disk/evidence/${ownerId}/receipt.json`,{schema:'lycheedev.disk-owner.v1',id:ownerId,owner:'local-check',directory:join(root,'.tmp/rust-scratch',ownerId),state:'active',activePid:17,command,commandSHA256:sha256(Buffer.from(JSON.stringify(command)))}),row(`.tmp/rust-disk/evidence/${ownerId}/process-${runId}.request.json`,{command:'cargo',args:command.slice(1),cwd:root,stdout:`${base}.stdout.bin`,stderr:`${base}.stderr.bin`,receipt:`${base}.receipt.json`})],absentNativeReceipt:`.tmp/rust-disk/evidence/${ownerId}/process-${runId}.receipt.json`};for(const item of snapshot.files)writeFileSync(join(root,item.path),Buffer.from(item.base64,'base64'));const prepared={schema:'lycheedev.channel-recovery-prepare.v1',fenceId,root,snapshot,boot:boot(),toolHashes:{javascript:'b'.repeat(64),powershell:'c'.repeat(64)},oldOwnerHistoricalState:'unknown',oldOwnerReaped:false};return {root,snapshot,prepared};}
function change(snapshot,index,update){const copy=structuredClone(snapshot),entry=copy.files[index],value=JSON.parse(Buffer.from(entry.base64,'base64'));update(value);copy.files[index]=row(entry.path,value);return copy;}
test('only strict native counter decrease grants a new epoch; wall clock fields are ignored',()=>{assert.equal(validateTransition(boot(),boot('9999999')),true);for(const time of ['12345678901234567','12345678901234568'])assert.throws(()=>validateTransition(boot(),{...boot(time),wallClock:'1900-01-01',lastBootUpTime:'changed'}),/new_full_boot_unproven/);});
test('sleep, hibernate and delayed apply cannot bypass the strict decrease',()=>{for(const time of ['1000','1001','9999999999'])assert.throws(()=>validateTransition(boot('1000'),boot(time)),/new_full_boot_unproven/);});
test('counter strings preserve adjacent values above the JavaScript integer boundary',()=>{assert.equal(validateTransition(boot('9007199254740993'),boot('9007199254740992')),true);assert.throws(()=>validateTransition(boot('9007199254740992'),boot('9007199254740993')),/new_full_boot_unproven/);});
test('negative, floating, overflow, numeric and absent counters are rejected',()=>{for(const value of [-1,7,null,'-1','1.5','1e9','9223372036854775808','18446744073709551616'])assert.throws(()=>bootKey(boot(value)),/boot_identity_invalid/);assert.throws(()=>bootKey({schema:'lycheedev.windows-boot-counter.v1',machineSHA256:'a'.repeat(64)}),/boot_identity_invalid/);});
test('different host and regressing observation counter are rejected',()=>{assert.throws(()=>validateTransition(boot(),boot('1','d'.repeat(64))),/different_machine/);assert.throws(()=>validateStable(boot('1000'),boot('999')),/boot_changed_during_observation/);assert.equal(validateStable(boot('1000'),boot('1000')),true);});
test('snapshot captures exact original files without changing the lease or owner',t=>{const {root,snapshot,prepared}=fixture(t);assert.deepEqual(capture(root,ownerId),snapshot);assert.equal(validatePreparation(prepared),prepared);assert.equal(validateCurrent(prepared,capture(root,ownerId),boot('2')),true);for(const file of snapshot.files)assert.equal(readFileSync(join(root,file.path)).toString('base64'),file.base64);});
test('semantically equal but byte-different original evidence rejects',t=>{const {root,prepared}=fixture(t);const path=join(root,prepared.snapshot.files[0].path);writeFileSync(path,readFileSync(path,'utf8')+'\n');assert.throws(()=>validateCurrent(prepared,capture(root,ownerId),boot('2')),/original_evidence_changed/);});
test('false reap, wrong owner, different channel and changed commands reject',t=>{const {snapshot}=fixture(t);for(const [index,update]of [[0,x=>x.channel='symbols'],[0,x=>x.pid=18],[0,x=>x.token='bad'],[1,x=>x.id=runId],[1,x=>x.owner='foreign'],[1,x=>x.handlesReaped=true],[1,x=>x.state='succeeded'],[2,x=>x.args=['test']]])assert.throws(()=>validateSnapshot(change(snapshot,index,update)));});
test('snapshot hash, encoding and path drift reject',t=>{const {snapshot}=fixture(t);for(const mutate of [x=>x.files[0].sha256='f'.repeat(64),x=>x.files[1].base64+='=',x=>x.files[2].path='../foreign',x=>x.absentNativeReceipt='../foreign']){const value=structuredClone(snapshot);mutate(value);assert.throws(()=>validateSnapshot(value));}});
test('an actual receipt appearing blocks recovery and preserves the lease',t=>{const {root,snapshot}=fixture(t);writeFileSync(join(root,snapshot.absentNativeReceipt),'{}');assert.throws(()=>capture(root,ownerId),/native_receipt_exists/);assert.equal(readFileSync(join(root,snapshot.files[0].path)).toString('base64'),snapshot.files[0].base64);});

test('Windows recovery tool identity works without PowerShell module autoload',{skip:process.platform!=='win32'},t=>{
  const directory=mkdtempSync(join(tmpdir(),'recovery-tool-hash-'));t.after(()=>rmSync(directory,{recursive:true,force:true}));
  const psPath=fileURLToPath(new URL('./rust-channel-recovery.ps1',import.meta.url));
  const jsPath=fileURLToPath(new URL('./rust-channel-recovery.mjs',import.meta.url));
  const source=readFileSync(psPath,'utf8');
  const hashFunction=source.split(/\r?\n/).find(line=>line.startsWith('function Hash-Bytes('));
  const identityCheck=source.split(/\r?\n/).find(line=>line.includes("throw 'recovery.tool_changed_since_prepare'"));
  assert(hashFunction&&identityCheck);
  const literal=value=>"'"+value.replaceAll("'","''")+"'";
  const script=join(directory,'verify.ps1');
  writeFileSync(script,`$ErrorActionPreference='Stop'
$PSModuleAutoLoadingPreference='None'
$PSCommandPath=${literal(psPath)}
$PSScriptRoot=${literal(fileURLToPath(new URL('.',import.meta.url)))}
${hashFunction}
$prepared=@{toolHashes=@{powershell='${sha256(readFileSync(psPath))}';javascript='${sha256(readFileSync(jsPath))}'}}
${identityCheck}
[Console]::Out.Write('verified')
`);
  const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',script],{encoding:'utf8',windowsHide:true,shell:false,timeout:30000,maxBuffer:1<<20});
  assert.equal(run.error,undefined);assert.equal(run.status,0,run.stderr);assert.equal(run.stdout,'verified');
});
test('ambiguous requests reject without choosing a recent one',t=>{const {root,snapshot}=fixture(t);writeFileSync(join(root,'.tmp/rust-disk/evidence',ownerId,`process-${token}.request.json`),'{}');assert.throws(()=>capture(root,ownerId),/exactly_one_request_required/);assert.equal(readFileSync(join(root,snapshot.files[0].path)).toString('base64'),snapshot.files[0].base64);});
test('no broad kill, deletion, reboot, alternate target or event-log dependency in recovery',()=>{const js=readFileSync(new URL('./rust-channel-recovery.mjs',import.meta.url),'utf8'),ps=readFileSync(new URL('./rust-channel-recovery.ps1',import.meta.url),'utf8');assert.doesNotMatch(js,/rmSync|unlinkSync|renameSync|execSync/);assert.doesNotMatch(ps,/Remove-Item|Move-Item|Restart-Computer|Stop-Process|TerminateProcess|TerminateJobObject/);assert.match(ps,/SetFileInformationByHandle\(handle,3,data/);assert.match(ps,/Write-NewFlushed \$fencePath[\s\S]*\$held\[0\]\.RenameNoReplace\(\$archive\)/);assert.match(ps,/oldOwnerReaped=\$false/);assert.doesNotMatch(ps,/EventLog|Get-WinEvent|LastBootUpTime/);});

function preparationFixture(t){const f=fixture(t),directory=join(f.root,'.tmp/rust-disk/recovery',ownerId,fenceId);mkdirSync(directory,{recursive:true});const bytes=Buffer.from(JSON.stringify(f.prepared)),digest=sha256(bytes);writeFileSync(join(directory,'prepared.json'),bytes);const fence={schema:'lycheedev.channel-boot-fence.v1',state:'prepared',ownerId,fenceId,preparationSHA256:digest,originalLeaseSHA256:f.snapshot.files[0].sha256,channel:'ordinary',target:join(f.root,'target'),archive:join(directory,'ordinary.lease.original'),beforeBoot:f.prepared.boot,afterBoot:boot('2'),oldOwnerHistoricalState:'unknown',oldOwnerReaped:false,originalOwnerReceiptUnchanged:true,scratchPreserved:true};return {...f,directory,digest,fence};}
test('read-only inspect distinguishes before-commit evidence and leaves the original lease',t=>{const f=preparationFixture(t);assert.equal(inspect(f.root,ownerId,fenceId,f.digest).state,'waiting_for_verified_restart');writeFileSync(join(f.directory,'fence.json'),JSON.stringify(f.fence));assert.equal(inspect(f.root,ownerId,fenceId,f.digest).state,'prepared_not_committed');assert.equal(readFileSync(join(f.root,f.snapshot.files[0].path)).toString('base64'),f.snapshot.files[0].base64);});
test('inspect recognizes exact archived commit after a lost reply and never touches a new lease',t=>{const f=preparationFixture(t);writeFileSync(join(f.directory,'fence.json'),JSON.stringify(f.fence));const lease=join(f.root,f.snapshot.files[0].path);renameSync(lease,join(f.directory,'ordinary.lease.original'));assert.equal(inspect(f.root,ownerId,fenceId,f.digest).state,'commit_observed');writeFileSync(lease,'new unrelated lease');const result=inspect(f.root,ownerId,fenceId,f.digest);assert.equal(result.currentLease,'present_do_not_modify');assert.equal(readFileSync(lease,'utf8'),'new unrelated lease');});
test('inspection rejects archive corruption, invalid preparation digest and owner evidence drift',t=>{const f=preparationFixture(t);assert.throws(()=>inspect(f.root,ownerId,fenceId,'f'.repeat(64)),/preparation_digest/);writeFileSync(join(f.directory,'fence.json'),JSON.stringify(f.fence));renameSync(join(f.root,f.snapshot.files[0].path),join(f.directory,'ordinary.lease.original'));writeFileSync(join(f.directory,'ordinary.lease.original'),'bad');assert.throws(()=>inspect(f.root,ownerId,fenceId,f.digest),/archive_integrity/);});

test('Windows native fence fixture compiles the production C# and verifies locking and no-replace rename',{skip:process.platform!=='win32'},t=>{
  const directory=join(process.cwd(),'.tmp',`windows-channel-fence-native-${randomUUID()}`);mkdirSync(directory,{recursive:true});t.diagnostic(`Preserved native fence fixture: ${directory}`);
  const sourcePath=new URL('./rust-channel-recovery.ps1',import.meta.url),source=readFileSync(sourcePath,'utf8');const match=source.match(/Add-Type -TypeDefinition @'\n([\s\S]*?)\n'@/);assert(match);
  const script=join(directory,'fixture.ps1');
  writeFileSync(script,`$ErrorActionPreference='Stop'
$tokens=$null;$parseErrors=$null
[void][System.Management.Automation.Language.Parser]::ParseFile('${fileURLToPath(sourcePath).replaceAll("'","''")}',[ref]$tokens,[ref]$parseErrors)
if($parseErrors.Count -gt 0){throw ($parseErrors|Out-String)}
Add-Type -TypeDefinition @'
${match[1]}
'@
$first=[ToolkitChannelFence]::Counter();$second=[ToolkitChannelFence]::Counter()
if($second -lt $first){throw 'counter_regressed'}
$lease=Join-Path $PSScriptRoot 'fixture.lease';$archive=Join-Path $PSScriptRoot 'committed.archive';$existing=$archive;$fence=Join-Path $PSScriptRoot 'fixture.fence.json'
$bytes=[byte[]]@(0,255,128,123,34,120,34,125,10)
[IO.File]::WriteAllBytes($lease,$bytes);[IO.File]::WriteAllText($existing,'do not overwrite');[IO.File]::WriteAllText($fence,'prepared fence stays unchanged')
$owned=New-Object ToolkitChannelFence+LockedFile $lease,$true
try{
 if([Convert]::ToBase64String($owned.Read(1024)) -cne [Convert]::ToBase64String($bytes)){throw 'read_identity'}
 $refused=$false;try{[IO.File]::WriteAllText($lease,'bad')}catch{$refused=$true};if(-not $refused){throw 'write_not_locked'}
 $refused=$false;try{[IO.File]::Delete($lease)}catch{$refused=$true};if(-not $refused){throw 'delete_not_locked'}
 $refused=$false;try{$owned.RenameNoReplace($existing)}catch{$refused=$true};if(-not $refused){throw 'archive_overwritten'}
 if(-not [IO.File]::Exists($lease) -or [IO.File]::ReadAllText($existing) -cne 'do not overwrite'){throw 'failed_commit_changed_files'}
 [IO.File]::Delete($existing) # Remove only this fixture's artificial destination obstruction.
 $owned.RenameNoReplace($archive)
 if([IO.File]::ReadAllText($fence) -cne 'prepared fence stays unchanged'){throw 'retry_rewrote_fence'}
 if([IO.File]::Exists($lease) -or -not [IO.File]::Exists($archive)){throw 'rename_not_committed'}
 if([Convert]::ToBase64String($owned.Read(1024)) -cne [Convert]::ToBase64String($bytes)){throw 'held_bytes_changed'}
}finally{$owned.Dispose()}
if([Convert]::ToBase64String([IO.File]::ReadAllBytes($archive)) -cne [Convert]::ToBase64String($bytes)){throw 'archived_bytes_changed'}
@{passed=$true;cases=8;firstCounter=$first.ToString();secondCounter=$second.ToString()}|ConvertTo-Json -Compress
`);
  const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',script],{encoding:'utf8',windowsHide:true,shell:false,timeout:60000,maxBuffer:1<<20});writeFileSync(join(directory,'stdout.txt'),run.stdout||'');writeFileSync(join(directory,'stderr.txt'),run.stderr||'');assert.equal(run.error,undefined);assert.equal(run.status,0,run.stderr);assert.equal(JSON.parse(run.stdout.replace(/^\uFEFF/,'')).passed,true);
});

test('a prepared fence can be reused only with the same bound originals and prior verified counter reset',t=>{const f=preparationFixture(t);assert.equal(validateFence(f.prepared,f.digest,f.fence),true);for(const update of [x=>x.state='committed',x=>x.channel='symbols',x=>x.oldOwnerReaped=true,x=>x.target=join(f.root,'other-target'),x=>x.beforeBoot=boot('123'),x=>x.afterBoot=f.prepared.boot,x=>x.preparationSHA256='0'.repeat(64),x=>x.originalOwnerReceiptUnchanged=false]){const value=structuredClone(f.fence);update(value);assert.throws(()=>validateFence(f.prepared,f.digest,value));}const ps=readFileSync(new URL('./rust-channel-recovery.ps1',import.meta.url),'utf8');assert.match(ps,/if\(\[IO.File\]::Exists\(\$fencePath\)\)\{/);assert.match(ps,/Assert-NewBoot \$prepared.boot \$oldFence.afterBoot/);assert.match(ps,/else\{Write-NewFlushed \$fencePath/);assert.doesNotMatch(ps,/Exists\(\$archive\).*Exists\(\$fencePath\)/);});
