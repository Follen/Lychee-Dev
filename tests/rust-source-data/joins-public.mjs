// Historical old-CLI oracle only. Never a current Agent acceptance entry.
if (process.argv[2] !== '--historical-oracle-only') throw new Error('historical.oracle_explicit_mode_required: use current Agent/typed native queue');
process.argv.splice(2, 1);
import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
import {verificationProof} from './verification-proof.mjs';
const directory=resolve(process.argv[2]??'.tmp/rust-data-joins-public');mkdirSync(directory,{recursive:true});
const binaries={go:resolve('.tmp/lycheedev-oracle.exe'),rust:resolve(process.env.LYCHEEDEV_RUST_BIN??'target/release/lycheedev.exe')};
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex'),before=Object.fromEntries(Object.entries(binaries).map(([k,v])=>[k,hash(v)]));
const queries=[
 ['self-inner','SELECT a.ID,b.MapName_lang FROM Map a JOIN Map b ON a.ID=b.ID ORDER BY a.ID LIMIT 20'],
 ['self-left','SELECT a.ID,b.ID FROM Map a LEFT JOIN Map b ON a.ID+100000=b.ID ORDER BY a.ID LIMIT 20'],
 ['qualified-static','SELECT Map.ID,b.ID FROM static.Map JOIN static.Map b ON Map.ID=b.ID ORDER BY Map.ID LIMIT 20'],
 ['cross-table-inner','SELECT a.ID,c.ID FROM Map a JOIN ChrClasses c ON a.ID=c.ID ORDER BY a.ID'],
 ['cross-table-left','SELECT c.ID,a.ID FROM ChrClasses c LEFT JOIN Map a ON c.ID+100000=a.ID ORDER BY c.ID'],
 ['cross-table-cross','SELECT c.ID,a.ID FROM ChrClasses c CROSS JOIN Map a WHERE a.ID=0 ORDER BY c.ID'],
 ['ambiguous','SELECT ID FROM Map a JOIN Map b ON a.ID=b.ID'],
 ['duplicate-alias','SELECT a.ID FROM Map a JOIN Map a ON TRUE']
];
const common=['--snapshot','PIN-5e5e7a341fdc93b9f965da5682e7b9f69cccd3b7bfc0a179d4d7a2bdf21ac2a3','--installation','D:/Game/World of Warcraft/_retail_','--home',resolve('.tmp/rust-data-db2/home'),'--format','json'];
const parse=text=>JSON.parse(text,(k,v,c)=>{if(typeof v==='number'&&c?.source&&/^-?\d+$/.test(c.source)){const n=BigInt(c.source);if(n>BigInt(Number.MAX_SAFE_INTEGER)||n<BigInt(Number.MIN_SAFE_INTEGER))return n;}return v;});
const cases=[];
for(let index=0;index<queries.length;index++){
 const [name,query]=queries[index],runs={};
 // First acquire the exact immutable DBD into the shared task-owned cache.
 // Network preparation is separated from the subsequent offline comparison.
 if(index===3&&!process.env.LYCHEEDEV_JOIN_OFFLINE_ONLY){
   const prep=spawnSync(binaries.rust,['data','sql','--sql','SELECT ID FROM ChrClasses LIMIT 1',...common],{cwd:resolve('.'),encoding:'utf8',windowsHide:true,timeout:180000,maxBuffer:16<<20});
   writeFileSync(join(directory,'preparation.json'),prep.stdout??'');writeFileSync(join(directory,'preparation.stderr'),prep.stderr??'');
   if(prep.status!==0)throw Error('fixed ChrClasses preparation failed; raw evidence retained');
 }
 for(const [engine,binary]of Object.entries(binaries)){
  const run=spawnSync(binary,['data','sql','--sql',query,...common,'--offline'],{cwd:resolve('.'),encoding:'utf8',windowsHide:true,timeout:180000,maxBuffer:32<<20});
  writeFileSync(join(directory,`${index}-${engine}.json`),run.stdout??'');writeFileSync(join(directory,`${index}-${engine}.stderr`),run.stderr??'');
  const env=parse(run.stdout);
  const result=env.result;
  // Mutable counters/timings and ephemeral captures are excluded. The Rust-only
  // verification declaration is separately asserted below, not treated as timing.
  // Actual fixed identities, all schema fields, row values and whole-content
  // proofs remain in the comparison; original envelopes remain beside it.
  const sources=result?.sources?.map(source=>({
   complete:source.complete,truncated:source.truncated,sources:source.sources,layoutHash:source.layoutHash,schema:source.schema,
   file:Object.fromEntries(Object.entries(source.file).filter(([key])=>!['decodedCache','resources','encodingIO','verification'].includes(key)))
  }));
  runs[engine]={exit:run.status,ok:env.ok,...(result?{complete:result.complete,partial:result.partial,query:result.query,result:result.result,sources}:{error:env.error})};
  if(engine==='rust'&&env.ok){
   runs[engine].verificationProof=verificationProof(result);
   const verified=[];
   for(const capture of env.captures??[]){
    const proof=spawnSync(binary,['evidence','verify',capture.id,'--home',resolve('.tmp/rust-data-db2/home'),'--format','json'],{encoding:'utf8',windowsHide:true,timeout:180000,maxBuffer:16<<20});
    writeFileSync(join(directory,`${index}-${capture.id}-verify.json`),proof.stdout??'');
    verified.push({id:capture.id,exit:proof.status,verified:parse(proof.stdout).result?.verified===true});
   }
   runs[engine].captureProof=verified;
  }
 }
 const {captureProof,verificationProof:verification,...rustCompared}=runs.rust;
 cases.push({name,query,equal:isDeepStrictEqual(runs.go,rustCompared)&&(verification===undefined||verification.passed)&&(captureProof===undefined||captureProof.length>0&&captureProof.every(p=>p.exit===0&&p.verified)),...runs});
}
const after=Object.fromEntries(Object.entries(binaries).map(([k,v])=>[k,hash(v)]));
const report={scope:'Eight sequential public JOIN result/error cases; exact fixed Retail/CN/zhCN Map+ChrClasses local CASC/DBD/WDC fields and retained content identity. Excludes mutable cache/resource/timing fields and separately asserts Rust-only verification claims; whole Encoding content is unverified; not full SQL or performance.',binarySHA256:before,artifactStable:isDeepStrictEqual(before,after),cases,passed:isDeepStrictEqual(before,after)&&cases.every(c=>c.equal)};
writeFileSync(join(directory,'report.json'),JSON.stringify(report,(k,v)=>typeof v==='bigint'?`${v}n`:v,2)+'\n');
console.log(JSON.stringify({report:join(directory,'report.json'),passed:report.passed,differences:cases.filter(c=>!c.equal)},(k,v)=>typeof v==='bigint'?`${v}n`:v,2));
if(!report.passed)process.exitCode=1;
