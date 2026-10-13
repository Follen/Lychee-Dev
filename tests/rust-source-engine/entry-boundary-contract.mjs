// Independent checks for interrupted entry prefixes and conditional closure existence.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
export const boundaryFiles=['host-entry-depth.lua','host-entry-depth-call.lua','host-entry-conditional.lua','host-entry-unreachable.lua','host-entry-vararg.lua'];
export function boundaryInputs(root){return boundaryFiles.map(file=>{const diskPath='tests/rust-source-engine/wow/native/'+file,bytes=readFileSync(join(root,diskPath));return{file,diskPath,bytes:bytes.length,sha256:sha(bytes)};});}
export function boundaryAssertions(frames,root=process.cwd()){
 const expected=[],add=(name,value)=>expected.push({name:'boundary:'+name,expected:value});
 for(const name of ['depth-source-call','depth-entry'])for(const[key,value]of [
  ['verified_prefix','verified_prefix_with_explicit_boundaries'],['budget_incomplete_truncated',true],['real_boundary_occurrence',true],['prefix_host_calls',['Source','Use']],['suffix_not_evaluated',true],['frame_effects_incomplete',true],['prefix_writes_retained',true],['complete_provider_rejected',true],['prefix_query_coverage','verified_prefix_with_explicit_boundaries'],['entry_completion_honest',true],
 ])add(name+':'+key,value);
 const row=frames?.find(frame=>frame.event==='boundary_scenario'&&frame.case==='conditional-entry'),g=row?.graph,n=g?.native_invocations;
 const source=readFileSync(join(root,'tests/rust-source-engine/wow/native/host-entry-conditional.lua'));
 const flag=g?.uses.find(use=>source.subarray(use.site.start,use.site.end).toString('utf8')==='Flag');
 const frame=n?.frames.find(frame=>frame.origin?.kind==='explicit_entry'),cell=Object.values(frame?.captures??{})[0];
 const latest=g?.writes.findLast(write=>write.cell===cell);
 for(const[key,value]of [
  ['existence_assumption',true],['creation_loop_evidence',true],['no_old_control_state',true],['body_preserves_predicate',[[{condition:flag?.id,outcome:true}]]],['body_preserves_branch',n?.entries[0]?.assumption.conditional_branch],['latest_capture_value',true],['latest_capture_write',[latest?.id]],['no_creation_heap_restore',true],['conditional_query_evidence',true],['bounded_assumption_complete',true],
 ])add('conditional-entry:'+key,value);
 for(const[name,value]of [['unreachable-entry:syntax_candidate_retained',true],['unreachable-entry:no_loaded_closure',true],['unreachable-entry:selection_rejected',true],['unsupported-entry:not_budget_prefix',true],['entry:cancel_priority',7],['entry:deadline_priority',5]])add(name,value);
 return expected;
}
export function verifyEntryBoundaries(root,frames){
 const checks=[],check=(name,expected,actual)=>{let passed=true,error;try{assert.deepEqual(actual,expected);}catch(e){passed=false;error=e.message;}checks.push({name,expected,actual,passed,error});};
 const rows=frames.filter(frame=>frame.event==='boundary_scenario');
 check('boundary cases',['depth-source-call','depth-entry','conditional-entry','unreachable-entry'],rows.map(row=>row.case));
 const typedSite=site=>({file:site.file,start:site.start,end:site.end});
 for(const row of rows){
  const name=row.case,g=row.graph,n=g.native_invocations,h=g.host_runtime;
  const sources=new Map(g.snapshot_inputs.admission.files.map(file=>[file.file,readFileSync(join(root,'tests/rust-source-engine/wow/native',file.file))]));
  const text=site=>sources.get(site.file)?.subarray(site.start,site.end).toString('utf8');
  const real=site=>sources.has(site?.file)&&Number.isInteger(site.start)&&Number.isInteger(site.end)&&site.start>=0&&site.end>site.start&&site.end<=sources.get(site.file).length;
  check(name+':source execution disabled',false,row.sourceExecuted);
  check(name+':snapshot digest',sha(g.snapshot_inputs_json),g.snapshot_key);
  check(name+':source sites',true,g.uses.every(use=>real(use.site))&&g.calls.every(call=>real(call.site)));
  if(name.startsWith('depth-')){
   check(name+':exact depth reason',['native block depth budget exceeded'],n.control.boundaries.map(boundary=>boundary.reason));
   check(name+':resource classification',[false,true,'budget'],[n.control.complete,n.control.truncated,n.control.boundaries[0]?.kind]);
   check(name+':source body fixed nesting',65,[...sources.get('host-entry-depth.lua').toString('utf8').matchAll(/^    do$/gm)].length);
   check(name+':real frontier site',true,n.control.boundaries.every(boundary=>real(boundary.site)&&text(boundary.site).startsWith('do')&&JSON.stringify(boundary.site)===JSON.stringify(n.control.occurrences[boundary.occurrence]?.site)));
   check(name+':prefix calls',['Source','Use'],h.calls.map(call=>h.bindings[call.host_binding]?.declaration.name));
   check(name+':no suffix calls',false,g.calls.some(call=>text(call.site)==='Use(Source())'));
   const frame=n.frames.find(frame=>frame.closure!=null),write=g.writes.find(write=>text(write.site)==='prefix');
   check(name+':concrete effects unavailable',false,frame?.effects_complete);
   check(name+':no final returns',[],frame?.return_facts);
   const effects=name==='depth-entry'?n.entries?.[0]?.effect_write_versions:g.calls[frame?.call]?.effect_write_versions;
   check(name+':actual prefix local write',true,effects?.includes(write?.id));
   check(name+':query is prefix','verified_prefix_with_explicit_boundaries',row.prefixCalls.coverage);
   if(name==='depth-entry')check(name+':entry effects incomplete',false,n.entries?.[0]?.effects_complete);
  }
  if(name==='conditional-entry'){
   const entry=n.entries[0],frame=n.frames[entry.frame],assumption=entry.assumption;
   const closure=n.closures[entry.closure],creation=n.control.occurrences[assumption.creation_occurrence];
   const flag=g.uses.find(use=>text(use.site)==='Flag'),predicate=[[{condition:flag?.id,outcome:true}]];
   check(name+':explicit assumption rule','selected_closure_exists',assumption.rule);
   check(name+':source creation context',closure.creation_context,assumption.creation_context);
   check(name+':creation paths retained',predicate,assumption.paths);
   check(name+':creation occurrence agrees',predicate,creation.paths);
   check(name+':creation happened in loop',1,creation.loops.length);
   const occurrences=n.control.occurrences.filter(occurrence=>occurrence.frame===frame.id);
   check(name+':entry starts new control ancestry',null,occurrences[0]?.parent);
   check(name+':entry is not still in creation loop',true,occurrences.every(occurrence=>occurrence.loops.length===0));
   check(name+':entry inherits predicates only',true,occurrences.every(occurrence=>JSON.stringify(occurrence.paths)===JSON.stringify(predicate)));
   const calls=g.calls.filter(call=>n.query_views[call.context]?.kind==='entry');
   check(name+':one body call',1,calls.length);
   check(name+':body branch provenance',assumption.conditional_branch,calls[0]?.conditional_branch);
   const argument=g.uses[calls[0]?.arguments[0]],cell=Object.values(frame.captures)[0],latest=g.writes.findLast(write=>write.cell===cell);
   check(name+':latest captured cell write',[latest.id],argument?.writes);
   const nil=decision=>decision?.state==='known_absent'||decision?.state==='bound'&&g.values[decision.detail]?.kind.kind==='primitive'&&g.values[decision.detail]?.kind.detail==='nil';
   check(name+':no creation heap restore',true,nil(argument?.decision)&&entry.return_facts.every(fact=>nil(g.uses[fact].decision)));
   check(name+':latest write is after declaration',true,latest.site.start>g.functions[frame.function].closure.end);
   check(name+':conditional source query','conditional_abstract_call',row.entryCalls.rows[0]?.evidence_kind);
   const request={candidate:entry.request.candidate,load_occurrence:entry.request.load_occurrence,creation_context:entry.request.creation_context,arguments:{kind:entry.request.arguments.kind,values:entry.request.arguments.values}};
   const typedAssumption={rule:assumption.rule,creation_context:assumption.creation_context,creation_occurrence:assumption.creation_occurrence,paths:assumption.paths,conditional_branch:assumption.conditional_branch};
   check(name+':entry proof',sha(JSON.stringify([g.snapshot_key,'explicit_entry',entry.id,request,entry.closure,entry.argument_values,typedAssumption])),entry.proof_id);
  }
  if(name==='unreachable-entry'){
   check(name+':no imagined loaded closure',[0,0],[n.closures.length,g.calls.length]);
   check(name+':unreachable selection rejected',true,row.error.includes('requires one loaded closure instance; candidates=[]'));
   const file=g.snapshot_inputs.admission.files.find(file=>file.file===row.candidate.closure.file);
   check(name+':syntax candidate remains authenticated',sha(JSON.stringify(['source-explicit-entry-load-plan-v2',file,typedSite(row.candidate.closure)])),row.candidate.id);
  }
 }
 return{schema:'lycheedev.source-entry-boundaries-independent.v1',passed:checks.every(check=>check.passed),checks};
}
