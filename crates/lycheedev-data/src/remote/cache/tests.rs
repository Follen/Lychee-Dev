use super::*;
use md5::{Digest, Md5};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    home: PathBuf,
    control: OperationControl,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/remote-cache-fixtures")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                storage::utc_now().replace([':', '.'], "-"),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        Self {
            home,
            control: OperationControl::default(),
        }
    }
    fn store(&self) -> Store<'_> {
        Store {
            home: &self.home,
            control: &self.control,
            budget: None,
        }
    }
}
#[derive(Default)]
struct Fetch {
    values: RefCell<BTreeMap<String, Result<Vec<u8>>>>,
    calls: RefCell<Vec<String>>,
}
impl MetadataFetcher for Fetch {
    fn fetch(
        &self,
        locator: &str,
        maximum: usize,
        control: &OperationControl,
        _: Option<&Budget>,
    ) -> Result<Vec<u8>> {
        control.check()?;
        self.calls.borrow_mut().push(locator.into());
        let raw = self
            .values
            .borrow()
            .get(locator)
            .cloned()
            .unwrap_or_else(|| Err(fault("records.remote_http", 5, "fixture absent")))?;
        if raw.len() > maximum {
            return Err(super::super::limit());
        }
        Ok(raw)
    }
}
fn route() -> Distribution {
    Distribution {
        path: "tpr/wow".into(),
        hosts: vec!["cdn.example.test".into()],
    }
}
fn key(raw: &[u8]) -> String {
    crate::casc::hex(&Md5::digest(raw))
}
fn cdns(path: &str) -> Vec<u8> {
    format!("Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|{path}|cdn.example.test\n").into_bytes()
}
fn observation(store: &Store<'_>, marker: &str) -> Observation {
    Observation {
        product_code: "wow".into(),
        region: "cn".into(),
        observed_at: ObservationTime::parse(&storage::utc_now()).unwrap(),
        versions: store
            .publish_blob(marker.as_bytes(), 1 << 20)
            .unwrap()
            .try_into()
            .unwrap(),
        distribution: store
            .publish_blob(&cdns(marker), 1 << 20)
            .unwrap()
            .try_into()
            .unwrap(),
    }
}

