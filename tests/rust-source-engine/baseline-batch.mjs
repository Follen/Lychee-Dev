// Current no-host baselines share one owned Cargo build and one frozen input closure.
import assert from 'node:assert/strict';
import {copyFileSync,existsSync,lstatSync,mkdirSync,readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {dirname,join,resolve,sep} from 'node:path';
import {spawnSync} from 'node:child_process';
import {hashFile,runCargo,sourceDigest,verifyArtifact} from '../../tools/rust-build.mjs';
import {admission} from '../../tools/rust-disk.mjs';
import {verifyFrozenContext} from './context-identity.mjs';
import {originalV7Contract} from './wow-v7-contract.mjs';
import {controlContract} from './control-contract.mjs';
import {queryProbeContract} from './query-facade-contract.mjs';

const root=resolve('.'),[mode,output,input]=process.argv.slice(2);
const examples=['emmy_source_control_probe','emmy_source_query_probe','emmy_native_context_probe'];
const buildCommand=()=>['build','--locked','--offline','-j','1','-p','lycheedev-source','-p','lycheedev-cli','--features','lycheedev-source/emmy-prototype','--bin','lycheedev',...examples.flatMap(name=>['--example',name]),'--message-format=json'];
const nativeContract='tests/rust-source-engine/source-control-v6/context-native-contract.json';
const helperFiles=['baseline-batch.mjs','control-owned.mjs','control-contract.mjs','control-independent.mjs','control-object-independent.mjs','query-facade-owned.mjs','query-facade-contract.mjs','query-facade-independent.mjs','context-native-owned.mjs','native-independent-verify.mjs','context-identity.mjs','context-build.mjs','wow-v7-contract.mjs','wow-v7-contract.json'].map(name=>'tests/rust-source-engine/'+name);
const hash=value=>createHash('sha256').update(JSON.stringify(value)).digest('hex');
const write=(path,value)=>writeFileSync(path,JSON.stringify(value,null,2)+'\n',{flag:'wx'});
function requirements(){
 const native=JSON.parse(readFileSync(join(root,nativeContract)));
 const fixtures=[...controlContract(root).inputs.map(row=>row.diskPath),...queryProbeContract(root).inputs.map(row=>row.diskPath),...native.inputs.map(row=>'tests/rust-source-engine/wow/'+row.diskFile)];
 return [...new Set([...helperFiles,nativeContract,...examples.map(name=>'crates/lycheedev-source/examples/'+name+'.rs'),...fixtures])].sort();
}
function frozenInputs(path){
 const frozen=verifyFrozenContext(root,path),names=new Set(frozen.record.inputs.map(row=>row.path));
 for(const name of requirements())assert(names.has(name),'batch freeze missing '+name);
 return frozen;
}
function quotas(){return admission(root,{requiredBytes:2*1024**3,cacheReserveBytes:2*1024**3});}
function newOutput(path){const out=resolve(path);assert(out.startsWith(root+sep)&&!existsSync(out),'new workspace artifact directory required');mkdirSync(out);return out;}
function preserveFile(from,to,expected=hashFile(from)){mkdirSync(dirname(to),{recursive:true});copyFileSync(from,to);assert.equal(hashFile(from),expected);assert.equal(hashFile(to),expected);}
function preserveSourceTree(destination,expected){
 let entries=0,bytes=0;
 function copy(name){const from=join(root,name),to=join(destination,name),info=lstatSync(from);assert(++entries<=100000&&!info.isSymbolicLink());
  if(info.isDirectory()){mkdirSync(to,{recursive:true});for(const child of readdirSync(from).sort())copy(join(name,child));}
  else{assert(info.isFile()&&info.size<=64*2**20);bytes+=info.size;assert(bytes<=512*2**20);preserveFile(from,to,hashFile(from,64*2**20));}}
 for(const name of ['Cargo.toml','Cargo.lock','crates','release','.cargo','vendor','LICENSE','THIRD_PARTY_NOTICES.md','tests/rust-source-data/generate-unicode-simple.mjs'])if(existsSync(join(root,name)))copy(name);
 assert.equal(sourceDigest(destination),expected);return{directory:destination,sourceTree:expected,entries,bytes};
}
function preserveContext(out,frozen){
 preserveFile(frozen.path,join(out,'frozen-context.json'),frozen.sha256);
 for(const row of frozen.record.inputs)preserveFile(join(root,row.path),join(out,'inputs',row.path),row.sha256);
 for(const row of frozen.record.changes)for(const[kind,file,source]of[['before',row.before,'.tmp/rust-source-engine-research-01/upstream'],['after',row.after,'.tmp/rust-source-binding-context-01/upstream']])if(file)preserveFile(join(root,source,row.path),join(out,'patch',kind,row.path),file.sha256);
}
function sealReceipt(path){const receipt=JSON.parse(readFileSync(path));assert.equal(receipt.state,'retired');assert.equal(receipt.handlesReaped,true);for(const log of receipt.sealedLogs)assert.equal(hashFile(log.path),log.sha256);return{path,sha256:hashFile(path),sealedLogs:receipt.sealedLogs};}
function verifyBatch(path){
 const batch=JSON.parse(readFileSync(path));assert.equal(batch.schema,'lycheedev.source-baseline-build-batch.v1');assert.deepEqual(batch.examples,examples);assert.equal(sourceDigest(root),batch.sourceTree);
 const frozen=frozenInputs(batch.contextIdentity.path);assert.equal(frozen.sha256,batch.contextIdentity.sha256);assert.equal(sourceDigest(batch.localInputSnapshot.directory),batch.sourceTree);
 assert.equal(hashFile(batch.custody.path),batch.custody.sha256);assert.equal(hashFile(batch.buildReceipt.path),batch.buildReceipt.sha256);sealReceipt(batch.buildReceipt.path);
 const receipt=JSON.parse(readFileSync(batch.buildReceipt.path)),commandRecord=JSON.parse(readFileSync(join(dirname(batch.buildReceipt.path),'command.json'))),messages=readFileSync(join(dirname(batch.buildReceipt.path),'stdout.bin'),'utf8').trim().split(/\r?\n/).filter(Boolean).map(line=>JSON.parse(line));
 assert.deepEqual(batch.command,['cargo',...buildCommand()]);assert.deepEqual(receipt.command,batch.command);assert.deepEqual([commandRecord.command,...commandRecord.args],batch.command);assert.equal(commandRecord.exit,0);assert.equal(receipt.inputHashes.sourceTree,batch.sourceTree);assert.deepEqual(commandRecord.inputHashes,receipt.inputHashes);
 assert.deepEqual(batch.artifacts.map(row=>row.example),examples);
 for(const artifact of batch.artifacts){assert.equal(hashFile(artifact.path),artifact.sha256);const row=JSON.parse(readFileSync(artifact.path));assert.equal(row.sourceTree,batch.sourceTree);assert.deepEqual(row.command,batch.command);assert.equal(row.buildReceipt,batch.buildReceipt.path);assert.equal(row.sameInvocation,true);assert.equal(row.environmentSHA256,commandRecord.environmentSHA256);assert.equal(hashFile(row.binary),row.binarySHA256);assert.equal(hashFile(row.pdb),row.pdbSHA256);assert.deepEqual(messages.filter(message=>message.reason==='compiler-artifact'&&message.target?.name===artifact.example&&message.target?.kind?.includes('example')),[row.cargoArtifact]);}
 assert.equal(hashFile(batch.cliArtifact.path),batch.cliArtifact.sha256);const cli=verifyArtifact(root,batch.cliArtifact.path);
 assert.equal(cli.schema,'lycheedev.rust-build-artifact.v1');assert.equal(cli.profile,'dev');assert.equal(cli.sameInvocation,true);assert.equal(cli.sourceTree,batch.sourceTree);assert.deepEqual(cli.inputHashes,receipt.inputHashes);assert.deepEqual(cli.command,batch.command);assert.equal(cli.buildEvidence,batch.buildReceipt.path);assert.equal(cli.environmentSHA256,commandRecord.environmentSHA256);assert.equal(cli.sourceRoot,root);assert.equal(cli.channel,commandRecord.target);assert.equal(cli.sourceBinary,join(dirname(batch.cliArtifact.path),'lycheedev.exe'));assert.equal(cli.pdb,join(dirname(batch.cliArtifact.path),'lycheedev.pdb'));assert.equal(lstatSync(cli.sourceBinary).size,cli.bytes);
 const cliMessages=messages.filter(message=>message.reason==='compiler-artifact'&&message.target?.name==='lycheedev'&&message.target?.kind?.includes('bin'));assert.deepEqual(cliMessages,[cli.compilerMessage]);assert.equal(resolve(cli.compilerMessage.manifest_path),join(root,'crates/lycheedev-cli/Cargo.toml'));assert.equal(cli.compilerMessage.profile.test,false);assert.equal(resolve(cli.compilerMessage.executable),join(commandRecord.target,'debug/lycheedev.exe'));assert(cli.compilerMessage.filenames.map(name=>resolve(name)).includes(join(commandRecord.target,'debug/lycheedev.pdb')));assert.deepEqual(cli.requestedFeatures,['lycheedev-source/emmy-prototype']);assert.deepEqual(cli.contextIdentity,batch.contextIdentity);assert.deepEqual(cli.localInputSnapshot,batch.localInputSnapshot);assert.equal(cli.defaultSourceMigration,false);assert.equal(cli.fullSourceAcceptance,false);
 return{batch,frozen};
}
function plan(freezePath){
 const record=JSON.parse(readFileSync(freezePath)),required=requirements(),names=new Set(record.inputs.map(row=>row.path));let freeze,quota;
 try{freeze={valid:true,sha256:frozenInputs(freezePath).sha256};}catch(error){freeze={valid:false,error:error.message};}
 try{quota={admitted:true,...quotas()};}catch(error){quota={admitted:false,error:error.message,...error.report};}
 return{schema:'lycheedev.source-baseline-batch-plan.v1',requiredInputs:required,missingInputs:required.filter(name=>!names.has(name)),freeze,quota,
  cargoInvocations:2,buildCommand:buildCommand(),buildExamples:examples,cli:{package:'lycheedev-cli',binary:'lycheedev',sameBuildInvocation:true,requestedFeatures:['lycheedev-source/emmy-prototype'],defaultSourceMigration:false},strictClippy:['clippy','--locked','--offline','-j','1','-p','lycheedev-source','--features','emmy-prototype','--all-targets','--','-D','warnings'],
  acceptance:{control:'10 scenarios / existing independent contract',query:'existing fixed assertion contract plus independent verifier',native:nativeContract},
  limits:{cargoTimeoutMs:600000,cargoLogBytes:8*2**20,cacheReserveBytes:2*1024**3,requiredDiskBytes:2*1024**3,eachProbeJobMs:30000,eachProbeJobBytes:2**31},sourceExecuted:false};
}
function build(output,freezePath){
 const frozen=frozenInputs(freezePath),quota=quotas(),out=newOutput(output),sourceTree=sourceDigest(root);
 const command=buildCommand();
 const compileDefine={name:'LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256',value:frozen.record.patchSHA256},environment={[compileDefine.name]:compileDefine.value},preparedEnvironment={...process.env,...environment};
 const localInputSnapshot=preserveSourceTree(join(out,'source-snapshot'),sourceTree);preserveContext(out,frozen);
 const custody={path:join(out,'input-custody.json')};write(custody.path,{schema:'lycheedev.emmy-context-input-custody.v1',sourceTree,localInputSnapshot,freezeSHA256:frozen.sha256,patchSHA256:frozen.record.patchSHA256,command:['cargo',...command],state:'preserved_before_build',quota});custody.sha256=hashFile(custody.path);
 try{
  const rows=runCargo(root,command,{owner:'source-baseline-batch',environment,onSuccess(invocation){
   assert.equal(invocation.inputHashes.sourceTree,sourceTree);assert.equal(sourceDigest(localInputSnapshot.directory),sourceTree);assert.deepEqual(frozenInputs(freezePath),frozen);
   const expectedEnvironmentSHA256=hash(Object.entries({...preparedEnvironment,CARGO_TARGET_DIR:invocation.target}).sort(([a],[b])=>a.localeCompare(b)));assert.equal(invocation.environmentSHA256,expectedEnvironmentSHA256);
   const messages=invocation.stdout.trim().split(/\r?\n/).filter(Boolean).map(line=>JSON.parse(line)),artifacts=[];
   assert.equal(messages.filter(row=>row.reason==='build-finished').length,1);assert.equal(messages.find(row=>row.reason==='build-finished').success,true);
   for(const example of examples){
    const emitted=messages.filter(row=>row.reason==='compiler-artifact'&&row.target?.name===example&&row.target?.kind?.includes('example'));assert.equal(emitted.length,1,'one current Cargo artifact per example');
    const sourceBinary=join(invocation.target,'debug/examples',example+'.exe');assert.equal(resolve(emitted[0].executable),resolve(sourceBinary));
    const binary=join(out,example+'.exe'),pdb=join(out,example+'.pdb');preserveFile(sourceBinary,binary);preserveFile(sourceBinary.replace(/\.exe$/,'.pdb'),pdb);
    const binarySHA256=hashFile(binary),pdbSHA256=hashFile(pdb),executionIdentity={legacySnapshotScope:null,freezeSHA256:frozen.sha256,patchSHA256:frozen.record.patchSHA256,binarySHA256,pdbSHA256,observedInvocationEnvironmentSHA256:invocation.environmentSHA256,compileDefine};
    const record={schema:'lycheedev.emmy-context-build.v1',example,binary,binarySHA256,pdb,pdbSHA256,sameInvocation:true,buildReceipt:invocation.buildEvidence,sourceTree,environmentSHA256:invocation.environmentSHA256,command:['cargo',...command],programSHA256:hashFile(join(root,'crates/lycheedev-source/examples/'+example+'.rs')),fixtures:originalV7Contract(root).fixtures,upstreamIdentity:frozen.record.baseIdentity,matrixMode:'regressions',localInputSnapshot,
     programInputFiles:frozen.record.inputs.filter(row=>row.path.startsWith('tests/rust-source-engine/wow/')&&row.path.endsWith('.lua')),contextIdentity:{path:frozen.path,sha256:frozen.sha256,patchSHA256:frozen.record.patchSHA256,rulesVersion:frozen.record.rulesVersion},compilePatchEnvironment:compileDefine.name,compileEnvironmentProof:{compileDefine,expectedEnvironmentSHA256,observedInvocationEnvironmentSHA256:invocation.environmentSHA256,target:invocation.target},executionIdentity,executionIdentitySHA256:hash(executionIdentity),runnerSHA256:hashFile(import.meta.filename),cargoArtifact:emitted[0],scope:'current no-host baseline only; not full Source acceptance'};
    const path=join(out,example+'.artifact.json');write(path,record);artifacts.push({example,path,sha256:hashFile(path)});
   }
   const cliMessages=messages.filter(row=>row.reason==='compiler-artifact'&&row.target?.name==='lycheedev'&&row.target?.kind?.includes('bin'));assert.equal(cliMessages.length,1,'one current Cargo artifact for the CLI');const compilerMessage=cliMessages[0];assert.equal(resolve(compilerMessage.manifest_path),join(root,'crates/lycheedev-cli/Cargo.toml'));assert.equal(compilerMessage.profile.test,false);
   const emittedBinary=join(invocation.target,'debug/lycheedev.exe'),emittedPdb=emittedBinary.replace(/\.exe$/,'.pdb');assert.equal(resolve(compilerMessage.executable),emittedBinary);assert(compilerMessage.filenames.map(name=>resolve(name)).includes(emittedPdb),'matching PDB must be emitted by the same CLI artifact');const cliDirectory=join(out,'cli'),sourceBinary=join(cliDirectory,'lycheedev.exe'),pdb=join(cliDirectory,'lycheedev.pdb');preserveFile(emittedBinary,sourceBinary);preserveFile(emittedPdb,pdb);
   const cliRecord={schema:'lycheedev.rust-build-artifact.v1',profile:'dev',sourceRoot:root,sourceBinary,binarySHA256:hashFile(sourceBinary),bytes:lstatSync(sourceBinary).size,pdb,pdbSHA256:hashFile(pdb),inputHashes:invocation.inputHashes,sourceTree,environmentSHA256:invocation.environmentSHA256,channel:invocation.target,sameInvocation:true,command:['cargo',...command],buildEvidence:invocation.buildEvidence,compilerMessage,requestedFeatures:['lycheedev-source/emmy-prototype'],contextIdentity:{path:frozen.path,sha256:frozen.sha256},localInputSnapshot,compileDefine,scope:'Current CLI built with the Source emmy-prototype feature for public Data query parity; not default Source migration or full Source acceptance',defaultSourceMigration:false,fullSourceAcceptance:false};
   const cliPath=join(cliDirectory,'artifact.json');write(cliPath,cliRecord);const cliArtifact={path:cliPath,sha256:hashFile(cliPath)};return{artifacts,cliArtifact,invocation};
  }});
  const buildReceipt=sealReceipt(rows.invocation.buildEvidence),batch={schema:'lycheedev.source-baseline-build-batch.v1',examples,command:['cargo',...command],sourceTree,contextIdentity:{path:frozen.path,sha256:frozen.sha256},localInputSnapshot,custody,buildReceipt,artifacts:rows.artifacts,cliArtifact:rows.cliArtifact};
  write(join(out,'artifact.json'),batch);verifyBatch(join(out,'artifact.json'));return{artifact:join(out,'artifact.json'),examples,cliArtifact:rows.cliArtifact,buildReceipt};
 }catch(error){write(join(out,'failure.json'),{error:error.message,buildEvidence:error.buildEvidence,admission:error.report,custody,sourceTree,fullSourceAcceptance:false});throw error;}
}
function clippy(output,batchPath){
 const {batch,frozen}=verifyBatch(batchPath),quota=quotas(),out=newOutput(output),command=['clippy','--locked','--offline','-j','1','-p','lycheedev-source','--features','emmy-prototype','--all-targets','--','-D','warnings'];
 write(join(out,'input-custody.json'),{buildBatch:resolve(batchPath),buildBatchSHA256:hashFile(batchPath),sourceTree:batch.sourceTree,localInputSnapshot:batch.localInputSnapshot,contextIdentity:batch.contextIdentity,custody:batch.custody,quota,command:['cargo',...command]});
 try{const result=runCargo(root,command,{owner:'source-baseline-strict-clippy',environment:{LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256:frozen.record.patchSHA256},onSuccess(invocation){assert.equal(invocation.inputHashes.sourceTree,batch.sourceTree);assert.deepEqual(frozenInputs(frozen.path),frozen);return invocation;}});
  const report={schema:'lycheedev.source-baseline-clippy.v1',passed:true,sourceTree:batch.sourceTree,contextIdentity:batch.contextIdentity,buildBatch:resolve(batchPath),command:['cargo',...command],buildReceipt:sealReceipt(result.buildEvidence),scope:'lycheedev-source emmy-prototype all targets; warnings denied'};write(join(out,'report.json'),report);return report;
 }catch(error){write(join(out,'failure.json'),{passed:false,error:error.message,buildEvidence:error.buildEvidence,admission:error.report,sourceTree:batch.sourceTree});throw error;}
}
function run(output,batchPath){
 const {batch}=verifyBatch(batchPath),out=newOutput(output),artifact=name=>batch.artifacts.find(row=>row.example===name).path;
 const jobs=[['control','control-owned.mjs',[join(out,'control'),artifact(examples[0])]],['query','query-facade-owned.mjs',[join(out,'query'),artifact(examples[1])]],['native','context-native-owned.mjs',[join(out,'native'),artifact(examples[2]),nativeContract]],['query-independent','query-facade-independent.mjs',[join(out,'query/report.json'),join(out,'query-independent.json')]],['native-independent','native-independent-verify.mjs',[join(out,'native/report.json'),join(out,'native-independent.json')]]];
 const results=[];for(const[name,script,args]of jobs){const child=spawnSync(process.execPath,[join(root,'tests/rust-source-engine',script),...args],{cwd:root,windowsHide:true,shell:false,timeout:70000,maxBuffer:2**21});writeFileSync(join(out,name+'.stdout.txt'),child.stdout??'');writeFileSync(join(out,name+'.stderr.txt'),child.stderr??'');results.push({name,command:[process.execPath,script,...args],exit:child.status,error:child.error?.message});if(child.status!==0||child.error)break;}
 const reports=['control/report.json','query/report.json','native/report.json','query-independent.json','native-independent.json'].map(name=>{const path=join(out,name);if(!existsSync(path))return{path,passed:false,missing:true};const result=JSON.parse(readFileSync(path));return{path,sha256:hashFile(path),passed:result.passed,checks:result.checks?.length,assertions:result.assertions?.length,handlesReaped:result.handlesReaped};});
 verifyBatch(batchPath);const report={schema:'lycheedev.source-baseline-run-batch.v1',passed:results.length===jobs.length&&results.every(row=>row.exit===0&&!row.error)&&reports.every(row=>row.passed===true),buildBatch:resolve(batchPath),sourceTree:batch.sourceTree,contextIdentity:batch.contextIdentity,results,reports,fullSourceAcceptance:false};write(join(out,'report.json'),report);if(!report.passed)process.exitCode=1;return report;
}
if(mode==='plan')console.log(JSON.stringify(plan(output),null,2));
else if(mode==='build')console.log(JSON.stringify(build(output,input)));
else if(mode==='clippy')console.log(JSON.stringify(clippy(output,input)));
else if(mode==='run')console.log(JSON.stringify(run(output,input)));
else throw new Error('usage: baseline-batch.mjs plan <freeze> | build <new-out> <freeze> | run|clippy <new-out> <build-batch-artifact>');
