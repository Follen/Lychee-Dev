// Compare production reconstruction with same-scope retained projections.
// Rows, order, digest, analysis, XML maps and semantic receipts stay exact.
fn context_projection_fixture() -> ResearchSession {
    context_projection_load_fixture(true)
}
fn context_projection_load_fixture(complete_load:bool) -> ResearchSession {
    let control=OperationControl::default();
    // Positive entry cases must finish loading before the explicit entry can
    // run. The former inputs below distinguish a recursive load boundary from
    // an unknown pre-definition call, whose conservative effects permit loading.
    let target=if complete_load {
        "\u{feff}-- 中文 😀\r\nfunction Target(x)\r\n if x then Other(nil) end\r\n return Other(x)\r\nend; function Other(x) return x end\r\nAlias=Target\r\n"
    } else {
        "\u{feff}-- 中文 😀\r\nfunction Target(x)\r\n if x then Target(nil) end\r\n return Other(x)\r\nend\r\nAlias=Target\r\n"
    };
    let second=if complete_load {"target.lua\nalias.lua\nFrames.xml\na-callers.lua\nz-shadow.lua\n"}
        else {"a-callers.lua\nFrames.xml\ntarget.lua\nz-shadow.lua\n"};
    let files=BTreeMap::from([
        ("target.lua".into(),target.into()),
        ("a-callers.lua".into(),"local Alias=Target\nAlias(1)\nTarget(2)\nlocal function Outer() Target(3) end\n".into()),
        ("alias.lua".into(),"Alias(4)\n".into()),
        ("z-shadow.lua".into(),"local function Target() end\nTarget()\nlocal function Caller() Target() end\n".into()),
    ]);
    let documents=BTreeMap::from([
        ("One.toc".into(),"target.lua\na-callers.lua\nalias.lua\nz-shadow.lua\nFrames.xml\n".into()),
        ("Two.toc".into(),second.into()),
        ("Frames.xml".into(),"\u{feff}<Ui>\r\n<Frame name=\"Template\" virtual=\"true\"><Scripts><OnLoad>local s = &quot;😀&quot;&#10;Tar&#103;et(self)</OnLoad></Scripts></Frame>\r\n<Frame name=\"Actual\" inherits=\"Template\"/>\r\n</Ui>".into()),
    ]);
    ResearchSession::prepare(SnapshotRequest{encoding_warnings:vec![],files,load_documents:documents,environment_documents:BTreeMap::new(),
        source:json!({"repository":"context-projection-test","product":"retail"}),
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        api:vec![],environment_manifest:None,load_plans:["One.toc","Two.toc"].map(|manifest|LoadEvidence{manifest:manifest.into(),ordered:vec![],issues:vec![]}).to_vec(),input_gaps:vec![]},&control).unwrap()
}

