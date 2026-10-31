//! Valid empty chunks retain their authenticated load and native control state.
use super::*;
use std::{sync::atomic::Ordering, time::{Duration, Instant}};

fn admitted(files:&[(&str,&str)],ordered:&[&str])->Arc<SourceInputs> {
    let control=OperationControl::uncancelable(Instant::now()+Duration::from_secs(30));
    let host=TrustedHostEnvironment::from_host_material("empty-script-test".into(),false,vec![],vec![],&control).unwrap();
    SourceInputs::admit(SourceInputSpec{
        files:files.iter().map(|(path,text)|SourceFileInput{
            path:(*path).into(),original_utf8:(*text).into(),content_sha256:hash(text.as_bytes()),
            analysis_utf8:text.strip_prefix('\u{feff}').unwrap_or(text).into(),
            map:if text.starts_with('\u{feff}'){SourceMap::Utf8BomPrefix}else{SourceMap::Identity},
        }).collect(),
        workspace_root:std::env::current_dir().unwrap().join("empty-script-fixture"),
        source_identity:json!({"test":"empty-script"}),phase:"after_load".into(),
        ordered_occurrences:ordered.iter().map(|file|(*file).into()).collect(),
        config:crate::engine_config::research(),host,
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        limits:SourceLimits::default(),
    },&control).unwrap()
}

fn fixture(files:&[(&str,&str)],ordered:&[&str])->PreparedSnapshot {
    let inputs=admitted(files,ordered);let control=inputs.admission_control.clone();
    prepare(inputs,&control).unwrap()
}

fn chunk(prepared:&PreparedSnapshot,file:&str)->LuaChunk {
    LuaChunk::cast(prepared.analysis.semantic_model(prepared.files[file]).syntax_tree().unwrap().get_red_root()).unwrap()
}

#[test]
fn pinned_empty_trivia_and_empty_statement_shapes_remain_distinct() {
    let files=[("zero.lua",""),("bom.lua","\u{feff}"),("space.lua"," \t\r\n"),("comment.lua","-- comment only\n"),("semicolon.lua",";")];
    let prepared=fixture(&files,&[]);
    for (file,_) in files {
        let model=prepared.analysis.semantic_model(prepared.files[file]);
        let tree=model.syntax_tree().expect("admitted fixture syntax tree");
        assert!(tree.get_errors().is_empty(),"{file}");
        // No errors is None in the pinned API; verify tree presence separately.
        assert!(model.parse_errors().is_none(),"{file}");
        let chunk=LuaChunk::cast(tree.get_red_root()).unwrap();
        let body=ScriptBody::from_chunk(&chunk,prepared.inputs.analysis_text(file).unwrap()).unwrap();
        if matches!(file,"zero.lua"|"bom.lua") {
            assert!(chunk.get_block().is_none());assert!(matches!(body,ScriptBody::Empty(range) if range.is_empty()));
        }else{
            let block=chunk.get_block().expect("trivia and semicolon retain their real Block");
            assert!(matches!(body,ScriptBody::Block(_)));
            assert_eq!(block.get_stats().count(),usize::from(file=="semicolon.lua"));
            if file=="semicolon.lua" {assert!(matches!(block.get_stats().next(),Some(LuaStat::EmptyStat(_))));}
        }
    }
    // The empty representation is not a general missing-block recovery path.
    assert_eq!(ScriptBody::from_chunk(&chunk(&prepared,"zero.lua"),"local x=1").err().unwrap(),"block");
}

