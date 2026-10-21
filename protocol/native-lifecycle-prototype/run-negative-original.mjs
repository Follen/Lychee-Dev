// One Root-authorized ordinary Cargo lane. Source/Foundation remain frozen.
// The CLI below uses the existing governed cache, admission and process owner.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {copyFileSync,existsSync,lstatSync,mkdirSync,readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {basename,dirname,join,resolve} from 'node:path';
import {hashFile,ownedUserEnvironment,sourceDigest} from '../../tools/rust-build.mjs';
import {contained} from '../../tools/rust-disk.mjs';
import {ownedSpawnSync} from '../../tools/rust-owned-process.mjs';

const root=resolve(process.cwd()),protocol=contained(root,join(root,'protocol/native-lifecycle-prototype'));
const evidence=contained(root,join(root,'.tmp/rust-supervision',`native-p3-original-negative-build-${randomUUID()}`));
mkdirSync(evidence,{recursive:true});
const entries=readdirSync(protocol);assert(entries.length<=32,'P3 inventory budget');
const paths=entries.sort().map(name=>{
  assert(!name.includes('/')&&!name.includes('\\'));
  const path=contained(root,join(protocol,name)),info=lstatSync(path);assert(info.isFile()&&!info.isSymbolicLink());
  return `protocol/native-lifecycle-prototype/${name}`;
}).concat(['protocol/native-v1-prototype/gate.rs','protocol/native-v1-prototype/crypto.lua','protocol/native-v1-prototype/observation.lua',
  'crates/lycheedev-live/tests/native_p3_original_negative.rs','tools/rust-cargo.mjs','tools/rust-build.mjs','tools/rust-disk.mjs',
  'tools/rust-owned-process.mjs','tools/rust-owned-process.ps1','docs/toolkit/rust-native-shared-lifecycle-matrix-2026-10-08.md']);
const snapshot=()=>paths.map(path=>({path,sha256:hashFile(contained(root,join(root,path)),1<<20)}));
const before=snapshot(),sourceTreeBefore=sourceDigest(root),sources=contained(evidence,join(evidence,'source-snapshot'));
const json=(name,value)=>writeFileSync(contained(evidence,join(evidence,name)),JSON.stringify(value,null,2)+'\n',{flag:'wx'});
json('inputs-before.json',{schema:'lycheedev.protocol-prototype.compile-inputs.v1',sourceTree:sourceTreeBefore,inputs:before});
mkdirSync(sources);
for(const input of before){
  const target=contained(sources,join(sources,input.path));mkdirSync(dirname(target),{recursive:true});
  copyFileSync(contained(root,join(root,input.path)),target);assert.equal(hashFile(target,1<<20),input.sha256);
}
const args=['tools/rust-cargo.mjs','--owner','native-p3-original-negatives','--','test','--locked','--offline','-j1','-p','lycheedev-live',
  '--test','native_p3_original_negative','--','--nocapture'];
