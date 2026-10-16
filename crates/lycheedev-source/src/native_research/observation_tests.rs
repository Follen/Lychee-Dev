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
