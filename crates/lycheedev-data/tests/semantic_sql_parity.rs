//! Fixed Go 3.2 semantics.go / semantics_test.go / review_budget_test.go.
//! Authenticated synthetic local fixtures; no native/client/network-run claim.
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{BUILD, Cell::U, DEFS, Fixture, table};
use lycheedev_core::{
    Outcome,
    control::{OperationControl, TrackedCancellation},
    evidence,
    metadata::Metadata,
    storage,
};
use serde_json::{Value, json};
use std::{
    fs,
    time::{Duration, Instant},
};

fn fixture() -> Fixture {
    Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![U(1), U(1)], vec![U(2), U(2)]],
    )])
}
fn cache(fx: &Fixture, suffix: &str, raw: &[u8]) -> Value {
    let url = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{DEFS}/{suffix}");
    let key = format!("definition/{}", storage::sha256(url.as_bytes()));
    let blob = evidence::publish_blob(&fx.home, raw, 4 << 20).unwrap();
    let mut metadata = Metadata::open(&fx.home, true).unwrap();
    let generation = metadata.generation(&key).unwrap();
    metadata
        .put(&key, &json!({"url":url,"blob":blob}), generation)
        .unwrap();
    blob
}
fn query(fx: &Fixture, sql: &str) -> Outcome {
    lycheedev_data::dispatch(&fx.invocation("data sql", &[("--sql", sql)])).unwrap()
}
fn count(fx: &Fixture) -> usize {
    Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 1000)
        .unwrap()
        .len()
}
fn verify(fx: &Fixture, out: &Outcome) {
    assert_eq!(out.provenance.len(), 1);
    let capture = &out.provenance[0];
    assert_eq!(
        evidence::read_capture(&fx.home, capture["id"].as_str().unwrap()).unwrap(),
        *capture
    );
    let raw = evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), out.data);
    let mut public = Vec::new();
    lycheedev_core::public_json::write_line(&mut public, &out.data, &out.public_numbers).unwrap();
    let mut wire: Value = serde_json::from_slice(&public).unwrap();
    // These three schema-declared fields are binary64. Go's public encoder
    // renders integral floats without .0, while the archived Value retains
    // serde's token. Compare their exact f64 values and retain exact Value
    // equality for every other field, especially u64 cells and parameters.
    for key in ["definitionsMs", "preparationMs", "executionMs"] {
        let actual = wire["timings"][key].as_f64().unwrap();
        let expected = out.data["timings"][key].as_f64().unwrap();
        assert!(actual.is_finite() && expected.is_finite(), "{key}");
        assert_eq!(actual.to_bits(), expected.to_bits(), "{key}");
        if expected == 0.0 {
            assert_eq!(wire["timings"][key], json!(0), "{key}");
        }
        wire["timings"][key] = out.data["timings"][key].clone();
    }
    assert_eq!(wire, out.data);
    assert_eq!(out.data["complete"], true);
    assert_eq!(out.data["partial"], false);
    assert_eq!(capture["complete"], true);
    assert_eq!(capture["truncated"], false);
}
#[test]
fn go_semantic_sql_exact_rows_builds_conditions_and_source_provenance() {
    let fx = fixture();
    let mapping = cache(&fx,"meta/mapping.dbdm",b"ENUM Sample::Value State Sample::ID=999\nFLAGS Sample::Value Flags\nFLAGS Sample::Other Flags\n");
    let enums = cache(
        &fx,
        "meta/enums/State.dbde",
        b"(BUILD 1.0.0.1) 1 OLD\n(BUILD 12.1.0.69933) 1 ACTIVE // current\n2 // unnamed\n",
    );
    let flags = cache(
        &fx,
        "meta/flags/Flags.dbdf",
        b"1 FIRST\n2 SECOND\n0x8000000000000000 HIGH // high bit\n",
    );
    let out = query(
        &fx,
        "SELECT Field,Kind,Definition,Value,Name,Comment,ConditionField,ConditionValue FROM MeTa.sAmPlE ORDER BY Field,Kind,Value",
    );
    assert_eq!(
        out.data["result"]["columns"],
        json!([
            "Field",
            "Kind",
            "Definition",
            "Value",
            "Name",
            "Comment",
            "ConditionField",
            "ConditionValue"
        ])
    );
    assert_eq!(
        out.data["result"]["rows"],
        json!([
            ["Other", "FLAGS", "Flags", 1, "FIRST", "", "", ""],
            ["Other", "FLAGS", "Flags", 2, "SECOND", "", "", ""],
            [
                "Other",
                "FLAGS",
                "Flags",
                9223372036854775808u64,
                "HIGH",
                "high bit",
                "",
                ""
            ],
            [
                "Value",
                "ENUM",
                "State",
                1,
                "ACTIVE",
                "current",
                "Sample::ID",
                "999"
            ],
            [
                "Value",
                "ENUM",
                "State",
                2,
                null,
                "unnamed",
                "Sample::ID",
                "999"
            ],
            ["Value", "FLAGS", "Flags", 1, "FIRST", "", "", ""],
            ["Value", "FLAGS", "Flags", 2, "SECOND", "", "", ""],
            [
                "Value",
                "FLAGS",
                "Flags",
                9223372036854775808u64,
                "HIGH",
                "high bit",
                "",
                ""
            ],
        ])
    );
    assert_eq!(out.data["sources"], json!([]));
    assert_eq!(out.data["timings"]["decodedSourceBytes"], 0);
    assert_eq!(
        out.data["semanticSources"],
        json!([{"table":"Sample","commit":DEFS,"build":BUILD,"mapping":mapping,"definitions":{"enums/State.dbde":enums,"flags/Flags.dbdf":flags}}])
    );
    assert!(out.data.get("effectiveSources").is_none());
    verify(&fx, &out);
    let joined = query(
        &fx,
        "SELECT s.ID,m.Name,m.ConditionField,m.ConditionValue FROM static.Sample s JOIN meta.Sample m ON s.Value=m.Value WHERE m.Kind='ENUM' ORDER BY s.ID",
    );
    assert_eq!(
        joined.data["result"]["rows"],
        json!([
            [1, "ACTIVE", "Sample::ID", "999"],
            [2, null, "Sample::ID", "999"]
        ])
    );
    assert_eq!(joined.data["sources"].as_array().unwrap().len(), 1);
    assert_eq!(joined.data["semanticSources"].as_array().unwrap().len(), 1);
    verify(&fx, &joined);
}
#[test]
fn go_semantic_sql_no_mapping_is_empty_with_columns_and_can_be_joined() {
    let fx = fixture();
    let mapping = cache(
        &fx,
        "meta/mapping.dbdm",
        b"COLOR Sample::Value ignored\nFLAGS Other::Value NotDownloaded\n",
    );
    let out = query(&fx, "SELECT * FROM meta.Sample");
    assert_eq!(out.data["result"]["rows"], json!([]));
    assert_eq!(out.data["result"]["columns"].as_array().unwrap().len(), 8);
    assert_eq!(out.data["semanticSources"][0]["mapping"], mapping);
    assert_eq!(out.data["semanticSources"][0]["definitions"], json!({}));
    verify(&fx, &out);
    let out = query(
        &fx,
        "SELECT s.ID,m.Name FROM Sample s LEFT JOIN meta.Sample m ON s.Value=m.Value ORDER BY s.ID",
    );
    assert_eq!(out.data["result"]["rows"], json!([[1, null], [2, null]]));
    let out = query(&fx, "SELECT COUNT(*) AS Total FROM meta.Sample");
    assert_eq!(out.data["result"]["rows"], json!([[0]]));
}
#[test]
fn semantic_sql_missing_and_invalid_sources_publish_no_capture_then_recover() {
    let fx = fixture();
    let sql = "SELECT Value,Name FROM meta.Sample";
    let before = count(&fx);
    assert_eq!(
        lycheedev_data::dispatch(&fx.invocation("data sql", &[("--sql", sql)]))
            .unwrap_err()
            .code,
        "records.definition_unavailable_offline"
    );
    cache(&fx, "meta/mapping.dbdm", b"ENUM Sample::Value State\n");
    assert_eq!(
        lycheedev_data::dispatch(&fx.invocation("data sql", &[("--sql", sql)]))
            .unwrap_err()
            .code,
        "records.definition_unavailable_offline"
    );
    cache(
        &fx,
        "meta/enums/State.dbde",
        b"18446744073709551616 TOO_BIG\n",
    );
    assert_eq!(
        lycheedev_data::dispatch(&fx.invocation("data sql", &[("--sql", sql)]))
            .unwrap_err()
            .code,
        "records.definition_identity"
    );
    cache(&fx, "meta/enums/State.dbde", b"(BOGUS 1) 2 BAD\n");
    assert_eq!(
        lycheedev_data::dispatch(&fx.invocation("data sql", &[("--sql", sql)]))
            .unwrap_err()
            .code,
        "records.definition_identity"
    );
    assert_eq!(count(&fx), before);
    cache(&fx, "meta/enums/State.dbde", b"18446744073709551615 MAX\n");
    let out = query(&fx, sql);
    assert_eq!(out.data["result"]["rows"], json!([[u64::MAX, "MAX"]]));
    assert_eq!(count(&fx), before + 1);
    verify(&fx, &out);
}
#[test]
fn semantic_sql_cached_blob_corruption_never_refetches_or_publishes() {
    let fx = fixture();
    cache(&fx, "meta/mapping.dbdm", b"ENUM Sample::Value State\n");
    let raw = b"1 ACTIVE\n";
    let blob = cache(&fx, "meta/enums/State.dbde", raw);
    let path = evidence::blob_path(&fx.home, blob["sha256"].as_str().unwrap()).unwrap();
    fs::write(&path, b"2 BROKEN\n").unwrap();
    let before = count(&fx);
    assert_eq!(
        lycheedev_data::dispatch(
            &fx.invocation("data sql", &[("--sql", "SELECT Name FROM meta.Sample")])
        )
        .unwrap_err()
        .code,
        "vault.blob_integrity"
    );
    assert_eq!(count(&fx), before);
    fs::write(path, raw).unwrap();
    assert_eq!(
        query(&fx, "SELECT Name FROM meta.Sample").data["result"]["rows"],
        json!([["ACTIVE"]])
    );
}
#[test]
fn semantic_sql_pin_parameter_source_precedence_and_query_lexemes_survive() {
    let fx = fixture();
    let file = fx.root.join("semantic-parameter.json");
    fs::write(
        &file,
        br#"{"sql":"SELECT :n FROM meta.Missing","parameters":{"n":1e999}}"#,
    )
    .unwrap();
    let mut inv = fx.invocation("data sql", &[("--file", file.to_str().unwrap())]);
    assert_eq!(
        lycheedev_data::dispatch(&inv).unwrap_err().code,
        "query.numeric_range"
    );
    inv.flags
        .insert("--snapshot".into(), "PIN-does-not-exist".into());
    assert_ne!(
        lycheedev_data::dispatch(&inv).unwrap_err().code,
        "query.numeric_range"
    );
    cache(&fx, "meta/mapping.dbdm", b"ENUM Sample::Value State\n");
    cache(&fx, "meta/enums/State.dbde", b"1 ACTIVE\n");
    fs::write(
        &file,
        br#"{"sql":"SELECT :n,:f,Value FROM meta.Sample","parameters":{"n":-0,"f":1e-7}}"#,
    )
    .unwrap();
    let out =
        lycheedev_data::dispatch(&fx.invocation("data sql", &[("--file", file.to_str().unwrap())]))
            .unwrap();
    let mut public = Vec::new();
    lycheedev_core::public_json::write_line(&mut public, &out.data, &out.public_numbers).unwrap();
    let text = String::from_utf8(public).unwrap();
    assert!(text.contains(r#""n":-0"#), "{text}");
    assert!(text.contains(r#""f":1e-7"#), "{text}");
    assert!(text.contains(r#""rows":[[0,1e-7,1]]"#), "{text}");
    verify(&fx, &out);
}
#[test]
fn semantic_sql_cancelled_or_expired_query_has_no_capture_and_fresh_retry_works() {
    let fx = fixture();
    cache(&fx, "meta/mapping.dbdm", b"ENUM Sample::Value State\n");
    cache(&fx, "meta/enums/State.dbde", b"1 ACTIVE\n");
    let before = count(&fx);
    let sql = "SELECT Value FROM meta.Sample";
    let mut inv = fx.invocation("data sql", &[("--sql", sql)]);
    let cancellation = TrackedCancellation::new();
    inv.control = OperationControl::tracked(
        Instant::now() + Duration::from_secs(30),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert_eq!(
        lycheedev_data::dispatch(&inv).unwrap_err().code,
        "command.cancelled"
    );
    inv.control = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        lycheedev_data::dispatch(&inv).unwrap_err().code,
        "command.deadline"
    );
    assert_eq!(count(&fx), before);
    assert_eq!(query(&fx, sql).data["result"]["rows"], json!([[1]]));
}

#[test]
fn semantic_sql_memoizes_definitions_and_keeps_the_128_document_limit() {
    let fx = fixture();
    let mapping = (0..128)
        .map(|i| format!("ENUM Sample::Value D{i}\n"))
        .collect::<String>();
    cache(&fx, "meta/mapping.dbdm", mapping.as_bytes());
    for i in 0..128 {
        cache(&fx, &format!("meta/enums/D{i}.dbde"), b"// empty catalog\n");
    }
    let out = query(&fx, "SELECT * FROM meta.Sample");
    assert_eq!(out.data["result"]["rows"], json!([]));
    assert_eq!(
        out.data["semanticSources"][0]["definitions"]
            .as_object()
            .unwrap()
            .len(),
        128
    );
    let before = count(&fx);
    cache(
        &fx,
        "meta/mapping.dbdm",
        (mapping + "ENUM Sample::Value D128\n").as_bytes(),
    );
    assert_eq!(
        lycheedev_data::dispatch(
            &fx.invocation("data sql", &[("--sql", "SELECT * FROM meta.Sample")])
        )
        .unwrap_err()
        .code,
        "records.metadata_limit",
        "The 129th resource is rejected before its unavailable cache is opened"
    );
    assert_eq!(count(&fx), before);
    cache(
        &fx,
        "meta/mapping.dbdm",
        b"ENUM Sample::A D0\nENUM Sample::B D0\n",
    );
    cache(&fx, "meta/enums/D0.dbde", b"1 ONE\n");
    let out = query(&fx, "SELECT Field,Name FROM meta.Sample ORDER BY Field");
    assert_eq!(
        out.data["result"]["rows"],
        json!([["A", "ONE"], ["B", "ONE"]])
    );
    assert_eq!(
        out.data["semanticSources"][0]["definitions"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn semantic_sql_combined_rows_are_bounded_before_query_capture() {
    let fx = fixture();
    cache(
        &fx,
        "meta/mapping.dbdm",
        b"ENUM Sample::A State\nENUM Sample::B State\n",
    );
    cache(
        &fx,
        "meta/enums/State.dbde",
        "0\n".repeat(50_001).as_bytes(),
    );
    let before = count(&fx);
    assert_eq!(
        lycheedev_data::dispatch(
            &fx.invocation("data sql", &[("--sql", "SELECT COUNT(*) FROM meta.Sample")])
        )
        .unwrap_err()
        .code,
        "records.metadata_limit"
    );
    assert_eq!(count(&fx), before);
    cache(&fx, "meta/enums/State.dbde", b"1 ONE\n");
    assert_eq!(
        query(&fx, "SELECT COUNT(*) FROM meta.Sample").data["result"]["rows"],
        json!([[2]])
    );
}
#[test]
fn semantic_sql_effective_join_keeps_both_explicit_provenance_chains() {
    let fx = fixture();
    cache(&fx, "meta/mapping.dbdm", b"ENUM Sample::Value State\n");
    cache(&fx, "meta/enums/State.dbde", b"1 ONE\n2 TWO\n");
    let mut hotfix = b"XFTH".to_vec();
    hotfix.extend(9u32.to_le_bytes());
    hotfix.extend(69933u32.to_le_bytes());
    hotfix.resize(44, 0);
    let mut header = [0u8; 32];
    header[..4].copy_from_slice(b"XFTH");
    for (offset, value) in [(8, 1u32), (16, 0xf00d0000 + 201), (20, 1), (24, 8)] {
        header[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    header[28] = 1;
    hotfix.extend(header);
    hotfix.extend(1u32.to_le_bytes());
    hotfix.extend(2u32.to_le_bytes());
    let capture = evidence::commit_capture(&fx.home,&hotfix,"application/octet-stream",json!({"kind":"hotfix-cache","locator":"fixture:semantic-effective","snapshot":fx.pin,"dataBuild":BUILD}),true,false).unwrap();
    let file = fx.root.join("semantic-effective.json");
    fs::write(&file,serde_json::to_vec(&json!({"sql":"SELECT e.ID,m.Name FROM effective.Sample e JOIN meta.Sample m ON e.Value=m.Value ORDER BY e.ID", "hotfix":[capture["id"]]})).unwrap()).unwrap();
    let out =
        lycheedev_data::dispatch(&fx.invocation("data sql", &[("--file", file.to_str().unwrap())]))
            .unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[1, "TWO"], [2, "TWO"]]));
    assert_eq!(
        out.data["effectiveSources"][0]["captures"][0]["id"],
        capture["id"]
    );
    assert_eq!(out.data["semanticSources"][0]["table"], "Sample");
    assert_eq!(out.data["sources"].as_array().unwrap().len(), 1);
    verify(&fx, &out);
}

#[test]
fn semantic_sql_max_bytes_admission_precedes_missing_definitions() {
    let fx = fixture();
    let before = count(&fx);
    for value in ["0", "536870913", "not-a-number"] {
        let error = lycheedev_data::dispatch(&fx.invocation(
            "data sql",
            &[
                ("--sql", "SELECT Value FROM meta.Missing"),
                ("--max-bytes", value),
            ],
        ))
        .unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments", "{value}");
    }
    assert_eq!(count(&fx), before);
}

#[test]
fn semantic_sql_unicode_folded_mapping_keeps_canonical_table_identity() {
    let fx = Fixture::new(&[table(
        "Skill",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![U(1), U(1)]],
    )]);
    cache(
        &fx,
        "meta/mapping.dbdm",
        "ENUM ſKILL::Value State\nENUM SKıLL::Other MustNotLoad\n".as_bytes(),
    );
    cache(&fx, "meta/enums/State.dbde", b"1 MATCH\n");
    let out = query(&fx, "SELECT Field,Value,Name FROM meta.Skill");
    assert_eq!(out.data["result"]["rows"], json!([["Value", 1, "MATCH"]]));
    assert_eq!(out.data["semanticSources"][0]["table"], "Skill");
    assert_eq!(
        out.data["semanticSources"][0]["definitions"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    verify(&fx, &out);
}
