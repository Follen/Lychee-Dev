// Deliberately separate output mode: the 201-check regression stays within 2 MiB.
const DEPTH_SOURCE:&str=include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-depth.lua");
const DEPTH_CALL:&str=include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-depth-call.lua");
const CONDITIONAL_ENTRY:&str=include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-conditional.lua");
const UNREACHABLE_ENTRY:&str=include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-unreachable.lua");
const VARARG_ENTRY:&str=include_str!("../../../../tests/fixtures/source-native/wow/native/host-entry-vararg.lua");
fn boundary_nil(graph:&Graph,decision:&Decision)->bool{matches!(decision,Decision::KnownAbsent(_))||matches!(decision,Decision::Bound(value) if matches!(&graph.values[*value].kind,ValueKind::Primitive(kind) if kind=="nil"))}
fn run_entry_boundaries(out:&mut Output)->Result<()> {
 out.emit(json!({"event":"build_identity","engineCommit":ENGINE_COMMIT,"engineArchiveSHA256":ENGINE_ARCHIVE_SHA256}))?;
 for explicit in [false,true]{
  let name=if explicit{"depth-entry"}else{"depth-source-call"};let c=entry_control();
  let prepared=prepare_files(&[("host-entry-depth.lua",DEPTH_SOURCE),("host-entry-depth-call.lua",DEPTH_CALL)],false,&c)?;
  let candidate=unique_entry(&prepared,"host-entry-depth.lua",1,&c)?;
  let (files,request)=if explicit{(vec!["host-entry-depth.lua".into()],Some(entry_request(&candidate,EntryArguments::Exact(vec![]))))}else{(vec!["host-entry-depth.lua".into(),"host-entry-depth-call.lua".into()],None)};
  let graph=prepared.build_plan(name,"after_load",LoadPlan::Known(files),request,None,&c).map_err(|error|error.to_string())?;
  let native=graph.native_invocations.as_ref().ok_or("depth native prefix")?;let host=graph.host_runtime.as_ref().ok_or("depth host prefix")?;
  let frame=native.frames.iter().find(|frame|frame.closure.is_some()).ok_or("depth concrete frame")?;
  let boundary=native.control.boundaries.first().ok_or("depth budget boundary")?;
  out.check(&format!("boundary:{name}:verified_prefix"),json!("verified_prefix_with_explicit_boundaries"),json!(native.control.coverage))?;
  out.check(&format!("boundary:{name}:budget_incomplete_truncated"),json!(true),json!(!native.control.complete&&native.control.truncated&&matches!(boundary.kind,NativeBoundaryKind::Budget)&&boundary.reason=="native block depth budget exceeded"))?;
  out.check(&format!("boundary:{name}:real_boundary_occurrence"),json!(true),json!(boundary.site.text(prepared.inputs()).is_ok_and(|text|text.starts_with("do"))&&boundary.occurrence.and_then(|id|native.control.occurrences.get(id)).is_some_and(|occurrence|occurrence.site==boundary.site&&occurrence.context==boundary.context&&occurrence.frame==Some(frame.id))))?;
  let prefix=host.calls.iter().filter_map(|call|call.host_binding.map(|binding|host.bindings[binding].declaration.name())).collect::<Vec<_>>();
  out.check(&format!("boundary:{name}:prefix_host_calls"),json!(["Source","Use"]),json!(prefix))?;
  out.check(&format!("boundary:{name}:suffix_not_evaluated"),json!(true),json!(graph.calls.iter().all(|call|call.site.text(prepared.inputs()).is_ok_and(|text|text!="Use(Source())"))))?;
  out.check(&format!("boundary:{name}:frame_effects_incomplete"),json!(true),json!(!frame.effects_complete&&frame.return_facts.is_empty()))?;
  let write=graph.writes.iter().find(|write|write.site.text(prepared.inputs()).is_ok_and(|text|text=="prefix")).ok_or("verified prefix write")?;
  let effects=if explicit{&native.entries[0].effect_write_versions}else{&graph.calls[frame.call.ok_or("source frame call")?].effect_write_versions};
  out.check(&format!("boundary:{name}:prefix_writes_retained"),json!(true),json!(effects.contains(&write.id)&&matches!(write.decision,Decision::Bound(value) if matches!(graph.values[value].kind,ValueKind::HostReturn{..}))))?;
  let selection=if explicit{NativeViewSelection::Entry(frame.id)}else{NativeViewSelection::Invocation(frame.id)};
  out.check(&format!("boundary:{name}:complete_provider_rejected"),json!(true),json!(prepared.freeze(&graph,selection,&c).is_err()))?;
  let queries=prepared.source_queries(&c).and_then(|query|query.with_execution(&graph,QuerySelector::AllOccurrences,&c)).map_err(|error|error.to_string())?;
  let calls=queries.calls(QueryTarget::All,PageRequest::default()).map_err(|error|error.to_string())?;
  out.check(&format!("boundary:{name}:prefix_query_coverage"),json!("verified_prefix_with_explicit_boundaries"),json!(calls.coverage))?;
  let entry_honest=if explicit{native.entries.len()==1&&!native.entries[0].effects_complete&&native.entries[0].return_facts.is_empty()}else{native.entries.is_empty()};
  out.check(&format!("boundary:{name}:entry_completion_honest"),json!(true),json!(entry_honest))?;
  out.emit(json!({"event":"boundary_scenario","case":name,"graph":graph,"prefixCalls":calls,"sourceExecuted":false}))?;
 }
 let c=entry_control();let prepared=prepare_case("host-entry-conditional.lua",CONDITIONAL_ENTRY,false,&c)?;
 let candidate=unique_entry(&prepared,"host-entry-conditional.lua",4,&c)?;
 let graph=prepared.build_plan("conditional-entry","after_load",LoadPlan::Known(vec!["host-entry-conditional.lua".into()]),Some(entry_request(&candidate,EntryArguments::Exact(vec![]))),None,&c).map_err(|error|error.to_string())?;
 let native=graph.native_invocations.as_ref().ok_or("conditional native")?;let entry=&native.entries[0];let frame=&native.frames[entry.frame.ok_or("conditional frame")?];
 let creation=&native.control.occurrences[entry.assumption.creation_occurrence.ok_or("conditional creation occurrence")?];
 let flag=graph.uses.iter().find(|fact|fact.site.text(prepared.inputs()).is_ok_and(|text|text=="Flag")).ok_or("Flag condition")?;
 let expected=vec![vec![NativePredicate{condition:flag.id,outcome:true}]];
 out.check("boundary:conditional-entry:existence_assumption",json!(true),json!(entry.assumption.rule=="selected_closure_exists"&&entry.assumption.paths==expected&&entry.assumption.conditional_branch.is_some()))?;
 out.check("boundary:conditional-entry:creation_loop_evidence",json!(true),json!(!creation.loops.is_empty()&&creation.paths==expected))?;
 let first=native.control.occurrences.iter().find(|occurrence|occurrence.frame==Some(frame.id)).ok_or("entry initial occurrence")?;
 out.check("boundary:conditional-entry:no_old_control_state",json!(true),json!(first.parent.is_none()&&first.loops.is_empty()&&first.paths==expected))?;
 let call=graph.calls.iter().find(|call|matches!(native.query_views.get(&call.context),Some(NativeQueryView::Entry{..}))).ok_or("conditional body call")?;
 let occurrence=&native.control.occurrences[native.control.context_occurrences[&call.context]];
 out.check("boundary:conditional-entry:body_preserves_predicate",json!(expected),json!(occurrence.paths))?;
 out.check("boundary:conditional-entry:body_preserves_branch",json!(entry.assumption.conditional_branch),json!(call.conditional_branch))?;
 let argument=&graph.uses[call.arguments[0]];
 out.check("boundary:conditional-entry:latest_capture_value",json!(true),json!(boundary_nil(&graph,&argument.decision)&&entry.return_facts.iter().all(|fact|boundary_nil(&graph,&graph.uses[*fact].decision))))?;
 let cell=frame.captures.values().next().ok_or("conditional capture cell")?;let latest=graph.writes.iter().rev().find(|write|&write.cell==cell).ok_or("latest captured write")?;
 out.check("boundary:conditional-entry:latest_capture_write",json!(vec![latest.id]),json!(argument.writes))?;
 let host=graph.host_runtime.as_ref().ok_or("conditional host")?;
 let old=host.calls.iter().find(|call|call.host_binding.is_some_and(|binding|host.bindings[binding].declaration.name()=="Source")).ok_or("original Source")?;
 out.check("boundary:conditional-entry:no_creation_heap_restore",json!(true),json!(old.returns[0].selected!=argument.decision))?;
 let calls=prepared.source_queries(&c).and_then(|query|query.with_execution(&graph,QuerySelector::Entry{frame:frame.id},&c)).and_then(|query|query.calls(QueryTarget::All,PageRequest::default())).map_err(|error|error.to_string())?;
 out.check("boundary:conditional-entry:conditional_query_evidence",json!(true),json!(calls.rows.len()==1&&calls.rows[0].evidence_kind=="conditional_abstract_call"&&calls.rows[0].conditional_branch==entry.assumption.conditional_branch))?;
 out.check("boundary:conditional-entry:bounded_assumption_complete",json!(true),json!(native.control.complete&&entry.effects_complete))?;
 out.emit(json!({"event":"boundary_scenario","case":"conditional-entry","graph":graph,"entryCalls":calls,"sourceExecuted":false}))?;

 let c=entry_control();let prepared=prepare_case("host-entry-unreachable.lua",UNREACHABLE_ENTRY,false,&c)?;
 let candidate=unique_entry(&prepared,"host-entry-unreachable.lua",3,&c)?;
 out.check("boundary:unreachable-entry:syntax_candidate_retained",json!(true),json!(candidate.closure.file=="host-entry-unreachable.lua"))?;
 let graph=prepared.build("unreachable-load",None,&c).map_err(|error|error.to_string())?;
 out.check("boundary:unreachable-entry:no_loaded_closure",json!(true),json!(graph.native_invocations.as_ref().is_some_and(|native|native.closures.is_empty())&&graph.calls.is_empty()))?;
 let failure=prepared.build_plan("unreachable-entry","after_load",LoadPlan::Known(vec!["host-entry-unreachable.lua".into()]),Some(entry_request(&candidate,EntryArguments::Exact(vec![]))),None,&c).err().ok_or("unreachable entry accepted")?;
 out.check("boundary:unreachable-entry:selection_rejected",json!(true),json!(failure.to_string().contains("requires one loaded closure instance; candidates=[]")))?;
 out.emit(json!({"event":"boundary_scenario","case":"unreachable-entry","graph":graph,"candidate":candidate,"error":failure.to_string(),"sourceExecuted":false}))?;

 let c=entry_control();let prepared=prepare_case("host-entry-vararg.lua",VARARG_ENTRY,false,&c)?;let candidate=unique_entry(&prepared,"host-entry-vararg.lua",1,&c)?;
 let failure=prepared.build_plan("unsupported-entry","after_load",LoadPlan::Known(vec!["host-entry-vararg.lua".into()]),Some(entry_request(&candidate,EntryArguments::Exact(vec![]))),None,&c).err().ok_or("unsupported entry accepted")?;
 out.check("boundary:unsupported-entry:not_budget_prefix",json!(true),json!(failure.to_string().contains("native vararg formal shape unavailable")))?;
 c.tracked_cancel_handle().unwrap().cancel();
 out.check("boundary:entry:cancel_priority",json!(7),json!(prepared.build_plan("cancelled-entry","after_load",LoadPlan::Known(vec!["host-entry-vararg.lua".into()]),Some(entry_request(&candidate,EntryArguments::Exact(vec![]))),None,&c).err().map(|error|error.exit_code)))?;
 let c=OperationControl::uncancelable(Instant::now()+Duration::from_secs(1));
 let prepared=prepare_case("host-entry-vararg.lua",VARARG_ENTRY,false,&c)?;let candidate=unique_entry(&prepared,"host-entry-vararg.lua",1,&c)?;
 std::thread::sleep(c.remaining().map_err(|error|error.to_string())?+Duration::from_millis(2));
 out.check("boundary:entry:deadline_priority",json!(5),json!(prepared.build_plan("expired-entry","after_load",LoadPlan::Known(vec!["host-entry-vararg.lua".into()]),Some(entry_request(&candidate,EntryArguments::Exact(vec![]))),None,&c).err().map(|error|error.exit_code)))?;
 Ok(())
}
