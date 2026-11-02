use super::super::control_tests::Fixture;
use super::*;
use lycheedev_core::metadata::Metadata;
fn raw() -> Vec<u8> {
    br#"<div id="app" data-page='{"component":"Hotfixes","props":{"hotfixes":{"current_page":1,"last_page":1,"per_page":25,"total":0,"data":[]}}}'></div>"#.to_vec()
}
#[test]
fn receipt_owner_roundtrip_preserves_cas_generation_and_typed_wire_rules() {
    let f = Fixture::new("wago");
    let budget = f.budget();
    let q = f.request.query.clone();
    let identity = Identity::new(&q, 1);
    let bytes = raw();
    let page = parse::parse(&bytes, &budget, &f.ctx.control).unwrap();
    let receipt = store(
        &f.ctx,
        identity.clone(),
        url("https://wago.tools", "", 1),
        &bytes,
        &page,
        "2026-10-16T00:00:00Z".into(),
        &budget,
    )
    .unwrap();
    let key = format!("hotfix-wago-page/{}", identity.key(&budget).unwrap());
    let first = DataCacheMetadata::read_document_raw_controlled_with_check(
        &f.ctx.home,
        &key,
        &f.ctx.control,
        || Ok(()),
    )
    .unwrap();
    store(
        &f.ctx,
        identity.clone(),
        "different irrelevant request url".into(),
        &bytes,
        &page,
        "2026-10-16T01:00:00Z".into(),
        &budget,
    )
    .unwrap();
    assert_eq!(
        DataCacheMetadata::read_document_raw_controlled_with_check(
            &f.ctx.home,
            &key,
            &f.ctx.control,
            || Ok(())
        )
        .unwrap()
        .generation,
        first.generation
    );
    let (loaded, body) = load(&f.ctx, &identity, &budget).unwrap().unwrap();
    assert_eq!(body, bytes);
    assert_eq!(loaded.response, receipt.response);
    let wire = String::from_utf8(first.bytes).unwrap();
    // Current typed receipts reject legacy-duplicated or case-variant fields
    // instead of applying Go's last-wins JSON rules.
    let appended = format!(
        "{},\"CapturedAt\":null,\"TOTAL\":null,\"total\":7,\"unknown\":[[]]}}",
        &wire[..wire.len() - 1]
    );
    let error = receipt_wire::decode(appended.as_bytes(), &f.ctx, &budget).unwrap_err();
    assert_eq!(error.code, "records.hotfix_cache_corrupt");
    // Escaped JSON strings decode to the same typed instant.
    let escaped = wire.replace("2026-10-16T00:00:00Z", r"2026-10-16T00:00:00\u005a");
    let decoded = receipt_wire::decode(escaped.as_bytes(), &f.ctx, &budget).unwrap();
    assert_eq!(decoded.captured_at, receipt.captured_at);
    assert!(f.ctx.control.stop_cause().unwrap().is_none());
}
#[test]
fn expected_miss_or_conflict_with_cleanup_is_never_swallowed() {
    for code in ["vault.record_missing", "vault.generation_conflict"] {
        let expected = ToolkitError::new(code, "fixture", 3);
        assert!(pure(&expected, code));
        let compound = lycheedev_core::metadata::owned_write::finish_owned::<()>(
            Err(expected),
            Err(ToolkitError::new("external.io", "close failed", 5)),
        )
        .unwrap_err();
        assert!(!pure(&compound, code));
        let mapped = mapped_cause("cache_write", 5, compound.clone());
        assert_eq!(
            mapped.details["originalCause"],
            serde_json::to_value(compound).unwrap()
        );
    }
}
#[test]
fn receipt_independent_deadline_is_not_cache_corruption_or_offline_miss() {
    let f = Fixture::new("wago");
    let budget = Budget::new(
        Cost::defaults(),
        std::time::Instant::now() - std::time::Duration::from_secs(1),
        None,
    );
    let q = f.request.query.clone();
    let error = load(&f.ctx, &Identity::new(&q, 1), &budget).err().unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    f.ctx.control.check().unwrap();
}

#[test]
fn hotfix_pin_opens_metadata_before_unmarshalable_time_identity() {
    let f = Fixture::new("wago");
    let budget = f.budget();
    std::fs::create_dir_all(f.ctx.home.join("state")).unwrap();
    std::fs::write(f.ctx.home.join("state/toolkit.sqlite"), b"broken SQLite").unwrap();
    let change = selection::ChangePin {
        provider: "wago".into(),
        product: "retail".into(),
        full_build: "12.0.7.68974".into(),
        region: "us".into(),
        scope: "fixture".into(),
        fetched_at: "2026-10-16T00:00:00+24:00".into(),
        sha256: "a".repeat(64),
    };
    let failure = super::super::pin(&f.ctx, change, &budget).unwrap_err();
    assert_eq!(failure.code, "vault.metadata");
    assert!(!failure.message.contains("Time.MarshalJSON"));
    f.ctx.control.check().unwrap();
}

#[test]
fn wago_page_missing_private_alias_never_reads_historical_shared_page() {
    let f = Fixture::new("wago");
    let budget = f.budget();
    let query = f.request.query.clone();
    let identity = Identity::new(&query, 1);
    let bytes = raw();
    let page = parse::parse(&bytes, &budget, &f.ctx.control).unwrap();
    let receipt = store(
        &f.ctx,
        identity.clone(),
        url("https://wago.tools", "", 1),
        &bytes,
        &page,
        "2026-10-16T00:00:00Z".into(),
        &budget,
    )
    .unwrap();
    let key = format!("hotfix-wago-page/{}", identity.key(&budget).unwrap());
    let mut shared = Metadata::open(&f.ctx.home, true).unwrap();
    shared.put(&key, &receipt, None).unwrap();
    shared.close().unwrap();
    assert!(
        crate::cache_fixture::Cache::new(&f.ctx.home)
            .delete(&key)
            .unwrap()
    );
    assert!(load(&f.ctx, &identity, &budget).unwrap().is_none());
    let shared = Metadata::open(&f.ctx.home, false).unwrap();
    let historical: Value = shared.read(&key).unwrap();
    assert_eq!(historical["response"], receipt.response);
    shared.close().unwrap();
}

#[test]
fn typed_query_time_window_denotes_the_same_rfc3339_instant_and_scope() {
    let f=Fixture::new("wago");let mut query=f.request.query.clone();
    query.from=Some("2026-10-16T00:00:00+02:00".into());query.to=Some("2026-10-15T22:00:00Z".into());
    query.validate().unwrap();
    assert_eq!(model::parse_time(query.from.as_ref().unwrap()).unwrap(),model::parse_time(query.to.as_ref().unwrap()).unwrap());
    let scope=query.scope().unwrap();assert!(scope.contains("from:2026-10-15T22:00:00Z"));assert!(scope.contains("to:2026-10-15T22:00:00Z"));
}