json('command.json',{command:process.execPath,args,timeoutMs:300000,owner:'native-p3-original-negatives',sourceTree:sourceTreeBefore});
const run=ownedSpawnSync(process.execPath,args,{cwd:root,evidence,env:{...ownedUserEnvironment(evidence),LYCHEEDEV_LIFECYCLE_NEGATIVE_ROOT:root},timeout:300000,cargoBuild:true});
json('process.json',{status:run.status,error:run.error?.message,handlesReaped:run.handlesReaped,proof:run.proof,logs:run.logs});
const stdout=run.stdout.toString(),stderr=run.stderr.toString();
writeFileSync(contained(evidence,join(evidence,'stdout.txt')),stdout,{flag:'wx'});
writeFileSync(contained(evidence,join(evidence,'stderr.txt')),stderr,{flag:'wx'});
let after,sourceTreeAfter,testEvidence,artifacts=[],buildEvidence;
try{
  after=snapshot();sourceTreeAfter=sourceDigest(root);json('inputs-after.json',{sourceTree:sourceTreeAfter,inputs:after});
  const summary=stdout.trim().split(/\r?\n/).at(-1)||stderr.trim().split(/\r?\n/).at(-1);
  try{const record=JSON.parse(summary);if(record.buildEvidence)buildEvidence=contained(root,resolve(record.buildEvidence));}catch{}
  // rust-cargo intentionally does not forward partial test stdout on failure;
  // recover only its bounded, owner-recorded governed build stdout.
  let testOutput=stdout;
  if(buildEvidence&&run.status!==0){
    const inner=contained(root,join(dirname(buildEvidence),'stdout.bin'));
    if(existsSync(inner)){assert(lstatSync(inner).isFile()&&lstatSync(inner).size<=8<<20);testOutput=readFileSync(inner,'utf8');}
  }
  const lines=testOutput.split(/\r?\n/).filter(line=>line.startsWith('P3_ORIGINAL_NEGATIVE_EVIDENCE='));assert(lines.length<=1,'duplicate test evidence');
  if(lines.length===1){
    testEvidence=contained(root,resolve(lines[0].slice('P3_ORIGINAL_NEGATIVE_EVIDENCE='.length)));
    contained(join(root,'.tmp/rust-supervision'),testEvidence);
    const inputsPath=contained(testEvidence,join(testEvidence,'inputs.json'));assert(lstatSync(inputsPath).size<=65536);
    const inputs=JSON.parse(readFileSync(inputsPath));assert(Array.isArray(inputs)&&inputs.length<=32);
    const executable=inputs.find(item=>/^native_p3_original_negative-[0-9a-f]+\.exe$/.test(basename(item.path)));
    assert(executable,'actual test executable identity missing');
    const exe=contained(join(root,'target'),resolve(executable.path));assert.equal(hashFile(exe,128<<20),executable.sha256);
    const pdb=contained(join(root,'target'),exe.replace(/\.exe$/,'.pdb'));assert(existsSync(pdb),'matching PDB missing');
    for(const path of [exe,pdb]){
      const sha256=hashFile(path,128<<20),saved=contained(evidence,join(evidence,basename(path)));
      copyFileSync(path,saved);assert.equal(hashFile(saved,128<<20),sha256);assert.equal(hashFile(path,128<<20),sha256);
      artifacts.push({source:path,path:saved,sha256});
    }
    // Seal the actual consumed fixture/runtime inputs too, before deciding
    // pass/fail. Executable artifacts are kept separately with matching PDB.
    const actual=contained(evidence,join(evidence,'actual-input-snapshot'));mkdirSync(actual);
    const lua=resolve(process.env.LYCHEEDEV_LUA51||'C:/Program Files (x86)/Lua/5.1/lua.exe');
    const allowed=new Set(before.map(item=>resolve(root,item.path)).concat(lua));
    for(const [index,input]of inputs.entries()){
      const path=resolve(input.path);assert(path===exe||allowed.has(path),'unknown actual fixture input');
      if(path!==lua)contained(root,path);
      assert.equal(hashFile(path,128<<20),input.sha256);
      if(path===exe)continue;
      const saved=contained(actual,join(actual,`${index}-${basename(path)}`));copyFileSync(path,saved);assert.equal(hashFile(saved,128<<20),input.sha256);
    }
  }
  assert.deepEqual(after,before,'P3/additional input changed during compile/test');assert.equal(sourceTreeAfter,sourceTreeBefore,'compiled source input changed');
  assert.equal(run.status,0,stderr);assert(run.handlesReaped,'owned tree terminal missing');assert(testEvidence,'test never reached actual fixture');
  const resultPath=contained(testEvidence,join(testEvidence,'result.json'));assert(lstatSync(resultPath).size<=65536);
  const result=JSON.parse(readFileSync(resultPath));assert.equal(result.state,'original_review_counterexamples_reproduced');assert.equal(result.finalProtocolPassed,false);
  json('result.json',{schema:'lycheedev.protocol-prototype.p3-original-counterexamples.compiled-run.v1',state:'original_review_counterexamples_reproduced',
    before,after,inputsUnchanged:true,sourceTreeBefore,sourceTreeAfter,sourceSnapshot:sources,testEvidence,buildEvidence,artifacts,
    process:{status:run.status,handlesReaped:run.handlesReaped,proof:run.proof},scope:result.scope,excluded:result.excluded,
    restoreUnwitnessedLiveness:'unclosed_conservative_quarantine',finalProtocolPassed:false});
  console.log(JSON.stringify({state:'original_review_counterexamples_reproduced',evidence,testEvidence,buildEvidence}));
}catch(error){
  json('failure.json',{schema:'lycheedev.protocol-prototype.p3-original-counterexamples.compiled-run.v1',state:'failed',error:error.stack,
    before,after,sourceTreeBefore,sourceTreeAfter,sourceSnapshot:sources,testEvidence,buildEvidence,artifacts,
    process:{status:run.status,handlesReaped:run.handlesReaped,proof:run.proof},finalProtocolPassed:false});
  console.error(JSON.stringify({state:'failed',evidence,testEvidence,buildEvidence,error:error.message}));process.exitCode=1;
}
