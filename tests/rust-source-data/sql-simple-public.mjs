// Historical old-CLI oracle only. Never a current Agent acceptance entry.
if (process.argv[2] !== '--historical-oracle-only') throw new Error('historical.oracle_explicit_mode_required: use current Agent/typed native queue');
process.argv.splice(2, 1);
import {spawnSync} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
import {resolve,join} from 'node:path';
const root=resolve('.'),output=resolve(process.argv[2]??'.tmp/rust-data-db2/sql-simple-fixed');
mkdirSync(output,{recursive:true});
const query="SELECT UPPER('ß') AS upper_value, LOWER('İ') AS lower_value, UPPER('ᾀ') AS greek_upper, LOWER('ΟΣ') AS sigma_lower, UPPER('ﬃ') AS ligature FROM Map LIMIT 1";
const common=['data','sql','--sql',query,'--snapshot','PIN-5e5e7a341fdc93b9f965da5682e7b9f69cccd3b7bfc0a179d4d7a2bdf21ac2a3','--installation','D:/Game/World of Warcraft/_retail_','--offline','--home',join(root,'.tmp/rust-data-db2/home'),'--format','json'];
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex'),runs={};
for(const [name,binary]of [['go',join(root,'.tmp/lycheedev-oracle.exe')],['rust',join(root,'target/release/lycheedev.exe')]]){
 const run=spawnSync(binary,common,{encoding:'utf8',timeout:180000,maxBuffer:16<<20,cwd:root});
 writeFileSync(join(output,`${name}.json`),run.stdout??'');writeFileSync(join(output,`${name}.stderr`),run.stderr??'');
 const envelope=JSON.parse(run.stdout??'null');runs[name]={exit:run.status,binarySHA256:hash(binary),result:envelope?.result?.result,query:envelope?.result?.query,error:envelope?.error,resourceAccounting:envelope?.result?.resourceAccounting};
}
const report={schema:'lycheedev.sql-simple-case-public.v1',query,runs,passed:runs.go.exit===0&&runs.rust.exit===0&&isDeepStrictEqual(runs.go.result,runs.rust.result)&&isDeepStrictEqual(runs.go.query,runs.rust.query),scope:'Same exact DataPin/CASC/WDC/SQL public CLI; retained Unicode simple-mapping cases only, not whole SQL parity/performance'};
writeFileSync(join(output,'report.json'),JSON.stringify(report,null,2)+'\n');
process.stdout.write(JSON.stringify(report,null,2)+'\n');
