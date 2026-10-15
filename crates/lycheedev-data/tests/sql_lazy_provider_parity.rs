//! Fixed Go 3.2.3 actual local SQL consumer over authenticated CASC fixtures.
//! No native run is implied by the presence of these regression sources.
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell, Fixture, table};
use lycheedev_core::{evidence, metadata::Metadata};
use serde_json::{Value, json};
fn captures(f: &Fixture) -> usize {
    let metadata = Metadata::open(&f.home, false).unwrap();
    let count = metadata.list::<Value>("capture/", 100).unwrap().len();
    metadata.close().unwrap();
    count
}
fn verify(f: &Fixture, out: &lycheedev_core::Outcome) {
    let capture = &out.provenance[0];
    let raw = evidence::read_blob(&f.home, &capture["blob"], 16 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), out.data);
    assert_eq!(out.data["sources"][0]["file"]["source"], "installation");
    assert!(out.data["memoryBudget"]["retainedBytes"].is_u64());
}
#[test]
fn local_default_numeric_in_preserves_candidate_order_and_exact_capture() {
    let f = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[
            vec![Cell::U(1), Cell::U(10)],
            vec![Cell::U(2), Cell::U(20)],
            vec![Cell::U(3), Cell::U(30)],
        ],
    )]);
    for (sql, expected) in [
        (
            "SELECT ID,Value FROM Sample WHERE ID IN (3,1,3.0,NULL,2)",
            json!([[3, 30], [1, 10], [2, 20]]),
        ),
        (
            "SELECT ID FROM Sample WHERE ID IN (3,1) ORDER BY ID",
            json!([[1], [3]]),
        ),
        ("SELECT ID FROM Sample WHERE 2=ID", json!([[2]])),
        ("SELECT ID FROM Sample WHERE ID NOT IN (3,NULL)", json!([])),
    ] {
        let out = lycheedev_data::dispatch(&f.invocation("data sql", &[("--sql", sql)])).unwrap();
        assert_eq!(out.data["result"]["rows"], expected, "{sql}");
        verify(&f, &out);
    }
}
#[test]
fn local_default_missing_id_equal_and_in_skip_unread_corrupt_rows() {
    let f = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Name", "text")],
        &[vec![Cell::U(1), Cell::BadString]],
    )]);
    for sql in [
        "SELECT ID FROM Sample WHERE ID=99",
        "SELECT ID FROM Sample WHERE ID IN (99,98)",
    ] {
        let out = lycheedev_data::dispatch(&f.invocation("data sql", &[("--sql", sql)])).unwrap();
        assert_eq!(out.data["result"]["rows"], json!([]));
        verify(&f, &out);
    }
    for sql in [
        "SELECT ID FROM Sample",
        "SELECT ID FROM Sample WHERE ID IN (1)",
        "SELECT ID FROM Sample WHERE ID+0=99",
    ] {
        let before = captures(&f);
        let error =
            lycheedev_data::dispatch(&f.invocation("data sql", &[("--sql", sql)])).unwrap_err();
        assert_eq!(error.code, "table.invalid_format", "{sql}");
        assert_eq!(captures(&f), before);
    }
}
#[test]
fn local_default_global_binding_precedes_corrupt_row_decode() {
    let f = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Name", "text")],
        &[vec![Cell::U(1), Cell::BadString]],
    )]);
    for sql in [
        "SELECT Missing FROM Sample",
        "SELECT a.ID FROM Sample a JOIN Sample b ON a.ID=b.ID ORDER BY Missing",
    ] {
        let before = captures(&f);
        let error =
            lycheedev_data::dispatch(&f.invocation("data sql", &[("--sql", sql)])).unwrap_err();
        assert_eq!(error.code, "query.unresolved_binding", "{sql}");
        assert_eq!(captures(&f), before);
    }
}
#[test]
fn local_default_effective_equal_replaces_corrupt_base_without_decoding_it() {
    let t = table(
        "Sample",
        201,
        &[("ID", "uint"), ("Name", "text")],
        &[vec![Cell::U(1), Cell::BadString]],
    );
    let hash = t.hash;
    let f = Fixture::new(&[t]);
    let mut raw = b"XFTH".to_vec();
    raw.extend(9u32.to_le_bytes());
    raw.extend(69933u32.to_le_bytes());
    raw.resize(44, 0);
    let mut payload = 1u32.to_le_bytes().to_vec();
    payload.extend(b"patched\0");
    let mut header = vec![0u8; 32];
    header[..4].copy_from_slice(b"XFTH");
    for (at, value) in [(8, 1u32), (16, hash), (20, 1), (24, payload.len() as u32)] {
        header[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    header[28] = 1;
    raw.extend(header);
    raw.extend(payload);
    let capture=evidence::commit_capture(&f.home,&raw,"application/octet-stream",json!({"kind":"hotfix-cache","locator":"fixture:lazy-effective","snapshot":f.pin,"dataBuild":catalog_fixture::BUILD}),true,false).unwrap();
    let request = f.root.join("lazy-effective-query.json");
    std::fs::write(&request,serde_json::to_vec(&json!({"sql":"SELECT ID,Name FROM effective.Sample WHERE ID IN (1,1.0)","hotfix":[capture["id"]]})).unwrap()).unwrap();
    let out = lycheedev_data::dispatch(
        &f.invocation("data sql", &[("--file", request.to_str().unwrap())]),
    )
    .unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[1, "patched"]]));
    verify(&f, &out);
    assert_eq!(out.data["effectiveSources"][0]["replacedOrAdded"], 1);
}

