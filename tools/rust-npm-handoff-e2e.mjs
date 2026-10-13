// Isolated local registry and global-prefix fixture. Never publishes or touches the user's prefix.
import {spawn,spawnSync} from 'node:child_process';import {createServer} from 'node:http';import {mkdirSync,readFileSync,writeFileSync,cpSync,readdirSync,existsSync} from 'node:fs';import {resolve,join,dirname} from 'node:path';import {createHash} from 'node:crypto';
import {buildRust} from './rust-build.mjs';
import {beginScratch,contained} from './rust-disk.mjs';
import {ownedSpawn} from './rust-owned-process.mjs';
const root=resolve(process.cwd()),out=contained(root,resolve(process.argv[2]||'.tmp/rust-npm-handoff-e2e'));
const job=beginScratch(root,'rust-npm-handoff-fixture',{command:process.argv}),logs=[];
let jobState='failed',handlesReaped=true,commandSequence=0,fixtureServer;
try{
mkdirSync(out);const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const artifactFlag=process.argv.indexOf('--artifacts'),frozen=artifactFlag>=0?resolve(process.argv[artifactFlag+1]):null;
const faultMode=process.argv.includes('--fault-prefix')?'prefix':process.argv.includes('--fault-target')?'target':null;
const rejectGo=process.argv.includes('--reject-go-first');
const npm=process.env.npm_execpath||join(dirname(process.execPath),'node_modules/npm/bin/npm-cli.js');if(!existsSync(npm))throw Error('Explicit local npm runtime unavailable');
const source=join(out,'source');if(!frozen){mkdirSync(source);for(const name of ['Cargo.toml','Cargo.lock','crates','release','.cargo'])cpSync(join(root,name),join(source,name),{recursive:true});}
const buildEnvironment={LYCHEEDEV_BUILD_COMMIT:'9d3d3598b8bac7efabf0b937df30508075eaa841',LYCHEEDEV_BUILD_DIRTY:'true'};
const env={...process.env,...buildEnvironment,npm_config_cache:join(job.directory,'npm-cache')};
async function run(file,args,{cwd=root,processEnv=env,log,limit=600000}={}){
 handlesReaped=false;
 const result=await ownedSpawn(file,args,{cwd,env:processEnv,evidence:job.evidence,timeout:limit});
 handlesReaped=result.handlesReaped;logs.push(...result.logs);
 if(log)writeFileSync(log,Buffer.concat([result.stdout,result.stderr]));
 if(result.error||!handlesReaped)throw new Error(`fixture.process_unresolved:${result.error?.message||'tree_reap_unverified'}`);
 return {code:result.status,stdout:result.stdout.toString(),stderr:result.stderr.toString()};
}
const archives=new Map();const artifacts=[];
if(frozen){for(const artifact of JSON.parse(readFileSync(join(frozen,'artifacts.json')))){const filename=`lycheedev-${artifact.version}.tgz`,raw=readFileSync(join(frozen,filename));if(sha(raw)!==artifact.tarballSHA256)throw Error('Frozen fixture tarball hash differs');writeFileSync(join(out,filename),raw);archives.set(artifact.version,{record:{name:'lycheedev',version:artifact.version,filename,integrity:'sha512-'+createHash('sha512').update(raw).digest('base64'),shasum:createHash('sha1').update(raw).digest('hex')},raw});artifacts.push(artifact);}writeFileSync(join(out,'artifacts.json'),JSON.stringify(artifacts,null,2));}
else{
for(const version of ['3.1.2','3.1.3','3.1.4']){
 const manifest=readFileSync(join(source,'Cargo.toml'),'utf8').replace(/version = "3\.1\.[234]"/,'version = "'+version+'"');writeFileSync(join(source,'Cargo.toml'),manifest);
 const lock=readFileSync(join(source,'Cargo.lock'),'utf8').replace(/(name = "lycheedev-[^"]+"\r?\nversion = ")3\.1\.[234](")/g,(_match,prefix,suffix)=>prefix+version+suffix);writeFileSync(join(source,'Cargo.lock'),lock);
 const compiled=buildRust(root,join(out,`native-${version}`),{sourceRoot:source,environment:buildEnvironment,oneShot:true,owner:'rust-npm-handoff-fixture'});
 const binary=readFileSync(compiled.sourceBinary);if(sha(binary)!==compiled.binarySHA256)throw Error('Fixture sealed native artifact changed');const packageRoot=join(out,`package-${version}`);mkdirSync(packageRoot);
 const pkg=JSON.parse(readFileSync(join(root,'packages/npm/lycheedev/package.json')));pkg.version=version;pkg.bin={lycheedev:'bin/lycheedev.mjs'};writeFileSync(join(packageRoot,'package.json'),JSON.stringify(pkg));mkdirSync(join(packageRoot,'bin'));cpSync(join(root,'packages/npm/lycheedev/bin/lycheedev-rust.mjs'),join(packageRoot,'bin/lycheedev.mjs'));
 mkdirSync(join(packageRoot,'native/windows-amd64'),{recursive:true});writeFileSync(join(packageRoot,'native/windows-amd64/lycheedev.exe'),binary);
 const files={'addon/Lychee Dev.toc':`## Interface: 120100, 50504, 38002, 16001\n## Version: ${version}\nBridge/NativeRuntime.lua\n`,'addon/Bridge/NativeRuntime.lua':'-- isolated fixture; no game loading\n','skill/SKILL.md':'fixture skill\n'};const resources=[];
 for(const [path,text]of Object.entries(files)){mkdirSync(dirname(join(packageRoot,'payload',path)),{recursive:true});writeFileSync(join(packageRoot,'payload',path),text);resources.push({path,bytes:Buffer.byteLength(text),sha256:sha(Buffer.from(text))});}
 writeFileSync(join(packageRoot,'release.json'),JSON.stringify({schema:'lycheedev.release.v1',version,commit:'1'.repeat(40),runtime:'rust',npmChannel:{schema:'lycheedev.npm-channel.v1',version,distTag:'next',preserveLatest:'2.5.1'},binaries:{'windows-amd64':{path:'native/windows-amd64/lycheedev.exe',bytes:binary.length,sha256:sha(binary)}},resources}));
 const packed=await run(process.execPath,[npm,'pack','--ignore-scripts','--json','--pack-destination',out],{cwd:packageRoot});if(packed.code!==0)throw Error(packed.stderr);const record=JSON.parse(packed.stdout)[0];archives.set(version,{record,raw:readFileSync(join(out,record.filename))});artifacts.push({version,binarySHA256:sha(binary),tarballSHA256:sha(archives.get(version).raw)});writeFileSync(join(out,'artifacts.json'),JSON.stringify(artifacts,null,2));
}
}
let selected='3.1.3';const requests=[];const server=fixtureServer=createServer((request,response)=>{requests.push(request.url);if(request.url==='/lycheedev'){const versions={};for(const[version,{record}]of archives)versions[version]={name:'lycheedev',version,dist:{tarball:`http://127.0.0.1:${server.address().port}/lycheedev/-/${record.filename}`,integrity:record.integrity,shasum:record.shasum}};response.setHeader('Content-Type','application/json');response.end(JSON.stringify({name:'lycheedev','dist-tags':{next:selected,latest:'2.5.1'},versions}));}else{const file=request.url.split('/').at(-1),entry=[...archives.values()].find(({record})=>record.filename===file);if(!entry){response.statusCode=404;response.end();}else response.end(entry.raw);}});await new Promise(ok=>server.listen(0,'127.0.0.1',ok));
const prefix=join(out,'isolated-prefix');mkdirSync(prefix);const registry=`http://127.0.0.1:${server.address().port}/`;const processEnv={...env,npm_config_registry:registry};const installed=await run(process.execPath,[npm,'install','--global','--prefix',prefix,'--ignore-scripts','--no-audit','--no-fund',join(out,archives.get('3.1.2').record.filename)],{processEnv,log:join(out,'initial-install.log')});if(installed.code!==0)throw Error(installed.stderr);
const targets=join(out,'targets.json');writeFileSync(targets,'[]');const home=join(out,'relative-workspace');const cases=[];const faults=[];
const launcher=join(prefix,'node_modules/lycheedev/bin/lycheedev.mjs');
const target=join(out,'managed-skill/lycheedev');
if(rejectGo){
 const goRoot=join(out,'go-package'),packedRoot=join(out,'go-archive');mkdirSync(goRoot);mkdirSync(packedRoot);
 const initial=JSON.parse(readFileSync(join(prefix,'node_modules/lycheedev/package.json')));writeFileSync(join(goRoot,'package.json'),JSON.stringify(initial));
 mkdirSync(join(goRoot,'bin'));writeFileSync(join(goRoot,'bin/lycheedev.mjs'),'throw new Error("Rejected Go fixture must never launch");\n');
 const goBytes=readFileSync(join(root,'.tmp/lycheedev-oracle.exe'));mkdirSync(join(goRoot,'native/windows-amd64'),{recursive:true});writeFileSync(join(goRoot,'native/windows-amd64/lycheedev.exe'),goBytes);
 const goResources=[];for(const[path,text]of Object.entries({'addon/Lychee Dev.toc':'## Version: 3.1.2\n','skill/SKILL.md':'frozen Go fixture\n'})){mkdirSync(dirname(join(goRoot,'payload',path)),{recursive:true});writeFileSync(join(goRoot,'payload',path),text);goResources.push({path,bytes:Buffer.byteLength(text),sha256:sha(Buffer.from(text))});}
 writeFileSync(join(goRoot,'release.json'),JSON.stringify({schema:'lycheedev.release.v1',version:'3.1.2',commit:'9d3d3598b8bac7efabf0b937df30508075eaa841',binaries:{'windows-amd64':{path:'native/windows-amd64/lycheedev.exe',bytes:goBytes.length,sha256:sha(goBytes)}},resources:goResources}));
 const packed=await run(process.execPath,[npm,'pack','--ignore-scripts','--json','--pack-destination',packedRoot],{cwd:goRoot});if(packed.code!==0)throw Error(packed.stderr);
 const record=JSON.parse(packed.stdout)[0],raw=readFileSync(join(packedRoot,record.filename));archives.set('3.1.2',{record:{...record,filename:'current-go-next.tgz'},raw});selected='3.1.2';
 const native=join(prefix,'node_modules/lycheedev/native/windows-amd64/lycheedev.exe'),oldSHA=sha(readFileSync(native)),oldManifest=sha(readFileSync(join(prefix,'node_modules/lycheedev/release.json')));
 const rejected=await run(process.execPath,[launcher,'update','--file',targets,'--home',home,'--format','json'],{processEnv,log:join(out,'go-next-rejected.log')});
 let envelope;try{envelope=JSON.parse(rejected.stdout);}catch{}
 const journal=JSON.parse(readFileSync(join(prefix,'.lycheedev-update/transaction.json')));
 const proof={kind:'go-next-rejection',exit:rejected.code,code:envelope?.error?.code,phase:journal.phase,goBinarySHA256:sha(goBytes),goBinaryExecuted:false,prefixBinaryPreserved:sha(readFileSync(native))===oldSHA,prefixManifestPreserved:sha(readFileSync(join(prefix,'node_modules/lycheedev/release.json')))===oldManifest};faults.push(proof);
 if(proof.exit!==3||proof.code!=='update.runtime_incompatible'||proof.phase!=='rejected'||!proof.prefixBinaryPreserved||!proof.prefixManifestPreserved)throw Error('Go next was not safely rejected with terminal receipt:'+JSON.stringify(proof));
}
if(faultMode==='target'){
 mkdirSync(dirname(target),{recursive:true});
 const install=await run(process.execPath,[launcher,'skill','install','--release',join(prefix,'node_modules/lycheedev'),'--path',target,'--format','json'],{cwd:out,processEnv,log:join(out,'initial-target-install.log')});
 if(install.code!==0)throw Error('Managed fixture initial install failed:'+install.stdout+install.stderr);
 writeFileSync(targets,JSON.stringify([{component:'skill',path:target}]));
}
try{for(const version of ['3.1.3','3.1.4']){
 if(faultMode==='target'&&version==='3.1.4'){
  const second=join(out,'second-skill/lycheedev');mkdirSync(dirname(second),{recursive:true});
  const added=await run(process.execPath,[launcher,'skill','install','--release',join(prefix,'node_modules/lycheedev'),'--path',second,'--format','json'],{cwd:out,processEnv,log:join(out,'second-target-install.log')});
  if(added.code!==0)throw Error('New lifecycle target initialization failed:'+added.stdout+added.stderr);
  writeFileSync(targets,JSON.stringify([{component:'skill',path:target},{component:'skill',path:second}]));
 }
 selected=version;const args=[launcher,'update','--file',targets,'--home',home,'--format','json'];let result;
 if(faultMode&&version==='3.1.3'){
  const heldPath=faultMode==='target'?join(target,'SKILL.md'):join(prefix,'node_modules/lycheedev/payload/skill/SKILL.md');
  const heldSHA=sha(readFileSync(heldPath));const script=join(out,'deny-owned-file.ps1');
  writeFileSync(script,`$held = [IO.File]::Open($env:FIX_HELD, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)\ntry { & $env:FIX_NODE $env:FIX_LAUNCHER update --file $env:FIX_TARGETS --home $env:FIX_HOME --format json; $result = $LASTEXITCODE } finally { $held.Dispose() }\nexit $result\n`);
  result=await run('powershell.exe',['-NoProfile','-NonInteractive','-File',script],{processEnv:{...processEnv,FIX_HELD:heldPath,FIX_NODE:process.execPath,FIX_LAUNCHER:launcher,FIX_TARGETS:targets,FIX_HOME:home},log:join(out,`denied-${faultMode}.log`)});
  const journalPath=join(prefix,'.lycheedev-update/transaction.json'),journal=JSON.parse(readFileSync(journalPath));
  const fault={kind:faultMode,exit:result.code,phase:journal.phase,heldSHA256:heldSHA,heldFilePreserved:existsSync(heldPath)&&sha(readFileSync(heldPath))===heldSHA};faults.push(fault);
  if(result.code!==5||!fault.heldFilePreserved||(faultMode==='target'&&journal.phase!=='prefix'))throw Error('Expected real deny-write/delete cutpoint was not observed:'+JSON.stringify(fault));
  const helper=join(journal.staged,'native/windows-amd64/lycheedev.exe'),handoff=join(prefix,'.lycheedev-update',`handoff-${journal.nonce}.json`);
  if(sha(readFileSync(helper))!==journal.helperSha256)throw Error('Recovery helper identity differs');
  const helperArgs=['__lycheedev_update_apply',handoff,journal.nonce];
  if(faultMode==='target'){
   const script=join(out,'resume-with-prefix-held.ps1');
   writeFileSync(script,`$held = [IO.File]::Open($env:FIX_HELD, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)\ntry { $arguments = @($env:FIX_ARGS | ConvertFrom-Json); & $env:FIX_BINARY @arguments; $result = $LASTEXITCODE } finally { $held.Dispose() }\nexit $result\n`);
   result=await run('powershell.exe',['-NoProfile','-NonInteractive','-File',script],{processEnv:{...processEnv,FIX_HELD:join(prefix,'node_modules/lycheedev/payload/skill/SKILL.md'),FIX_BINARY:helper,FIX_ARGS:JSON.stringify(helperArgs)},log:join(out,'resume-target-prefix-held.log')});
   fault.resumePrefixProtected=true;
  }else result=await run(helper,helperArgs,{processEnv,log:join(out,`resume-${faultMode}.log`)});
  const completed=readFileSync(journalPath),repeat=await run(helper,['__lycheedev_update_apply',handoff,journal.nonce],{processEnv,log:join(out,'completed-repeat.log')});
  fault.resumeExit=result.code;fault.repeatExit=repeat.code;fault.repeatJournalUnchanged=completed.equals(readFileSync(journalPath));
  if(repeat.code!==0||!fault.repeatJournalUnchanged)throw Error('Completed exact helper retry changed the journal or failed');
 }else result=await run(process.execPath,args,{processEnv,log:join(out,`update-${version}.log`)});
 let envelope;try{envelope=JSON.parse(result.stdout);}catch{}const actual=JSON.parse(readFileSync(join(prefix,'node_modules/lycheedev/package.json'))).version;
 const targetVersion=faultMode==='target'?JSON.parse(readFileSync(join(target,'.lycheedev-install.json'))).version:null;
 cases.push({expected:version,actual,exit:result.code,envelope,targetVersion});if(result.code!==0||actual!==version||!envelope?.ok)break;
}}
finally{await new Promise(ok=>server.close(ok));const report={schema:'lycheedev.rust-npm-handoff-e2e.v1',prefix,registry,artifacts,cases,faults,requests,state:cases.length===2&&cases.every(c=>c.exit===0&&c.actual===c.expected&&c.envelope.ok)?'passed':'failed',scope:`three genuine Rust fixture binaries; local registry and isolated prefix; ${faultMode||'normal'} profile, no publication; not all fault/profile or final release acceptance`};writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({state:report.state,cases:cases.map(({expected,actual,exit})=>({expected,actual,exit})),report:join(out,'report.json')}));if(report.state!=='passed')process.exitCode=1;}
logs.push(join(out,'report.json'));if(process.exitCode!==1)jobState='succeeded';
}catch(error){
 if(error.buildEvidence){const build=JSON.parse(readFileSync(error.buildEvidence));if(!build.handlesReaped)handlesReaped=false;}
 const failure=join(job.evidence,'failure.json');writeFileSync(failure,JSON.stringify({error:error.message,buildEvidence:error.buildEvidence})+'\n');logs.push(failure);throw error;
}finally{
 if(fixtureServer?.listening)await new Promise(ok=>fixtureServer.close(ok));
 job.seal({state:handlesReaped?jobState:'interrupted',handlesReaped,logs});
 if(handlesReaped)job.cleanup();
}