#[test]
fn empty_known_loads_keep_occurrences_views_activations_and_explicit_entry() {
    let prepared=fixture(&[("zero.lua",""),("code.lua","function F(x) return x end\n"),("bom.lua","\u{feff}")],&["zero.lua","code.lua","zero.lua","bom.lua"]);
    {
        let graph=prepared.build("empty-known",None,&prepared.control).unwrap();
        assert_eq!(graph.script_load_occurrences,["0:zero.lua","1:code.lua","2:zero.lua","3:bom.lua"]);
        assert_eq!(graph.functions.len(),1);assert_eq!(graph.functions[0].load_occurrence,1);
        assert!(graph.writes.iter().all(|write|write.site.file=="code.lua"));
        if let Some(native)=&graph.native_invocations {
            assert!(native.control.complete);assert!(!native.control.truncated);
            for (occurrence,file,start) in [(0,"zero.lua",0),(2,"zero.lua",0),(3,"bom.lua",3)] {
                let name=format!("load:{occurrence}:{file}");
                assert!(matches!(native.query_views.get(&name),Some(NativeQueryView::Load{occurrence:found}) if *found==occurrence));
                let activation=native.control.activations.iter().find(|row|row.context.starts_with(&format!("{name}:activation:"))).unwrap();
                assert_eq!(activation.scope,Site{file:file.into(),start,end:start});assert!(activation.cells.is_empty());
                assert!(matches!(native.query_views.get(&activation.context),Some(NativeQueryView::Load{occurrence:found}) if *found==occurrence));
                let terminal=native.control.terminals.iter().find(|row|row.context==activation.context).unwrap();
                assert!(matches!(terminal.outcome,NativeControl::Continue));assert!(terminal.occurrence.is_none());assert!(terminal.checkpoint.is_none());
                assert!(native.control.context_observations.contains_key(&activation.context));
            }
            assert!(native.control.occurrences.iter().all(|row|row.site.file=="code.lua"));
        }else{panic!("native invocation facts missing");}
    }
    let candidate=prepared.entry_catalog(&prepared.control).unwrap().into_iter().next().unwrap();
    let graph=prepared.build_plan("empty-before-entry","after_load",LoadPlan::Known(prepared.inputs.ordered.clone()),
        Some(EntryRequest{candidate:candidate.id.clone(),load_occurrence:Some(1),creation_context:None,arguments:EntryArguments::Exact(vec![EntryArgument::Number(7.0)])}),None,&prepared.control).unwrap();
    let native=graph.native_invocations.as_ref().unwrap();assert_eq!(native.entries.len(),1);
    assert_eq!(native.entries[0].request.candidate,candidate.id);assert!(native.entries[0].effects_complete);
    assert_eq!(native.closures[native.entries[0].closure].load_occurrence,1);
}

#[test]
fn empty_toc_and_xml_loads_reach_the_real_graph_consumer_in_order() {
    let sources=[("Addon.toc","zero.lua\nUI.xml\ncode.lua\nzero.lua\n"),("UI.xml","<Ui><Script file=\"bom.lua\"/><Script file=\"zero.lua\"/></Ui>"),("zero.lua",""),("bom.lua","\u{feff}"),("code.lua","local value=1\n")];
    let prepared=fixture(&sources[2..],&[]);
    let program=Arc::new(crate::xml_load::Program::from_sources("Addon.toc",sources.iter().map(|(path,text)|(*path,text.as_bytes())),&prepared.control).unwrap());
    assert_eq!(program.lua_files(),["zero.lua","bom.lua","zero.lua","code.lua","zero.lua"]);
    assert_eq!(program.documents["zero.lua"],hash(b""));
    let before=serde_json::to_vec(program.as_ref()).unwrap();
    let graph=prepared.build_xml_plan("empty-xml",program.clone(),None,&prepared.control).unwrap();
    assert_eq!(graph.script_load_occurrences,["0:zero.lua","1:bom.lua","2:zero.lua","3:code.lua","4:zero.lua"]);
    let native=graph.native_invocations.as_ref().unwrap();assert!(native.control.complete);
    assert_eq!(native.control.terminals.len(),5);assert_eq!(native.control.activations.len(),5);
    assert_eq!(native.control.occurrences.len(),1);assert_eq!(native.control.occurrences[0].load,3);
    assert_eq!(graph.xml.as_ref().unwrap().gaps,program.gaps);
    assert_eq!(serde_json::to_vec(program.as_ref()).unwrap(),before);
}

#[test]
fn empty_unknown_plan_retains_knowledge_boundary_without_execution() {
    let prepared=fixture(&[("empty.lua",""),("valid.lua","local value=1\n")],&[]);
    let files=vec!["empty.lua".into(),"valid.lua".into()];
    let graph=prepared.build_plan("empty-unknown","after_load",LoadPlan::Unknown(files.clone()),None,None,&prepared.control).unwrap();
    assert!(graph.script_load_occurrences.is_empty());assert!(graph.values.is_empty());assert!(graph.writes.is_empty());
    let native=graph.native_invocations.as_ref().unwrap();assert!(!native.control.complete);assert!(!native.control.truncated);
    assert_eq!(native.control.coverage,"unavailable_execution_plan");assert_eq!(native.execution_boundaries.len(),1);
    assert_eq!(native.execution_boundaries[0].reason,"load_order_unknown");
    assert_eq!(serde_json::to_value(&native.execution_boundaries[0].load_plan).unwrap(),serde_json::to_value(LoadPlan::Unknown(files)).unwrap());
    assert!(native.control.activations.is_empty());assert!(native.control.terminals.is_empty());assert!(native.query_views.is_empty());
}

