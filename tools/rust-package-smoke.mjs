// Isolated, unpublished candidate smoke. This does not establish full business parity or release readiness.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readdirSync, readFileSync, writeFileSync, copyFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { join, resolve, dirname } from 'node:path';
import { stageLuac } from './luac.mjs';
import { stageLuaLS } from './luals.mjs';
import { buildRust,verifyArtifact } from './rust-build.mjs';
import { beginScratch,contained } from './rust-disk.mjs';
import {ownedSpawnSync} from './rust-owned-process.mjs';
const root=resolve(process.cwd()),[outArg,luacArg,homeArg,pinArg,lualsArg]=process.argv.slice(2);if(!outArg||!luacArg)throw new Error('usage: node tools/rust-package-smoke.mjs <new-directory> <verified-luac-build> [fixed-home] [fixed-pin] [verified-LuaLS-zip]');
const profileIndex=process.argv.indexOf('--profile'),profile=profileIndex>=0?process.argv[profileIndex+1]:'debug';if(!['debug','release'].includes(profile))throw new Error('candidate.invalid_profile');
const out=contained(root,resolve(outArg));
const job=beginScratch(root,'rust-package-smoke',{command:process.argv}),logs=[];
let state='failed',handlesReaped=true,commandSequence=0;
try{
mkdirSync(out);const pkgRoot=join(out,'package');mkdirSync(pkgRoot);const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
function run(command,args,cwd=root,log){
  const index=commandSequence++,metadata=join(job.evidence,`command-${index}.json`),stdout=join(job.evidence,`stdout-${index}.bin`),stderr=join(job.evidence,`stderr-${index}.bin`);
  handlesReaped=false;
  const result=ownedSpawnSync(command,args,{cwd,env:{...process.env,npm_config_cache:join(job.directory,'npm-cache')},evidence:job.evidence,timeout:300000});
  handlesReaped=result.handlesReaped;logs.push(...result.logs);
  writeFileSync(metadata,JSON.stringify({command,args,cwd,exit:result.status,signal:result.signal,error:result.error?.message})+'\n');writeFileSync(stdout,result.stdout||Buffer.alloc(0));writeFileSync(stderr,result.stderr||Buffer.alloc(0));logs.push(metadata,stdout,stderr);
  if(log)writeFileSync(log,Buffer.concat([result.stdout||Buffer.alloc(0),result.stderr||Buffer.alloc(0)]));
  if(!handlesReaped||result.error||result.status!==0)throw new Error(`candidate.failed: ${command}: ${result.error||result.status}\n${result.stdout}\n${result.stderr}`);return result.stdout.toString();
}
function npm(args,cwd){return run(process.execPath,[process.env.npm_execpath||join(dirname(process.execPath),'node_modules/npm/bin/npm-cli.js'),...args],cwd);}
// Reuse only an explicit sealed artifact. Ordinary mutable target output is not
// an identity. New builds use one of two quota-controlled project channels.
const artifactIndex=process.argv.indexOf('--artifact');
const artifact=artifactIndex>=0?verifyArtifact(root,process.argv[artifactIndex+1]):buildRust(root,join(out,'native-artifact'),{profile:profile==='debug'?'dev':'release',oneShot:true,owner:'rust-package-smoke'});
if((profile==='release'&&!['release','symbols'].includes(artifact.profile))||(profile==='debug'&&!['dev','debug'].includes(artifact.profile)))throw new Error('candidate.artifact_profile_mismatch');
const pkg=JSON.parse(readFileSync(join(root,'packages/npm/lycheedev/package.json')));pkg.bin={lycheedev:'bin/lycheedev.mjs'};writeFileSync(join(pkgRoot,'package.json'),JSON.stringify(pkg,null,2)+'\n');
for(const name of ['README.md','LICENSE','THIRD_PARTY_NOTICES'])copyFileSync(join(root,'packages/npm/lycheedev',name),join(pkgRoot,name));
const closureRoot=join(out,'license-closure');run(process.execPath,[join(root,'tools/rust-dependencies.mjs'),closureRoot]);
const closure=JSON.parse(readFileSync(join(closureRoot,'dependency-closure.json')));
if(closure.cargoLockSHA256!==hash(readFileSync(join(root,'Cargo.lock'))))throw new Error('candidate.stale_dependency_closure');
writeFileSync(join(pkgRoot,'THIRD_PARTY_NOTICES'),readFileSync(join(pkgRoot,'THIRD_PARTY_NOTICES'),'utf8')+'\n\nRust dependency closure\n\n'+readFileSync(join(closureRoot,'THIRD_PARTY_NOTICES'),'utf8')+'\n\nLua 5.1.5 compiler/runtime\n\n'+readFileSync(join(resolve(luacArg),'LICENSE'),'utf8'));
mkdirSync(join(pkgRoot,'bin'));copyFileSync(join(root,'packages/npm/lycheedev/bin/lycheedev-rust.mjs'),join(pkgRoot,'bin/lycheedev.mjs'));
const resources=[];function copyTree(source,prefix){for(const entry of readdirSync(source,{withFileTypes:true})){if(entry.isSymbolicLink())throw new Error('candidate.symlink');const src=join(source,entry.name),path=`${prefix}/${entry.name}`;if(entry.isDirectory())copyTree(src,path);else{const bytes=readFileSync(src);mkdirSync(dirname(join(pkgRoot,'payload',path)),{recursive:true});writeFileSync(join(pkgRoot,'payload',path),bytes);resources.push({path,bytes:bytes.length,sha256:hash(bytes)});}}}
copyTree(join(root,'addon'),'addon');copyTree(join(root,'skills/lycheedev'),'skill');stageLuac(pkgRoot,resources,resolve(luacArg));
if(lualsArg){await stageLuaLS(pkgRoot,resources,{archivePath:resolve(lualsArg)});const licenses=resources.filter(resource=>resource.path.startsWith('tool/luals/')&&/(^|\/)(license|copying|notice)([-_.]|$)/i.test(resource.path));writeFileSync(join(pkgRoot,'THIRD_PARTY_NOTICES'),readFileSync(join(pkgRoot,'THIRD_PARTY_NOTICES'),'utf8')+'\n\nPinned LuaLS 3.19.1 runtime\n\n'+licenses.map(resource=>`${resource.path}\n${readFileSync(join(pkgRoot,'payload',resource.path),'utf8')}`).join('\n\n'));}
const binary=readFileSync(artifact.sourceBinary);if(hash(binary)!==artifact.binarySHA256)throw new Error('candidate.artifact_changed');mkdirSync(join(pkgRoot,'native/windows-amd64'),{recursive:true});writeFileSync(join(pkgRoot,'native/windows-amd64/lycheedev.exe'),binary);
const commit=run('git',['rev-parse','HEAD']).trim();const release={schema:'lycheedev.release.v1',version:pkg.version,commit,runtime:'rust',npmChannel:JSON.parse(readFileSync(join(root,'release/npm-channel.json'))),binaries:{'windows-amd64':{path:'native/windows-amd64/lycheedev.exe',bytes:binary.length,sha256:hash(binary)}},resources};writeFileSync(join(pkgRoot,'release.json'),JSON.stringify(release,null,2)+'\n');
const packed=JSON.parse(npm(['pack','--ignore-scripts','--json','--pack-destination',out],pkgRoot))[0];const installation=join(out,'installed');mkdirSync(installation);npm(['install','--prefix',installation,'--ignore-scripts','--no-audit','--no-fund','--offline',join(out,packed.filename)],out);
const installed=join(installation,'node_modules/lycheedev');const version=JSON.parse(run(process.execPath,[join(installed,'bin/lycheedev.mjs'),'version','--format','json'],out));if(version.result.platform!=='windows/amd64'||!version.result.rust)throw new Error('candidate.not_rust');
const payload=join(installed,'payload/tool/luac');run(join(payload,'luac.exe'),['-v'],out);writeFileSync(join(out,'valid.lua'),'local value = 1\nreturn value\n');run(join(payload,'luac.exe'),['-p',join(out,'valid.lua')],out);
const addon=join(out,'test-addon');mkdirSync(addon);writeFileSync(join(addon,'Test.toc'),'## Interface: 120100\nGood.lua\n');writeFileSync(join(addon,'Good.lua'),'local value = 1\nreturn value\n');
let validation={state:'not_run'};if(homeArg&&pinArg){const response=JSON.parse(run(process.execPath,[join(installed,'bin/lycheedev.mjs'),'source','validate','--home',resolve(homeArg),'--snapshot',pinArg,'--path',addon,'--toc','Test.toc','--format','json'],out));writeFileSync(join(out,'source-validate.json'),JSON.stringify(response,null,2));if(!response.ok||!response.result.valid||!response.result.syntaxCompilation?.passed||response.result.syntaxCompilation.compilerSHA256!==hash(readFileSync(join(payload,'luac.exe'))))throw new Error('candidate.source_syntax_validation_failed');validation={state:'passed',compilerSHA256:response.result.syntaxCompilation.compilerSHA256,checkedLua:response.result.syntaxCompilation.checkedLua,captures:response.captures.map(c=>c.id)};}
const report={schema:'lycheedev.rust-npm-smoke.v1',state:'partial_passed',profile,ignoreScripts:true,offlineInstall:true,runtimeDependencies:pkg.dependencies||{},platform:version.result.platform,binarySHA256:hash(binary),compilerSHA256:hash(readFileSync(join(payload,'luac.exe'))),tgz:join(out,packed.filename),tgzSHA256:hash(readFileSync(join(out,packed.filename))),version,validation,semanticRuntime:{payload:lualsArg?'complete_pinned_inventory':'not_included',publicValidation:'not_run'},limitations:[`${profile} unpublished candidate only`,'full 89-command parity incomplete',lualsArg?'full LuaLS payload included; public semantic profile still needs independent validation':'semantic LuaLS payload not included in this syntax smoke','official corresponding-source rebuild and publication gates not run']};writeFileSync(join(out,'report.json'),JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report));
logs.push(join(out,'report.json'));state='succeeded';
}catch(error){
  if(error.buildEvidence){const build=JSON.parse(readFileSync(error.buildEvidence));if(!build.handlesReaped)handlesReaped=false;}
  const failure=join(job.evidence,'failure.json');writeFileSync(failure,JSON.stringify({error:error.message,buildEvidence:error.buildEvidence})+'\n');logs.push(failure);throw error;
}finally{
  job.seal({state:handlesReaped?state:'interrupted',handlesReaped,logs});
  if(handlesReaped)job.cleanup();
}
