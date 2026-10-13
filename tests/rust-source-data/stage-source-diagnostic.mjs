// Prepare only: fixed old tool payload + one newly sealed native EXE/PDB.
// This directory is a diagnostic release, never candidate/package acceptance.
import {createHash} from 'node:crypto';
import {existsSync,mkdirSync,readFileSync,writeFileSync,copyFileSync,lstatSync} from 'node:fs';
import {resolve,join,dirname} from 'node:path';
import {contained} from '../../tools/rust-disk.mjs';
import {hashFile} from '../../tools/rust-build.mjs';

const root=resolve('.'),[artifactArg,baselineArg,outputArg]=process.argv.slice(2);
if(!artifactArg||!baselineArg||!outputArg)throw new Error('sealed artifact, baseline release, fresh output required');
const artifactRoot=contained(root,resolve(artifactArg)),baseline=contained(root,resolve(baselineArg)),out=contained(root,resolve(outputArg));
if(existsSync(out))throw new Error('Fresh diagnostic output required');
const sha=raw=>createHash('sha256').update(raw).digest('hex');
const read=path=>{const info=lstatSync(path);if(!info.isFile()||info.isSymbolicLink()||info.size>(128<<20))throw new Error('Invalid bounded artifact file');return readFileSync(path);};
const artifact=JSON.parse(read(join(artifactRoot,'artifact.json')));
const native=read(join(artifactRoot,'lycheedev.exe')),pdbPath=contained(artifactRoot,join(artifactRoot,'lycheedev.pdb'));
// Match the build sealer's 512 MiB PDB cap and hash in bounded chunks.
const pdbSHA256=hashFile(pdbPath);
if(artifact.profile!=='symbols'||artifact.sameInvocation!==true||sha(native)!==artifact.binarySHA256||pdbSHA256!==artifact.pdbSHA256)throw new Error('Exact same-build EXE/PDB required');
const originalManifest=read(join(baseline,'release.json')),release=JSON.parse(originalManifest);
if(release.schema!=='lycheedev.release.v1'||release.runtime!=='rust'||release.binaries['windows-amd64'].path!=='native/windows-amd64/lycheedev.exe'||release.resources.length>20000)throw new Error('Invalid original diagnostic release');
const oldBinary=read(join(baseline,release.binaries['windows-amd64'].path));
if(sha(oldBinary)!==release.binaries['windows-amd64'].sha256)throw new Error('Original sealed native changed');
mkdirSync(out,{recursive:true});
let bytes=0;
const resources=[];
for(const resource of release.resources){
 const src=contained(baseline,join(baseline,'payload',resource.path));
 const dest=contained(out,join(out,'payload',resource.path));
 const raw=read(src);bytes+=raw.length;if(bytes>(512<<20))throw new Error('Diagnostic payload byte budget');
 if(raw.length!==resource.bytes||sha(raw)!==resource.sha256)throw new Error(`Original resource differs: ${resource.path}`);
 mkdirSync(dirname(dest),{recursive:true});writeFileSync(dest,raw);
 resources.push({path:resource.path,bytes:raw.length,sha256:sha(raw)});
}
const dest=join(out,'native/windows-amd64/lycheedev.exe');mkdirSync(dirname(dest),{recursive:true});writeFileSync(dest,native);
const savedPdb=join(out,'native/windows-amd64/lycheedev.pdb');
copyFileSync(pdbPath,savedPdb);
if(hashFile(pdbPath)!==pdbSHA256||hashFile(savedPdb)!==pdbSHA256)throw new Error('Matching PDB changed during copy');
release.binaries['windows-amd64']={path:'native/windows-amd64/lycheedev.exe',bytes:native.length,sha256:artifact.binarySHA256};
const manifest=Buffer.from(JSON.stringify(release,null,2)+'\n');writeFileSync(join(out,'release.json'),manifest);
const identity={schema:'lycheedev.source-diagnostic-release.v1',scope:'Original exact Source tool resources with only native EXE replaced by a sealed symbols build; not an npm or complete candidate acceptance',artifact:join(artifactRoot,'artifact.json'),binarySHA256:sha(native),pdbSHA256,baselineRelease:baseline,baselineManifestSHA256:sha(originalManifest),baselineBinarySHA256:sha(oldBinary),releaseManifestSHA256:sha(manifest),resources};
writeFileSync(join(out,'diagnostic-identity.json'),JSON.stringify(identity,null,2)+'\n');
console.log(JSON.stringify({release:out,binarySHA256:identity.binarySHA256,pdbSHA256:identity.pdbSHA256,releaseManifestSHA256:identity.releaseManifestSHA256,resources:resources.length,bytes}));