#[test]
fn configuration_download_hit_and_offline_rehash_exact_key() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let fetch = Fetch::default();
    let raw = b"archives = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n".to_vec();
    let digest = key(&raw);
    fetch.values.borrow_mut().insert(
        route().config_url("cdn.example.test", &digest).unwrap(),
        Ok(raw.clone()),
    );
    let (first, reference) = store
        .configuration(&route(), &digest, false, &fetch)
        .unwrap();
    assert_eq!(first.raw, raw);
    let (cached, cached_reference) = store
        .configuration(&route(), &digest, true, &fetch)
        .unwrap();
    assert_eq!(cached.raw, first.raw);
    assert_eq!(cached_reference.sha256, reference.sha256);
    assert_eq!(fetch.calls.borrow().len(), 1);
    assert_eq!(
        store
            .configuration(&route(), &"b".repeat(32), true, &fetch)
            .unwrap_err()
            .code,
        "records.remote_unavailable_offline"
    );
}
#[test]
fn corrupt_cached_blob_is_terminal_and_never_replaced_by_network() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let fetch = Fetch::default();
    let raw = b"archives = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n".to_vec();
    let digest = key(&raw);
    fetch.values.borrow_mut().insert(
        route().config_url("cdn.example.test", &digest).unwrap(),
        Ok(raw),
    );
    let (_, reference) = store
        .configuration(&route(), &digest, false, &fetch)
        .unwrap();
    let path = evidence::blob_path(&fixture.home, &reference.sha256).unwrap();
    std::fs::write(path, b"corrupt").unwrap();
    assert!(
        store
            .configuration(&route(), &digest, false, &fetch)
            .is_err()
    );
    assert_eq!(fetch.calls.borrow().len(), 1);
}
#[test]
fn only_availability_errors_try_another_config_host() {
    for code in [
        "records.remote_http",
        "records.configuration_integrity",
        "records.query_resource_budget",
        "command.cancelled",
    ] {
        let fixture = Fixture::new();
        let store = fixture.store();
        let fetch = Fetch::default();
        let route = Distribution {
            path: "tpr/wow".into(),
            hosts: vec!["first.example.test".into(), "second.example.test".into()],
        };
        let raw = b"archives = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n".to_vec();
        let digest = key(&raw);
        fetch.values.borrow_mut().insert(
            route.config_url(&route.hosts[0], &digest).unwrap(),
            Err(fault(code, 5, code)),
        );
        fetch
            .values
            .borrow_mut()
            .insert(route.config_url(&route.hosts[1], &digest).unwrap(), Ok(raw));
        let result = store.configuration(&route, &digest, false, &fetch);
        if code == "records.remote_http" {
            assert!(result.is_ok());
            assert_eq!(fetch.calls.borrow().len(), 2);
        } else {
            assert_eq!(result.unwrap_err().code, code);
            assert_eq!(fetch.calls.borrow().len(), 1);
        }
    }
}
#[test]
fn malformed_or_wrong_digest_download_is_not_cached() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let fetch = Fetch::default();
    let digest = "a".repeat(32);
    fetch.values.borrow_mut().insert(
        route().config_url("cdn.example.test", &digest).unwrap(),
        Ok(b"wrong".to_vec()),
    );
    assert_eq!(
        store
            .configuration(&route(), &digest, false, &fetch)
            .unwrap_err()
            .code,
        "records.configuration_integrity"
    );
    assert!(
        store
            .read::<BlobRef>(&format!("remote-config/{digest}"))
            .unwrap()
            .is_none()
    );
}
#[test]
fn catalog_pair_cas_keeps_versions_and_distribution_from_one_observation() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let first = observation(&store, "tpr/first");
    let other = observation(&store, "tpr/second");
    store.remember(&first, None).unwrap();
    store.remember(&other, None).unwrap();
    let (retained, generation) = store.observation("wow", "cn").unwrap().unwrap();
    assert_eq!(retained.versions.sha256, first.versions.sha256);
    assert_eq!(retained.distribution.sha256, first.distribution.sha256);
    store.remember(&other, Some(generation)).unwrap();
    let (retained, _) = store.observation("wow", "cn").unwrap().unwrap();
    assert_eq!(retained.versions.sha256, other.versions.sha256);
    assert_eq!(retained.distribution.sha256, other.distribution.sha256);
}
#[test]
fn corrupt_observation_identity_does_not_fetch_online_fallback() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut value = observation(&store, "tpr/first");
    value.product_code = "wow_cn_beta".into();
    store.put("remote-catalog/wow/cn", &value, None).unwrap();
    assert_eq!(
        store.observation("wow", "cn").unwrap_err().code,
        "records.remote_identity"
    );
    value.product_code = "wow".into();
    value.observed_at = ObservationTime::default();
    assert_eq!(
        value.validate("wow", "cn").unwrap_err().code,
        "records.remote_identity"
    );
}
#[test]
fn reader_route_refresh_only_fetches_cdns_once_and_keeps_observed_pair() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let fetch = Fetch::default();
    let first = observation(&store, "tpr/first");
    store.remember(&first, None).unwrap();
    let locator = catalog_url("cn", "wow", "cdns").unwrap();
    fetch
        .values
        .borrow_mut()
        .insert(locator.clone(), Ok(cdns("tpr/refreshed")));
    let mut route = Route::prepare(&store, "wow", "cn", "wow", false, &fetch).unwrap();
    assert_eq!(route.current.path, "tpr/first");
    assert!(fetch.calls.borrow().is_empty());
    assert!(route.refresh(&store, &fetch).unwrap());
    assert_eq!(route.current.path, "tpr/refreshed");
    assert!(!route.refresh(&store, &fetch).unwrap());
    assert_eq!(*fetch.calls.borrow(), vec![locator]);
    let (retained, _) = store.observation("wow", "cn").unwrap().unwrap();
    assert_eq!(retained.versions.sha256, first.versions.sha256);
    assert_eq!(retained.distribution.sha256, first.distribution.sha256);
    let mut offline = Route::prepare(&store, "wow", "cn", "wow", true, &fetch).unwrap();
    assert_eq!(offline.current.path, "tpr/refreshed");
    assert!(!offline.refresh(&store, &fetch).unwrap());
}
#[test]
fn build_requires_all_root_encoding_sizes_and_exact_product_uid() {
    let build = b"root = 11111111111111111111111111111111\nencoding = 22222222222222222222222222222222 33333333333333333333333333333333\nencoding-size = 123 456\nbuild-uid = wow\n".to_vec();
    let cdn = b"archives = 44444444444444444444444444444444\n".to_vec();
    let release = catalog::Release {
        full_build: "12.1.0.9".into(),
        build_config: key(&build),
        cdn_config: key(&cdn),
    };
    let build = configuration(build, &release.build_config, None).unwrap();
    let cdn = configuration(cdn, &release.cdn_config, None).unwrap();
    assert_eq!(
        complete_build("wow", &release, build.clone(), cdn.clone())
            .unwrap()
            .encoding_bytes,
        456
    );
    assert_eq!(
        complete_build("wow_cn_beta", &release, build.clone(), cdn.clone())
            .unwrap_err()
            .code,
        "records.invalid_metadata"
    );
    for field in ["root", "encoding", "encoding-size"] {
        let mut changed = build.clone();
        changed.fields.remove(field);
        assert_eq!(
            complete_build("wow", &release, changed, cdn.clone())
                .unwrap_err()
                .code,
            "records.invalid_metadata"
        );
    }
}

