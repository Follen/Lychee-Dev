use super::*;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    home: PathBuf,
    path: PathBuf,
    control: OperationControl,
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-spec-file-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        crate::workspace::initialize(&home).unwrap();
        let path = home.join("selection.json");
        Self {
            home,
            path,
            control: OperationControl::uncancelable(Instant::now() + Duration::from_secs(60)),
        }
    }
    fn run(&self, raw: &[u8]) -> Result<PinnedSet> {
        fs::write(&self.path, raw).unwrap();
        resolve_selection_file_controlled(&self.home, &self.path, &self.control)
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.home).unwrap();
    }
}
fn source() -> Value {
    json!({"repository":"wow-ui-source","product":"retail","requestedRef":"live","exactCommit":"a".repeat(40),"parserRevision":"spec-fixture"})
}
fn spec() -> Vec<u8> {
    serde_json::to_vec(&json!({"source":source()})).unwrap()
}
#[test]
fn real_selection_file_publishes_exact_reusable_pin_and_reauthenticates_winner() {
    let f = Fixture::new();
    let pin = f.run(&spec()).unwrap();
    let again = f.run(&spec()).unwrap();
    assert_eq!(pin.id, again.id);
    assert_eq!(
        load_pinned_controlled(&f.home, &pin.id, &f.control)
            .unwrap()
            .source,
        pin.source
    );
    let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
    db.execute(
        "UPDATE documents SET value=?2 WHERE key=?1",
        rusqlite::params![format!("pin/{}", pin.id), br#"{"schema":"bad"}"#.as_slice()],
    )
    .unwrap();
    db.close().unwrap();
    assert!(f.run(&spec()).is_err());
    let m = Metadata::open(&f.home, false).unwrap();
    assert_eq!(
        m.read::<Value>(&format!("pin/{}", pin.id)).unwrap()["schema"],
        "bad"
    );
    m.close().unwrap();
    f.cleanup();
}
#[test]
fn spec_unknown_top_level_fields_are_rejected_before_trailing_data_and_without_metadata() {
    let f = Fixture::new();
    for raw in [
        br#"{"source":{"unknown":true}} {}"#.as_slice(),
        br#"{"unknown":true} malformed"#.as_slice(),
    ] {
        let error = f.run(raw).unwrap_err();
        assert_eq!(error.code, "command.failed");
        assert!(!f.home.join("state/toolkit.sqlite").exists());
    }
    let mut raw = spec();
    raw.extend_from_slice(b" garbage");
    let error = f.run(&raw).unwrap_err();
    assert!(error.message.contains("trailing"), "{}", error.message);
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    f.cleanup();
}
#[test]
fn input_limit_is_actual_sixty_four_kib_plus_one_probe_not_metadata_length() {
    struct Source {
        remaining: usize,
        delivered: usize,
    }
    impl Read for Source {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = self.remaining.min(out.len());
            out[..n].fill(b' ');
            self.remaining -= n;
            self.delivered += n;
            Ok(n)
        }
    }
    let control = OperationControl::default();
    let mut input = Source {
        remaining: 1 << 20,
        delivered: 0,
    };
    assert_eq!(
        read(&mut input, &control).unwrap_err().message,
        "selection.spec_limit"
    );
    assert_eq!(input.delivered, 65537);
    let f = Fixture::new();
    let mut raw = spec();
    raw.resize(65536, b' ');
    assert!(f.run(&raw).is_ok());
    raw.push(b' ');
    assert_eq!(f.run(&raw).unwrap_err().message, "selection.spec_limit");
    f.cleanup();
}
#[test]
fn typed_spec_timestamp_is_normalized_and_invalid_or_unknown_documents_are_rejected() {
    let control = OperationControl::default();
    let changes = |fetched_at: &str, extra: &str| {
        format!(
            r#"{{"changes":{{"provider":"fixture","product":"retail","fullBuild":"1.2.3.4","region":"us","scope":"all","fetchedAt":"{fetched_at}","sha256":"{}"}}{extra}}}"#,
            "f".repeat(64)
        )
    };
    let decoded = wire::decode(
        changes("2026-10-15T01:02:03.1200+00:00", "").as_bytes(),
        &control,
    )
    .unwrap();
    assert_eq!(
        decoded.spec.changes.unwrap().fetched_at,
        "2026-10-15T01:02:03.12Z"
    );
    let error = wire::decode(changes("bad", "").as_bytes(), &control)
        .err()
        .unwrap();
    assert_eq!(error.message, "invalid selection timestamp");
    assert_eq!(error.code, "command.failed");
    let error = wire::decode(
        changes("2026-10-15T00:00:00+24:00", "").as_bytes(),
        &control,
    )
    .err()
    .unwrap();
    assert_eq!(error.message, "invalid selection timestamp");
    let error = wire::decode(br#"{"unknown":true,"source":null}"#, &control)
        .err()
        .unwrap();
    assert!(error.message.contains("unknown field"), "{}", error.message);
}
#[test]
fn valid_decoded_spec_opens_metadata_before_pin_validation_and_parent_precedes_bad_build() {
    let f = Fixture::new();
    fs::create_dir_all(f.home.join("state")).unwrap();
    fs::write(f.home.join("state/toolkit.sqlite"), b"not sqlite").unwrap();
    assert_eq!(f.run(b"{}").unwrap_err().code, "vault.metadata");
    f.cleanup();
    let f = Fixture::new();
    let raw = serde_json::to_vec(&json!({
        "parent":"PIN-missing",
        "data":{
            "product":"retail",
            "region":"us",
            "fullBuild":"+1.2.3.4",
            "buildConfig":"b".repeat(32),
            "cdnConfig":"c".repeat(32),
            "language":"enUS",
            "definitionCommit":"d".repeat(40)
        }
    }))
    .unwrap();
    assert_eq!(f.run(&raw).unwrap_err().code, "vault.record_missing");
    f.cleanup();
}
#[test]
fn original_stop_before_open_and_during_real_read_preserves_ordered_control_and_observed_bytes() {
    let f = Fixture::new();
    let token = crate::control::TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(60), token.clone());
    token.cancel();
    assert_eq!(
        resolve_selection_file_controlled(&f.home, &f.path, &control)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert!(control.stop_cause().unwrap().is_some());
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    struct StopRead {
        token: crate::control::TrackedCancellation,
        delivered: usize,
    }
    impl Read for StopRead {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            out[..3].copy_from_slice(b"abc");
            self.delivered += 3;
            self.token.cancel();
            Ok(3)
        }
    }
    let token = crate::control::TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(60), token.clone());
    let mut input = StopRead {
        token,
        delivered: 0,
    };
    assert_eq!(
        read(&mut input, &control).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(input.delivered, 3);
    f.cleanup();
}
#[test]
fn checked_file_close_failure_keeps_primary_and_original_stop_after_committed_work() {
    let token = crate::control::TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(60), token.clone());
    token.cancel();
    let result: Result<()> = finish(
        Err(failed("primary")),
        Err(ToolkitError::new("external.io", "close", 5)),
        &control,
    );
    let error = result.unwrap_err();
    assert_eq!(
        error.message,
        format!("primary\nclose\n{}", control.check().unwrap_err().message)
    );
    let error = completed::<()>(Err(error), Some("PIN-committed".into())).unwrap_err();
    assert_eq!(error.details["completedPin"], "PIN-committed");
    assert!(error.message.contains("primary\nclose"));
    assert!(
        error
            .message
            .contains("selection pin already committed: PIN-committed")
    );
    assert!(!error.details.is_null());
    let stopped = control.check().unwrap_err();
    let error = finish::<()>(
        Err(stopped.clone()),
        Err(ToolkitError::new("external.io", "close", 5)),
        &control,
    )
    .unwrap_err();
    assert_eq!(error.message, format!("{}\nclose", stopped.message));
}
#[test]
fn typed_workspace_file_resolution_retains_relative_cwd_without_added_snapshot_context() {
    let f = Fixture::new();
    fs::write(&f.path, spec()).unwrap();
    let ctx = crate::request::RequestContext {
        home: f.home.clone(),
        cwd: f.home.clone(),
        control: f.control.clone(),
    };
    let out = crate::workspace::resolve_file(&ctx, Path::new("selection.json")).unwrap();
    assert!(out.data["id"].as_str().unwrap().starts_with("PIN-"));
    assert_eq!(out.selection, json!({}));
    assert!(out.provenance.is_empty());
    f.cleanup();
}
#[test]
fn child_pin_inherits_matching_parent_source_and_rejects_conflict() {
    let f = Fixture::new();
    let parent = f.run(&spec()).unwrap();
    let inherited = source();
    let child = f
        .run(&serde_json::to_vec(&json!({"parent":parent.id,"source":inherited})).unwrap())
        .unwrap();
    assert_eq!(child.source, parent.source);
    assert_eq!(child.parent, parent.id);
    assert_ne!(child.id, parent.id);
    let mut conflicting = source();
    conflicting["requestedRef"] = json!("other");
    let error = f
        .run(&serde_json::to_vec(&json!({"parent":parent.id,"source":conflicting})).unwrap())
        .unwrap_err();
    assert_eq!(error.message, "selection.fixed_source_conflict");
    assert_eq!(error.exit_code, 5);
    f.cleanup();
}
#[test]
fn validation_precedence_and_invalid_timestamp_precede_publication() {
    let f = Fixture::new();
    let error = f.run(br#"{"source":{}}"#).unwrap_err();
    assert_eq!(error.code, "command.failed");
    assert!(error.message.contains("missing field"), "{}", error.message);
    let raw = format!(
        r#"{{"changes":{{"provider":"fixture","product":"retail","fullBuild":"1.2.3.4","region":"us","scope":"all","fetchedAt":"2026-10-15T00:00:00+24:00","sha256":"{}"}}}}"#,
        "f".repeat(64)
    );
    assert_eq!(
        f.run(raw.as_bytes()).unwrap_err().message,
        "invalid selection timestamp"
    );
    // Both rejected specs stop during typed decoding, before metadata is opened.
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    f.cleanup();
}
#[test]
fn real_metadata_schema_lease_wait_observes_original_deadline_and_releases_input() {
    let f = Fixture::new();
    fs::write(&f.path, spec()).unwrap();
    let lease =
        crate::metadata::owned_write::owned_lease(&f.home, "metadata-schema", &f.control).unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(30));
    let error = resolve_selection_file_controlled(&f.home, &f.path, &control).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.deadline", 5)
    );
    assert!(control.stop_cause().unwrap().is_some());
    assert!(
        control
            .ordered_child(Instant::now() + Duration::from_secs(1))
            .is_ok()
    );
    lease.close().unwrap();
    // Delete/recreate is also the Windows proof that the failed invocation did
    // not retain its input File. A later fresh invocation can acquire metadata.
    fs::remove_file(&f.path).unwrap();
    assert!(f.run(&spec()).is_ok());
    f.cleanup();
}

