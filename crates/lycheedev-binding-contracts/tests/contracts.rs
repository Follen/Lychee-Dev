use lycheedev_binding_contracts::*;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn defaults_and_raw_metadata_round_trip() {
    let value = json!({"kind":"function", "name":"GetFrame", "returns":[
        {"index":1,"name":"frame","type":"Frame","raw":{"Secret":true}}
    ],"raw":{"MayReturnNothing":true,"nested":{"unchanged":[1,"two",null]}}});
    let record: EnvironmentRecord = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(record.returns[0].inner_type, "");
    assert!(!record.returns[0].nilable);
    assert_eq!(record.returns[0].line, 0);
    assert!(record.owner.is_none());
    assert!(
        record.parameters.is_empty()
            && record.payload.is_empty()
            && record.fields.is_empty()
            && record.values.is_empty()
    );
    let output = serde_json::to_value(&record).unwrap();
    assert_eq!(output["raw"], value["raw"]);
    assert_eq!(output["returns"][0]["raw"], value["returns"][0]["raw"]);
    assert_eq!(output["returns"][0]["type"], "Frame");
    assert!(output["returns"][0].get("type_name").is_none());
    assert_eq!(output["declarationRole"], "");
    assert_eq!(output["endLine"], 0);
    let restored: EnvironmentRecord = serde_json::from_value(output).unwrap();
    assert_eq!(restored.raw, record.raw);
}

fn callable() -> EnvironmentCallable {
    EnvironmentCallable {
        origin: EnvironmentOrigin {
            material: "api.json".into(),
            material_sha256: "a".repeat(64),
            pointer: "/0".into(),
            identity: "api:0".into(),
        },
        declaration: EnvironmentRecord {
            kind: "function".into(),
            name: "GetFrame".into(),
            returns: vec![EnvironmentSlot {
                index: 1,
                name: "frame".into(),
                type_name: "Frame".into(),
                ..Default::default()
            }],
            owner: Some(EnvironmentOwner {
                kind: "script-object".into(),
                name: "FrameOwner".into(),
                source: json!({"path":"api.json"}),
            }),
            ..Default::default()
        },
        types: Arc::new(EnvironmentTypeCatalog {
            identity: "catalog".into(),
            nominal_objects: BTreeMap::from([("Frame".into(), json!({"owner":"FrameOwner"}))]),
            ..Default::default()
        }),
    }
}

#[test]
fn nominal_return_is_one_based_exact_and_non_nilable() {
    let mut value = callable();
    assert_eq!(value.nominal_return_name(0), None);
    assert_eq!(value.nominal_return_name(1), Some("Frame"));
    assert_eq!(value.nominal_return_name(2), None);
    value.declaration.returns[0].nilable = true;
    assert_eq!(value.nominal_return_name(1), None);
    value.declaration.returns[0].nilable = false;
    value.declaration.returns[0].index = 2;
    assert_eq!(value.nominal_return_name(1), None);
    value.declaration.returns[0].index = 1;
    Arc::make_mut(&mut value.types).nominal_objects.clear();
    assert_eq!(value.nominal_return_name(1), None);
}

#[test]
fn receiver_requires_unique_nominal_script_object_owner() {
    let mut value = callable();
    assert_eq!(value.receiver_type_name(), Some("Frame"));
    Arc::make_mut(&mut value.types)
        .nominal_objects
        .insert("Other".into(), json!({"owner":"FrameOwner"}));
    assert_eq!(value.receiver_type_name(), None);
    Arc::make_mut(&mut value.types)
        .nominal_objects
        .remove("Other");
    value.declaration.owner.as_mut().unwrap().kind = "namespace".into();
    assert_eq!(value.receiver_type_name(), None);
    value.declaration.owner = None;
    assert_eq!(value.receiver_type_name(), None);
}

#[test]
fn decisions_keep_absence_unavailable_and_unmanaged_distinct() {
    let absent: BindingDecision<()> = BindingDecision::KnownAbsent { proof: "p".into() };
    let unavailable: BindingDecision<()> = BindingDecision::Unavailable {
        proof: "p".into(),
        reason: "budget".into(),
    };
    assert!(matches!(absent, BindingDecision::KnownAbsent { .. }));
    assert!(matches!(unavailable,BindingDecision::Unavailable {reason,..} if reason=="budget"));
    assert!(matches!(
        BindingDecision::<()>::NotManaged,
        BindingDecision::NotManaged
    ));
    assert!(matches!(
        BindingDecision::Bound(7),
        BindingDecision::Bound(7)
    ));
}

#[test]
fn object_view_identity_includes_context_observation_and_object() {
    let key = ObjectViewKey {
        context: "a".into(),
        observation: 1,
        object_id: 2,
    };
    let mut keys = std::collections::HashSet::from([key.clone()]);
    assert!(!keys.insert(key.clone()));
    assert!(keys.insert(ObjectViewKey {
        context: "b".into(),
        ..key.clone()
    }));
    assert!(keys.insert(ObjectViewKey {
        observation: 2,
        ..key.clone()
    }));
    assert!(keys.insert(ObjectViewKey {
        object_id: 3,
        ..key
    }));
}
