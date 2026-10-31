import test from 'node:test';import assert from 'node:assert/strict';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';import {tmpdir} from 'node:os';import {join,dirname} from 'node:path';
import {buildRust,hashFile,runCargo,verifyArtifact} from './rust-build.mjs';
import * as build from './rust-build.mjs';
test('sealed artifact reuse verifies exact binary and same-invocation PDB without starting compiler',()=>{const root=mkdtempSync(join(tmpdir(),'lycheedev-artifact-'));try{const sourceBinary=join(root,'lycheedev.exe'),pdb=join(root,'lycheedev.pdb'),manifest=join(root,'artifact.json');writeFileSync(sourceBinary,'frozen bytes');writeFileSync(pdb,'matching symbols');const record={schema:'lycheedev.rust-build-artifact.v1',profile:'symbols',sourceBinary,binarySHA256:hashFile(sourceBinary),pdb,pdbSHA256:hashFile(pdb),sameInvocation:true};writeFileSync(manifest,JSON.stringify(record));assert.deepEqual(verifyArtifact(root,manifest),record);writeFileSync(pdb,'changed symbols');assert.throws(()=>verifyArtifact(root,manifest),/symbol_integrity/);writeFileSync(pdb,'matching symbols');writeFileSync(sourceBinary,'changed bytes');assert.throws(()=>verifyArtifact(root,manifest),/artifact_integrity/);assert.throws(()=>hashFile(sourceBinary,1),/file_budget/);assert.equal(readFileSync(sourceBinary,'utf8'),'changed bytes');}finally{rmSync(root,{recursive:true,force:true});}});

function sourceFixture(){
  const root=mkdtempSync(join(tmpdir(),'lycheedev-runner-'));
  for(const directory of ['crates','release','.cargo'])mkdirSync(join(root,directory));
  for(const path of ['Cargo.toml','Cargo.lock','.cargo/config.toml'])writeFileSync(join(root,path),'fixture');
  return root;
}

test('root compilation fixtures change the source digest while their runtime state stays ignored',()=>{
  const root=sourceFixture();
  try{
    const absent=build.sourceDigest(root),fixtures=join(root,'tests/fixtures');
    mkdirSync(fixtures,{recursive:true});
    writeFileSync(join(fixtures,'compiled-input.lua'),'return 1');
    const original=build.sourceDigest(root);assert.notEqual(original,absent);
    writeFileSync(join(fixtures,'compiled-input.lua'),'return 2');
    const changed=build.sourceDigest(root);assert.notEqual(changed,original);
    for(const name of ['.lycheedev-locks','.lycheedev','target','target-symbols']){
      mkdirSync(join(fixtures,name));writeFileSync(join(fixtures,name,'state'),'runtime state');
    }
    assert.equal(build.sourceDigest(root),changed);
    writeFileSync(join(fixtures,'.lycheedev-locks/state'),'another runtime state');
    assert.equal(build.sourceDigest(root),changed);
  }finally{rmSync(root,{recursive:true,force:true});}
});

test('Cargo jobs accept only integer values from one through four',()=>{
  assert.equal(build.validateCargoJobs(),1);
  for(const value of [1,2,3,4,'1','2','3','4'])assert.equal(build.validateCargoJobs(value),Number(value));
  for(const value of [0,5,-1,1.5,NaN,Infinity,'','2.5','NaN','--one-shot',null,true,[]]){
    assert.throws(()=>build.validateCargoJobs(value),/build.invalid_jobs/);
  }
});

