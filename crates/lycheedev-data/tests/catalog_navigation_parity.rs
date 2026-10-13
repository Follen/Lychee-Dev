//! Frozen-Go item/decor behavior on small authenticated local sources. These
//! tests exercise the real dispatcher/provider/capture chain, never a row mock.
#[path = "support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture, decor_rows, decors, items, table};
use lycheedev_core::{control::OperationControl, metadata::Metadata};
use serde_json::{Value, json};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

fn run(fx: &Fixture, command: &str, flags: &[(&str, &str)]) -> lycheedev_core::Outcome {
    let result = lycheedev_data::dispatch(&fx.invocation(command, flags)).unwrap();
    fx.verify(&result);
    result
}
fn captures(fx: &Fixture) -> usize {
    Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 100)
        .unwrap()
        .len()
}

#[test]
fn item_identity_class_partial_names_slots_and_captures_match_frozen_go() {
    let fx = Fixture::new(&items());
    let found = run(&fx, "data item get", &[("--item-id", "7")]);
    assert!(found.complete);
    assert_eq!(
        found.data["item"],
        json!({"id":7,"name":"腰带|cffaabbcc中文\r\n","inventoryType":6,"quality":4,"slotName":"Waist","classID":4,"subclassID":2})
    );
    assert_eq!(
        found.data["tables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["Item", "ItemSparse"]
    );
    assert_eq!(found.data["relations"], json!([]));
    assert_eq!(found.data["partialReasons"], Value::Null);
    assert_eq!(found.data["pin"]["language"], "zhCN");
    assert_eq!(found.provenance[0]["provenance"]["locator"], "item:get:7");
    let partial = run(&fx, "data item get", &[("--item-id", "9")]);
    assert!(!partial.complete);
    assert_eq!(partial.data["partialReasons"], json!(["row_missing"]));
    assert_eq!(partial.data["truncated"], false);
    assert_eq!(partial.data["item"]["slotName"], "Unknown");
    assert_eq!(partial.data["item"]["classID"], 0);
    assert_eq!(partial.data["item"]["subclassID"], 0);
    assert_eq!(partial.warnings,json!(["This result is only partially covered; inspect partialReasons for the unavailable tables and unresolved relations."]).as_array().unwrap().clone());
    let before = captures(&fx);
    let absent = lycheedev_data::dispatch(&fx.invocation("data item get", &[("--item-id", "99")]))
        .unwrap_err();
    assert_eq!(
        (
            absent.code.as_str(),
            absent.exit_code,
            absent.message.as_str()
        ),
        ("records.record_not_found", 3, "records.record_not_found")
    );
    assert_eq!(captures(&fx), before);
}

#[test]
fn multi_table_item_query_reuses_metadata_instead_of_replaying_each_table_session() {
    let fx = Fixture::new(&items());
    let schema = |name: &str| {
        let mut inv = fx.invocation("data db2 schema", &[]);
        inv.positional = Some(name.into());
        let outcome = lycheedev_data::dispatch(&inv).unwrap();
        fx.verify(&outcome);
        outcome.data["resources"]["metadataBytes"].as_u64().unwrap()
    };
    // Compare all queries on authenticated warm inputs, without changing the
    // source or weakening any validation in the combined request.
    schema("Item");
    schema("ItemSparse");
    let independent = schema("Item") + schema("ItemSparse");
    let together = run(&fx, "data item get", &[("--item-id", "7")]);
    let admitted = together.data["resources"]["metadataBytes"]
        .as_u64()
        .unwrap();
    assert!(admitted > 0 && admitted < independent);
    assert_eq!(together.data["tables"].as_array().unwrap().len(), 2);
    for proof in together.data["tableVerification"].as_array().unwrap() {
        assert_eq!(proof["encodingFramingEKey"], true);
        assert_eq!(proof["encodingRequestedPagesMD5"], true);
        assert_eq!(proof["rootWholeDecodedCKey"], true);
        assert_eq!(proof["contentWholeDecodedCKey"], true);
    }
}

#[test]
fn decor_list_keeps_zero_models_sorts_exactly_and_probes_one_extra_row() {
    let fx = Fixture::new(&[decors(&decor_rows())]);
    let limited = run(&fx, "data decor list", &[("--limit", "2")]);
    assert_eq!(limited.data["count"], 2);
    assert!(!limited.complete);
    assert_eq!(limited.data["truncationReason"], "limit_reached");
    assert_eq!(limited.data["partial"], false);
    assert_eq!(
        limited.data["items"][0],
        json!({"id":10,"name":"无模型🙂","modelFileDataID":0,"thumbnailFileDataID":600,"itemID":41,"gameObjectID":700,"type":-1,"modelType":0})
    );
    assert_eq!(limited.data["items"][1]["id"], 20);
    for limit in ["3", "5000"] {
        let full = run(&fx, "data decor list", &[("--limit", limit)]);
        assert!(full.complete);
        assert_eq!(full.data["count"], 3);
        assert_eq!(full.data["truncationReason"], "none");
        assert_eq!(full.data["items"][2]["id"], 30);
    }
    let empty = Fixture::new(&[decors(&[])]);
    let full = run(&empty, "data decor list", &[("--limit", "1")]);
    assert!(full.complete);
    assert_eq!(full.data["items"], json!([]));
    assert_eq!(full.data["count"], 0);
    let mut malformed = decor_rows();
    malformed[2][1] = Cell::BadString;
    let fx = Fixture::new(&[decors(&malformed)]);
    // Row20 is the extra row after ID10. A corrupt unused name still fails;
    // truncation cannot skip decoding the next matched row's full contents.
    let error = lycheedev_data::dispatch(&fx.invocation("data decor list", &[("--limit", "1")]))
        .unwrap_err();
    assert_eq!(error.code, "table.invalid_format");
    assert_eq!(captures(&fx), 0);
}

#[test]
fn decor_get_all_selectors_preserve_ordered_ambiguity_and_original_relations() {
    let fx = Fixture::new(&[decors(&decor_rows())]);
    for (key, want, field, ids) in [
        ("--id", "10", "ID", vec![10]),
        ("--item-id", "42", "ItemID", vec![20, 30]),
        (
            "--model-file-data-id",
            "501",
            "ModelFileDataID",
            vec![20, 30],
        ),
    ] {
        let result = run(&fx, "data decor get", &[(key, want)]);
        assert_eq!(result.data["item"]["id"], ids[0]);
        let expected:Vec<_>=ids.iter().map(|id|json!({"fromTable":"HouseDecor","fromField":field,"fromID":id,"toKind":"decor","toID":id,"depth":0})).collect();
        assert_eq!(result.data["relations"], json!(expected));
        assert_eq!(result.data["partial"], ids.len() > 1);
        assert_eq!(result.complete, ids.len() == 1);
        assert_eq!(
            result.data["partialReasons"],
            if ids.len() > 1 {
                json!(["relation_ambiguous"])
            } else {
                Value::Null
            }
        );
        assert_eq!(
            result.provenance[0]["provenance"]["locator"],
            format!("decor:get:{field}:{want}")
        );
    }
    // An explicitly zero optional decor ID does not create a second key.
    let found = run(
        &fx,
        "data decor get",
        &[("--id", "0"), ("--model-file-data-id", "501")],
    );
    assert_eq!(found.data["item"]["id"], 20);
    let before = captures(&fx);
    for selectors in [
        &[][..],
        &[("--id", "0")][..],
        &[("--id", "10"), ("--item-id", "42")][..],
    ] {
        let error =
            lycheedev_data::dispatch(&fx.invocation("data decor get", selectors)).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("records.request_conflict", 2)
        );
    }
    let absent =
        lycheedev_data::dispatch(&fx.invocation("data decor get", &[("--id", "99")])).unwrap_err();
    assert_eq!(
        (absent.code.as_str(), absent.exit_code),
        ("records.record_not_found", 3)
    );
    assert_eq!(captures(&fx), before);
}

