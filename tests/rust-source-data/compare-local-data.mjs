import {readFile,writeFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {isDeepStrictEqual} from 'node:util';
const [rustPath,goPath,oraclePath,output]=process.argv.slice(2);
// JSON.parse's source text keeps u64 Root hashes exact; ordinary JS Number
// comparison would silently round values above 2^53 in both oracle reports.
const read=async p=>JSON.parse((await readFile(p,'utf8')).replace(/^\uFEFF/,''),(_key,value,context)=>{
    if(typeof value==='number' && /^-?\d+$/.test(context.source) && !Number.isSafeInteger(value))return BigInt(context.source);
    return value;
});
const rust=await read(rustPath),go=await read(goPath),table=rust.tables[0];
const checks={rowZero:isDeepStrictEqual(table.row,go.row),sqlZero:isDeepStrictEqual([table.sqlResult.columns,table.sqlResult.rows],[go.sqlResult.columns,go.sqlResult.rows]),rootVariants:isDeepStrictEqual(rust.entries,go.entries),wholeMapSHA256:table.decoded.sha256===go.decodedSHA256,metadata:isDeepStrictEqual(rust.identity,go.metadata),allRows:isDeepStrictEqual(table.allRows,go.allRows),sixQueries:isDeepStrictEqual(table.queries.map(({complete,...q})=>q),go.queries)};
let differingRows=0;const examples=[];
for(let i=0;i<Math.max(table.allRows.length,go.allRows.length);i++)if(!isDeepStrictEqual(table.allRows[i],go.allRows[i])){differingRows++;if(examples.length<3){const a=table.allRows[i],b=go.allRows[i];const fields=[...new Set([...Object.keys(a??{}),...Object.keys(b??{})])].filter(k=>!isDeepStrictEqual(a?.[k],b?.[k]));examples.push({index:i,id:a?.ID,fields:fields.map(k=>({field:k,rust:a?.[k],go:b?.[k]}))});}}
const report={schema:'lycheedev.rust-go-local-data-diff.v1',result:Object.values(checks).every(Boolean)?'pass':'fail',rustPath,goPath,checks,differingRows,examples,rows:table.allRows.length,queries:table.queries.length,rustMs:rust.elapsedMs,goMs:go.elapsedMs,performanceAcceptance:'not_run; single nonalternating samples not 50% gate',oracleAdapterSHA256:createHash('sha256').update(await readFile(oraclePath)).digest('hex'),frozenGoCommit:'9d3d3598b8bac7efabf0b937df30508075eaa841'};
const exact=(_key,value)=>typeof value==='bigint'?value.toString():value;
await writeFile(output,JSON.stringify(report,exact,2)+'\n');console.log(JSON.stringify(report,exact));
if(report.result==='fail')process.exitCode=1;
