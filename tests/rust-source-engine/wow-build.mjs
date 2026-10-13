// Seal this example's exact outputs while the governed Cargo channel is held.
import assert from 'node:assert/strict';
import {copyFileSync,existsSync,lstatSync,mkdirSync,readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {join,relative,resolve,sep} from 'node:path';
import {hashFile,runCargo} from '../../tools/rust-build.mjs';
import {fixedBusinessContract} from './wow-business-contract.mjs';

const root=resolve('.'),out=resolve(process.argv[2]);
assert(out.startsWith(root+sep)&&!existsSync(out),'new build artifact directory required');
assert(readFileSync(join(root,'crates/lycheedev-source/Cargo.toml'),'utf8').includes('path = "../../.tmp/rust-source-engine-research-01/upstream/crates/emmylua_code_analysis"'),
 'This original-engine sealer cannot attest a private fork. Use a context-aware sealer after freezing the actual patch inventory.');
const matrixMode=process.argv[3]??'baseline';assert(['baseline','regressions'].includes(matrixMode));
const contractOptions={regressions:matrixMode==='regressions'};
const contract=fixedBusinessContract(root,contractOptions);
const identityPath=join(root,'.tmp/rust-source-engine-research-01/source-identity.json');
const upstream=JSON.parse(readFileSync(identityPath));
assert.equal(upstream.commit,'08b0873962e83bb8307e9a81052e3f41578fbc78');assert.equal(upstream.files.length,848);
assert.equal(resolve(upstream.source),join(root,'.tmp/rust-source-engine-research-01/upstream'));
function verifyUpstream(){
 const names=[];let visited=0;
 function walk(path){assert(++visited<10000);const stat=lstatSync(path);assert(!stat.isSymbolicLink(),'no links in admitted upstream');if(stat.isDirectory()){for(const name of readdirSync(path))walk(join(path,name));}else{assert(stat.isFile());names.push(relative(upstream.source,path).split(sep).join('/'));}}
 walk(upstream.source);assert.deepEqual(names.sort(),upstream.files.map(row=>row.path).sort());
 let bytes=0;for(const row of upstream.files){const path=join(upstream.source,row.path);assert.equal(lstatSync(path).size,row.bytes);assert.equal(hashFile(path),row.sha256);bytes+=row.bytes;}
 assert.equal(bytes,upstream.totalBytes);return{commit:upstream.commit,files:upstream.files.length,bytes,manifestSHA256:hashFile(identityPath)};
}
const upstreamIdentity=verifyUpstream();
const program=join(root,'crates/lycheedev-source/examples/emmy_wow_binding_probe.rs');
const programSHA256=hashFile(program);
const command=['build','--locked','--offline','-j','1','-p','lycheedev-source','--features','emmy-prototype','--example','emmy_wow_binding_probe'];
const record=runCargo(root,command,{owner:'source-emmy-wow-binding',onSuccess(invocation){
 assert.equal(hashFile(program),programSHA256,'example changed during build');
 assert.deepEqual(fixedBusinessContract(root,contractOptions),contract,'embedded fixture changed during build');
 assert.deepEqual(verifyUpstream(),upstreamIdentity,'upstream changed during build');
 mkdirSync(out);
 const sourceBinary=join(invocation.target,'debug/examples/emmy_wow_binding_probe.exe');
 const sourcePdb=sourceBinary.replace(/\.exe$/,'.pdb');
 const binary=join(out,'emmy_wow_binding_probe.exe'),pdb=binary.replace(/\.exe$/,'.pdb');
 const binarySHA256=hashFile(sourceBinary),pdbSHA256=hashFile(sourcePdb);
 copyFileSync(sourceBinary,binary);copyFileSync(sourcePdb,pdb);
 for(const path of [sourceBinary,binary])assert.equal(hashFile(path),binarySHA256);
 for(const path of [sourcePdb,pdb])assert.equal(hashFile(path),pdbSHA256);
 const artifact={schema:'lycheedev.emmy-wow-build.v1',binary,binarySHA256,pdb,pdbSHA256,sameInvocation:true,buildReceipt:invocation.buildEvidence,sourceTree:invocation.inputHashes.sourceTree,environmentSHA256:invocation.environmentSHA256,command:['cargo',...command],programSHA256,fixtures:contract.inputs,upstreamIdentity,matrixMode};
 copyFileSync(program,join(out,'emmy_wow_binding_probe.rs'));
 writeFileSync(join(out,'contract.json'),JSON.stringify(contract,null,2)+'\n');
 for(const input of contract.inputs)copyFileSync(join(root,'tests/rust-source-engine/wow',input.file),join(out,input.file));
 writeFileSync(join(out,'artifact.json'),JSON.stringify(artifact,null,2)+'\n');
 return artifact;
}});
console.log(JSON.stringify(record));
