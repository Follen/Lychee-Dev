//! Current bound-domain identity, coverage and ownership tests.
use super::*;
use crate::engine::{ResultOwner, lifetime::SessionMemory};
use lycheedev_core::control::OperationControl;
use serde_json::json;

type Case = (DomainRequest, Value, Vec<Value>);
fn table(name: &str) -> Value {
    json!({"name":name,"mode":"static","hotfix_captures":[]})
}
fn lookup(name: &str, id: u32) -> Value {
    source(json!({"kind":"domain_lookup","table":table(name),"ids":[id]}))
}
fn scan(name: &str, filter: Option<(&str, u32)>, limit: usize) -> Value {
    source(json!({"kind":"domain_scan","table":table(name),"filter":filter,"limit":limit}))
}
fn source(request: Value) -> Value {
    let name = request["table"]["name"].clone();
    json!({"request":request,"physicalLocatorIdentity":"authenticated_source_index","complete":true,"sources":[{"sourceMode":"static","sourceIdentity":{"table":{"name":name}},"publicIdEqualsPhysicalLocator":true,"effective":null,"complete":true}]})
}
fn decorated(mut report: Value) -> Value {
    report["complete"] = json!(true);
    report["truncated"] = json!(false);
    report["relations"] = json!([]);
    report
}
fn cases() -> Vec<Case> {
    let mode = SourceMode::Static;
    vec![
        (
            DomainRequest::ItemGet {
                item_id: 9,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"item":{"id":9}})),
            vec![lookup("ItemSparse", 9), lookup("Item", 9)],
        ),
        (
            DomainRequest::ItemModels {
                item_id: 9,
                race_id: -4,
                gender: 2,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"itemID":9,"raceID":-4,"gender":2,"displayID":0})),
            vec![
                lookup("ItemSparse", 9),
                scan("ItemModifiedAppearance", Some(("ItemID", 9)), 65536),
            ],
        ),
        (
            DomainRequest::ItemTextures {
                item_id: 9,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"itemID":9,"displayID":0})),
            vec![
                lookup("ItemSparse", 9),
                scan("ItemModifiedAppearance", Some(("ItemID", 9)), 65536),
            ],
        ),
        (
            DomainRequest::ItemGeosets {
                item_id: 9,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"itemID":9,"displayID":0})),
            vec![
                lookup("ItemSparse", 9),
                scan("ItemModifiedAppearance", Some(("ItemID", 9)), 65536),
            ],
        ),
        (
            DomainRequest::SpellInfo {
                spell_id: 9,
                max_depth: 5,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"spellID":9})),
            vec![
                lookup("SpellName", 9),
                lookup("Spell", 9),
                scan("SpellEffect", Some(("SpellID", 9)), 65536),
                scan("SpellMisc", Some(("SpellID", 9)), 1),
            ],
        ),
        (
            DomainRequest::SpellAuras {
                spell_id: 9,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"spellID":9})),
            vec![scan("SpellEffect", Some(("SpellID", 9)), 65536)],
        ),
        (
            DomainRequest::SpellSummons {
                spell_id: 9,
                npc_id: 17,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"spellID":9,"npcID":17})),
            vec![scan("SpellEffect", Some(("SpellID", 9)), 65536)],
        ),
        (
            DomainRequest::EncounterGet {
                encounter_id: 9,
                max_depth: 32,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"journalEncounterID":9})),
            vec![scan(
                "JournalEncounterSection",
                Some(("JournalEncounterID", 9)),
                10000,
            )],
        ),
        (
            DomainRequest::DecorList {
                limit: 3,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"count":0,"items":[]})),
            vec![scan("HouseDecor", None, 3)],
        ),
        (
            DomainRequest::DecorGet {
                field: DecorSelector::ItemId,
                wanted: 9,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"item":{"id":1,"itemID":9,"modelFileDataID":11}})),
            vec![scan("HouseDecor", Some(("ItemID", 9)), 5000)],
        ),
        (
            DomainRequest::CreatureDisplay {
                display_id: 9,
                file_data_id: 0,
                mode: mode.clone(),
                hotfix_captures: vec![],
            },
            decorated(json!({"displayID":9,"fileDataID":11})),
            vec![
                lookup("CreatureDisplayInfo", 9),
                scan(
                    "CreatureDisplayInfoGeosetData",
                    Some(("CreatureDisplayInfoID", 9)),
                    65536,
                ),
            ],
        ),
        (
            DomainRequest::CreatureModel {
                file_data_id: 9,
                mode,
                hotfix_captures: vec![],
            },
            decorated(json!({"fileDataID":9,"displays":[]})),
            vec![scan("CreatureModelData", Some(("FileDataID", 9)), 65536)],
        ),
    ]
}
fn memory() -> SessionMemory {
    SessionMemory::new(16 << 20).unwrap()
}
fn owner(memory: &SessionMemory) -> DomainOwner {
    DomainOwner::new(
        memory
            .begin_bounded(100_000_000, OperationControl::default(), 8 << 20)
            .unwrap(),
        memory.budget.clone(),
    )
}
// Only test fixtures can synthesize this internal ResultOwner; no constructor
// accepting caller JSON or session tokens is exposed by the production API.
fn owned(memory: &SessionMemory, value: Value, dependencies: Vec<String>) -> OwnedProvenance {
    ResultOwner {
        schema: None,
        dependencies,
        batches: vec![],
        columns: vec![],
        provenance: value,
        provenance_bytes: 4096,
        complete: true,
        rows: 0,
        bytes: 0,
        session: memory.budget.clone(),
        page_bytes: 4096,
        _lifetimes: vec![],
        _output: None,
    }
    .owned_provenance()
    .unwrap()
}
fn finish(
    memory: &SessionMemory,
    request: &DomainRequest,
    report: Value,
    sources: Vec<Value>,
) -> Result<DomainResult> {
    let owner = owner(memory).bind_request(request)?;
    owner.budget().charge(Cost::retained(8192)).unwrap();
    let captures = binding(request).1;
    let sources = sources
        .into_iter()
        .map(|value| owned(memory, value, captures.to_vec()))
        .collect();
    owner.finish(report, sources)
}
fn code(result: Result<DomainResult>) -> String {
    result.err().expect("must reject").code
}

