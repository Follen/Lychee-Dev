// Derive fixed expectations from the original source and authenticated host JSON.
// This checks shared facts, never runs a second Lua evaluator.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {hostFactsContract} from './host-facts-contract.mjs';
const cases=['direct','shadow','colon','slots','transfers','phi','open-arity','unknown-effects','capture-mutation','cardinality','return-join','unknown-tail'];
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const sameSite=(a,b)=>a?.file===b?.file&&a?.start===b?.start&&a?.end===b?.end;
export function verifyHostRuntime(root,frames){
 const fixed=hostFactsContract(root),checks=[];
 function check(name,expected,actual){let passed=true,error;try{assert.deepEqual(actual,expected);}catch(e){passed=false;error=e.message;}checks.push({name,expected,actual,passed,error});}
 const scenarios=frames.filter(x=>x.event==='scenario');
 check('twelve fixed cases exactly once',cases,scenarios.map(x=>x.case));
 for(const name of cases){
  const row=scenarios.find(x=>x.case===name),spec=fixed.cases.find(x=>x.name===name);
  check(name+':scenario',true,!!row);if(!row)continue;
  const g=row.graph,h=g.host_runtime,n=g.native_invocations;
  check(name+':host facts exist',true,!!h);if(!h)continue;
  check(name+':rule identity',h.profile,row.inputIdentity.rules.hostRuntime);
  check(name+':snapshot rule identity',h.profile,g.snapshot_inputs.adapter.hostRuntime);
  check(name+':snapshot authenticated',sha(g.snapshot_inputs_json),g.snapshot_key);
  check(name+':host-only analysis scope',[false,false],[row.sourceExecuted,row.fullSourceAcceptance]);
  check(name+':explicit entry capability','source-explicit-entry-load-plan-v2',h.profile.explicitEntryCapability);
  check(name+':no explicit entry request',false,h.profile.entryRequest);
  check(name+':no explicit entry execution',0,(n.entries??[]).length);
  check(name+':source coverage',spec.files.map((file,index)=>`${index}:${file}`),g.script_load_occurrences);
  const source=readFileSync(join(root,'tests/rust-source-engine/wow/native',spec.files[0]));
  const sourceString=source.toString('utf8');
  const text=site=>source.subarray(site.start,site.end).toString('utf8');
  const bounded=site=>site?.file===spec.files[0]&&Number.isInteger(site.start)&&Number.isInteger(site.end)&&site.start>=0&&site.start<=site.end&&site.end<=source.length&&Buffer.byteLength(source.subarray(0,site.start).toString('utf8'))===site.start&&Buffer.byteLength(source.subarray(0,site.end).toString('utf8'))===site.end;
  const concrete=call=>{const view=n.query_views[call.context];return view?.kind==='load'||view?.kind==='invocation'&&n.frames[view.frame]?.call!=null;};
  check(name+':seven initial bindings',7,h.bindings.length);
  for(const b of h.bindings){
   const expected=fixed.bindings[b.binding],value=g.values[b.value],materialName=name==='open-arity'?'fixed-host-open-arity-v1':'fixed-host-v2';
   const material=fixed.materials.find(x=>x.name===materialName);
   check(name+':binding:'+b.binding+':name',expected.name,b.declaration.name);
   check(name+':binding:'+b.binding+':environment origin',{environment:{material:material.name,material_sha256:material.sha256,pointer:expected.declaration.pointer,identity:expected.identity}},value.origin);
   check(name+':binding:'+b.binding+':material identity',expected.identity,b.declaration.identity);
   if(expected.kind==='callable')check(name+':binding:'+b.binding+':raw rule',JSON.parse(fixed.materials.find(x=>x.name==='original-flow-rules-v1').bytes).calls[expected.name],b.declaration.rules.record);
  }
  check(name+':all source uses real',true,g.uses.every(x=>bounded(x.site)));
  check(name+':all source writes real',true,g.writes.every(x=>bounded(x.site)));
  check(name+':call IDs coherent',true,h.calls.every(x=>g.calls[x.call]?.context===x.context&&g.calls[x.call]?.resultFactId===x.scalar_result));
  const fresh=new Set();
  for(const call of h.calls){
   const original=g.calls[call.call];
   check(name+':call:'+call.call+':view',n.query_views[call.context],call.view);
   check(name+':call:'+call.call+':argument indices',call.arguments.map((_,i)=>i+1),call.arguments.map(x=>x.slot));
   check(name+':call:'+call.call+':source arguments',true,call.syntactic_arguments.every(bounded));
   check(name+':call:'+call.call+':slot facts coherent',true,[...call.arguments,...call.returns].every(x=>g.uses[x.use_fact]&&JSON.stringify(g.uses[x.use_fact].decision)===JSON.stringify(x.selected)&&JSON.stringify(g.uses[x.use_fact].writes)===JSON.stringify(x.writes)));
   if(call.return_arities!=null)check(name+':call:'+call.call+':exact only singleton',call.return_arities.length===1?call.return_arities[0]:null,call.return_arity);
   if(call.host_binding!=null){
    const binding=h.bindings[call.host_binding],decl=JSON.parse(fixed.materials.find(x=>x.name===binding.declaration.declaration.material).bytes);
    const record=[...decl.callables,...decl.predicates].find(x=>x.name===binding.declaration.name);
    check(name+':call:'+call.call+':callee identity',{kind:'host_callable',detail:call.host_binding},g.values[call.callee_value]?.kind);
    check(name+':call:'+call.call+':declared arity',record.returnArity??null,call.return_arity);
    check(name+':call:'+call.call+':open tail',record.returnArity==null,call.return_open_tail!=null);
    for(const slot of call.returns){
     check(name+':call:'+call.call+':fresh slot:'+slot.slot,'bound',slot.selected.state);
     const id=slot.selected.detail,value=g.values[id];check(name+':call:'+call.call+':distinct return:'+slot.slot,false,fresh.has(id));fresh.add(id);
     check(name+':call:'+call.call+':source return origin:'+slot.slot,original.site,value?.origin);
     check(name+':call:'+call.call+':host return identity:'+slot.slot,{kind:'host_return',detail:{binding:call.host_binding,call:call.call,slot:slot.slot}},value?.kind);
    }
    if(binding.declaration.name==='issecretvalue')check(name+':predicate is not assumed true','unknown_not_derived_from_risk_label',call.predicate?.truth);
   }
  }
  for(const expected of spec.calls){
   const selected=g.calls.filter(call=>sameSite(call.site,expected.site)&&concrete(call));
   check(name+':'+expected.marker+':'+expected.call+':count',expected.count,selected.length);
   for(const call of selected){const slots=h.calls.find(x=>x.call===call.id);
    if(expected.host)check(name+':selected host:'+call.id,expected.host,h.bindings[slots?.host_binding]?.declaration.name);
    if(expected.sourceClosure)check(name+':selected source closure:'+call.id,'function',g.values[g.uses[call.callee]?.decision.detail]?.kind.kind);
    if(expected.arguments!==undefined)check(name+':exact arguments:'+call.id,[expected.arguments],slots?.argument_arities);
    if(expected.argumentArities)check(name+':argument alternatives:'+call.id,expected.argumentArities,slots?.argument_arities);
    if(expected.returnArities)check(name+':return alternatives:'+call.id,expected.returnArities,slots?.return_arities);
    if(expected.openArguments)check(name+':open arguments:'+call.id,[null,true],[slots?.argument_arities,!!slots?.argument_open_tail]);
    if(expected.returns!==undefined)check(name+':return arity:'+call.id,expected.returns,slots?.return_arity);
    if(expected.unresolved)check(name+':unresolved callee:'+call.id,'unavailable',g.uses[call.callee]?.decision.state);
   }
  }
  check(name+':captures structural only',true,h.capture_references.every(x=>!('selected' in x)&&g.values[x.owner_value]?.kind.kind==='function'&&x.cell===n.capture_facts.find(c=>c.closure===x.closure&&c.cell===x.cell)?.cell));
  check(name+':no capture value edge',false,h.transfers.some(x=>x.kind==='capture_cell_reference'));
  check(name+':transfer references bounded',true,h.transfers.every(x=>x.source_uses.every(id=>g.uses[id])&&x.source_writes.every(id=>g.writes[id])));
  const environment=row.environmentContext.rows.filter(x=>x.origin.environment);
  check(name+':environment query has no fake snippet',true,environment.length===1&&!!environment[0].origin.environment&&!('snippet' in environment[0])&&!('exhibited' in environment[0]));
  if(name==='open-arity')for(const expression of ['Use(b)','Use(c)']){
   const call=g.calls.find(x=>text(x.site)===expression&&concrete(x));
   check(name+':open slot is unavailable:'+expression,'unavailable',g.uses[call?.arguments[0]]?.decision.state);
  }
  if(name==='return-join'){
   const call=g.calls.find(x=>text(x.site)==='Choose()'&&concrete(x));const slots=h.calls.find(x=>x.call===call?.id);
   const target=slots?.returns[0]?.use_fact,edge=h.transfers.find(x=>x.kind==='conditional_return_candidates'&&x.destination.use_fact===target);
   check(name+':first slot source fan-in',2,edge?.return_candidates?.length);
   check(name+':original return calls', ['Pair','Source'],edge?.return_candidates?.map(x=>g.values[x.selected.detail]?.kind.detail.binding).map(id=>h.bindings[id]?.declaration.name).sort());
   check(name+':both condition polarities',[false,true],[...new Set(edge?.return_candidates?.flatMap(x=>x.paths.flatMap(path=>path.map(p=>p.outcome))))].sort());
  }
  check(name+':frozen input exact bytes',sha(source),row.inputIdentity.files[0]?.sha256);
  check(name+':original string roundtrip',source.length,Buffer.byteLength(sourceString));
 }
 return{schema:'lycheedev.source-host-runtime-independent.v1',passed:checks.every(x=>x.passed),checks,
  scope:'fixed host/slots/shared transfer and cardinality facts; no explicit entry, guard secrecy flow or production API acceptance'};
}
