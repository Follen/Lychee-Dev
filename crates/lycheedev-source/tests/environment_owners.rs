use lycheedev_source::environment::{self, Identity, Limits};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn script_object_owners_preserve_old_method_names_and_original_source() {
    let raw = "\u{feff}local API = {\r\n Name='WidgetAPI', Type='ScriptObject', ObjectType='Userdata',\r\n Functions={{Name='Shared',Type='Function',Arguments={{Name='cb',Type='Done',Nilable=false}}},{Name='MethodOnly',Type='Function'}},\r\n Tables={{Name='Done',Type='CallbackType',Arguments={{Name='ok',Type='bool'}},Returns={{Name='value',Type='number'}}}}\r\n}\r\nAPIDocumentation:AddDocumentationTable(API)\r\n";
    let documents = BTreeMap::from([
        ("API.lua".into(), raw.as_bytes().to_vec()),
        ("Global.lua".into(), b"APIDocumentation:AddDocumentationTable({Name='Global',Type='System',Functions={{Name='Shared',Type='Function'}}})".to_vec()),
    ]);
    let identity = Identity { repository:"wow-ui-source".into(),commit:"a".repeat(40),client:"retail".into(),dependency_commits:BTreeMap::new(),generator_version:environment::GENERATOR.into() };
    let result = environment::build(identity.clone(),&documents,Limits::default(),&Default::default()).unwrap();
    let repeated = environment::build(identity,&documents,Limits::default(),&Default::default()).unwrap();
    assert_eq!(result.manifest.facts_sha256,repeated.manifest.facts_sha256);
    let root = result.facts.iter().find(|record|record.declaration_role=="script-object-root").unwrap();
    assert_eq!(root.name,"WidgetAPI");
    assert_eq!(root.kind,"script-object");
    assert_eq!(root.raw["ObjectType"],"Userdata");
    assert!(root.owner.is_none());
    let owned = result.facts.iter().filter(|record|record.owner.is_some()).collect::<Vec<_>>();
    assert_eq!(owned.iter().map(|record|record.name.as_str()).collect::<Vec<_>>(),vec!["MethodOnly","Shared"]);
    for method in owned {
        let owner=method.owner.as_ref().unwrap();
        assert_eq!(owner.name,"WidgetAPI");
        assert_eq!(owner.kind,"script-object");
        assert_eq!(owner.source["rules"],environment::DECLARATION_RULES);
        assert_eq!(owner.source["contentHash"],lycheedev_core::storage::sha256(raw.as_bytes()));
        assert_eq!(owner.source["startByte"],raw.find('{').unwrap());
        assert_eq!(owner.source["endByte"],raw.find("\r\nAPIDocumentation").unwrap());
        assert_eq!(owner.source["coordinate"],"utf8-byte");
    }
    assert_eq!(result.facts.iter().filter(|record|record.name=="Shared").count(),2);
    assert!(result.facts.iter().any(|record|record.name=="Shared"&&record.owner.is_none()));
    let rules=lycheedev_source::flow::from_facts(lycheedev_source::flow::RuleIdentity{
        client:result.manifest.identity.client.clone(),api_commit:result.manifest.identity.commit.clone(),
        metadata_digest:result.manifest.input_sha256.clone(),version:"owner-aware-fixture-v1".into(),
    },&result,&Default::default()).unwrap();
    // This unchanged declaration fixture has no secret/taint annotations.
    // from_facts returns risk rules only, not a general callable directory.
    assert!(rules.calls.is_empty());
    assert!(result.definitions.contains("function MethodOnly() end"));
    assert!(result.definitions.contains("---@alias Done fun(ok:boolean):number"));
    assert!(!result.definitions.contains("---@class WidgetAPI"));
    assert!(result.coverage.gaps.iter().any(|gap|gap.code=="script_object_nominal_identity_unavailable"));
    assert!(!result.coverage.gaps.iter().any(|gap|gap.code=="unsupported_record_kind"&&gap.detail=="callbacktype"));
    let encoded=serde_json::to_value(&result.facts).unwrap();
    let decoded:Vec<environment::ApiRecord>=serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(),encoded);
    assert_eq!(root.observed_span["contentHash"],json!(lycheedev_core::storage::sha256(raw.as_bytes())));
}

#[test]
fn owner_methods_do_not_collide_with_global_secret_rules() {
    let documents=BTreeMap::from([
        ("API.lua".into(),b"APIDocumentation:AddDocumentationTable({Name='WidgetAPI',Type='ScriptObject',Functions={{Name='Shared',Type='Function',SecretArguments='AllowedWhenUntainted',Arguments={{Name='value',Type='number'}}},{Name='MethodOnly',Type='Function',SecretReturns=true,Returns={{Name='value',Type='number'}}}}})".to_vec()),
        ("Global.lua".into(),b"APIDocumentation:AddDocumentationTable({Name='Global',Type='System',Functions={{Name='Shared',Type='Function',SecretArguments='NotAllowed',Arguments={{Name='value',Type='number'}}}}})".to_vec()),
    ]);
    let environment=environment::build(Identity{repository:"wow-ui-source".into(),commit:"a".repeat(40),client:"retail".into(),dependency_commits:BTreeMap::new(),generator_version:environment::GENERATOR.into()},&documents,Limits::default(),&Default::default()).unwrap();
    assert_eq!(environment.facts.iter().filter(|record|record.name=="Shared").count(),2);
    assert!(environment.facts.iter().any(|record|record.name=="MethodOnly"&&record.owner.is_some()));
    let rules=lycheedev_source::flow::from_facts(lycheedev_source::flow::RuleIdentity{
        client:environment.manifest.identity.client.clone(),api_commit:environment.manifest.identity.commit.clone(),
        metadata_digest:environment.manifest.input_sha256.clone(),version:"owner-aware-fixture-v1".into(),
    },&environment,&Default::default()).unwrap();
    assert_eq!(rules.calls.len(),1);
    let shared=&rules.calls["Shared"];
    assert_eq!(shared.path,"Global.lua");
    assert_eq!(shared.raw["SecretArguments"],"NotAllowed");
    assert_eq!(shared.arguments[&1].condition,"SecretArguments=NotAllowed");
    assert!(!rules.calls.contains_key("MethodOnly"));
    assert!(!rules.issues.iter().any(|issue|issue.code=="duplicate_rule"));
}
