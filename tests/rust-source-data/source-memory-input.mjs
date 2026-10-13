// Independent Git oracle for the fixed Retail SDK memory experiment.
// Invoke this Node process inside the owned wrapper so every Git child belongs
// to the same Job. This does not use the Rust index or its cached manifests.
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync,mkdirSync,writeFileSync} from 'node:fs';
import {resolve,join} from 'node:path';

const mirror=resolve(process.argv[2]??''),output=resolve(process.argv[3]??'');
if(process.argv.length!==4||existsSync(output))throw new Error('Exact mirror and fresh output required');
mkdirSync(output,{recursive:true});
const commit='31c7f7b9cc79e56c986b365c06a6afbcf3c9177b';
const calls=[];
function git(args,input){
 const run=spawnSync('git',['--no-optional-locks','-C',mirror,...args],{input,timeout:60000,maxBuffer:128<<20,windowsHide:true});
 calls.push({args,status:run.status,error:run.error?.message,stderr:run.stderr?.toString('utf8'),bytes:run.stdout?.length});
 writeFileSync(join(output,'git-calls.json'),JSON.stringify(calls,null,2)+'\n');
 if(run.error||run.status!==0)throw new Error(`Independent Git read failed: ${run.error?.message??run.stderr}`);
 return run.stdout;
}
const commitRaw=git(['cat-file','commit',commit]);
if(createHash('sha1').update(`commit ${commitRaw.length}\0`).update(commitRaw).digest('hex')!==commit)throw new Error('Commit object identity differs');
const rootTree=/^tree ([0-9a-f]{40})\n/.exec(commitRaw.toString('utf8'))?.[1];
if(!rootTree)throw new Error('Missing pinned root tree');
const tree=git(['ls-tree','-r','-l','-z','--full-tree',commit]);
writeFileSync(join(output,'git-ls-tree.bin'),tree);
const entries=tree.toString('utf8').split('\0').filter(Boolean).map(entry=>{
 const match=/^(\d+) blob ([0-9a-f]{40})\s+(\d+)\t([\s\S]+)$/.exec(entry);
 if(!match)throw new Error('Unexpected Git tree record');
 return {mode:match[1],gitSHA1:match[2],bytes:Number(match[3]),path:match[4]};
});
const documents=entries.filter(e=>['100644','100755'].includes(e.mode)&&/\.(lua|xml|toc)$/i.test(e.path));
if(documents.some(e=>e.bytes>(32<<20)))throw new Error('Pinned document exceeds existing source bound');
for(let offset=0;offset<documents.length;offset+=64){
 const batch=documents.slice(offset,offset+64);
 const raw=git(['cat-file','--batch'],batch.map(e=>e.gitSHA1+'\n').join(''));
 let at=0;
 for(const doc of batch){
  const end=raw.indexOf(10,at),header=raw.subarray(at,end).toString('utf8');
  if(header!==`${doc.gitSHA1} blob ${doc.bytes}`)throw new Error('Batch header differs');
  const data=raw.subarray(end+1,end+1+doc.bytes);
  if(data.length!==doc.bytes||raw[end+1+doc.bytes]!==10)throw new Error('Batch framing differs');
  if(createHash('sha1').update(`blob ${doc.bytes}\0`).update(data).digest('hex')!==doc.gitSHA1)throw new Error('Git blob SHA1 differs');
  doc.sha256=createHash('sha256').update(data).digest('hex');
  at=end+doc.bytes+2;
 }
 if(at!==raw.length)throw new Error('Extra Git batch output');
}
const digest=createHash('sha256');
for(const doc of documents){const size=Buffer.alloc(8);size.writeBigUInt64LE(BigInt(doc.bytes));digest.update(doc.path).update('\0').update(doc.sha256).update('\0').update(size);}
const lua=documents.filter(doc=>/\.lua$/i.test(doc.path));
if(lua.length!==2551||lua.some(doc=>doc.bytes>(1<<20)))throw new Error('Fixed Retail full Lua manifest differs');
const manifest={schema:'lycheedev.source-memory-input.v1',basis:'Independent pinned Git commit/tree and every framed blob SHA1 plus SHA256; not the Rust index',commit,rootTree,treeSHA256:createHash('sha256').update(tree).digest('hex'),documents:documents.length,luaFiles:lua.length,luaBytes:lua.reduce((sum,doc)=>sum+doc.bytes,0),documentPathSHA256SizeDigest:digest.digest('hex'),files:documents};
writeFileSync(join(output,'manifest.json'),JSON.stringify(manifest,null,2)+'\n');
console.log(JSON.stringify({manifest:join(output,'manifest.json'),documents:manifest.documents,luaFiles:manifest.luaFiles,digest:manifest.documentPathSHA256SizeDigest}));
