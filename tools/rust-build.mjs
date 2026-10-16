import {spawnSync} from 'node:child_process';
import {closeSync,copyFileSync,existsSync,lstatSync,mkdirSync,openSync,readFileSync,readSync,readdirSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {dirname,join,resolve} from 'node:path';
import {acquireChannel,admission,beginFreshCache,beginScratch,contained,sha256} from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
import {emmyRecipeFiles,emmyRecipePath,emmySupportFiles,inspectEmmy,verifyEmmyWiring} from './rust-emmy-identity.mjs';

export function hashFile(path,max=512<<20){const file=lstatSync(path);if(!file.isFile()||file.isSymbolicLink()||file.size>max)throw new Error('build.file_budget');const fd=openSync(path,'r'),hash=createHash('sha256'),chunk=Buffer.alloc(65536);let bytes=0;try{for(;;){const read=readSync(fd,chunk,0,chunk.length,null);if(!read)break;bytes+=read;if(bytes>max)throw new Error('build.file_budget');hash.update(chunk.subarray(0,read));}if(bytes!==file.size)throw new Error('build.file_changed');return hash.digest('hex');}finally{closeSync(fd);}}
export function sourceDigest(root){const hash=createHash('sha256');let entries=0;function visit(path,name){if(++entries>100000)throw new Error('build.source_budget');const info=lstatSync(path);if(info.isSymbolicLink())throw new Error('build.source_reparse');if(info.isDirectory()){for(const child of readdirSync(path).sort())visit(join(path,child),`${name}/${child}`);}else if(info.isFile()){hash.update(JSON.stringify([name,info.size,hashFile(path,64<<20)]));}else throw new Error('build.source_non_regular');}for(const name of ['Cargo.toml','Cargo.lock','crates','release','.cargo'])visit(join(root,name),name);for(const name of ['vendor','LICENSE','THIRD_PARTY_NOTICES.md','tests/rust-source-data/generate-unicode-simple.mjs'])if(existsSync(join(root,name)))visit(join(root,name),name);if(existsSync(join(root,emmyRecipePath))){for(const name of emmyRecipeFiles)visit(join(root,emmyRecipePath,name),`${emmyRecipePath}/${name}`);if(existsSync(join(root,emmyRecipePath,'patches')))visit(join(root,emmyRecipePath,'patches'),`${emmyRecipePath}/patches`);for(const name of emmySupportFiles.filter(path=>path.startsWith('tools/')))visit(join(root,name),name);}return hash.digest('hex');}

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
  const binary=contained(root,record.sourceBinary);if(hashFile(binary,128<<20)!==record.binarySHA256)throw new Error('build.artifact_integrity');
  if(record.pdb){const pdb=contained(root,record.pdb);if(hashFile(pdb)!==record.pdbSHA256||record.sameInvocation!==true)throw new Error('build.symbol_integrity');}
  return record;
}

