use super::*;
use crate::control::{StopCause, TrackedCancellation};
use std::{
    collections::BTreeMap,
    io::Read,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Fixture {
    root: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-archive-r39-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        Self { root, home }
    }
    fn inv(&self, command: &str, id: Option<&str>) -> Invocation {
        Invocation {
            command: command.into(),
            positional: id.map(str::to_owned),
            cwd: self.root.clone(),
            home: self.home.clone(),
            flags: BTreeMap::new(),
            control: OperationControl::tracked(
                Instant::now() + Duration::from_secs(30),
                TrackedCancellation::new(),
            ),
        }
    }
    fn bundle(&self, ids: &[&str]) -> Invocation {
        let mut inv = self.inv("evidence bundle", None);
        inv.flags.insert("--ids".into(), ids.join(","));
        inv.flags.insert("--output".into(), "bundle.zip".into());
        inv
    }
    fn capture(&self, bytes: &[u8], locator: &str) -> Value {
        commit_capture_bounded(
            &self.home,
            CaptureInput {
                bytes,
                maximum_bytes: bytes.len(),
                media_type: "application/octet-stream",
                provenance: json!({"kind":"fixture","locator":locator}),
                complete: true,
                truncated: false,
            },
            &OperationControl::default(),
        )
        .unwrap()
    }
    fn raw(&self, key: &str, bytes: &[u8], generation: i64) {
        Metadata::with_historical_write(&self.home, &OperationControl::default(), |writer| {
            writer.commit(&[HistoricalMutation {
                key,
                expected_generation: generation,
                bytes,
            }])
        })
        .unwrap();
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.root).unwrap();
    }
}
#[test]
fn default_keep_is_idempotent_and_returns_typed_historical_retention() {
    let f = Fixture::new();
    let capture = f.capture(b"body", "keep");
    let id = capture["id"].as_str().unwrap();
    let inv = f.inv("evidence keep", Some(id));
    let first = super::super::dispatch(&inv).unwrap().data;
    assert_eq!(first["capture"], capture);
    assert_eq!(first["created"], true);
    let next = super::super::dispatch(&inv).unwrap().data;
    assert_eq!(next["created"], false);
    assert_eq!(next["retention"], first["retention"]);
    let raw = format!(
        r#"{{"SCHEMA":"lycheedev.evidence-retention.v1","captureId":"wrong","captureId":"{id}","captureId":null,"blob":{{"sha256":"{}"}},"blob":{{"bytes":4}},"keptAt":null,"ignored":42}}"#,
        capture["blob"]["sha256"].as_str().unwrap()
    );
    f.raw(&format!("evidence/keep/{id}"), raw.as_bytes(), 1);
    let next = super::super::dispatch(&inv).unwrap().data;
    assert_eq!(next["retention"]["keptAt"], "0001-01-01T00:00:00Z");
    assert!(next["retention"].get("ignored").is_none());
    assert_eq!(next["retention"]["blob"], capture["blob"]);
    assert_eq!(inv.control.stop_cause().unwrap(), None);
    inv.control.ordered_child(inv.control.deadline()).unwrap();
    f.cleanup();
}
#[test]
fn keep_rejects_raw_escaped_time_and_mismatched_blob_without_replacing_existing_record() {
    for invalid in [
        r#""2026-10-15T00:00:00\u005a""#,
        r#""2026-10-15 00:00:00Z""#,
    ] {
        let f = Fixture::new();
        let capture = f.capture(b"body", "bad");
        let id = capture["id"].as_str().unwrap();
        let key = format!("evidence/keep/{id}");
        let raw = format!(
            r#"{{"schema":"lycheedev.evidence-retention.v1","captureId":"{id}","blob":{},"keptAt":{invalid}}}"#,
            capture["blob"]
        );
        f.raw(&key, raw.as_bytes(), 0);
        let error = super::super::dispatch(&f.inv("evidence keep", Some(id))).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("evidence.invalid_retention", 4)
        );
        Metadata::with_historical_write(&f.home, &OperationControl::default(), |writer| {
            assert_eq!(writer.document(&key)?.bytes, raw.as_bytes());
            Ok(())
        })
        .unwrap();
        f.cleanup();
    }
}
#[test]
fn held_write_owner_does_not_reacquire_the_same_lease_and_preserves_tombstone_generation() {
    let f = Fixture::new();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(3));
    Metadata::with_historical_write(&f.home, &control, |writer| {
        writer.with_write_lease(|writer| {
            writer.commit(&[HistoricalMutation {
                key: "fixture",
                expected_generation: 0,
                bytes: b"null",
            }])?;
            writer.delete(&[("fixture".into(), 1)])?;
            writer.commit(&[HistoricalMutation {
                key: "fixture",
                expected_generation: 0,
                bytes: b"true",
            }])?;
            assert_eq!(writer.document("fixture")?.generation, 2);
            Ok(())
        })
    })
    .unwrap();
    f.cleanup();
}
#[test]
fn keep_cancel_precedes_metadata_creation_and_active_write_contention_uses_original_deadline() {
    let f = Fixture::new();
    let stop = TrackedCancellation::new();
    stop.cancel();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop);
    assert_eq!(
        keep(&f.home, "invalid", &control).unwrap_err().code,
        "command.cancelled"
    );
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    let capture = f.capture(b"body", "wait");
    let id = capture["id"].as_str().unwrap();
    let guard = crate::metadata::historical_write::historical_lease(
        &f.home,
        "metadata-write",
        &OperationControl::default(),
    )
    .unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(200));
    assert_eq!(
        keep(&f.home, id, &control).unwrap_err().code,
        "command.deadline"
    );
    guard.close().unwrap();
    assert_eq!(
        control.stop_cause().unwrap(),
        Some(StopCause::DeadlineExceeded)
    );
    f.cleanup();
}
#[test]
fn default_bundle_preserves_selection_order_deduplicates_objects_and_writes_manifest_last() {
    let f = Fixture::new();
    let a = f.capture(b"body", "first");
    let b = f.capture(b"body", "second");
    let aid = a["id"].as_str().unwrap();
    let bid = b["id"].as_str().unwrap();
    let result = super::super::dispatch(&f.bundle(&[bid, aid])).unwrap().data;
    assert_eq!(result["bytes"], 4);
    assert_eq!(result["captures"], json!([b, a]));
    assert_eq!(result["complete"], true);
    assert_eq!(result["schema"], "lycheedev.evidence-bundle.v1");
    let file = fs::File::open(f.root.join("bundle.zip")).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    assert_eq!(zip.len(), 4);
    let names: Vec<_> = (0..zip.len())
        .map(|n| zip.by_index(n).unwrap().name().to_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            format!("objects/{}", storage::sha256(b"body")),
            format!("captures/{bid}.json"),
            format!("captures/{aid}.json"),
            "manifest.json".into()
        ]
    );
    let mut raw = Vec::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_end(&mut raw)
        .unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), result);
    drop(zip);
    f.cleanup();
}
#[test]
fn bundle_selection_existing_output_and_workspace_output_fail_before_source_reads() {
    let f = Fixture::new();
    let id = format!("CAP-{}", "a".repeat(64));
    let error = super::super::dispatch(&f.bundle(&[&id, &id])).unwrap_err();
    assert_eq!(error.message, "evidence.bundle_selection");
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    fs::write(f.root.join("bundle.zip"), b"winner").unwrap();
    let error = super::super::dispatch(&f.bundle(&[&id])).unwrap_err();
    assert_eq!(error.message, "evidence.bundle_output");
    assert_eq!(fs::read(f.root.join("bundle.zip")).unwrap(), b"winner");
    let mut inv = f.bundle(&[&id]);
    inv.flags.insert(
        "--output".into(),
        f.home.join("inside.zip").to_string_lossy().into(),
    );
    assert_eq!(
        super::super::dispatch(&inv).unwrap_err().message,
        "evidence.bundle_output"
    );
    assert!(!f.home.join("inside.zip").exists());
    f.cleanup();
}
#[test]
fn bundle_corrupt_later_payload_never_publishes_partial_zip_and_releases_all_owners() {
    let f = Fixture::new();
    let a = f.capture(b"a", "first");
    let b = f.capture(b"b", "second");
    let aid = a["id"].as_str().unwrap();
    let bid = b["id"].as_str().unwrap();
    fs::write(
        blob_path(&f.home, b["blob"]["sha256"].as_str().unwrap()).unwrap(),
        b"X",
    )
    .unwrap();
    // Workspace initialization owns a sibling .lycheedev-locks directory.
    // Compare the exact pre-operation names: no ZIP or private stage may remain.
    let entries = || {
        let mut names = fs::read_dir(&f.root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let before = entries();
    assert_eq!(
        before,
        vec![
            std::ffi::OsString::from(".lycheedev-locks"),
            std::ffi::OsString::from("home"),
        ]
    );
    let error = super::super::dispatch(&f.bundle(&[aid, bid])).unwrap_err();
    assert!(!f.root.join("bundle.zip").exists(), "{error:?}");
    assert_eq!(entries(), before, "{error:?}");
    f.cleanup();
}
#[test]
fn bundle_publication_facts_wrap_all_late_close_causes_without_changing_primary_identity() {
    use lycheedev_windows::filesystem::destination::StagedPublication;
    for state in [StagedPublication::Published, StagedPublication::Unknown] {
        let first = ToolkitError::new("command.cancelled", "stopped", 7);
        let cleanup = ToolkitError::new("vault.io", "close failed", 5);
        let error = bundle::annotate_publication::<()>(
            finish_historical(Err(first), Err(cleanup)),
            state,
            Path::new("output.zip"),
            "stage",
        )
        .unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("command.cancelled", 7)
        );
        assert!(error.message.contains("close failed"));
        assert_eq!(
            error.details["primary"]["details"]["cleanup"]["code"],
            "vault.io"
        );
        assert_eq!(
            error.details["publication"]["state"],
            if state == StagedPublication::Published {
                "published"
            } else {
                "unknown"
            }
        );
    }
}
fn historical(f: &Fixture, bytes: i64, locator: &str) -> String {
    let reference = CaptureRef {
        schema: "lycheedev.capture.v1".into(),
        id: String::new(),
        blob: BlobRef {
            sha256: storage::sha256(b"body"),
            bytes: 0,
        },
        media_type: "application/octet-stream".into(),
        provenance: Provenance {
            kind: "fixture".into(),
            locator: locator.into(),
            snapshot: String::new(),
            operation_id: String::new(),
            source_commit: String::new(),
            base_snapshot: String::new(),
            base_source_commit: String::new(),
            data_build: String::new(),
            session: String::new(),
        },
        complete: true,
        truncated: false,
        captured_at: "2026-10-15T00:00:00Z".into(),
        dependencies: vec![],
    };
    let unsigned = String::from_utf8(storage::identity_json(&reference).unwrap())
        .unwrap()
        .replace("\"bytes\":0", &format!("\"bytes\":{bytes}"));
    let id = format!("CAP-{}", storage::sha256(unsigned.as_bytes()));
    let raw = unsigned.replace("\"id\":\"\"", &format!("\"id\":\"{id}\""));
    f.raw(&format!("capture/{id}"), raw.as_bytes(), 0);
    id
}
#[test]
fn keep_preserves_signed_historical_blob_identity_without_reading_payload() {
    let f = Fixture::new();
    let id = historical(&f, -1, "historical");
    let result = super::super::dispatch(&f.inv("evidence keep", Some(&id)))
        .unwrap()
        .data;
    assert_eq!(result["capture"]["blob"]["bytes"], -1);
    assert_eq!(result["retention"]["blob"]["bytes"], -1);
    let key = format!("evidence/keep/{id}");
    let mut changed = result["retention"].clone();
    changed["blob"]["bytes"] = json!(0);
    f.raw(&key, &serde_json::to_vec(&changed).unwrap(), 1);
    assert_eq!(
        super::super::dispatch(&f.inv("evidence keep", Some(&id)))
            .unwrap_err()
            .code,
        "evidence.invalid_retention"
    );
    f.cleanup();
}
#[test]
fn bundle_per_object_limit_precedes_missing_blob_and_duplicate_digest_is_revalidated() {
    let f = Fixture::new();
    let oversized = historical(&f, (32 << 20) + 1, "large");
    let error = super::super::dispatch(&f.bundle(&[&oversized])).unwrap_err();
    assert_eq!(error.message, "evidence.bundle_limit");
    assert!(!f.root.join("bundle.zip").exists());
    let valid = f.capture(b"body", "valid");
    let invalid = historical(&f, 3, "same-digest-wrong-length");
    let error =
        super::super::dispatch(&f.bundle(&[valid["id"].as_str().unwrap(), &invalid])).unwrap_err();
    assert_eq!(error.code, "vault.blob_integrity");
    assert!(!f.root.join("bundle.zip").exists());
    let negative = historical(&f, -1, "negative");
    assert_eq!(
        super::super::dispatch(&f.bundle(&[&negative]))
            .unwrap_err()
            .code,
        "vault.blob_integrity"
    );
    f.cleanup();
}

#[test]
fn existing_data_keep_is_readable_after_quota_reduction() {
    let f = Fixture::new();
    let capture = commit_data_capture_bounded_with_check(
        &f.home,
        CaptureInput {
            bytes: b"Data keep",
            maximum_bytes: 64,
            media_type: "text/plain",
            provenance: json!({"kind":"fixture","locator":"Data keep"}),
            complete: true,
            truncated: false,
        },
        &OperationControl::default(),
        || Ok(()),
    )
    .unwrap();
    let id = capture["id"].as_str().unwrap();
    let first = keep(&f.home, id, &OperationControl::default()).unwrap();
    assert_eq!(first["created"], true);
    let mut config = workspace::default_config();
    config["dataCache"]["maxBytes"] = json!(1);
    storage::atomic_json(&f.home.join("config.json"), &config).unwrap();
    let second = keep(&f.home, id, &OperationControl::default()).unwrap();
    assert_eq!(second["created"], false);
    assert_eq!(first["retention"], second["retention"]);
}
