use super::super::control_tests::Fixture;
use super::*;
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
    let owner = Metadata::open(&f.inv.home, false).unwrap();
    let first = owner
        .read_document_on_owner_controlled(&key, &f.inv.control)
        .unwrap();
    owner.close().unwrap();
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
    let owner = Metadata::open(&f.inv.home, false).unwrap();
    assert_eq!(
        owner
            .read_document_on_owner_controlled(&key, &f.inv.control)
            .unwrap()
            .generation,
        first.generation
    );
    owner.close().unwrap();
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
