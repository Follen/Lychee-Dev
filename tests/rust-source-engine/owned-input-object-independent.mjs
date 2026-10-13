// Validate frozen object facts independently of native member/type APIs.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {existsSync,readFileSync,writeFileSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {hashFile} from '../../tools/rust-build.mjs';
const [reportName,outName]=process.argv.slice(2);assert(reportName&&outName);
const reportPath=resolve(reportName),out=resolve(outName);assert(!existsSync(out));
const report=JSON.parse(readFileSync(reportPath));assert.equal(report.schema,'lycheedev.emmy-native-context-owned.v1');
const contract=JSON.parse(readFileSync(join(dirname(reportPath),'fixed-contract.json')));
assert.equal(contract.schema,'lycheedev.emmy-native-context-contract.v1');
assert(['wow-native-binding-context-v2-object-observations','wow-native-binding-context-v4-source-inputs'].includes(contract.rulesVersion));
const checks=[];
function check(name,expected,actual){let passed=true,error;try{assert.deepEqual(actual,expected);}catch(e){passed=false;error=e.message;}checks.push({name,expected,actual,passed,error});}
const digest=value=>createHash('sha256').update(JSON.stringify(value)).digest('hex');
// Native structs serialize in declaration order; the JSONL Value maps sort
// keys. Reconstruct the reviewed struct encoding rather than hashing map order.
const viewKey=key=>({context:key.context,observation:key.observation,object_id:key.object_id});
const siteKey=site=>({file:site.file,start:site.start,end:site.end});
const decisionKey=decision=>decision.detail===undefined?{state:decision.state}:{state:decision.state,detail:decision.detail};
const enumKey=enumeration=>enumeration.reason===undefined?{state:enumeration.state}:{state:enumeration.state,reason:enumeration.reason};
const slotKey=slot=>({key:slot.key,presence:slot.presence,selected_value:decisionKey(slot.selected_value),destination_write:slot.destination_write,source_write:slot.source_write,copy_origin:slot.copy_origin,nested_object_view:slot.nested_object_view});
check('actual complete owned transport, fixed assertions and Lua51 syntax',true,report.transportComplete&&report.assertionContractComplete&&report.handlesReaped&&report.syntaxComplete);
check('sealed EXE',report.artifact.binarySHA256,hashFile(report.artifact.binary));
check('matching sealed PDB',report.artifact.pdbSHA256,hashFile(report.artifact.pdb));
const sources=new Map();
for(const fixture of report.artifact.fixtures){
 const path=join(dirname(report.artifact.sealedBuild.binary),'inputs/tests/rust-source-engine/wow',fixture.diskFile);
 check(`fixed bytes ${fixture.file}`,fixture.sha256,hashFile(path));sources.set(fixture.file,readFileSync(path));
}
const frames=report.frames.filter(frame=>frame.event==='scenario');check('one fixed object scenario',['object-versions'],frames.map(frame=>frame.name));
assert.equal(frames.length,1);const graph=frames[0].graph;
const expectedTypes=report.frames.filter(frame=>frame.event==='object_expected_type');
check('one source-backed native expected record',1,expectedTypes.length);
const expectedFields={keys:['value'],fields:{}};
for(const field of ['value','run','added']){const label=field==='value'?'number':null;expectedFields.fields[field]={memberInfo:label,withKey:label?[label]:[],withKeyAll:label?[label]:[],memberType:label};}
check('annotation retains exact native value:number structure',expectedFields,expectedTypes[0]?.memberAPIs);
check('fixed original annotation bytes','---@param obj { value: number }',sources.get('object-versions.lua').toString('utf8').split('\n')[0]);
const inputs=JSON.parse(graph.snapshot_inputs_json);
check('actual rules identity',contract.rulesVersion,inputs.adapter.rulesVersion);
assert.equal(contract.rulesVersion,'wow-native-binding-context-v4-source-inputs');
check('declared observation bounds',{objects:64,views:512,slots:4096,fieldsPerObject:128,depth:64,mutations:4096,sharedConstructionSteps:10000},inputs.adapter.objectObservationLimits);
check('actual immutable invocation limits',{blockDepth:64,callDepth:32,callObservationReturnSlots:32,callObservations:256,captureEdges:2048,capturesPerClosure:128,checkpointSlots:4096,checkpoints:128,closures:256,escapeEvents:4096,expressionDepth:64,frames:256,parameters:32,reachableNodes:4096,restores:128,retainedEscapeRoots:4096,returnArity:32,sharedConstructionSteps:10000,uncertainEffects:4096},inputs.adapter.invocationLimits);
check('actual query selector policy','source-templates-or-explicit-invocation-or-load-v1',inputs.adapter.queryViewProfile);
const admission=inputs.admission;
check('actual owned admission schema','lycheedev.native-source-inputs.v1',admission?.schema);
check('full admission digest in graph identity',digest(admission),inputs.admissionSHA256);
check('admission exact source bytes and analysis maps',contract.inputs.map(row=>({file:row.file,bytes:row.bytes,sha256:row.sha256,analysisBytes:row.bytes,analysisSHA256:row.sha256,map:{kind:'identity'}})),admission.files);
check('admission carries actual phase/order',[inputs.executionPhase,inputs.orderedScriptLoadOccurrences],[admission.executionPhase,admission.orderedScriptLoadOccurrences]);
check('no syntax pass invented in admission','pending',admission.syntaxCheck.state);
const views=graph.object_views??[],mutations=graph.object_mutations??[];
check('nonempty persisted views',true,views.length>0);check('view count bounded',true,views.length<=512);
check('mutation count bounded',true,mutations.length>0&&mutations.length<=4096);
check('contiguous observation sequence',mutations.map((_,i)=>i+1),mutations.map(row=>row.observation));
check('contiguous view identities',views.map((_,i)=>i),views.map(row=>row.id));
check('unique view keys',views.length,new Set(views.map(view=>JSON.stringify(view.key))).size);
check('global slot budget',true,views.reduce((sum,view)=>sum+view.fields.length,0)<=4096);
function text(site){const source=sources.get(site.file);assert(source&&site.start>=0&&site.end>=site.start&&site.end<=source.length);return source.subarray(site.start,site.end).toString('utf8');}
function presence(selected){
 if(selected.state==='known_absent')return 'absent';
 if(selected.state!=='bound')return 'unavailable';
 const value=graph.values[selected.detail];
 if(value.kind.kind==='primitive')return value.kind.detail==='nil'?'absent':'present';
 return ['object','function'].includes(value.kind.kind)?'present':'unavailable';
}
for(const mutation of mutations){
 check(`mutation ${mutation.observation} original source range`,true,text(mutation.site).length>0);
 if(mutation.kind==='field_write'){
  const write=graph.writes[mutation.write];check(`mutation ${mutation.observation} exact write source`,mutation.site,write?.site);
  check(`mutation ${mutation.observation} object owner`,true,write?.cell.startsWith(`object:${mutation.object_id}:field:`));
 }else check(`mutation ${mutation.observation} legal event`,true,['create','unknown'].includes(mutation.kind));
}
for(const view of views){
 const tag=`view ${view.id}`;
 check(`${tag} context`,graph.snapshot_key,view.key.context);
 check(`${tag} observation exists`,true,mutations.some(event=>event.observation===view.key.observation));
 const history=mutations.filter(event=>event.object_id===view.key.object_id&&event.observation<=view.key.observation);
 check(`${tag} has original allocation`,true,history.some(event=>event.kind==='create'));
 const writes=new Map(),writeObservations=new Map();let unknown,unknownObservation=0;
 for(const event of history){if(event.kind==='field_write'){const write=graph.writes[event.write];writes.set(write.cell.slice(`object:${view.key.object_id}:field:`.length),event.write);writeObservations.set(event.write,event.observation);}if(event.kind==='unknown'){unknown=event.reason;unknownObservation=event.observation;}}
 check(`${tag} exact latest field history`,[...writes.keys()].sort(),view.fields.map(slot=>slot.key));
 check(`${tag} per-object field budget`,true,view.fields.length<=128);
 let enumExpected=unknown?{state:'unavailable',reason:unknown}:{state:'complete'};
 for(const slot of view.fields){
  const write=graph.writes[writes.get(slot.key)],selected=unknown&&writeObservations.get(write.id)<=unknownObservation?{state:'unavailable',detail:unknown}:write.decision;
  check(`${tag} ${slot.key} current write`,write.id,slot.destination_write);
  check(`${tag} ${slot.key} selected value`,selected,slot.selected_value);
  check(`${tag} ${slot.key} Lua key presence`,presence(selected),slot.presence);
  if(slot.presence==='unavailable'&&enumExpected.state==='complete')enumExpected={state:'unavailable',reason:`field_presence_unavailable:${slot.key}`};
  const copy=graph.copies.find(edge=>edge.destination_object===view.key.object_id&&edge.destination_field_write===write.id);
  check(`${tag} ${slot.key} copy edge`,copy?.id??null,slot.copy_origin);
  check(`${tag} ${slot.key} source write`,copy?.source_field_write??null,slot.source_write);
  if(copy){check(`${tag} ${slot.key} copied value identity`,graph.writes[copy.source_field_write].decision,write.decision);check(`${tag} ${slot.key} destination identity`,write.id,copy.destination_field_write);}
  const value=selected.state==='bound'?graph.values[selected.detail]:null;
  if(value?.kind.kind==='object'){
   const child=views[slot.nested_object_view];check(`${tag} ${slot.key} child exists`,true,!!child);
   check(`${tag} ${slot.key} same heap observation`,view.key.observation,child?.key.observation);
   check(`${tag} ${slot.key} actual shallow identity`,value.kind.detail,child?.key.object_id);
  }else check(`${tag} ${slot.key} no fabricated nested object`,null,slot.nested_object_view);
 }
 check(`${tag} complete versus unavailable enumeration`,enumExpected,view.enumeration);
 check(`${tag} complete shape proof`,digest([viewKey(view.key),view.fields.map(slotKey),enumKey(view.enumeration)]),view.proof_id);
}
for(const fact of graph.uses){
 const object=fact.decision.state==='bound'&&graph.values[fact.decision.detail]?.kind.kind==='object';
 check(`use ${fact.id} exact native proof`,digest([graph.snapshot_key,fact.id,siteKey(fact.site),fact.context,decisionKey(fact.decision),fact.writes,fact.object_view??null]),fact.proof_id);
 if(object){const view=views[fact.object_view];check(`use ${fact.id} actual object view`,graph.values[fact.decision.detail].kind.detail,view?.key.object_id);}
}
const specs={source_old:{value:'number',run:'Identity'},source_new:{added:'boolean',run:'Replacement',value:'string'},copied_old:{run:'Identity',value:'number'},copied_new:{added:'boolean',run:'Replacement',value:'string'},twin:{added:'boolean',run:'Replacement',value:'string'},dynamic:{added:'unavailable',run:'unavailable',value:'unavailable'}};
const observations=report.frames.filter(frame=>frame.event==='object_observation');
check('exact interleaved observation queries',['source_old','source_new','source_old','copied_old','copied_new','copied_old','twin','dynamic'],observations.map(row=>row.query));
for(const row of observations){
 const fact=graph.uses[row.factId],view=views[fact.object_view],tag=`${row.query} step ${row.interleaveStep}`;
 check(`${tag} frozen wrapper is actual graph entry`,view,row.frozenObjectView.view);
 check(`${tag} provider epoch comes from actual view`,digest(viewKey(view.key)),row.binding.selectedValue.epoch);
 const fields={};for(const slot of view.fields){const value=slot.selected_value.state==='bound'?graph.values[slot.selected_value.detail]:null;fields[slot.key]=!value?'unavailable':value.kind.kind==='function'?graph.functions[value.kind.detail].name:value.kind.detail;}
 check(`${tag} independent exact source field values`,specs[row.query],fields);
 check(`${tag} expected enumeration`,row.query==='dynamic'?{state:'unavailable',reason:'dynamic_field_write'}:{state:'complete'},view.enumeration);
}
const result={schema:'lycheedev.owned-input-object-independent.v1',passed:checks.every(check=>check.passed),checks,
 scope:'fixed five-input object observation/provenance gate only; native member, compatibility and argument diagnostics remain separate business gates',
 inputReport:reportPath,inputReportSHA256:hashFile(reportPath)};
writeFileSync(out,JSON.stringify(result,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({out,passed:result.passed,checks:checks.length,failures:checks.filter(check=>!check.passed).map(check=>check.name)}));if(!result.passed)process.exitCode=1;
