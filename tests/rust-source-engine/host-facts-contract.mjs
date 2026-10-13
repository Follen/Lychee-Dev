// Source-derived expectations and separately trusted toy host assumptions.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {commonFlowContract} from './flow-common-contract.mjs';
import {sourcePoint} from './query-facade-contract.mjs';
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
export function hostFactsContract(root){
 const flow=commonFlowContract(root),host=structuredClone(flow.host);
 host.schema='lycheedev.fixed-flow-host.v2';
 host.scope='explicit toy declarations/effects/arity only; production WoW API acceptance remains not_run';
 for(const item of host.callables)item.returnArity={Source:1,Pair:2,Use:0,UseSecond:0,Allow:0}[item.name];
 for(const item of host.predicates)item.returnArity=1;
 const open=structuredClone(host);open.schema='lycheedev.fixed-flow-host.open-arity.v1';
 for(const item of open.callables)delete item.returnArity;
 const material=(name,value)=>{const bytes=Buffer.from(JSON.stringify(value));return{name,bytes:bytes.toString('utf8'),length:bytes.length,sha256:sha(bytes)};};
 const materials=[material('fixed-host-v2',host),material('original-flow-rules-v1',flow.rules),material('fixed-host-open-arity-v1',open)];
 const bindings=[...host.callables.map((item,index)=>({kind:'callable',name:item.name,identity:item.identity,declaration:{material:'fixed-host-v2',pointer:`/callables/${index}`},rules:{material:'original-flow-rules-v1',pointer:`/calls/${item.name}`},returnArity:item.returnArity})),
  ...host.predicates.map((item,index)=>({kind:'predicate',name:item.name,identity:item.identity,declaration:{material:'fixed-host-v2',pointer:`/predicates/${index}`},returnArity:item.returnArity})),
  ...host.initialObjects.map((item,index)=>({kind:'empty_object',name:item.name,identity:item.identity,declaration:{material:'fixed-host-v2',pointer:`/initialObjects/${index}`}}))];
 const specs=[
  {name:'direct',files:['host-direct.lua'],host:'exact',calls:[
   {marker:'local safe, secret = Pair()',call:'Pair()',host:'Pair',count:1,returns:2},
   {marker:'local x = sourceAlias()',call:'sourceAlias()',host:'Source',count:1,returns:1},
   {marker:'Use(x)',call:'Use(x)',host:'Use',count:1,arguments:1},
   {marker:'UseSecond(Pair())',call:'UseSecond(Pair())',host:'UseSecond',count:1,arguments:2},
   {marker:'UseSecond(Pair())',call:'Pair()',host:'Pair',count:1,returns:2},
   {marker:'Allow(x)',call:'Allow(x)',host:'Allow',count:1},
   {marker:'local guard = issecretvalue(x)',call:'issecretvalue(x)',host:'issecretvalue',count:1,returns:1}],
   invariants:['all Source/Pair slots have fresh per-call values','predicate subject points to x selected value and use/write observation','host native typing has no declared types and remains unknown']},
  {name:'shadow',files:['host-shadow.lua'],host:'exact',calls:[
   {marker:'alias(savedSource())',call:'alias(savedSource())',host:'Use',count:1},
   {marker:'alias(savedSource())',call:'savedSource()',host:'Source',count:1},
   {marker:'Use(savedSource())',call:'Use(savedSource())',sourceClosure:true,count:1},
   {marker:'Use(savedSource())',call:'savedSource()',host:'Source',count:1},
   {marker:'    alias(Source())',call:'alias(Source())',host:'Use',count:1},
   {marker:'    alias(Source())',call:'Source()',sourceClosure:true,count:1},
   {marker:'alias(saved())',call:'alias(saved())',host:'Use',count:1},
   {marker:'alias(saved())',call:'saved()',host:'Source',count:1},
   {marker:'alias(saved())\nalias(Source())',call:'alias(Source())',host:'Use',count:1},
   {marker:'alias(saved())\nalias(Source())',call:'Source()',sourceClosure:true,count:1}],
   invariants:['old alias keeps selected host identity after global write','local Source and replaced global Source are source closures, not APIs']},
  {name:'colon',files:['host-colon.lua'],host:'exact',calls:[
   {marker:'local x = Box:Wrap(Source())',call:'Box:Wrap(Source())',sourceClosure:true,count:1,arguments:2,receiver:true},
   {marker:'local x = Box:Wrap(Source())',call:'Source()',host:'Source',count:1},
   {marker:'local viaDot = Box.Wrap(Box, Pair())',call:'Box.Wrap(Box, Pair())',sourceClosure:true,count:1,arguments:3},
   {marker:'local viaDot = Box.Wrap(Box, Pair())',call:'Pair()',host:'Pair',count:1}],
   invariants:['Box origin is environment record, never invented Lua declaration','colon receiver precedes v; dot receiver is explicit first argument','viaDot copies Pair slot1 and does not take slot2']},
  {name:'slots',files:['host-slots.lua'],host:'exact',calls:[
   {marker:'UseSecond(Pair())',call:'UseSecond(Pair())',host:'UseSecond',count:1,arguments:2},
   {marker:'UseSecond((Pair()))',call:'UseSecond((Pair()))',host:'UseSecond',count:1,arguments:1},
   {marker:'local a, b = Relay(Pair())',call:'Relay(Pair())',sourceClosure:true,count:1,arguments:2},
   {marker:'local x, y = Relay((Pair()))',call:'Relay((Pair()))',sourceClosure:true,count:1,arguments:1}],
   invariants:['parentheses reduce list tail to first slot','missing source formal b becomes nil for parenthesized call','return-slot and argument-slot edges preserve actual indices']},
  {name:'transfers',files:['host-transfers.lua'],host:'exact',calls:[
   {marker:'Use(copied)',call:'Use(copied)',host:'Use',count:1},
   {marker:'Use(object.field)',call:'Use(object.field)',host:'Use',count:1},
   {marker:'Use(a())',call:'Use(a())',host:'Use',count:1},
   {marker:'Use(b())',call:'Use(b())',host:'Use',count:1}],
   invariants:['copied preserves old Source slot despite object.field=nil','field read after nil is known absent','two returned closures capture distinct selected arguments/cells']},
  {name:'entry',files:['host-entry.lua'],host:'exact',entry:{kind:'symbol',file:'host-entry.lua',name:'Demo'},calls:[
   {marker:'local x = Source()',call:'Source()',host:'Source',count:1},
   {marker:'    Use(x)',call:'Use(x)',host:'Use',count:1},
   {marker:'    Use(value)',call:'Use(value)',host:'Use',count:1}],
   invariants:['entry frame is explicit abstract query request, not source call or template','symbolic formal parameter retains original declaration identity','source entry body calls keep original source sites']},
  {name:'entry-ambiguous',files:['host-entry-ambiguous.lua'],host:'exact',entry:{kind:'line',file:'host-entry-ambiguous.lua',line:1},selectorRequired:true,calls:[
   {marker:'function A() Use(Source()) end',call:'Use(Source())',host:'Use',count:0},
   {marker:'function A() Use(Source()) end',call:'Source()',host:'Source',count:0}],
   invariants:['two source candidate declarations are preserved','no arbitrary first/last function choice','no explicit-entry frame when target is ambiguous']},
  {name:'entry-exact-a',files:['host-entry-ambiguous.lua'],host:'exact',entry:{kind:'symbol',file:'host-entry-ambiguous.lua',name:'A'},calls:[
   {marker:'function A() Use(Source()) end',call:'Use(Source())',host:'Use',count:1},
   {marker:'function A() Use(Source()) end',call:'Source()',host:'Source',count:1}]},
  {name:'guard',files:['host-guard.lua'],host:'exact',entry:{kind:'symbol',file:'host-guard.lua',name:'Guarded'},calls:[
   {marker:'local x = Source()',call:'Source()',host:'Source',count:1},
   {marker:'if issecretvalue(x) then return end',call:'issecretvalue(x)',host:'issecretvalue',count:1},
   {marker:'    Use(x)',call:'Use(x)',host:'Use',count:1}],
   invariants:['risk label does not turn predicate into constant true','both true return and false continuation remain explicit','Use path depends on predicate false for exact subject']},
  {name:'phi',files:['host-phi.lua'],host:'exact',calls:[
   {marker:'local x = Source()',call:'Source()',host:'Source',count:1},
   {marker:'    x = Pair()',call:'Pair()',host:'Pair',count:1},
   {marker:'    x = Source()',call:'Source()',host:'Source',count:1},
   {marker:'Use(x)',call:'Use(x)',host:'Use',count:1}],
   invariants:['phi edges preserve both selected writes/conditions, no fabricated single return value']},
  {name:'unknown-order',files:['host-order-a.lua','host-order-b.lua','host-order-main.lua'],host:'exact',unknownOrder:true,calls:[
   {file:'host-order-main.lua',marker:'local x = Wrap(Source())',call:'Wrap(Source())',count:0},
   {file:'host-order-main.lua',marker:'local x = Wrap(Source())',call:'Source()',host:'Source',count:0}],
   invariants:['all three input files covered syntactically','execution order remains unknown and graph incomplete','no invented ordered load occurrence or selected Wrap value']},
  {name:'open-arity',files:['host-open-arity.lua'],host:'open',calls:[
   {marker:'local a, b, c = Source()',call:'Source()',host:'Source',count:1},
   {marker:'Use(b)',call:'Use(b)',host:'Use',count:1},
   {marker:'Use(c)',call:'Use(c)',host:'Use',count:1}],
   invariants:['declared slot1 is distinct from unknown open tail','missing slots2/3 unavailable rather than nil','return completeness explicitly unknown']},
  {name:'unknown-effects',files:['host-unknown.lua'],host:'exact',calls:[
   {marker:'local localValue = Source()',call:'Source()',host:'Source',count:1},
   {marker:'UnknownCall()',call:'UnknownCall()',unresolved:true,count:1},
   {marker:'knownAlias(localValue)',call:'knownAlias(localValue)',host:'Use',count:1},
   {marker:'Use(localValue)',call:'Use(localValue)',unresolved:true,count:1}],
   invariants:['known initial API binding and unknown ambient names are separate','uncertain global rebind cannot change copied local alias identity','unescaped localValue storage is not invalidated','external execution/effect uncertainty remains explicit']},
  {name:'capture-mutation',files:['host-capture-mutation.lua'],host:'exact',calls:[
   {marker:'Use(f())',call:'Use(f())',host:'Use',count:1},
   {marker:'Use(f())',call:'f()',sourceClosure:true,count:1}],
   invariants:['capture references a storage cell, not a creation-time value or owner closure','f return and Use argument select the latest nil write']},
  {name:'cardinality',files:['host-cardinality.lua'],host:'exact',calls:[
   {marker:'UseSecond(Variable())',call:'UseSecond(Variable())',host:'UseSecond',count:1,argumentArities:[0,2]},
   {marker:'UseSecond(Variable())',call:'Variable()',sourceClosure:true,count:1,returnArities:[0,2]},
   {marker:'UseSecond((Variable()))',call:'UseSecond((Variable()))',host:'UseSecond',count:1,arguments:1}],
   invariants:['bounded return-count alternatives are not an exact max width','conditional slot absence preserves its path predicates','parenthesized call passes one scalar even on zero-return path']},
  {name:'return-join',files:['host-return-join.lua'],host:'exact',calls:[
   {marker:'Use(Choose())',call:'Use(Choose())',host:'Use',count:1,argumentArities:[1,2]},
   {marker:'Use(Choose())',call:'Choose()',sourceClosure:true,count:1,returnArities:[1,2]}],
   invariants:['conditional_return_candidates connect each original return-use to merged slots','Source and Pair slot1 provenance and path conditions remain separately traceable','slot2 can be absent and is not unconditionally present']},
  {name:'unknown-tail',files:['host-unknown-tail.lua'],host:'exact',calls:[
   {marker:'savedUse(UnknownCall())',call:'savedUse(UnknownCall())',host:'Use',count:1,openArguments:true},
   {marker:'savedUse((UnknownCall()))',call:'savedUse((UnknownCall()))',host:'Use',count:1,arguments:1},
   {marker:'savedUse(Relay(UnknownCall()))',call:'Relay(UnknownCall())',sourceClosure:true,count:1,openArguments:true}],
   invariants:['unknown call in list tail is open, not exact one','parentheses adjust the unknown call to exactly one scalar','unknown open inputs to Relay formals remain unavailable rather than fabricated nil']},
 ];
 const inputs=[...new Set(specs.flatMap(x=>x.files))].map(file=>{const diskPath=`tests/rust-source-engine/wow/native/${file}`,bytes=readFileSync(join(root,diskPath));return{file,diskPath,bytes:bytes.length,sha256:sha(bytes)};});
 for(const spec of specs)for(const call of spec.calls){call.file??=spec.files[0];const text=readFileSync(join(root,'tests/rust-source-engine/wow/native',call.file),'utf8');call.site=sourcePoint(text,call.file,call.marker,call.call);assert(call.count>=0);}
 return{schema:'lycheedev.source-host-facts-contract.v1',state:'fixed_spec_not_execution_evidence',inputs,materials,bindings,cases:specs,
  originalFlowMatrixSHA256:flow.matrixSHA256,originalFlowFixtureSHA256:flow.frozenSHA256,
  requiredEvidence:['environment/source origins remain distinct','material bytes/SHA/pointer authenticated','API identity follows selected value','per-call fresh return slots','syntactic and adjusted arguments/receiver','common source-backed transfer edges','exact frame/entry/occurrence ownership','unknown arity/effects/order separate from completion and truncation','original admission budget/token and owned process release'],
  scope:'common host/slot/entry facts prerequisite; no secrecy-flow findings, production API acceptance, default migration or total Goal completion'};
}
