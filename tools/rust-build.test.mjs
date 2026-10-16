import test from 'node:test';import assert from 'node:assert/strict';
import {existsSync,mkdirSync,mkdtempSync,readFileSync,rmSync,writeFileSync} from 'node:fs';import {tmpdir} from 'node:os';import {join} from 'node:path';
import {buildRust,hashFile,runCargo,verifyArtifact} from './rust-build.mjs';
test('sealed artifact reuse verifies exact binary and same-invocation PDB without starting compiler',()=>{const root=mkdtempSync(join(tmpdir(),'lycheedev-artifact-'));try{const sourceBinary=join(root,'lycheedev.exe'),pdb=join(root,'lycheedev.pdb'),manifest=join(root,'artifact.json');writeFileSync(sourceBinary,'frozen bytes');writeFileSync(pdb,'matching symbols');const record={schema:'lycheedev.rust-build-artifact.v1',profile:'symbols',sourceBinary,binarySHA256:hashFile(sourceBinary),pdb,pdbSHA256:hashFile(pdb),sameInvocation:true};writeFileSync(manifest,JSON.stringify(record));assert.deepEqual(verifyArtifact(root,manifest),record);writeFileSync(pdb,'changed symbols');assert.throws(()=>verifyArtifact(root,manifest),/symbol_integrity/);writeFileSync(pdb,'matching symbols');writeFileSync(sourceBinary,'changed bytes');assert.throws(()=>verifyArtifact(root,manifest),/artifact_integrity/);assert.throws(()=>hashFile(sourceBinary,1),/file_budget/);assert.equal(readFileSync(sourceBinary,'utf8'),'changed bytes');}finally{rmSync(root,{recursive:true,force:true});}});