#[test]
fn formal_forever_file_publishes_complete_fixed_identity_and_reauthenticates_cas_winner() {
    let f = Fixture::new();
    let raw = serde_json::to_vec(&json!({
        "source": {"repository":"wow-ui-source","product":"forever","requestedRef":"forever","exactCommit":"4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069","parserRevision":"file-fixture"},
        "data": {"product":"forever","region":"cn","fullBuild":"1.60.1.70658","buildConfig":"165410aca770316ea0f88100bb4830c2","cdnConfig":"c74a0a5afa69d7b60e07f7dae9fc7969","language":"zhCN","definitionCommit":"e".repeat(40)},
        "changes": {"provider":"fixture","product":"forever","fullBuild":"1.60.1.70658","region":"cn","scope":"all","fetchedAt":"2026-10-31T00:00:00Z","sha256":"f".repeat(64)}
    })).unwrap();
    let pin = f.run(&raw).unwrap();
    assert_eq!(pin.source.as_ref().unwrap().product, "forever");
    assert_eq!(
        pin.data.as_ref().unwrap().build_config,
        "165410aca770316ea0f88100bb4830c2"
    );
    assert_eq!(
        pin.data.as_ref().unwrap().cdn_config,
        "c74a0a5afa69d7b60e07f7dae9fc7969"
    );
    assert_eq!(pin.data.as_ref().unwrap().full_build, "1.60.1.70658");
    assert_eq!(pin.changes.as_ref().unwrap().product, "forever");
    let key = format!("pin/{}", pin.id);
    let before = Metadata::read_document_raw_controlled(&f.home, &key, &f.control).unwrap();
    assert_eq!(f.run(&raw).unwrap().id, pin.id);
    let after = Metadata::read_document_raw_controlled(&f.home, &key, &f.control).unwrap();
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.bytes, before.bytes);
    assert_eq!(fs::read(&f.path).unwrap(), raw);
    let metadata = Metadata::open_controlled(&f.home, false, &f.control).unwrap();
    assert_eq!(metadata.list::<Value>("pin/", 100).unwrap().len(), 1);
    assert!(metadata.list::<Value>("capture/", 100).unwrap().is_empty());
    metadata.close().unwrap();
    f.cleanup();
}

