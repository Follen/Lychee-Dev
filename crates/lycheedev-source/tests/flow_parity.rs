use lycheedev_core::control::OperationControl;
use lycheedev_source::flow::{self,Budget,RuleSet,Target};
use serde_json::Value;
use std::collections::BTreeMap;

#[test]
fn frozen_go_secret_flow_all_fields() {
    let fixture:Value=serde_json::from_str(include_str!("../src/flow/fixtures/frozen-go.json")).unwrap();
    let rules:RuleSet=serde_json::from_value(fixture["rules"].clone()).unwrap();
    let mut failures=Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        // Owner-approved Lua 5.1 identifier compatibility is an improvement;
        // keep the original Go rejection in the fixture instead of normalizing it.
        if case["name"]=="goto-identifier" {continue;}
        let files=case["files"].as_object().unwrap().iter().map(|(path,text)|(path.clone(),text.as_str().unwrap().as_bytes().to_vec())).collect::<BTreeMap<_,_>>();
        let target:Target=serde_json::from_value(case["target"].clone()).unwrap();
        let mut budget=Budget::default();
        if let Some(max)=case["budget"]["maxPaths"].as_u64(){budget.max_paths=max as usize;}
        match flow::analyze(&files,&target,&rules,budget,&OperationControl::default()) {
            Ok(actual)=>{let actual=serde_json::to_value(actual).unwrap();if actual!=case["expected"]{failures.push(serde_json::json!({"case":case["name"],"actual":actual,"expected":case["expected"]}));}},
            Err(error)=>failures.push(serde_json::json!({"case":case["name"],"error":error.to_string()})),
        }
    }
    if !failures.is_empty(){let report=serde_json::to_string_pretty(&failures).unwrap();let out=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/flow-oracle/differences.json");std::fs::write(out,&report).unwrap();panic!("{} real all-field differences; .tmp/flow-oracle/differences.json\n{report}",failures.len());}
}

fn fixture_rules()->RuleSet{serde_json::from_str::<Value>(include_str!("../src/flow/fixtures/frozen-go.json")).map(|v|serde_json::from_value(v["rules"].clone()).unwrap()).unwrap()}

#[test]
fn lua51_goto_identifier_is_observed_but_jump_syntax_is_rejected() {
    let rules=fixture_rules();
    let target=Target{path:"main.lua".into(),..Default::default()};
    let files=BTreeMap::from([("main.lua".into(),b"local goto=Source()\nUse(goto)\n".to_vec())]);
    let result=flow::analyze(&files,&target,&rules,Budget::default(),&OperationControl::default()).unwrap();
    assert!(result.coverage.complete);
    assert_eq!(result.findings.len(),1);
    assert_eq!(result.findings[0].steps.iter().find(|s|s.kind=="assignment").unwrap().name,"goto");
    for code in ["goto label\n::label::\n","::label::\nUse(Source())\n","local x=1//2\nUse(Source())\n"] {
        let files=BTreeMap::from([("main.lua".into(),code.as_bytes().to_vec())]);
        let result=flow::analyze(&files,&target,&rules,Budget::default(),&OperationControl::default()).unwrap();
        assert!(!result.coverage.complete,"{code}");
        assert!(result.findings.is_empty(),"Lua 5.2+ syntax entered Lua 5.1 execution model: {code}");
        assert!(result.boundaries.iter().any(|b|b.code=="parse_error"));
    }
}

#[test]
fn caller_control_and_actual_input_and_output_admission() {
    let rules=fixture_rules();let target=Target{path:"main.lua".into(),..Default::default()};
    let control=OperationControl::default();control.cancel_handle().store(true,std::sync::atomic::Ordering::Release);
    assert_eq!(flow::analyze(&BTreeMap::from([("main.lua".into(),b"Use(Source())".to_vec())]),&target,&rules,Budget::default(),&control).unwrap_err().exit_code,7);
    assert_eq!(flow::analyze(&BTreeMap::from([("main.lua".into(),vec![b'x';(1<<20)+1])]),&target,&rules,Budget::default(),&OperationControl::default()).unwrap_err().code,"flow.budget");
    assert_eq!(flow::analyze(&BTreeMap::from([("../main.lua".into(),b"Use(Source())".to_vec())]),&Target::default(),&rules,Budget::default(),&OperationControl::default()).unwrap_err().code,"flow.invalid_input");
    let mut huge=rules;huge.identity.client="\u{1}".repeat(256);huge.identity.api_commit="\u{1}".repeat(256);huge.identity.metadata_digest="\u{1}".repeat(256);huge.identity.version="\u{1}".repeat(256);
    let budget=Budget{max_output_bytes:2048,..Default::default()};
    assert_eq!(flow::analyze(&BTreeMap::from([("main.lua".into(),b"Use(Source())".to_vec())]),&target,&huge,budget,&OperationControl::default()).unwrap_err().code,"flow.budget");
}

#[test]
fn pinned_secret_metadata_conditions_events_and_ambiguity() {
    use lycheedev_source::environment::{self,Identity,Limits};
    let documents=BTreeMap::from([("Generated.lua".into(),br#"local API={Name="Demo",Type="System",Functions={{Name="Source",Type="Function",SecretReturns=true,SecretArguments="AllowedWhenUntainted",Arguments={{Name="v",Type="number"}},Returns={{Name="v",Type="number"}}},{Name="Sink",Type="Function",SecretArguments="AllowedWhenUntainted",Arguments={{Name="v",Type="number"}}},{Name="Sink",Type="Function",SecretArguments="AllowedWhenTainted",Arguments={{Name="v",Type="number"}}},{Name="Sink",Type="Function",SecretArguments="AllowedWhenUntainted",Arguments={{Name="v",Type="number"}}}},Events={{Name="EVENT_X",Type="Event",SecretPayloads=true,Payload={{Name="v",Type="number"}}}}}
APIDocumentation:AddDocumentationTable(API)
"#.to_vec())]);
    let control=OperationControl::default();
    let parsed=environment::build(Identity{repository:"Blizzard_APIDocumentationGenerated".into(),commit:"a".repeat(40),client:"retail".into(),dependency_commits:Default::default(),generator_version:environment::GENERATOR.into()},&documents,Limits::default(),&control).unwrap();
    let identity=flow::RuleIdentity{client:"retail".into(),api_commit:"a".repeat(40),metadata_digest:parsed.manifest.input_sha256.clone(),version:"1".into()};
    let rules=flow::from_facts(identity.clone(),&parsed,&control).unwrap();
    assert_eq!(rules.calls["Source"].returns[&1].state,"possible");
    assert_eq!(rules.calls["Source"].arguments[&1].secret,"conditional");
    assert_eq!(rules.calls["Source"].arguments[&1].condition,"SecretArguments=AllowedWhenUntainted");
    assert!(!rules.calls.contains_key("Demo.Source"));
    assert!(!rules.calls.contains_key("Sink"));
    assert!(!rules.calls.contains_key("EVENT_X"));
    assert_eq!(rules.events["EVENT_X"].returns[&1].state,"possible");
    assert!(rules.issues.iter().any(|i|i.code=="duplicate_rule"));
    assert!(rules.issues.iter().any(|i|i.code=="event_payload_mapping_unavailable"));
    let mut wrong=identity;wrong.metadata_digest="b".repeat(64);
    assert_eq!(flow::from_facts(wrong,&parsed,&control).unwrap_err().exit_code,4);
}