#[test]
fn local_default_correlated_in_subquery_and_cardinality_use_public_command_mapping() {
    let f = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[
            vec![Cell::U(1), Cell::U(10)],
            vec![Cell::U(2), Cell::U(20)],
            vec![Cell::U(3), Cell::U(30)],
        ],
    )]);
    let sql = "SELECT s.ID FROM Sample s WHERE s.ID IN (SELECT t.ID FROM Sample t WHERE t.Value >= s.Value) ORDER BY s.ID";
    let out = lycheedev_data::dispatch(&f.invocation("data sql", &[("--sql", sql)])).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[1], [2], [3]]));
    verify(&f, &out);
    let before = captures(&f);
    let error = lycheedev_data::dispatch(&f.invocation(
        "data sql",
        &[(
            "--sql",
            "SELECT ID FROM Sample WHERE ID IN (SELECT ID,Value FROM Sample)",
        )],
    ))
    .unwrap_err();
    assert_eq!(error.code, "query.invalid_cardinality");
    assert_eq!(error.exit_code, 4);
    assert_eq!(captures(&f), before);
}

#[test]
fn local_default_lazy_sql_preserves_ordered_invocation_control_after_capture() {
    let f = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(10)]],
    )]);
    let inv = f.invocation(
        "data sql",
        &[("--sql", "SELECT ID FROM Sample WHERE ID IN (1,99)")],
    );
    inv.control.ordered_child(inv.control.deadline()).unwrap();
    let out = lycheedev_data::dispatch(&inv).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[1]]));
    assert!(inv.control.stop_cause().unwrap().is_none());
    inv.control
        .ordered_child(inv.control.deadline())
        .unwrap()
        .check()
        .unwrap();
    verify(&f, &out);
}

#[test]
fn local_default_bare_and_static_names_share_root_and_explicit_catalog_bypasses_cte() {
    let f = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint")],
        &[vec![Cell::U(1)], vec![Cell::U(2)]],
    )]);
    for (sql, expected) in [
        (
            "SELECT a.ID,b.ID FROM Sample a JOIN static.Sample b ON a.ID=b.ID ORDER BY a.ID",
            json!([[1, 1], [2, 2]]),
        ),
        (
            "WITH Sample AS (SELECT ID FROM static.Sample WHERE ID=2) SELECT c.ID,s.ID FROM Sample c JOIN static.Sample s ON c.ID=s.ID",
            json!([[2, 2]]),
        ),
    ] {
        let out = lycheedev_data::dispatch(&f.invocation("data sql", &[("--sql", sql)])).unwrap();
        assert_eq!(out.data["result"]["rows"], expected);
        assert_eq!(
            out.data["sources"].as_array().unwrap().len(),
            1,
            "one canonical physical preparation"
        );
        verify(&f, &out);
    }
}

#[test]
fn local_default_nested_union_tail_uses_enclosing_callback_lexical_scope() {
    let f = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(10)], vec![Cell::U(2), Cell::U(20)]],
    )]);
    // Fixed Go compound.go executes the UNION suffix with the enclosing
    // executeSelect callback, whose CTE environment is the outer `allowed`.
    let sql = "WITH allowed AS (SELECT ID FROM Sample WHERE ID=1) SELECT s.ID FROM Sample s WHERE s.ID IN (WITH allowed AS (SELECT ID FROM Sample WHERE ID=2) SELECT ID FROM Sample WHERE ID=1 UNION ALL SELECT ID FROM Sample WHERE ID=2 ORDER BY ID IN (SELECT ID FROM allowed) DESC LIMIT 1)";
    let out = lycheedev_data::dispatch(&f.invocation("data sql", &[("--sql", sql)])).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[1]]));
    verify(&f, &out);
}

#[test]
fn local_default_nested_union_tail_preserves_runtime_qualifier_error_without_capture() {
    let f = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(10)], vec![Cell::U(2), Cell::U(20)]],
    )]);
    // Go's saved fields bind the outer qualifier during preflight, but its
    // restored compound row resolver uses ordinal qualifiers at execution.
    let sql = "SELECT s.ID FROM Sample s WHERE s.ID IN (SELECT ID FROM Sample WHERE ID=1 UNION ALL SELECT ID FROM Sample WHERE ID=2 ORDER BY ID IN (SELECT ID FROM Sample x WHERE x.ID=s.ID))";
    let before = captures(&f);
    let error = lycheedev_data::dispatch(&f.invocation("data sql", &[("--sql", sql)])).unwrap_err();
    assert_eq!(error.code, "query.unresolved_binding");
    assert_eq!(captures(&f), before);
}
