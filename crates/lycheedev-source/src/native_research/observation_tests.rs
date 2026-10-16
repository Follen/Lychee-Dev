// Observation must leave full rows, scope evidence, shared charges, errors and
// the original control unchanged. These exercise both full and unknown loads.
#[test]
fn query_allocation_observation_preserves_complete_wire_and_shared_costs() {
    for loaded in [false,true] {
        for direction in ["incoming","outgoing","both"] {
            let mut session=if loaded {context_projection_fixture()} else {
                planner_fixture(BTreeMap::from([("target.lua".into(),
                    "\u{feff}-- 中文 😀\r\nfunction Target(x)\r\n return Other(x)\r\nend\r\nfunction Other(x) return x end\r\nTarget(1)\r\n".into())]),BTreeMap::new(),None)
            };
            let query=planned_query("Target","target.lua",2,if loaded{5}else{4},"function",direction);
            let control=session.prepared.operation_control().clone();
            let expected=session.query(query.clone(),&control).unwrap();
            let expected_steps=session.prepared.research_steps();
            let mut phases=Vec::new();
            let actual=session.query_observed(query,&control,|phase|phases.push(phase)).unwrap();
            assert_eq!(serde_json::to_vec(&actual).unwrap(),serde_json::to_vec(&expected).unwrap());
            assert_eq!(session.prepared.research_steps(),expected_steps);
            for phase in ["query_execute_operations","query_execution_identity","query_operations_executed","query_scope_evidence","query_projected","query_reply_assembled"] {
                assert!(phases.contains(&phase),"missing {phase}: {phases:?}");
            }
            if direction!="outgoing" {
                for phase in ["query_operation_authenticate","query_relations_target_declarations","query_relations_target_values","query_relations_use_index","query_relations_use_sort","query_relations_file_syntax","query_relations_reference_rows","query_relations_xml_rows","query_relations_control_json","query_references_digest","query_references_retain","query_type_observations","query_operation_answer"] {
                    assert!(phases.contains(&phase),"missing {phase}: {phases:?}");
                }
                assert!(phases.iter().position(|phase|*phase=="query_references_digest")<phases.iter().position(|phase|*phase=="query_type_observations"));
            }
            if direction!="incoming" {assert!(phases.contains(&"query_definition_resolve"));}
        }
    }
}

#[test]
fn query_allocation_observation_keeps_original_cancellation() {
    for stop in ["query_relations_use_sort","query_relations_file_syntax","query_relations_control_json","query_type_observations","query_scope_evidence"] {
        let mut session=context_projection_fixture();
        let control=session.prepared.operation_control().clone();
        let query=planned_query("Target","target.lua",2,5,"function","incoming");
        let cancel=control.cancel_handle();let mut seen=false;
        let error=session.query_observed(query,&control,|phase|{
            if phase==stop {seen=true;cancel.store(true,std::sync::atomic::Ordering::Release);}
        }).unwrap_err();
        assert!(seen,"phase did not execute: {stop}");
        assert_eq!(error.code,"command.cancelled");
        assert_eq!(control.check().unwrap_err().code,"command.cancelled");
    }
}

#[test]
fn query_allocation_observation_preserves_authenticated_api_type_execution() {
    let path="Interface/AddOns/Blizzard_APIDocumentationGenerated/TimerDocumentation.lua";
    let text="local API = {\n Name = \"Timer\",\n Type = \"System\",\n Namespace = \"C_Timer\",\n Functions = {\n  {\n   Name = \"After\",\n   Type = \"Function\",\n  },\n },\n}\nAPIDocumentation:AddDocumentationTable(API)\n";
    let identity=environment::Identity{repository:"wow-ui-source".into(),commit:"a".repeat(40),client:"retail".into(),dependency_commits:BTreeMap::new(),generator_version:environment::GENERATOR.into()};
    let api=environment::build(identity,&BTreeMap::from([(path.into(),text.as_bytes().to_vec())]),environment::Limits::default(),&OperationControl::default()).unwrap();
    let record=api.facts.iter().find(|record|record.name=="C_Timer.After").unwrap();
    let query=planned_query("C_Timer.After",path,record.line,record.end_line,"api-function","incoming");
    let mut session=planner_fixture(BTreeMap::from([("addon.lua".into(),"function Addon() C_Timer.After() end".into())]),BTreeMap::from([(path.into(),text.into())]),Some(api));
    let control=session.prepared.operation_control().clone();
    let expected=session.query(query.clone(),&control).unwrap();let expected_steps=session.prepared.research_steps();
    let mut phases=Vec::new();
    let actual=session.query_observed(query,&control,|phase|phases.push(phase)).unwrap();
    assert_eq!(serde_json::to_vec(&actual).unwrap(),serde_json::to_vec(&expected).unwrap());
    assert_eq!(session.prepared.research_steps(),expected_steps);
    assert!(phases.contains(&"query_type_api"));
    assert!(!phases.contains(&"query_type_observations"));
    assert_eq!(actual["semanticExecution"]["answers"][0]["operations"][1]["executed"],true);
}

