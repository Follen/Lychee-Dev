import test from 'node:test';
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
test('MSVC policy binds one exact compiler, linker, and existing same-directory helper',()=>{
  const policy=JSON.parse(readFileSync(new URL('./rust-build-auxiliary.json',import.meta.url)));
  assert.equal(policy.msvcCompilers.length,1);
  const cl=policy.msvcCompilers[0],link=cl.linker,helper=policy.helpers.find(row=>row.programFilesPath===cl.helperProgramFilesPath);
  assert(helper);assert.equal(cl.kind,'msvc-cl');assert.equal(link.kind,'msvc-link');assert.equal(helper.kind,'msvc-vctip');
  const directory=path=>path.slice(0,path.lastIndexOf('/'));
  assert.equal(directory(cl.programFilesPath),directory(link.programFilesPath));assert.equal(directory(cl.programFilesPath),directory(helper.programFilesPath));
  assert.equal(cl.sha256,'b0a1818ebae6da7f18230a84e76ce839212584ac4c2ab55b97a0d5268085da57');
  assert.equal(link.sha256,'a8c42e25c91ca2453e1110540ac08f0fc12d7539454a11b9af8baeecb469837d');
  for(const entry of [cl,link,helper]){assert.match(entry.sha256,/^[a-f0-9]{64}$/);assert.ok(entry.bytes>0);assert.match(entry.fileVersion,/^\d+\.\d+\.\d+\.\d+$/);assert.match(entry.signerThumbprint,/^[A-F0-9]{40}$/);assert.equal(entry.signerSubject,'CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US');}
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