function sourceFixture(){
  const root=mkdtempSync(join(tmpdir(),'lycheedev-runner-'));
  for(const directory of ['crates','release','.cargo'])mkdirSync(join(root,directory));
  for(const path of ['Cargo.toml','Cargo.lock','.cargo/config.toml'])writeFileSync(join(root,path),'fixture');
  return root;
}
test('Cargo runner seals exact output bytes and refuses another owner during execution',()=>{
  const root=sourceFixture();
  try{
    const output=Buffer.from([0,255,128,13,10]);
    const result=runCargo(root,['test','--locked','--offline'],{oneShot:true,execute(command,args,options){
      assert.equal(command,'cargo');assert.equal(options.env.CARGO_TARGET_DIR,join(root,'target'));assert.equal(options.env.CARGO_INCREMENTAL,'0');
      assert.throws(()=>runCargo(root,['check'],{execute(){throw Error('must not execute');}}),/EEXIST/);
      return {status:0,handlesReaped:true,stdout:output,stderr:Buffer.from('diagnostic')};
    }});
    const receipt=JSON.parse(readFileSync(result.buildEvidence));assert.equal(receipt.state,'retired');assert.equal(receipt.handlesReaped,true);
    const stdout=receipt.sealedLogs.find(log=>log.path.endsWith('stdout.bin'));assert.deepEqual(readFileSync(stdout.path),output);
    assert.equal(existsSync(join(root,'.tmp/rust-disk/channels/ordinary.lease')),false);
  }finally{rmSync(root,{recursive:true,force:true});}
});
test('owned fetch records the resolved lockfile without treating source edits as compilation',()=>{
  const root=sourceFixture();
  try{
    const result=runCargo(root,['fetch'],{execute(command,args,options){
      assert.equal(command,'cargo');assert.deepEqual(args,['fetch']);
      writeFileSync(join(root,'Cargo.lock'),'resolved dependency closure');
      writeFileSync(join(root,'crates/active.rs'),'source worker continues');
      return{status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
    }});
    assert.equal(result.inputHashes.resolvedCargoLock,hashFile(join(root,'Cargo.lock')));
    assert.notEqual(result.inputHashes.resolvedCargoLock,result.inputHashes.priorCargoLock);
    const receipt=JSON.parse(readFileSync(result.buildEvidence));
    const command=JSON.parse(readFileSync(receipt.sealedLogs.find(row=>row.path.endsWith('command.json')).path));
    assert.equal(command.compilation,false);assert.equal(command.inputHashes.resolvedCargoLock,result.inputHashes.resolvedCargoLock);
    assert.equal(receipt.handlesReaped,true);
  }finally{rmSync(root,{recursive:true,force:true});}
});
test('interrupted Cargo retains its channel and scratch; unsafe flags cannot start a compiler',()=>{
  const root=sourceFixture();
  try{
    assert.throws(()=>runCargo(root,['build','--target-dir=foreign'],{execute(){throw Error('must not execute');}}),/uncontrolled_cargo_option/);
    let evidence;
    try{runCargo(root,['test'],{execute(){return {status:null,error:new Error('ETIMEDOUT'),stdout:Buffer.from('partial'),stderr:Buffer.alloc(0)};}});}catch(error){evidence=error.buildEvidence;assert.match(error.message,/ETIMEDOUT/);}
    const receipt=JSON.parse(readFileSync(evidence));assert.equal(receipt.state,'interrupted');assert.equal(receipt.handlesReaped,false);
    assert.equal(existsSync(receipt.directory),true);assert.equal(existsSync(join(root,'.tmp/rust-disk/channels/ordinary.lease')),true);
    assert.throws(()=>runCargo(root,['check'],{execute(){throw Error('must not execute');}}),/EEXIST/);
  }finally{rmSync(root,{recursive:true,force:true});}
});
test('locked metadata seals the dependency graph while source edits continue and rejects lock mutation',()=>{
  const root=sourceFixture();
  try{
    const result=runCargo(root,['metadata','--locked','--offline'],{execute(){
      writeFileSync(join(root,'crates/active.rs'),'concurrent implementation');
      return{status:0,handlesReaped:true,stdout:Buffer.from('{"packages":[]}'),stderr:Buffer.alloc(0)};
    }});
    assert.equal(result.inputHashes.priorCargoLock,result.inputHashes.resolvedCargoLock);
    const receipt=JSON.parse(readFileSync(result.buildEvidence));
    const command=JSON.parse(readFileSync(receipt.sealedLogs.find(row=>row.path.endsWith('command.json')).path));
    assert.equal(command.compilation,false);
    assert.throws(()=>runCargo(root,['metadata','--locked','--offline'],{execute(){
      writeFileSync(join(root,'Cargo.lock'),'unexpected changed lock');
      return{status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
    }}),/lock_changed_during_metadata/);
  }finally{rmSync(root,{recursive:true,force:true});}
});
test('fresh offline invocations use distinct Cargo homes and retire only their owned cache after preserving outputs',()=>{
  const root=sourceFixture(),homes=[],targets=[];
  try{
    mkdirSync(join(root,'target'));writeFileSync(join(root,'target/protected-cache'),'other owner');
    for(let index=0;index<2;index++){
      const result=runCargo(root,['build','--locked','--offline'],{freshOffline:true,execute(command,args,options){
        homes.push(options.env.CARGO_HOME);targets.push(options.env.CARGO_TARGET_DIR);
        assert.equal(options.env.CARGO_NET_OFFLINE,'true');assert.equal(options.env.CARGO_INCREMENTAL,'0');
        assert.equal(existsSync(join(options.env.CARGO_TARGET_DIR,'previous-build')),false);
        writeFileSync(join(options.env.CARGO_TARGET_DIR,'previous-build'),'fresh object');
        return {status:0,handlesReaped:true,stdout:Buffer.from('complete'),stderr:Buffer.alloc(0)};
      },onSuccess(invocation){
        const artifact=join(root,`artifact-${index}.exe`);writeFileSync(artifact,'matching release');
        return {...invocation,preservedArtifacts:[{path:artifact,sha256:hashFile(artifact)}]};
      }});
      assert.equal(existsSync(result.target),false);assert.equal(existsSync(result.cargoHome),false);
      const receipt=JSON.parse(readFileSync(result.freshCacheReceipt));assert.equal(receipt.initiallyAbsent,true);assert.equal(receipt.state,'retired');
      assert.equal(readFileSync(join(root,'target/protected-cache'),'utf8'),'other owner');
    }
    assert.notEqual(homes[0],homes[1]);assert.equal(targets[0],targets[1]);
  }finally{rmSync(root,{recursive:true,force:true});}
});
test('fresh offline gate refuses a leftover cache without executing or deleting it',()=>{
  const root=sourceFixture();
  try{
    const leftover=join(root,'target/.corresponding-source');mkdirSync(leftover,{recursive:true});writeFileSync(join(leftover,'keep'),'unresolved');
    assert.throws(()=>runCargo(root,['build','--offline'],{freshOffline:true,execute(){throw Error('must not execute');},onSuccess(){}}),/fresh_cache_not_empty/);
    assert.equal(readFileSync(join(leftover,'keep'),'utf8'),'unresolved');
  }finally{rmSync(root,{recursive:true,force:true});}
});


test('only optimized product builds receive the fixed 30 minute preparation allowance',()=>{
  for(const profile of ['dev','release','symbols']){
    const root=sourceFixture();
    try{
      const expected=profile==='dev'?600000:1800000;
      const record=buildRust(root,join(root,'artifact'),{profile,execute(command,args,options){
        assert.equal(options.timeout,expected);assert.deepEqual(args.slice(0,4),['build','--locked','--offline','-j']);
        const directory=join(options.env.CARGO_TARGET_DIR,profile==='dev'?'debug':profile);mkdirSync(directory,{recursive:true});
        writeFileSync(join(directory,'lycheedev.exe'),'fixture binary');
        if(profile==='symbols')writeFileSync(join(directory,'lycheedev.pdb'),'fixture matching symbols');
        return {status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
      }});
      const receipt=JSON.parse(readFileSync(record.buildEvidence));
      const command=JSON.parse(readFileSync(receipt.sealedLogs.find(row=>row.path.endsWith('command.json')).path));
      assert.equal(command.timeoutMs,expected);assert.equal(record.profile,profile);
    }finally{rmSync(root,{recursive:true,force:true});}
  }
  for(const args of [['test','--release'],['check','--profile','release'],['fetch'],['build','--profile','release']]){
    const root=sourceFixture();
    try{runCargo(root,args,{timeoutMs:1800000,execute(command,received,options){
      assert.equal(options.timeout,600000);return {status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
    }});}finally{rmSync(root,{recursive:true,force:true});}
  }
});
