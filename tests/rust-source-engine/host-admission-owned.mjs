// Authenticate host material against an owned, frozen, actually executed probe.
import assert from 'node:assert/strict';
import {copyFileSync,existsSync,lstatSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {createHash,randomUUID} from 'node:crypto';
import {dirname,join,resolve,sep} from 'node:path';
import {spawnSync} from 'node:child_process';
import {hashFile,sourceDigest} from '../../tools/rust-build.mjs';
import {verifyFrozenContext} from './context-identity.mjs';
import {hostFactsContract} from './host-facts-contract.mjs';

const root=resolve('.'),[output,buildName]=process.argv.slice(2);
assert(output&&buildName,'usage: host-admission-owned.mjs <new-out> <build-artifact.json>');
const out=resolve(output);assert(out.startsWith(root+sep)&&!existsSync(out));
const build=JSON.parse(readFileSync(buildName));
assert.equal(build.example,'emmy_source_host_probe');assert.equal(build.sameInvocation,true);
assert.equal(sourceDigest(root),build.sourceTree);
const frozen=verifyFrozenContext(root,build.contextIdentity.path);
assert.equal(frozen.sha256,build.contextIdentity.sha256);
assert.equal(frozen.record.patchSHA256,build.contextIdentity.patchSHA256);
const receipt=JSON.parse(readFileSync(build.buildReceipt));
assert.equal(receipt.state,'retired');assert.equal(receipt.handlesReaped,true);
assert.equal(receipt.inputHashes.sourceTree,build.sourceTree);assert.deepEqual(receipt.command,build.command);
for(const log of receipt.sealedLogs)assert.equal(hashFile(log.path),log.sha256);
const fixed=hostFactsContract(root),materialPath='tests/rust-source-engine/host-facts-materials.json';
const materialFile=JSON.parse(readFileSync(join(root,materialPath)));
assert.deepEqual(materialFile.materials,fixed.materials);
assert.deepEqual(materialFile.bindings,fixed.bindings);
for(const name of [materialPath,'tests/rust-source-engine/host-admission-owned.mjs']){
 const row=frozen.record.inputs.find(x=>x.path===name);assert(row&&row.sha256===hashFile(join(root,name)));
}
for(const row of fixed.inputs){const f=frozen.record.inputs.find(x=>x.path===row.diskPath);assert(f&&f.sha256===row.sha256&&f.bytes===row.bytes);}
mkdirSync(out);
const binary=join(out,'emmy_source_host_probe.exe'),pdb=join(out,'emmy_source_host_probe.pdb');
for(const[from,to,sha]of[[build.binary,binary,build.binarySHA256],[build.pdb,pdb,build.pdbSHA256]]){
 assert.equal(hashFile(from),sha);copyFileSync(from,to);assert.equal(hashFile(to),sha);
}
copyFileSync(import.meta.filename,join(out,'runner-snapshot.mjs'));
writeFileSync(join(out,'fixed-host-contract.json'),JSON.stringify(fixed,null,2)+'\n',{flag:'wx'});
const wrapperSource=readFileSync(join(root,'tools/rust-owned-process.ps1'),'utf8');
const anchor='var limits=new LIMITS();limits.basic.flags=0x2000;Set(job,9,limits);';
assert.equal(wrapperSource.split(anchor).length,2);
const wrapper=join(out,'owned-host-admission-2gib.ps1');
writeFileSync(wrapper,wrapperSource.replace(anchor,'var limits=new LIMITS();limits.basic.flags=0x2000|0x200;limits.jobMemory=new UIntPtr(2147483648UL);Set(job,9,limits);'));
const base=join(out,'process-'+randomUUID());
const request={command:binary,args:[],cwd:root,timeoutMs:30000,orphanMs:5000,cargoBuild:false,
 stdout:base+'.stdout.jsonl',stderr:base+'.stderr.txt',receipt:base+'.receipt.json'};
writeFileSync(base+'.request.json',JSON.stringify(request,null,2));
const artifact={schema:'lycheedev.source-host-admission-artifact.v1',sealedBuild:build,binary,pdb,
 binarySHA256:build.binarySHA256,pdbSHA256:build.pdbSHA256,wrapperSHA256:hashFile(wrapper),
 runnerSHA256:hashFile(import.meta.filename),contractSHA256:hashFile(join(out,'fixed-host-contract.json')),
 totalDeadlineMs:30000,jobMemoryLimitBytes:2**31,sourceExecuted:false,scope:'host admission; no abstract execution or flow'};
writeFileSync(join(out,'artifact.json'),JSON.stringify(artifact,null,2));
const run=spawnSync('powershell.exe',['-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',wrapper,'-Request',base+'.request.json'],
 {cwd:root,windowsHide:true,timeout:55000,maxBuffer:1<<20});
writeFileSync(join(out,'wrapper-result.json'),JSON.stringify({status:run.status,error:run.error?.message,stdout:run.stdout?.toString(),stderr:run.stderr?.toString()},null,2));
let proof,frames,error;
try{proof=JSON.parse(readFileSync(request.receipt));const raw=readFileSync(request.stdout);assert(raw.length>0&&raw.length<=2**21&&raw.at(-1)===10);frames=raw.toString('utf8').trimEnd().split('\n').map(line=>JSON.parse(line));}catch(e){error=String(e);}
const checks=[];
function check(name,expected,actual){let passed=true,message;try{assert.deepEqual(actual,expected);}catch(e){passed=false;message=e.message;}checks.push({name,expected,actual,passed,error:message});}
const ownership=run.status===0&&!run.error&&proof?.rootReaped===true&&proof.treeReaped===true&&proof.handlesClosed===true&&proof.toolIdentityLocksClosed===true&&proof.outputReadersComplete===true&&proof.activeProcesses===0;
check('owned process terminal',true,ownership);check('clean execution',0,proof?.exitCode);
check('fixed executable',build.binarySHA256,hashFile(binary));check('matching symbols',build.pdbSHA256,hashFile(pdb));
check('fixed compilation source tree',build.sourceTree,sourceDigest(root));
check('compiled patch',[frozen.record.patchSHA256],frames?.filter(x=>x.event==='build_identity').map(x=>x.compiledPatchSHA256));
check('wire schemas',true,frames?.every(x=>x.schema==='lycheedev.source-host-admission-probe.v1'));
check('material bytes hashes',fixed.materials.map(x=>({name:x.name,sha256:x.sha256,bytes:x.length})),frames?.filter(x=>x.event==='material_identity').map(({name,sha256,bytes})=>({name,sha256,bytes})));
const negative=['hash_mismatch','duplicate_name','duplicate_identity','negative_arity','fractional_arity','oversized_arity','risk_slot_exceeds_arity','unsupported_effect','retained_arguments','source_execution','config_execution','wrong_pointer_kind','missing_record','noncanonical_index','missing_material','wrong_rule_record','duplicate_json_keys','malformed_json','second_binding_admission','foreign_binding_control','foreign_input_control'];
const expected=[{name:'host:exact:seven_bindings',expected:7},{name:'host:open:seven_bindings',expected:7},
 ...negative.map(name=>({name:'host:'+name,expected:4})),{name:'host:cancelled',expected:7},{name:'host:deadline',expected:5},
 {name:'host:source_origin_compatible',expected:{file:'host-direct.lua',start:0,end:5}}];
const assertions=frames?.filter(x=>x.event==='assertion')??[];
check('fixed assertion coverage',expected,assertions.map(({name,expected})=>({name,expected})));
for(const row of assertions){check(row.name,row.expected,row.actual);check(row.name+':reported',true,row.passed);}
const admissions=frames?.filter(x=>x.event==='admitted')??[];
check('exact and open admissions',['exact','open'],admissions.map(x=>x.profile));
for(const a of admissions){
 const declarations=JSON.parse(fixed.materials.find(x=>x.name===(a.profile==='exact'?'fixed-host-v2':'fixed-host-open-arity-v1')).bytes);
 check(a.profile+':original binding names',fixed.bindings.map(x=>x.name),a.bindings.map(x=>x.name));
 check(a.profile+':input SHA',createHash('sha256').update(JSON.stringify(a.inputIdentity)).digest('hex'),a.inputIdentitySHA256);
 check(a.profile+':host bound into input',a.bindings,a.inputIdentity.environment.bindings);
 check(a.profile+':no execution',[false,false,false,false],[a.sourceExecuted,a.nativeRuntimeIntegrated,a.inputIdentity.sourceExecution,a.inputIdentity.configExecution]);
 for(let i=0;i<a.bindings.length;i++){
  const b=a.bindings[i],f=fixed.bindings[i],pointer=f.declaration.pointer;
  const material=fixed.materials.find(x=>x.name===(a.profile==='exact'?'fixed-host-v2':'fixed-host-open-arity-v1'));
  check(a.profile+':binding:'+f.name+':identity',f.identity,b.identity);
  check(a.profile+':binding:'+f.name+':origin',{material:material.name,material_sha256:material.sha256,pointer,identity:f.identity},b.declaration);
  check(a.profile+':binding:'+f.name+':kind',f.kind,b.kind);
  check(a.profile+':binding:'+f.name+':no fake span',false,['file','start','end'].some(key=>key in b.declaration));
  if(f.kind==='callable'){
   const d=declarations.callables.find(x=>x.name===f.name);
   check(a.profile+':binding:'+f.name+':arity',d.returnArity??null,b.return_arity);
   const rules=fixed.materials.find(x=>x.name==='original-flow-rules-v1');
   check(a.profile+':binding:'+f.name+':raw rules',JSON.parse(rules.bytes).calls[f.name],b.rules.record);
   check(a.profile+':binding:'+f.name+':rule origin',{material:rules.name,material_sha256:rules.sha256,pointer:f.rules.pointer,identity:f.identity},b.rules.evidence);
  }else if(f.kind==='predicate')check(a.profile+':predicate slots',[1,1],[b.argument,b.return_arity]);
 }
 check(a.profile+':luac remains unchecked','pending',a.inputIdentity.syntaxCheck.state);
}
check('environment origin',1,frames?.filter(x=>x.event==='environment_origin').length);
const env=frames?.find(x=>x.event==='environment_origin')?.origin;
check('source and environment evidence distinct',true,!!env?.environment&&!('file' in env)&&!('start' in env)&&!('end' in env));
check('single honest terminal',[{complete:true,error:null,checked:expected.length,failed:0,fullSourceAcceptance:false,nativeRuntimeIntegrated:false}],frames?.filter(x=>x.event==='end').map(({complete,error,checked,failed,fullSourceAcceptance,nativeRuntimeIntegrated})=>({complete,error,checked,failed,fullSourceAcceptance,nativeRuntimeIntegrated})));
const report={schema:'lycheedev.source-host-admission-owned.v1',passed:!error&&checks.every(x=>x.passed),artifact,process:proof,handlesReaped:ownership,checks,frames,error,
 scope:'fixed trusted host input admission only; runtime integration, flow, actual APIs, full Retail and total acceptance remain unverified'};
writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({report:join(out,'report.json'),passed:report.passed,checks:checks.length,assertions:assertions.length,handlesReaped:ownership,error}));
if(!report.passed)process.exitCode=1;
