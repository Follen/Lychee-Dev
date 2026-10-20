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
        let out = crate::migration_oracle::dispatch(&f.invocation(sql)).unwrap();
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
        let out = crate::migration_oracle::dispatch(&f.invocation(sql)).unwrap();
        assert_eq!(out.data["result"]["rows"], json!([]));
        f.verify(&out);
    }
    for sql in [
        "SELECT ID FROM RemoteSample",
        "SELECT ID FROM RemoteSample WHERE ID IN (7)",
        "SELECT ID FROM RemoteSample WHERE ID+0=99",
    ] {
        let before = f.captures().len();
        let error = crate::migration_oracle::dispatch(&f.invocation(sql)).unwrap_err();
        assert_eq!(error.code, "table.invalid_format", "{sql}");
        assert_eq!(f.captures().len(), before);
    }
    f.close();
}
#[test]
fn default_remote_global_binding_error_precedes_corrupt_right_row_decode() {
    let f = SqlFixture::new(true, true);
    let before = f.captures().len();
    let error = crate::migration_oracle::dispatch(&f.invocation(
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
    let out = crate::migration_oracle::dispatch(&f.effective_invocation(
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
    let out = crate::migration_oracle::dispatch(&inv).unwrap();
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
    let out = crate::migration_oracle::dispatch(&f.effective_invocation(
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

#[test]
fn default_remote_quoted_physical_names_and_dotted_alias_preserve_equal_order() {
    let f = SqlFixture::with_rows(false, true, 3);
    for sql in [
        r#"SELECT "r.s"."ID" FROM "static"."RemoteSample" AS "r.s" WHERE "r.s"."ID" IN (9,7,9.0,NULL,8)"#,
        r#"SELECT "RemoteSample"."ID" FROM "ſtatic"."RemoteSample" WHERE "RemoteSample"."ID" IN (9,7,9.0,NULL,8)"#,
    ] {
        let out = crate::migration_oracle::dispatch(&f.invocation(sql)).unwrap();
        assert_eq!(out.data["result"]["rows"], json!([[9], [7], [8]]), "{sql}");
        f.verify(&out);
    }
    f.close();
}

#[test]
fn default_remote_quoted_effective_names_keep_overlay_and_original_capture() {
    let f = SqlFixture::new(false, true);
    let cache = f.hotfix_capture(&f.snapshot, true, false);
    let out = crate::migration_oracle::dispatch(&f.effective_invocation(
        r#"SELECT "e.f"."ID" FROM "effective"."RemoteSample" AS "e.f" WHERE "e.f"."ID" IN (7,99)"#,
        std::slice::from_ref(&cache),
    ))
    .unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    assert_eq!(out.data["effectiveSources"][0]["captures"][0]["id"], cache);
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_quoted_meta_identifier_avoids_unrequested_physical_options() {
    let f = SqlFixture::new(false, false);
    let mut inv = f.invocation(r#"SELECT COUNT(*) FROM "meta"."RemoteSample" AS "m.n""#);
    inv.flags
        .insert("--installation".into(), "not-a-source-for-meta".into());
    inv.flags
        .insert("--key-file".into(), "missing-private-keys".into());
    inv.flags
        .insert("--content-variant".into(), "invalid-unread-variant".into());
    let out = crate::migration_oracle::dispatch(&inv).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[0]]));
    assert_eq!(out.data["sources"], json!([]));
    assert_eq!(out.data["semanticSources"].as_array().unwrap().len(), 1);
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_schema_preflight_binding_precedes_later_missing_definition() {
    let f = SqlFixture::new(true, true);
    for sql in [
        "WITH early AS (SELECT Missing FROM RemoteSample), later AS (SELECT ID FROM NoSuchTable) SELECT ID FROM later",
        "SELECT Missing FROM RemoteSample UNION ALL SELECT ID FROM NoSuchTable",
    ] {
        let before = f.captures().len();
        let error = crate::migration_oracle::dispatch(&f.invocation(sql)).unwrap_err();
        assert_eq!(error.code, "query.unresolved_binding", "{sql}");
        assert_eq!(f.captures().len(), before);
    }
    f.close();
}

#[test]
fn default_remote_binding_only_cte_does_not_prepare_casc_or_keys() {
    let f = SqlFixture::new(false, false);
    // A forward CTE is reserved before definitions run, so binding fails
    // without requesting a physical source. FROM and names obey Go grammar.
    let mut inv = f.invocation("WITH early AS (SELECT Missing FROM later), later AS (SELECT ID FROM NoSuchTable) SELECT ID FROM later");
    inv.flags
        .insert("--installation".into(), "unrequested-installation".into());
    inv.flags.insert("--keys".into(), "unrequested-keys".into());
    let before = f.captures().len();
    assert_eq!(
        crate::migration_oracle::dispatch(&inv).unwrap_err().code,
        "query.unresolved_binding"
    );
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    assert_eq!(f.captures().len(), before);
    f.close();
}

#[test]
fn default_remote_root_resolution_reuses_case_canonical_table_across_passes() {
    let f = SqlFixture::with_rows(false, true, 3);
    let out = crate::migration_oracle::dispatch(&f.invocation("SELECT ID FROM RemoteSample WHERE ID IN (9,7) UNION ALL SELECT ID FROM STATIC.remotesample WHERE ID=8")).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[9], [7], [8]]));
    assert_eq!(out.data["sources"].as_array().unwrap().len(), 1);
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_scalar_exists_and_analyze_use_one_session_and_original_capture() {
    let f = SqlFixture::with_rows(false, true, 3);
    let sql = "SELECT r.ID,(SELECT x.ID FROM RemoteSample x WHERE x.ID=r.ID),EXISTS (SELECT ID FROM RemoteSample x WHERE x.ID=r.ID) FROM RemoteSample r ORDER BY r.ID";
    let out = crate::migration_oracle::dispatch(&f.invocation(sql)).unwrap();
    assert_eq!(
        out.data["result"]["rows"],
        json!([[7, 7, true], [8, 8, true], [9, 9, true]])
    );
    assert_eq!(out.data["sources"].as_array().unwrap().len(), 1);
    f.verify(&out);
    let inv =
        f.invocation("EXPLAIN ANALYZE SELECT ID FROM RemoteSample WHERE ID IN (9,7,9.0,NULL,8)");
    let out = crate::migration_oracle::dispatch(&inv).unwrap();
    assert_eq!(out.data["result"]["rows"], Value::Null);
    assert_eq!(
        out.data["result"]["plan"]["measured"]["scans"],
        json!([{"table":{"catalog":"static","name":"remotesample"},"calls":4,"rows":3,"lookups":4}])
    );
    assert_eq!(out.data["result"]["plan"]["measured"]["outputRows"], 3);
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
fn default_remote_effective_analyze_uses_true_overlay_lookup_and_capture_identity() {
    let f = SqlFixture::with_rows(false, true, 3);
    let cache = f.hotfix_capture(&f.snapshot, true, false);
    let out = crate::migration_oracle::dispatch(&f.effective_invocation(
        "EXPLAIN ANALYZE SELECT ID FROM effective.RemoteSample WHERE ID IN (9,7,9.0,NULL,8)",
        std::slice::from_ref(&cache),
    ))
    .unwrap();
    assert_eq!(
        out.data["result"]["plan"]["measured"]["scans"],
        json!([{"table":{"catalog":"effective","name":"remotesample"},"calls":4,"rows":3,"lookups":4}])
    );
    assert_eq!(out.data["effectiveSources"][0]["captures"][0]["id"], cache);
    f.verify(&out);
    f.close();
}
#[test]
fn default_remote_analyze_and_scalar_failures_return_no_partial_capture() {
    let f = SqlFixture::with_rows(false, true, 3);
    for sql in [
        "SELECT (SELECT ID FROM RemoteSample) FROM RemoteSample",
        "EXPLAIN ANALYZE SELECT (SELECT ID FROM RemoteSample) FROM RemoteSample",
        "SELECT (SELECT ID,ID FROM RemoteSample WHERE FALSE) FROM RemoteSample WHERE FALSE",
    ] {
        let before = f.captures().len();
        assert!(crate::migration_oracle::dispatch(&f.invocation(sql)).is_err(), "{sql}");
        assert_eq!(f.captures().len(), before);
    }
    f.close();
}

#[test]
fn default_remote_cast_like_complete_lazy_scan_and_capture() {
    let f = SqlFixture::with_rows(false, true, 3);
    let out=crate::migration_oracle::dispatch(&f.invocation("SELECT CAST(ID AS TEXT),CAST('true' AS BOOLEAN) FROM RemoteSample WHERE CAST(ID AS TEXT) LIKE '9'")).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([["9", true]]));
    f.verify(&out);
    let out =
        crate::migration_oracle::dispatch(&f.invocation(
            "EXPLAIN ANALYZE SELECT ID FROM RemoteSample WHERE CAST(ID AS TEXT) LIKE '_'",
        ))
        .unwrap();
    assert_eq!(out.data["result"]["plan"]["measured"]["outputRows"], 3);
    assert_eq!(
        out.data["result"]["plan"]["measured"]["scans"][0]["rows"],
        3
    );
    f.verify(&out);
    f.close();
}

#[test]
fn default_remote_effective_historical_source_keeps_original_control_ordered() {
    let f = SqlFixture::new(false, true);
    let cache = f.hotfix_capture(&f.snapshot, true, false);
    let inv = f.effective_invocation(
        "SELECT ID FROM effective.RemoteSample WHERE ID IN (7,99)",
        std::slice::from_ref(&cache),
    );
    let out = crate::migration_oracle::dispatch(&inv).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    assert_eq!(out.data["effectiveSources"][0]["captures"][0]["id"], cache);
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
fn default_remote_sql_compiler_stop_publishes_no_capture_or_file() {
    let f = SqlFixture::new(false, false);
    let inv = f.invocation("SELECT FROM RemoteSample");
    let before = f.captures().len();
    inv.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(crate::migration_oracle::dispatch(&inv).unwrap_err().code, "command.cancelled");
    assert_eq!(f.captures().len(), before);
    f.close();
}

#[test]
fn default_remote_nonfinal_suffix_reaches_real_cte_resolver_before_syntax() {
    let f = SqlFixture::new(false, true);
    let before = f.captures().len();
    let sql = "WITH c AS (SELECT Missing FROM RemoteSample) SELECT ID FROM RemoteSample LIMIT 1 UNION ALL SELECT ID FROM RemoteSample";
    let error = crate::migration_oracle::dispatch(&f.invocation(sql)).unwrap_err();
    assert_eq!(error.code, "query.unresolved_binding");
    assert_eq!(f.captures().len(), before);
    let error = crate::migration_oracle::dispatch(
        &f.invocation("SELECT ID FROM RemoteSample LIMIT 1 UNION ALL SELECT ID FROM RemoteSample"),
    )
    .unwrap_err();
    assert_eq!(error.code, "query.invalid_syntax");
    assert_eq!(f.captures().len(), before);
    f.close();
}
#[test]
fn default_remote_right_union_cte_and_nested_suffix_use_one_complete_compiler() {
    let f = SqlFixture::new(false, true);
    let out=crate::migration_oracle::dispatch(&f.invocation("SELECT ID FROM RemoteSample UNION ALL WITH c AS (SELECT ID FROM RemoteSample) SELECT ID FROM c ORDER BY ID LIMIT 1")).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    f.verify(&out);
    let before = f.captures().len();
    let error=crate::migration_oracle::dispatch(&f.invocation("SELECT EXISTS(SELECT ID FROM RemoteSample LIMIT 1 UNION ALL SELECT ID FROM RemoteSample) FROM RemoteSample")).unwrap_err();
    assert_eq!(error.code, "query.invalid_syntax");
    assert_eq!(f.captures().len(), before);
    f.close();
}

#[test]
fn default_remote_invalid_cte_grammar_fails_before_any_physical_options() {
    let f = SqlFixture::new(false, false);
    for sql in [
        "WITH early AS (SELECT Missing), later AS (SELECT ID FROM NoSuchTable) SELECT ID FROM later",
        "WITH first AS (SELECT Missing FROM later), later AS (SELECT ID FROM NoSuchTable) SELECT ID FROM later",
    ] {
        let mut inv = f.invocation(sql);
        inv.flags
            .insert("--installation".into(), "unrequested-installation".into());
        inv.flags.insert("--keys".into(), "unrequested-keys".into());
        let before = f.captures().len();
        let error = crate::migration_oracle::dispatch(&inv).unwrap_err();
        assert_eq!(error.code, "query.invalid_syntax", "{sql}: {error:?}");
        assert!(f.source.backend.calls.lock().unwrap().is_empty());
        assert_eq!(f.captures().len(), before);
    }
    f.close();
}