test('buildRust sends default or explicit Cargo jobs and refuses invalid jobs before execution',()=>{
  for(const jobs of [undefined,2]){
    const root=sourceFixture();
    try{
      let calls=0;
      const record=buildRust(root,join(root,'artifact'),{...(jobs===undefined?{}:{jobs}),execute(command,args,options){
        calls++;assert.equal(command,'cargo');
        assert.deepEqual(args,['build','--locked','--offline','-j',String(jobs??1),'--profile','dev','-p','lycheedev-cli']);
        const directory=join(options.env.CARGO_TARGET_DIR,'debug');mkdirSync(directory,{recursive:true});
        writeFileSync(join(directory,'lycheedev.exe'),'fixture binary');
        return {status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
      }});
      assert.equal(calls,1);assert.equal(record.command[record.command.indexOf('-j')+1],String(jobs??1));
    }finally{rmSync(root,{recursive:true,force:true});}
  }
  const root=sourceFixture();
  try{
    let calls=0;
    for(const jobs of [0,5,2.5,'bad',null,true])assert.throws(()=>buildRust(root,join(root,'artifact'),{
      jobs,execute(){calls++;throw new Error('compiler must not start');},
    }),/build.invalid_jobs/);
    assert.equal(calls,0);assert.equal(existsSync(join(root,'.tmp')),false);
  }finally{rmSync(root,{recursive:true,force:true});}
});

test('build CLI parses explicit jobs with one-shot and rejects unknown, repeated or missing options',()=>{
  assert.deepEqual(build.parseBuildOptions(['output']),{directory:'output',profile:'dev',oneShot:false,jobs:1});
  assert.deepEqual(build.parseBuildOptions(['output','release','--jobs','2','--one-shot']),{directory:'output',profile:'release',oneShot:true,jobs:2});
  assert.deepEqual(build.parseBuildOptions(['output','--one-shot','--jobs','4']),{directory:'output',profile:'dev',oneShot:true,jobs:4});
  assert.deepEqual(build.parseBuildOptions(['output','release','--timings','--jobs','2']),{directory:'output',profile:'release',oneShot:false,jobs:2,timings:true});
  for(const args of [[],['output','--unknown'],['output','--jobs'],['output','--jobs','--one-shot'],['output','--jobs','2','--jobs','3'],['output','--one-shot','--one-shot'],['output','release','dev'],['output','--jobs','2.5'],['output','--jobs','5'],['output','--jobs=2'],['output','--profile','release']]){
    assert.throws(()=>build.parseBuildOptions(args),/build\.|usage:/,JSON.stringify(args));
  }
  assert.throws(()=>build.parseBuildOptions(['output','--timings','--timings']),/build.duplicate_option/);
});

test('build artifact preserves the retired receipt, exact logs and this invocation timing HTML',()=>{
  const root=sourceFixture();let originalEvidence;
  try{
    const output=join(root,'artifact'),stdout=Buffer.from([0,255,128,13,10]),stderr=Buffer.from('fingerprint: dependency dirty because input changed\r\n');
    const html=Buffer.from('<!doctype html>\r\n本次 Cargo timings\r\n');
    const record=buildRust(root,output,{timings:true,environment:{CARGO_LOG:'cargo::compiler::fingerprint=info'},execute(command,args,options){
      assert.equal(command,'cargo');assert.equal(args.at(-1),'--timings');assert.equal(options.env.CARGO_LOG,'cargo::compiler::fingerprint=info');
      originalEvidence=options.evidence;
      const target=options.env.CARGO_TARGET_DIR;mkdirSync(join(target,'debug'),{recursive:true});writeFileSync(join(target,'debug/lycheedev.exe'),'fixture executable');
      mkdirSync(join(target,'cargo-timings'));writeFileSync(join(target,'cargo-timings/cargo-timing.html'),html);writeFileSync(join(target,'cargo-timings/unrelated-old.html'),'old report');
      writeFileSync(join(target,'do-not-copy-object'),'unrelated target object');
      return {status:0,handlesReaped:true,stdout,stderr};
    }});
    assert.equal(record.buildEvidence,join(output,'evidence/receipt.json'));
    const originalReceipt=readFileSync(join(originalEvidence,'receipt.json')),copy=readFileSync(record.buildEvidence);
    assert.deepEqual(copy,originalReceipt);assert.equal(record.buildEvidenceSHA256,hashFile(record.buildEvidence));
    const receipt=JSON.parse(copy);assert.equal(receipt.state,'retired');assert.equal(existsSync(receipt.directory),false);
    assert.deepEqual(readFileSync(join(output,'evidence/stdout.bin')),stdout);assert.deepEqual(readFileSync(join(output,'evidence/stderr.bin')),stderr);
    for(const log of receipt.sealedLogs)assert.equal(hashFile(join(output,'evidence',log.path.split(/[\\/]/).at(-1))),log.sha256);
    assert.deepEqual(readFileSync(record.cargoTimings.path),html);assert.equal(record.cargoTimings.sha256,hashFile(record.cargoTimings.path));
    const metadata=JSON.parse(readFileSync(join(output,'evidence/command.json')));
    assert(Number.isInteger(metadata.elapsedMs)&&metadata.elapsedMs>=0);assert(Date.parse(metadata.endedAtUtc)>=Date.parse(metadata.startedAtUtc));
    assert.equal(existsSync(join(output,'evidence/unrelated-old.html')),false);assert.equal(existsSync(join(output,'target')),false);
    assert.deepEqual(verifyArtifact(root,join(output,'artifact.json')),record);
    // The uploaded output remains self-contained after the original evidence
    // has gone; the original receipt's historical paths are never rewritten.
    rmSync(originalEvidence,{recursive:true});assert.deepEqual(readFileSync(record.buildEvidence),copy);
    for(const log of receipt.sealedLogs)assert.equal(hashFile(join(output,'evidence',log.path.split(/[\\/]/).at(-1))),log.sha256);
  }finally{rmSync(root,{recursive:true,force:true});}
});

test('evidence copy rejects a changed sealed log and a filename collision without replacing output',()=>{
  const root=sourceFixture();
  try{
    const invocation=runCargo(root,['build'],{execute(){return {status:0,handlesReaped:true,stdout:Buffer.from('original'),stderr:Buffer.alloc(0)};}});
    const receipt=JSON.parse(readFileSync(invocation.buildEvidence)),log=receipt.sealedLogs.find(row=>row.path.endsWith('stdout.bin'));
    const output=join(root,'artifact');mkdirSync(output);
    writeFileSync(log.path,'changed');assert.throws(()=>build.preserveBuildEvidence(root,invocation.buildEvidence,output),/build.evidence_log_corrupt/);
    assert.equal(existsSync(join(output,'evidence')),false);
    writeFileSync(log.path,'original');
    receipt.sealedLogs.push({...log});writeFileSync(invocation.buildEvidence,JSON.stringify(receipt));
    assert.throws(()=>build.preserveBuildEvidence(root,invocation.buildEvidence,output),/build.evidence_filename_collision/);
    assert.equal(existsSync(join(output,'evidence')),false);
    receipt.sealedLogs.pop();writeFileSync(invocation.buildEvidence,JSON.stringify(receipt));
    build.preserveBuildEvidence(root,invocation.buildEvidence,output);
    const bytes=readFileSync(join(output,'evidence/receipt.json'));
    assert.throws(()=>build.preserveBuildEvidence(root,invocation.buildEvidence,output),/EEXIST/);
    assert.deepEqual(readFileSync(join(output,'evidence/receipt.json')),bytes);
  }finally{rmSync(root,{recursive:true,force:true});}
});

test('explicit timings refuses a missing or unchanged old HTML instead of adopting cached timing evidence',()=>{
  for(const stale of [false,true]){
    const root=sourceFixture();
    try{
      if(stale){mkdirSync(join(root,'target/cargo-timings'),{recursive:true});writeFileSync(join(root,'target/cargo-timings/cargo-timing.html'),'previous invocation');}
      assert.throws(()=>buildRust(root,join(root,'artifact'),{timings:true,execute(){return {status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};}}),/build.cargo_timings_not_updated/);
      assert.equal(existsSync(join(root,'artifact/artifact.json')),false);
      assert.equal(existsSync(join(root,'.tmp/rust-disk/channels/ordinary.lease')),false);
    }finally{rmSync(root,{recursive:true,force:true});}
  }
});
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

test('test-created runtime locks do not invalidate the build, but source edits still do',()=>{
  const root=sourceFixture(), crate=join(root,'crates/fixture');
  try{
    mkdirSync(join(crate,'src'),{recursive:true});
    writeFileSync(join(crate,'src/lib.rs'),'pub fn original() {}');
    const successful=()=>({status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)});
    const result=runCargo(root,['test','--locked','--offline'],{execute(){
      mkdirSync(join(crate,'.lycheedev-locks'));
      writeFileSync(join(crate,'.lycheedev-locks/owner.lock'),'runtime owner');
      return successful();
    }});
    assert.equal(JSON.parse(readFileSync(result.buildEvidence)).state,'retired');
    assert.throws(()=>runCargo(root,['test','--locked','--offline'],{execute(){
      writeFileSync(join(crate,'src/lib.rs'),'pub fn edited() {}');
      return successful();
    }}),/build.source_changed_during_compile/);
    assert.equal(existsSync(join(root,'.tmp/rust-disk/channels/ordinary.lease')),false);
  }finally{rmSync(root,{recursive:true,force:true});}
});

