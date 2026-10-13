// Independent source/provenance checks for the fixed invocation lifecycle cases.
// Consumes sealed results only; it does not call the native inference provider.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync,readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
const [reportName,outName]=process.argv.slice(2);assert(reportName&&outName);
const input=resolve(reportName),out=resolve(outName);assert(!existsSync(out));
const report=JSON.parse(readFileSync(input)),contract=JSON.parse(readFileSync(join(dirname(input),'fixed-contract.json')));
assert.equal(contract.command,'invocation-lifecycle');
const checks=[];
function check(name,expected,actual){let passed=true,error;try{assert.deepEqual(actual,expected);}catch(e){passed=false;error=e.message;}checks.push({name,expected,actual,passed,error});}
const hash=x=>createHash('sha256').update(x).digest('hex');
const digest=x=>hash(JSON.stringify(x));
const site=x=>({file:x.file,start:x.start,end:x.end});
const decision=x=>x.detail===undefined?{state:x.state}:{state:x.state,detail:x.detail};
check('complete owned transport, assertions, syntax and cleanup',true,report.transportComplete&&report.assertionContractComplete&&report.syntaxComplete&&report.handlesReaped);
check('sealed EXE',report.artifact.binarySHA256,hashFile(report.artifact.binary));
check('matching PDB',report.artifact.pdbSHA256,hashFile(report.artifact.pdb));
const sources=new Map();for(const row of report.artifact.fixtures){const p=join(dirname(report.artifact.sealedBuild.binary),'inputs/tests/rust-source-engine/wow',row.diskFile);check(`source ${row.file}`,row.sha256,hashFile(p));sources.set(row.file,readFileSync(p));}
function text(s){const b=sources.get(s.file);assert(b&&s.start>=0&&s.end>s.start&&s.end<=b.length);return b.subarray(s.start,s.end).toString('utf8');}
const scenarios=report.frames.filter(x=>x.event==='scenario');
check('all eight fixed scenarios',contract.scenarios,scenarios.map(x=>x.name));
for(const s of scenarios){
 const g=s.graph,n=g.native_invocations,prefix=s.name;
 check(`${prefix} snapshot exact bytes`,g.snapshot_key,hash(g.snapshot_inputs_json));
 const inputs=JSON.parse(g.snapshot_inputs_json);
 check(`${prefix} invocation rules`,contract.rulesVersion,inputs.adapter.rulesVersion);
 check(`${prefix} source-only scope`,false,s.fullSourceAcceptance);
 check(`${prefix} contiguous frames`,n.frames.map((_,i)=>i),n.frames.map(x=>x.id));
 check(`${prefix} contiguous closures`,n.closures.map((_,i)=>i),n.closures.map(x=>x.id));
 check(`${prefix} frame bound`,true,n.frames.length<=256);
 check(`${prefix} closure bound`,true,n.closures.length<=256);
 check(`${prefix} retained external roots bounded`,true,(n.escaped_roots??[]).length<=4096);
 for(const f of n.frames){
  const label=`${prefix} frame ${f.id}`;
  check(`${label} source function`,true,!!g.functions[f.function]);
  check(`${label} query identity`,true,!!n.query_views[f.context]);
  check(`${label} bounded captures`,true,Object.keys(f.captures).length<=128);
  if(f.call===null){check(`${label} source template excludes concrete closure`,null,f.closure);continue;}
  const c=g.calls[f.call],closure=n.closures[f.closure];
  check(`${label} call exists`,true,!!c);check(`${label} original call syntax`,true,text(c.site).includes('('));
  check(`${label} closure actual function`,f.function,closure?.function);
  check(`${label} captures are cell references`,closure?.captures,f.captures);
  check(`${label} explicit invocation selector`,{frame:f.id,kind:'invocation'},n.query_views[f.context]);
  for(const id of f.return_facts){const r=g.uses[id];check(`${label} return ${id} exists`,true,!!r);check(`${label} return ${id} same frame`,true,r?.context===f.context||r?.context.startsWith(`${f.context}:`));}
 }
 const queries=report.frames.filter(x=>x.event==='lifecycle_query'&&x.scenario===prefix);
 for(const q of queries){
  const f=g.uses[q.factId],label=`${prefix} ${q.query}`;
  check(`${label} actual source range`,q.sourceSite,f?.site);check(`${label} original source text`,true,text(q.sourceSite).length>0);
  check(`${label} actual selected decision`,q.decision,f?.decision);
  check(`${label} selected fact proof`,q.proofId,f?.proof_id);
  check(`${label} independent source proof`,digest([g.snapshot_key,f.id,site(f.site),f.context,decision(f.decision),f.writes,f.object_view??null]),f.proof_id);
 }
 if(prefix==='captures'){
  const instances=n.closures.filter(c=>g.functions[c.function].closure.file==='invocation-captures.lua'&&Object.keys(c.captures).length);
  check('factory makes two instances of one source closure',2,instances.length);
  check('factory preserves same function source',1,new Set(instances.map(c=>c.function)).size);
  check('factory instances own separate captured cells',2,new Set(instances.flatMap(c=>Object.values(c.captures))).size);
 }
 if(prefix==='short-circuit')check('short-circuit only evaluates required call sites',['SetA()','SetB()'],g.calls.filter(c=>c.context.startsWith('load:')).map(c=>text(c.site)));
 if(prefix==='selector'){
  const qs=report.frames.filter(q=>q.event==='selected_frame_query'&&q.scenario===prefix);
  check('two explicit body query frames',2,qs.length);check('body query cache identities separate',2,new Set(qs.map(q=>q.contextIdentity)).size);
  check('one source body queried twice',1,new Set(qs.map(q=>JSON.stringify(q.sourceSite))).size);
  for(const q of qs)check(`selected ${q.query} actual frame call`,q.call,n.frames[q.frame]?.call);
 }
 if(prefix.startsWith('escape')){
  check(`${prefix} external roots persist`,true,(n.escaped_roots??[]).length>0);
  if(prefix!=='escape')check(`${prefix} captured cells retained`,true,(n.escaped_roots??[]).some(x=>x.kind==='cell'&&x.identity.includes('local:')));
 }
}
const result={schema:'lycheedev.invocation-lifecycle-independent.v1',passed:checks.every(x=>x.passed),checks,inputReport:input,inputReportSHA256:hashFile(input),scope:'eight fixed source-only lifecycle cases; no full Source or public command acceptance'};
writeFileSync(out,JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({out,passed:result.passed,checks:checks.length,failures:checks.filter(x=>!x.passed).map(x=>x.name)}));if(!result.passed)process.exitCode=1;
