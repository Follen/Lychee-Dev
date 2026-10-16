// Unpublished corresponding-source gate: sequential fresh homes and caches.
import {spawnSync} from 'node:child_process';
import {copyFileSync,cpSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {admission,contained,sha256} from './rust-disk.mjs';
import {hashFile,runCargo,sourceDigest} from './rust-build.mjs';
import {stageEmmySources} from './rust-source-dependencies.mjs';

const root=resolve(process.cwd()),out=contained(root,resolve(process.argv[2]||'.tmp/rust-source-rebuild'));
admission(root,{requiredBytes:2*1024**3});mkdirSync(out);
const source=join(out,'source');mkdirSync(source);
for(const name of ['Cargo.toml','Cargo.lock','crates','release','LICENSE','THIRD_PARTY_NOTICES.md'])cpSync(join(root,name),join(source,name),{recursive:true});
mkdirSync(join(source,'tests/rust-source-data'),{recursive:true});
copyFileSync(join(root,'tests/rust-source-data/generate-unicode-simple.mjs'),join(source,'tests/rust-source-data/generate-unicode-simple.mjs'));
const emmy=stageEmmySources(root,source);
cpSync(join(root,'.tmp/rust-vendor/source'),join(source,'vendor'),{recursive:true});
mkdirSync(join(source,'.cargo'));
writeFileSync(join(source,'.cargo/config.toml'),'[source.crates-io]\nreplace-with="vendored-sources"\n[source.vendored-sources]\ndirectory="vendor"\n[target.x86_64-pc-windows-msvc]\nrustflags=["-C","target-feature=+crt-static"]\n');
const compiler=JSON.parse(readFileSync(join(source,'release/tools/rust.json'))),tools=[];
for(const [name,expected] of [['rustc',compiler.rustc],['cargo',compiler.cargo]]){
  const located=spawnSync('where.exe',[name],{encoding:'utf8',windowsHide:true,shell:false,timeout:30000});
  const executable=located.stdout?.trim().split(/\r?\n/)[0];
  if(located.status!==0||!executable)throw new Error(`rust-rebuild.toolchain_missing:${name}`);
  const version=spawnSync(executable,['--version','--verbose'],{windowsHide:true,shell:false,timeout:30000,maxBuffer:1<<20});
  writeFileSync(join(out,`${name}-identity.stdout.bin`),version.stdout||Buffer.alloc(0));
  writeFileSync(join(out,`${name}-identity.stderr.bin`),version.stderr||Buffer.alloc(0));
  if(version.status!==0||!version.stdout.toString().startsWith(`${name} ${expected} `))throw new Error(`rust-rebuild.toolchain_identity_mismatch:${name}`);
  tools.push({name,executable,executableSHA256:hashFile(executable),versionSHA256:sha256(version.stdout)});
}
const sourceCommit=spawnSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8',windowsHide:true,shell:false,timeout:30000,maxBuffer:1<<20});
if(sourceCommit.status!==0||!(/^[a-f0-9]{40}$/.test(sourceCommit.stdout.trim())))throw new Error('rust-rebuild.source_commit_unavailable');
const sealedSource={commit:sourceCommit.stdout.trim(),snapshotSHA256:sourceDigest(source),dirty:true};
const report={schema:'lycheedev.rust-source-rebuild.v1',state:'running',offline:true,locked:true,compiler,tools,emmy,sourceIdentity:sealedSource,sourceDigest:sealedSource.snapshotSHA256,sourceCargoLockSHA256:hashFile(join(source,'Cargo.lock')),builds:[],scope:`uncommitted immutable Rust-only snapshot including the complete ${emmy.identity==='public-upstream-unmodified-v1'?'unmodified public Emmy dependency and zero-patch recipe':'historical public-base Emmy reconstruction and patch recipe'}; historical private patch not recovered; two fresh Cargo homes and sequential empty controlled caches; not an official tagged release source archive`};
const reportPath=join(out,'report.json'),save=()=>writeFileSync(reportPath,JSON.stringify(report,null,2)+'\n');save();
for(const name of ['first','second']){
  const environment={CARGO_NET_OFFLINE:'true',LYCHEEDEV_BUILD_COMMIT:sealedSource.commit,LYCHEEDEV_BUILD_DIRTY:'true',CFLAGS:'/DNDEBUG /Brepro',CXXFLAGS:'/DNDEBUG /Brepro',CARGO_ENCODED_RUSTFLAGS:['-C','target-feature=+crt-static','-C','link-arg=/Brepro','--remap-path-prefix',`${source}=/lycheedev`,'--remap-path-prefix',`${source.replaceAll('\\','/')}=/lycheedev`].join('\x1f')};
  try{
    const built=runCargo(root,['build','--locked','--offline','-j','1','--release','-p','lycheedev-cli','--target',compiler.target],{sourceRoot:source,environment,owner:`rust-source-rebuild/${name}`,freshOffline:true,onSuccess(invocation){
      const generated=join(invocation.target,compiler.target,'release/lycheedev.exe'),binary=join(out,`${name}-lycheedev.exe`),digest=hashFile(generated,128<<20);
      copyFileSync(generated,binary);if(hashFile(binary,128<<20)!==digest||hashFile(generated,128<<20)!==digest)throw new Error('rust-rebuild.artifact_changed');
      return {name,sha256:digest,binary,inputHashes:invocation.inputHashes,environmentSHA256:invocation.environmentSHA256,buildEvidence:invocation.buildEvidence,freshCacheReceipt:invocation.freshCacheReceipt,cargoHome:invocation.cargoHome,preservedArtifacts:[{path:binary,sha256:digest}]};
    }});
    if(sourceDigest(source)!==report.sourceDigest)throw new Error('rust-rebuild.source_changed');
    for(const tool of tools)if(hashFile(tool.executable)!==tool.executableSHA256)throw new Error('rust-rebuild.toolchain_changed');
    report.builds.push(built);save();
  }catch(error){report.state='failed';report.error=error.message;report.failedBuild=name;report.buildEvidence=error.buildEvidence;save();throw error;}
}
report.state=report.builds[0].sha256===report.builds[1].sha256?'passed':'failed';save();
console.log(JSON.stringify(report));process.exitCode=report.state==='passed'?0:1;