#[test]
fn shared_analysis_two_real_scopes_keep_full_rows_answers_and_last_analysis() {
    let control=OperationControl::default();
    let query=planned_query("Target","target.lua",1,1,"function","incoming");
    let snapshot=SnapshotRequest{
        files:BTreeMap::from([("target.lua".into(),"function Target() end\n".into()),("caller.lua".into(),"Target()\n".into())]),
        load_documents:BTreeMap::from([("A.toc".into(),"target.lua\ncaller.lua\n".into()),("B.toc".into(),"target.lua\n".into())]),
        environment_documents:BTreeMap::new(),source:json!({"repository":"shared-analysis-two-scopes","product":"retail"}),
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        api:vec![],environment_manifest:None,load_plans:vec![
            LoadEvidence{manifest:"A.toc".into(),ordered:vec!["target.lua".into(),"caller.lua".into()],issues:vec![]},
            LoadEvidence{manifest:"B.toc".into(),ordered:vec!["target.lua".into()],issues:vec![]}],input_gaps:vec![]};
    let mut session=ResearchSession::prepare(snapshot,&control).unwrap();
    let mut expected_scopes=Vec::new();let mut expected_references=Vec::new();let mut expected_rows=Vec::new();
    for program in &session.xml.programs {
        let graph=session.prepared.build_xml_plan("source-research-load",program.clone(),None,&control).unwrap();
        let legacy=session.prepared.full_scan_research_relations(&query.symbol,"incoming",Some(&graph),&session.xml).unwrap();
        let mut projection=session.prepared.research_relations(&query.symbol,"incoming",Some(&graph),&session.xml).unwrap();
        assert_eq!(serde_json::to_vec(&original_call_labels(projection.clone())).unwrap(),serde_json::to_vec(&legacy).unwrap());
        let scope=json!({"manifest":program.manifest,"loadContext":program.context,"snapshot":graph.snapshot_key,"entries":graph.native_invocations.as_ref().map(|facts|&facts.entries),"kind":"alternative_manifest_hypothetical_after_load"});
        // The previous full-buffer hash is an independent byte oracle for the
        // actual native rows, not a second call to the new streaming digest.
        expected_references.push(json!({"analysis":projection["analysis"],"relationCount":projection["relations"].as_array().unwrap().len(),"relationsSHA256":hash(&serde_json::to_vec(&projection["relations"]).unwrap())}));
        for mut row in take_relation_rows(&mut projection){row["loadScope"]=scope.clone();expected_rows.push(row);}
        expected_scopes.push(json!({"analysis":projection["analysis"],"scope":scope,"xmlFacts":graph.xml}));
    }
    assert_eq!(expected_scopes.len(),2);
    assert_ne!(expected_scopes[0]["analysis"],expected_scopes[1]["analysis"]);
    let actual=session.query(query,&control).unwrap();
    assert_eq!(serde_json::to_vec(&actual["loadScopes"]).unwrap(),serde_json::to_vec(&expected_scopes).unwrap());
    assert_eq!(serde_json::to_vec(&actual["relations"]).unwrap(),serde_json::to_vec(&expected_rows).unwrap());
    assert_eq!(actual["semanticExecution"]["answers"].as_array().unwrap().len(),2);
    for (scope,expected) in expected_references.iter().enumerate(){
        assert_eq!(serde_json::to_vec(&actual["semanticExecution"]["answers"][scope]["operations"][0]["result"]).unwrap(),serde_json::to_vec(expected).unwrap());
    }
    let receipt=&actual["semanticExecution"]["receipt"];
    let complete=expected_scopes.iter().all(|scope|scope["analysis"]["state"]=="complete")&&actual["gaps"].as_array().unwrap().is_empty()
        &&actual["semanticPlan"]["plan"]["unavailable"].as_array().unwrap().is_empty()
        &&["unavailable_scopes","uncertain_answers","unexecuted_operations"].iter().all(|key|receipt[*key]==0);
    let mut expected_top=expected_scopes[1]["analysis"].clone();
    expected_top["state"]=json!(if complete{"complete"}else{"partial"});
    expected_top["apiDeclarationCount"]=json!(0);expected_top["cacheHit"]=json!(false);
    assert_eq!(serde_json::to_vec(&actual["analysis"]).unwrap(),serde_json::to_vec(&expected_top).unwrap());
}

