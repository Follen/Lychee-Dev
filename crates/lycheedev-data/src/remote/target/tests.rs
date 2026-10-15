use super::*;
use md5::{Digest, Md5};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
struct Fixture {
    home: PathBuf,
    control: OperationControl,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/remote-target-fixtures")
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
    values: BTreeMap<String, Vec<u8>>,
    calls: RefCell<Vec<String>>,
    cancel_on: Option<usize>,
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
            .get(locator)
            .cloned()
            .ok_or_else(|| fault("records.remote_http", 5, "fixture unavailable"))?;
        if raw.len() > maximum {
            return Err(super::super::limit());
        }
        if self.cancel_on == Some(self.calls.borrow().len()) {
            control.tracked_cancel_handle()?.cancel();
        }
        Ok(raw)
    }
}
#[derive(Default)]
struct Defs {
    calls: RefCell<Vec<(String, bool, Instant)>>,
}
impl DefinitionResolver for Defs {
    fn resolve(
        &self,
        _: &Path,
        reference: &str,
        offline: bool,
        control: &OperationControl,
        _: Option<&Budget>,
    ) -> Result<String> {
        control.check()?;
        self.calls
            .borrow_mut()
            .push((reference.into(), offline, control.deadline()));
        Ok(if reference.is_empty() {
            "a".repeat(40)
        } else {
            reference.into()
        })
    }
}
fn request() -> Request {
    Request {
        product: "retail".into(),
        region: "cn".into(),
        locale: "zhCN".into(),
        ..Default::default()
    }
}
fn inputs(product: &str, build: &str) -> Fetch {
    let (_, slot) = super::product(product).unwrap();
    let build_raw = format!(
        "root = {}\nencoding = {} {}\nencoding-size = 123 456\nbuild-uid = {slot}\n",
        "1".repeat(32),
        "2".repeat(32),
        "3".repeat(32)
    )
    .into_bytes();
    let cdn = b"archives = 44444444444444444444444444444444\n".to_vec();
    let key = |raw: &[u8]| crate::casc::hex(&Md5::digest(raw));
    let (b, c) = (key(&build_raw), key(&cdn));
    let route = catalog::Distribution {
        path: "tpr/wow".into(),
        hosts: vec!["cdn.example.test".into()],
    };
    let mut fetch = Fetch::default();
    let build_id = build
        .rsplit('.')
        .next()
        .expect("fixture full build has components");
    fetch.values.insert(
        cache::catalog_url("cn", slot, "versions").unwrap(),
        format!("Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:4|VersionsName!STRING:0\ncn|{b}|{c}|{build_id}|{build}\n").into_bytes(),
    );
    fetch.values.insert(
        cache::catalog_url("cn", slot, "cdns").unwrap(),
        b"Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|tpr/wow|cdn.example.test\n".to_vec(),
    );
    fetch
        .values
        .insert(route.config_url("cdn.example.test", &b).unwrap(), build_raw);
    fetch
        .values
        .insert(route.config_url("cdn.example.test", &c).unwrap(), cdn);
    fetch
}
#[test]
fn fixed_product_profiles_and_legacy_alias_have_distinct_cache_and_slot_identity() {
    for (name, code, slot) in [
        ("retail", "wow", "wow"),
        ("classic", "wow_classic", "wow_classic"),
        ("classic-era", "wow_classic_era", "wow_classic_era"),
        ("titan", "wow_classic_titan", "wow_classic_titan"),
        ("forever-asia-beta", "wow_classic_beta", "wow_classic_beta"),
        ("forever-cn-beta", "wow_cn_beta", "wow_cn_beta"),
        ("anniversary", "wow_anniversary", "wow_anniversary"),
        ("forever", "wow_forever", "wow_classic_beta"),
    ] {
        assert_eq!(super::product(name).unwrap(), (code, slot));
        Request {
            product: name.into(),
            ..request()
        }
        .validate()
        .unwrap();
    }
    for name in ["wow", "retail-beta", "forever-cn", "Retail", ""] {
        assert!(super::product(name).is_err());
    }
    for name in ["forever", "forever-asia-beta", "forever-cn-beta"] {
        assert!(release_product(name, "1.60.1.9").is_ok());
        assert!(release_product(name, "1.61.0.9").is_err());
    }
    assert!(release_product("retail", "99.1.0.1").is_ok());
}
#[test]
fn admission_precedes_io_and_preserves_language_region_build_reference_order() {
    let mut q = request();
    q.locale = "bad".into();
    q.region = "bad".into();
    assert_eq!(
        q.validate().unwrap_err().code,
        "selection.unsupported_data_language"
    );
    q.locale = "enUS".into();
    assert_eq!(
        q.validate().unwrap_err().code,
        "selection.unsupported_data_region"
    );
    q.region = "us".into();
    q.full_build = "+1.2.3.4".into();
    assert_eq!(q.validate().unwrap_err().code, "records.invalid_metadata");
    q.full_build.clear();
    q.definitions = "master".into();
    assert_eq!(
        q.validate().unwrap_err().code,
        "records.definition_identity"
    );
}
#[test]
fn two_minute_child_preserves_ordered_and_legacy_cancellation_without_promotion() {
    let original = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    let child = scoped_control(&original, None).unwrap();
    assert_eq!(child.deadline(), original.deadline());
    original.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(child.check().unwrap_err().code, "command.cancelled");
    let raw = Arc::new(AtomicBool::new(false));
    let legacy = OperationControl::new(Instant::now() + Duration::from_secs(300), raw.clone());
    let before = Instant::now();
    let child = scoped_control(&legacy, None).unwrap();
    assert!(child.deadline() <= Instant::now() + Duration::from_secs(120));
    assert!(child.deadline() >= before + Duration::from_secs(120));
    assert_eq!(
        child.stop_cause().unwrap_err().code,
        "control.cancellation_order_unavailable"
    );
    raw.store(true, Ordering::Release);
    assert_eq!(child.check().unwrap_err().code, "command.cancelled");
}
#[test]
fn online_then_offline_target_retains_exact_pair_pin_and_capture_provenance() {
    let fixture = Fixture::new();
    let fetch = inputs("retail", "12.1.5.70000");
    let defs = Defs::default();
    let q = request();
    let online = resolve_with(&fixture.home, &q, &fixture.control, None, &fetch, &defs).unwrap();
    assert_eq!(fetch.calls.borrow().len(), 4);
    assert_eq!(online.pin.data.as_ref().unwrap().full_build, "12.1.5.70000");
    let offline = resolve_with(
        &fixture.home,
        &Request { offline: true, ..q },
        &fixture.control,
        None,
        &Fetch::default(),
        &defs,
    )
    .unwrap();
    assert_eq!(online.pin.id, offline.pin.id);
    assert_eq!(online.observed_at, offline.observed_at);
    assert!(offline.offline);
    assert_ne!(online.capture.id, offline.capture.id);
    assert_eq!(offline.capture.provenance.kind, "remote-target");
    assert_eq!(offline.capture.provenance.snapshot, offline.pin.id);
    let bytes = fixture
        .store()
        .read_blob(&offline.capture.blob, 65536)
        .unwrap();
    let capture: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(capture["offline"], true);
    assert_eq!(capture["configurations"].as_array().unwrap().len(), 2);
    assert!(
        capture["versionURL"]
            .as_str()
            .unwrap()
            .ends_with("/wow/versions")
    );
}
#[test]
fn missing_or_corrupt_observation_never_falls_back_to_network_offline() {
    let fixture = Fixture::new();
    let fetch = inputs("retail", "12.1.5.70000");
    assert_eq!(
        resolve_with(
            &fixture.home,
            &Request {
                offline: true,
                ..request()
            },
            &fixture.control,
            None,
            &fetch,
            &Defs::default()
        )
        .unwrap_err()
        .code,
        "records.remote_unavailable_offline"
    );
    assert!(fetch.calls.borrow().is_empty());
    fixture
        .store()
        .put(
            "remote-catalog/wow/cn",
            &serde_json::json!({"productCode":"wrong"}),
            None,
        )
        .unwrap();
    assert_eq!(
        resolve_with(
            &fixture.home,
            &request(),
            &fixture.control,
            None,
            &fetch,
            &Defs::default()
        )
        .unwrap_err()
        .code,
        "records.remote_identity"
    );
    assert!(fetch.calls.borrow().is_empty());
}
#[test]
fn interrupted_pair_fetch_cannot_publish_half_an_observation_or_pin() {
    let fixture = Fixture::new();
    let mut fetch = inputs("retail", "12.1.5.70000");
    fetch.cancel_on = Some(2);
    let defs = Defs::default();
    let error = resolve_with(
        &fixture.home,
        &request(),
        &fixture.control,
        None,
        &fetch,
        &defs,
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(fetch.calls.borrow().len(), 2);
    assert!(defs.calls.borrow().is_empty());
    assert!(fixture.store().observation("wow", "cn").unwrap().is_none());
}
#[test]
fn reused_slot_wrong_release_fails_before_configuration_or_definition_fetch() {
    let fixture = Fixture::new();
    let fetch = inputs("forever-cn-beta", "5.5.4.70000");
    let defs = Defs::default();
    let error = resolve_with(
        &fixture.home,
        &Request {
            product: "forever-cn-beta".into(),
            ..request()
        },
        &fixture.control,
        None,
        &fetch,
        &defs,
    )
    .unwrap_err();
    assert_eq!(error.code, "selection.data_product_build_mismatch");
    assert_eq!(fetch.calls.borrow().len(), 2);
    assert!(defs.calls.borrow().is_empty());
}
#[test]
fn parent_data_inherits_exact_build_and_definition_and_product_mismatch_stops_early() {
    let fixture = Fixture::new();
    let fetch = inputs("retail", "12.1.5.70000");
    let defs = Defs::default();
    let original = resolve_with(
        &fixture.home,
        &request(),
        &fixture.control,
        None,
        &fetch,
        &defs,
    )
    .unwrap();
    let derived = resolve_with(
        &fixture.home,
        &Request {
            parent: original.pin.id.clone(),
            offline: true,
            ..request()
        },
        &fixture.control,
        None,
        &Fetch::default(),
        &defs,
    )
    .unwrap();
    assert_eq!(derived.pin.data, original.pin.data);
    assert_eq!(defs.calls.borrow().last().unwrap().0, "a".repeat(40));
    let other = Fetch::default();
    let error = resolve_with(
        &fixture.home,
        &Request {
            parent: original.pin.id,
            product: "classic".into(),
            ..request()
        },
        &fixture.control,
        None,
        &other,
        &defs,
    )
    .unwrap_err();
    assert_eq!(error.code, "selection.product_mismatch");
    assert!(other.calls.borrow().is_empty());
}

#[test]
fn online_refresh_does_not_consume_old_blob_refs_or_marshal_old_time() {
    let fixture = Fixture::new();
    fixture
        .store()
        .put(
            "remote-catalog/wow/cn",
            &serde_json::json!({
                "productCode":"wow", "region":"cn", "observedAt":"2026-10-14T22:00:00+24:00",
                "versions":{"sha256":"invalid", "bytes":-1}, "distribution":null
            }),
            None,
        )
        .unwrap();
    let fetch = inputs("retail", "12.1.5.70000");
    let result = resolve_with(
        &fixture.home,
        &request(),
        &fixture.control,
        None,
        &fetch,
        &Defs::default(),
    )
    .unwrap();
    assert_eq!(fetch.calls.borrow().len(), 4);
    assert!(result.observed_at.ends_with('Z'));
    let (observation, _) = fixture.store().observation("wow", "cn").unwrap().unwrap();
    assert!(observation.versions.bytes > 0);
}

#[test]
fn offline_unmarshalable_time_fails_at_capture_after_pin_publication() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let fetch = inputs("retail", "12.1.5.70000");
    let versions = &fetch.values[&cache::catalog_url("cn", "wow", "versions").unwrap()];
    let cdns = &fetch.values[&cache::catalog_url("cn", "wow", "cdns").unwrap()];
    let release = catalog::release(versions, "cn", "").unwrap();
    let route = catalog::distribution(cdns, "cn").unwrap();
    store
        .configuration(&route, &release.build_config, false, &fetch)
        .unwrap();
    store
        .configuration(&route, &release.cdn_config, false, &fetch)
        .unwrap();
    let versions_ref = store.publish_blob(versions, catalog::MAX_BYTES).unwrap();
    let distribution_ref = store.publish_blob(cdns, catalog::MAX_BYTES).unwrap();
    store
        .put(
            "remote-catalog/wow/cn",
            &serde_json::json!({
                "productCode":"wow", "region":"cn", "observedAt":"2026-10-14T22:00:00+24:00",
                "versions":versions_ref, "distribution":distribution_ref
            }),
            None,
        )
        .unwrap();
    let none = Fetch::default();
    let defs = Defs::default();
    assert!(
        resolve_with(
            &fixture.home,
            &Request {
                offline: true,
                ..request()
            },
            &fixture.control,
            None,
            &none,
            &defs
        )
        .is_err()
    );
    assert!(none.calls.borrow().is_empty());
    assert_eq!(defs.calls.borrow().len(), 1);
    let metadata =
        lycheedev_core::metadata::Metadata::open_controlled(&fixture.home, false, &fixture.control)
            .unwrap();
    let pins = metadata.list::<PinnedSet>("pin/", 10).unwrap();
    let captures = metadata.list::<serde_json::Value>("capture/", 10).unwrap();
    metadata.close().unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].data.as_ref().unwrap().full_build, "12.1.5.70000");
    assert!(captures.is_empty());
}