#[test]
fn observation_rejects_zero_instant_in_every_offset_spelling() {
    let mut value = Observation {
        product_code: "wow".into(),
        region: "cn".into(),
        observed_at: ObservationTime::default(),
        versions: CachedBlob {
            sha256: "a".repeat(64),
            bytes: 1,
        },
        distribution: CachedBlob {
            sha256: "b".repeat(64),
            bytes: 1,
        },
    };
    for stamp in [
        "0001-01-01T00:00:00Z",
        "0001-01-01T01:00:00+01:00",
        "0000-12-31T23:00:00-01:00",
    ] {
        value.observed_at = ObservationTime::parse(stamp).unwrap();
        assert_eq!(
            value.validate("wow", "cn").unwrap_err().code,
            "records.remote_identity",
            "{stamp}"
        );
    }
    for stamp in [
        "0001-01-01T00:00:00.000000001Z",
        "2026-10-15T01:00:00+01:00",
    ] {
        value.observed_at = ObservationTime::parse(stamp).unwrap();
        value.validate("wow", "cn").unwrap();
    }
}
#[test]
fn cancelled_cache_publication_never_creates_the_document() {
    let fixture = Fixture::new();
    fixture.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        fixture
            .store()
            .put("remote-config/cancelled", &serde_json::json!({"v":1}), None)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    let control = OperationControl::default();
    assert_eq!(
        Metadata::read_document_raw_controlled(&fixture.home, "remote-config/cancelled", &control)
            .unwrap_err()
            .code,
        "vault.record_missing"
    );
}
#[test]
fn cache_lease_wait_uses_original_deadline_and_checked_owner_close() {
    let fixture = Fixture::new();
    let guard = fixture.store().lease("held-cache").unwrap();
    let control =
        OperationControl::uncancelable(std::time::Instant::now() + Duration::from_millis(30));
    let store = Store {
        home: &fixture.home,
        control: &control,
        budget: None,
    };
    let started = std::time::Instant::now();
    let result = store.lease("held-cache");
    guard.close().unwrap();
    assert_eq!(result.err().unwrap().code, "command.deadline");
    assert!(started.elapsed() < Duration::from_millis(500));
}

#[test]
fn expected_miss_conflict_and_busy_never_hide_composite_cleanup() {
    for code in [
        "vault.record_missing",
        "vault.generation_conflict",
        "workspace.busy",
    ] {
        let simple = fault(code, 3, "expected");
        assert!(pure_expected(&simple, code));
        let mut cleanup = fault("external.io", 9, "close failed");
        cleanup.details = serde_json::json!({"native":123});
        let combined =
            finish::<()>(Err(simple.clone()), Err(cleanup.clone()), Ok(()), Ok(())).unwrap_err();
        assert!(!pure_expected(&combined, code));
        assert_eq!(
            combined.details["cleanup"],
            serde_json::to_value(cleanup).unwrap()
        );
        let mut raw_close = simple;
        raw_close.details = serde_json::json!({"metadataCloseError":"fixture"});
        assert!(!pure_expected(&raw_close, code));
    }
}

