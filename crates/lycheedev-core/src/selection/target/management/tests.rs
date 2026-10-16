use super::*;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    home: PathBuf,
    control: OperationControl,
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-target-management-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        crate::workspace::initialize(&home).unwrap();
        Self {
            home,
            control: OperationControl::uncancelable(Instant::now() + Duration::from_secs(60)),
        }
    }
    fn inv(&self, command: &str, name: Option<&str>) -> Invocation {
        Invocation {
            command: command.into(),
            positional: name.map(str::to_owned),
            flags: BTreeMap::new(),
            home: self.home.clone(),
            cwd: self.home.clone(),
            control: self.control.clone(),
        }
    }
    fn add(&self, name: &str) -> Invocation {
        let mut inv = self.inv("target add", Some(name));
        for (key, value) in [
            ("--product", "retail"),
            ("--region", "us"),
            ("--locale", "enUS"),
            ("--remote", "true"),
        ] {
            inv.flags.insert(key.into(), value.into());
        }
        inv
    }
    fn run(&self, command: &str, name: Option<&str>) -> Result<Outcome> {
        crate::workspace::dispatch(&self.inv(command, name))
    }
    fn seed(&self, rows: impl IntoIterator<Item = (String, Value)>) {
        Metadata::open(&self.home, true).unwrap().close().unwrap();
        let mut db = rusqlite::Connection::open(self.home.join("state/toolkit.sqlite")).unwrap();
        let tx = db.transaction().unwrap();
        for (key, value) in rows {
            tx.execute(
                "INSERT OR REPLACE INTO documents(key,generation,value) VALUES(?1,1,?2)",
                rusqlite::params![key, serde_json::to_vec(&value).unwrap()],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        db.close().unwrap();
    }
    fn config(&self, name: &str) -> Value {
        json!({"schema":"lycheedev.target.v1","name":name,"product":"retail","region":"us","locale":"enUS","source":"remote"})
    }
    fn reference(&self, name: &str, kind: &str, id: &str) -> Value {
        json!({"schema":"lycheedev.target-ref.v1","name":name,"kind":kind,"id":id,"createdAt":"2026-10-15T00:00:00Z"})
    }
    fn sql(&self, sql: &str) {
        let db = rusqlite::Connection::open(self.home.join("state/toolkit.sqlite")).unwrap();
        db.execute_batch(sql).unwrap();
        db.close().unwrap();
    }
    fn missing(&self, key: &str) -> bool {
        let m = Metadata::open(&self.home, false).unwrap();
        let result = m.generation(key).unwrap().is_none();
        m.close().unwrap();
        result
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.home).unwrap();
    }
}
#[test]
fn empty_list_does_not_create_database_and_invalid_add_does_not_touch_workspace() {
    let f = Fixture::new();
    assert_eq!(
        f.run("target list", None).unwrap().data,
        json!({"targets":[]})
    );
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    for name in ["---".to_owned(), "a".repeat(65)] {
        let error = crate::workspace::dispatch(&f.add(&name)).unwrap_err();
        assert_eq!(error.code, "selection.target_format");
        assert_eq!(error.exit_code, 2);
    }
    let mut bad = f.add("valid");
    bad.flags.insert("--locale".into(), "invalid".into());
    assert_eq!(
        crate::workspace::dispatch(&bad).unwrap_err().code,
        "command.invalid_arguments"
    );
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    f.cleanup();
}
#[test]
fn target_list_pages_beyond_old_ten_thousand_cap_and_sorts_value_names() {
    let f = Fixture::new();
    f.seed((0..10003).map(|i| {
        (
            format!("target/{i:05}"),
            f.config(&format!("name-{:05}", 10002 - i)),
        )
    }));
    f.seed([
        ("target-ref/name-00000/bad".into(), json!({"wrong":true})),
        ("unrelated/bad".into(), json!({"wrong":true})),
    ]);
    let out = f.run("target list", None).unwrap();
    let values = out.data["targets"].as_array().unwrap();
    assert_eq!(values.len(), 10003);
    assert_eq!(values[0]["name"], "name-00000");
    assert_eq!(values[10002]["name"], "name-10002");
    f.cleanup();
}
#[test]
fn add_reads_only_exact_generation_and_replace_recreate_preserve_monotonic_cas() {
    let f = Fixture::new();
    f.seed([("target/unrelated".into(), json!({"malformed":true}))]);
    let mut add = f.add("name");
    crate::workspace::dispatch(&add).unwrap();
    let error = crate::workspace::dispatch(&add).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("selection.target_exists", 3)
    );
    add.flags.insert("--replace".into(), "true".into());
    crate::workspace::dispatch(&add).unwrap();
    // Old unrelated malformed config must block show/list, but never add.
    assert_eq!(
        f.run("target show", Some("name")).unwrap_err().code,
        "selection.target_format"
    );
    f.sql("DELETE FROM documents WHERE key='target/unrelated'");
    assert_eq!(
        f.run("target remove", Some("name")).unwrap().data,
        json!({"name":"name","removedRefCount":0})
    );
    add.flags.remove("--replace");
    crate::workspace::dispatch(&add).unwrap();
    write(&f.home, &f.control, |w| {
        assert_eq!(w.document("target/name")?.generation, 3);
        assert_eq!(
            w.delete(&[("target/name".into(), 2)]).unwrap_err().code,
            "vault.generation_conflict"
        );
        assert_eq!(w.document("target/name")?.generation, 3);
        Ok(())
    })
    .unwrap();
    f.cleanup();
}
#[test]
fn removed_target_reference_recreated_by_resolution_rejects_stale_generation() {
    let f = Fixture::new();
    crate::workspace::dispatch(&f.add("recreated")).unwrap();
    let pin = crate::selection::pin_selection(
        &f.home,
        SelectionSpec {
            data: Some(DataPin {
                product: "retail".into(),
                region: "us".into(),
                language: "enUS".into(),
                full_build: "12.1.5.70000".into(),
                build_config: "1".repeat(32),
                cdn_config: "2".repeat(32),
                definition_commit: "a".repeat(40),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let key = format!("target-ref/recreated/pin/{}", pin.id);
    for generation in [1, 2] {
        let resolved = resolve_controlled_with_check(
            &f.home,
            "recreated",
            "",
            &f.control,
            || Ok(()),
            |_, _| Ok(pin.clone()),
        )
        .unwrap();
        assert_eq!(resolved.pin.id, pin.id);
        write(&f.home, &f.control, |writer| {
            assert_eq!(writer.document(&key)?.generation, generation);
            if generation == 2 {
                assert_eq!(
                    writer.delete(&[(key.clone(), 1)]).unwrap_err().code,
                    "vault.generation_conflict"
                );
                assert_eq!(writer.document(&key)?.generation, 2);
            }
            Ok(())
        })
        .unwrap();
        if generation == 1 {
            let removed = f.run("target remove", Some("recreated")).unwrap();
            assert_eq!(removed.data["removedRefCount"], 1);
            assert!(f.missing(&key));
            assert!(!f.missing(&format!("pin/{}", pin.id)));
            crate::workspace::dispatch(&f.add("recreated")).unwrap();
        }
    }
    f.cleanup();
}
#[test]
fn relative_installation_is_fixed_at_save_without_requiring_directory_to_exist() {
    let f = Fixture::new();
    let mut inv = f.add("local");
    inv.flags.remove("--remote");
    inv.flags
        .insert("--installation".into(), "game/../other".into());
    let out = crate::workspace::dispatch(&inv).unwrap();
    assert_eq!(
        out.data["installation"],
        f.home.join("other").to_string_lossy().as_ref()
    );
    assert!(!f.home.join("other").exists());
    f.cleanup();
}
#[test]
fn exact_case_collision_show_and_remove_keep_other_target_and_no_empty_optional_fields() {
    let f = Fixture::new();
    for name in ["Foo", "foo"] {
        crate::workspace::dispatch(&f.add(name)).unwrap();
    }
    assert_eq!(
        f.run("target show", Some("Foo")).unwrap().data["config"]["name"],
        "Foo"
    );
    assert_eq!(
        f.run("target show", Some("FOO")).unwrap_err().code,
        "selection.target_ambiguous"
    );
    let shown = f.run("target show", Some("foo")).unwrap();
    assert_eq!(shown.data["references"], json!([]));
    assert!(shown.data.get("activeOperations").is_none());
    assert!(shown.data.get("resolvedPins").is_none());
    f.run("target remove", Some("foo")).unwrap();
    assert!(f.run("target show", Some("Foo")).is_ok());
    f.cleanup();
}
#[test]
fn paginated_reference_output_and_removal_match_go_unknown_kind_and_kept_pin_contract() {
    let f = Fixture::new();
    crate::workspace::dispatch(&f.add("many")).unwrap();
    f.seed((0..1002).map(|i| {
        (
            format!("target-ref/many/pin/PIN-{i:04}"),
            f.reference("many", "pin", &format!("PIN-{i:04}")),
        )
    }));
    f.seed([
        (
            "target-ref/many/legacy/odd".into(),
            f.reference("many", "legacy", "odd"),
        ),
        ("pin/PIN-0000".into(), json!({"immutable":"remains"})),
    ]);
    let show = f.run("target show", Some("many")).unwrap();
    assert_eq!(show.data["references"].as_array().unwrap().len(), 1003);
    assert_eq!(show.data["references"][0]["kind"], "legacy");
    assert_eq!(show.data["resolvedPins"].as_array().unwrap().len(), 1002);
    let report = f.run("target remove", Some("many")).unwrap();
    assert_eq!(report.data["removedRefCount"], 1003);
    assert_eq!(report.data["removedPins"][0], "odd");
    assert!(report.data.get("keptPins").is_none());
    assert!(!f.missing("pin/PIN-0000"));
    assert!(f.missing("target-ref/many/pin/PIN-1001"));
    f.cleanup();
}
#[test]
fn active_operation_blocks_removal_before_configuration_or_reference_changes() {
    let f = Fixture::new();
    crate::workspace::dispatch(&f.add("active")).unwrap();
    f.seed([
        (
            "target-ref/active/operation/OP-b".into(),
            f.reference("active", "operation", "OP-b"),
        ),
        (
            "target-ref/active/operation/OP-a".into(),
            f.reference("active", "operation", "OP-a"),
        ),
    ]);
    let show = f.run("target show", Some("active")).unwrap();
    assert_eq!(show.data["activeOperations"], json!(["OP-a", "OP-b"]));
    let error = f.run("target remove", Some("active")).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("selection.target_in_use", 3)
    );
    assert_eq!(error.message, "selection.target_in_use: OP-a, OP-b");
    assert!(!f.missing("target/active"));
    f.cleanup();
}
#[test]
fn deletion_keeps_go_config_then_reference_order_on_historical_key_mismatch() {
    let f = Fixture::new();
    crate::workspace::dispatch(&f.add("broken")).unwrap();
    // The reference payload is valid, but its storage key is historical nonsense.
    f.seed([(
        "target-ref/broken/pin/actual-key".into(),
        f.reference("broken", "pin", "other-key"),
    )]);
    assert_eq!(
        f.run("target remove", Some("broken")).unwrap_err().code,
        "vault.record_missing"
    );
    assert!(f.missing("target/broken"));
    assert!(!f.missing("target-ref/broken/pin/actual-key"));
    f.cleanup();
}
#[test]
fn reference_deletions_commit_in_sixty_four_key_batches_without_replaying_success() {
    let f = Fixture::new();
    crate::workspace::dispatch(&f.add("batch")).unwrap();
    f.seed((0..129).map(|i| {
        (
            format!("target-ref/batch/pin/{i:03}"),
            f.reference("batch", "pin", &format!("{i:03}")),
        )
    }));
    f.sql("CREATE TRIGGER fail_second BEFORE DELETE ON documents WHEN OLD.key='target-ref/batch/pin/064' BEGIN SELECT RAISE(ABORT,'fixture-second-batch'); END;");
    assert_eq!(
        f.run("target remove", Some("batch")).unwrap_err().code,
        "vault.metadata"
    );
    assert!(f.missing("target/batch"));
    assert!(f.missing("target-ref/batch/pin/063"));
    assert!(!f.missing("target-ref/batch/pin/064"));
    assert!(!f.missing("target-ref/batch/pin/128"));
    f.cleanup();
}
#[test]
fn later_operation_created_after_snapshot_is_not_an_extra_global_delete_guard() {
    let f = Fixture::new();
    crate::workspace::dispatch(&f.add("race")).unwrap();
    let raw = serde_json::to_string(&f.reference("race", "operation", "OP-late")).unwrap();
    f.sql(&format!("CREATE TRIGGER late AFTER DELETE ON documents WHEN OLD.key='target/race' BEGIN INSERT INTO documents VALUES('target-ref/race/operation/OP-late',1,'{}'); END;",raw.replace('\'',"''")));
    assert_eq!(
        f.run("target remove", Some("race")).unwrap().data["removedRefCount"],
        0
    );
    assert!(!f.missing("target-ref/race/operation/OP-late"));
    f.cleanup();
}
#[test]
fn reference_wire_timestamp_duplicates_null_and_escape_admission_match_go() {
    let control = OperationControl::default();
    let raw=br#"{"schema":"lycheedev.target-ref.v1","name":"n","kind":"pin","id":"old","ID":"new","id":null,"createdAt":"2026-10-15T1:02:03,1200Z","CREATEDAT":null}"#;
    let r: Reference = serde_json::from_slice(raw).unwrap();
    assert_eq!(r.id, "new");
    assert_eq!(
        serde_json::to_value(r).unwrap()["createdAt"],
        "2026-10-15T01:02:03.12Z"
    );
    for raw in [
        br#"{"createdAt":"2026-10-15T00:00:00\u005a"}"#.as_slice(),
        br#"{"createdAt":"2026-10-15 00:00:00Z"}"#.as_slice(),
    ] {
        let normalized = crate::evidence::normalize_go_json_strings(raw, &control).unwrap();
        assert!(serde_json::from_slice::<Reference>(&normalized).is_err());
    }
    let zero: Reference = serde_json::from_slice(br#"{"createdAt":null}"#).unwrap();
    assert_eq!(
        serde_json::to_value(zero).unwrap()["createdAt"],
        "0001-01-01T00:00:00Z"
    );
}
#[test]
fn unmarshal_only_timezone_is_legal_for_remove_but_show_reports_marshal_failure() {
    let f = Fixture::new();
    crate::workspace::dispatch(&f.add("time")).unwrap();
    let mut value = f.reference("time", "pin", "x");
    value["createdAt"] = json!("2026-10-15T00:00:00+24:00");
    f.seed([("target-ref/time/pin/x".into(), value)]);
    assert_eq!(f.run("target show", Some("time")).unwrap_err().exit_code, 5);
    assert_eq!(
        f.run("target remove", Some("time")).unwrap().data["removedRefCount"],
        1
    );
    f.cleanup();
}
#[test]
fn cancelled_original_scope_rejects_management_without_new_metadata_and_keeps_lineage() {
    let f = Fixture::new();
    let token = crate::control::TrackedCancellation::new();
    let mut inv = f.add("cancelled");
    inv.control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(60), token.clone());
    token.cancel();
    let error = crate::workspace::dispatch(&inv).unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(inv.control.stop_cause().is_ok());
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    f.cleanup();
}

#[test]
fn all_named_read_routes_reject_every_non_one_metadata_version() {
    let f = Fixture::new();
    f.seed([("target/version".into(), f.config("version"))]);
    for version in [-1, 0, 2] {
        f.sql(&format!("PRAGMA user_version={version}"));
        for command in ["target list", "target show"] {
            let error = f.run(command, Some("version")).unwrap_err();
            assert_eq!(error.code, "vault.unsupported_workspace");
        }
        assert_eq!(
            lookup_controlled_with_check(&f.home, "version", &f.control, || Ok(()))
                .unwrap_err()
                .code,
            "vault.unsupported_workspace"
        );
    }
    f.cleanup();
}

#[test]
fn show_rejects_unsupported_product_after_valid_target_name() {
    let f = Fixture::new();
    let mut config = f.config("bad-product");
    config["product"] = json!("not-a-wow-product");
    f.seed([("target/bad-product".into(), config)]);
    assert_eq!(
        f.run("target show", Some("bad-product")).unwrap_err().code,
        "selection.unsupported_data_product"
    );
    f.cleanup();
}
