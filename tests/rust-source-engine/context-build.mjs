// Build both isolated profiles against an independently reviewed frozen fork.
import assert from 'node:assert/strict';
import {copyFileSync,existsSync,lstatSync,mkdirSync,readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {join,resolve,sep} from 'node:path';
import {hashFile,runCargo,sourceDigest} from '../../tools/rust-build.mjs';
import {fixedBusinessContract} from './wow-business-contract.mjs';
import {verifyFrozenContext} from './context-identity.mjs';
import {originalV7Contract} from './wow-v7-contract.mjs';

const root=resolve('.');
// Preserve the complete local Rust input tree, including Core/Windows changes
// outside the optional Source manifest. A digest alone cannot reconstruct it.
function preserveSourceTree(destination,expected){
 let entries=0,bytes=0;
 function copy(name){
  assert(++entries<=100000,'source custody entry budget');
  const from=join(root,name),to=join(destination,name),info=lstatSync(from);
  assert(!info.isSymbolicLink(),'source custody forbids links');
  if(info.isDirectory()){
   mkdirSync(to,{recursive:true});for(const child of readdirSync(from).sort())copy(join(name,child));
  }else{
   assert(info.isFile()&&info.size<=64*2**20,'source custody file budget');
   bytes+=info.size;assert(bytes<=512*2**20,'source custody total budget');
   const sha=hashFile(from,64*2**20);mkdirSync(resolve(to,'..'),{recursive:true});copyFileSync(from,to);
   assert.equal(hashFile(from,64*2**20),sha);assert.equal(hashFile(to,64*2**20),sha);
  }
 }
 for(const name of ['Cargo.toml','Cargo.lock','crates','release','.cargo'])copy(name);
 for(const name of ['vendor','LICENSE','THIRD_PARTY_NOTICES.md','tests/rust-source-data/generate-unicode-simple.mjs'])
  if(existsSync(join(root,name)))copy(name);
 assert.equal(sourceDigest(destination),expected,'preserved actual local compile inputs');
 return{directory:destination,sourceTree:expected,entries,bytes};
}
const [output,freezePath,example]=process.argv.slice(2);
assert(output&&freezePath&&example,'usage: context-build.mjs <new-out> <frozen-context.json> <example>');
assert(['emmy_wow_binding_probe','emmy_native_context_probe','emmy_source_input_probe','emmy_source_query_probe','emmy_source_control_probe','emmy_source_host_probe','emmy_source_host_execution_probe'].includes(example),'explicit isolated example required');
const out=resolve(output);assert(out.startsWith(root+sep)&&!existsSync(out),'new artifact directory required');
const frozen=verifyFrozenContext(root,freezePath);
const programPath=`crates/lycheedev-source/examples/${example}.rs`;
assert(frozen.record.inputs.some(row=>row.path===programPath),'example must be in reviewed input manifest');
const programSHA256=hashFile(join(root,programPath));
const contract=fixedBusinessContract(root,{regressions:true});
assert.deepEqual(contract.inputs,originalV7Contract(root).fixtures,'unchanged original V7 fixture hashes required');
for(const input of contract.inputs){
 const row=frozen.record.inputs.find(row=>row.path===`tests/rust-source-engine/wow/${input.file}`);
 assert(row&&row.sha256===input.sha256&&row.bytes===input.bytes,'all 11 unchanged original fixtures must be frozen');
}
const command=['build','--locked','--offline','-j','1','-p','lycheedev-source','--features','emmy-prototype','--example',example,'--message-format=json'];
const compileDefine={name:'LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256',value:frozen.record.patchSHA256};
const preparedEnvironment={...process.env,[compileDefine.name]:compileDefine.value};
mkdirSync(out);
const admittedSourceTree=sourceDigest(root);
const localInputSnapshot=preserveSourceTree(join(out,'source-snapshot'),admittedSourceTree);
copyFileSync(frozen.path,join(out,'frozen-context.json'));
for(const row of frozen.record.inputs){
 const destination=join(out,'inputs',row.path);mkdirSync(resolve(destination,'..'),{recursive:true});
 copyFileSync(join(root,row.path),destination);assert.equal(hashFile(destination),row.sha256);
}
for(const row of frozen.record.changes){
 for(const[kind,file,source]of[['before',row.before,'.tmp/rust-source-engine-research-01/upstream'],['after',row.after,'.tmp/rust-source-binding-context-01/upstream']]){
  if(!file)continue;const destination=join(out,'patch',kind,row.path);mkdirSync(resolve(destination,'..'),{recursive:true});
  copyFileSync(join(root,source,row.path),destination);assert.equal(hashFile(destination),file.sha256);
 }
}
writeFileSync(join(out,'input-custody.json'),JSON.stringify({schema:'lycheedev.emmy-context-input-custody.v1',
 sourceTree:admittedSourceTree,localInputSnapshot,freezeSHA256:frozen.sha256,patchSHA256:frozen.record.patchSHA256,
 command:['cargo',...command],state:'preserved_before_build',scope:'actual local source, reviewed Source closure and full private patch; dependency base is separately pinned'},null,2)+'\n');
const artifact=runCargo(root,command,{
 owner:`source-emmy-context-${example==='emmy_wow_binding_probe'?'adapter':'native'}`,
 environment:{LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256:frozen.record.patchSHA256},
 onSuccess(invocation){
  assert.equal(invocation.inputHashes.sourceTree,admittedSourceTree);
  assert.equal(sourceDigest(localInputSnapshot.directory),admittedSourceTree);
  const expectedEnvironmentSHA256=createHash('sha256').update(JSON.stringify(Object.entries({...preparedEnvironment,CARGO_TARGET_DIR:invocation.target}).sort(([a],[b])=>a.localeCompare(b)))).digest('hex');
  assert.equal(invocation.environmentSHA256,expectedEnvironmentSHA256,'actual Cargo environment must contain the exact whitelisted patch define');
  assert.deepEqual(verifyFrozenContext(root,freezePath),frozen,'entire frozen context must stay fixed during Cargo');
  assert.deepEqual(fixedBusinessContract(root,{regressions:true}),contract);
  const sourceBinary=join(invocation.target,'debug/examples',`${example}.exe`),sourcePdb=sourceBinary.replace(/\.exe$/,'.pdb');
  const binarySHA256=hashFile(sourceBinary),pdbSHA256=hashFile(sourcePdb);
  const binary=join(out,`${example}.exe`),pdb=binary.replace(/\.exe$/,'.pdb');
  copyFileSync(sourceBinary,binary);copyFileSync(sourcePdb,pdb);
  for(const path of [sourceBinary,binary])assert.equal(hashFile(path),binarySHA256);
  for(const path of [sourcePdb,pdb])assert.equal(hashFile(path),pdbSHA256);
  copyFileSync(frozen.path,join(out,'frozen-context.json'));
  assert.equal(hashFile(join(out,'frozen-context.json')),frozen.sha256);
  for(const row of frozen.record.inputs){
   const destination=join(out,'inputs',row.path);mkdirSync(resolve(destination,'..'),{recursive:true});
   copyFileSync(join(root,row.path),destination);assert.equal(hashFile(destination),row.sha256);
  }
  // Preserve every patch byte and its original counterpart independently of
  // the mutable private dependency directory, including additions/deletions.
  for(const row of frozen.record.changes){
   for(const [kind,file,source] of [['before',row.before,'.tmp/rust-source-engine-research-01/upstream'],['after',row.after,'.tmp/rust-source-binding-context-01/upstream']]){
    if(!file)continue;const destination=join(out,'patch',kind,row.path);
    mkdirSync(resolve(destination,'..'),{recursive:true});copyFileSync(join(root,source,row.path),destination);
    assert.equal(hashFile(destination),file.sha256);
   }
  }
  const record={schema:example==='emmy_wow_binding_probe'?'lycheedev.emmy-wow-build.v1':'lycheedev.emmy-context-build.v1',
   binary,binarySHA256,pdb,pdbSHA256,sameInvocation:true,buildReceipt:invocation.buildEvidence,
   sourceTree:invocation.inputHashes.sourceTree,environmentSHA256:invocation.environmentSHA256,command:['cargo',...command],
   programSHA256,fixtures:contract.inputs,upstreamIdentity:frozen.record.baseIdentity,matrixMode:'regressions',localInputSnapshot,
   programInputFiles:frozen.record.inputs.filter(row=>row.path.startsWith('tests/rust-source-engine/wow/')&&row.path.endsWith('.lua')),
   contextIdentity:{path:frozen.path,sha256:frozen.sha256,patchSHA256:frozen.record.patchSHA256,rulesVersion:frozen.record.rulesVersion},
   compilePatchEnvironment:'LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256',example,
   compileEnvironmentProof:{compileDefine,expectedEnvironmentSHA256,observedInvocationEnvironmentSHA256:invocation.environmentSHA256,target:invocation.target},
   runnerSHA256:hashFile(join(root,'tests/rust-source-engine/context-build.mjs')),
   scope:'isolated native context candidate; no full Source acceptance or final engine choice'};
  record.executionIdentity={legacySnapshotScope:example==='emmy_wow_binding_probe'?'original-v7-primitive-profile':null,
   freezeSHA256:frozen.sha256,patchSHA256:frozen.record.patchSHA256,binarySHA256,pdbSHA256,
   observedInvocationEnvironmentSHA256:invocation.environmentSHA256,compileDefine};
  record.executionIdentitySHA256=createHash('sha256').update(JSON.stringify(record.executionIdentity)).digest('hex');
  writeFileSync(join(out,'contract.json'),JSON.stringify(contract,null,2)+'\n');
  writeFileSync(join(out,'artifact.json'),JSON.stringify(record,null,2)+'\n');return record;
 }
});
console.log(JSON.stringify(artifact));
