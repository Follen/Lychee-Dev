//! Pruning keeps the caller's stop scope and reports completed deletions even
//! when a later scan, lease, unlink, or checked close fails.
use super::*;
use serde::Serialize;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    target_bytes: u64,
    before_bytes: u64,
    after_bytes: u64,
    reclaimed_bytes: u64,
    removed_objects: usize,
    removed_uncommitted: usize,
    skipped_protected: usize,
    skipped_in_use: usize,
    skipped_uncommitted: usize,
    skipped_foreign: usize,
    truncated: bool,
    complete: bool,
    dry_run: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    ScanObjects,
    ScanStaging,
    ScanMarkers,
    QuotaLease,
    ObjectLease,
    ObjectLocked,
    ObjectUnlink,
    ObjectRemoved,
    StageLease,
    StageLocked,
    StageUnlink,
    StageRemoved,
}

pub(super) fn prune(inv: &Invocation) -> Result<Outcome> {
    prune_observed(inv, |_| {})
}

fn prune_observed(inv: &Invocation, observe: impl FnMut(Phase)) -> Result<Outcome> {
    prune_with_leases(inv, observe, |resource| {
        ExclusiveLock::resource_controlled(&inv.home.join("locks"), resource, &inv.control)
    })
}

fn prune_with_leases(
    inv: &Invocation,
    mut observe: impl FnMut(Phase),
    mut acquire: impl FnMut(&str) -> Result<ExclusiveLock>,
) -> Result<Outcome> {
    let mut report = Report {
        target_bytes: inv.usize_flag("--target-bytes", 0, 0, usize::MAX)? as u64,
        dry_run: inv.flag_bool("--dry-run"),
        ..Report::default()
    };
    let bound = inv.usize_flag("--max-objects", 0, 0, 100000)?;
    let check = &mut |phase| {
        observe(phase);
        inv.control.check()
    };
    let result = (|| {
        workspace::open_controlled(&inv.home, &inv.control)?;
        let (objects, foreign) = objects_checked(&inv.home, || check(Phase::ScanObjects))?;
        let staging = stages_checked(&inv.home, || check(Phase::ScanStaging))?;
        let (_, _, marker_foreign) = markers_checked(&inv.home, || check(Phase::ScanMarkers))?;
        let mut object_bytes = sum_bytes(objects.iter().map(|object| object.bytes))?;
        let mut staging_bytes = sum_bytes(staging.iter().map(|(_, bytes, _)| *bytes))?;
        report.before_bytes = sum_bytes([object_bytes, staging_bytes])?;
        report.after_bytes = report.before_bytes;
        report.skipped_foreign = foreign + marker_foreign;
        check(Phase::QuotaLease)?;
        let quota = acquire("cache:quota")?;
        let result = (|| {
            for object in &objects {
                check(Phase::ObjectLease)?;
                if object_bytes <= report.target_bytes {
                    break;
                }
                if bound != 0 && report.removed_objects >= bound {
                    report.truncated = true;
                    break;
                }
                let lease = match acquire(&format!("cache:obj:{}", object.digest)) {
                    Ok(lease) => lease,
                    Err(error) if error.code == "workspace.busy" && error.details.is_null() => {
                        report.skipped_in_use += 1;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let result = (|| {
                    check(Phase::ObjectLocked)?;
                    if retain_checked(&inv.home, &object.digest, || inv.control.check())?
                        .is_some_and(|value| !value["owners"].as_array().unwrap().is_empty())
                    {
                        report.skipped_protected += 1;
                        return Ok(());
                    }
                    storage::guard_path_with_check(&inv.home, &object.path, || {
                        inv.control.check()
                    })?;
                    check(Phase::ObjectUnlink)?;
                    if !report.dry_run {
                        remove_if_present(&object.path)?;
                    }
                    // Account immediately after the operation. A later cancellation
                    // or close error must not pretend this unlink was rolled back.
                    object_bytes -= object.bytes;
                    report.after_bytes -= object.bytes;
                    report.reclaimed_bytes += object.bytes;
                    report.removed_objects += 1;
                    check(Phase::ObjectRemoved)
                })();
                crate::selection::selection_finish(result, lease.close())?;
            }
            for (path, bytes, name) in &staging {
                check(Phase::StageLease)?;
                if !inv.flag_bool("--uncommitted") {
                    report.skipped_uncommitted += 1;
                    continue;
                }
                let Some(id) = name
                    .strip_prefix("put-")
                    .and_then(|name| name.strip_suffix(".part"))
                    .filter(|id| valid_digest(id, 32))
                else {
                    report.skipped_foreign += 1;
                    continue;
                };
                let lease = match acquire(&format!("cache:stage:{id}")) {
                    Ok(lease) => lease,
                    Err(error) if error.code == "workspace.busy" && error.details.is_null() => {
                        report.skipped_in_use += 1;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let result = (|| {
                    check(Phase::StageLocked)?;
                    storage::guard_path_with_check(&inv.home, path, || inv.control.check())?;
                    check(Phase::StageUnlink)?;
                    if !report.dry_run {
                        remove_if_present(path)?;
                    }
                    staging_bytes -= bytes;
                    report.after_bytes -= bytes;
                    report.reclaimed_bytes += bytes;
                    report.removed_uncommitted += 1;
                    check(Phase::StageRemoved)
                })();
                crate::selection::selection_finish(result, lease.close())?;
            }
            inv.control.check()?;
            debug_assert_eq!(report.after_bytes, object_bytes + staging_bytes);
            report.complete = !report.truncated && object_bytes <= report.target_bytes;
            Ok(())
        })();
        crate::selection::selection_finish(result, quota.close())?;
        inv.control.check()
    })();
    if let Err(mut error) = result {
        report.complete = false;
        let primary = error.clone();
        error.details =
            json!({"schema":"lycheedev.cache-prune-error.v1","primary":primary,"result":report});
        return Err(error);
    }
    let complete = report.complete;
    let mut outcome = Outcome::new(serde_json::to_value(report)?);
    outcome.complete = complete;
    Ok(outcome)
}

fn sum_bytes(bytes: impl IntoIterator<Item = u64>) -> Result<u64> {
    bytes.into_iter().try_fold(0u64, |total, bytes| {
        total
            .checked_add(bytes)
            .ok_or_else(|| ToolkitError::invalid("cache byte count overflow"))
    })
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests;
