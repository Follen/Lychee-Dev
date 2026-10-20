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
    let q = Query::from_invocation(&f.inv).unwrap();
    let identity = Identity::new(&q, 1);
    let bytes = raw();
    let page = parse::parse(&bytes, &budget, &f.inv.control).unwrap();
    let receipt = store(
        &f.inv,
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
        &f.inv.home,
        &key,
        &f.inv.control,
        || Ok(()),
    )
    .unwrap();
    store(
        &f.inv,
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
            &f.inv.home,
            &key,
            &f.inv.control,
            || Ok(())
        )
        .unwrap()
        .generation,
        first.generation
    );
    let (loaded, body) = load(&f.inv, &identity, &budget).unwrap().unwrap();
    assert_eq!(body, bytes);
    assert_eq!(loaded.response, receipt.response);
    let wire = String::from_utf8(first.bytes).unwrap();
    let appended = format!(
        "{},\"CapturedAt\":null,\"TOTAL\":null,\"total\":7,\"unknown\":[[]]}}",
        &wire[..wire.len() - 1]
    );
    let decoded = receipt_wire::decode(appended.as_bytes(), &f.inv, &budget).unwrap();
    assert_eq!(decoded.total, 7);
    assert_eq!(decoded.captured_at, receipt.captured_at);
    let escaped = wire.replace("2026-10-16T00:00:00Z", r"2026-10-16T00:00:00\u005a");
    assert!(receipt_wire::decode(escaped.as_bytes(), &f.inv, &budget).is_err());
    assert!(f.inv.control.stop_cause().unwrap().is_none());
}
#[test]
fn expected_miss_or_conflict_with_cleanup_is_never_swallowed() {
    for code in ["vault.record_missing", "vault.generation_conflict"] {
        let expected = ToolkitError::new(code, "fixture", 3);
        assert!(pure(&expected, code));
        let compound = lycheedev_core::metadata::historical_write::finish_historical::<()>(
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
    let q = Query::from_invocation(&f.inv).unwrap();
    let error = load(&f.inv, &Identity::new(&q, 1), &budget).err().unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    f.inv.control.check().unwrap();
}

fn seed_historical_time(f: &Fixture, budget: &Budget, number: i64, last: i64, stamp: &str) {
    let q = Query::from_invocation(&f.inv).unwrap();
    let identity = Identity::new(&q, number);
    let page_json = json!({"component":"Hotfixes","props":{"hotfixes":{
        "current_page":number,"last_page":last,"per_page":25,"total":0,"data":[],
        "next_page_url": if number < last { format!("/hotfixes?page={}", number+1) } else { String::new() }
    }}});
    let bytes = format!("<div id=\"app\" data-page='{}'></div>", page_json).into_bytes();
    let parsed = parse::parse(&bytes, budget, &f.inv.control).unwrap();
    let receipt = store(
        &f.inv,
        identity.clone(),
        url("https://wago.tools", "", number),
        &bytes,
        &parsed,
        "2026-10-16T00:00:00Z".into(),
        budget,
    )
    .unwrap();
    // The Go reader accepts these historical raw time tokens although the Go
    // writer cannot produce a >=24h offset. Seed via the generic document owner,
    // never weaken the receipt producer's JSON time serializer.
    let mut value = serde_json::to_value(receipt).unwrap();
    value["capturedAt"] = json!(stamp);
    let key = format!("hotfix-wago-page/{}", identity.key(budget).unwrap());
    let current = DataCacheMetadata::read_document_raw_controlled_with_check(
        &f.inv.home,
        &key,
        &f.inv.control,
        || budget.check(),
    )
    .unwrap();
    // store() already published this key. The intentional historical rewrite
    // must compare that generation; None means create, not unconditional update.
    DataCacheMetadata::put_controlled_with_check(
        &f.inv.home,
        &key,
        &value,
        Some(current.generation),
        &f.inv.control,
        || budget.check(),
    )
    .unwrap();
}
#[test]
fn historical_noncanonical_offset_compares_without_early_cache_error() {
    for (first, second, expected) in [
        (
            "2026-10-16T00:00:00+24:00",
            "2026-10-15T00:00:00.000000001Z",
            "2026-10-15T00:00:00.000000001Z",
        ),
        (
            "2026-10-15T22:00:00Z",
            "2026-10-16T00:00:00+00:60",
            "2026-10-16T00:00:00+01:00",
        ),
    ] {
        let mut f = Fixture::new("wago");
        f.inv.flags.insert("--offline".into(), "true".into());
        let budget = f.budget();
        seed_historical_time(&f, &budget, 1, 2, first);
        seed_historical_time(&f, &budget, 2, 2, second);
        let result = super::super::run(&f.inv, &budget).unwrap();
        assert_eq!(result.change.fetched_at, expected);
        assert_eq!(result.coverage.captured_at.as_deref(), Some(expected));
        assert!(result.capture.is_some());
        let pin = selection::load_pinned_controlled(&f.inv.home, &result.snapshot, &f.inv.control)
            .unwrap();
        assert_eq!(pin.changes.as_ref(), Some(&result.change));
        f.inv.control.check().unwrap();
    }
}
#[test]
fn selected_unmarshalable_time_fails_at_pin_marshal_after_complete_page_walk() {
    for second in ["2026-10-15T00:00:00Z", "2026-10-14T23:59:59.999999999Z"] {
        let mut f = Fixture::new("wago");
        f.inv.flags.insert("--offline".into(), "true".into());
        let budget = f.budget();
        seed_historical_time(&f, &budget, 1, 2, "2026-10-16T00:00:00+24:00");
        seed_historical_time(&f, &budget, 2, 2, second);
        // Go After is strictly greater: an equal second page cannot replace the
        // first timezone representation merely to make marshaling succeed.
        let query_result = query(
            &f.inv,
            Query::from_invocation(&f.inv).unwrap(),
            FetchBudget::default(),
            &budget,
            &PublicFetcher,
            "https://wago.tools",
        )
        .unwrap();
        assert_eq!(query_result.coverage.end_page, 2);
        assert_eq!(query_result.change.fetched_at, "2026-10-16T00:00:00+24:00");
        let error = super::super::run(&f.inv, &budget).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("command.failed", 5)
        );
        assert!(
            error
                .message
                .contains("Time.MarshalJSON: timezone hour outside of range")
        );
        let owner = Metadata::open_controlled(&f.inv.home, false, &f.inv.control).unwrap();
        assert!(owner.list::<Value>("pin/", 100).unwrap().is_empty());
        assert!(owner.list::<Value>("capture/", 100).unwrap().is_empty());
        owner.close().unwrap();
        f.inv.control.check().unwrap();
    }
}

#[test]
fn hotfix_pin_opens_metadata_before_unmarshalable_time_identity() {
    let f = Fixture::new("wago");
    let budget = f.budget();
    std::fs::create_dir_all(f.inv.home.join("state")).unwrap();
    std::fs::write(f.inv.home.join("state/toolkit.sqlite"), b"broken SQLite").unwrap();
    let change = selection::ChangePin {
        provider: "wago".into(),
        product: "retail".into(),
        full_build: "12.0.7.68974".into(),
        region: "us".into(),
        scope: "fixture".into(),
        fetched_at: "2026-10-16T00:00:00+24:00".into(),
        sha256: "a".repeat(64),
    };
    let failure = super::super::pin(&f.inv, change, &budget).unwrap_err();
    assert_eq!(failure.code, "vault.metadata");
    assert!(!failure.message.contains("Time.MarshalJSON"));
    f.inv.control.check().unwrap();
}

#[test]
fn query_offset_uses_exact_instant_and_retains_late_marshal_failure_after_pin() {
    let mut f = Fixture::new("wago");
    f.inv.flags.insert("--offline".into(), "true".into());
    f.inv
        .flags
        .insert("--from".into(), "2026-10-16T00:00:00+24:00".into());
    f.inv
        .flags
        .insert("--to".into(), "2026-10-15T00:00:00Z".into());
    let budget = f.budget();
    let q = Query::from_invocation(&f.inv).unwrap();
    assert_eq!(q.from.as_deref(), Some("2026-10-16T00:00:00+24:00"));
    assert!(q.scope().unwrap().contains("from:2026-10-15T00:00:00Z"));
    assert_eq!(
        model::parse_time(q.from.as_ref().unwrap()).unwrap(),
        model::parse_time(q.to.as_ref().unwrap()).unwrap()
    );
    seed_historical_time(&f, &budget, 1, 1, "2026-10-16T00:00:00Z");
    let error = super::super::run(&f.inv, &budget).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.failed", 5)
    );
    let owner = Metadata::open_controlled(&f.inv.home, false, &f.inv.control).unwrap();
    assert_eq!(owner.list::<Value>("pin/", 100).unwrap().len(), 1);
    assert!(owner.list::<Value>("capture/", 100).unwrap().is_empty());
    owner.close().unwrap();
    f.inv
        .flags
        .insert("--from".into(), "2026-10-15T23:00:00+00:60".into());
    f.inv
        .flags
        .insert("--to".into(), "2026-10-16T00:00:00Z".into());
    let normalized = Query::from_invocation(&f.inv).unwrap();
    assert_eq!(
        normalized.from.as_deref(),
        Some("2026-10-15T23:00:00+01:00")
    );
    assert!(serde_json::to_value(normalized).is_ok());
    for (input, expected) in [
        (
            "2026-10-15   1:02:03,12345678999",
            "2026-10-15T01:02:03.123456789Z",
        ),
        ("0000-01-01T00:00:00+24:00", "-0001-12-31T00:00:00Z"),
        ("9999-12-31T23:00:00-24:60", "10000-01-02T00:00:00Z"),
    ] {
        let mut query = Query::from_invocation(&f.inv).unwrap();
        query.from = Some(input.into());
        assert!(query.scope().unwrap().contains(&format!("from:{expected}")));
    }
    assert!(model::parse_time("2026-10-15\t01:02:03").is_err());
    assert!(model::parse_time("2026-10-15 01:02:03Z").is_err());
}

#[test]
fn wago_page_missing_private_alias_never_reads_historical_shared_page() {
    let f = Fixture::new("wago");
    let budget = f.budget();
    let query = Query::from_invocation(&f.inv).unwrap();
    let identity = Identity::new(&query, 1);
    let bytes = raw();
    let page = parse::parse(&bytes, &budget, &f.inv.control).unwrap();
    let receipt = store(
        &f.inv,
        identity.clone(),
        url("https://wago.tools", "", 1),
        &bytes,
        &page,
        "2026-10-16T00:00:00Z".into(),
        &budget,
    )
    .unwrap();
    let key = format!("hotfix-wago-page/{}", identity.key(&budget).unwrap());
    let mut shared = Metadata::open(&f.inv.home, true).unwrap();
    shared.put(&key, &receipt, None).unwrap();
    shared.close().unwrap();
    assert!(
        crate::cache_fixture::Cache::new(&f.inv.home)
            .delete(&key)
            .unwrap()
    );
    assert!(load(&f.inv, &identity, &budget).unwrap().is_none());
    let shared = Metadata::open(&f.inv.home, false).unwrap();
    let historical: Value = shared.read(&key).unwrap();
    assert_eq!(historical["response"], receipt.response);
    shared.close().unwrap();
}
