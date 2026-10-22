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

pub(super) fn prune(
    context: &crate::request::RequestContext,
    target_bytes: u64,
    dry_run: bool,
    max_objects: usize,
    include_uncommitted: bool,
) -> Result<Outcome> {
    prune_observed(context, target_bytes, dry_run, max_objects, include_uncommitted, |_| {})
}

fn prune_observed(
    context: &crate::request::RequestContext,
    target_bytes: u64,
    dry_run: bool,
    max_objects: usize,
    include_uncommitted: bool,
    observe: impl FnMut(Phase),
) -> Result<Outcome> {
    prune_with_leases(context, target_bytes, dry_run, max_objects, include_uncommitted, observe, |resource| {
        ExclusiveLock::resource_controlled(&context.home.join("locks"), resource, &context.control)
    })
}

fn prune_with_leases(
    context: &crate::request::RequestContext,
    target_bytes: u64,
    dry_run: bool,
    bound: usize,
    include_uncommitted: bool,
    mut observe: impl FnMut(Phase),
    mut acquire: impl FnMut(&str) -> Result<ExclusiveLock>,
) -> Result<Outcome> {
    let mut report = Report {
        target_bytes,
        dry_run,
        ..Report::default()
    };
    let check = &mut |phase| {
        observe(phase);
        context.control.check()
    };
    let result = (|| {
        workspace::open_controlled(&context.home, &context.control)?;
        let (objects, foreign) = objects_checked(&context.home, || check(Phase::ScanObjects))?;
        let staging = stages_checked(&context.home, || check(Phase::ScanStaging))?;
        let (_, _, marker_foreign) = markers_checked(&context.home, || check(Phase::ScanMarkers))?;
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
                    if retain_checked(&context.home, &object.digest, || context.control.check())?
                        .is_some_and(|value| !value["owners"].as_array().unwrap().is_empty())
                    {
                        report.skipped_protected += 1;
                        return Ok(());
                    }
                    storage::guard_path_with_check(&context.home, &object.path, || {
                        context.control.check()
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
                if !include_uncommitted {
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
                    storage::guard_path_with_check(&context.home, path, || context.control.check())?;
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
            context.control.check()?;
            debug_assert_eq!(report.after_bytes, object_bytes + staging_bytes);
            report.complete = !report.truncated && object_bytes <= report.target_bytes;
            Ok(())
        })();
        crate::selection::selection_finish(result, quota.close())?;
        context.control.check()
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