// The same lease and terminal rules cover build, test, check and clippy. The
// injected executor is used only by fault tests; production invokes Cargo.
export function runCargo(root,args,options={}){
  return runCargoWithDeadline(root,args,options,600000);
}
// Optimized product preparation has a larger fixed build allowance. Keep this
// private so test/check/fetch and arbitrary Cargo callers retain their limit.
function runCargoWithDeadline(root,args,{sourceRoot=root,owner='rust-cargo',channelKind='ordinary',oneShot=false,environment={},freshOffline=false,execute=ownedSpawnSync,onSuccess}={},timeoutMs){
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
  if(hasSource)verifyEmmyWiring(readFileSync(sourceManifest,'utf8'),{optional:true});
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
    const env={...process.env,...environment,CARGO_TARGET_DIR:target};
    if(fresh){env.CARGO_HOME=join(job.directory,'cargo-home');mkdirSync(env.CARGO_HOME);env.CARGO_NET_OFFLINE='true';}
    if(oneShot||fresh)env.CARGO_INCREMENTAL='0';
    const environmentSHA256=sha256(Buffer.from(JSON.stringify(Object.entries(env).sort(([a],[b])=>a.localeCompare(b)))));
    terminal={state:'interrupted',handlesReaped:false,logs:[]};
    const result=execute('cargo',args,{cwd:sourceRoot,env,evidence:job.evidence,cargoBuild:true,windowsHide:true,shell:false,timeout:timeoutMs,maxBuffer:8<<20});
    writeFileSync(stdout,result.stdout||Buffer.alloc(0));writeFileSync(stderr,result.stderr||Buffer.alloc(0));
    writeFileSync(metadata,JSON.stringify({command:'cargo',args,timeoutMs,cwd:sourceRoot,channel:channel.target,target,cargoHome:fresh?env.CARGO_HOME:undefined,freshCacheReceipt:fresh?.receipt,oneShot,environmentSHA256,exit:result.status,signal:result.signal,error:result.error?.message,inputHashes:inputs},null,2)+'\n');
    terminal={state:result.handlesReaped===true?(result.status===0&&!result.error?'succeeded':'failed'):'interrupted',handlesReaped:result.handlesReaped===true,logs:[metadata,stdout,stderr,...(result.logs||[])]};
    if(result.error||result.status!==0||!terminal.handlesReaped)throw Object.assign(new Error(`build.cargo_failed:${result.error?.message||(!terminal.handlesReaped?'tree_reap_unverified':result.status)}`),{buildEvidence:job.receipt});
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

export function buildRust(root,artifactDirectory,{profile='dev',oneShot=false,owner='rust-build',packageName='lycheedev-cli',sourceRoot=root,environment={},execute=ownedSpawnSync}={}){
  root=resolve(root);sourceRoot=resolve(sourceRoot);
  if(!['dev','release','symbols'].includes(profile))throw new Error('build.invalid_profile');
  const args=['build','--locked','--offline','-j','1','--profile',profile,'-p',packageName];
  return runCargoWithDeadline(root,args,{sourceRoot,owner,execute,channelKind:profile==='symbols'?'symbols':'ordinary',oneShot:oneShot||profile!=='dev',environment,onSuccess(invocation){
    const output=profile==='dev'?'debug':profile;
    artifactDirectory=contained(root,resolve(artifactDirectory));mkdirSync(artifactDirectory);
    const source=join(invocation.target,output,'lycheedev.exe'),binary=join(artifactDirectory,'lycheedev.exe');
    const expected=hashFile(source,128<<20);copyFileSync(source,binary);
    if(hashFile(binary,128<<20)!==expected||hashFile(source,128<<20)!==expected)throw new Error('build.artifact_changed');
    const record={schema:'lycheedev.rust-build-artifact.v1',profile,sourceRoot,sourceBinary:binary,binarySHA256:expected,bytes:lstatSync(binary).size,inputHashes:invocation.inputHashes,environmentSHA256:invocation.environmentSHA256,channel:invocation.target,sameInvocation:true,command:['cargo',...args],buildEvidence:invocation.buildEvidence};
    if(profile==='symbols'){
      const original=join(invocation.target,output,'lycheedev.pdb');if(!existsSync(original))throw new Error('build.matching_symbols_missing');
      record.pdb=join(artifactDirectory,'lycheedev.pdb');record.pdbSHA256=hashFile(original);copyFileSync(original,record.pdb);
      if(hashFile(original)!==record.pdbSHA256||hashFile(record.pdb)!==record.pdbSHA256)throw new Error('build.symbols_changed');
    }
    writeFileSync(join(artifactDirectory,'artifact.json'),JSON.stringify(record,null,2)+'\n');return record;
  }},profile==='dev'?600000:1800000);
}
if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){
  const [directory,profile='dev']=process.argv.slice(2);if(!directory)throw new Error('usage: node tools/rust-build.mjs <new-artifact-directory> [dev|release|symbols] [--one-shot]');
  console.log(JSON.stringify(buildRust(process.cwd(),directory,{profile,oneShot:process.argv.includes('--one-shot')}),null,2));
}
