//! Actual default CDN SQL uses the same query-borrowed lazy provider as local.
use super::*;
#[test]
fn default_remote_numeric_in_uses_candidate_order_and_explicit_order_only() {
    let f = SqlFixture::with_rows(false, true, 3);
    for (sql, expected) in [
        (
            "SELECT ID FROM static.RemoteSample WHERE ID IN (9,7,9.0,NULL,8)",
            json!([[9], [7], [8]]),
        ),
        (
            "SELECT ID FROM static.RemoteSample WHERE ID IN (9,7) ORDER BY ID",
            json!([[7], [9]]),
        ),
    ] {
        let out = crate::dispatch(&f.invocation(sql)).unwrap();
        assert_eq!(out.data["result"]["rows"], expected);
        f.verify(&out);
        assert!(out.data["memoryBudget"]["retainedBytes"].is_u64());
    }
    f.close();
}
#[test]
fn default_remote_missing_equal_and_in_do_not_decode_corrupt_rows() {
    let f = SqlFixture::new(true, true);
    for sql in [
        "SELECT ID FROM RemoteSample WHERE ID=99",
        "SELECT ID FROM RemoteSample WHERE ID IN (99,98)",
    ] {
        let out = crate::dispatch(&f.invocation(sql)).unwrap();
        assert_eq!(out.data["result"]["rows"], json!([]));
        f.verify(&out);
    }
    for sql in [
        "SELECT ID FROM RemoteSample",
        "SELECT ID FROM RemoteSample WHERE ID IN (7)",
        "SELECT ID FROM RemoteSample WHERE ID+0=99",
    ] {
        let before = f.captures().len();
        let error = crate::dispatch(&f.invocation(sql)).unwrap_err();
        assert_eq!(error.code, "table.invalid_format", "{sql}");
        assert_eq!(f.captures().len(), before);
    }
    f.close();
}
#[test]
fn default_remote_global_binding_error_precedes_corrupt_right_row_decode() {
    let f = SqlFixture::new(true, true);
    let before = f.captures().len();
    let error = crate::dispatch(&f.invocation(
        "SELECT a.ID FROM RemoteSample a JOIN RemoteSample b ON a.ID=b.ID ORDER BY Missing",
    ))
    .unwrap_err();
    assert_eq!(error.code, "query.unresolved_binding");
    assert_eq!(f.captures().len(), before);
    f.close();
}
#[test]
fn default_remote_effective_missing_equal_skips_corrupt_base_and_keeps_capture_identity() {
    let f = SqlFixture::new(true, true);
    let cache = f.hotfix_capture(&f.snapshot, true, false);
    let out = crate::dispatch(&f.effective_invocation(
        "SELECT ID FROM effective.RemoteSample WHERE ID IN (99,98)",
        std::slice::from_ref(&cache),
    ))
    .unwrap();
    assert_eq!(out.data["result"]["rows"], json!([]));
    assert_eq!(out.data["effectiveSources"][0]["captures"][0]["id"], cache);
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_lazy_sql_does_not_expose_ordered_invocation_control() {
    let f = SqlFixture::new(false, true);
    let inv = f.invocation("SELECT ID FROM RemoteSample WHERE ID IN (7,99)");
    inv.control.ordered_child(inv.control.deadline()).unwrap();
    let out = crate::dispatch(&inv).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    assert!(inv.control.stop_cause().unwrap().is_none());
    inv.control
        .ordered_child(inv.control.deadline())
        .unwrap()
        .check()
        .unwrap();
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_static_and_effective_aliases_consume_one_bundle_with_independent_schemas() {
    let f = SqlFixture::with_rows(false, true, 3);
    let cache = f.hotfix_capture(&f.snapshot, true, false);
    let out = crate::dispatch(&f.effective_invocation(
        "SELECT s.ID,e.ID,e.__hotfix_source FROM static.RemoteSample s JOIN effective.RemoteSample e ON s.ID=e.ID ORDER BY s.ID",
        std::slice::from_ref(&cache),
    )).unwrap();
    assert_eq!(
        out.data["result"]["rows"],
        json!([[7, 7, null], [8, 8, null], [9, 9, null]])
    );
    assert_eq!(out.data["sources"].as_array().unwrap().len(), 2);
    assert_eq!(out.data["effectiveSources"].as_array().unwrap().len(), 1);
    assert_eq!(out.data["effectiveSources"][0]["captures"][0]["id"], cache);
    f.verify(&out);
    f.close();
}
