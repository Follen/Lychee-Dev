// Run ONLY in a Root-approved controlled ordinary compilation window.
import {randomUUID} from 'node:crypto';
import {copyFileSync,existsSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {join,resolve,basename} from 'node:path';
import assert from 'node:assert/strict';
import {runCargo,hashFile} from '../../tools/rust-build.mjs';
import {contained} from '../../tools/rust-disk.mjs';
const root=process.cwd(),evidence=join(root,'.tmp/rust-supervision',`native-exact-commit-build-${randomUUID()}`);
mkdirSync(evidence,{recursive:true});
const paths=['protocol/native-exact-commit-prototype/host.rs','protocol/native-exact-commit-prototype/receiver-auth-expiry.lua',
  'protocol/native-exact-commit-prototype/run-rust.mjs','protocol/native-v1-prototype/gate.rs',
  'protocol/native-v1-prototype/crypto.lua','protocol/native-v1-prototype/observation.lua',
  'crates/lycheedev-live/tests/native_exact_commit_prototype.rs','tools/rust-build.mjs','tools/rust-owned-process.mjs'];
const snapshot=()=>paths.map(path=>({path,sha256:hashFile(join(root,path),1<<20)}));
const before=snapshot();
writeFileSync(join(evidence,'compile-inputs-before.json'),JSON.stringify({schema:'lycheedev.protocol-prototype.compile-inputs.v1',inputs:before},null,2)+'\n',{flag:'wx'});
const sources=join(evidence,'source-snapshot');mkdirSync(sources);
for(const input of before){const path=join(root,input.path),target=join(sources,basename(input.path));assert(!existsSync(target));copyFileSync(path,target);assert.equal(hashFile(target),input.sha256);}
try{
  const result=runCargo(root,['test','-p','lycheedev-live','--test','native_exact_commit_prototype','--locked','--offline','-j','1','--','--nocapture'],{
    owner:'native-exact-commit-sealed',oneShot:false,isolateUser:true,environment:{LYCHEEDEV_EXACT_PROTOTYPE_ROOT:root},onSuccess(invocation){
      const after=snapshot();assert.deepEqual(after,before,'protocol input changed during compile/run');
      const lines=invocation.stdout.split(/\r?\n/).filter(line=>line.startsWith('EXACT_COMMIT_EVIDENCE='));assert.equal(lines.length,1);
      const testEvidence=contained(root,resolve(lines[0].slice('EXACT_COMMIT_EVIDENCE='.length)));
      const input=JSON.parse(readFileSync(join(testEvidence,'inputs.json')));
      const executable=input.inputs.find(item=>/^native_exact_commit_prototype-[0-9a-f]+\.exe$/.test(basename(item.path)));
      assert(executable,'actual executable identity missing');const exe=contained(invocation.target,executable.path);assert.equal(hashFile(exe),executable.sha256);
      const pdb=exe.replace(/\.exe$/,'.pdb');assert(existsSync(pdb));const artifacts=[];
      for(const path of [exe,pdb]){const saved=join(evidence,basename(path)),sha256=hashFile(path,128<<20);copyFileSync(path,saved);assert.equal(hashFile(saved),sha256);artifacts.push({source:path,path:saved,sha256});}
      const record={schema:'lycheedev.protocol-prototype.exact-commit.compiled-run.v1',state:'passed_scoped_prototype_with_expected_clock_counterexample',
        additionalInputsBefore:before,additionalInputsAfter:after,additionalInputsUnchanged:true,sourceSnapshot:sources,
        sourceTree:invocation.inputHashes.sourceTree,buildEvidence:invocation.buildEvidence,testEvidence,artifacts,
        scope:'Rust fixture, real Lua51 privately signed outcomes and exact file publication; once-loader/clock models; fixed binding',finalProtocolPassed:false};
      writeFileSync(join(evidence,'result.json'),JSON.stringify(record,null,2)+'\n',{flag:'wx'});return {record,stdout:invocation.stdout,stderr:invocation.stderr};
    }
  });
  process.stdout.write(result.stdout);process.stderr.write(result.stderr);console.log(JSON.stringify({state:result.record.state,evidence,buildEvidence:result.record.buildEvidence}));
}catch(error){writeFileSync(join(evidence,'failure.json'),JSON.stringify({state:'failed',error:error.message,buildEvidence:error.buildEvidence,
  additionalInputsBefore:before,additionalInputsAfter:snapshot(),sourceSnapshot:sources,finalProtocolPassed:false},null,2)+'\n',{flag:'wx'});
  console.error(JSON.stringify({state:'failed',evidence,error:error.message,buildEvidence:error.buildEvidence}));process.exitCode=1;
}