#[test]
fn family_wide_blizzard_source_parent_supports_exact_regional_data_track() {
    let fixture = Fixture::new();
    let source = selection::SourcePin {
        repository: "wow-ui-source".into(),
        product: "forever".into(),
        requested_ref: "refs/heads/forever".into(),
        exact_commit: "b".repeat(40),
        parser_revision: "fixture-parser".into(),
    };
    let parent = selection::pin_selection_controlled(
        &fixture.home,
        SelectionSpec {
            source: Some(source.clone()),
            ..Default::default()
        },
        &fixture.control,
    )
    .unwrap();
    let result = resolve_with(
        &fixture.home,
        &Request {
            parent: parent.id,
            product: "forever-cn-beta".into(),
            ..request()
        },
        &fixture.control,
        None,
        &inputs("forever-cn-beta", "1.60.1.70000"),
        &Defs::default(),
    )
    .unwrap();
    assert_eq!(result.pin.source.as_ref(), Some(&source));
    assert_eq!(result.pin.data.as_ref().unwrap().product, "forever-cn-beta");
}

#[test]
fn target_two_minute_cap_does_not_reclassify_independent_budget_deadline() {
    let control = OperationControl::default();
    let budget = Budget::new(
        crate::resource::Cost::defaults(),
        Instant::now() + Duration::from_millis(30),
        None,
    );
    let before = Instant::now();
    let child = scoped_control(&control, Some(&budget)).unwrap();
    assert!(child.deadline() >= before + Duration::from_secs(120));
    assert!(child.deadline() <= Instant::now() + Duration::from_secs(120));
    assert!(child.deadline() < control.deadline());
    std::thread::sleep(budget.deadline.saturating_duration_since(Instant::now()));
    child.check().unwrap();
    control.check().unwrap();
    assert!(child.stop_cause().unwrap().is_none());
    let error = budget.check().unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
}

