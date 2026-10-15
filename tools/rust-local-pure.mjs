// Bounded local pure-source regression only. Never substitutes for Windows CI.
import {closeSync,copyFileSync,existsSync,lstatSync,mkdirSync,openSync,readFileSync,readSync,realpathSync,writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {dirname,join,relative,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {acquireChannel,beginScratch,contained,sha256} from './rust-disk.mjs';
import {localOwnedSpawn} from './rust-local-owned-process.mjs';

export const sourcePaths=Object.freeze([
  'crates/lycheedev-core/src/raw_json.rs',
  'crates/lycheedev-core/src/raw_json/tests.rs',
  'crates/lycheedev-core/src/mailbox.rs',
  'crates/lycheedev-core/src/mailbox/code_bytes.rs',
  'crates/lycheedev-core/src/mailbox/code_bytes_tests.rs',
  'crates/lycheedev-source/src/native/original_lines.rs',
  'crates/lycheedev-source/src/process/stderr_observation.rs',
]);
export const bindingContractPaths=Object.freeze([
  'crates/lycheedev-binding-contracts/Cargo.toml',
  'crates/lycheedev-binding-contracts/src/lib.rs',
  'crates/lycheedev-binding-contracts/tests/contracts.rs',
]);
// Fixed reviewed production-to-harness mapping; arbitrary paths are never accepted.
export function harnessSourcePath(name) {
  if (!sourcePaths.includes(name)) throw new Error('local.unknown_source');
  if (name === 'crates/lycheedev-source/src/process/stderr_observation.rs') return 'stderr_observation.rs';
  return name === 'crates/lycheedev-source/src/native/original_lines.rs'
    ? 'original_lines.rs' : name.slice('crates/lycheedev-core/src/'.length);
}
export const harnessPaths=Object.freeze(['tests/rust-local-pure/Cargo.toml','tests/rust-local-pure/src/lib.rs','tests/rust-local-pure/check_cache.py']);
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
function hashFile(path) {const stat=lstatSync(path);if(!stat.isFile()||stat.isSymbolicLink()||stat.size>64<<20)throw new Error('local.source_budget');return sha256(readFileSync(path));}
function hashTool(path) {
  const stat=lstatSync(path);if(!stat.isFile()||stat.isSymbolicLink()||stat.size>512<<20)throw new Error('local.tool_budget');
  const file=openSync(path,'r'),hash=createHash('sha256'),buffer=Buffer.alloc(65536);let bytes=0;
  try {for(;;){const n=readSync(file,buffer,0,buffer.length,null);if(!n)break;bytes+=n;if(bytes>512<<20)throw new Error('local.tool_budget');hash.update(buffer.subarray(0,n));}}
  finally {closeSync(file);}
  if(bytes!==stat.size)throw new Error('local.tool_changed_while_hashing');return hash.digest('hex');
}
function toolIdentities(toolchain,python) {
  const base=realpathSync(join(toolchain,'..'));
  const paths=new Set([join(toolchain,'cargo'),python,realpathSync('/usr/bin/cc'),realpathSync('/usr/bin/ld'),join(base,'lib/rustlib/multirust-channel-manifest.toml')]);
  for(const name of ['rustc','rust-std']) {
    const manifest=join(base,`lib/rustlib/manifest-${name}-x86_64-unknown-linux-gnu`);paths.add(manifest);
    const bytes=readFileSync(manifest);if(bytes.length>128<<10)throw new Error('local.tool_manifest_budget');
    for(const line of bytes.toString().trim().split('\n')) {
      if(!line.startsWith('file:'))throw new Error('local.tool_manifest_shape');
      paths.add(contained(base,join(base,line.slice(5))));
    }
  }
  if(paths.size>512)throw new Error('local.tool_inventory_budget');
  return Object.fromEntries([...paths].sort().map(path=>[path,hashTool(path)]));
}
export function snapshotInputs(repository,paths) {
  return Object.fromEntries(paths.map(path=>[path,hashFile(contained(repository,join(repository,path)))]));
}
export function cargoConfigurations(cwd,home) {
  const paths=new Set([join(home,'config'),join(home,'config.toml')]);
  for(let directory=cwd;;directory=dirname(directory)) {
    paths.add(join(directory,'.cargo/config'));paths.add(join(directory,'.cargo/config.toml'));
    if(directory===dirname(directory))break;
  }
  return Object.fromEntries([...paths].sort().map(path=>{
    try {const stat=lstatSync(path);if(stat.size>65536)throw new Error('local.cargo_config_budget');return [path,hashFile(path)];}
    catch(error){if(error.code==='ENOENT')return [path,null];throw error;}
  }));
}
function copyInputs(repository,destination,hashes) {
  for(const [name,hash] of Object.entries(hashes)) {
    const target=join(destination,name);mkdirSync(dirname(target),{recursive:true});copyFileSync(join(repository,name),target);
    if(hashFile(target)!==hash)throw new Error('local.source_changed_during_snapshot');
  }
  for(const name of sourcePaths) {
    const target=join(destination,'tests/rust-local-pure/src',harnessSourcePath(name));
    mkdirSync(dirname(target),{recursive:true});copyFileSync(join(repository,name),target);
    if(hashFile(target)!==hashes[name])throw new Error('local.source_changed_during_snapshot');
  }
}
function checkStage(stage,hashes,initializeLock) {
  for(const [name,hash]of Object.entries(hashes)) {
    if(initializeLock&&name==='tests/rust-local-pure/Cargo.lock')continue;
    if(hashFile(join(stage,name))!==hash)throw new Error('local.staged_source_changed');
  }
  for(const name of sourcePaths)if(hashFile(join(stage,'tests/rust-local-pure/src',harnessSourcePath(name)))!==hashes[name])throw new Error('local.compiled_source_changed');
}
export function verifyTestResults(stdout,expected) {
  if(!expected.length||new Set(expected).size!==expected.length)throw new Error('local.invalid_test_expectations');
  const summaries=[...stdout.matchAll(/^test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;/gm)];
  if(summaries.length!==1||Number(summaries[0][1])!==expected.length)throw new Error('local.test_count_mismatch');
  for(const name of expected)if(stdout.split('\n').filter(line=>/^test /.test(line)&&(line===`test ${name} ... ok`||line.endsWith(`::${name} ... ok`))).length!==1)throw new Error(`local.test_not_passed:${name}`);
  return {passed:expected.length,failed:0,ignored:0,filtered:0,names:expected};
}
// Staged review may share only its registered parent repository's build channels.
// No CLI path option and no unregistered source/target directory are admitted.
export function checkedAdmissionRepository(repository,admissionRepository=repository) {
  repository=realpathSync(repository);admissionRepository=realpathSync(admissionRepository);
  if(repository===admissionRepository)return admissionRepository;
  const match=/^\.tmp\/rust-scratch\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\/stage$/.exec(relative(admissionRepository,repository));
  if(!match)throw new Error('local.unregistered_stage');
  contained(admissionRepository,repository);
  const receipt=contained(admissionRepository,join(admissionRepository,'.tmp/rust-disk/evidence',match[1],'receipt.json'));
  const stat=lstatSync(receipt);if(!stat.isFile()||stat.size>1<<20)throw new Error('local.stage_receipt_budget');
  const owner=JSON.parse(readFileSync(receipt));
  if(owner.schema!=='lycheedev.disk-owner.v1'||owner.id!==match[1]||owner.directory!==dirname(repository)||owner.owner!=='local-owned-contract-runner-stage'||owner.retention!=='preserve'||owner.state!=='succeeded'||owner.handlesReaped!==true)throw new Error('local.unregistered_stage');
  return admissionRepository;
}
export async function runLocalPure({initializeLock=false,bindingContracts=false,repository=root,admissionRepository=repository,rustBin=process.env.LYCHEEDEV_LOCAL_RUST_BIN,cargoHome=process.env.CARGO_HOME,python=process.env.LYCHEEDEV_LOCAL_PYTHON||'/usr/bin/python3'}={}) {
  repository=realpathSync(repository);
  admissionRepository=checkedAdmissionRepository(repository,admissionRepository);
  if(initializeLock&&bindingContracts)throw new Error('local.conflicting_modes');
  const sibling=join(dirname(admissionRepository),'rust-dev');
  cargoHome=realpathSync(cargoHome||join(sibling,'cargo'));
  const toolchain=realpathSync(rustBin||join(process.env.RUSTUP_HOME||join(sibling,'rustup'),'toolchains/stable-x86_64-unknown-linux-gnu/bin'));
  python=realpathSync(python);
  const tools=toolIdentities(toolchain,python);
  const lock='tests/rust-local-pure/Cargo.lock';
  if(!initializeLock&&!existsSync(join(repository,lock)))throw new Error('local.lock_missing');
  const paths=[...sourcePaths,...bindingContractPaths,...harnessPaths,...(existsSync(join(repository,lock))?[lock]:[])];
  const inputs=snapshotInputs(repository,paths);
  const runnerHashes=snapshotInputs(repository,['tools/rust-local-pure.mjs','tools/rust-local-owned-process.mjs','tools/rust-disk.mjs']);
  const args=initializeLock?['generate-lockfile','--offline']:['test','--offline','--locked','--profile','symbols','-j','1',...(bindingContracts?['--test','binding_contracts']:['--lib']),'--','--test-threads=1'];
  const owner=bindingContracts?'local-binding-contract-regressions':'local-pure-regressions';
  const job=beginScratch(admissionRepository,owner,{command:['cargo',...args],inputs:{sources:inputs,runners:runnerHashes,tools},admissionOptions:{requiredBytes:512<<20,cacheReserveBytes:512<<20,scratchReserveBytes:16<<20}});
  let channel,terminal={state:'failed',handlesReaped:true,logs:[]};
  try {
    channel=acquireChannel(admissionRepository,'symbols',owner);
    mkdirSync(channel.target,{recursive:true});
    const stage=join(job.directory,'source');mkdirSync(stage);copyInputs(repository,stage,inputs);
    const cwd=join(stage,'tests/rust-local-pure'),home=join(job.directory,'home'),tmp=join(job.directory,'tmp');mkdirSync(home);mkdirSync(tmp);
    const env={PATH:`${toolchain}:/usr/bin:/bin`,HOME:home,TMPDIR:tmp,TMP:tmp,TEMP:tmp,CARGO_HOME:cargoHome,RUSTC:join(toolchain,'rustc'),RUSTDOC:join(toolchain,'rustdoc'),CARGO_TARGET_DIR:channel.target,CARGO_NET_OFFLINE:'true',CARGO_INCREMENTAL:'0',LANG:'C.UTF-8'};
    const bindingInputs=Object.fromEntries(bindingContractPaths.map(name=>[name,inputs[name]]));
    const bindingIdentity=join(cwd,'binding-contracts-inputs.json');writeFileSync(bindingIdentity,JSON.stringify(bindingInputs,null,2)+'\n');
    const configurations=cargoConfigurations(cwd,cargoHome);
    const configurationPath=join(job.evidence,'cargo-configurations.json');writeFileSync(configurationPath,JSON.stringify(configurations,null,2)+'\n');terminal.logs.push(configurationPath);
    async function checkDependencies() {
      terminal.handlesReaped=false;terminal.state='interrupted';
      const result=await localOwnedSpawn(python,[join(cwd,'check_cache.py'),join(cwd,'Cargo.lock'),cargoHome,bindingIdentity],{cwd,env,evidence:job.evidence,timeout:60000});
      terminal.logs.push(...result.logs);terminal.handlesReaped=result.handlesReaped;
      if(result.error||result.status!==0)throw new Error(`local.cache_verification_failed:${result.error?.message||result.stderr.toString()}`);
      return JSON.parse(result.stdout.toString());
    }
    let dependencies;if(!initializeLock)dependencies=await checkDependencies();
    const priorLogs=terminal.logs;
    terminal={state:'interrupted',handlesReaped:false,logs:[]};
    const result=await localOwnedSpawn(join(toolchain,'cargo'),args,{cwd,env,evidence:job.evidence,writable:[home,tmp,channel.target,cargoHome,...(initializeLock?[cwd]:[])],timeout:300000});
    terminal={state:result.status===0&&!result.error?'succeeded':'failed',handlesReaped:result.handlesReaped,logs:[...priorLogs,...result.logs]};
    if(!result.handlesReaped)terminal.state='interrupted';
    if(result.error||result.status!==0)throw new Error(`local.cargo_failed:${result.error?.message||result.status}`);
    if(JSON.stringify(snapshotInputs(repository,paths))!==JSON.stringify(inputs))throw new Error('local.source_changed_during_test');
    if(JSON.stringify(snapshotInputs(repository,Object.keys(runnerHashes)))!==JSON.stringify(runnerHashes))throw new Error('local.runner_changed_during_test');
    checkStage(stage,inputs,initializeLock);
    if(JSON.stringify(JSON.parse(readFileSync(bindingIdentity)))!==JSON.stringify(bindingInputs))throw new Error('local.binding_identity_changed');
    if(JSON.stringify(cargoConfigurations(cwd,cargoHome))!==JSON.stringify(configurations))throw new Error('local.cargo_configuration_changed');
    if(JSON.stringify(toolIdentities(toolchain,python))!==JSON.stringify(tools))throw new Error('local.tool_changed_during_test');
    const afterDependencies=await checkDependencies();
    if(dependencies&&JSON.stringify(dependencies)!==JSON.stringify(afterDependencies))throw new Error('local.dependencies_changed_during_test');
    dependencies=afterDependencies;terminal.state='succeeded';
    let tests;
    if(initializeLock)copyFileSync(join(cwd,'Cargo.lock'),join(repository,lock));
    else {
      const expected=(bindingContracts?['crates/lycheedev-binding-contracts/tests/contracts.rs']:sourcePaths).flatMap(name=>[...readFileSync(join(stage,name),'utf8').matchAll(/#\[test\]\s*fn (\w+)\(/g)].map(match=>match[1]));
      tests=verifyTestResults(result.stdout.toString(),expected);
    }
    const summary={schema:'lycheedev.local-pure-result.v1',state:'succeeded',operation:initializeLock?'lock-resolution':'test',scope:bindingContracts?'Exact owned binding-contracts crate and its unmodified integration tests; not adapter, engine, Windows or whole-workspace acceptance':'Exact production pure modules only; not Windows or whole-workspace acceptance',suite:bindingContracts?'binding-contracts':'pure-modules',profile:'symbols',channel:channel.target,inputs,runnerHashes,tools,configurations,dependencies,lockSHA256:hashFile(join(cwd,'Cargo.lock')),tests,processProof:result.proof,stdout:result.stdout.toString(),stderr:result.stderr.toString()};
    const path=join(job.evidence,'summary.json');writeFileSync(path,JSON.stringify(summary,null,2)+'\n');terminal.logs.push(path);
    return {...summary,evidence:job.receipt};
  } catch(error) {
    if(terminal.handlesReaped)terminal.state='failed';
    const path=join(job.evidence,'failure.json');writeFileSync(path,JSON.stringify({error:error.message,inputs,runnerHashes},null,2)+'\n');terminal.logs.push(path);error.evidence=job.receipt;throw error;
  } finally {
    job.seal(terminal);
    // Preserve failed scratch and all receipts. Release only this invocation's
    // own lease after verified terminal process/pipe cleanup.
    if(terminal.handlesReaped){if(terminal.state==='succeeded')job.cleanup();channel?.release();}
  }
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  const args=process.argv.slice(2);if(args.length>1||(args.length&&!['--initialize-lock','--binding-contracts'].includes(args[0])))throw new Error('usage: node tools/rust-local-pure.mjs [--initialize-lock | --binding-contracts]');
  try {console.log(JSON.stringify(await runLocalPure({initializeLock:args[0]==='--initialize-lock',bindingContracts:args[0]==='--binding-contracts'}),null,2));}
  catch(error){console.error(JSON.stringify({error:error.message,evidence:error.evidence}));process.exitCode=1;}
}
