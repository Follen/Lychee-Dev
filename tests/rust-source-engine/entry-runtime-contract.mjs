// Independent source/fact checks for the explicit entry and load-plan seam.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
export const entryCases=['entry','entry-empty','entry-literal','entry-ambiguous','entry-exact-a','entry-exact-b','entry-capture-a','entry-capture-b','known-order-ab','known-order-ba','unknown-order'];
export const entryInputCases=['entry','entry-ambiguous','entry-exact-a','unknown-order'];
export function entryAssertions(){
 const rows=[],add=(name,expected)=>rows.push({name:'execution:'+name,expected});
 for(const name of ['entry','entry-empty','entry-literal'])for(const[key,value]of [
  ['complete',true],['real_entry_origin',true],['source_body_calls_only',3],['formal_argument_state',name==='entry'?'unavailable':name==='entry-empty'?'known_absent':'bound'],
  ['source_assignment_to_use',true],['formal_to_use',true],['return_source_slot',true],['entry_query_view',true],['native_entry_provider',name==='entry-empty'?'Nil':'Unknown'],
 ])add(name+':'+key,value);
 // Exact request expectation is derived from the separately emitted source candidate.
 for(const [name,value]of [
  ['entry:argument_identity_distinct',3],['entry:invalid_line_rejected',4],['entry:duplicate_closure_requires_selector',true],['entry:exact_load_owns_closure',1],
  ['entry-ambiguous:two_candidates',2],['entry-ambiguous:no_arbitrary_execution',true],['entry-exact-a:chosen_body_call_count',2],['entry-exact-b:chosen_body_call_count',0],
  ['entry-ambiguous:target_identity_distinct',true],['entry-capture:instance_selector_required',true],
  ['entry-capture-a:owns_exact_capture_cells',true],['entry-capture-a:returns_owned_capture','Source'],['entry-capture-b:owns_exact_capture_cells',true],['entry-capture-b:returns_owned_capture','Pair'],
  ['known-order-ab:selected_wrap_return','nil'],['known-order-ab:phase_does_not_erase_known_order',true],['known-order-ba:selected_wrap_return','source'],['known-order-ba:phase_does_not_erase_known_order',true],['known-order:identity_distinct',true],
  ['unknown-order:knowledge_boundary',true],['unknown-order:no_fabricated_load',true],['unknown-order:both_definition_candidates',true],['unknown-order:syntax_calls_retained',3],['unknown-order:provider_rejects_incomplete',true],['unknown-order:identity_distinct',true],
 ])add(name,value);
 return rows;
}
const sha=value=>createHash('sha256').update(value).digest('hex');
export function verifyEntryRuntime(root,frames){
 const rows=frames.filter(row=>row.event==='execution_scenario'),checks=[];
 const check=(name,expected,actual)=>{let passed=true,error;try{assert.deepEqual(actual,expected);}catch(e){passed=false;error=e.message;}checks.push({name,expected,actual,passed,error});};
 check('exact execution cases',entryCases,rows.map(row=>row.case));
 for(const row of rows){
  const name=row.case,g=row.graph,n=g.native_invocations,h=g.host_runtime,a=g.snapshot_inputs.admission;
  check(name+':input execution disabled',false,row.sourceExecuted);
  check(name+':snapshot digest',sha(g.snapshot_inputs_json),g.snapshot_key);
  check(name+':load-plan evidence',true,!!a.loadPlan);
  const sources=new Map(a.files.map(file=>[file.file,readFileSync(join(root,'tests/rust-source-engine/wow/native',file.file))]));
  for(const file of a.files){const bytes=sources.get(file.file);check(name+':input:'+file.file,file.sha256,sha(bytes));}
  const real=site=>{const bytes=sources.get(site?.file);return !!bytes&&Number.isInteger(site.start)&&Number.isInteger(site.end)&&site.start>=0&&site.end>site.start&&site.end<=bytes.length&&Buffer.byteLength(bytes.subarray(0,site.start).toString('utf8'))===site.start&&Buffer.byteLength(bytes.subarray(0,site.end).toString('utf8'))===site.end;};
  check(name+':real call sites',true,g.calls.every(call=>real(call.site)));
  check(name+':real use sites',true,g.uses.every(use=>real(use.site)));
  for(const candidate of row.candidates??[]){
   check(name+':candidate real:'+candidate.id,true,real(candidate.closure));
   const file=a.files.find(file=>file.file===candidate.closure.file);
   // Candidate digest serializes the typed Rust Site in declaration order;
   // the enclosing emitted JSON Value sorts object keys independently.
   const site={file:candidate.closure.file,start:candidate.closure.start,end:candidate.closure.end};
   check(name+':candidate source identity:'+candidate.id,sha(JSON.stringify(['source-explicit-entry-load-plan-v2',file,site])),candidate.id);
  }
  const entries=n.entries??[];
  if(name==='unknown-order'){
   check(name+':no arbitrary effects',[0,0,0,0,0],[g.calls.length,g.writes.length,n.frames.length,n.control.occurrences.length,g.script_load_occurrences.length]);
   check(name+':honest coverage',[false,false,'unavailable_execution_plan'],[n.control.complete,n.control.truncated,n.control.coverage]);
   check(name+':knowledge boundary',[{kind:'knowledge',reason:'load_order_unknown',load_plan:a.loadPlan}],n.execution_boundaries);
   check(name+':no implicit entry',0,entries.length);
   check(name+':syntax only calls',true,row.syntaxCalls.rows.length===3&&row.syntaxCalls.rows.every(call=>call.evidence_kind==='syntactic_call_site'));
  }else if(name==='entry-ambiguous'){
   check(name+':selector only',[2,0,0],[(row.candidates??[]).length,entries.length,g.calls.length]);
  }else if(name.startsWith('known-order')){
   const expected=name==='known-order-ab'?'host-order-b.lua':'host-order-a.lua';
   const call=g.calls.find(call=>call.site.file==='host-order-main.lua'&&sources.get(call.site.file).subarray(call.site.start,call.site.end).toString('utf8')==='Wrap(Source())');
   const frame=n.frames.find(frame=>frame.call===call?.id);
   check(name+':selected implementation',expected,g.functions[frame?.function]?.closure.file);
   check(name+':known plan and unknown phase',['known','unknown',true],[a.loadPlan.kind,g.phase,n.control.complete]);
  }else{
   check(name+':one explicit request',1,entries.length);
   const entry=entries[0],frame=n.frames[entry?.frame];if(!entry||!frame)continue;
   check(name+':request authenticated',a.explicitEntry,entry.request);
   check(name+':not a fake call',null,frame.call);
   check(name+':origin',{kind:'explicit_entry',entry:entry.id},frame.origin);
   check(name+':closure owner',entry.closure,frame.closure);
   check(name+':capture ownership',n.closures[entry.closure].captures,frame.captures);
   check(name+':frame returns',entry.return_facts,frame.return_facts);
   check(name+':no invented argument use',true,entry.parameters.every(binding=>binding.argument_fact===null));
  check(name+':entry capability','source-explicit-entry-load-plan-v2',h.profile.explicitEntryCapability);
   check(name+':entry request profile',true,h.profile.entryRequest);
   check(name+':same host identity',h.profile,a.rules.hostRuntime);
   const calls=g.calls.filter(call=>n.query_views[call.context]?.kind==='entry');
   check(name+':all entry calls same frame',true,calls.every(call=>n.query_views[call.context].frame===frame.id));
   check(name+':body calls use common slots',true,calls.every(call=>h.calls.some(slot=>slot.call===call.id&&slot.view.kind==='entry'&&slot.view.frame===frame.id)));
   if(row.entryCalls)check(name+':facade uses entry',true,row.entryCalls.rows.every(call=>call.occurrence.kind==='entry'&&call.occurrence.frame===frame.id));
   if(name==='entry'||name==='entry-empty'||name==='entry-literal'){
    const request={candidate:row.candidates[0].id,load_occurrence:null,creation_context:null,arguments:name==='entry'?{kind:'unspecified'}:{kind:'exact',values:name==='entry-empty'?[]:[{kind:'boolean',value:true}]}};
    check(name+':exact parameter contract',request,entry.request);
    check(name+':formal declaration bytes','value',sources.get(entry.parameters[0].parameter.declaration.file).subarray(entry.parameters[0].parameter.declaration.start,entry.parameters[0].parameter.declaration.end).toString('utf8'));
    if(name==='entry-literal'){
     const value=g.values[entry.parameters[0].selected_value.detail];
     check(name+':argument origin',{entry:entry.id,slot:1},value.origin);
    }
   }
  }
 }
 return{schema:'lycheedev.source-entry-runtime-independent.v1',passed:checks.every(check=>check.passed),checks,scope:'explicit abstract source entry and unknown-order knowledge boundary; no secrecy-flow or production API acceptance'};
}
