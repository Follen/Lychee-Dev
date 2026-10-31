import assert from 'node:assert/strict';

import {closeSync,copyFileSync,existsSync,lstatSync,mkdirSync,openSync,readFileSync,readSync,readdirSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {basename,dirname,join,resolve} from 'node:path';
import {homedir} from 'node:os';
import {acquireChannel, beginFreshCache, beginScratch, contained, sha256} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {emmyRecipeFiles,emmyRecipePath,emmySupportFiles,inspectEmmy,verifyEmmyWiring} from './rust-emmy-identity.mjs';

export function hashFile(path,max=512<<20){const file=lstatSync(path);if(!file.isFile()||file.isSymbolicLink()||file.size>max)throw new Error('build.file_budget');const fd=openSync(path,'r'),hash=createHash('sha256'),chunk=Buffer.alloc(65536);let bytes=0;try{for(;;){const read=readSync(fd,chunk,0,chunk.length,null);if(!read)break;bytes+=read;if(bytes>max)throw new Error('build.file_budget');hash.update(chunk.subarray(0,read));}if(bytes!==file.size)throw new Error('build.file_changed');return hash.digest('hex');}finally{closeSync(fd);}}
export function sourceDigest(root){const hash=createHash('sha256');let entries=0;
  // Runtime lock/state directories are created inside crates by the tests
  // themselves. They are not source inputs and must not change the digest while
  // a test batch is running (that would look like a concurrent source edit).
  const generated=new Set(['.lycheedev-locks','.lycheedev','target','target-symbols']);
  function visit(path,name){if(++entries>100000)throw new Error('build.source_budget');const info=lstatSync(path);if(info.isSymbolicLink())throw new Error('build.source_reparse');if(info.isDirectory()){for(const child of readdirSync(path).sort()){if(generated.has(child))continue;visit(join(path,child),`${name}/${child}`);}}else if(info.isFile()){hash.update(JSON.stringify([name,info.size,hashFile(path,64<<20)]));}else throw new Error('build.source_non_regular');}for(const name of ['Cargo.toml','Cargo.lock','crates','release','.cargo'])visit(join(root,name),name);for(const name of ['vendor','LICENSE','THIRD_PARTY_NOTICES.md','tools/generate-unicode-simple.mjs','tests/fixtures'])if(existsSync(join(root,name)))visit(join(root,name),name);if(existsSync(join(root,emmyRecipePath))){for(const name of emmyRecipeFiles)visit(join(root,emmyRecipePath,name),`${emmyRecipePath}/${name}`);if(existsSync(join(root,emmyRecipePath,'patches')))visit(join(root,emmyRecipePath,'patches'),`${emmyRecipePath}/patches`);for(const name of emmySupportFiles.filter(path=>path.startsWith('tools/')))visit(join(root,name),name);}return hash.digest('hex');}

// Existing Cargo isolation template, also used by owned acceptance controllers.
// Preserve the installed offline compiler caches before replacing user paths.
export function ownedUserEnvironment(directory,parent=process.env){
  const env={...parent},cargoHome=env.CARGO_HOME??join(homedir(),'.cargo'),rustupHome=env.RUSTUP_HOME??join(homedir(),'.rustup');
  const user=join(directory,'user'),temp=join(directory,'temp');
  for(const name of Object.keys(env))if(/^(HOME|USERPROFILE|CODEX_HOME|APPDATA|LOCALAPPDATA|TEMP|TMP)$/i.test(name)||/^LYCHEEDEV_(?:HOME|BIN)$/i.test(name))delete env[name];
  Object.assign(env,{CARGO_HOME:cargoHome,RUSTUP_HOME:rustupHome,HOME:user,USERPROFILE:user,CODEX_HOME:join(user,'.codex'),APPDATA:join(user,'AppData/Roaming'),LOCALAPPDATA:join(user,'AppData/Local'),TEMP:temp,TMP:temp,LYCHEEDEV_HOME:join(directory,'home')});
  for(const key of ['HOME','TEMP','APPDATA','LOCALAPPDATA'])mkdirSync(env[key],{recursive:true});
  return env;
}

// Fetch resolves the lockfile without compiling product code. Bind manifests
// and Cargo configuration; source workers can continue editing Rust meanwhile.
function manifestDigest(root){
  const rows=['Cargo.toml'];
  for(const name of readdirSync(join(root,'crates')).sort())if(existsSync(join(root,'crates',name,'Cargo.toml')))rows.push(`crates/${name}/Cargo.toml`);
  for(const name of readdirSync(join(root,'.cargo')).sort())if(lstatSync(join(root,'.cargo',name)).isFile())rows.push(`.cargo/${name}`);
  return sha256(Buffer.from(JSON.stringify(rows.map(name=>[name,hashFile(join(root,name),1<<20)]))));
}

export function verifyArtifact(root,path){
  path=contained(root,resolve(path));const stat=lstatSync(path);if(!stat.isFile()||stat.size>65536)throw new Error('build.invalid_artifact_record');
  const record=JSON.parse(readFileSync(path));if(!['lycheedev.rust-build-artifact.v1','lycheedev.optimized-symbol-artifact.v1'].includes(record.schema))throw new Error('build.unknown_artifact');
  const binary=contained(root,record.sourceBinary);if(hashFile(binary,512<<20)!==record.binarySHA256)throw new Error('build.artifact_integrity');
  if(record.pdb){const pdb=contained(root,record.pdb);if(hashFile(pdb)!==record.pdbSHA256||record.sameInvocation!==true)throw new Error('build.symbol_integrity');}
  return record;
}

// Preserve the terminal producer proof, not its scratch directory or Cargo
// cache. Original receipt paths remain historical evidence; copied logs are
// found by basename beside this byte-identical receipt, as for formal Lua.
export function preserveBuildEvidence(root,receiptPath,artifactDirectory){
  const source=contained(root,resolve(receiptPath)),receiptBytes=readFileSync(source),receipt=JSON.parse(receiptBytes);
  assert.equal(receipt.schema,'lycheedev.disk-owner.v1','build.evidence_schema');
  assert.equal(receipt.category,'scratch','build.evidence_category');
  assert.equal(receipt.state,'retired','build.evidence_not_retired');
  assert.equal(receipt.handlesReaped,true,'build.evidence_not_reaped');
  assert.equal(receipt.activePid,null,'build.evidence_active');
  assert(Array.isArray(receipt.sealedLogs)&&receipt.sealedLogs.length>0,'build.evidence_logs_missing');
  const names=new Set(['receipt.json']);
  const logs=receipt.sealedLogs.map(row=>{
    const path=contained(dirname(source),row.path),name=basename(path);
    assert(!names.has(name),'build.evidence_filename_collision');names.add(name);
    const digest=hashFile(path,16<<20);assert.equal(digest,row.sha256,'build.evidence_log_corrupt');
    return {path,name,digest};
  });
  const directory=contained(root,join(resolve(artifactDirectory),'evidence'));mkdirSync(directory);
  for(const log of logs){
    const bytes=readFileSync(log.path);assert.equal(sha256(bytes),log.digest,'build.evidence_log_changed');
    writeFileSync(join(directory,log.name),bytes,{flag:'wx'});
    assert.equal(hashFile(join(directory,log.name),16<<20),log.digest,'build.evidence_copy_changed');
  }
  const buildEvidence=join(directory,'receipt.json');
  assert.deepEqual(readFileSync(source),receiptBytes,'build.evidence_receipt_changed');
  writeFileSync(buildEvidence,receiptBytes,{flag:'wx'});
  const buildEvidenceSHA256=sha256(receiptBytes);
  assert.equal(hashFile(buildEvidence,16<<20),buildEvidenceSHA256,'build.evidence_receipt_copy_changed');
  const preserved={buildEvidence,buildEvidenceSHA256};
  const timing=logs.find(log=>log.name==='cargo-timing.html');
  if(timing)preserved.cargoTimings={path:join(directory,timing.name),sha256:timing.digest};
  return preserved;
}

function timingSignature(path){
  if(!existsSync(path))return undefined;
  const info=lstatSync(path,{bigint:true});
  if(!info.isFile()||info.isSymbolicLink())throw new Error('build.invalid_cargo_timings');
  return [info.ino,info.size,info.mtimeNs,info.ctimeNs].map(String).join(':');
}

// The same lease and terminal rules cover build, test, check and clippy. The
// injected executor is used only by fault tests; production invokes Cargo.
export function runCargo(root,args,options={}){
  const compiling=['build','test','check','clippy'].includes(args[0]);
  return runCargoWithDeadline(root,args,options,compiling?3600000:900000);
}
function runCargoWithDeadline(root,args,{sourceRoot=root,owner='rust-cargo',channelKind='ordinary',oneShot=false,environment={},isolateUser=false,freshOffline=false,execute=ownedSpawnSync,onSuccess}={},timeoutMs){
  root=resolve(root);sourceRoot=resolve(sourceRoot);
  if(sourceRoot!==root)contained(root,sourceRoot);
  if(!['build','test','check','clippy','fetch','metadata'].includes(args[0]))throw new Error('build.unsupported_cargo_command');
  const fetching=args[0]==='fetch',inspecting=args[0]==='metadata',compiling=!fetching&&!inspecting;
  if(args.some(value=>/^--(?:target-dir|config|manifest-path)(?:=|$)/.test(value)))throw new Error('build.uncontrolled_cargo_option');
  if(process.env.CARGO_TARGET_DIR||environment.CARGO_TARGET_DIR)throw new Error('build.uncontrolled_target_override');
  if(freshOffline&&(channelKind!=='ordinary'||!onSuccess||environment.CARGO_HOME))throw new Error('build.invalid_fresh_gate');
  const sourceManifest=join(sourceRoot,'crates/lycheedev-source/Cargo.toml');
  const adapterManifest=join(sourceRoot,'crates/lycheedev-emmy-adapter/Cargo.toml');
  let emmy;
  const hasSource=existsSync(sourceManifest),hasAdapter=existsSync(adapterManifest);
  if(hasSource)verifyEmmyWiring(readFileSync(sourceManifest,'utf8'),{optional:false});
  if(hasAdapter)verifyEmmyWiring(readFileSync(adapterManifest,'utf8'),{optional:false});
  if(hasSource||hasAdapter){
    emmy=inspectEmmy(sourceRoot);
  }
  // This existing runtime variable identifies the authenticated dependency tree,
  // including the zero-patch public upstream identity.
  // It must never contain the historical private manifest/patch identity.
  const patch=emmy?.patchedTreeSHA256;
  if(patch){
    for(const value of [process.env.LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256,environment.LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256])
      if(value!==undefined&&value!==patch)throw new Error('build.emmy_patch_identity_differs');
    environment={...environment,LYCHEEDEV_EMMY_CONTEXT_PATCH_SHA256:patch};
  }
  const inputs={...(!compiling?{manifests:manifestDigest(sourceRoot),priorCargoLock:hashFile(join(sourceRoot,'Cargo.lock'))}:{sourceTree:sourceDigest(sourceRoot)}),
    ...(emmy?{emmyPatch:patch,emmy:{schema:emmy.schema,identitySHA256:emmy.identitySHA256,recipeSHA256:emmy.recipeSHA256,patchedTreeSHA256:patch,historicalPrivatePatchRecovered:false}}:{})};
  const job=beginScratch(root,owner,{command:['cargo',...args],inputs,admissionOptions:{requiredBytes:2*1024**3,cacheReserveBytes:2*1024**3}});
  let channel;
  try{channel=acquireChannel(root,channelKind,owner);}catch(error){job.seal({state:'failed',handlesReaped:true});job.cleanup();throw error;}
  const metadata=join(job.evidence,'command.json'),stdout=join(job.evidence,'stdout.bin'),stderr=join(job.evidence,'stderr.bin');
  let terminal={state:'failed',handlesReaped:true,logs:[]},fresh;
  try{
    if(freshOffline)fresh=beginFreshCache(root,channel,job);
    const target=fresh?fresh.directory:channel.target;
    const inherited={...process.env,...environment,CARGO_TARGET_DIR:target};
    const env=isolateUser?ownedUserEnvironment(job.directory,inherited):inherited;
    if(fresh){env.CARGO_HOME=join(job.directory,'cargo-home');mkdirSync(env.CARGO_HOME);env.CARGO_NET_OFFLINE='true';}
    if(oneShot||fresh)env.CARGO_INCREMENTAL='0';
    const environmentSHA256=sha256(Buffer.from(JSON.stringify(Object.entries(env).sort(([a],[b])=>a.localeCompare(b)))));
    const timingPath=args.includes('--timings')?join(target,'cargo-timings/cargo-timing.html'):undefined;
    const previousTiming=timingPath?timingSignature(timingPath):undefined;
    terminal={state:'interrupted',handlesReaped:false,logs:[]};
    const startedAtUtc=new Date().toISOString(),startedTick=performance.now();
    const result=execute('cargo',args,{cwd:sourceRoot,env,evidence:job.evidence,cargoBuild:true,windowsHide:true,shell:false,timeout:timeoutMs,maxBuffer:8<<20});
    const elapsedMs=Math.round(performance.now()-startedTick),endedAtUtc=new Date().toISOString();
    writeFileSync(stdout,result.stdout||Buffer.alloc(0));writeFileSync(stderr,result.stderr||Buffer.alloc(0));
    writeFileSync(metadata,JSON.stringify({command:'cargo',args,timeoutMs,startedAtUtc,endedAtUtc,elapsedMs,cwd:sourceRoot,channel:channel.target,target,cargoHome:fresh?env.CARGO_HOME:undefined,freshCacheReceipt:fresh?.receipt,oneShot,...(isolateUser?{isolatedUserRoot:env.HOME}:{}),environmentSHA256,exit:result.status,signal:result.signal,error:result.error?.message,inputHashes:inputs},null,2)+'\n');
    terminal={state:result.handlesReaped===true?(result.status===0&&!result.error?'succeeded':'failed'):'interrupted',handlesReaped:result.handlesReaped===true,logs:[metadata,stdout,stderr,...(result.logs||[])]};
    if(result.error||result.status!==0||!terminal.handlesReaped)throw Object.assign(new Error(`build.cargo_failed:${result.error?.message||(!terminal.handlesReaped?'tree_reap_unverified':result.status)}`),{buildEvidence:job.receipt});
    if(timingPath){
      const currentTiming=timingSignature(timingPath);
      if(!currentTiming||currentTiming===previousTiming)throw new Error('build.cargo_timings_not_updated');
      const digest=hashFile(timingPath,16<<20),preserved=join(job.evidence,'cargo-timing.html');
      writeFileSync(preserved,readFileSync(timingPath),{flag:'wx'});
      if(hashFile(preserved,16<<20)!==digest||hashFile(timingPath,16<<20)!==digest)throw new Error('build.cargo_timings_changed');
      terminal.logs.push(preserved);
    }
    if(!compiling){
      if(manifestDigest(sourceRoot)!==inputs.manifests)throw new Error('build.manifests_changed_during_fetch');
      inputs.resolvedCargoLock=hashFile(join(sourceRoot,'Cargo.lock'));
      if(inspecting&&inputs.resolvedCargoLock!==inputs.priorCargoLock)throw new Error('build.lock_changed_during_metadata');
      const command=JSON.parse(readFileSync(metadata));command.inputHashes=inputs;command.compilation=false;
      writeFileSync(metadata,JSON.stringify(command,null,2)+'\n');
    }else if(sourceDigest(sourceRoot)!==inputs.sourceTree)throw new Error('build.source_changed_during_compile');
    if(emmy){
      const after=inspectEmmy(sourceRoot);
      if(after.patchedTreeSHA256!==patch||after.recipeSHA256!==emmy.recipeSHA256)throw new Error('build.emmy_identity_changed_during_cargo');
    }
    const invocation={target,inputHashes:inputs,environmentSHA256,buildEvidence:job.receipt,freshCacheReceipt:fresh?.receipt,cargoHome:fresh?env.CARGO_HOME:undefined,stdout:result.stdout?.toString()||'',stderr:result.stderr?.toString()||''};
    const resultValue=onSuccess?onSuccess(invocation):invocation;
    if(fresh)fresh.sealArtifacts(resultValue.preservedArtifacts);
    return resultValue;
  }catch(error){if(terminal.handlesReaped)terminal.state='failed';if(!terminal.logs.length){writeFileSync(metadata,JSON.stringify({command:'cargo',args,cwd:sourceRoot,error:error.message,inputHashes:inputs},null,2)+'\n');terminal.logs=[metadata];}throw error;}finally{
    job.seal(terminal);
    if(terminal.handlesReaped){if(fresh)fresh.cleanup();job.cleanup();channel.release();}
  }
}

export function validateCargoJobs(value=1){
  if(!['number','string'].includes(typeof value)||(typeof value==='string'&&!/^[1-4]$/.test(value)))throw new Error('build.invalid_jobs');
  const jobs=Number(value);if(!Number.isInteger(jobs)||jobs<1||jobs>4)throw new Error('build.invalid_jobs');return jobs;
}

export function parseBuildOptions(argv){
  const positional=[],seen=new Set();let oneShot=false,jobs=1,timings=false;
  for(let index=0;index<argv.length;index++){
    const arg=argv[index];
    if(!arg.startsWith('-')){positional.push(arg);continue;}
    if(!['--one-shot','--jobs','--timings'].includes(arg))throw new Error(`build.unknown_option: ${arg}`);
    if(seen.has(arg))throw new Error(`build.duplicate_option: ${arg}`);seen.add(arg);
    if(arg==='--one-shot'){oneShot=true;continue;}
    if(arg==='--timings'){timings=true;continue;}
    const value=argv[++index];if(value===undefined||value.startsWith('-'))throw new Error('build.option_value_missing: --jobs');
    jobs=validateCargoJobs(value);
  }
  if(!positional[0])throw new Error('usage: node tools/rust-build.mjs <new-artifact-directory> [dev|release|symbols] [--one-shot] [--jobs 1..4] [--timings]');
  if(positional.length>2)throw new Error('build.unexpected_argument');
  const [directory,profile='dev']=positional;if(!['dev','release','symbols'].includes(profile))throw new Error('build.invalid_profile');
  return{directory,profile,oneShot,jobs,...(timings?{timings:true}:{})};
}

export function buildRust(root,artifactDirectory,{profile='dev',oneShot=false,jobs=1,timings=false,isolateUser=false,owner='rust-build',packageName='lycheedev-cli',sourceRoot=root,environment={},execute=ownedSpawnSync}={}){
  root=resolve(root);sourceRoot=resolve(sourceRoot);
  if(!['dev','release','symbols'].includes(profile))throw new Error('build.invalid_profile');
  const args=['build','--locked','--offline','-j',String(validateCargoJobs(jobs)),'--profile',profile,'-p',packageName];
  if(timings)args.push('--timings');
  const record=runCargo(root,args,{sourceRoot,owner,execute,channelKind:profile==='symbols'?'symbols':'ordinary',oneShot,isolateUser,environment,onSuccess(invocation){
    const output=profile==='dev'?'debug':profile;
    artifactDirectory=contained(root,resolve(artifactDirectory));
    mkdirSync(dirname(artifactDirectory),{recursive:true});mkdirSync(artifactDirectory);
    const source=join(invocation.target,output,'lycheedev.exe'),binary=join(artifactDirectory,'lycheedev.exe');
    // 128 MiB no longer covers the current debug binary (134,360,576 B); the
    // cap only rejects runaway outputs, so keep a fixed 512 MiB bound for both
    // local debug artifacts and the (smaller) shipped release binary.
    const binaryBudget=512<<20;
    const expected=hashFile(source,binaryBudget);copyFileSync(source,binary);
    if(hashFile(binary,binaryBudget)!==expected||hashFile(source,binaryBudget)!==expected)throw new Error('build.artifact_changed');
    const record={schema:'lycheedev.rust-build-artifact.v1',profile,sourceRoot,sourceBinary:binary,binarySHA256:expected,bytes:lstatSync(binary).size,inputHashes:invocation.inputHashes,environmentSHA256:invocation.environmentSHA256,channel:invocation.target,sameInvocation:true,command:['cargo',...args],buildEvidence:invocation.buildEvidence};
    if(profile==='symbols'){
      const original=join(invocation.target,output,'lycheedev.pdb');if(!existsSync(original))throw new Error('build.matching_symbols_missing');
      record.pdb=join(artifactDirectory,'lycheedev.pdb');record.pdbSHA256=hashFile(original);copyFileSync(original,record.pdb);
      if(hashFile(original)!==record.pdbSHA256||hashFile(record.pdb)!==record.pdbSHA256)throw new Error('build.symbols_changed');
    }
    return record;
  }});
  Object.assign(record,preserveBuildEvidence(root,record.buildEvidence,artifactDirectory));
  writeFileSync(join(artifactDirectory,'artifact.json'),JSON.stringify(record,null,2)+'\n',{flag:'wx'});
  return record;
}
if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){
  const {directory,...options}=parseBuildOptions(process.argv.slice(2));
  console.log(JSON.stringify(buildRust(process.cwd(),directory,{...options,isolateUser:options.profile==='release'}),null,2));
}
