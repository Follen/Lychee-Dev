// Additional cases; the original twelve cases and 144 assertions stay intact.
const ENTRY_SOURCE:&str=include_str!("../../../../tests/rust-source-engine/wow/native/host-entry.lua");
const AMBIGUOUS_SOURCE:&str=include_str!("../../../../tests/rust-source-engine/wow/native/host-entry-ambiguous.lua");
const ORDER_SOURCES:&[(&str,&str)]=&[
 ("host-order-a.lua",include_str!("../../../../tests/rust-source-engine/wow/native/host-order-a.lua")),
 ("host-order-b.lua",include_str!("../../../../tests/rust-source-engine/wow/native/host-order-b.lua")),
 ("host-order-main.lua",include_str!("../../../../tests/rust-source-engine/wow/native/host-order-main.lua")),
];
fn entry_control()->OperationControl{OperationControl::new(Instant::now()+Duration::from_secs(30),Arc::new(AtomicBool::new(false)))}
fn unique_entry(prepared:&PreparedSnapshot,file:&str,line:usize,c:&OperationControl)->Result<EntryCandidate>{
 match prepared.entry_candidates(file,line,c).map_err(|error|error.to_string())?{EntryResolution::Unique{candidate}=>Ok(candidate),_=>Err("unique entry candidate expected".into())}
}
fn entry_request(candidate:&EntryCandidate,arguments:EntryArguments)->EntryRequest{EntryRequest{candidate:candidate.id.clone(),load_occurrence:None,creation_context:None,arguments}}
fn run_entry_cases(out:&mut Output)->Result<()> {
 let c=entry_control();let prepared=prepare_case("host-entry.lua",ENTRY_SOURCE,false,&c)?;
 let candidate=unique_entry(&prepared,"host-entry.lua",1,&c)?;
 let mut keys=Vec::new();
 for (name,arguments,expected) in [
  ("entry",EntryArguments::Unspecified,"unavailable"),
  ("entry-empty",EntryArguments::Exact(vec![]),"known_absent"),
  ("entry-literal",EntryArguments::Exact(vec![EntryArgument::Boolean(true)]),"bound"),
 ]{
  let request=entry_request(&candidate,arguments);
  let graph=prepared.build_plan(name,"after_load",LoadPlan::Known(vec!["host-entry.lua".into()]),Some(request.clone()),Some("explicit-entry-v1"),&c).map_err(|error|error.to_string())?;
  keys.push(graph.snapshot_key.clone());
  let native=graph.native_invocations.as_ref().ok_or("entry native facts")?;let entry=native.entries.first().ok_or("entry fact")?;
  let frame=native.frames.get(entry.frame.ok_or("entry frame")?).ok_or("entry frame missing")?;
  let host=graph.host_runtime.as_ref().ok_or("entry host facts")?;
  let selected=|expression:&str|->Result<&CommonCallSlots>{host.calls.iter().find(|call|matches!(call.view,NativeQueryView::Entry{..})&&graph.calls[call.call].site.text(prepared.inputs()).is_ok_and(|text|text==expression)).ok_or_else(||format!("entry call missing: {expression}"))};
  out.check(&format!("execution:{name}:complete"),json!(true),json!(native.control.complete))?;
  out.check(&format!("execution:{name}:real_entry_origin"),json!(true),json!(frame.call.is_none()&&frame.closure==Some(entry.closure)&&matches!(frame.origin,Some(NativeInvocationOrigin::ExplicitEntry{entry:0}))&&native.entries.len()==1))?;
  out.check(&format!("execution:{name}:source_body_calls_only"),json!(3),json!(graph.calls.iter().filter(|call|matches!(native.query_views.get(&call.context),Some(NativeQueryView::Entry{..}))).count()))?;
  out.check(&format!("execution:{name}:formal_argument_state"),json!(expected),serde_json::to_value(&entry.parameters[0].selected_value).map_err(|error|error.to_string())?["state"].clone())?;
  let source=selected("Source()")?.returns[0].selected.clone();
  out.check(&format!("execution:{name}:source_assignment_to_use"),json!(true),json!(selected("Use(x)")?.arguments[0].selected==source))?;
  out.check(&format!("execution:{name}:formal_to_use"),json!(true),json!(selected("Use(value)")?.arguments[0].selected==entry.parameters[0].selected_value))?;
  out.check(&format!("execution:{name}:return_source_slot"),json!(true),json!(entry.return_facts.len()==1&&graph.uses[entry.return_facts[0]].decision==source))?;
  let syntax=prepared.source_queries(&c).map_err(|error|error.to_string())?;
  let queries=syntax.with_execution(&graph,QuerySelector::Entry{frame:frame.id},&c).map_err(|error|error.to_string())?;
  let calls=queries.calls(QueryTarget::All,PageRequest::default()).map_err(|error|error.to_string())?;
  out.check(&format!("execution:{name}:entry_query_view"),json!(true),json!(calls.rows.len()==3&&calls.rows.iter().all(|call|call.occurrence==QuerySelector::Entry{frame:frame.id})))?;
  let fact=&graph.uses[graph.calls[selected("Use(value)")?.call].arguments[0]];
  let site=prepared.binding_site(&fact.site)?;
  let ty=prepared.query(&graph,NativeViewSelection::Entry(frame.id),&c,|analysis,session,control|{control.check()?;let model=analysis.semantic_model_with_context(site.file_id,session);Ok(format!("{:?}",model.type_of_expr(site.syntax)))}).map_err(|error|error.to_string())?;
  out.check(&format!("execution:{name}:native_entry_provider"),json!(if name=="entry-empty"{"Nil"}else{"Unknown"}),json!(ty))?;
  out.check(&format!("execution:{name}:request_in_snapshot"),json!(request),graph.snapshot_inputs.admission()["explicitEntry"].clone())?;
  out.emit(json!({"event":"execution_scenario","case":name,"graph":graph,"candidates":[candidate],"entryCalls":calls,"nativeType":ty,"sourceExecuted":false}))?;
 }
 out.check("execution:entry:argument_identity_distinct",json!(3),json!(keys.iter().collect::<std::collections::BTreeSet<_>>().len()))?;
 out.check("execution:entry:invalid_line_rejected",json!(4),json!(prepared.entry_candidates("host-entry.lua",0,&c).err().map(|error|error.exit_code)))?;
 let duplicate=prepared.build_plan("entry-duplicate","after_load",LoadPlan::Known(vec!["host-entry.lua".into(),"host-entry.lua".into()]),Some(entry_request(&candidate,EntryArguments::Unspecified)),None,&c);
 out.check("execution:entry:duplicate_closure_requires_selector",json!(true),json!(duplicate.is_err_and(|error|error.to_string().contains("requires one loaded closure instance"))))?;
 let mut request=entry_request(&candidate,EntryArguments::Exact(vec![]));request.load_occurrence=Some(1);
 let graph=prepared.build_plan("entry-selected-load","after_load",LoadPlan::Known(vec!["host-entry.lua".into(),"host-entry.lua".into()]),Some(request),None,&c).map_err(|error|error.to_string())?;
 let native=graph.native_invocations.as_ref().ok_or("selected load facts")?;
 out.check("execution:entry:exact_load_owns_closure",json!(1),json!(native.closures[native.entries[0].closure].load_occurrence))?;

 let c=entry_control();let prepared=prepare_case("host-entry-ambiguous.lua",AMBIGUOUS_SOURCE,false,&c)?;
 let resolution=prepared.entry_candidates("host-entry-ambiguous.lua",1,&c).map_err(|error|error.to_string())?;
 let EntryResolution::SelectorRequired{candidates}=resolution else{return Err("line ambiguity not preserved".into())};
 out.check("execution:entry-ambiguous:two_candidates",json!(2),json!(candidates.len()))?;
 let graph=prepared.build_plan("entry-ambiguous","after_load",LoadPlan::Known(vec!["host-entry-ambiguous.lua".into()]),None,None,&c).map_err(|error|error.to_string())?;
 out.check("execution:entry-ambiguous:no_arbitrary_execution",json!(true),json!(graph.calls.is_empty()&&graph.native_invocations.as_ref().is_some_and(|native|native.entries.is_empty())))?;
 out.emit(json!({"event":"execution_scenario","case":"entry-ambiguous","graph":graph,"candidates":candidates,"sourceExecuted":false}))?;
 let mut keys=Vec::new();
 for (index,name,count) in [(0,"entry-exact-a",2),(1,"entry-exact-b",0)]{
  let graph=prepared.build_plan(name,"after_load",LoadPlan::Known(vec!["host-entry-ambiguous.lua".into()]),Some(entry_request(&candidates[index],EntryArguments::Exact(vec![]))),None,&c).map_err(|error|error.to_string())?;
  keys.push(graph.snapshot_key.clone());let native=graph.native_invocations.as_ref().ok_or("exact native facts")?;let frame=native.entries[0].frame.ok_or("exact frame")?;
  out.check(&format!("execution:{name}:chosen_body_call_count"),json!(count),json!(graph.calls.iter().filter(|call|matches!(native.query_views.get(&call.context),Some(NativeQueryView::Entry{..}))).count()))?;
  out.check(&format!("execution:{name}:exact_source_closure"),json!(candidates[index].closure),json!(graph.functions[native.frames[frame].function].closure))?;
  out.emit(json!({"event":"execution_scenario","case":name,"graph":graph,"candidates":candidates,"sourceExecuted":false}))?;
 }
 out.check("execution:entry-ambiguous:target_identity_distinct",json!(true),json!(keys[0]!=keys[1]))?;

 let c=entry_control();let source=include_str!("../../../../tests/rust-source-engine/wow/native/host-transfers.lua");let prepared=prepare_case("host-transfers.lua",source,false,&c)?;
 let resolution=prepared.entry_candidates("host-transfers.lua",7,&c).map_err(|error|error.to_string())?;
 let EntryResolution::SelectorRequired{candidates}=resolution else{return Err("nested closure line must keep both candidates".into())};
 let candidate=candidates.iter().min_by_key(|candidate|candidate.closure.end-candidate.closure.start).ok_or("capture candidate")?;
 let baseline=prepared.build("entry-capture-baseline",EvaluatorProfile::Native,None,&c).map_err(|error|error.to_string())?;
 let native=baseline.native_invocations.as_ref().ok_or("capture baseline facts")?;
 let instances=native.closures.iter().filter(|closure|baseline.functions[closure.function].closure==candidate.closure).collect::<Vec<_>>();
 let duplicate=prepared.build_plan("entry-capture-ambiguous","after_load",LoadPlan::Known(vec!["host-transfers.lua".into()]),Some(entry_request(candidate,EntryArguments::Exact(vec![]))),None,&c);
 out.check("execution:entry-capture:instance_selector_required",json!(true),json!(duplicate.is_err_and(|error|error.to_string().contains("requires one loaded closure instance"))))?;
 for (index,name,expected) in [(0,"entry-capture-a","Source"),(1,"entry-capture-b","Pair")]{
  let mut request=entry_request(candidate,EntryArguments::Exact(vec![]));request.creation_context=Some(instances.get(index).ok_or("factory closure instance")?.creation_context.clone());
  let graph=prepared.build_plan(name,"after_load",LoadPlan::Known(vec!["host-transfers.lua".into()]),Some(request),None,&c).map_err(|error|error.to_string())?;
  let native=graph.native_invocations.as_ref().ok_or("capture entry facts")?;let entry=&native.entries[0];let frame=&native.frames[entry.frame.ok_or("capture entry frame")?];
  out.check(&format!("execution:{name}:owns_exact_capture_cells"),json!(true),json!(frame.captures==native.closures[entry.closure].captures&&!frame.captures.is_empty()))?;
  let selected=&graph.uses[entry.return_facts[0]].decision;let host=graph.host_runtime.as_ref().ok_or("capture host")?;
  let actual=match selected{Decision::Bound(value)=>match graph.values[*value].kind{ValueKind::HostReturn{binding,..}=>host.bindings[binding].declaration.name(),_=>"other"},_=>"unknown"};
  out.check(&format!("execution:{name}:returns_owned_capture"),json!(expected),json!(actual))?;
  out.emit(json!({"event":"execution_scenario","case":name,"graph":graph,"candidates":candidates,"sourceExecuted":false}))?;
 }

 let c=entry_control();let prepared=prepare_files(ORDER_SOURCES,false,&c)?;let mut keys=Vec::new();
 for (name,order,expected) in [
  ("known-order-ab",vec!["host-order-a.lua".into(),"host-order-b.lua".into(),"host-order-main.lua".into()],"nil"),
  ("known-order-ba",vec!["host-order-b.lua".into(),"host-order-a.lua".into(),"host-order-main.lua".into()],"source"),
 ]{
  let graph=prepared.build_plan(name,"unknown",LoadPlan::Known(order),None,None,&c).map_err(|error|error.to_string())?;
  keys.push(graph.snapshot_key.clone());let host=graph.host_runtime.as_ref().ok_or("ordered host facts")?;
  let use_=host.calls.iter().find(|call|call.host_binding.is_some_and(|binding|host.bindings[binding].declaration.name()=="Use")).ok_or("ordered consumer")?;
  let selected=&use_.arguments[0].selected;
  let actual=match selected{Decision::KnownAbsent(_)=>"nil",Decision::Bound(value)=>match &graph.values[*value].kind{ValueKind::Primitive(kind) if kind=="nil"=>"nil",ValueKind::HostReturn{binding,..} if host.bindings[*binding].declaration.name()=="Source"=>"source",_=>"other"},_=>"unknown"};
  out.check(&format!("execution:{name}:selected_wrap_return"),json!(expected),json!(actual))?;
  out.check(&format!("execution:{name}:phase_does_not_erase_known_order"),json!(true),json!(graph.phase=="unknown"&&graph.native_invocations.as_ref().is_some_and(|native|native.control.complete)&&graph.script_load_occurrences.len()==3))?;
  out.emit(json!({"event":"execution_scenario","case":name,"graph":graph,"sourceExecuted":false}))?;
 }
 out.check("execution:known-order:identity_distinct",json!(true),json!(keys[0]!=keys[1]))?;
 let files=ORDER_SOURCES.iter().map(|(file,_)|(*file).into()).collect();
 let graph=prepared.build_plan("unknown-order","after_load",LoadPlan::Unknown(files),None,None,&c).map_err(|error|error.to_string())?;
 let native=graph.native_invocations.as_ref().ok_or("unknown order native facts")?;
 out.check("execution:unknown-order:knowledge_boundary",json!(true),json!(!native.control.complete&&!native.control.truncated&&native.execution_boundaries.len()==1&&native.execution_boundaries[0].reason=="load_order_unknown"))?;
 out.check("execution:unknown-order:no_fabricated_load",json!(true),json!(graph.script_load_occurrences.is_empty()&&graph.calls.is_empty()&&graph.writes.is_empty()&&native.frames.is_empty()&&native.control.occurrences.is_empty()))?;
 let declarations=[unique_entry(&prepared,"host-order-a.lua",1,&c)?,unique_entry(&prepared,"host-order-b.lua",1,&c)?];
 out.check("execution:unknown-order:both_definition_candidates",json!(true),json!(declarations[0].id!=declarations[1].id))?;
 let syntax_calls=prepared.source_queries(&c).and_then(|query|query.calls(QueryTarget::All,PageRequest::default())).map_err(|error|error.to_string())?;
 out.check("execution:unknown-order:syntax_calls_retained",json!(3),json!(syntax_calls.rows.len()))?;
 out.check("execution:unknown-order:provider_rejects_incomplete",json!(true),json!(prepared.freeze(&graph,NativeViewSelection::SourceTemplates,&c).is_err()))?;
 out.check("execution:unknown-order:identity_distinct",json!(true),json!(keys.iter().all(|key|key!=&graph.snapshot_key)))?;
 out.emit(json!({"event":"execution_scenario","case":"unknown-order","graph":graph,"candidates":declarations,"syntaxCalls":syntax_calls,"sourceExecuted":false}))?;
 Ok(())
}