#[test]
fn independent_budget_stop_after_definitions_prevents_pin_and_capture_publication() {
    struct StopDefinitions(lycheedev_core::control::TrackedCancellation);
    impl DefinitionResolver for StopDefinitions {
        fn resolve(
            &self,
            _: &Path,
            _: &str,
            _: bool,
            _: &OperationControl,
            _: Option<&Budget>,
        ) -> Result<String> {
            self.0.cancel();
            Ok("a".repeat(40))
        }
    }
    let fixture = Fixture::new();
    let token = lycheedev_core::control::TrackedCancellation::new();
    let independent =
        OperationControl::tracked(Instant::now() + Duration::from_secs(5), token.clone());
    let budget = Budget::controlled(
        crate::resource::Cost::defaults(),
        independent.deadline(),
        independent,
    );
    let fetch = inputs("retail", "12.1.5.70000");
    let error = resolve_with(
        &fixture.home,
        &request(),
        &fixture.control,
        Some(&budget),
        &fetch,
        &StopDefinitions(token),
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    fixture.control.check().unwrap();
    assert_eq!(fetch.calls.borrow().len(), 4);
    let metadata =
        lycheedev_core::metadata::Metadata::open_controlled(&fixture.home, false, &fixture.control)
            .unwrap();
    assert!(metadata.list::<PinnedSet>("pin/", 10).unwrap().is_empty());
    assert!(
        metadata
            .list::<serde_json::Value>("capture/", 10)
            .unwrap()
            .is_empty()
    );
    metadata.close().unwrap();
}

#[test]
fn target_fixture_has_required_build_id_and_rejects_missing_or_mismatched_id() {
    let fetch = inputs("retail", "12.1.5.70000");
    let raw = &fetch.values[&cache::catalog_url("cn", "wow", "versions").unwrap()];
    assert_eq!(
        catalog::release(raw, "cn", "").unwrap().full_build,
        "12.1.5.70000"
    );
    let text = std::str::from_utf8(raw).unwrap();
    let missing = text.replace("|BuildId!DEC:4", "").replace("|70000|", "|");
    assert_eq!(
        catalog::release(missing.as_bytes(), "cn", "")
            .unwrap_err()
            .code,
        "records.invalid_metadata"
    );
    let mismatch = text.replace("|70000|", "|70001|");
    assert_eq!(
        catalog::release(mismatch.as_bytes(), "cn", "")
            .unwrap_err()
            .code,
        "records.invalid_metadata"
    );
}

#[test]
fn target_pin_schema_wait_keeps_short_budget_identity_before_publication() {
    struct HoldPinSchema(RefCell<Option<storage::ExclusiveLock>>);
    impl DefinitionResolver for HoldPinSchema {
        fn resolve(
            &self,
            home: &Path,
            _: &str,
            _: bool,
            control: &OperationControl,
            budget: Option<&Budget>,
        ) -> Result<String> {
            control.check()?;
            budget.unwrap().check()?;
            *self.0.borrow_mut() = Some(storage::ExclusiveLock::resource(
                &home.join("locks"),
                "metadata-schema",
            )?);
            Ok("a".repeat(40))
        }
    }
    let fixture = Fixture::new();
    let fetch = inputs("retail", "12.1.5.70000");
    let definitions = HoldPinSchema(RefCell::new(None));
    let budget = Budget::new(
        crate::resource::Cost::defaults(),
        Instant::now() + Duration::from_millis(500),
        None,
    );
    let started = Instant::now();
    let error = resolve_with(
        &fixture.home,
        &request(),
        &fixture.control,
        Some(&budget),
        &fetch,
        &definitions,
    )
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    fixture.control.check().unwrap();
    assert_eq!(fetch.calls.borrow().len(), 4);
    definitions
        .0
        .borrow_mut()
        .take()
        .expect("definitions reached before budget stopped")
        .close()
        .unwrap();
    let metadata =
        lycheedev_core::metadata::Metadata::open_controlled(&fixture.home, false, &fixture.control)
            .unwrap();
    assert!(metadata.list::<PinnedSet>("pin/", 10).unwrap().is_empty());
    assert!(
        metadata
            .list::<serde_json::Value>("capture/", 10)
            .unwrap()
            .is_empty()
    );
    metadata.close().unwrap();
    fixture
        .store()
        .lease("metadata-schema")
        .unwrap()
        .close()
        .unwrap();
}