#[test]
fn malformed_and_missing_sources_keep_their_original_first_errors() {
    // Malformed syntax is rejected by preparation before any execution plan.
    let inputs=admitted(&[("empty.lua",""),("broken.lua","local =\n")],&["empty.lua","broken.lua"]);
    let control=inputs.admission_control.clone();
    let error=prepare(inputs,&control).err().unwrap();
    assert_eq!(error.code,"source.native_unavailable");assert_eq!(error.message,"native source parse error: broken.lua");assert_eq!(error.exit_code,3);assert_eq!(error.details,Value::Null);
    let prepared=fixture(&[("empty.lua","")],&[]);
    for plan in [LoadPlan::Known(vec!["empty.lua".into(),"missing.lua".into()]),LoadPlan::Unknown(vec!["empty.lua".into(),"missing.lua".into()])] {
        let error=prepared.build_plan("missing-empty","after_load",plan,None,None,&prepared.control).err().unwrap();
        assert_eq!(error.code,"source.native_invalid_input");assert_eq!(error.message,"invalid explicit load plan");assert_eq!(error.exit_code,4);
    }
}

fn builder(prepared:&PreparedSnapshot)->Builder {
    let seed=prepared.build_execution("empty-block-reference","after_load",&[],None,&prepared.control).unwrap();
    Builder{inputs:seed.inputs.clone(),control:prepared.control.clone(),graph:seed.facts.clone(),
        harvest:harvest(&prepared.analysis,&["input.lua"],&prepared.inputs,&prepared.control).unwrap(),
        cells:BTreeMap::new(),objects:vec![],bodies:vec![],heap_observation:0,object_versions:vec![],object_view_ids:BTreeMap::new(),object_slots:0,field_write_observations:BTreeMap::new(),observation_failure:None,native:NativeInvocationState::default(),xml_objects:BTreeMap::new()}
}

fn context()->Context {Context{file:"input.lua".into(),occurrence:0,name:"load:0:input.lua".into(),deferred:false,parameters:BTreeMap::new(),conditional_branch:None}}

#[test]
fn empty_native_body_matches_zero_statement_control_charge_and_budget_errors() {
    for remaining in [0,1,2] {
        let mut outcomes=Vec::new();
        for text in [""," \t\n"] {
            let prepared=fixture(&[("input.lua",text)],&[]);let mut builder=builder(&prepared);
            let used=prepared.inputs.shared_steps.load(Ordering::Relaxed);
            let fill=prepared.inputs.corpus.limits.construction_steps-used-remaining;
            prepared.inputs.charge(fill).unwrap();
            let before=prepared.inputs.shared_steps.load(Ordering::Relaxed);
            let result=if text.is_empty(){builder.load_file("input.lua",0)}else{builder.block(chunk(&prepared,"input.lua").get_block().unwrap(),&context())};
            let spent=prepared.inputs.shared_steps.load(Ordering::Relaxed)-before;
            assert_eq!(spent,remaining);
            assert_eq!(builder.native.facts.control.activations.len(),usize::from(remaining==2));
            assert_eq!(builder.native.facts.control.terminals.len(),usize::from(remaining==2));
            outcomes.push(result);
        }
        assert_eq!(outcomes[0],outcomes[1]);
        if remaining==2 {assert!(outcomes[0].is_ok());}else{assert!(outcomes[0].as_ref().unwrap_err().contains("shared operation step budget exceeded"));}
    }
    // Retaining an empty load still consumes the original terminal allowance.
    // A quota failure with no statement occurrence must not invent a prefix site.
    let mut errors=Vec::new();
    for text in [""," \t\n"] {
        let prepared=fixture(&[("input.lua",text)],&[]);
        let error=prepared.build_plan("empty-terminal-limit","after_load",LoadPlan::Known(vec!["input.lua".into();CONTROL_TERMINAL_LIMIT+1]),None,None,&prepared.control).err().unwrap();
        assert_eq!(error.code,"source.native_unavailable");assert_eq!(error.message,"native terminal path budget exceeded");
        errors.push(serde_json::to_value(error).unwrap());
    }
    assert_eq!(errors[0],errors[1]);
}

#[test]
fn empty_public_loads_preserve_cancellation_and_deadline_first_error() {
    for expired in [false,true] {
        let mut prepared=fixture(&[("empty.lua","")],&["empty.lua"]);
        let stopped=stopped_control(expired,prepared.control.deadline());
        prepared.control=stopped.clone();
        let expected=serde_json::to_value(stopped.check().unwrap_err()).unwrap();
        let before=prepared.inputs.shared_steps.load(Ordering::Relaxed);
        let error=prepared.build("empty-stopped",None,&stopped).err().unwrap();
        assert_eq!(serde_json::to_value(error).unwrap(),expected);
        let error=prepared.build_plan("empty-stopped","after_load",LoadPlan::Known(vec!["empty.lua".into(),"missing.lua".into()]),None,None,&stopped).err().unwrap();
        assert_eq!(serde_json::to_value(error).unwrap(),expected);
        assert_eq!(prepared.inputs.shared_steps.load(Ordering::Relaxed),before);
    }
}
