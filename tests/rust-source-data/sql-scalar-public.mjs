// Historical old-CLI oracle only. Never a current Agent acceptance entry.
if (process.argv[2] !== '--historical-oracle-only') throw new Error('historical.oracle_explicit_mode_required: use current Agent/typed native queue');
process.argv.splice(2, 1);
import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {resolve,join} from 'node:path';
import {isDeepStrictEqual} from 'node:util';
const directory=resolve(process.argv[2]??'.tmp/rust-data-db2/sql-scalar-fixed');mkdirSync(directory,{recursive:true});
const binaries={go:resolve('.tmp/lycheedev-oracle.exe'),rust:resolve(process.env.LYCHEEDEV_RUST_BIN??'target/release/lycheedev.exe')};
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex'),before=Object.fromEntries(Object.entries(binaries).map(([k,v])=>[k,hash(v)]));
const expressions=['LOWER(NULL, 1)','UPPER(NULL, 1)','LENGTH(NULL, 1)','ABS(NULL, 1)','HAS_FLAG(NULL, 1, 2)','BIT_AND(NULL, 1, 2)','COALESCE()','NULLIF(NULL, 1, 2)','1 / 2','-5 % 2','NULL AND FALSE','NULL OR TRUE','9223372036854775807 + 1','18446744073709551615 + 1','ABS(-9223372036854775808)','BIT_AND(-1, 18446744073709551615)',"CASE WHEN TRUE THEN 'valid' ELSE 1 / 0 END",'NULLIF(1, 1.0)','CASE WHEN TRUE THEN 1 ELSE LOWER(NULL,1) END'];
const common=['--snapshot','PIN-5e5e7a341fdc93b9f965da5682e7b9f69cccd3b7bfc0a179d4d7a2bdf21ac2a3','--installation','D:/Game/World of Warcraft/_retail_','--offline','--home',resolve('.tmp/rust-data-db2/home'),'--format','json'],cases=[];
for(let index=0;index<expressions.length;index++){
 const expression=expressions[index],query=`SELECT ${expression} AS value FROM Map LIMIT 1`,runs={};
 for(const [engine,binary]of Object.entries(binaries)){
  const run=spawnSync(binary,['data','sql','--sql',query,...common],{cwd:resolve('.'),encoding:'utf8',windowsHide:true,timeout:180000,maxBuffer:16<<20});
  writeFileSync(join(directory,`${index}-${engine}.json`),run.stdout??'');writeFileSync(join(directory,`${index}-${engine}.stderr`),run.stderr??'');
  const env=JSON.parse(run.stdout,(k,v,c)=>{if(typeof v==='number'&&c?.source&&/^-?\d+$/.test(c.source)){const n=BigInt(c.source);if(n>BigInt(Number.MAX_SAFE_INTEGER)||n<BigInt(Number.MIN_SAFE_INTEGER))return n;}return v;});
  runs[engine]={exit:run.status,ok:env.ok,result:env.result?.result,error:env.error};
 }
 cases.push({expression,query,equal:isDeepStrictEqual(runs.go,runs.rust),...runs});
}
const after=Object.fromEntries(Object.entries(binaries).map(([k,v])=>[k,hash(v)]));
const report={scope:'Original18 scalar/error cases plus unreachable-branch arity binding; exact public result/error objects, one local pin. Not wholeSQL/performance.',binarySHA256:before,artifactStable:isDeepStrictEqual(before,after),cases,passed:isDeepStrictEqual(before,after)&&cases.every(c=>c.equal)};
writeFileSync(join(directory,'report.json'),JSON.stringify(report,(k,v)=>typeof v==='bigint'?`${v}n`:v,2)+'\n');
process.stdout.write(JSON.stringify({report:join(directory,'report.json'),passed:report.passed,differences:cases.filter(c=>!c.equal)},(k,v)=>typeof v==='bigint'?`${v}n`:v,2)+'\n');
if(!report.passed)process.exitCode=1;
