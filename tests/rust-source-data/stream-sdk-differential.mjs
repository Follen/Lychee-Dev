// Historical old-CLI oracle only. Never a current Agent acceptance entry.
if (process.argv[2] !== '--historical-oracle-only') throw new Error('historical.oracle_explicit_mode_required: use current Agent/typed native queue');
process.argv.splice(2, 1);
import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync} from 'node:fs';
import {isDeepStrictEqual} from 'node:util';
import {resolve,join} from 'node:path';
import {createHash} from 'node:crypto';
const root=resolve('.'),directory=join(root,'.tmp/rust-data-db2/stream-sdk'),oracle=join(root,'.tmp/lycheedev-oracle.exe');
const parse=raw=>raw.trim().split('\n').filter(Boolean).map(line=>JSON.parse(line,(key,value,context)=>{if(typeof value==='number'&&context?.source&&/^-?\d+$/.test(context.source)){const n=BigInt(context.source);if(n>BigInt(Number.MAX_SAFE_INTEGER)||n<BigInt(Number.MIN_SAFE_INTEGER))return n;}return value;}));
const common=['data','db2','stream','Map','--snapshot','PIN-5e5e7a341fdc93b9f965da5682e7b9f69cccd3b7bfc0a179d4d7a2bdf21ac2a3','--installation','D:/Game/World of Warcraft/_retail_','--offline','--home',join(root,'.tmp/rust-data-db2/home'),'--format','jsonl'];
const cases=[];
for(const [name,args] of [['complete',['--limit','200000']],['truncated',['--limit','3']],['projected',['--limit','3','--fields','ID,Directory']],['filtered',['--limit','200000','--filter','ID=00']]]){
 const run=spawnSync(oracle,[...common,...args],{cwd:root,encoding:'utf8',timeout:180000,maxBuffer:16<<20});
 writeFileSync(join(directory,`go-${name}.jsonl`),run.stdout??'');writeFileSync(join(directory,`go-${name}.stderr`),run.stderr??'');
 const go=parse(run.stdout),rust=parse(readFileSync(join(directory,`rust-${name}.jsonl`),'utf8')),differences=[];
 if(go.length!==rust.length)differences.push({path:'frames.length',go:go.length,rust:rust.length});
 for(let i=0;i<go.length;i++)if(!isDeepStrictEqual(go[i],rust[i]))differences.push({path:`frames[${i}]`,go:go[i],rust:rust[i]});
 cases.push({name,goExit:run.status,frames:go.length,differences,passed:run.status===0&&differences.length===0});
}
const report={schema:'lycheedev.stream-sdk-go-differential.v1',goBinarySHA256:createHash('sha256').update(readFileSync(oracle)).digest('hex'),cases,passed:cases.every(c=>c.passed),scope:'All four exact real typed frame sequences, including full1184 rows. Rust SDK provider output, not Rust public CLI sink acceptance; no walltime gate.'};
writeFileSync(join(directory,'differential-report.json'),JSON.stringify(report,(k,v)=>typeof v==='bigint'?`${v}n`:v,2)+'\n');
process.stdout.write(JSON.stringify({passed:report.passed,cases:cases.map(c=>({name:c.name,frames:c.frames,differences:c.differences.length}))},null,2)+'\n');
