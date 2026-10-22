use lycheedev_core::{
    cache, evidence,
    metadata::Metadata,
    selection::{self, SelectionSpec, SourcePin},
    storage::{self, ExclusiveLock},
    workspace,
    request::RequestContext,
};
use serde_json::json;
use std::{fs, path::PathBuf};
static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-rust-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn initialized() -> Temp {
    let temp = Temp::new();
    workspace::initialize(&temp.0).unwrap();
    temp
}
#[test]
fn bounded_identity_hash_rejects_actual_size_and_observes_cancel_before_open() {
    use lycheedev_core::control::{OperationControl, TrackedCancellation};
    use std::time::{Duration, Instant};
    let temp = initialized();
    let path = temp.0.join("identity.bin");
    fs::File::create(&path).unwrap().set_len(128 << 20).unwrap();
    let control = OperationControl::default();
    assert_eq!(
        storage::locked_sha256(&path, 8, &control)
            .unwrap_err()
            .exit_code,
        4
    );
    fs::write(&path, b"sealed identity").unwrap();
    let (held, digest) = storage::locked_sha256(&path, 64, &control).unwrap();
    assert_eq!(digest, storage::sha256(b"sealed identity"));
    #[cfg(windows)]
    {
        assert!(fs::write(&path, b"changed").is_err());
        assert!(fs::remove_file(&path).is_err());
    }
    drop(held);
    let token = TrackedCancellation::new();
    token.cancel();
    let cancelled = OperationControl::tracked(
        Instant::now() + Duration::from_secs(10),
        token,
    );
    assert_eq!(
        storage::locked_sha256(&temp.0.join("missing"), 64, &cancelled)
            .unwrap_err()
            .exit_code,
        7
    );
    let expired = OperationControl::uncancelable(Instant::now());
    assert_eq!(
        storage::locked_sha256(&path, 64, &expired)
            .unwrap_err()
            .exit_code,
        5
    );
}
#[test]
fn doctor_provider_warnings_do_not_fail_but_error_checks_retain_the_combined_report() {
    let temp = initialized();
    let request = RequestContext { home: temp.0.clone(), cwd: temp.0.clone(), control: Default::default() };
    let warning =
        json!({"id":"luals.runtime","ok":false,"status":"warn","detail":"fixture unavailable"});
    let report = lycheedev_core::doctor::inspect_with_checks(&request, vec![warning]).unwrap();
    assert_eq!(report.data["healthy"], true);
    assert!(
        report.data["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["id"] == "luals.runtime")
    );
    let error=lycheedev_core::doctor::inspect_with_checks(&request,vec![json!({"id":"provider.verified-input","ok":false,"status":"error","detail":"invalid fixture"})]).unwrap_err();
    assert_eq!(error.exit_code, 3);
    assert_eq!(error.details["result"]["healthy"], false);
    assert!(error.details["result"]["checks"].as_array().unwrap().len() > 1);
    assert_eq!(
        lycheedev_core::doctor::inspect_with_checks(
            &request,
            vec![json!({"id":"invalid","ok":true,"status":"pretend-ready"})]
        )
        .unwrap_err()
        .exit_code,
        4
    );
}
fn context(home: &std::path::Path) -> RequestContext {
    RequestContext { home: home.to_owned(), cwd: home.to_owned(), control: Default::default() }
}

#[test]
fn replacement_is_atomic_and_updates_existing_windows_file() {
    let temp = initialized();
    let path = temp.0.join("config.json");
    storage::atomic_write(&path, b"old").unwrap();
    storage::atomic_write(&path, b"new").unwrap();
    assert_eq!(fs::read(path).unwrap(), b"new");
    assert_eq!(
        fs::read_dir(&temp.0)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|p| p.file_name().to_string_lossy().starts_with(".lycheedev-"))
            .count(),
        0
    );
}
#[test]
fn kernel_lock_cannot_be_stolen_by_stale_timestamp() {
    let temp = initialized();
    let path = temp.0.join("locks/owner.lock");
    let held = ExclusiveLock::acquire(&path).unwrap();
    assert_eq!(ExclusiveLock::acquire(&path).err().unwrap().exit_code, 6);
    drop(held);
    assert!(ExclusiveLock::acquire(&path).is_ok());
    assert!(path.exists());
}
#[test]
fn cas_rejects_lost_update_and_preserves_committed_document() {
    let temp = initialized();
    let mut first = Metadata::open(&temp.0, true).unwrap();
    first.put("test/a", &json!({"n":1}), None).unwrap();
    let mut second = Metadata::open(&temp.0, true).unwrap();
    second.put("test/a", &json!({"n":2}), Some(1)).unwrap();
    assert_eq!(
        first
            .put("test/a", &json!({"n":3}), Some(1))
            .err()
            .unwrap()
            .code,
        "vault.generation_conflict"
    );
    assert_eq!(
        first.read::<serde_json::Value>("test/a").unwrap(),
        json!({"n":2})
    );
}
#[test]
fn newer_sqlite_schema_refuses_open_without_downgrade() {
    let temp = initialized();
    drop(Metadata::open(&temp.0, true).unwrap());
    let db = rusqlite::Connection::open(temp.0.join("state/toolkit.sqlite")).unwrap();
    db.pragma_update(None, "user_version", 2).unwrap();
    assert_eq!(
        Metadata::open(&temp.0, true).err().unwrap().code,
        "vault.schema_newer"
    );
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn startup_intent_consume_is_atomic_and_rejection_keeps_the_original() {
    let temp = initialized();
    let key = "source/native-session-start/fixed";
    let mut store = Metadata::open(&temp.0, true).unwrap();
    store
        .put(key, &json!({"identity":"expected"}), None)
        .unwrap();
    let rejected = store.consume::<serde_json::Value, ()>(key, |_| {
        Err(lycheedev_core::ToolkitError::invalid("different launcher"))
    });
    assert_eq!(rejected.unwrap_err().exit_code, 4);
    assert_eq!(
        store.read::<serde_json::Value>(key).unwrap(),
        json!({"identity":"expected"})
    );
    let home = temp.0.clone();
    let first = std::thread::spawn(move || {
        Metadata::open(&home, true)
            .unwrap()
            .consume::<serde_json::Value, _>(key, |value| Ok(value))
    });
    let home = temp.0.clone();
    let second = std::thread::spawn(move || {
        Metadata::open(&home, true)
            .unwrap()
            .consume::<serde_json::Value, _>(key, |value| Ok(value))
    });
    let outcomes = [first.join().unwrap(), second.join().unwrap()];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter_map(|result| result.as_ref().err())
            .next()
            .unwrap()
            .code,
        "vault.record_missing"
    );
}
#[test]
fn initialize_never_adopts_populated_legacy_directory() {
    let temp = Temp::new();
    fs::create_dir(&temp.0).unwrap();
    fs::write(temp.0.join("foreign.txt"), b"owner").unwrap();
    assert_eq!(
        workspace::initialize(&temp.0).err().unwrap().code,
        "vault.legacy_detected"
    );
    assert_eq!(fs::read(temp.0.join("foreign.txt")).unwrap(), b"owner");
    assert!(!temp.0.join("workspace.json").exists());
}
#[test]
fn pins_serialize_in_retained_wire_order_and_inheritance_cannot_replace_source() {
    let temp = initialized();
    let source = SourcePin {
        repository: "wow-ui-source".into(),
        product: "retail".into(),
        requested_ref: "refs/heads/live".into(),
        exact_commit: "1".repeat(40),
        parser_revision: "lua-ast-r1".into(),
    };
    let pin = selection::pin_selection(
        &temp.0,
        SelectionSpec {
            source: Some(source.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let unsigned = format!(
        "{{\"schema\":\"lycheedev.selection.v1\",\"id\":\"\",\"source\":{{\"repository\":\"wow-ui-source\",\"product\":\"retail\",\"requestedRef\":\"refs/heads/live\",\"exactCommit\":\"{}\",\"parserRevision\":\"lua-ast-r1\"}}}}",
        "1".repeat(40)
    );
    assert_eq!(
        pin.id,
        format!("PIN-{}", storage::sha256(unsigned.as_bytes()))
    );
    let mut changed = source;
    changed.exact_commit = "2".repeat(40);
    assert_eq!(
        selection::pin_selection(
            &temp.0,
            SelectionSpec {
                parent: pin.id.clone(),
                source: Some(changed),
                ..Default::default()
            }
        )
        .err()
        .unwrap()
        .code,
        "selection.fixed_source_conflict"
    );
    assert_eq!(selection::load_pinned(&temp.0, &pin.id).unwrap().id, pin.id);
}
#[test]
fn corrupt_original_evidence_is_detected_and_cache_prune_keeps_evidence() {
    let temp = initialized();
    let capture = evidence::commit_capture(
        &temp.0,
        b"original",
        "text/plain",
        json!({"kind":"source","locator":"a.lua"}),
        true,
        false,
    )
    .unwrap();
    cache::publish(&temp.0, b"ordinary", 100, None).unwrap();
    cache::dispatch(&context(&temp.0), cache::CacheRequest::Prune {
        target_bytes: 0, dry_run: false, max_objects: 0, include_uncommitted: false,
    })
    .unwrap();
    assert_eq!(
        evidence::read_blob(&temp.0, &capture["blob"], 100).unwrap(),
        b"original"
    );
    fs::write(
        evidence::blob_path(&temp.0, capture["blob"]["sha256"].as_str().unwrap()).unwrap(),
        b"changed",
    )
    .unwrap();
    assert_eq!(
        evidence::read_blob(&temp.0, &capture["blob"], 100)
            .err()
            .unwrap()
            .code,
        "vault.blob_integrity"
    );
}
#[test]
fn tampered_capture_manifest_is_not_accepted_under_original_identity() {
    let temp = initialized();
    let capture = evidence::commit_capture(
        &temp.0,
        b"original",
        "text/plain",
        json!({"kind":"source","locator":"<&>\u{2028}.lua"}),
        true,
        false,
    )
    .unwrap();
    let id = capture["id"].as_str().unwrap();
    assert!(evidence::read_capture(&temp.0, id).is_ok());
    let mut modified = capture.clone();
    modified["complete"] = json!(false);
    Metadata::open(&temp.0, true)
        .unwrap()
        .put(&format!("capture/{id}"), &modified, Some(1))
        .unwrap();
    assert_eq!(
        evidence::read_capture(&temp.0, id).err().unwrap().code,
        "evidence.capture_integrity"
    );
}
#[test]
fn protected_cache_survives_prune_and_corruption_is_visible() {
    let temp = initialized();
    let blob = cache::publish(
        &temp.0,
        b"protected",
        100,
        Some(json!({"kind":"operation","owner":"OP-active","retainedAt":"2026-10-08T00:00:00Z"})),
    )
    .unwrap();
    let result = cache::dispatch(&context(&temp.0), cache::CacheRequest::Prune {
        target_bytes: 0, dry_run: false, max_objects: 0, include_uncommitted: false,
    })
    .unwrap();
    assert!(!result.complete);
    assert_eq!(result.data["skippedProtected"], 1);
    let digest = blob["sha256"].as_str().unwrap();
    fs::write(
        temp.0
            .join("cache/objects")
            .join(&digest[..2])
            .join(&digest[2..]),
        b"bad",
    )
    .unwrap();
    assert_eq!(
        cache::dispatch(&context(&temp.0), cache::CacheRequest::Verify)
            .err()
            .unwrap()
            .code,
        "vault.cache_integrity"
    );
}
#[test]
fn paths_cannot_escape_selected_root() {
    let temp = initialized();
    assert!(storage::safe_relative(&temp.0, "../foreign").is_err());
    assert!(storage::safe_relative(&temp.0, "C:\\foreign").is_err());
    assert!(storage::guard_path(&temp.0, &temp.0.parent().unwrap().join("foreign")).is_err());
}
#[test]
fn newer_config_is_not_overwritten_with_defaults() {
    let temp = initialized();
    storage::atomic_json(
        &temp.0.join("config.json"),
        &json!({"schema":"lycheedev.config.v2","cache":{"maxBytes":3},"download":{"workers":3}}),
    )
    .unwrap();
    assert_eq!(
        workspace::set_config(&RequestContext { home: temp.0.clone(), cwd: temp.0.clone(), control: Default::default() }, &workspace::ConfigUpdate { download_workers: Some(1), ..Default::default() })
        .err()
        .unwrap()
        .code,
        "vault.config_schema_newer"
    );
    assert_eq!(
        storage::read_json::<serde_json::Value>(&temp.0.join("config.json")).unwrap()["download"]["workers"],
        3
    );
}

#[test]
fn fresh_archive_preserves_opaque_legacy_bytes_and_refuses_new_workspace() {
    let temp = Temp::new();
    fs::create_dir(&temp.0).unwrap();
    let old = vec![0xff, 0, 0x80, 1, 2, 3];
    fs::write(temp.0.join("profiles"), &old).unwrap();
    let planned = lycheedev_core::archive::plan(&temp.0).unwrap();
    assert_eq!(planned["state"], "planned");
    assert!(
        !lycheedev_core::archive::journal_path(&temp.0)
            .unwrap()
            .exists()
    );
    let completed = lycheedev_core::archive::fresh(&temp.0).unwrap();
    let archive = PathBuf::from(completed["plan"]["archivePath"].as_str().unwrap());
    assert_eq!(fs::read(archive.join("profiles")).unwrap(), old);
    assert!(temp.0.join("workspace.json").exists());
    assert_eq!(lycheedev_core::archive::fresh(&temp.0).unwrap(), completed);
    let new = initialized();
    assert_eq!(
        lycheedev_core::archive::plan(&new.0).err().unwrap().code,
        "vault.workspace_exists"
    );
    let _ = fs::remove_dir_all(archive);
    let _ = fs::remove_file(lycheedev_core::archive::journal_path(&temp.0).unwrap());
}
#[test]
fn archive_recovers_move_before_state_checkpoint_without_reimport() {
    let temp = Temp::new();
    fs::create_dir(&temp.0).unwrap();
    fs::write(temp.0.join("opaque.bin"), b"original legacy bytes").unwrap();
    let plan = lycheedev_core::archive::plan(&temp.0).unwrap();
    let journal = lycheedev_core::archive::journal_path(&temp.0).unwrap();
    storage::atomic_json(&journal, &plan).unwrap();
    let archive = PathBuf::from(plan["archivePath"].as_str().unwrap());
    fs::rename(&temp.0, &archive).unwrap();
    let result = lycheedev_core::archive::fresh(&temp.0).unwrap();
    assert_eq!(result["plan"]["state"], "complete");
    assert!(!temp.0.join("opaque.bin").exists());
    assert_eq!(
        fs::read(archive.join("opaque.bin")).unwrap(),
        b"original legacy bytes"
    );
    let _ = fs::remove_dir_all(archive);
    let _ = fs::remove_file(journal);
}
#[test]
fn archive_rejects_redirected_intent_destination_before_any_move() {
    let temp = Temp::new();
    fs::create_dir(&temp.0).unwrap();
    fs::write(temp.0.join("owner.bin"), b"kept").unwrap();
    let mut plan = lycheedev_core::archive::plan(&temp.0).unwrap();
    let journal = lycheedev_core::archive::journal_path(&temp.0).unwrap();
    plan["archivePath"] = json!(temp.0.parent().unwrap().join("unrelated-owner"));
    storage::atomic_json(&journal, &plan).unwrap();
    assert_eq!(
        lycheedev_core::archive::fresh(&temp.0).err().unwrap().code,
        "vault.archive_state"
    );
    assert_eq!(fs::read(temp.0.join("owner.bin")).unwrap(), b"kept");
    let _ = fs::remove_file(journal);
}

#[test]
fn doctor_is_readonly_for_metadata_and_surfaces_corrupt_cache_result() {
    let temp = initialized();
    let request = RequestContext { home: temp.0.clone(), cwd: temp.0.clone(), control: Default::default() };
    let report = lycheedev_core::doctor::inspect(&request).unwrap();
    assert_eq!(report.data["healthy"], true);
    assert!(!temp.0.join("state/toolkit.sqlite").exists());
    let blob = cache::publish(&temp.0, b"managed", 100, None).unwrap();
    let digest = blob["sha256"].as_str().unwrap();
    fs::write(
        temp.0
            .join("cache/objects")
            .join(&digest[..2])
            .join(&digest[2..]),
        b"damaged",
    )
    .unwrap();
    let error = lycheedev_core::doctor::inspect(&request).err().unwrap();
    assert_eq!(error.exit_code, 3);
    assert_eq!(error.details["result"]["healthy"], false);
    assert!(
        error.details["result"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["id"] == "vault.cache_integrity" && check["status"] == "error")
    );
    assert_eq!(
        fs::read(
            temp.0
                .join("cache/objects")
                .join(&digest[..2])
                .join(&digest[2..])
        )
        .unwrap(),
        b"damaged"
    );
}

#[test]
fn staging_accounting_orphan_retention_and_active_stage_guards_are_honest() {
    let temp = initialized();
    let root = temp.0.join("cache/tmp");
    fs::create_dir_all(&root).unwrap();
    let id = "a".repeat(32);
    let file = root.join(format!("put-{id}.part"));
    fs::write(&file, b"unfinished").unwrap();
    let owner =
        ExclusiveLock::resource(&temp.0.join("locks"), &format!("cache:stage:{id}")).unwrap();
    let status = cache::dispatch(&context(&temp.0), cache::CacheRequest::Status).unwrap();
    assert_eq!(status.data["uncommittedCount"], 1);
    assert_eq!(status.data["uncommittedBytes"], 10);
    let protected = cache::dispatch(&context(&temp.0), cache::CacheRequest::Prune {
        target_bytes: 0, dry_run: false, max_objects: 0, include_uncommitted: true,
    })
    .unwrap();
    assert_eq!(protected.data["skippedInUse"], 1);
    assert!(file.exists());
    drop(owner);
    let removed = cache::dispatch(&context(&temp.0), cache::CacheRequest::Prune {
        target_bytes: 0, dry_run: false, max_objects: 0, include_uncommitted: true,
    })
    .unwrap();
    assert_eq!(removed.data["removedUncommitted"], 1);
    assert!(!file.exists());
    let digest = "b".repeat(64);
    storage::atomic_json(&temp.0.join("cache/retain").join(format!("{digest}.json")),&json!({"sha256":digest,"owners":[{"kind":"operation","owner":"OP-unknown","retainedAt":"2026-10-08T00:00:00Z"}]})).unwrap();
    let status = cache::dispatch(&context(&temp.0), cache::CacheRequest::Status).unwrap();
    assert_eq!(status.data["orphanMarkers"], 1);
    let verify = cache::dispatch(&context(&temp.0), cache::CacheRequest::Verify)
        .err()
        .unwrap();
    assert_eq!(verify.details["result"]["missing"][0], digest);
}
#[test]
fn evidence_removal_refuses_external_references_then_preserves_shared_original_bytes() {
    let temp = initialized();
    let first = evidence::commit_capture(
        &temp.0,
        b"same original",
        "text/plain",
        json!({"kind":"test","locator":"first"}),
        true,
        false,
    )
    .unwrap();
    let second = evidence::commit_capture(
        &temp.0,
        b"same original",
        "text/plain",
        json!({"kind":"test","locator":"second"}),
        true,
        false,
    )
    .unwrap();
    let first_id = first["id"].as_str().unwrap();
    let second_id = second["id"].as_str().unwrap();
    let mut metadata = Metadata::open(&temp.0, true).unwrap();
    metadata
        .put("operation/pending", &json!({"capture":first_id}), None)
        .unwrap();
    assert_eq!(
        evidence::remove_capture(&temp.0, first_id)
            .err()
            .unwrap()
            .code,
        "evidence.capture_referenced"
    );
    assert!(evidence::read_capture(&temp.0, first_id).is_ok());
    metadata.delete("operation/pending").unwrap();
    let first_removed = evidence::remove_capture(&temp.0, first_id).unwrap();
    assert_eq!(first_removed["blobShared"], true);
    assert_eq!(
        evidence::read_blob(&temp.0, &second["blob"], 100).unwrap(),
        b"same original"
    );
    let second_removed = evidence::remove_capture(&temp.0, second_id).unwrap();
    assert_eq!(second_removed["blobRemoved"], true);
    assert!(
        !evidence::blob_path(&temp.0, second["blob"]["sha256"].as_str().unwrap())
            .unwrap()
            .exists()
    );
}
#[test]
fn retention_uses_published_wire_schema_and_cannot_hide_another_owner() {
    let temp = initialized();
    let capture = evidence::commit_capture(
        &temp.0,
        b"kept",
        "text/plain",
        json!({"kind":"test","locator":"capture"}),
        true,
        false,
    )
    .unwrap();
    let id = capture["id"].as_str().unwrap();
    let context = context(&temp.0);
    let request = evidence::EvidenceRequest::Keep { id: id.to_owned() };
    let first = evidence::dispatch(&context, request.clone()).unwrap();
    let second = evidence::dispatch(&context, request).unwrap();
    assert_eq!(
        first.data["retention"]["schema"],
        "lycheedev.evidence-retention.v1"
    );
    assert_eq!(second.data["created"], false);
    let metadata = Metadata::open(&temp.0, false).unwrap();
    assert!(
        metadata
            .read::<serde_json::Value>(&format!("evidence/keep/{id}"))
            .is_ok()
    );
    let removed = evidence::remove_capture(&temp.0, id).unwrap();
    assert_eq!(removed["retentionRemoved"], true);
}

#[test]
fn target_file_pins_exact_resolved_context_and_running_reference_blocks_remove() {
    let temp = initialized();
    let ctx = RequestContext { home: temp.0.clone(), cwd: temp.0.clone(), control: Default::default() };
    assert_eq!(
        selection::target::execute(&ctx, &selection::target::TargetRequest::List)
            .unwrap()
            .data["targets"],
        json!([])
    );
    let spec = temp.0.join("resolved-spec.json");
    storage::atomic_json(&spec,&json!({"source":{"repository":"wow-ui-source","product":"retail","requestedRef":"refs/heads/live","exactCommit":"1".repeat(40),"parserRevision":"source-v1"}})).unwrap();
    let selected = workspace::resolve_file(&ctx, &spec).unwrap();
    let pin = selected.data["id"].as_str().unwrap();
    assert!(selection::load_pinned(&temp.0, pin).is_ok());
    selection::target::execute(&ctx, &selection::target::TargetRequest::Add {
        config: selection::target::TargetConfig { schema: "lycheedev.target.v1".into(), name: "explicit".into(), product: "retail".into(), region: "us".into(), locale: "enUS".into(), source: "remote".into(), ..Default::default() }, replace: false,
    }).unwrap();
    let mut metadata = Metadata::open(&temp.0, true).unwrap();
    metadata.put("target-ref/explicit/operation/OP-running",&json!({"schema":"lycheedev.target-ref.v1","name":"explicit","kind":"operation","id":"OP-running","createdAt":"2026-10-08T00:00:00Z"}),None).unwrap();
    let remove = selection::target::TargetRequest::Remove { key: "explicit".into() };
    assert_eq!(
        selection::target::execute(&ctx, &remove).err().unwrap().code,
        "selection.target_in_use"
    );
    metadata
        .delete("target-ref/explicit/operation/OP-running")
        .unwrap();
    metadata.put(&format!("target-ref/explicit/pin/{pin}"),&json!({"schema":"lycheedev.target-ref.v1","name":"explicit","kind":"pin","id":pin,"createdAt":"2026-10-08T00:00:00Z"}),None).unwrap();
    let removed = selection::target::execute(&ctx, &remove).unwrap();
    assert_eq!(removed.data["removedRefCount"], 1);
    assert!(selection::load_pinned(&temp.0, pin).is_ok());
}

#[test]
fn content_identity_timestamps_match_reference_rfc3339_nano_normalization() {
    let value = chrono::DateTime::parse_from_rfc3339("2026-10-08T00:00:00.123456700Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(storage::timestamp(value), "2026-10-08T00:00:00.1234567Z");
    let zero = chrono::DateTime::parse_from_rfc3339("2026-10-08T00:00:00.000000000Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(storage::timestamp(zero), "2026-10-08T00:00:00Z");
    let offset =
        chrono::DateTime::parse_from_rfc3339("2026-10-08T08:00:00.123456700+08:00").unwrap();
    assert_eq!(
        storage::timestamp_offset(offset),
        "2026-10-08T08:00:00.1234567+08:00"
    );
}
#[test]
fn nested_operation_control_never_resets_deadline_or_loses_caller_cancel() {
    use lycheedev_core::control::TrackedCancellation;
    use std::time::{Duration, Instant};
    let token = TrackedCancellation::new();
    let original = lycheedev_core::control::OperationControl::tracked(
        Instant::now() + Duration::from_secs(30),
        token.clone(),
    );
    let nested = original.clone();
    assert_eq!(original.deadline(), nested.deadline());
    assert!(nested.remaining().unwrap() <= Duration::from_secs(30));
    token.cancel();
    assert_eq!(original.check().unwrap_err().exit_code, 7);
    assert_eq!(nested.remaining().unwrap_err().code, "command.cancelled");
    let expired = lycheedev_core::control::OperationControl::uncancelable(
        Instant::now() - Duration::from_millis(1),
    );
    assert_eq!(expired.check().unwrap_err().code, "command.deadline");
}
#[cfg(windows)]
#[test]
fn evidence_removal_resumes_after_windows_delete_denial_and_keeps_the_original_receipt() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = initialized();
    let capture = evidence::commit_capture(
        &temp.0,
        b"owned original",
        "text/plain",
        json!({"kind":"test","locator":"locked"}),
        true,
        false,
    )
    .unwrap();
    let id = capture["id"].as_str().unwrap();
    let path = evidence::blob_path(&temp.0, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    let error = evidence::remove_capture(&temp.0, id).unwrap_err();
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.details["result"]["manifestRemoved"], true);
    assert!(evidence::read_capture(&temp.0, id).is_err());
    let metadata = Metadata::open(&temp.0, false).unwrap();
    let intent: serde_json::Value = metadata.read(&format!("evidence/remove/{id}")).unwrap();
    assert_eq!(intent["capture"], capture);
    assert_eq!(fs::read(&path).unwrap(), b"owned original");
    drop(held);
    let done = evidence::remove_capture(&temp.0, id).unwrap();
    assert_eq!(done["blobRemoved"], true);
    assert_eq!(done["blobMissing"], false);
    assert!(!path.exists());
    assert_eq!(evidence::remove_capture(&temp.0, id).unwrap(), done);
}

#[test]
fn evidence_corruption_and_invalid_retention_cannot_dispose_original_identity() {
    let temp = initialized();
    let capture = evidence::commit_capture(
        &temp.0,
        b"original",
        "text/plain",
        json!({"kind":"test","locator":"corrupt"}),
        true,
        false,
    )
    .unwrap();
    let id = capture["id"].as_str().unwrap();
    let path = evidence::blob_path(&temp.0, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
    let expanded = fs::OpenOptions::new().write(true).open(&path).unwrap();
    expanded.set_len(128 << 20).unwrap();
    assert_eq!(
        evidence::read_blob(&temp.0, &capture["blob"], 64)
            .unwrap_err()
            .code,
        "vault.blob_integrity"
    );
    assert_eq!(
        evidence::remove_capture(&temp.0, id).unwrap_err().code,
        "vault.blob_integrity"
    );
    assert!(evidence::read_capture(&temp.0, id).is_ok());
    fs::write(&path, b"original").unwrap();
    Metadata::open(&temp.0, true)
        .unwrap()
        .put(
            &format!("evidence/keep/{id}"),
            &json!({"schema":"future","captureId":"wrong capture","blob":capture["blob"]}),
            None,
        )
        .unwrap();
    // Current typed retention decoding rejects the incompatible document before
    // any removal action.
    assert_eq!(
        evidence::remove_capture(&temp.0, id).unwrap_err().code,
        "input.invalid"
    );
    assert_eq!(fs::read(path).unwrap(), b"original");
    assert!(evidence::read_capture(&temp.0, id).is_ok());
}

#[cfg(windows)]
#[test]
fn evidence_pending_intent_rejects_forgery_and_recovers_delete_before_checkpoint() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = initialized();
    let capture = evidence::commit_capture(
        &temp.0,
        b"checkpoint",
        "text/plain",
        json!({"kind":"test","locator":"cut-point"}),
        true,
        false,
    )
    .unwrap();
    let id = capture["id"].as_str().unwrap();
    let path = evidence::blob_path(&temp.0, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert_eq!(
        evidence::remove_capture(&temp.0, id).unwrap_err().exit_code,
        5
    );
    let key = format!("evidence/remove/{id}");
    let mut metadata = Metadata::open(&temp.0, true).unwrap();
    let original: serde_json::Value = metadata.read(&key).unwrap();
    let mut forged = original.clone();
    forged["capture"]["provenance"]["locator"] = json!("different immutable request");
    let generation = metadata.generation(&key).unwrap().unwrap();
    metadata.put(&key, &forged, Some(generation)).unwrap();
    assert_eq!(
        evidence::remove_capture(&temp.0, id).unwrap_err().code,
        "evidence.removal_integrity"
    );
    assert_eq!(fs::read(&path).unwrap(), b"checkpoint");
    let generation = metadata.generation(&key).unwrap().unwrap();
    metadata.put(&key, &original, Some(generation)).unwrap();
    drop(held);
    // The owned delete completed, but the process died before its result was durable.
    fs::remove_file(&path).unwrap();
    let result = evidence::remove_capture(&temp.0, id).unwrap();
    assert_eq!(result["blobRemoved"], true);
    assert_eq!(result["blobMissing"], false);
    assert_eq!(evidence::remove_capture(&temp.0, id).unwrap(), result);
}

#[test]
fn regional_projects_lock_shared_source_and_preserve_exact_data_identity() {
    use selection::{ChangePin, DataPin};
    let temp = initialized();
    let source = SourcePin {
        repository: "wow-ui-source".into(),
        product: "forever".into(),
        requested_ref: "forever".into(),
        exact_commit: "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069".into(),
        parser_revision: "fixture-v1".into(),
    };
    let parent = selection::pin_selection(
        &temp.0,
        SelectionSpec {
            source: Some(source),
            ..Default::default()
        },
    )
    .unwrap();
    let mut ids = vec![];
    for product in ["forever-asia-beta", "forever-cn-beta"] {
        let data = DataPin {
            product: product.into(),
            region: "cn".into(),
            language: "zhCN".into(),
            full_build: "1.60.1.70338".into(),
            build_config: "c".repeat(32),
            cdn_config: "d".repeat(32),
            definition_commit: "e".repeat(40),
        };
        let changes = ChangePin {
            product: product.into(),
            region: "cn".into(),
            provider: "fixture".into(),
            scope: "full".into(),
            full_build: data.full_build.clone(),
            fetched_at: "2026-10-14T00:00:00Z".into(),
            sha256: "f".repeat(64),
        };
        let pin = selection::pin_selection(
            &temp.0,
            SelectionSpec {
                parent: parent.id.clone(),
                data: Some(data),
                changes: Some(changes),
                ..Default::default()
            },
        )
        .unwrap();
        ids.push(pin.id.clone());
        assert_eq!(
            selection::load_pinned(&temp.0, &pin.id)
                .unwrap()
                .data
                .as_ref()
                .unwrap()
                .product,
            product
        );
        let path = temp.0.join(product);
        std::fs::create_dir_all(&path).unwrap();
        let path = path.to_str().unwrap();
        selection::execute_project(&RequestContext { home: temp.0.clone(), cwd: temp.0.clone(), control: Default::default() }, &selection::ProjectRequest::Init { path: path.into(), product: product.into() })
        .unwrap();
        selection::execute_project(&RequestContext { home: temp.0.clone(), cwd: temp.0.clone(), control: Default::default() }, &selection::ProjectRequest::Lock { path: Some(path.into()), snapshot: pin.id.clone() })
        .unwrap();
        let status = selection::inspect_project(std::path::Path::new(path)).unwrap();
        assert_eq!(status["state"], "locked");
        assert_eq!(status["lock"]["selection"]["source"]["product"], "forever");
        assert_eq!(status["lock"]["selection"]["data"]["product"], product);
        let opposite = if product == "forever-cn-beta" {
            "forever-asia-beta"
        } else {
            "forever-cn-beta"
        };
        let mut replacement = pin.data.clone().unwrap();
        replacement.product = opposite.into();
        assert_eq!(
            selection::pin_selection(
                &temp.0,
                SelectionSpec {
                    parent: pin.id.clone(),
                    data: Some(replacement),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            "selection.fixed_data_conflict"
        );
    }
    assert_ne!(ids[0], ids[1]);
    assert!(
        selection::load_pinned(&temp.0, &parent.id)
            .unwrap()
            .data
            .is_none()
    );
    let cn_path = temp.0.join("forever-cn-beta");
    assert_eq!(
        selection::execute_project(&RequestContext { home: temp.0.clone(), cwd: temp.0.clone(), control: Default::default() }, &selection::ProjectRequest::Lock { path: Some(cn_path), snapshot: ids[0].clone() })
        .unwrap_err()
        .code,
        "selection.project_conflict"
    );
}