#[test]
fn observation_go_time_reads_raw_tokens_and_defers_marshal_failure() {
    for (stamp, canonical) in [
        (
            "2026-10-14T2:00:00,123456789012Z",
            Some("2026-10-14T02:00:00.123456789Z"),
        ),
        (
            "2026-10-14T22:00:00+00:60",
            Some("2026-10-14T22:00:00+01:00"),
        ),
        ("2026-10-14T22:00:00+24:00", None),
    ] {
        let raw = format!(r#"{{"productCode":"wow","region":"cn","observedAt":"{stamp}"}}"#);
        let value: Observation = serde_json::from_str(&raw).unwrap();
        value.validate("wow", "cn").unwrap();
        assert_eq!(value.observed_at.canonical(), canonical);
        assert_eq!(serde_json::to_vec(&value).is_ok(), canonical.is_some());
    }
    for stamp in [
        "2026-10-14 22:00:00Z",
        "2026-10-14T22:00:00UTC",
        " 2026-10-14T22:00:00Z",
        r"2026-10-14T22:00:00\u005a",
    ] {
        let raw = format!(r#"{{"observedAt":"{stamp}"}}"#);
        assert!(
            serde_json::from_str::<Observation>(&raw).is_err(),
            "{stamp}"
        );
    }
}

#[test]
fn observation_go_duplicate_case_null_and_nested_merge_are_preserved() {
    let raw = r#"{"ProductCode":"wrong","productCode":"wow","PRODUCTCODE":null,"region":"cn","observedAt":"2026-10-14T22:00:00Z","ObservedAt":null,"versions":{"sha256":"first","bytes":-2},"verſions":{"SHA256":"last"},"VERSIONS":null,"distribution":null,"ignored":{"foo":1}}"#;
    let value: Observation = serde_json::from_str(raw).unwrap();
    value.validate("wow", "cn").unwrap();
    assert_eq!(value.versions.sha256, "last");
    assert_eq!(value.versions.bytes, -2);
    assert_eq!(value.distribution.bytes, 0);
    assert_eq!(value.distribution.sha256, "");
    for raw in ["null", "{}", r#"{"observedAt":null}"#] {
        let value: Observation = serde_json::from_str(raw).unwrap();
        assert!(value.observed_at.is_zero());
    }
    for raw in [
        r#"{"versions":{"bytes":1.0}}"#,
        r#"{"versions":{"bytes":1e0}}"#,
        r#"{"observedAt":42}"#,
        r#"{"observedAt":"bad","observedAt":"2026-10-14T22:00:00Z"}"#,
    ] {
        assert!(serde_json::from_str::<Observation>(raw).is_err(), "{raw}");
    }
}

#[test]
fn route_fallback_accepts_zero_time_and_unused_negative_invalid_version_reference() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut observed = observation(&store, "tpr/first");
    observed.observed_at = ObservationTime::default();
    observed.versions = CachedBlob {
        sha256: "not-a-digest".into(),
        bytes: -1,
    };
    store.put("remote-catalog/wow/cn", &observed, None).unwrap();
    let fetch = Fetch::default();
    let route = Route::prepare(&store, "wow", "cn", "wow", true, &fetch).unwrap();
    assert_eq!(route.current.path, "tpr/first");
    assert!(fetch.calls.borrow().is_empty());
    assert_eq!(
        store.observation("wow", "cn").unwrap_err().code,
        "records.remote_identity"
    );
}

#[test]
fn target_observation_validates_identity_time_before_consumed_blob_references() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut observed = observation(&store, "tpr/first");
    observed.versions = CachedBlob {
        sha256: "invalid".into(),
        bytes: -1,
    };
    observed.distribution.bytes = i64::MAX;
    store.put("remote-catalog/wow/cn", &observed, None).unwrap();
    let (value, _) = store.observation("wow", "cn").unwrap().unwrap();
    assert!(
        store
            .read_blob(&value.versions, catalog::MAX_BYTES)
            .is_err()
    );
    assert!(
        store
            .read_blob(&value.distribution, catalog::MAX_BYTES)
            .is_err()
    );
}