#[test]
fn decor_ambiguity_row_budget_is_independent_of_relation_count_and_order() {
    use Cell::{I, Text, U};
    let rows: Vec<_> = (1..=5001)
        .rev()
        .map(|id| {
            vec![
                U(id),
                Text(String::new()),
                U(0),
                U(0),
                U(42),
                U(0),
                I(0),
                I(0),
            ]
        })
        .collect();
    let fx = Fixture::new(&[decors(&rows)]);
    let result = run(&fx, "data decor get", &[("--item-id", "42")]);
    assert!(!result.complete);
    assert_eq!(result.data["item"]["id"], 1);
    assert_eq!(result.data["relations"].as_array().unwrap().len(), 5000);
    assert_eq!(result.data["relations"][4999]["fromID"], 5000);
    assert_eq!(result.data["partialReasons"], json!(["relation_ambiguous"]));
    assert_eq!(result.data["truncationReason"], "row_budget");
    assert_eq!(result.warnings.len(), 2);
}

#[test]
fn catalog_navigation_errors_cancellation_locale_and_corrupt_sources_fail_closed() {
    let fx = Fixture::new(&[decors(&decor_rows())]);
    for (command, flags, code, exit) in [
        ("data decor list", vec![], "command.invalid_arguments", 2),
        (
            "data decor list",
            vec![("--limit", "0")],
            "records.limit_required",
            2,
        ),
        (
            "data decor list",
            vec![("--limit", "5001")],
            "records.query_limit",
            2,
        ),
        ("data item get", vec![], "command.invalid_arguments", 2),
        (
            "data item get",
            vec![("--item-id", "7")],
            "schema.definition_not_found",
            3,
        ),
    ] {
        let error = lycheedev_data::dispatch(&fx.invocation(command, &flags)).unwrap_err();
        assert_eq!((error.code.as_str(), error.exit_code), (code, exit));
    }
    let mut cancelled = fx.invocation("data decor list", &[("--limit", "1")]);
    let cancel = cancelled.control.cancel_handle();
    cancel.store(true, Ordering::Release);
    assert_eq!(
        lycheedev_data::dispatch(&cancelled).unwrap_err().exit_code,
        7
    );
    cancelled.control = OperationControl::new(Instant::now(), Arc::new(AtomicBool::new(false)));
    assert_eq!(
        lycheedev_data::dispatch(&cancelled).unwrap_err().code,
        "command.deadline"
    );
    assert_eq!(captures(&fx), 0);
    let wrong_locale = Fixture::locale(&[decors(&decor_rows())], "enUS", 0x40);
    let error =
        lycheedev_data::dispatch(&wrong_locale.invocation("data decor list", &[("--limit", "1")]))
            .unwrap_err();
    assert_eq!(error.code, "records.content_missing");
    assert_eq!(captures(&wrong_locale), 0);
    let correct_locale = Fixture::locale(&[decors(&decor_rows())], "enUS", 2);
    let full = run(&correct_locale, "data decor list", &[("--limit", "3")]);
    assert_eq!(full.data["pin"]["language"], "enUS");
    let _ = run(&fx, "data decor list", &[("--limit", "3")]);
    let before = captures(&fx);
    let archive = fx.root.join("game/Data/data/data.000");
    let original = fs::read(&archive).unwrap();
    let mut damaged = original.clone();
    damaged[39] ^= 1;
    fs::write(&archive, damaged).unwrap();
    let result = lycheedev_data::dispatch(&fx.invocation("data decor list", &[("--limit", "3")]));
    fs::write(&archive, original).unwrap();
    assert_eq!(result.unwrap_err().code, "archive.index_integrity");
    assert_eq!(captures(&fx), before);
}

