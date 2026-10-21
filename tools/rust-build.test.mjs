import fs from 'node:fs';import {syncBuiltinESMExports} from 'node:module';
import test from 'node:test';import assert from 'node:assert/strict';
import {existsSync,statSync,mkdirSync,mkdtempSync,readFileSync,rmSync,writeFileSync} from 'node:fs';import {tmpdir} from 'node:os';import {join,dirname} from 'node:path';
import {defaults} from './rust-disk.mjs';
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
      const record=buildRust(root,join(root,'artifact'),{profile:'release',...(isolateUser===undefined?{}:{isolateUser}),
        environment:{HOME:'foreign-home',USERPROFILE:'foreign-user',CODEX_HOME:'foreign-agent',CARGO_HOME:'compiler-cache',RUSTUP_HOME:'compiler-tools'},
        execute(command,args,options){
          assert.equal(command,'cargo');assert.equal(options.timeout,600000);
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
        }});
      const receipt=JSON.parse(readFileSync(record.buildEvidence)),command=JSON.parse(readFileSync(join(dirname(record.buildEvidence),'command.json')));
      assert.equal(command.isolatedUserRoot,isolateUser?join(receipt.directory,'user'):undefined);
      assert.equal(command.oneShot,false);assert.equal(receipt.state,'retired');
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


test('all product profiles keep the fixed ten minute deadline and only explicit one-shot disables incremental',()=>{
  for(const profile of ['dev','release','symbols']){
    for(const oneShot of [undefined,false,true]){
    const root=sourceFixture();
    try{
      const record=buildRust(root,join(root,'artifact'),{profile,...(oneShot===undefined?{}:{oneShot}),environment:{CARGO_INCREMENTAL:'1'},execute(command,args,options){
        assert.equal(options.timeout,600000);
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
      assert.equal(command.timeoutMs,600000);assert.equal(command.oneShot,oneShot===true);assert.equal(record.profile,profile);
      assert.equal(record.sameInvocation,true);assert.equal(receipt.state,'retired');
      assert.deepEqual(verifyArtifact(root,join(root,'artifact/artifact.json')),record);
    }finally{rmSync(root,{recursive:true,force:true});}
    }
  }
  for(const args of [['test','--release'],['check','--profile','release'],['fetch'],['build','--profile','release']]){
    const root=sourceFixture();
    try{runCargo(root,args,{timeoutMs:1800000,execute(command,received,options){
      assert.equal(options.timeout,600000);return {status:0,handlesReaped:true,stdout:Buffer.alloc(0),stderr:Buffer.alloc(0)};
    }});}finally{rmSync(root,{recursive:true,force:true});}
  }
});


test('scoped Source exporter Cargo admission keeps 5 GiB default and accepts exact explicit 6 GiB without environment overrides', t => {
  const root = sourceFixture(), protectedPath = join(root, '.tmp/rust-scratch/protected/logical-size');
  // Only the measured logical size is synthetic. Do not create a multi-GiB
  // file: truncate does not guarantee sparse allocation on Windows.
  let logicalBytes = defaults.scratchBytes + 1; const originalStat = fs.lstatSync;
  const statMock = t.mock.method(fs, 'lstatSync', (...args) => { const stat = originalStat(...args); if (args[0] === protectedPath) stat.size = logicalBytes; return stat; }); syncBuiltinESMExports();
  try {
    mkdirSync(join(root, '.tmp/rust-scratch/protected'), {recursive: true}); writeFileSync(protectedPath, 'x');
    let calls = 0; const execute = (_command, _args, options) => { calls++; assert(!Object.keys(options.env).some(key => /^LYCHEEDEV_DISK_/i.test(key))); return {status: 0, handlesReaped: true, stdout: Buffer.alloc(0), stderr: Buffer.alloc(0)}; };
    const args = ['test', '--locked', '--offline', '-j', '1', '-p', 'lycheedev-source', '--lib', '--no-run', '--message-format=json-render-diagnostics'];
    const options = {owner: 'source-postcard-exporter-build', oneShot: false, execute};
    assert.throws(() => runCargo(root, args, options), /disk.scratch_quota/); assert.equal(calls, 0);
    const limits = {...defaults, scratchBytes: 6 * 1024 ** 3}, result = runCargo(root, args, {...options, limits});
    const receipt = JSON.parse(readFileSync(result.buildEvidence)); assert.equal(receipt.state, 'retired'); assert.deepEqual(receipt.admission.limits, limits); assert.equal(receipt.admission.scratchBytes, defaults.scratchBytes + 1);
    const command = JSON.parse(readFileSync(join(dirname(result.buildEvidence), 'command.json'))); assert.equal(command.oneShot, false); assert.equal(command.timeoutMs, 600000);
    assert.equal(receipt.admission.cacheReserveBytes, 2 * 1024 ** 3); assert.equal(receipt.admission.requiredBytes, 2 * 1024 ** 3); assert.equal(statSync(protectedPath).size, 1); assert.equal(calls, 1);
    logicalBytes = 6 * 1024 ** 3 + 1; assert.throws(() => runCargo(root, args, {...options, limits}), /disk.scratch_quota/); assert.equal(calls, 1);
    for (const change of [{limits: {...limits, scratchBytes: 7 * 1024 ** 3}}, {limits: {...limits, freeReserveBytes: 0}}, {limits: {...limits, cacheBytes: 30 * 1024 ** 3}}, {limits: {...limits, maxEntries: 1e6}}, {limits: {...limits, unknown: 1}}, {owner: 'ordinary-build'}, {environment: {LYCHEEDEV_DISK_SCRATCH_GIB: '6'}}, {environment: {lycheedev_disk_unknown: '6'}}]) assert.throws(() => runCargo(root, args, {...options, limits, ...change}), /scoped_/);
    for (const altered of [['test'], ['build', ...args.slice(1)], [...args, '--release']]) assert.throws(() => runCargo(root, altered, {...options, limits}), /scoped_cargo_args/);
    assert.throws(() => runCargo(root, args, {...options, limits, oneShot: true}));
    assert.equal(calls, 1); assert.equal(statSync(protectedPath).size, 1);
  } finally { statMock.mock.restore(); syncBuiltinESMExports(); rmSync(root, {recursive: true, force: true}); }
});
