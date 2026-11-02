use super::*;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Fixture {
    home: PathBuf,
    control: OperationControl,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-named-target-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        crate::workspace::initialize(&home).unwrap();
        Metadata::open(&home, true).unwrap().close().unwrap();
        Self {
            home,
            control: OperationControl::uncancelable(Instant::now() + Duration::from_secs(60)),
        }
    }
    fn seed(&self, rows: impl IntoIterator<Item = (String, Vec<u8>)>) {
        // Explicit test-only historical-row injection, including corrupt rows.
        let mut db = rusqlite::Connection::open(self.home.join("state/toolkit.sqlite")).unwrap();
        let tx = db.transaction().unwrap();
        for (key, value) in rows {
            tx.execute(
                "INSERT OR REPLACE INTO documents(key,generation,value) VALUES(?1,1,?2)",
                rusqlite::params![key, value],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        db.close().unwrap();
    }
    fn config(&self, name: &str) {
        self.seed([(
            format!("target/{name}"),
            serde_json::to_vec(&config(name)).unwrap(),
        )]);
    }
    fn pin(&self, build: &str) -> PinnedSet {
        super::super::pin_selection(
            &self.home,
            SelectionSpec {
                data: Some(DataPin {
                    product: "retail".into(),
                    region: "cn".into(),
                    language: "zhCN".into(),
                    full_build: build.into(),
                    build_config: "1".repeat(32),
                    cdn_config: "2".repeat(32),
                    definition_commit: "a".repeat(40),
                }),
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn lookup(&self, key: &str) -> Result<TargetConfig> {
        lookup_controlled_with_check(&self.home, key, &self.control, || Ok(()))
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.home).unwrap();
    }
}
fn config(name: &str) -> TargetConfig {
    TargetConfig {
        schema: "lycheedev.target.v1".into(),
        name: name.into(),
        product: "retail".into(),
        region: "cn".into(),
        locale: "zhCN".into(),
        source: "remote".into(),
        ..Default::default()
    }
}
#[test]
fn current_forever_target_accepts_formal_and_rejects_retired_products() {
    let mut target = config("formal");
    target.product = "forever".into();
    for region in ["cn", "us", "eu", "kr", "tw"] {
        target.region = region.into();
        target.validate().unwrap();
    }
    for product in [
        "forever-asia-beta",
        "forever-cn-beta",
        "wow_forever",
        "wowf",
    ] {
        target.product = product.into();
        assert_eq!(
            target.validate().unwrap_err().code,
            "selection.unsupported_data_product"
        );
    }
}
#[test]
fn named_lookup_reads_all_pages_but_never_reference_history() {
    let f = Fixture::new();
    f.seed((0..1002).map(|i| {
        let name = format!("target-{i:04}");
        (
            format!("target/{name}"),
            serde_json::to_vec(&config(&name)).unwrap(),
        )
    }));
    f.seed([(
        "target-ref/target-1001/operation/bad".into(),
        b"not-json".to_vec(),
    )]);
    assert_eq!(f.lookup("target-1001").unwrap().name, "target-1001");
    assert_eq!(f.lookup("TARGET-1001").unwrap().name, "target-1001");
    assert_eq!(f.lookup("target-100").unwrap_err().exit_code, 2);
    // Exact match cannot short-circuit validation of a later page.
    f.seed([("target/zz-invalid".into(), br#"{"name":"bad"}"#.to_vec())]);
    assert_eq!(
        f.lookup("target-0000").unwrap_err().code,
        "selection.unsupported_data_product"
    );
    f.cleanup();
}
#[test]
fn named_case_collision_exact_fold_and_prefix_match_fixed_order() {
    let f = Fixture::new();
    for name in ["Retail", "retail", "Retail-eu", "Kilo", "sample"] {
        f.config(name);
    }
    assert_eq!(f.lookup("Retail").unwrap().name, "Retail");
    assert_eq!(f.lookup("retail").unwrap().name, "retail");
    assert_eq!(
        f.lookup("RETAIL").unwrap_err().code,
        "selection.target_ambiguous"
    );
    assert_eq!(f.lookup("KILO").unwrap().name, "Kilo");
    assert_eq!(f.lookup("ſAMPLE").unwrap().name, "sample");
    assert_eq!(f.lookup("retail-e").unwrap().name, "Retail-eu");
    assert_eq!(f.lookup("").unwrap_err().exit_code, 3);
    f.cleanup();
}
#[test]
fn named_wire_retains_duplicate_null_unknown_and_depth_contract() {
    let f = Fixture::new();
    let raw=br#"{"schema":"lycheedev.target.v1","name":"wrong","NAME":"wire","Name":null,"product":"retail","region":"cn","locale":"zhCN","source":"remote","unknown":{"a":1}}"#;
    f.seed([("target/wire".into(), raw.to_vec())]);
    assert_eq!(f.lookup("wire").unwrap().name, "wire");
    for depth in [9999usize, 10000] {
        let mut raw = serde_json::to_vec(&config("depth")).unwrap();
        raw.pop();
        raw.extend_from_slice(b",\"unknown\":");
        raw.extend(std::iter::repeat_n(b'[', depth));
        raw.extend(std::iter::repeat_n(b']', depth));
        raw.push(b'}');
        f.seed([("target/depth".into(), raw)]);
        let result = f.lookup("wire");
        if depth == 9999 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err().code, "selection.target_format");
        }
    }
    f.cleanup();
}
#[test]
fn named_resolution_does_not_recheck_deleted_configuration_or_existing_ref() {
    let f = Fixture::new();
    f.config("moving");
    let pin = f.pin("12.1.5.70000");
    let key = format!("target-ref/moving/pin/{}", pin.id);
    f.seed([(
        key.clone(),
        br#"{"unrelated":"immutable-existing"}"#.to_vec(),
    )]);
    let resolved = resolve_controlled_with_check(
        &f.home,
        "moving",
        "",
        &f.control,
        || Ok(()),
        |_, _| {
            let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
            db.execute("DELETE FROM documents WHERE key='target/moving'", [])
                .unwrap();
            db.close().unwrap();
            Ok(pin.clone())
        },
    )
    .unwrap();
    assert_eq!(resolved.pin.id, pin.id);
    let metadata = Metadata::open(&f.home, false).unwrap();
    assert_eq!(
        metadata.read::<Value>(&key).unwrap(),
        json!({"unrelated":"immutable-existing"})
    );
    assert_eq!(metadata.generation(&key).unwrap(), Some(1));
    metadata.close().unwrap();
    f.cleanup();
}
#[test]
fn named_relative_installation_fails_before_preparer_and_override_is_explicit() {
    let f = Fixture::new();
    let mut local = config("old");
    local.source = "installation".into();
    local.installation = "relative/client".into();
    f.seed([("target/old".into(), serde_json::to_vec(&local).unwrap())]);
    let error = resolve_controlled_with_check(
        &f.home,
        "old",
        "",
        &f.control,
        || Ok(()),
        |_, _| panic!("old relative target cannot prepare"),
    )
    .unwrap_err();
    assert_eq!(error.code, "selection.target_format");
    assert_eq!(error.exit_code, 2);
    let mut remote = config("new");
    remote.definitions = "config-reference".into();
    f.seed([("target/new".into(), serde_json::to_vec(&remote).unwrap())]);
    let pin = f.pin("12.1.5.70000");
    for (input, expected) in [("", "config-reference"), ("override", "override")] {
        resolve_controlled_with_check(
            &f.home,
            "new",
            input,
            &f.control,
            || Ok(()),
            |_, definitions| {
                assert_eq!(definitions, expected);
                Ok(pin.clone())
            },
        )
        .unwrap();
    }
    f.cleanup();
}
#[test]
fn named_prepare_errors_and_empty_identity_do_not_write_reference() {
    let f = Fixture::new();
    f.config("named");
    let mut original = ToolkitError::new("records.remote_unavailable_offline", "offline", 3)
        .with_details(json!({"cause":"kept"}));
    let error = resolve_controlled_with_check(
        &f.home,
        "named",
        "",
        &f.control,
        || Ok(()),
        |_, _| Err(original.clone()),
    )
    .unwrap_err();
    original.message = "selection.target_prepare_failed: offline".into();
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    let mut pin = f.pin("12.1.5.70000");
    pin.id.clear();
    assert_eq!(
        resolve_controlled_with_check(&f.home, "named", "", &f.control, || Ok(()), |_, _| Ok(pin))
            .unwrap_err()
            .code,
        "command.failed"
    );
    let metadata = Metadata::open(&f.home, false).unwrap();
    assert!(
        metadata
            .list::<Value>("target-ref/", 10)
            .unwrap()
            .is_empty()
    );
    metadata.close().unwrap();
    f.cleanup();
}
#[test]
fn named_raw_page_observes_independent_stop_while_sqlite_is_busy() {
    let f = Fixture::new();
    f.config("busy");
    let metadata = Metadata::open_controlled(&f.home, false, &f.control).unwrap();
    let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let deadline = Instant::now() + Duration::from_millis(50);
    let mut checks = 0;
    let error = metadata
        .visit_raw_page_controlled_with_check(
            "target/",
            "target/",
            1000,
            &f.control,
            || {
                checks += 1;
                if Instant::now() >= deadline {
                    Err(ToolkitError::new(
                        "records.query_resource_budget",
                        "independent budget",
                        3,
                    ))
                } else {
                    Ok(())
                }
            },
            |_, _, _| Ok(()),
        )
        .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    assert!(checks > 2);
    assert!(f.control.check().is_ok());
    db.execute_batch("ROLLBACK").unwrap();
    db.close().unwrap();
    metadata.close().unwrap();
    assert!(f.control.ordered_child(f.control.deadline()).is_ok());
    f.cleanup();
}
#[test]
fn named_original_cancel_precedes_lookup_and_never_exposes_raw_token() {
    let f = Fixture::new();
    f.config("named");
    f.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(f.lookup("named").unwrap_err().code, "command.cancelled");
    assert!(f.control.stop_cause().is_ok());
    f.cleanup();
}
#[test]
fn named_raw_page_checks_independent_stop_after_actual_row_without_replaying_visitor() {
    let f = Fixture::new();
    f.config("a");
    f.config("b");
    let metadata = Metadata::open_controlled(&f.home, false, &f.control).unwrap();
    let delivered = std::cell::Cell::new(0);
    let error = metadata
        .visit_raw_page_controlled_with_check(
            "target/",
            "target/",
            1000,
            &f.control,
            || {
                if delivered.get() > 0 {
                    Err(ToolkitError::new(
                        "records.query_resource_budget",
                        "independent stop after row",
                        3,
                    ))
                } else {
                    Ok(())
                }
            },
            |_, _, _| {
                delivered.set(delivered.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(delivered.get(), 1);
    metadata.close().unwrap();
    assert!(f.control.check().is_ok());
    assert!(f.control.ordered_child(f.control.deadline()).is_ok());
    f.cleanup();
}
#[test]
fn named_expected_conflict_with_cleanup_is_not_silently_accepted() {
    let conflict = ToolkitError::new("vault.generation_conflict", "conflict", 3);
    assert!(pure(&conflict, "vault.generation_conflict"));
    let compound = crate::metadata::owned_write::finish_owned::<()>(
        Err(conflict),
        Err(ToolkitError::new("test.close", "close", 5)),
    )
    .unwrap_err();
    assert!(!pure(&compound, "vault.generation_conflict"));
}
