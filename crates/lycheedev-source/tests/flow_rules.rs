use lycheedev_source::flow;
use lycheedev_core::control::OperationControl;
use std::collections::BTreeMap;
use serde_json::json;
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