#[test]
fn all_twelve_typed_requests_bind_and_validate_through_one_domain_result() {
    for (request, report, sources) in cases() {
        let memory = memory();
        let result = finish(&memory, &request, report, sources).unwrap();
        result
            .validate_request(&request, &result._owner.lifetime.budget)
            .unwrap();
        assert_eq!(result._owner.bound_request.as_ref(), Some(&request));
        let serialized = serde_json::to_value(&result).unwrap();
        assert!(serialized["report"]["relations"].is_array());
        assert!(!serialized["sources"].as_array().unwrap().is_empty());
        drop(result);
        assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, 0);
    }
}
#[test]
fn bound_request_cannot_be_relabelled_as_any_other_variant() {
    let cases = cases();
    for (index, (request, report, sources)) in cases.iter().enumerate() {
        let memory = memory();
        let result = finish(&memory, request, report.clone(), sources.clone()).unwrap();
        for (other, (request, _, _)) in cases.iter().enumerate() {
            if other != index {
                assert_eq!(
                    result
                        .validate_request(request, &result._owner.lifetime.budget)
                        .unwrap_err()
                        .code,
                    "data.domain.request_identity"
                );
            }
        }
    }
}
#[test]
fn exact_binding_includes_nonidentity_options_and_capture_order() {
    for (request, report, sources) in cases() {
        let memory = memory();
        let result = finish(&memory, &request, report, sources).unwrap();
        let mut wrong = request.clone();
        match &mut wrong {
            DomainRequest::ItemGet { item_id, .. }
            | DomainRequest::ItemTextures { item_id, .. }
            | DomainRequest::ItemGeosets { item_id, .. } => *item_id += 1,
            DomainRequest::ItemModels { race_id, .. } => *race_id += 1,
            DomainRequest::SpellInfo { max_depth, .. }
            | DomainRequest::EncounterGet { max_depth, .. } => *max_depth -= 1,
            DomainRequest::SpellAuras { spell_id, .. } => *spell_id += 1,
            DomainRequest::SpellSummons { npc_id, .. } => *npc_id += 1,
            DomainRequest::DecorList { limit, .. } => *limit += 1,
            DomainRequest::DecorGet { field, .. } => *field = DecorSelector::Id,
            DomainRequest::CreatureDisplay { display_id, .. } => *display_id += 1,
            DomainRequest::CreatureModel { file_data_id, .. } => *file_data_id += 1,
        }
        assert_eq!(
            result
                .validate_request(&wrong, &result._owner.lifetime.budget)
                .unwrap_err()
                .code,
            "data.domain.request_identity"
        );
    }
    let (mut request, report, mut sources) = cases().remove(0);
    let DomainRequest::ItemGet {
        mode,
        hotfix_captures,
        ..
    } = &mut request
    else {
        unreachable!()
    };
    *mode = SourceMode::Effective;
    *hotfix_captures = vec!["CAP-b".into(), "CAP-a".into()];
    for source in &mut sources {
        effective(source, &["CAP-b", "CAP-a"]);
    }
    let memory = memory();
    let result = finish(&memory, &request, report, sources).unwrap();
    let DomainRequest::ItemGet {
        hotfix_captures, ..
    } = &mut request
    else {
        unreachable!()
    };
    hotfix_captures.reverse();
    assert_eq!(
        result
            .validate_request(&request, &result._owner.lifetime.budget)
            .unwrap_err()
            .code,
        "data.domain.request_identity"
    );
}
#[test]
fn every_report_must_match_its_exact_request_identity() {
    for (request, mut report, sources) in cases() {
        match &request {
            DomainRequest::ItemGet { .. } => report["item"]["id"] = json!(10),
            DomainRequest::ItemModels { .. } => report["gender"] = json!(3),
            DomainRequest::ItemTextures { .. } | DomainRequest::ItemGeosets { .. } => {
                report["itemID"] = json!(10)
            }
            DomainRequest::SpellInfo { .. } | DomainRequest::SpellAuras { .. } => {
                report["spellID"] = json!(10)
            }
            DomainRequest::SpellSummons { .. } => report["npcID"] = json!(18),
            DomainRequest::EncounterGet { .. } => report["journalEncounterID"] = json!(10),
            DomainRequest::DecorList { .. } => report["count"] = json!(1),
            DomainRequest::DecorGet { .. } => report["item"]["itemID"] = json!(10),
            DomainRequest::CreatureDisplay { .. } => report["displayID"] = json!(10),
            DomainRequest::CreatureModel { .. } => report["fileDataID"] = json!(10),
        }
        assert_eq!(
            code(finish(&memory(), &request, report, sources)),
            "data.domain.request_identity"
        );
    }
}
#[test]
fn all_domains_reject_unknown_roles_and_missing_seed_sources() {
    for (request, report, sources) in cases() {
        let mut wrong = sources.clone();
        wrong.push(lookup("UnrelatedPrivateTable", 9));
        assert_eq!(
            code(finish(&memory(), &request, report.clone(), wrong)),
            "data.domain.request_identity"
        );
        assert_eq!(
            code(finish(&memory(), &request, report, vec![])),
            "data.domain.request_identity"
        );
    }
}
#[test]
fn typed_provenance_rejects_wrong_kind_ids_filter_limit_and_unknown_fields() {
    let (request, report, sources) = cases().remove(0);
    for mutation in 0..6 {
        let mut wrong = sources.clone();
        match mutation {
            0 => wrong[0]["request"]["kind"] = json!("sql"),
            1 => wrong[0]["request"]["ids"] = json!([10]),
            2 => wrong[0]["request"]["ids"] = json!([9, 9]),
            3 => wrong[0]["request"]["sql"] = json!("SELECT 9"),
            4 => wrong[0]["physicalLocatorIdentity"] = json!("caller_claim"),
            _ => wrong[0]["sources"][0]["sourceIdentity"]["table"]["name"] = json!("Spell"),
        }
        assert_eq!(
            code(finish(&memory(), &request, report.clone(), wrong)),
            "data.domain.request_identity"
        );
    }
    let (request, report, sources) = cases().remove(8);
    for mutation in 0..3 {
        let mut wrong = sources.clone();
        match mutation {
            0 => wrong[0]["request"]["limit"] = json!(4),
            1 => wrong[0]["request"]["filter"] = json!(["ID", 9]),
            _ => wrong[0]["request"]["filter"] = json!(["ID", 9, "extra"]),
        }
        assert_eq!(
            code(finish(&memory(), &request, report.clone(), wrong)),
            "data.domain.request_identity"
        );
    }
}
fn effective(source: &mut Value, history: &[&str]) {
    source["request"]["table"]["mode"] = json!("effective");
    source["request"]["table"]["hotfix_captures"] = json!(history);
    source["sources"][0]["sourceMode"] = json!("effective");
    source["sources"][0]["effective"] = json!({"table":source["request"]["table"]["name"],"captures":history.iter().map(|id|json!({"id":id})).collect::<Vec<_>>()});
}
#[test]
fn mode_history_receipts_and_private_dependencies_all_have_to_match() {
    let (mut request, report, mut sources) = cases().remove(0);
    let DomainRequest::ItemGet {
        mode,
        hotfix_captures,
        ..
    } = &mut request
    else {
        unreachable!()
    };
    *mode = SourceMode::Effective;
    *hotfix_captures = vec!["CAP-b".into(), "CAP-a".into()];
    for source in &mut sources {
        effective(source, &["CAP-b", "CAP-a"]);
    }
    for mutation in 0..5 {
        let mut wrong = sources.clone();
        match mutation {
            0 => wrong[0]["request"]["table"]["hotfix_captures"] = json!(["CAP-a", "CAP-b"]),
            1 => {
                wrong[0]["sources"][0]["effective"]["captures"] =
                    json!([{"id":"CAP-a"},{"id":"CAP-b"}])
            }
            2 => wrong[0]["sources"][0]["sourceMode"] = json!("static"),
            3 => wrong[0]["request"]["table"]["mode"] = json!("static"),
            _ => wrong[0]["sources"][0]["effective"]["table"] = json!("Spell"),
        }
        assert_eq!(
            code(finish(&memory(), &request, report.clone(), wrong)),
            "data.domain.request_identity"
        );
    }
    let memory = memory();
    let owner = owner(&memory).bind_request(&request).unwrap();
    let wrong = sources
        .into_iter()
        .map(|source| owned(&memory, source, vec!["CAP-b".into(), "CAP-wrong".into()]))
        .collect();
    assert_eq!(
        code(owner.finish(report, wrong)),
        "data.domain.request_identity"
    );
}
#[test]
fn cross_session_owned_sources_are_rejected_before_request_validation() {
    let (request, report, sources) = cases().remove(0);
    let first = memory();
    let second = memory();
    let owner = owner(&first).bind_request(&request).unwrap();
    let foreign = sources
        .into_iter()
        .map(|source| owned(&second, source, vec![]))
        .collect();
    assert_eq!(
        code(owner.finish(report, foreign)),
        "records.source_identity"
    );
}
#[test]
fn explicit_partial_and_business_truncation_are_preserved_not_previewed() {
    let (request, mut report, mut sources) = cases().remove(0);
    report["complete"] = json!(false);
    report["partial"] = json!(true);
    report["partialReasons"] = json!(["row_missing"]);
    report["truncated"] = json!(true);
    report["relations"] = json!([{"fromTable":"Item","fromField":"ID","fromID":9,"toKind":"item","toID":9,"depth":0}]);
    report["additionalFullPayload"] = json!("full payload, never a preview");
    sources[1]["complete"] = json!(false);
    sources[1]["sources"][0]["complete"] = json!(false);
    let memory = memory();
    let result = finish(&memory, &request, report.clone(), sources).unwrap();
    result
        .validate_request(&request, &result._owner.lifetime.budget)
        .unwrap();
    assert!(!result.complete().unwrap());
    assert!(result.truncated().unwrap());
    assert_eq!(serde_json::to_value(&result).unwrap()["report"], report);
}
#[test]
fn complete_report_cannot_hide_incomplete_source_or_truncation() {
    let (request, report, sources) = cases().remove(0);
    for mutation in 0..5 {
        let mut report = report.clone();
        let mut sources = sources.clone();
        match mutation {
            0 => sources[0]["complete"] = json!(false),
            1 => sources[0]["sources"][0]["complete"] = json!(false),
            2 => report["truncated"] = json!(true),
            3 => report["partial"] = json!(true),
            _ => report["relations"] = Value::Null,
        }
        assert_eq!(
            code(finish(&memory(), &request, report, sources)),
            "data.domain.request_identity"
        );
    }
}
#[test]
fn missing_completeness_is_not_inferred_as_success() {
    let (request, report, sources) = cases().remove(0);
    for key in ["complete", "truncated"] {
        let mut report = report.clone();
        report.as_object_mut().unwrap().remove(key);
        assert_eq!(
            code(finish(&memory(), &request, report, sources.clone())),
            "data.domain.completeness_missing"
        );
    }
}
#[test]
fn unbound_owners_cannot_adopt_domain_sources_at_capture_time() {
    let (request, report, sources) = cases().remove(0);
    let memory = memory();
    let owner = owner(&memory);
    let sources = sources
        .into_iter()
        .map(|source| owned(&memory, source, vec![]))
        .collect();
    let result = owner.finish(report, sources).unwrap();
    assert_eq!(
        result
            .validate_request(&request, &result._owner.lifetime.budget)
            .unwrap_err()
            .code,
        "data.domain.request_identity"
    );
}
#[test]
fn alternate_decor_and_creature_selectors_are_bound_explicitly() {
    for (field, key, column) in [
        (DecorSelector::Id, "id", "ID"),
        (
            DecorSelector::ModelFileDataId,
            "modelFileDataID",
            "ModelFileDataID",
        ),
    ] {
        let request = DomainRequest::DecorGet {
            field,
            wanted: 9,
            mode: SourceMode::Static,
            hotfix_captures: vec![],
        };
        let mut report = decorated(json!({"item":{}}));
        report["item"][key] = json!(9);
        finish(
            &memory(),
            &request,
            report,
            vec![scan("HouseDecor", Some((column, 9)), 5000)],
        )
        .unwrap();
    }
    let request = DomainRequest::CreatureDisplay {
        display_id: 0,
        file_data_id: 9,
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    };
    let report = decorated(json!({"displayID":19,"fileDataID":9,"requestedFileDataID":9}));
    let sources = vec![
        scan("CreatureModelData", Some(("FileDataID", 9)), 65536),
        scan("CreatureDisplayInfo", Some(("ModelID", 29)), 65536),
        lookup("CreatureModelData", 29),
        scan(
            "CreatureDisplayInfoGeosetData",
            Some(("CreatureDisplayInfoID", 19)),
            65536,
        ),
    ];
    finish(&memory(), &request, report, sources).unwrap();
}
#[test]
fn domain_response_retains_and_refunds_shared_admission_through_serialization() {
    let memory = memory();
    let owner = owner(&memory);
    owner.budget().charge(Cost::retained(1024)).unwrap();
    let result = owner.finish(json!({"complete":false}), vec![]).unwrap();
    let retained = memory.budget.snapshot().unwrap().retained_bytes;
    assert!(retained >= 1024);
    serde_json::to_writer(std::io::sink(), &result).unwrap();
    assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, retained);
    drop(result);
    assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn a_domain_owner_cannot_be_rebound() {
    let request = cases().remove(0).0;
    let memory = memory();
    let owner = owner(&memory).bind_request(&request).unwrap();
    assert_eq!(
        owner.bind_request(&request).err().unwrap().code,
        "data.domain.request_identity"
    );
}
#[test]
fn related_source_role_allowlist_accepts_shared_algorithm_shapes() {
    let mut cases = cases();
    cases[1].2.extend([
        lookup("ItemAppearance", 0),
        lookup("ItemDisplayInfo", 0),
        scan("ModelFileData", Some(("ModelResourcesID", 17)), 65536),
        scan("TextureFileData", Some(("MaterialResourcesID", 18)), 65536),
        lookup("ComponentModelFileData", 19),
    ]);
    cases[2].2.extend([
        lookup("ItemAppearance", 17),
        lookup("ItemDisplayInfo", 0),
        scan(
            "ItemDisplayInfoMaterialRes",
            Some(("ItemDisplayInfoID", 0)),
            65536,
        ),
        scan("TextureFileData", Some(("MaterialResourcesID", 18)), 65536),
    ]);
    cases[3].2.extend([
        lookup("ItemAppearance", 17),
        lookup("ItemDisplayInfo", 19),
        scan(
            "HelmetGeosetData",
            Some(("HelmetGeosetVisDataID", 18)),
            65536,
        ),
    ]);
    cases[4].2.extend([
        lookup("Spell", 9),
        lookup("SpellName", 19),
        lookup("Spell", 19),
        scan("SpellEffect", Some(("SpellID", 19)), 65536),
        scan("SpellMisc", Some(("SpellID", 19)), 1),
        lookup("SpellCastTimes", 0),
        lookup("SpellDuration", 18),
        lookup("SpellRange", 19),
    ]);
    cases[7].2.extend([
        lookup("SpellName", 19),
        scan("SpellMisc", Some(("SpellID", 19)), 1),
    ]);
    cases[10].2.push(lookup("CreatureModelData", 17));
    cases[11].2.extend([
        scan("CreatureDisplayInfo", Some(("ModelID", 18)), 65536),
        scan(
            "CreatureDisplayInfoGeosetData",
            Some(("CreatureDisplayInfoID", 19)),
            65536,
        ),
    ]);
    for (request, report, sources) in cases {
        finish(&memory(), &request, report, sources).unwrap();
    }
}
#[test]
fn report_size_limit_rejects_instead_of_returning_truncated_success() {
    let memory = SessionMemory::new(128 << 20).unwrap();
    let owner = DomainOwner::new(
        memory
            .begin_bounded(100_000_000, OperationControl::default(), 64 << 20)
            .unwrap(),
        memory.budget.clone(),
    );
    owner.budget().charge(Cost::retained(20 << 20)).unwrap();
    let report =
        json!({"complete":false,"truncated":false,"relations":[],"payload":"x".repeat(16<<20)});
    assert_eq!(
        code(owner.finish(report, vec![])),
        "data.domain.result_limit"
    );
    assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn creature_file_selector_binds_requested_file_separately_from_computed_relation() {
    let request = DomainRequest::CreatureDisplay {
        display_id: 0,
        file_data_id: 9,
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    };
    let report = decorated(json!({"displayID":19,"fileDataID":10,"requestedFileDataID":9}));
    let sources = vec![
        scan("CreatureModelData", Some(("FileDataID", 9)), 65536),
        scan("CreatureDisplayInfo", Some(("ModelID", 29)), 65536),
        lookup("CreatureModelData", 29),
        scan(
            "CreatureDisplayInfoGeosetData",
            Some(("CreatureDisplayInfoID", 19)),
            65536,
        ),
    ];
    let memory = memory();
    // A different computed file or zero is a valid shared-algorithm result;
    // neither changes the selector bound to this request and source scan.
    for computed in [0, 10] {
        let mut report = report.clone();
        report["fileDataID"] = json!(computed);
        let result = finish(&memory, &request, report, sources.clone()).unwrap();
        result
            .validate_request(&request, &result._owner.lifetime.budget)
            .unwrap();
        assert_eq!(result.report()["fileDataID"], computed);
    }
    for mutation in 0..4 {
        let mut report = report.clone();
        let mut sources = sources.clone();
        match mutation {
            0 => report["requestedFileDataID"] = json!(10),
            1 => {
                report
                    .as_object_mut()
                    .unwrap()
                    .remove("requestedFileDataID");
            }
            2 => sources[0]["request"]["filter"] = json!(["FileDataID", 10]),
            _ => {
                sources.remove(0);
            }
        }
        assert_eq!(
            code(finish(&memory, &request, report, sources)),
            "data.domain.request_identity"
        );
    }
}

#[test]
fn completed_domain_validation_uses_new_capture_control_not_cancelled_origin() {
    let memory = memory();
    let (request, report, values) = cases().remove(0);
    let control = OperationControl::default();
    let cancellation = control.tracked_cancel_handle().unwrap();
    let lifetime = memory.begin_bounded(100_000_000, control, 8 << 20).unwrap();
    let owner = DomainOwner::new(lifetime, memory.budget.clone())
        .bind_request(&request)
        .unwrap();
    owner.budget().charge(Cost::retained(8192)).unwrap();
    let sources = values
        .into_iter()
        .map(|value| owned(&memory, value, vec![]))
        .collect();
    let result = owner.finish(report, sources).unwrap();
    cancellation.cancel();
    assert!(result._owner.lifetime.budget.check().is_err());
    let fresh = memory
        .begin_bounded(100_000_000, OperationControl::default(), 1 << 20)
        .unwrap();
    result.validate_request(&request, &fresh.budget).unwrap();
}