// Each scope owns an immutable Graph. Its memo contains no direction-dependent
// coverage; those fields must still be recomputed for the selected projection.
#[test]
fn shared_control_same_graph_both_directions_and_distinct_graphs_preserve_wire() {
    let session=context_projection_fixture();let control=session.prepared.operation_control();
    let mut previous_control=None;
    for program in &session.xml.programs {
        let graph=session.prepared.build_xml_plan("shared-control-directions",program.clone(),None,control).unwrap();
        let graph_before=serde_json::to_vec(&graph).unwrap();
        let mut same_graph_control=None;
        for direction in ["incoming","both"] {
            let query=planned_query("Target","target.lua",2,5,"function",direction);
            let plan=session.prepared.plan_research_operations(&query,&session.xml,None).unwrap();
            let expected=session.prepared.full_scan_research_relations(&query.symbol,direction,Some(&graph),&session.xml).unwrap();
            let before=session.prepared.research_steps();
            let fresh=session.prepared.research_relations(&query.symbol,direction,Some(&graph),&session.xml).unwrap();
            let cost=session.prepared.research_steps()-before;
            // The frozen oracle predates only incoming caller labels. Preserve
            // its exact existing inverse; all analysis/coverage stays untouched.
            assert_eq!(serde_json::to_vec(&original_call_labels(fresh.clone())).unwrap(),serde_json::to_vec(&expected).unwrap());
            let expected=fresh;
            let (answers,_,projection)=session.prepared.execute_research_operations(&plan,&query,Some(&graph),&session.xml,None).unwrap().consume(&plan).unwrap();
            let shared=answers[0].analysis_control().unwrap();
            if let Some(first)=&same_graph_control {assert!(std::sync::Arc::ptr_eq(first,shared));}
            else {same_graph_control=Some(std::sync::Arc::clone(shared));}
            if let Some(previous)=&previous_control {assert!(!std::sync::Arc::ptr_eq(previous,shared));}
            let before=session.prepared.research_steps();
            let actual=session.prepared.project_research_relations_observed(&plan,&query,Some(&graph),&session.xml,projection,&mut |_|{}).unwrap();
            assert_eq!(session.prepared.research_steps()-before,cost);
            assert!(std::sync::Arc::ptr_eq(shared,actual.analysis.control_identity()));
            assert_eq!(serde_json::to_vec(&actual).unwrap(),serde_json::to_vec(&expected).unwrap());
            assert_eq!(serde_json::to_vec(&graph).unwrap(),graph_before,"memo is absent from the public Graph and snapshot bytes");
            if direction=="both" {previous_control=Some(std::sync::Arc::clone(shared));}
        }
    }
}

