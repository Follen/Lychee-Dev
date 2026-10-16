// Frozen copy of the pre-move build_graph. Scope finalization and XML loading
// call the separately frozen whole-Vec-copy methods; all other implementation
// and types are the same as the candidate. Included only by sharing_tests.
// Source SHA256: d1618420541509af71975b4e448f80259f865475af5ea4b018e93dd92986d3c8
// This temporary differential oracle is not a second production evaluator.
fn graph_move_copying_reference(prepared:&PreparedSnapshot,name:&str,snapshot_inputs:GraphSnapshotEvidence,native_profile:bool,inputs:Arc<SourceInputs>)->Result<Graph> {
    let analysis=&prepared.analysis;let control=&prepared.control;let owner=prepared.owner.clone();
    let phase=inputs.phase.clone();let occurrence_files=inputs.ordered.clone();let ordered:Vec<&str>=occurrence_files.iter().map(String::as_str).collect();
    control.check().map_err(|e|e.to_string())?;
    let mut unique=match &inputs.load_plan{Some(LoadPlan::Unknown(files))=>files.iter().map(String::as_str).collect(),_=>ordered.to_vec()};unique.sort();unique.dedup();
    let snapshot_inputs_json=String::from_utf8(serialize_json(&snapshot_inputs,control,if inputs.corpus.limits.research{32<<20}else{2097152}).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let snapshot_key=controlled_hash(snapshot_inputs_json.as_bytes(),control).map_err(|e|e.to_string())?;
    let harvest=harvest(analysis,&unique,&inputs,control)?;
    let graph=GraphFacts{snapshot_key,snapshot_inputs,snapshot_inputs_json,scenario:name.into(),phase,script_load_occurrences:ordered.iter().enumerate().map(|(i,n)|format!("{i}:{n}")).collect(),values:vec![],functions:vec![],writes:vec![],uses:vec![],calls:vec![],copies:vec![],returns:vec![],branches:vec![],raw_emmy:harvest.raw.clone(),object_views:vec![],object_mutations:vec![],native_invocations:None,host_runtime:None,xml:None};
    let mut builder=Builder{inputs:inputs.clone(),control:control.clone(),native_profile,graph,harvest,cells:BTreeMap::new(),objects:vec![],bodies:vec![],heap_observation:0,object_versions:vec![],object_view_ids:BTreeMap::new(),object_slots:0,field_write_observations:BTreeMap::new(),observation_failure:None,native:NativeInvocationState::default(),xml_objects:BTreeMap::new()};
    builder.native.facts.global_environment_open = !inputs.host.data.closed;
    if native_profile {builder.host_initialize()?;}
    if let Some(plan@LoadPlan::Unknown(_))=&inputs.load_plan{
        builder.native.facts.control.complete=false;builder.native.facts.control.coverage="unavailable_execution_plan".into();
        builder.native.facts.execution_boundaries.push(ExecutionBoundary{kind:NativeBoundaryKind::Knowledge,reason:"load_order_unknown".into(),load_plan:plan.clone()});
    }
    let load_result=if let Some(program)=inputs.xml.as_ref(){builder.xml_load_copying_reference(program)}else{
        let mut result=Ok(());
        for (occurrence,file) in ordered.iter().enumerate(){
            result=builder.load_file(file,occurrence);
            if result.is_err()||native_profile&&!builder.native.facts.control.complete{break;}
        }
        result
    };
    if let Err(reason)=load_result{
        control.check().map_err(|e|e.to_string())?;
        if !native_profile{return Err(reason)}
        if (builder.native.facts.control.complete||reason!="native invocation stopped at explicit control boundary")&&!builder.native_recover_budget(&reason)?{return Err(reason)}
    }    if native_profile && builder.native.facts.control.complete && let Some(request)=inputs.entry.clone()
        && let Err(reason)=builder.native_explicit_entry(prepared,request){
        control.check().map_err(|error|error.to_string())?;
        if (reason!="native invocation stopped at explicit control boundary" || builder.native.facts.control.complete)
            && !builder.native_recover_budget(&reason)?{return Err(reason)}
    }
    if (!native_profile||builder.native.facts.control.complete)
        && let Err(reason)=builder.deferred_bodies(){control.check().map_err(|e|e.to_string())?;
            if !native_profile || !builder.native_recover_budget(&reason)?{return Err(reason)}
    }
    if native_profile {
        builder.flow_publish_contexts();
        builder.host_finish_facts_copying_reference()?;
        if builder.native.typed_cells.borrow().len()>8192{return Err("typed storage identity budget exceeded".into())}
        builder.native.facts.storages=builder.native.typed_cells.borrow().iter().map(|(cell,owner)|StorageFact{cell:cell.clone(),owner:owner.clone()}).collect();
        builder.graph.native_invocations=Some(builder.native.facts.clone());
    }
    let complete=!native_profile||builder.native.facts.control.complete;
    if complete {if let Some(reason)=builder.observation_failure{return Err(reason)}
        if builder.graph.object_views.iter().any(|view|view.proof_id.is_empty()){return Err("cannot freeze an incomplete object observation".into())}
        if builder.graph.calls.iter().any(|call|call.result_fact.is_none()){return Err("cannot freeze a call without its result fact".into())}
    }
    control.check().map_err(|e|e.to_string())?;
    Ok(Graph{facts:builder.graph,inputs,control:control.clone(),owner})
}