fn assert_context_projection_equal(session:&ResearchSession,query:&QueryRequest,graph:Option<&Graph>) {
    let plan=session.prepared.plan_research_operations(query,&session.xml,None).unwrap();
    let incoming=session.prepared.research_relations_with_context_observed(&query.symbol,"incoming",graph,&session.xml,None,None,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap();
    let before=session.prepared.research_steps();
    let expected=session.prepared.research_relations_with_context_observed(&query.symbol,"both",graph,&session.xml,None,None,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap();
    let cost=session.prepared.research_steps()-before;
    assert_ne!(serde_json::to_vec(&incoming).unwrap(),serde_json::to_vec(&expected).unwrap(),"outgoing/overlapping rows must not be replaced by incoming rows");
    let completion=session.prepared.execute_research_operations_observed(&plan,query,graph,&session.xml,None,&mut |_|{}).unwrap();
    let mut execution=plan::ResearchExecution::new(&plan);
    let projection=execution.scope(json!({"kind":"context-projection-equivalence"}),completion).unwrap();
    assert!(projection.is_some(),"the fixture must exercise retained file rows");
    let before=session.prepared.research_steps();let reused=context_projection_reused_files();
    let actual=session.prepared.project_research_relations_observed(&plan,query,graph,&session.xml,projection,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap();
    assert!(context_projection_reused_files()>reused,"the second pass must actually move an eligible file");
    assert_eq!(session.prepared.research_steps()-before,cost,"same facade steps and bounded return reserve");
    assert_eq!(serde_json::to_vec(&actual).unwrap(),serde_json::to_vec(&expected).unwrap());
    let (evidence,receipt)=execution.finish();
    let references=&evidence["answers"][0]["operations"][0];
    assert_eq!(references["result"]["relationsSHA256"],digest(&incoming["relations"]));
    assert_eq!(references["result"]["relationCount"],incoming["relations"].as_array().unwrap().len());
    assert_eq!(references["result"]["analysis"],incoming["analysis"]);
    assert_eq!(references["executed"],graph.is_some());
    assert_eq!(receipt.completed_batches,if graph.is_some(){plan.batch_count()}else{0});
}

#[test]
fn context_projection_preserves_all_rows_digests_coverage_unicode_xml_and_entries() {
    let session=context_projection_fixture();let control=session.prepared.operation_control();
    let query=planned_query("Target","target.lua",2,5,"function","both");
    assert_context_projection_equal(&session,&query,None);
    for program in &session.xml.programs {
        let graph=session.prepared.build_xml_plan("context-load",program.clone(),None,control).unwrap();
        assert!(graph.native_invocations.as_ref().unwrap().control.complete,"positive fixture must finish loading before entry");
        assert_context_projection_equal(&session,&query,Some(&graph));
        let entry=session.prepared.entry_catalog(control).unwrap().iter().find(|entry|entry.closure.file=="target.lua"&&entry.start_line==2).unwrap();
        for argument in [EntryArgument::Nil,EntryArgument::Boolean(true)] {
            let request=EntryRequest{candidate:entry.id.clone(),load_occurrence:None,creation_context:None,arguments:EntryArguments::Exact(vec![argument])};
            let graph=session.prepared.build_xml_plan("context-entry",program.clone(),Some(request),control).unwrap();
            assert_eq!(graph.native_invocations.as_ref().unwrap().entries.len(),1);
            assert_context_projection_equal(&session,&query,Some(&graph));
        }
    }
}

#[test]
fn context_projection_incomplete_load_does_not_fabricate_an_explicit_entry() {
    let session=context_projection_load_fixture(false);
    let control=session.prepared.operation_control();
    let entry=session.prepared.entry_catalog(control).unwrap().iter().find(|entry|entry.closure.file=="target.lua"&&entry.start_line==2).unwrap();
    // One.toc calls the already-loaded Target recursively. Two.toc instead
    // calls an unknown global before definition; it is covered separately below.
    let program=session.xml.programs.iter().find(|program|program.manifest=="One.toc").unwrap();
    let request=EntryRequest{candidate:entry.id.clone(),load_occurrence:None,creation_context:None,arguments:EntryArguments::Exact(vec![EntryArgument::Nil])};
    for request in [None,Some(request)] {
        let graph=session.prepared.build_xml_plan("incomplete-context-entry",program.clone(),request,control).unwrap();
        let facts=graph.native_invocations.as_ref().unwrap();
        assert!(!facts.control.complete,"original recursive load failure remains explicit: {:?}",facts.control.boundaries);
        assert!(!facts.control.truncated,"a known recursive boundary is not a resource truncation");
        assert_eq!(facts.control.coverage,"verified_prefix_with_explicit_boundaries");
        assert_eq!(facts.control.boundaries.len(),1);
        let boundary=&facts.control.boundaries[0];
        assert!(matches!(boundary.kind,NativeBoundaryKind::Knowledge));
        assert_eq!(boundary.reason,"recursive_invocation_requires_summary");
        assert_eq!(boundary.site.file,"target.lua");
        assert_eq!(boundary.site.text(session.prepared.inputs()).unwrap(),"Target(nil)");
        assert!(facts.entries.is_empty(),"do not invent an entry after a stopped load");
    }
}

#[test]
fn context_projection_unknown_load_calls_preserve_uncertainty_and_real_entry() {
    let session=context_projection_load_fixture(false);
    let control=session.prepared.operation_control();
    let program=session.xml.programs.iter().find(|program|program.manifest=="Two.toc").unwrap();
    let load=session.prepared.build_xml_plan("unknown-context-load",program.clone(),None,control).unwrap();
    let facts=load.native_invocations.as_ref().unwrap();
    assert!(facts.control.complete,"unknown call effects do not stop bounded loading: {:?}",facts.control.boundaries);
    assert!(facts.control.boundaries.is_empty());
    assert!(facts.entries.is_empty());
    let calls=load.calls.iter().filter(|call|call.site.file=="a-callers.lua"&&call.context.starts_with("load:0:")).collect::<Vec<_>>();
    assert_eq!(calls.len(),2,"both calls before Target's definition must be observed");
    for (call,(text,reason)) in calls.iter().zip([("Alias(1)","open_initial_host_environment"),("Target(2)","unknown_call_open_global_environment")]) {
        assert_eq!(call.site.text(session.prepared.inputs()).unwrap(),text);
        assert_eq!(load.uses[call.callee].decision,Decision::Unavailable(reason.into()));
        assert!(matches!(call.invocation_coverage,CallCoverage::Unavailable));
        assert!(matches!(call.effect_coverage,CallCoverage::Unavailable));
        assert!(facts.escape_events.iter().any(|event|event.call==call.id&&event.application_complete));
    }
    let entry=session.prepared.entry_catalog(control).unwrap().iter().find(|entry|entry.closure.file=="target.lua"&&entry.start_line==2).unwrap();
    for (argument,complete) in [(EntryArgument::Nil,true),(EntryArgument::Boolean(true),false)] {
        let request=EntryRequest{candidate:entry.id.clone(),load_occurrence:None,creation_context:None,arguments:EntryArguments::Exact(vec![argument])};
        let graph=session.prepared.build_xml_plan("unknown-context-entry",program.clone(),Some(request.clone()),control).unwrap();
        let facts=graph.native_invocations.as_ref().unwrap();
        assert_eq!(facts.control.complete,complete,"entry arguments determine recursion: {:?}",facts.control.boundaries);
        assert_eq!(facts.entries.len(),1,"a completed load permits one real entry even if that entry later stops");
        assert_eq!(facts.entries[0].request,request);
        assert!(facts.entries[0].frame.is_some());
        assert!(!facts.entries[0].effects_complete,"the nil entry calls unknown Other; the true entry stops at recursion");
        if complete {
            assert!(facts.control.boundaries.is_empty());
            assert!(graph.calls.iter().any(|call|call.site.file=="target.lua"&&call.context.starts_with("entry:0:")
                &&call.site.text(session.prepared.inputs()).unwrap()=="Other(x)"
                &&matches!(&graph.uses[call.callee].decision,Decision::Unavailable(reason) if reason=="unknown_call_open_global_environment")));
        } else {
            assert_eq!(facts.control.boundaries.len(),1);
            assert_eq!(facts.control.boundaries[0].reason,"recursive_invocation_requires_summary");
        }
    }
}

#[test]
fn context_projection_excludes_inherited_owner_files_from_reuse() {
    let control=OperationControl::default();
    let documents=BTreeMap::from([
        ("Main.toc".into(),"Templates.xml\nObjects.xml\n".into()),
        ("Templates.xml".into(),"<Ui>\n<Frame name=\"Template\" virtual=\"true\"><Scripts><OnLoad>\nlocal function Inner() Hidden(self) end\nTarget(self)\n</OnLoad></Scripts></Frame>\n</Ui>".into()),
        ("Objects.xml".into(),"<Ui>\n<Frame name=\"Actual\" inherits=\"Template\"/>\n</Ui>".into()),
    ]);
    let session=ResearchSession::prepare(SnapshotRequest{encoding_warnings:vec![],files:BTreeMap::new(),load_documents:documents.clone(),environment_documents:BTreeMap::new(),
        source:json!({"repository":"context-inheritance-test","product":"retail"}),
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        api:vec![],environment_manifest:None,load_plans:vec![LoadEvidence{manifest:"Main.toc".into(),ordered:vec![],issues:vec![]}],input_gaps:vec![]},&control).unwrap();
    let graph=session.prepared.build_xml_plan("context-inheritance",session.xml.programs[0].clone(),None,&control).unwrap();
    for (name,path) in [("Actual","Objects.xml"),("Template","Templates.xml")] {
        let facts=crate::facts::xml(documents[path].as_bytes());let declaration=facts.declarations.iter().find(|decl|decl.name==name).unwrap();
        let query=planned_query(name,path,declaration.line,declaration.end_line,&declaration.category,"both");
        // A distinct XML source path can still belong to the selected object.
        for (file,script) in &session.xml.scripts {
            assert_eq!(script.path,"Templates.xml");
            assert!(session.xml.may_contain_outgoing(&query.symbol,file));
            if name=="Actual" {
                let text=session.prepared.inputs().original(file).unwrap();let start=text.find("Target").unwrap();
                assert!(session.xml.within(&session.prepared,&query.symbol,&Site{file:file.clone(),start:start as u32,end:(start+6) as u32}).unwrap());
            }
        }
        let plan=session.prepared.plan_research_operations(&query,&session.xml,None).unwrap();
        let completion=session.prepared.execute_research_operations_observed(&plan,&query,Some(&graph),&session.xml,None,&mut |_|{}).unwrap();
        let (_,_,projection)=completion.consume(&plan).unwrap();
        assert!(projection.is_none(),"inherited outgoing files are never retained as invariant incoming rows");
        let before=session.prepared.research_steps();
        let expected=session.prepared.research_relations_with_context_observed(&query.symbol,"both",Some(&graph),&session.xml,None,None,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap();
        let steps=session.prepared.research_steps()-before;let before=session.prepared.research_steps();
        let actual=session.prepared.project_research_relations_observed(&plan,&query,Some(&graph),&session.xml,projection,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap();
        assert_eq!(session.prepared.research_steps()-before,steps);
        assert_eq!(serde_json::to_vec(&actual).unwrap(),serde_json::to_vec(&expected).unwrap());
    }
}

#[test]
fn context_projection_rejects_foreign_request_graph_xml_and_later_cancellation() {
    let session=context_projection_fixture();let other=context_projection_fixture();
    let query=planned_query("Target","target.lua",2,5,"function","both");
    let plan=session.prepared.plan_research_operations(&query,&session.xml,None).unwrap();
    let control=session.prepared.operation_control();
    let graphs=session.xml.programs.iter().map(|program|session.prepared.build_xml_plan("context-foreign",program.clone(),None,control).unwrap()).collect::<Vec<_>>();
    let projection=||session.prepared.execute_research_operations_observed(&plan,&query,Some(&graphs[0]),&session.xml,None,&mut |_|{}).unwrap().consume(&plan).unwrap().2;
    let identical=query.clone();
    assert!(session.prepared.project_research_relations_observed(&plan,&identical,Some(&graphs[0]),&session.xml,projection(),&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).is_err());
    assert!(session.prepared.project_research_relations_observed(&plan,&query,Some(&graphs[1]),&session.xml,projection(),&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).is_err());
    assert!(session.prepared.project_research_relations_observed(&plan,&query,None,&session.xml,projection(),&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).is_err());
    assert!(session.prepared.project_research_relations_observed(&plan,&query,Some(&graphs[0]),&other.xml,projection(),&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).is_err());
    assert!(other.prepared.project_research_relations_observed(&plan,&query,Some(&graphs[0]),&session.xml,projection(),&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).is_err());
    let retained=projection();assert!(retained.is_some());
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(session.prepared.project_research_relations_observed(&plan,&query,Some(&graphs[0]),&session.xml,retained,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap_err().code,"command.cancelled");
}

#[test]
fn context_projection_replays_budget_and_original_deadline_after_operations() {
    let files=BTreeMap::from([("target.lua".into(),"function Target() end\n".into()),("a-callers.lua".into(),"Target()\n".into())]);
    let session=planner_fixture(files.clone(),BTreeMap::new(),None);
    let query=planned_query("Target","target.lua",1,1,"function","both");
    let plan=session.prepared.plan_research_operations(&query,&session.xml,None).unwrap();
    let (_,_,projection)=session.prepared.execute_research_operations_observed(&plan,&query,None,&session.xml,None,&mut |_|{}).unwrap().consume(&plan).unwrap();assert!(projection.is_some());
    let remaining=session.prepared.inputs().limits().construction_steps-session.prepared.research_steps();
    session.prepared.carry_research_steps(remaining,session.prepared.operation_control()).unwrap();
    let before=session.prepared.research_steps();
    let cached=session.prepared.project_research_relations_observed(&plan,&query,None,&session.xml,projection,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap_err();
    let fresh=session.prepared.research_relations_with_context_observed(&query.symbol,"both",None,&session.xml,None,None,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap_err();
    assert_eq!(cached.code,"source.native_query_budget");assert_eq!(cached.code,fresh.code);assert_eq!(session.prepared.research_steps(),before);

    let mut session=planner_fixture(files,BTreeMap::new(),None);
    let control=OperationControl::uncancelable(std::time::Instant::now()+std::time::Duration::from_secs(2));
    session.prepared.begin_research_request(&control).unwrap();
    let plan=session.prepared.plan_research_operations(&query,&session.xml,None).unwrap();
    let (_,_,projection)=session.prepared.execute_research_operations_observed(&plan,&query,None,&session.xml,None,&mut |_|{}).unwrap().consume(&plan).unwrap();assert!(projection.is_some());
    std::thread::sleep(control.deadline().saturating_duration_since(std::time::Instant::now())+std::time::Duration::from_millis(1));
    assert_eq!(session.prepared.project_research_relations_observed(&plan,&query,None,&session.xml,projection,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap_err().code,"command.deadline");
}

#[test]
fn context_projection_large_rows_fall_back_without_truncation() {
    let files=BTreeMap::from([("target.lua".into(),"function Target() end\n".into()),(format!("{}-callers.lua","a".repeat(100)),"Target()\n".repeat(2500))]);
    let session=planner_fixture(files,BTreeMap::new(),None);
    let query=planned_query("Target","target.lua",1,1,"function","both");
    let plan=session.prepared.plan_research_operations(&query,&session.xml,None).unwrap();
    let (answers,_,projection)=session.prepared.execute_research_operations_observed(&plan,&query,None,&session.xml,None,&mut |_|{}).unwrap().consume(&plan).unwrap();
    assert!(projection.is_none(),"serialized file rows exceed the existing 1 MiB retention policy");
    let before=session.prepared.research_steps();
    let expected=session.prepared.research_relations_with_context_observed(&query.symbol,"both",None,&session.xml,None,None,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap();let cost=session.prepared.research_steps()-before;
    let before=session.prepared.research_steps();let reused=context_projection_reused_files();
    let actual=session.prepared.project_research_relations_observed(&plan,&query,None,&session.xml,projection,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap();
    assert_eq!(context_projection_reused_files(),reused);assert_eq!(session.prepared.research_steps()-before,cost);
    assert_eq!(serde_json::to_vec(&actual).unwrap(),serde_json::to_vec(&expected).unwrap());
    assert!(serde_json::to_vec(&actual["relations"]).unwrap().len()>1<<20);
    assert_eq!(actual["relations"].as_array().unwrap().iter().filter(|row|row["kind"]=="call").count(),2500);
    assert_eq!(answers[0]["result"]["relationCount"],actual["relations"].as_array().unwrap().len());
}

// Each side owns an independent prepared snapshot. Renewing between offsets
// starts the real request counter again; neither side inherits the other's
// projection, remaining budget, or previous failure.
fn projection_at_reserve_offset(session:&mut ResearchSession,query:&QueryRequest,offset:isize,reuse:bool)->(std::result::Result<Value,Value>,usize,usize,usize) {
    let control=session.prepared.operation_control().clone();
    session.prepared.begin_research_request(&control).unwrap();
    let plan=session.prepared.plan_research_operations(query,&session.xml,None).unwrap();
    let (_,_,projection)=session.prepared.execute_research_operations_observed(&plan,query,None,&session.xml,None,&mut |_|{}).unwrap().consume(&plan).unwrap();
    assert!(projection.is_some(),"both sides must compute an eligible same-scope projection first");
    let projection=if reuse{projection}else{drop(projection);None};
    // This is the production SourceQueries return reserve, deliberately fixed
    // here so a changed contract requires updating the boundary regression.
    let start=(session.prepared.inputs().limits().construction_steps-1024).checked_add_signed(offset).unwrap();
    session.prepared.carry_research_steps(start-session.prepared.research_steps(),&control).unwrap();
    let passes=research_projection_passes();let files=context_projection_reused_files();
    let result=session.prepared.project_research_relations_observed(&plan,query,None,&session.xml,projection,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap())
        .map_err(|error|json!({"code":error.code,"message":error.message,"details":error.details}));
    (result,session.prepared.research_steps(),research_projection_passes()-passes,context_projection_reused_files()-files)
}

#[test]
fn context_and_incoming_projection_match_every_reserve_boundary_and_safe_reuse() {
    for direction in ["incoming","both"] {
        for count in [1usize,2,4] {
            // A local function declaration has no expression-reference row;
            // all charged references are in the LAST file, z-callers.lua.
            let files=BTreeMap::from([("target.lua".into(),"local function Target() end\n".into()),("z-callers.lua".into(),"Target()\n".repeat(count))]);
            let mut original=planner_fixture(files.clone(),BTreeMap::new(),None);
            let mut candidate=planner_fixture(files,BTreeMap::new(),None);
            let query=planned_query("Target","target.lua",1,1,"function",direction);
            // Includes the reviewer's exact L-reserve-1 counterexample, both
            // adjacent starts, exhaustion, and a comfortably reusable prefix.
            for offset in -(3*count as isize+4)..=2 {
                let expected=projection_at_reserve_offset(&mut original,&query,offset,false);
                let actual=projection_at_reserve_offset(&mut candidate,&query,offset,true);
                assert_eq!(actual.0,expected.0,"direction={direction}, rows={count}, reserve offset={offset}");
                assert_eq!(actual.1,expected.1,"exact non-racing final counter: {direction}, {count}, {offset}");
                if count==1&&offset==-1 {
                    assert!(actual.0.is_ok(),"one step then two direct serialization charges must still succeed");
                    assert_eq!(actual.1,candidate.prepared.inputs().limits().construction_steps-1022);
                }
                if offset < -(3*count as isize) {
                    assert!(actual.0.is_ok());
                    if direction=="incoming" {assert_eq!(actual.2,0,"whole incoming result actually moved");}
                    else {assert!(actual.3>0,"eligible context file actually moved");}
                } else {
                    assert_eq!(actual.2,1,"edge uses the original projection path");
                    assert_eq!(actual.3,0,"edge never replays cached charges");
                }
            }
        }
    }
}

#[test]
fn context_projection_preserves_nonempty_xml_mixin_copy_tail() {
    let control=OperationControl::default();
    let files=BTreeMap::from([
        ("logic.lua".into(),"function Caller(x)\n return x\nend\nMixin={OnLoad=Caller}\n".into()),
        ("z-callers.lua".into(),"Caller(1)\n".into()),
    ]);
    let documents=BTreeMap::from([
        ("Main.toc".into(),"logic.lua\nz-callers.lua\nTemplates.xml\nObjects.xml\n".into()),
        ("Templates.xml".into(),"\u{feff}<Ui>\r\n<Frame name=\"Template\" virtual=\"true\"><Scripts><OnLoad>local text = &quot;😀&quot;&#10;Caller(self)</OnLoad></Scripts></Frame>\r\n</Ui>".into()),
        ("Objects.xml".into(),"<Ui>\n<Frame name=\"Actual\" inherits=\"Template\" mixin=\"Mixin\"/>\n</Ui>".into()),
    ]);
    let session=ResearchSession::prepare(SnapshotRequest{encoding_warnings:vec![],files,load_documents:documents,environment_documents:BTreeMap::new(),
        source:json!({"repository":"context-mixin-test","product":"retail"}),
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        api:vec![],environment_manifest:None,load_plans:vec![LoadEvidence{manifest:"Main.toc".into(),ordered:vec![],issues:vec![]}],input_gaps:vec![]},&control).unwrap();
    let graph=session.prepared.build_xml_plan("context-mixin",session.xml.programs[0].clone(),None,&control).unwrap();
    assert!(!graph.xml.as_ref().unwrap().copies.is_empty(),"fixture must perform real abstract XML mixin copies");
    let query=planned_query("Caller","logic.lua",1,3,"function","both");
    let original=session.prepared.research_relations_with_context_observed(&query.symbol,"both",Some(&graph),&session.xml,None,None,&mut |_|{}).map(|result|serde_json::to_value(result).unwrap()).unwrap();
    let rows=original["relations"].as_array().unwrap();
    let first_copy=rows.iter().position(|row|row["kind"]=="xml_mixin_copy").expect("selected Caller value must have an XML mixin-copy relation");
    assert!(rows[first_copy..].iter().all(|row|row["kind"]=="xml_mixin_copy"),"copies remain the final ordered tail");
    assert_context_projection_equal(&session,&query,Some(&graph));
}