#[test]
fn raw_cached_observation_normalizes_strings_without_unescaping_time() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let raw = serde_json::value::RawValue::from_string(r#"{"ProductCode":"wow","region":"cn","observedAt":"2026-10-14T22:00:00\u005a","extra":"\ud800"}"#.into()).unwrap();
    store.put("remote-catalog/wow/cn", &raw, None).unwrap();
    assert_eq!(
        store.observation("wow", "cn").unwrap_err().code,
        "records.remote_identity"
    );
}

#[test]
fn cache_put_held_metadata_schema_is_bounded_by_independent_budget_deadline() {
    let fixture = Fixture::new();
    let guard = fixture.store().lease("metadata-schema").unwrap();
    let budget = Budget::new(
        crate::resource::Cost::defaults(),
        Instant::now() + Duration::from_millis(30),
        None,
    );
    let store = Store {
        home: &fixture.home,
        control: &fixture.control,
        budget: Some(&budget),
    };
    let started = Instant::now();
    let result = store.put("held/open", &true, None);
    let elapsed = started.elapsed();
    guard.close().unwrap();
    let error = result.unwrap_err();
    assert!(
        serde_json::to_string(&error)
            .unwrap()
            .contains("records.query_resource_budget")
    );
    assert!(elapsed < Duration::from_millis(500), "{elapsed:?}");
    assert!(fixture.store().read::<bool>("held/open").unwrap().is_none());
}

#[test]
fn cache_put_held_metadata_schema_observes_original_cancellation() {
    let fixture = Fixture::new();
    let guard = fixture.store().lease("metadata-schema").unwrap();
    let cancellation = fixture.control.tracked_cancel_handle().unwrap();
    let started = Instant::now();
    let result = std::thread::scope(|scope| {
        let trigger = scope.spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            cancellation.cancel();
        });
        let result = fixture.store().put("held/cancel", &true, None);
        trigger.join().unwrap();
        result
    });
    guard.close().unwrap();
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert!(started.elapsed() < Duration::from_millis(500));
}

