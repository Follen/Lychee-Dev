import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {isDeepStrictEqual} from 'node:util';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
const root=resolve('.'),directory=resolve(process.argv[2]??'.tmp/rust-data-db2/stream-public-first');mkdirSync(directory,{recursive:true});
const binaries={go:join(root,'.tmp/lycheedev-oracle.exe'),rust:resolve(process.env.LYCHEEDEV_RUST_BIN??'target/release/lycheedev.exe')};
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex');
const before=Object.fromEntries(Object.entries(binaries).map(([k,v])=>[k,hash(v)]));
const parse=raw=>raw.trim().split('\n').filter(Boolean).map(line=>JSON.parse(line,(key,value,context)=>{if(typeof value==='number'&&context?.source&&/^-?\d+$/.test(context.source)){const n=BigInt(context.source);if(n>BigInt(Number.MAX_SAFE_INTEGER)||n<BigInt(Number.MIN_SAFE_INTEGER))return n;}return value;}));
const common=['data','db2','stream','Map','--snapshot','PIN-5e5e7a341fdc93b9f965da5682e7b9f69cccd3b7bfc0a179d4d7a2bdf21ac2a3','--installation','D:/Game/World of Warcraft/_retail_','--offline','--home',join(root,'.tmp/rust-data-db2/home')];
const cases=[];
for(const [name,args,success] of [
 ['complete',['--format','jsonl','--limit','200000'],true],['truncated',['--format','jsonl','--limit','3'],true],['projected',['--format','jsonl','--limit','3','--fields','ID,Directory'],true],['filtered',['--format','jsonl','--limit','200000','--filter','ID=00'],true],
 ['unknown-field',['--format','jsonl','--limit','3','--filter','Nope=1'],false],['bad-filter',['--format','jsonl','--limit','3','--filter','invalid'],false],['missing-limit',['--format','jsonl'],false],['wrong-format',['--format','json','--limit','3'],false],
]){
 const runs={};for(const [engine,binary]of Object.entries(binaries)){
  const run=spawnSync(binary,[...common,...args],{cwd:root,encoding:'utf8',timeout:180000,maxBuffer:16<<20});
  writeFileSync(join(directory,`${engine}-${name}.jsonl`),run.stdout??'');writeFileSync(join(directory,`${engine}-${name}.stderr`),run.stderr??'');
  runs[engine]={exit:run.status,frames:parse(run.stdout),error:run.error?.message};
 }
 const differences=[];
 if(runs.go.exit!==runs.rust.exit)differences.push({path:'exit',go:runs.go.exit,rust:runs.rust.exit});
 if(success){
  if(runs.go.frames.length!==runs.rust.frames.length)differences.push({path:'frames.length',go:runs.go.frames.length,rust:runs.rust.frames.length});
  for(let i=0;i<runs.go.frames.length;i++)if(!isDeepStrictEqual(runs.go.frames[i],runs.rust.frames[i]))differences.push({path:`frames[${i}]`,go:runs.go.frames[i],rust:runs.rust.frames[i]});
 }else{
  for(const engine of ['go','rust'])if(runs[engine].frames.some(f=>f.frame==='begin'||f.frame==='end'))differences.push({path:engine,kind:'typed-stream-started-on-refusal'});
  const error=engine=>{const frame=runs[engine].frames.findLast(f=>f.error||f.envelope?.error);return frame?.error??frame?.envelope?.error;};
  for(const engine of ['go','rust'])if(!error(engine))differences.push({path:engine,kind:'missing-error-payload'});
  for(const key of ['code','message','stage'])if(!isDeepStrictEqual(error('go')?.[key],error('rust')?.[key]))differences.push({path:`error.${key}`,go:error('go')?.[key],rust:error('rust')?.[key]});
 }
 cases.push({name,goExit:runs.go.exit,rustExit:runs.rust.exit,frames:runs.rust.frames.length,differences,passed:differences.length===0});
}
const after=Object.fromEntries(Object.entries(binaries).map(([k,v])=>[k,hash(v)]));
const report={schema:'lycheedev.typed-stream-public-differential.v1',binarySHA256:before,artifactSHA256After:after,artifactStable:isDeepStrictEqual(before,after),cases,passed:isDeepStrictEqual(before,after)&&cases.every(c=>c.passed),scope:'Public same fixed local Retail typed streams: full1184rows/arrays/floats/identity, projection/filter/truncation; prebegin error exits/stage/code/message. Not public pipe/CTRL_C or walltime gate.'};
writeFileSync(join(directory,'report.json'),JSON.stringify(report,(k,v)=>typeof v==='bigint'?`${v}n`:v,2)+'\n');
process.stdout.write(JSON.stringify({passed:report.passed,cases:cases.map(c=>({name:c.name,frames:c.frames,differences:c.differences}))},null,2)+'\n');
if(!report.passed)process.exitCode=1;