test('isolated native tests keep user and Agent writes in the owned scratch and retain offline compiler caches',()=>{
  const root=sourceFixture();
  try{
    const result=runCargo(root,['test','--locked','--offline'],{isolateUser:true,
      environment:{HOME:'foreign-home',USERPROFILE:'foreign-user',CODEX_HOME:'foreign-agent',LYCHEEDEV_BIN:'foreign-cli',CARGO_HOME:'compiler-cache',RUSTUP_HOME:'compiler-tools'},
      execute(command,args,options){
        const owner=JSON.parse(readFileSync(join(options.evidence,'receipt.json')));
        for(const key of ['HOME','USERPROFILE','CODEX_HOME','APPDATA','LOCALAPPDATA','TEMP','TMP','LYCHEEDEV_HOME'])assert(options.env[key].startsWith(owner.directory));
        assert.equal(options.env.CARGO_HOME,'compiler-cache');assert.equal(options.env.RUSTUP_HOME,'compiler-tools');assert.equal(options.env.LYCHEEDEV_BIN,undefined);
        assert(existsSync(options.env.HOME));assert(existsSync(options.env.TEMP));
        writeFileSync(join(options.env.HOME,'test-user-state'),'owned');
        return{status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
      }});
    const receipt=JSON.parse(readFileSync(result.buildEvidence));
    const command=JSON.parse(readFileSync(receipt.sealedLogs.find(row=>row.path.endsWith('command.json')).path));
    assert.equal(command.isolatedUserRoot,join(receipt.directory,'user'));assert.equal(receipt.state,'retired');assert.equal(existsSync(receipt.directory),false);
  }finally{rmSync(root,{recursive:true,force:true});}
});
test('buildRust forwards requested user isolation while preserving its default environment',()=>{
  for(const isolateUser of [undefined,true]){
    const root=sourceFixture();
    try{
      const artifactDirectory=join(root,'.tmp/ci/build-release/cli');
      assert.equal(existsSync(dirname(artifactDirectory)),false);
      const buildOptions={profile:'release',...(isolateUser===undefined?{}:{isolateUser}),
        environment:{HOME:'foreign-home',USERPROFILE:'foreign-user',CODEX_HOME:'foreign-agent',CARGO_HOME:'compiler-cache',RUSTUP_HOME:'compiler-tools'},
        execute(command,args,options){
          assert.equal(command,'cargo');assert.equal(options.timeout,3600000);
          const owner=JSON.parse(readFileSync(join(options.evidence,'receipt.json')));
          if(isolateUser){
            for(const key of ['HOME','USERPROFILE','CODEX_HOME','APPDATA','LOCALAPPDATA','TEMP','TMP','LYCHEEDEV_HOME'])assert(options.env[key].startsWith(owner.directory));
            assert(existsSync(options.env.HOME));assert(existsSync(options.env.TEMP));
          }else{
            assert.equal(options.env.HOME,'foreign-home');assert.equal(options.env.USERPROFILE,'foreign-user');assert.equal(options.env.CODEX_HOME,'foreign-agent');
          }
          assert.equal(options.env.CARGO_HOME,'compiler-cache');assert.equal(options.env.RUSTUP_HOME,'compiler-tools');
          const directory=join(options.env.CARGO_TARGET_DIR,'release');mkdirSync(directory,{recursive:true});writeFileSync(join(directory,'lycheedev.exe'),'fixture binary');
          return{status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
        }};
      const record=buildRust(root,artifactDirectory,buildOptions);
      const receipt=JSON.parse(readFileSync(record.buildEvidence)),command=JSON.parse(readFileSync(join(dirname(record.buildEvidence),'command.json')));
      assert.equal(command.isolatedUserRoot,isolateUser?join(receipt.directory,'user'):undefined);
      assert.equal(command.oneShot,false);assert.equal(receipt.state,'retired');
      const artifactBytes=readFileSync(join(artifactDirectory,'artifact.json'));
      assert.throws(()=>buildRust(root,artifactDirectory,buildOptions),{code:'EEXIST'});
      assert.deepEqual(readFileSync(join(artifactDirectory,'artifact.json')),artifactBytes);
      assert.deepEqual(verifyArtifact(root,join(artifactDirectory,'artifact.json')),record);
    }finally{rmSync(root,{recursive:true,force:true});}
  }
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


test('all product profiles allow an hour for compilation and only explicit one-shot disables incremental',()=>{
  for(const profile of ['dev','release','symbols']){
    for(const oneShot of [undefined,false,true]){
    const root=sourceFixture();
    try{
      const record=buildRust(root,join(root,'artifact'),{profile,...(oneShot===undefined?{}:{oneShot}),environment:{CARGO_INCREMENTAL:'1'},execute(command,args,options){
        assert.equal(options.timeout,3600000);
        assert.deepEqual(args,['build','--locked','--offline','-j','1','--profile',profile,'-p','lycheedev-cli']);
        assert.equal(options.env.CARGO_INCREMENTAL,oneShot===true?'0':'1');
        assert.equal(options.env.CARGO_TARGET_DIR,join(root,profile==='symbols'?'target-symbols':'target'));
        const directory=join(options.env.CARGO_TARGET_DIR,profile==='dev'?'debug':profile);mkdirSync(directory,{recursive:true});
        writeFileSync(join(directory,'lycheedev.exe'),'fixture binary');
        if(profile==='symbols')writeFileSync(join(directory,'lycheedev.pdb'),'fixture matching symbols');
        return {status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
      }});
      const receipt=JSON.parse(readFileSync(record.buildEvidence));
      const command=JSON.parse(readFileSync(receipt.sealedLogs.find(row=>row.path.endsWith('command.json')).path));
      assert.equal(command.timeoutMs,3600000);assert.equal(command.oneShot,oneShot===true);assert.equal(record.profile,profile);
      assert.equal(record.sameInvocation,true);assert.equal(receipt.state,'retired');
      assert.deepEqual(verifyArtifact(root,join(root,'artifact/artifact.json')),record);
    }finally{rmSync(root,{recursive:true,force:true});}
    }
  }
  for(const args of [['test','--release'],['test','--no-run'],['check','--profile','release'],['clippy'],['fetch'],['metadata','--locked','--offline'],['build','--profile','release']]){
    const root=sourceFixture();
    try{runCargo(root,args,{timeoutMs:1800000,execute(command,received,options){
      assert.equal(options.timeout,['fetch','metadata'].includes(args[0])?900000:3600000);return {status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
    }});}finally{rmSync(root,{recursive:true,force:true});}
  }
});
