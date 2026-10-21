// Extends the normal controlled runner with hashes for out-of-crate prototype inputs.
import {randomUUID} from 'node:crypto';
import {copyFileSync,existsSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {join,resolve,dirname,basename} from 'node:path';
import assert from 'node:assert/strict';
import {runCargo,hashFile} from '../../tools/rust-build.mjs';
import {contained} from '../../tools/rust-disk.mjs';
const root=process.cwd();
const evidence=join(root,'.tmp/rust-supervision',`native-v1-rust-${randomUUID()}`);
mkdirSync(evidence,{recursive:true});
const paths=['protocol/native-v1-prototype/crypto.lua','protocol/native-v1-prototype/observation.lua',
  'protocol/native-v1-prototype/p0_fixture.lua','protocol/native-v1-prototype/apply_counterexample.lua',
  'protocol/native-v1-prototype/gate.rs','protocol/native-v1-prototype/run-rust.mjs',
  'crates/lycheedev-live/tests/native_v1_prototype.rs','tools/rust-build.mjs','tools/rust-owned-process.mjs'];
const snapshot=()=>paths.map(path=>({path,sha256:hashFile(join(root,path),1<<20)}));
const before=snapshot();
writeFileSync(join(evidence,'compile-inputs-before.json'),JSON.stringify({schema:'lycheedev.protocol-prototype.compile-inputs.v1',createdAt:new Date().toISOString(),inputs:before},null,2)+'\n',{flag:'wx'});
try{
  const result=runCargo(root,['test','-p','lycheedev-live','--test','native_v1_prototype','--locked','--offline','-j','1','--','--nocapture'],{
    owner:'native-v1-p0-sealed',oneShot:false,isolateUser:true,onSuccess(invocation){
      const after=snapshot();assert.deepEqual(after,before,'additional source changed during compilation/execution');
      const lines=invocation.stdout.split(/\r?\n/).filter(line=>line.startsWith('P0_EVIDENCE='));assert.equal(lines.length,1);
      const testEvidence=contained(root,resolve(lines[0].slice('P0_EVIDENCE='.length)));
      const input=JSON.parse(readFileSync(join(testEvidence,'inputs.json')));
      const executable=input.inputs.find(item=>/^native_v1_prototype-[0-9a-f]+\.exe$/.test(basename(item.path)));
      assert(executable,'test did not bind its actual executable');
      const exe=contained(invocation.target,executable.path);assert.equal(hashFile(exe),executable.sha256);
      const pdb=exe.replace(/\.exe$/,'.pdb');assert(existsSync(pdb),'matching test PDB absent');
      const preserved=[];
      for(const path of [exe,pdb]){
        const saved=join(evidence,basename(path)),sha256=hashFile(path,128<<20);copyFileSync(path,saved);assert.equal(hashFile(saved),sha256);
        preserved.push({source:path,path:saved,sha256});
      }
      const record={schema:'lycheedev.protocol-prototype.compiled-run.v1',state:'passed_scoped_prototype',createdAt:new Date().toISOString(),
        additionalInputsBefore:before,additionalInputsAfter:after,additionalInputsUnchanged:true,
        sourceTree:invocation.inputHashes.sourceTree,buildEvidence:invocation.buildEvidence,testEvidence,artifacts:preserved,
        scope:'P0 fixed-binding public replay and current nonce gate; not final protocol/WoW/OS input',finalProtocolPassed:false};
      writeFileSync(join(evidence,'result.json'),JSON.stringify(record,null,2)+'\n',{flag:'wx'});
      return {record,stdout:invocation.stdout,stderr:invocation.stderr};
    }
  });
  process.stdout.write(result.stdout);process.stderr.write(result.stderr);
  console.log(JSON.stringify({state:'passed_scoped_prototype',evidence,buildEvidence:result.record.buildEvidence}));
}catch(error){
  const after=snapshot();
  writeFileSync(join(evidence,'failure.json'),JSON.stringify({schema:'lycheedev.protocol-prototype.compiled-run.v1',state:'failed',error:error.message,
    buildEvidence:error.buildEvidence,additionalInputsBefore:before,additionalInputsAfter:after,finalProtocolPassed:false},null,2)+'\n',{flag:'wx'});
  console.error(JSON.stringify({state:'failed',evidence,error:error.message,buildEvidence:error.buildEvidence}));process.exitCode=1;
}