#[test]
fn shared_control_survives_real_large_projection_fallback_without_missing_rows() {
    let control=OperationControl::default();
    let callers=format!("{}-callers.lua","a".repeat(100));
    let session=ResearchSession::prepare(SnapshotRequest{
        files:BTreeMap::from([("target.lua".into(),"function Target() end\n".into()),(callers,"Target()\n".repeat(2500))]),
        // The other file still has complete source-candidate rows even though
        // it is not loaded. Avoid inventing execution evidence for those calls.
        load_documents:BTreeMap::from([("Main.toc".into(),"target.lua\n".into())]),
        environment_documents:BTreeMap::new(),source:json!({"repository":"shared-control-large-fallback","product":"retail"}),
        luac:LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},
        api:vec![],environment_manifest:None,
        load_plans:vec![LoadEvidence{manifest:"Main.toc".into(),ordered:vec!["target.lua".into()],issues:vec![]}],input_gaps:vec![]},&control).unwrap();
    let graph=session.prepared.build_xml_plan("shared-control-large-fallback",session.xml.programs[0].clone(),None,&control).unwrap();
    for direction in ["incoming","both"] {
        let query=planned_query("Target","target.lua",1,1,"function",direction);
        let plan=session.prepared.plan_research_operations(&query,&session.xml,None).unwrap();
        let expected=session.prepared.full_scan_research_relations(&query.symbol,direction,Some(&graph),&session.xml).unwrap();
        let before=session.prepared.research_steps();
        let fresh=session.prepared.research_relations(&query.symbol,direction,Some(&graph),&session.xml).unwrap();
        let cost=session.prepared.research_steps()-before;
        assert_eq!(serde_json::to_vec(&original_call_labels(fresh.clone())).unwrap(),serde_json::to_vec(&expected).unwrap());
        let expected=fresh;
        let (answers,_,projection)=session.prepared.execute_research_operations(&plan,&query,Some(&graph),&session.xml,None).unwrap().consume(&plan).unwrap();
        assert!(projection.is_none(),"whole incoming and retained context rows exceed the unchanged 1 MiB limit");
        let shared=answers[0].analysis_control().unwrap();
        let before=session.prepared.research_steps();let passes=research_projection_passes();
        let actual=session.prepared.project_research_relations_observed(&plan,&query,Some(&graph),&session.xml,projection,&mut |_|{}).unwrap();
        assert_eq!(research_projection_passes()-passes,1);
        assert_eq!(session.prepared.research_steps()-before,cost);
        assert!(std::sync::Arc::ptr_eq(shared,actual.analysis.control_identity()));
        assert_eq!(serde_json::to_vec(&actual).unwrap(),serde_json::to_vec(&expected).unwrap());
        assert!(serde_json::to_vec(&actual.relations).unwrap().len()>1<<20);
        assert_eq!(actual.relations.iter().filter(|row|row["kind"]=="call").count(),2500);
        let answer=serde_json::to_value(&answers[0]).unwrap();
        let incoming=session.prepared.research_relations(&query.symbol,"incoming",Some(&graph),&session.xml).unwrap();
        assert_eq!(answer["result"]["analysis"],incoming["analysis"]);
        assert_eq!(answer["result"]["relationsSHA256"],hash(&serde_json::to_vec(&incoming["relations"]).unwrap()));
    }
}

#[test]
fn shared_control_preserves_original_near_reserve_reconstruction_and_errors() {
    fn run(direction:&str,offset:isize,reuse:bool)->(std::result::Result<Value,Value>,usize,usize) {
        let session=context_projection_fixture();let control=session.prepared.operation_control();
        let graph=session.prepared.build_xml_plan("shared-control-reserve",session.xml.programs[0].clone(),None,control).unwrap();
        let query=planned_query("Target","target.lua",2,5,"function",direction);
        let plan=session.prepared.plan_research_operations(&query,&session.xml,None).unwrap();
        let (answers,_,projection)=session.prepared.execute_research_operations(&plan,&query,Some(&graph),&session.xml,None).unwrap().consume(&plan).unwrap();
        assert!(projection.is_some());
        let projection=if reuse {projection} else {drop(projection);None};
        let start=(session.prepared.inputs().limits().construction_steps-1024).checked_add_signed(offset).unwrap();
        session.prepared.carry_research_steps(start-session.prepared.research_steps(),control).unwrap();
        let passes=research_projection_passes();
        let result=session.prepared.project_research_relations_observed(&plan,&query,Some(&graph),&session.xml,projection,&mut |_|{})
            .map(|result|{
                assert!(std::sync::Arc::ptr_eq(answers[0].analysis_control().unwrap(),result.analysis.control_identity()));
                result.into_value()
            }).map_err(|error|json!({"code":error.code,"message":error.message,"details":error.details}));
        (result,session.prepared.research_steps(),research_projection_passes()-passes)
    }
    for direction in ["incoming","both"] {
        for offset in [-2,-1,0,1] {
            let expected=run(direction,offset,false);let actual=run(direction,offset,true);
            assert_eq!(actual.0,expected.0,"full answer or original first error: {direction}, {offset}");
            assert_eq!(actual.1,expected.1,"same exact charge prefix: {direction}, {offset}");
            assert_eq!(actual.2,1,"the reserve boundary still reconstructs rows: {direction}, {offset}");
        }
    }
}
