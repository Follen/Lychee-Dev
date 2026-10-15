// Fixed contract-only Windows CI experiment. Product runCargo custody is unchanged.
// No local-Linux path, resume/adoption, arbitrary command, resource probe or retry.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync,readdirSync,lstatSync,copyFileSync,realpathSync,existsSync,openSync,readSync,closeSync} from 'node:fs';
import {join,relative,dirname,resolve,sep,isAbsolute} from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
const source=dirname(fileURLToPath(import.meta.url)), target='x86_64-pc-windows-msvc';
const mode=process.argv[2], approvedFreeze=process.argv[3];
if(process.platform!=='win32'||process.argv.length!==4||mode!=='contracts'||!/^[a-f0-9]{64}$/.test(approvedFreeze??''))throw Error('windows-contract.fixed_windows_contract_command');
for(const [key,value]of Object.entries({GITHUB_ACTIONS:'true',RUNNER_OS:'Windows',GITHUB_REPOSITORY:'Follen/Lychee-Dev',GITHUB_REF:'refs/heads/rust-refactor'}))assert.equal(process.env[key],value,`windows-contract.${key}`);
assert(/^[a-f0-9]{40}$/.test(process.env.GITHUB_SHA??''),'windows-contract.checkout_identity');
const repository=realpathSync(process.env.GITHUB_WORKSPACE);
assert.equal(realpathSync(source),realpathSync(join(repository,'tests/emmy-public-ingestion-three-way')),'windows-contract.fixed_payload_path');
const ordered=rows=>Object.fromEntries(Object.entries(rows).sort(([a],[b])=>a<b?-1:a>b?1:0));
function noLinks(path){for(let p=resolve(path);;p=dirname(p)){if(existsSync(p)&&lstatSync(p).isSymbolicLink())throw Error('windows-contract.reparse');if(p===dirname(p))break;}}
function hash(path,max=512<<20){
 noLinks(path);const before=lstatSync(path);if(!before.isFile()||before.size>max)throw Error('windows-contract.file_budget');
 const fd=openSync(path,'r'),h=createHash('sha256'),buffer=Buffer.alloc(65536);let bytes=0;
 try{for(;;){const n=readSync(fd,buffer,0,buffer.length,null);if(!n)break;bytes+=n;if(bytes>max)throw Error('windows-contract.file_grew');h.update(buffer.subarray(0,n));}}finally{closeSync(fd);}
 const after=lstatSync(path);if(bytes!==before.size||after.size!==before.size||after.mtimeMs!==before.mtimeMs||after.ctimeMs!==before.ctimeMs)throw Error('windows-contract.file_changed');return h.digest('hex');
}
function inventory(base,{maxEntries=8192,maxBytes=128<<20,skip=()=>false,rejectEmptyDirectories=true}={}){
 noLinks(base);assert(lstatSync(base).isDirectory(),'windows-contract.inventory_root');const rows={},directories=[];let entries=0,bytes=0;
 function visit(path){for(const name of readdirSync(path).sort()){if(++entries>maxEntries)throw Error('windows-contract.entry_budget');const p=join(path,name),rel=relative(base,p).split(sep).join('/'),info=lstatSync(p);if(info.isSymbolicLink())throw Error('windows-contract.reparse');if(skip(rel,info))continue;if(info.isDirectory()){directories.push(rel);visit(p);}else{if(!info.isFile()||(bytes+=info.size)>maxBytes)throw Error('windows-contract.tree_budget');rows[rel]=hash(p);}}}visit(base);if(rejectEmptyDirectories){const expected=new Set();for(const file of Object.keys(rows)){const parts=file.split('/');while(parts.length>1){parts.pop();expected.add(parts.join('/'));}}assert.deepEqual(directories.sort(),[...expected].sort(),'windows-contract.unexpected_empty_directory');}return ordered(rows);
}
const freezePath=join(source,'freeze.json');assert.equal(hash(freezePath),approvedFreeze,'windows-contract.reviewed_freeze');
const freeze=JSON.parse(readFileSync(freezePath)),inputs=JSON.parse(readFileSync(join(source,'inputs.json')));
assert.equal(freeze.schema,'lycheedev.emmy-windows-contract-freeze.v1');assert.equal(inputs.target,target);
function payloadCheck(){assert.equal(hash(freezePath),approvedFreeze);assert.deepEqual(inventory(source,{skip:p=>p==='freeze.json'}),freeze.files,'windows-contract.complete_payload');}
payloadCheck();
// Import the frozen owners only after their complete payload has been checked.
const {listedTests,suiteResult,traceRows,completeVariant,compareVariants}=await import('./assertions.mjs');
const {beginScratch,acquireChannel,sha256}=await import('./tools/rust-disk.mjs');
const {ownedSpawnSync}=await import('./tools/rust-owned-process.mjs');
function application(name){for(const directory of (process.env.PATH??'').split(';').filter(Boolean)){const p=join(directory,name);if(existsSync(p)){noLinks(p);if(lstatSync(p).isFile())return realpathSync(p);}}throw Error(`windows-contract.tool_missing:${name}`);}
const rustup=process.env.RUSTUP_HOME||join(process.env.USERPROFILE,'.rustup');
const toolchain=realpathSync(join(rustup,`toolchains/1.99.0-${target}`)),bin=join(toolchain,'bin');
const cargo=join(bin,'cargo.exe'),rustc=join(bin,'rustc.exe'),rustdoc=join(bin,'rustdoc.exe'),python=application('python.exe'),git=application('git.exe');
const powershell=join(process.env.SystemRoot,'System32/WindowsPowerShell/v1.0/powershell.exe');
const auxiliary=JSON.parse(readFileSync(join(source,'tools/rust-build-auxiliary.json')));
assert.equal(hash(cargo),auxiliary.cargoSHA256,'windows-contract.fixed_official_cargo');
const compiler=auxiliary.msvcCompilers.find(row=>row.programFilesPath.includes('/Enterprise/'));
assert(compiler,'windows-contract.fixed_msvc_policy');
const programFiles=process.env.ProgramW6432||process.env.ProgramFiles;
const linker=join(programFiles,compiler.linker.programFilesPath),cl=join(programFiles,compiler.programFilesPath),msvcBin=dirname(linker);
assert.equal(hash(linker),compiler.linker.sha256,'windows-contract.fixed_linker');assert.equal(hash(cl),compiler.sha256,'windows-contract.fixed_cl');
const msvcRoot=resolve(msvcBin,'../../..'),sdkRoot=join(process.env['ProgramFiles(x86)'],'Windows Kits/10');
const sdkVersions=readdirSync(join(sdkRoot,'Lib')).filter(v=>/^10\.0\.\d+\.0$/.test(v)&&existsSync(join(sdkRoot,'Lib',v,'um/x64/kernel32.lib'))&&existsSync(join(sdkRoot,'Lib',v,'ucrt/x64/ucrt.lib'))).sort((a,b)=>Number(b.split('.')[2])-Number(a.split('.')[2]));
assert(sdkVersions.length>0,'windows-contract.windows_sdk');const sdkVersion=sdkVersions[0];
const libraries=[join(msvcRoot,'lib/x64'),join(sdkRoot,'Lib',sdkVersion,'um/x64'),join(sdkRoot,'Lib',sdkVersion,'ucrt/x64')];
const headers=[join(msvcRoot,'include'),...['ucrt','shared','um','winrt'].map(p=>join(sdkRoot,'Include',sdkVersion,p))];
function toolIdentities(){
 const files=new Set([cargo,rustc,rustdoc,realpathSync(process.execPath),python,powershell,git,join(toolchain,'lib/rustlib/multirust-channel-manifest.toml')]);
 for(const component of ['rustc','rust-std']){const manifest=join(toolchain,`lib/rustlib/manifest-${component}-${target}`);files.add(manifest);const raw=readFileSync(manifest);assert(raw.length<=256<<10,'windows-contract.tool_manifest_budget');for(const line of raw.toString().trim().split(/\r?\n/)){assert(line.startsWith('file:'),'windows-contract.tool_manifest_shape');const p=resolve(toolchain,line.slice(5)),rel=relative(toolchain,p);assert(rel&&!rel.startsWith('..')&&!isAbsolute(rel),'windows-contract.tool_manifest_path');files.add(p);}}
 assert(files.size<=1024,'windows-contract.tool_file_count');
 const trees={};for(const directory of [msvcBin,...libraries,...headers])trees[realpathSync(directory)]=inventory(directory,{maxEntries:20000,maxBytes:1024**3,rejectEmptyDirectories:false});
 const pyRoot=dirname(python);for(const directory of [join(pyRoot,'DLLs'),join(pyRoot,'Lib')])trees[realpathSync(directory)]=inventory(directory,{maxEntries:20000,maxBytes:256<<20,rejectEmptyDirectories:false,skip:p=>p.split('/').includes('site-packages')});
 for(const name of readdirSync(pyRoot))if(/\.(exe|dll|zip)$/i.test(name))files.add(join(pyRoot,name));
 for(const name of readdirSync(dirname(powershell)))if(/\.(exe|dll|config)$/i.test(name))files.add(join(dirname(powershell),name));
 return {files:ordered(Object.fromEntries([...files].sort().map(path=>[realpathSync(path),hash(path)]))),trees,systemRuntime:{imageOS:process.env.ImageOS??null,imageVersion:process.env.ImageVersion??null,runnerOS:process.env.RUNNER_OS,node:process.version,architecture:process.arch,sdkVersion,scope:'Windows system DLL/.NET loader are trusted hosted-OS runtime, not represented as a reproduced whole-OS closure'}};
}
assert(/^v24\./.test(process.version)&&process.arch==='x64','windows-contract.node24_x64');
const tools=toolIdentities();
function configurations(cwd,home){const paths=new Set([join(home,'config'),join(home,'config.toml')]);for(let p=cwd;;p=dirname(p)){paths.add(join(p,'.cargo/config'));paths.add(join(p,'.cargo/config.toml'));if(p===dirname(p))break;}return ordered(Object.fromEntries([...paths].sort().map(p=>[p,existsSync(p)?hash(p,65536):null])));}
const job=beginScratch(repository,'emmy-public-ingestion-windows-contracts',{command:['contracts',approvedFreeze],inputs:{approvedFreeze,inputs,tools,github:{sha:process.env.GITHUB_SHA,runId:process.env.GITHUB_RUN_ID,attempt:process.env.GITHUB_RUN_ATTEMPT}},retention:'rebuildable',admissionOptions:{requiredBytes:4*1024**3,cacheReserveBytes:2*1024**3,scratchReserveBytes:512<<20}});
let channel,terminal={state:'failed',handlesReaped:true,logs:[]},originals,cacheReady=false,finalDependencyCheck;const stages=[];
function log(name,data){const path=join(job.evidence,name);writeFileSync(path,typeof data==='string'||Buffer.isBuffer(data)?data:JSON.stringify(data,null,2)+'\n');if(!terminal.logs.includes(path))terminal.logs.push(path);return path;}
try{
 channel=acquireChannel(repository,'symbols','emmy-public-ingestion-windows-contracts');mkdirSync(channel.target,{recursive:true});
 const home=join(job.directory,'cargo-home'),tmp=join(job.directory,'tmp');mkdirSync(home);mkdirSync(tmp);
 const env={};for(const key of ['SystemRoot','WINDIR','COMSPEC','PATHEXT','USERPROFILE','APPDATA','LOCALAPPDATA','ProgramFiles','ProgramFiles(x86)','ProgramW6432','NUMBER_OF_PROCESSORS','PROCESSOR_ARCHITECTURE','PROCESSOR_IDENTIFIER','GITHUB_WORKSPACE'])if(process.env[key]!==undefined)env[key]=process.env[key];
 Object.assign(env,{PATH:[bin,dirname(process.execPath),dirname(python),msvcBin,dirname(powershell),join(process.env.SystemRoot,'System32'),dirname(git)].join(';'),RUSTUP_HOME:rustup,CARGO_HOME:home,RUSTC:rustc,RUSTDOC:rustdoc,CARGO_TARGET_DIR:channel.target,CARGO_NET_OFFLINE:'true',CARGO_REGISTRIES_CRATES_IO_PROTOCOL:'sparse',CARGO_INCREMENTAL:'0',CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER:linker,LIB:libraries.join(';'),INCLUDE:headers.join(';'),TEMP:tmp,TMP:tmp,PYTHONDONTWRITEBYTECODE:'1'});
 const ancestorConfigs=configurations(job.directory,home);
 // No ambient configuration is admitted except the coordinator's frozen standard config.
 for(const [path,digest]of Object.entries(ancestorConfigs))if(digest!==null)assert(path===join(repository,'.cargo/config.toml')&&digest===freeze.stageConfigurationSHA256,'windows-contract.ambient_cargo_config');
 log('runtime-tools.json',tools);log('runtime-environment.json',{environmentSHA256:sha256(Buffer.from(JSON.stringify(ordered(env)))),values:env,ancestorConfigs,initialCargoHomeEmpty:readdirSync(home).length===0});
 function check(){payloadCheck();assert.deepEqual(toolIdentities(),tools,'windows-contract.tool_drift');assert.deepEqual(configurations(job.directory,home),ancestorConfigs,'windows-contract.configuration_drift');if(originals){assert.deepEqual(inventory(join(job.directory,'original')),inputs.original,'windows-contract.original848');assert.deepEqual(inventory(join(job.directory,'product')),inputs.product,'windows-contract.product853');assert.equal(hash(join(job.directory,'base.tar.gz')),inputs.archiveSHA256);}for(const s of stages){assert.deepEqual(inventory(s.cwd),s.expected,'windows-contract.stage_drift');assert.deepEqual(configurations(s.cwd,home),s.configs,'windows-contract.stage_config_drift');}}
 function run(cwd,command,args,{timeout=120000,fetch=false,cargoBuild=false}={}){
  check();terminal.state='interrupted';terminal.handlesReaped=false;
  const childEnv={...env,CARGO_NET_OFFLINE:fetch?'false':'true'};
  const result=ownedSpawnSync(command,args,{cwd,env:childEnv,evidence:job.evidence,timeout,cargoBuild});terminal.logs.push(...result.logs);terminal.handlesReaped=result.handlesReaped===true;
  check();if(result.error||result.status!==0||!terminal.handlesReaped)throw Error(`windows-contract.process_failed:${result.error?.message??result.status}:${result.stderr.toString().slice(-4000)}`);return result;
 }
 const observedHead=run(job.directory,git,['-C',repository,'-c','core.fsmonitor=false','rev-parse','HEAD']).stdout.toString().trim();assert.equal(observedHead,process.env.GITHUB_SHA,'windows-contract.actual_checkout');
 const rustVersion=run(job.directory,rustc,['-Vv']).stdout.toString();assert(rustVersion.includes('release: 1.99.0\n')||rustVersion.includes('release: 1.99.0\r\n'),'windows-contract.rust_version');assert(rustVersion.includes(`host: ${target}`),'windows-contract.rust_host');log('rust-version.txt',rustVersion);
 const py=run(job.directory,python,['-I','-B','-S','-c','import sys; assert (3,12) <= sys.version_info[:2] < (3,15); print(sys.version)']);log('python-version.txt',py.stdout);
 const assembled=run(job.directory,python,['-I','-B','-S',join(source,'prepare_inputs.py')],{timeout:360000});log('assembly.json',assembled.stdout);originals=true;check();
 function copy(root,path,dest,expected){const out=join(dest,path);mkdirSync(dirname(out),{recursive:true});copyFileSync(join(root,path),out);assert.equal(hash(out),expected,'windows-contract.copy_integrity');}
 for(const label of ['A','B','C']){
  const cwd=join(job.directory,label);mkdirSync(cwd);const expected={...inputs.owned.B};for(const [path,digest]of Object.entries(inputs.owned.B))copy(source,path,cwd,digest);
  if(label==='A'){const p='crates/lycheedev-emmy-adapter/src/declaration.rs';copyFileSync(join(source,'fixtures/old-declaration.rs'),join(cwd,p));expected[p]=inputs.owned.A[p];assert.equal(hash(join(cwd,p)),expected[p]);}
  const engine=label==='C'?'original':'product';for(const [path,digest]of Object.entries(inputs[engine])){const dest=`.tmp/emmy-reconstruction-v1/upstream/${path}`,out=join(cwd,dest);mkdirSync(dirname(out),{recursive:true});copyFileSync(join(job.directory,engine,path),out);assert.equal(hash(out),digest);expected[dest]=digest;}
  for(const [from,to]of [['harness-Cargo.toml','Cargo.toml'],[`Cargo-${label}.lock`,'Cargo.lock'],['stage-config.toml','.cargo/config.toml']]){mkdirSync(dirname(join(cwd,to)),{recursive:true});copyFileSync(join(source,from),join(cwd,to));expected[to]=freeze.files[from];assert.equal(hash(join(cwd,to)),expected[to]);}
  const s={label,cwd,expected:ordered(expected),configs:configurations(cwd,home)};stages.push(s);check();log(`${label}-stage.json`,{expected:s.expected,configs:s.configs});
 }
 const treeArgs=['tree','--locked','--offline','--target',target,'--package','lycheedev-emmy-adapter','--edges','normal,build,dev','--prefix','none','--format','{p}'];
 function dependencies(s,phase){const out=run(s.cwd,python,['-I','-B','-S',join(source,'check_selected_cache.py'),join(s.cwd,'Cargo.lock'),home,join(job.evidence,`${s.label}-tree.txt`),job.evidence],{timeout:180000}).stdout;const value=JSON.parse(out);assert(value.registry>0&&value.local.length===5,'windows-contract.nonempty_closure');log(`${s.label}-dependencies-${phase}.json`,out);return out;}
 finalDependencyCheck=()=>{const saved=terminal.state;for(const s of stages)if(s.dependencies)assert.deepEqual(dependencies(s,'final'),s.dependencies,'windows-contract.final_closure_drift');terminal.state=saved;};
 // Official Cargo fetch only; no custom registry downloader or version resolution.
 for(const s of stages){run(s.cwd,cargo,['fetch','--locked','--target',target],{timeout:600000,fetch:true});log(`${s.label}-tree.txt`,run(s.cwd,cargo,treeArgs).stdout);s.dependencies=dependencies(s,'prepared');}
 cacheReady=true;log('execution-inputs.json',{approvedFreeze,tools,dependencies:Object.fromEntries(stages.map(s=>[s.label,{lockSHA256:hash(join(s.cwd,'Cargo.lock')),sha256:sha256(s.dependencies),selected:JSON.parse(s.dependencies)}])),scope:inputs.scope});
 const results={};
 for(const s of stages){
  assert.deepEqual(dependencies(s,'before'),s.dependencies,'windows-contract.prepared_closure_drift');
  const args=['test','--locked','--offline','--target',target,'--package','lycheedev-emmy-adapter','--lib','--test','declaration_contract','--test','callback_contract','--test','declaration_rendering_contract','--test','declaration_ingestion_contract','--no-run','--message-format=json','-j','1'];
  const compiled=run(s.cwd,cargo,args,{timeout:600000,cargoBuild:true});
  const artifacts=compiled.stdout.toString().split(/\r?\n/).filter(Boolean).map(JSON.parse).filter(row=>row.reason==='compiler-artifact'&&row.profile.test&&row.executable);assert.equal(artifacts.length,5,'windows-contract.five_test_binaries');const binaries={};
  for(const artifact of artifacts){const name=artifact.target.name;assert(Object.hasOwn(inputs.tests,name)&&!Object.hasOwn(binaries,name),'windows-contract.unexpected_executable');const path=realpathSync(artifact.executable),rel=relative(realpathSync(channel.target),path);assert(rel&&!rel.startsWith('..')&&!isAbsolute(rel),'windows-contract.binary_outside_target');binaries[name]={path,sha256:hash(path)};}
  log(`${s.label}-binaries.json`,binaries);const suites={};let parity=[],outcomes=[],checkpoints=[];
  for(const [name,names]of Object.entries(inputs.tests)){const b=binaries[name];assert(b);assert.equal(hash(b.path),b.sha256);listedTests(run(s.cwd,b.path,['--list']).stdout,names);const output=run(s.cwd,b.path,['--nocapture','--test-threads=1']).stdout;assert.equal(hash(b.path),b.sha256);suites[name]=suiteResult(output,names);if(name==='declaration_rendering_contract')parity=traceRows(output,'DECLARATION_PARITY ');if(name==='declaration_ingestion_contract'){outcomes=traceRows(output,'DECLARATION_INGESTION_OUTCOME ');checkpoints=traceRows(output,'DECLARATION_INGESTION_CHECKPOINTS ');}}
  results[s.label]=completeVariant(suites,parity,outcomes,checkpoints,inputs.tests);log(`${s.label}-results.json`,{...results[s.label],binaries});assert.deepEqual(dependencies(s,'after'),s.dependencies,'windows-contract.post_closure_drift');
 }
 compareVariants(results);check();log('summary.json',{state:'succeeded',platform:'windows',target,perVariantTests:31,parityCount:450,constructorOutcomes:6,constructorCheckpointCountsCompared:false,results,resources:'NOT_RUN; separate Job cap/probe review required',productHardDeadline:'NOT_RUN',historicalLinuxOwnerNotAdopted:inputs.priorLinuxOwner});terminal.state='succeeded';
}catch(error){if(terminal.handlesReaped)terminal.state='failed';log('failure.json',{error:error.message,stack:error.stack,cachePrepared:cacheReady,variantsStaged:stages.map(s=>s.label)});process.exitCode=1;}
finally{
 try{if(terminal.handlesReaped&&finalDependencyCheck)finalDependencyCheck();else if(!terminal.handlesReaped)log('post-dependencies-skipped.json',{reason:'original owned handles not verified reaped'});payloadCheck();assert.deepEqual(toolIdentities(),tools);for(const s of stages)assert.deepEqual(inventory(s.cwd),s.expected);if(originals){assert.deepEqual(inventory(join(job.directory,'original')),inputs.original);assert.deepEqual(inventory(join(job.directory,'product')),inputs.product);}log('post-source-tools.json',{unchanged:true,originalFiles:originals?848:null,variants:stages.map(s=>s.label)});}catch(error){terminal.state=terminal.handlesReaped?'failed':'interrupted';log('post-source-tools-failure.json',{error:error.message});process.exitCode=1;}
 job.seal(terminal);if(terminal.handlesReaped){if(terminal.state==='succeeded')job.cleanup();channel?.release();}
}
console.log(JSON.stringify({state:terminal.state,handlesReaped:terminal.handlesReaped,receipt:job.receipt,evidence:job.evidence}));
