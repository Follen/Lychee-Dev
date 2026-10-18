//! Frozen Go 3766f9c internal/vault/cache.go:477–631. Execution NOT_RUN.
use super::*;
use crate::control::OperationControl;
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct Fixture {
    base: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "lychee-prune-contract-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let home = base.join("home");
        workspace::initialize(&home).unwrap();
        Self { base, home }
    }
    fn invocation(&self, flags: &[(&str, &str)]) -> Invocation {
        Invocation {
            command: "cache prune".into(),
            positional: None,
            flags: flags
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect(),
            home: self.home.clone(),
            cwd: self.base.clone(),
            control: OperationControl::default(),
        }
    }
    fn object(&self, bytes: &[u8]) -> (String, PathBuf) {
        let digest = storage::sha256(bytes);
        let path = self
            .home
            .join("cache/objects")
            .join(&digest[..2])
            .join(&digest[2..]);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        (digest, path)
    }
    fn stage(&self, id: &str, bytes: &[u8]) -> PathBuf {
        let path = self.home.join("cache/tmp").join(format!("put-{id}.part"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn protect(&self, digest: &str) {
        storage::atomic_json(&self.home.join("cache/retain").join(format!("{digest}.json")),
            &json!({"sha256":digest,"owners":[{"kind":"pinned","owner":"fixture","retainedAt":"2026-10-17T00:00:00Z"}]})).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
fn totals(value: &Value, before: u64, after: u64, reclaimed: u64) {
    assert_eq!(value["beforeBytes"], before);
    assert_eq!(value["afterBytes"], after);
    assert_eq!(value["reclaimedBytes"], reclaimed);
}
#[test]
fn staging_only_totals_include_preserved_deleted_and_dry_run_bytes() {
    for (include, dry) in [(false, false), (true, false), (false, true), (true, true)] {
        let f = Fixture::new();
        let stage = f.stage(&"a".repeat(32), b"staging");
        let mut inv = f.invocation(&[]);
        if include {
            inv.flags.insert("--uncommitted".into(), "true".into());
        }
        if dry {
            inv.flags.insert("--dry-run".into(), "true".into());
        }
        let out = dispatch(&inv).unwrap();
        totals(
            &out.data,
            7,
            if include { 0 } else { 7 },
            if include { 7 } else { 0 },
        );
        assert_eq!(out.data["removedObjects"], 0);
        assert_eq!(out.data["removedUncommitted"], usize::from(include));
        assert_eq!(out.data["skippedUncommitted"], usize::from(!include));
        assert_eq!(out.data["complete"], true);
        assert_eq!(out.data["truncated"], false);
        assert_eq!(out.data["dryRun"], dry);
        assert_eq!(stage.exists(), !include || dry);
        assert!(out.complete);
    }
}
#[test]
fn mixed_totals_preserve_protected_busy_and_foreign_staging_in_both_modes() {
    for dry in [false, true] {
        let f = Fixture::new();
        let (protected, protected_path) = f.object(b"keep");
        f.protect(&protected);
        let (busy, busy_path) = f.object(b"busy!");
        let lease =
            ExclusiveLock::resource(&f.home.join("locks"), &format!("cache:obj:{busy}")).unwrap();
        let (_, removed) = f.object(b"remove");
        let staged = f.stage(&"b".repeat(32), b"staging");
        let foreign = f.home.join("cache/tmp/foreign.tmp");
        fs::write(&foreign, b"???").unwrap();
        let mut inv = f.invocation(&[("--uncommitted", "true")]);
        if dry {
            inv.flags.insert("--dry-run".into(), "true".into());
        }
        let out = dispatch(&inv).unwrap();
        totals(&out.data, 25, 12, 13);
        assert_eq!(out.data["removedObjects"], 1);
        assert_eq!(out.data["removedUncommitted"], 1);
        assert_eq!(out.data["skippedProtected"], 1);
        assert_eq!(out.data["skippedInUse"], 1);
        assert_eq!(out.data["skippedForeign"], 1);
        assert_eq!(out.data["skippedUncommitted"], 0);
        assert_eq!(out.data["truncated"], false);
        assert!(!out.complete);
        assert!(protected_path.exists() && busy_path.exists() && foreign.exists());
        assert_eq!(removed.exists(), dry);
        assert_eq!(staged.exists(), dry);
        lease.close().unwrap();
    }
}
#[test]
fn target_applies_to_objects_while_object_bound_still_allows_staging_reclamation() {
    let f = Fixture::new();
    let (_, one) = f.object(b"aa");
    let (_, two) = f.object(b"bb");
    let stage = f.stage(&"c".repeat(32), b"staging");
    let out =
        dispatch(&f.invocation(&[("--max-objects", "1"), ("--uncommitted", "true")])).unwrap();
    totals(&out.data, 11, 2, 9);
    assert_eq!(out.data["removedObjects"], 1);
    assert_eq!(out.data["removedUncommitted"], 1);
    assert_eq!(out.data["truncated"], true);
    assert!(!out.complete);
    assert_ne!(one.exists(), two.exists());
    assert!(!stage.exists());

    let stage = f.stage(&"d".repeat(32), b"staging");
    let out = dispatch(&f.invocation(&[("--target-bytes", "2")])).unwrap();
    totals(&out.data, 9, 9, 0);
    assert!(out.complete && stage.exists());
}
#[test]
fn original_cancelled_or_expired_control_never_starts_pruning() {
    let f = Fixture::new();
    let (_, object) = f.object(b"object");
    let stage = f.stage(&"e".repeat(32), b"staging");
    for (cancelled, deadline, expected) in [
        (true, false, "command.cancelled"),
        (false, true, "command.deadline"),
    ] {
        let mut inv = f.invocation(&[("--uncommitted", "true")]);
        inv.control = OperationControl::new(
            if deadline {
                Instant::now() - Duration::from_secs(1)
            } else {
                Instant::now() + Duration::from_secs(60)
            },
            Arc::new(AtomicBool::new(cancelled)),
        );
        let error = dispatch(&inv).unwrap_err();
        assert_eq!(error.code, expected);
        assert_eq!(error.details["result"]["removedObjects"], 0);
        assert_eq!(error.details["result"]["removedUncommitted"], 0);
        assert_eq!(error.details["result"]["complete"], false);
        assert!(object.exists() && stage.exists());
        assert_eq!(fs::read_dir(f.home.join("locks")).unwrap().count(), 0);
    }
}
#[test]
fn cancellation_at_real_scan_lease_and_unlink_boundaries_preserves_committed_counts() {
    for phase in [
        Phase::ScanObjects,
        Phase::ScanStaging,
        Phase::ScanMarkers,
        Phase::QuotaLease,
        Phase::ObjectLease,
        Phase::ObjectLocked,
        Phase::ObjectUnlink,
        Phase::ObjectRemoved,
        Phase::StageLease,
        Phase::StageLocked,
        Phase::StageUnlink,
        Phase::StageRemoved,
    ] {
        let f = Fixture::new();
        let (digest, object) = f.object(b"object");
        storage::atomic_json(
            &f.home.join("cache/retain").join(format!("{digest}.json")),
            &json!({"sha256":digest,"owners":[]}),
        )
        .unwrap();
        let stage_id = "f".repeat(32);
        let stage = f.stage(&stage_id, b"staging");
        let parent = OperationControl::default();
        let signal = parent.tracked_cancel_handle().unwrap();
        let mut inv = f.invocation(&[("--uncommitted", "true")]);
        inv.control = parent.ordered_child(parent.deadline()).unwrap();
        let mut seen = 0;
        let scanning = matches!(
            phase,
            Phase::ScanObjects | Phase::ScanStaging | Phase::ScanMarkers
        );
        let error = prune_observed(&inv, |at| {
            if at == phase {
                seen += 1;
                // Stop inside traversal, not merely before the scanner starts.
                if seen == if scanning { 2 } else { 1 } {
                    signal.cancel();
                }
            }
        })
        .unwrap_err();
        assert_eq!(error.code, "command.cancelled", "{phase:?}");
        assert!(seen > 0);
        let result = &error.details["result"];
        let object_removed = matches!(
            phase,
            Phase::ObjectRemoved
                | Phase::StageLease
                | Phase::StageLocked
                | Phase::StageUnlink
                | Phase::StageRemoved
        );
        let stage_removed = phase == Phase::StageRemoved;
        assert_eq!(object.exists(), !object_removed, "{phase:?}");
        assert_eq!(stage.exists(), !stage_removed, "{phase:?}");
        assert_eq!(result["removedObjects"], usize::from(object_removed));
        assert_eq!(result["removedUncommitted"], usize::from(stage_removed));
        let reclaimed = u64::from(object_removed) * 6 + u64::from(stage_removed) * 7;
        totals(
            result,
            if scanning { 0 } else { 13 },
            if scanning { 0 } else { 13 - reclaimed },
            reclaimed,
        );
        assert_eq!(result["complete"], false);
        for resource in [
            "cache:quota".to_owned(),
            format!("cache:obj:{digest}"),
            format!("cache:stage:{stage_id}"),
        ] {
            ExclusiveLock::resource(&f.home.join("locks"), &resource)
                .unwrap()
                .close()
                .unwrap();
        }
    }
}
#[test]
fn dry_run_cancellation_reports_projections_without_removing_any_file() {
    let f = Fixture::new();
    let (_, object) = f.object(b"object");
    let stage = f.stage(&"a".repeat(32), b"staging");
    let inv = f.invocation(&[("--uncommitted", "true"), ("--dry-run", "true")]);
    let signal = inv.control.tracked_cancel_handle().unwrap();
    let error = prune_observed(&inv, |phase| {
        if phase == Phase::StageRemoved {
            signal.cancel();
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    totals(&error.details["result"], 13, 0, 13);
    assert_eq!(error.details["result"]["dryRun"], true);
    assert_eq!(error.details["result"]["complete"], false);
    assert!(object.exists() && stage.exists());
}
#[test]
fn unlink_failure_retains_prior_object_deletion_and_original_error() {
    let f = Fixture::new();
    let (_, object) = f.object(b"object");
    let stage = f.stage(&"b".repeat(32), b"staging");
    let preserved = stage.with_extension("preserved");
    let inv = f.invocation(&[("--uncommitted", "true")]);
    let error = prune_observed(&inv, |phase| {
        if phase == Phase::StageUnlink {
            fs::rename(&stage, &preserved).unwrap();
            fs::create_dir(&stage).unwrap();
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "external.io");
    totals(&error.details["result"], 13, 7, 6);
    assert_eq!(error.details["result"]["removedObjects"], 1);
    assert_eq!(error.details["result"]["removedUncommitted"], 0);
    assert_eq!(error.details["result"]["complete"], false);
    assert!(!object.exists());
    assert_eq!(fs::read(&preserved).unwrap(), b"staging");
}

#[test]
fn busy_staging_remains_accounted_while_unrelated_evidence_is_never_pruned() {
    let f = Fixture::new();
    let id = "c".repeat(32);
    let busy = f.stage(&id, b"staging");
    let available = f.stage(&"d".repeat(32), b"other");
    let evidence = f.home.join("blobs/evidence");
    fs::write(&evidence, b"retained evidence").unwrap();
    let lease =
        ExclusiveLock::resource(&f.home.join("locks"), &format!("cache:stage:{id}")).unwrap();
    let out = dispatch(&f.invocation(&[("--uncommitted", "true")])).unwrap();
    totals(&out.data, 12, 7, 5);
    assert_eq!(out.data["removedUncommitted"], 1);
    assert_eq!(out.data["skippedInUse"], 1);
    assert!(out.complete && busy.exists());
    assert!(!available.exists());
    assert_eq!(fs::read(&evidence).unwrap(), b"retained evidence");
    lease.close().unwrap();
}

#[test]
fn compound_busy_and_checked_close_errors_are_not_swallowed_as_skipped_leases() {
    for fail_staging in [false, true] {
        let f = Fixture::new();
        let (_, object) = f.object(b"object");
        let stage = f.stage(&"e".repeat(32), b"staging");
        let inv = f.invocation(&[("--uncommitted", "true")]);
        // Model the exact acquisition error composition, not a successful
        // handle or a claimed operating-system close failure. The checked
        // lock owner has independent one-close tests in storage.
        let acquisition_error = crate::metadata::historical_write::finish_historical::<()>(
            Err(ToolkitError::new("workspace.busy", "lock contended", 6)),
            Err(ToolkitError::io(std::io::Error::other(
                "injected close failure",
            ))),
        )
        .unwrap_err();
        let error = prune_with_leases(
            &inv,
            |_| {},
            |resource| {
                if resource.starts_with(if fail_staging {
                    "cache:stage:"
                } else {
                    "cache:obj:"
                }) {
                    Err(acquisition_error.clone())
                } else {
                    ExclusiveLock::resource_controlled(
                        &f.home.join("locks"),
                        resource,
                        &inv.control,
                    )
                }
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "workspace.busy");
        assert_eq!(error.message, acquisition_error.message);
        assert_eq!(
            error.details["primary"],
            serde_json::to_value(acquisition_error).unwrap()
        );
        let report = &error.details["result"];
        totals(
            report,
            13,
            if fail_staging { 7 } else { 13 },
            if fail_staging { 6 } else { 0 },
        );
        assert_eq!(report["skippedInUse"], 0);
        assert_eq!(report["removedObjects"], usize::from(fail_staging));
        assert_eq!(report["removedUncommitted"], 0);
        assert_eq!(report["complete"], false);
        assert_eq!(object.exists(), !fail_staging);
        assert!(stage.exists());
        ExclusiveLock::resource(&f.home.join("locks"), "cache:quota")
            .unwrap()
            .close()
            .unwrap();
    }
}
