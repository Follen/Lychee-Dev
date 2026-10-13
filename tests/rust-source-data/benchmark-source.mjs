// Explicit isolated fixture home/pin; never selects an installed/user workspace.
import {spawnSync} from 'node:child_process';
import {mkdirSync,writeFileSync,readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {performance} from 'node:perf_hooks';
import {isDeepStrictEqual} from 'node:util';
const [home,pin,go,rust,out]=process.argv.slice(2).map(v=>v);
if (![home,pin,go,rust,out].every(Boolean)) throw new Error('usage: benchmark-source.mjs HOME PIN GO RUST OUTPUT');
mkdirSync(out,{recursive:true});
const cases=[
  {name:'inspect',args:['source','inspect','--path','Interface/AddOns/Blizzard_APIDocumentationGenerated/UITimerDocumentation.lua','--line','10','--count','11']},
  {name:'exact',args:['source','query','C_Timer.After','--topic','api','--limit','3']},
  {name:'prefix',args:['source','query','C_Timer.A','--topic','api','--limit','3']},
  {name:'indexed',args:['source','query','seconds callback','--topic','api','--limit','3']},
];
const report={schema:'lycheedev.rust-source-file-benchmark.v1',date:new Date().toISOString(),home:resolve(home),pin,configuration:'optimized release binaries; same existing local source; sequential alternating engines; no network',samples:[],checks:[],remaining:['uncached Git/index construction parity','body/exploratory whole-source performance','full alias/scope/navigation/client matrix quality','all Source/Data 50% gate']};
for (const entry of cases) {
  const outputs=[];
  for(let sample=1;sample<=5;sample++) {
    const pair={};
    for(const [engine,binary] of [['go',go],['rust',rust]]) {
      const start=performance.now();const proc=spawnSync(resolve(binary),[...entry.args,'--snapshot',pin,'--home',resolve(home),'--format','json'],{encoding:'utf8',timeout:180000,maxBuffer:16<<20,windowsHide:true});
      const ms=performance.now()-start;
      writeFileSync(resolve(out,`${entry.name}-${engine}-${sample}.json`),proc.stdout||'');writeFileSync(resolve(out,`${entry.name}-${engine}-${sample}.stderr.txt`),proc.stderr||'');
      const row={name:entry.name,engine,sample,exit:proc.status,ms,bytes:Buffer.byteLength(proc.stdout||'')};report.samples.push(row);
      if(proc.status!==0) throw new Error(`${entry.name}/${engine}: ${proc.error?.message||proc.stderr||proc.stdout}`);
      pair[engine]=JSON.parse(proc.stdout).result;
    }
    outputs.push(pair);
  }
  const pair=outputs.at(-1);const baseline=pair.go;const current=pair.rust;
  const omitted=[];const changed=[];
  if(entry.name==='inspect') {for(const key of Object.keys(baseline)) {if(!(key in current))omitted.push(key);else if(!isDeepStrictEqual(baseline[key],current[key]))changed.push(key);}}
  else {
    for(const key of ['sourceId','product','requestedRef','matchedTag','resolvedCommit','snapshotId','complete','truncated','relations','nextCursor']) if(!isDeepStrictEqual(baseline[key],current[key]))changed.push(key);
    if(baseline.results.length!==current.results.length)changed.push('resultCount');
    for(let i=0;i<baseline.results.length;i++)for(const key of Object.keys(baseline.results[i])) {if(!(key in (current.results[i]||{})))omitted.push(`results[${i}].${key}`);else if(!isDeepStrictEqual(baseline.results[i][key],current.results[i][key]))changed.push(`results[${i}].${key}`);}
    for(const key of ['analyzedDocuments','skippedDocuments','diagnostics','complete'])if(!isDeepStrictEqual(baseline.coverage[key],current.coverage[key]))changed.push(`coverage.${key}`);
  }
  const timings=Object.fromEntries(['go','rust'].map(engine=>{const values=report.samples.filter(s=>s.name===entry.name&&s.engine===engine).map(s=>s.ms).sort((a,b)=>a-b);return[engine,{median:values[2],min:values[0],max:values[4]}];}));
  report.checks.push({name:entry.name,legacyFields:omitted.length===0&&changed.length===0?'passed':'different',omitted,changed,timings,ratio:timings.rust.median/timings.go.median,local50Percent:timings.rust.median<=timings.go.median*.5,matched:current.results?.length,preciseHits:current.results?.filter(r=>r.matchedBy==='exact_symbol').length,outputBytes:{go:report.samples.find(s=>s.name===entry.name&&s.engine==='go').bytes,rust:report.samples.find(s=>s.name===entry.name&&s.engine==='rust').bytes}});
}
report.state=report.checks.every(c=>c.legacyFields==='passed'&&c.local50Percent)?'selected_cases_passed_full_gate_open':'partial_failed';
writeFileSync(resolve(out,'report.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify(report.checks,null,2));