#[test]
fn retired_file_source_data_changes_and_shared_family_specs_reject_before_publication() {
    let f = Fixture::new();
    for product in ["forever-asia-beta", "forever-cn-beta"] {
        let mut source = source();
        source["product"] = json!(product);
        let data = json!({"product":product,"region":"cn","fullBuild":"1.60.1.70532","buildConfig":"b".repeat(32),"cdnConfig":"c".repeat(32),"language":"zhCN","definitionCommit":"d".repeat(40)});
        let changes = json!({"provider":"fixture","product":product,"fullBuild":"1.60.1.70532","region":"cn","scope":"all","fetchedAt":"2026-10-31T00:00:00Z","sha256":"e".repeat(64)});
        let mut family = source.clone();
        family["product"] = json!("forever");
        for spec in [
            json!({"source":source}),
            json!({"data":data}),
            json!({"changes":changes}),
            json!({"source":family,"data":data,"changes":changes}),
        ] {
            let raw = serde_json::to_vec(&spec).unwrap();
            let error = f.run(&raw).unwrap_err();
            assert_eq!(error.code, "command.failed");
            assert_eq!(error.exit_code, 5);
            assert_eq!(
                error.message,
                format!("selection.unsupported_data_product: retired product {product}")
            );
            assert!(error.details.is_null());
            assert_eq!(fs::read(&f.path).unwrap(), raw);
        }
    }
    let metadata = Metadata::open_controlled(&f.home, false, &f.control).unwrap();
    assert!(metadata.list::<Value>("pin/", 100).unwrap().is_empty());
    assert!(metadata.list::<Value>("capture/", 100).unwrap().is_empty());
    metadata.close().unwrap();
    // The failed file and writer owners are closed; this same original operation
    // can authenticate and publish a supported selection afterwards.
    fs::remove_file(&f.path).unwrap();
    assert_eq!(
        f.run(&spec()).unwrap().source.as_ref().unwrap().product,
        "retail"
    );
    f.cleanup();
}

