// Independent fixed-source checks. Does not run an alternative Lua evaluator.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {controlContract} from './control-contract.mjs';
export function verifyControl(root,frames){
 const contract=controlContract(root),checks=[];
 const check=(name,expected,actual)=>{let passed=true,error;try{assert.deepEqual(actual,expected)}catch(e){passed=false;error=e.message}checks.push({name,expected,actual,passed,error});};
 const scenarios=frames.filter(x=>x.event==='scenario');
 const sameSite=(a,b)=>a?.file===b?.file&&a?.start===b?.start&&a?.end===b?.end;
 check('all fixed control cases exactly once',contract.inputs.map(x=>x.file),scenarios.map(x=>x.file));
 const reasons={while:'while_prefix_verified_further_termination_unresolved',generic:'generic_iterator_identity_or_iteration_sequence_unavailable',budget:'numeric_loop_iteration_budget_exhausted','zero-step':'numeric_zero_step_not_in_supported_termination_subset','function-budget':'numeric_loop_iteration_budget_exhausted','function-generic':'generic_iterator_identity_or_iteration_sequence_unavailable'};
 for(const spec of contract.cases){
  const row=scenarios.find(x=>x.file===spec.file);if(!row){check(`${spec.name}:scenario exists`,true,false);continue}
  const g=row.graph,n=g.native_invocations,c=n?.control;
  check(`${spec.name}:control facts exist`,true,!!c);if(!c)continue;
  check(`${spec.name}:complete`,spec.completion==='complete',c.complete);
  check(`${spec.name}:truncated`,spec.truncated??false,c.truncated);
  if(spec.boundary)check(`${spec.name}:exact boundary`,true,c.boundaries.some(x=>x.reason===reasons[spec.name]&&x.truncated===(spec.truncated??false)));
  check(`${spec.name}:exact provider acceptance`,spec.freezeAccepted??spec.completion==='complete',row.freeze.accepted);
  if(spec.freezeReason)check(`${spec.name}:explicit occurrence ambiguity`,true,row.freeze.error?.includes(spec.freezeReason)??false);
  if(spec.completion!=='complete')check(`${spec.name}:incomplete provider unavailable`,'source.native_unavailable',row.freeze.code);
  const concrete=call=>{const view=n.query_views[call.context];return view?.kind==='load'||view?.kind==='invocation'&&n.frames[view.frame]?.call!=null};
  const number=fact=>{const u=g.uses[fact];return u?.decision?.state==='bound'?c.numbers[u.decision.detail]??null:null;};
  for(const expected of spec.calls){
   const calls=g.calls.filter(call=>sameSite(call.site,expected.site)&&concrete(call));
   check(`${spec.name}:${expected.marker}:call count`,expected.count,calls.length);
   if(expected.arguments)check(`${spec.name}:${expected.marker}:exact numeric arguments`,expected.arguments==='exact1through64'?Array.from({length:64},(_,i)=>i+1):expected.arguments,calls.map(call=>number(call.arguments[0])));
   for(const call of calls){
    const occurrence=c.context_occurrences[call.context],o=c.occurrences[occurrence];
    check(`${spec.name}:call ${call.id} occurrence`,true,!!o&&o.context===call.context||!!o&&call.context.startsWith(o.context));
    check(`${spec.name}:call ${call.id} occurrence source`,true,!!o&&o.site.file===call.site.file&&o.site.start<=call.site.start&&o.site.end>=call.site.end);
   }
  }
  if(spec.stoppedCaller){const calls=g.calls.filter(call=>sameSite(call.site,spec.stoppedCaller.site)&&concrete(call));check(spec.name+':stopped caller recorded',1,calls.length);check(spec.name+':no fabricated caller result',[null],calls.map(call=>call.resultFactId));}
  for(const expected of spec.selectedReturns??[]){const calls=g.calls.filter(call=>sameSite(call.site,expected.site)&&concrete(call));check(`${spec.name}:${expected.marker}:return count`,1,calls.length);check(`${spec.name}:${expected.marker}:exact return`,expected.number,calls.length===1?number(calls[0].resultFactId):null);}
  check(`${spec.name}:occurrence IDs unique and ordered`,true,c.occurrences.every((x,i)=>x.id===i));
  check(`${spec.name}:activation IDs ordered`,true,c.activations.every((x,i)=>x.id===i));
  for(const o of c.occurrences){check(`${spec.name}:occurrence ${o.id} bounded source`,true,o.site.file===spec.file&&Number.isInteger(o.site.start)&&Number.isInteger(o.site.end)&&o.site.start>=0&&o.site.end>=o.site.start&&o.site.end<=contract.inputs.find(x=>x.file===spec.file).bytes);for(const loop of o.loops)check(`${spec.name}:occurrence ${o.id} loop activation`,true,!!c.activations[loop.activation]&&loop.iteration>=0&&loop.iteration<64);}
  for(const e of c.edges)check(`${spec.name}:edge ${e.from}:${e.to} identities`,true,(e.from===null||!!c.occurrences[e.from])&&!!c.occurrences[e.to]&&(!e.condition||!!g.uses[e.condition.condition]));
  if(spec.name==='numeric'){
   const queries=frames.filter(x=>x.event==='occurrence_query'&&x.file===spec.file);check('numeric:all five repeated argument queries',5,queries.length);
   for(const q of queries){const u=g.uses[q.argumentFact];const id=u?.decision?.detail;const expected=createHash('sha256').update(JSON.stringify([g.snapshot_key,'value',id])).digest('hex');check('numeric:occurrence '+q.occurrence+' exact selected value',expected,q.definition.selected.value?.value_id);check('numeric:occurrence '+q.occurrence+' selection known','known',q.definition.selected.state);check('numeric:occurrence '+q.occurrence+' provider accepted',true,q.freezeAccepted);check('numeric:occurrence '+q.occurrence+' native constant',{state:'number_constant',value:number(q.argumentFact)},q.native);}
   check('numeric:repeated argument values distinct',5,new Set(queries.map(x=>x.definition.selected.value?.value_id).filter(Boolean)).size);
   const source=readFileSync(resolve(root,'tests/rust-source-engine/wow/native',spec.file),'utf8'),start=source.indexOf('for i = 1, 2 do\n    previous');
   const activations=c.activations.filter(a=>a.scope.start===start);
   check('numeric:capture loop has two scope activations',2,activations.length);
   const captured=source.indexOf('captured = i');const key=`${spec.file}:${captured}:${captured+8}`;
   check('numeric:captured declaration gets distinct cells',2,new Set(activations.map(a=>a.cells[key]).filter(Boolean)).size);
  }
  if(spec.name==='return')check('return:mixed exit frame retains both paths',true,n.frames.some(f=>f.call!=null&&f.return_paths?.length>=2&&f.return_paths.every(p=>p.outcome.kind==='return'&&p.paths.length>0)));
  const original=readFileSync(resolve(root,'tests/rust-source-engine/wow/native',spec.file));
  for(const[key,d]of Object.entries(c.derived_numbers??{})){
   const label=spec.name+':derived '+key;const site=x=>({file:x.file,start:x.start,end:x.end});const hash=createHash('sha256').update(JSON.stringify([g.snapshot_key,d.value,d.number_bits,site(d.origin),site(d.statement),d.header_facts,d.iteration,d.predecessor,d.occurrence,d.rule])).digest('hex');
   check(label+':exact proof',hash,d.proof_id);check(label+':value identity',Number(key),d.value);check(label+':exact numeric value',c.numbers[key],d.number);const bits=Buffer.alloc(8);bits.writeDoubleBE(d.number);check(label+':exact IEEE754 bits',bits.toString('hex'),d.number_bits);check(label+':rule','lua51-numeric-for-iteration-value-v1',d.rule);
   const statement=original.subarray(d.statement.start,d.statement.end).toString('utf8');const header=statement.split('\n')[0],match=/^for i\s*=\s*(-?\d+),\s*(-?\d+)(?:,\s*(-?\d+))? do$/.exec(header);
   check(label+':original fixed numeric-for header',true,!!match);check(label+':real declaration span','i',original.subarray(d.origin.start,d.origin.end).toString('utf8'));check(label+':declaration is in header',true,d.origin.start===d.statement.start+4&&d.origin.end===d.origin.start+1);
   if(match){const initial=Number(match[1]),step=match[3]===undefined?1:Number(match[3]);check(label+':value from fixed iteration',initial+step*d.iteration,d.number);check(label+':header source fact count',match[3]===undefined?2:3,d.header_facts.length);}
   for(const id of d.header_facts){const u=g.uses[id];check(label+':header fact '+id+' range',true,!!u&&u.site.file===spec.file&&u.site.start>d.origin.end&&u.site.end<=d.statement.start+Buffer.byteLength(header));}
   const o=c.occurrences[d.occurrence],loop=o?.loops.find(x=>sameSite(x.site,d.statement)&&x.iteration===d.iteration);check(label+':exact loop occurrence and activation',true,!!loop&&!!c.activations[loop.activation]);
   check(label+':predecessor continuity',true,d.iteration===0?d.predecessor===null:!!c.derived_numbers[d.predecessor]&&c.derived_numbers[d.predecessor].iteration===d.iteration-1&&sameSite(c.derived_numbers[d.predecessor].statement,d.statement));
  }
  if(['repeat','break'].includes(spec.name))check(`${spec.name}:break source exit retained`,true,c.exits.some(x=>x.outcome.kind==='break'));
  const cancel=frames.filter(x=>x.event==='cancel'&&x.file===spec.file);check(`${spec.name}:same operation cancellation`,[{code:'command.cancelled',exit:7}],cancel.map(x=>x.result));
 }
 check('honest complete terminal',[{complete:true,error:null,fullSourceAcceptance:false}],frames.filter(x=>x.event==='end').map(({complete,error,fullSourceAcceptance})=>({complete,error,fullSourceAcceptance})));
 return{schema:'lycheedev.source-control-independent.v1',passed:checks.every(x=>x.passed),checks,fullSourceAcceptance:false};
}
if(process.argv[1]&&resolve(process.argv[1])===resolve(import.meta.filename)){const[rootReport,out]=process.argv.slice(2),report=JSON.parse(readFileSync(rootReport));const result=verifyControl(resolve('.'),report.frames);writeFileSync(out,JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({out,passed:result.passed,checks:result.checks.length,failures:result.checks.filter(x=>!x.passed)}));if(!result.passed)process.exitCode=1;}