#[test]
fn decor_filter_uses_schema_and_exact_scalar_values_not_coercion() {
    use Cell::{Text, U};
    let wrong = table(
        "HouseDecor",
        403,
        &[("ID", "uint"), ("Name_lang", "text"), ("ItemID", "text")],
        &[vec![U(1), Text("保留原名".into()), Text("42".into())]],
    );
    let fx = Fixture::new(&[wrong]);
    let error = lycheedev_data::dispatch(&fx.invocation("data decor get", &[("--item-id", "42")]))
        .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code, error.message.as_str()),
        (
            "records.field_type_mismatch",
            2,
            "records.field_type_mismatch: ItemID is locstring"
        )
    );
    let error = lycheedev_data::dispatch(
        &fx.invocation("data decor get", &[("--model-file-data-id", "5")]),
    )
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.field_unknown", 2)
    );
    assert_eq!(captures(&fx), 0);
}

#[test]
#[ignore = "explicit fresh retained fixture for independent frozen-Go public differential"]
fn prepare_catalog_public_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LYCHEEDEV_CATALOG_FIXTURE").expect("fresh owned fixture root"),
    );
    let mut tables = items();
    tables.push(decors(&decor_rows()));
    let fx = Fixture::at(root, &tables, "zhCN", 0x40);
    let identity: Value =
        serde_json::from_slice(&fs::read(fx.root.join("input-identity.json")).unwrap()).unwrap();
    assert_eq!(identity["snapshot"], fx.pin);
}