#[test]
fn inherited_retired_target_rejects_without_changing_historical_read_or_cas_identity() {
    let f = Fixture::new();
    for product in ["forever-asia-beta", "forever-cn-beta"] {
        let legacy_spec = SelectionSpec {
            source: Some(SourcePin {
                repository: "wow-ui-source".into(),
                product: "forever".into(),
                requested_ref: "forever".into(),
                exact_commit: "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069".into(),
                parser_revision: "historical-file-fixture".into(),
            }),
            data: Some(DataPin {
                product: product.into(),
                region: "cn".into(),
                full_build: "1.60.1.70532".into(),
                build_config: "b".repeat(32),
                cdn_config: "c".repeat(32),
                language: "zhCN".into(),
                definition_commit: "d".repeat(40),
            }),
            ..Default::default()
        };
        // This low-level frozen identity writer is not a current target selector.
        let legacy = pin_selection_controlled(&f.home, legacy_spec.clone(), &f.control).unwrap();
        let key = format!("pin/{}", legacy.id);
        let before = Metadata::read_document_raw_controlled(&f.home, &key, &f.control).unwrap();
        let error = f
            .run(&serde_json::to_vec(&json!({"parent":legacy.id})).unwrap())
            .unwrap_err();
        assert_eq!(error.code, "command.failed");
        assert_eq!(error.exit_code, 5);
        assert_eq!(
            error.message,
            format!("selection.unsupported_data_product: retired product {product}")
        );
        assert_eq!(
            stored_pin(&before.bytes, &legacy.id, &f.control)
                .unwrap()
                .data,
            legacy.data
        );
        assert_eq!(
            load_pinned_controlled(&f.home, &legacy.id, &f.control)
                .unwrap()
                .data,
            legacy.data
        );
        assert_eq!(
            pin_selection_controlled(&f.home, legacy_spec, &f.control)
                .unwrap()
                .id,
            legacy.id
        );
        // Existing parent conflicts still take precedence over current admission.
        let mut conflict = legacy.data.clone().unwrap();
        conflict.build_config = "f".repeat(32);
        let error = f
            .run(&serde_json::to_vec(&json!({"parent":legacy.id,"data":conflict})).unwrap())
            .unwrap_err();
        assert_eq!(error.message, "selection.fixed_data_conflict");
        let malformed = json!({"data":{"product":product,"region":"cn","fullBuild":"+1.60.1.70532","buildConfig":"b".repeat(32),"cdnConfig":"c".repeat(32),"language":"zhCN","definitionCommit":"d".repeat(40)}});
        assert_eq!(
            f.run(&serde_json::to_vec(&malformed).unwrap())
                .unwrap_err()
                .message,
            "selection.unresolved_data"
        );
        let after = Metadata::read_document_raw_controlled(&f.home, &key, &f.control).unwrap();
        assert_eq!(after.generation, before.generation);
        assert_eq!(after.bytes, before.bytes);
    }
    let metadata = Metadata::open_controlled(&f.home, false, &f.control).unwrap();
    assert_eq!(metadata.list::<Value>("pin/", 100).unwrap().len(), 2);
    assert!(metadata.list::<Value>("capture/", 100).unwrap().is_empty());
    metadata.close().unwrap();
    f.cleanup();
}

#[test]
fn file_admission_does_not_restrict_other_original_source_product_strings() {
    let f = Fixture::new();
    for product in [
        "custom-source-product",
        "forever-asia-beta-extra",
        "FOREVER-ASIA-BETA",
    ] {
        let mut source = source();
        source["product"] = json!(product);
        let pin = f
            .run(&serde_json::to_vec(&json!({"source":source})).unwrap())
            .unwrap();
        assert_eq!(pin.source.as_ref().unwrap().product, product);
    }
    f.cleanup();
}
