import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import {resolve,join} from 'node:path';

const root=resolve(process.argv[2]??'.');
const home=resolve(root,'.tmp/rust-data-db2/home');
const output=resolve(process.argv[3]??join(root,'.tmp/rust-data-db2/public-first'));
mkdirSync(output,{recursive:true});
const binaries={go:join(root,'.tmp/lycheedev-oracle.exe'),rust:join(root,'target/release/lycheedev.exe')};
const hash=path=>createHash('sha256').update(readFileSync(path)).digest('hex');
const parse=raw=>JSON.parse(raw.replace(/^\uFEFF/,''),(key,value,context)=>{
  if(typeof value==='number' && context?.source && /^-?\d+$/.test(context.source)){
    const n=BigInt(context.source); if(n>BigInt(Number.MAX_SAFE_INTEGER)||n<BigInt(Number.MIN_SAFE_INTEGER))return n;
  }return value;
});
const common=['--snapshot','PIN-5e5e7a341fdc93b9f965da5682e7b9f69cccd3b7bfc0a179d4d7a2bdf21ac2a3','--installation','D:/Game/World of Warcraft/_retail_','--offline','--home',home,'--format','json'];
const cases=[
 ['record',['data','db2','--table','Map','--id','0']],
 ['page',['data','db2','--table','Map','--limit','3']],
 ['following',['data','db2','--table','Map','--limit','3','--after-id','36']],
 ['terminal',['data','db2','--table','Map','--after-id','4294967295']],
 ['schema',['data','db2','schema','Map']],
 ['search',['data','db2','search','Map','--field','Directory','--query','AZEROTH','--limit','1']],
 ['truncated',['data','db2','search','Map','--field','Directory','--query','a','--limit','2']],
 ['foreign-key',['data','db2','foreign-key','Map','--field','ID','--value','0','--limit','1']],
 ['invalid-field',['data','db2','foreign-key','Map','--field','Directory','--value','0','--limit','1']],
 ['missing-record',['data','db2','--table','Map','--id','4294967295']],
];
const reports=[];
for(const [name,args]of cases){
 const runs={};
 for(const [engine,binary]of Object.entries(binaries)){
  const start=performance.now(),run=spawnSync(binary,[...args,...common],{cwd:root,encoding:'utf8',timeout:180000,maxBuffer:16<<20});
  writeFileSync(join(output,`${engine}-${name}.json`),run.stdout??'');
  writeFileSync(join(output,`${engine}-${name}.stderr`),run.stderr??'');
  runs[engine]={exit:run.status,ms:performance.now()-start,error:run.error?.message,value:run.stdout?parse(run.stdout):null};
 }
 const differences=[],operational=[];
 const compare=(path,a,b)=>{
  if(path.endsWith('.decodedCache')||path.endsWith('.resources')||path.endsWith('.timings')){
   operational.push({path,equal:isDeepStrictEqual(a,b),go:a,rust:b});return;
  }
  if(isDeepStrictEqual(a,b))return;
  if(a&&typeof a==='object'&&!Array.isArray(a)&&b&&typeof b==='object'&&!Array.isArray(b)){
   for(const key of Object.keys(a))compare(`${path}.${key}`,a[key],b[key]);
  }else differences.push({path,kind:b===undefined?'missing':'different',go:a,rust:b});
 };
 compare('exit',runs.go.exit,runs.rust.exit);
 compare('ok',runs.go.value?.ok,runs.rust.value?.ok);
 compare('result',runs.go.value?.result,runs.rust.value?.result);
 if(!runs.go.value?.ok)compare('error',runs.go.value?.error,runs.rust.value?.error);
 compare('warnings',runs.go.value?.warnings,runs.rust.value?.warnings);
 const a=runs.go.value?.captures??[],b=runs.rust.value?.captures??[];
 compare('captures.length',a.length,b.length);
 for(let i=0;i<a.length;i++)for(const key of ['provenance','complete','truncated','mediaType'])compare(`captures[${i}].${key}`,a[i][key],b[i]?.[key]);
 reports.push({name,args,go:{exit:runs.go.exit,ms:runs.go.ms},rust:{exit:runs.rust.exit,ms:runs.rust.ms},differences,operational,legacyDataPassed:differences.length===0});
}
const report={schema:'lycheedev.db2-public-differential.v1',binarySHA256:Object.fromEntries(Object.entries(binaries).map(([k,v])=>[k,hash(v)])),home,client:common[3],cases:reports,legacyDataPassed:reports.every(x=>x.legacyDataPassed),scope:'All Go result fields, exits/errors/warnings and capture provenance compared; actual resource/cache/timing observations recorded separately, not claimed equal. New additive Rust fields ignored. One retained local Retail pin; no performance acceptance.'};
writeFileSync(join(output,'report.json'),JSON.stringify(report,(k,v)=>typeof v==='bigint'?`${v}n`:v,2)+'\n');
process.stdout.write(JSON.stringify({report:join(output,'report.json'),legacyDataPassed:report.legacyDataPassed,cases:reports.map(x=>({name:x.name,differences:x.differences}))},(k,v)=>typeof v==='bigint'?`${v}n`:v,2)+'\n');