#[test]
fn cache_put_held_metadata_schema_observes_independent_budget_cancellation() {
    for tracked in [false, true] {
        let fixture = Fixture::new();
        let guard = fixture.store().lease("metadata-schema").unwrap();
        let raw = Arc::new(AtomicBool::new(false));
        let other = OperationControl::default();
        let cancellation = other.tracked_cancel_handle().unwrap();
        let budget = if tracked {
            Budget::controlled(crate::resource::Cost::defaults(), other.deadline(), other)
        } else {
            Budget::new(
                crate::resource::Cost::defaults(),
                fixture.control.deadline(),
                Some(raw.clone()),
            )
        };
        let store = Store {
            home: &fixture.home,
            control: &fixture.control,
            budget: Some(&budget),
        };
        let started = Instant::now();
        let result = std::thread::scope(|scope| {
            let trigger = scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                if tracked {
                    cancellation.cancel();
                } else {
                    raw.store(true, Ordering::Release);
                }
            });
            let result = store.put("held/independent-cancel", &true, None);
            trigger.join().unwrap();
            result
        });
        guard.close().unwrap();
        assert_eq!(
            result.unwrap_err().code,
            if tracked {
                "command.cancelled"
            } else {
                "operation.cancelled"
            }
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        fixture.control.check().unwrap();
        assert!(
            fixture
                .store()
                .read::<bool>("held/independent-cancel")
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn post_close_stop_and_full_cleanup_cause_are_retained_without_retry() {
    let fixture = Fixture::new();
    let mut closes = 0;
    let cleanup = (|| {
        closes += 1;
        fixture.control.tracked_cancel_handle().unwrap().cancel();
        let mut error = fault("external.io", 9, "close failed");
        error.details = serde_json::json!({"native":123,"owner":"consumed"});
        Err(error)
    })();
    let error = fixture.store().finish(Ok(7), cleanup.clone()).unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(
        error.details["cleanup"],
        serde_json::to_value(cleanup.unwrap_err()).unwrap()
    );
    assert_eq!(error.details["control"]["code"], "command.cancelled");
    assert_eq!(
        fixture.store().finish(Ok(7), Ok(())).unwrap_err().code,
        "command.cancelled"
    );
}

#[test]
fn cached_unknown_fields_follow_go_ten_thousand_structural_depth_limit() {
    let fixture = Fixture::new();
    let store = fixture.store();
    for depth in [9999, 10000, 10001] {
        // Root object itself counts toward Go's structural depth.
        let raw = format!(
            r#"{{"productCode":"wow","region":"cn","observedAt":"2026-10-14T22:00:00Z","unknown":{}0{}}}"#,
            "[".repeat(depth - 1),
            "]".repeat(depth - 1)
        );
        let raw = serde_json::value::RawValue::from_string(raw).unwrap();
        let key = format!("remote-catalog/depth/{depth}");
        store.put(&key, &raw, None).unwrap();
        let result = store.read::<Observation>(&key);
        if depth <= 10000 {
            let (observed, _) = result.unwrap().unwrap();
            observed.validate("wow", "cn").unwrap();
        } else {
            assert_eq!(result.unwrap_err().code, "records.remote_identity");
        }
    }
}
#[test]
fn raw_admission_ignores_escaped_string_delimiters_in_cached_unknown_fields() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let value = serde_json::json!({"productCode":"wow","region":"cn","observedAt":"2026-10-14T22:00:00Z","unknown": "[{}]\\\"".repeat(10001)});
    store
        .put("remote-catalog/escaped-depth", &value, None)
        .unwrap();
    let (observed, _) = store
        .read::<Observation>("remote-catalog/escaped-depth")
        .unwrap()
        .unwrap();
    observed.validate("wow", "cn").unwrap();
}

#[test]
fn refresh_reads_only_generation_of_changed_alias_but_initial_prepare_validates_it() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let fetch = Fetch::default();
    let locator = catalog_url("cn", "wow", "cdns").unwrap();
    fetch
        .values
        .borrow_mut()
        .insert(locator.clone(), Ok(cdns("tpr/first")));
    let mut route = Route::prepare(&store, "wow", "cn", "wow", false, &fetch).unwrap();
    let key = "cdn-route/wow/cn";
    let generation = store.generation(key).unwrap();
    store
        .put(key, &serde_json::json!({"bytes":"bad"}), generation)
        .unwrap();
    assert_eq!(
        Route::prepare(&store, "wow", "cn", "wow", false, &fetch)
            .err()
            .unwrap()
            .code,
        "records.remote_identity"
    );
    assert_eq!(fetch.calls.borrow().len(), 1);
    fetch
        .values
        .borrow_mut()
        .insert(locator, Ok(cdns("tpr/refreshed")));
    assert!(route.refresh(&store, &fetch).unwrap());
    assert_eq!(route.current.path, "tpr/refreshed");
    assert!(!route.refresh(&store, &fetch).unwrap());
    assert_eq!(fetch.calls.borrow().len(), 2);
    let (reference, _) = store.read::<CachedBlob>(key).unwrap().unwrap();
    assert_eq!(
        store.read_blob(&reference, catalog::MAX_BYTES).unwrap(),
        cdns("tpr/refreshed")
    );
}

#[test]
fn bounded_blob_consumers_keep_independent_stop_and_do_not_rebill_cached_network() {
    let fixture = Fixture::new();
    let token = lycheedev_core::control::TrackedCancellation::new();
    let independent =
        OperationControl::tracked(Instant::now() + Duration::from_secs(5), token.clone());
    let budget = Budget::controlled(
        crate::resource::Cost::defaults(),
        independent.deadline(),
        independent,
    );
    let store = Store {
        home: &fixture.home,
        control: &fixture.control,
        budget: Some(&budget),
    };
    let bytes = vec![42; 65537];
    let blob = store.publish_blob(&bytes, bytes.len()).unwrap();
    assert_eq!(store.read_blob(&blob, bytes.len()).unwrap(), bytes);
    let used = budget.snapshot().unwrap();
    assert_eq!((used.network_bytes, used.network_requests), (0, 0));
    token.cancel();
    assert_eq!(
        store.read_blob(&blob, bytes.len()).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(
        store
            .publish_blob(b"must not publish", 64)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert!(
        !evidence::blob_path(&fixture.home, &storage::sha256(b"must not publish"))
            .unwrap()
            .exists()
    );
    fixture.control.check().unwrap();
}
