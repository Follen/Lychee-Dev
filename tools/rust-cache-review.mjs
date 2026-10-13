// Read-only candidate receipts. This never deletes or automatically adopts a cache.
import {existsSync,lstatSync,readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {contained} from './rust-disk.mjs';import {hashFile,sourceDigest} from './rust-build.mjs';
const root=resolve(process.cwd()),out=resolve(process.argv[2]||'.tmp/rust-npm-terminal-cache-review.json'),rows=[];
for(const name of readdirSync(join(root,'.tmp')).filter(name=>name.startsWith('rust-npm')).sort()){
  const base=join(root,'.tmp',name),cache=join(base,'compile-target');if(!existsSync(cache))continue;
  const row={owner:'rust_foundation/npm-handoff-fixture',directory:contained(root,cache),category:'legacy-build-cache',retention:'rebuildable',state:'requires_root_process_review',artifacts:[],evidence:[],sourcePreserved:false};
  try{
    if(!lstatSync(cache).isDirectory())throw new Error('cache.non_regular');
    const source=join(base,'source');row.sourceDigest=sourceDigest(source);row.sourcePreserved=true;
    const manifest=join(base,'artifacts.json');
    if(existsSync(manifest)){const artifacts=JSON.parse(readFileSync(manifest));for(const item of artifacts){const binary=join(base,`package-${item.version}/native/windows-amd64/lycheedev.exe`),tgz=join(base,`lycheedev-${item.version}.tgz`);if(hashFile(binary,128<<20)!==item.binarySHA256||hashFile(tgz,128<<20)!==item.tarballSHA256)throw new Error('cache.frozen_artifact_integrity');row.artifacts.push({version:item.version,binary,binarySHA256:item.binarySHA256,tgz,tgzSHA256:item.tarballSHA256});}}
    for(const file of readdirSync(base).filter(file=>/^(build-.*\.log|.*\.log|report\.json|artifacts\.json)$/.test(file))){const path=join(base,file);if(lstatSync(path).isFile())row.evidence.push({path,sha256:hashFile(path,64<<20)});}
    if(!row.evidence.length)throw new Error('cache.no_terminal_evidence');
    row.rebuildableCandidate=true;row.symbols='No matched debug PDB is used by these npm fixtures; preserved package EXEs/tgz are their accepted artifact identities. Other owners/stage12 matching symbols remain protected.';
  }catch(error){row.rebuildableCandidate=false;row.error=error.message;}
  rows.push(row);
}
const report={schema:'lycheedev.legacy-cache-candidates.v1',root,readOnly:true,activeProcessReview:'Required by Root immediately before reclaim; this report does not infer terminal state from mtime or PID.',rows};
writeFileSync(out,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify({report:out,candidates:rows.filter(row=>row.rebuildableCandidate).map(row=>row.directory),blocked:rows.filter(row=>!row.rebuildableCandidate).map(row=>({directory:row.directory,error:row.error}))},null,2));
