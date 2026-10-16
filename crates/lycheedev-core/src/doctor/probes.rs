use super::*;
use crate::metadata::historical_write::finish_historical;
fn observed<T>(result: Result<T>, control: &OperationControl) -> Result<T> {
    finish(result, control)
}
pub(super) fn metadata(inv: &Invocation) -> Result<Value> {
    let result = (|| {
        let metadata = Metadata::open_controlled(&inv.home, false, &inv.control)?;
        let version = metadata.schema_version_controlled(&inv.control);
        let result = finish_historical(version, metadata.close());
        observed(result, &inv.control)
    })();
    let result = diagnostic(result, &inv.control)?;
    Ok(match result {
        Ok(version) => check(
            "vault.metadata",
            true,
            "ok",
            "",
            format!("metadata readable; schema version {version}"),
            "",
        ),
        Err(error) if error.code == "vault.record_missing" && error.details.is_null() => check(
            "vault.metadata",
            false,
            "warn",
            "vault.record_missing",
            "metadata database not created yet",
            "run a command that records state, or ignore for a brand-new workspace",
        ),
        Err(error)
            if [
                "vault.schema_newer",
                "vault.schema_invalid",
                "vault.unsupported_workspace",
            ]
            .contains(&error.code.as_str()) =>
        {
            check(
                "vault.metadata",
                false,
                "error",
                "vault.workspace_schema_newer",
                format!("metadata schema is newer or corrupt: {}", error.message),
                "upgrade the toolkit; unsafe writes are refused",
            )
        }
        Err(error) => check(
            "vault.metadata",
            false,
            "error",
            "vault.unsupported_workspace",
            error.message,
            "inspect state/toolkit.sqlite",
        ),
    })
}
fn cleanup(
    result: Result<()>,
    errors: Vec<ToolkitError>,
    control: &OperationControl,
) -> Result<()> {
    let mut causes = Vec::new();
    if let Err(error) = result {
        causes.push(error)
    }
    causes.extend(errors);
    if let Err(error) = control.check() {
        if !causes.iter().any(|e| {
            e.code == error.code
                && e.message == error.message
                && e.exit_code == error.exit_code
                && e.details == error.details
        }) {
            causes.push(error)
        }
    }
    if causes.is_empty() {
        return Ok(());
    }
    let mut first = causes[0].clone();
    if causes.len() > 1 {
        first.message = causes
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        first.details = json!({"schema":"lycheedev.doctor-cleanup.v1","causes":causes});
    }
    Err(first)
}
pub(super) fn writable(inv: &Invocation) -> Result<Value> {
    use lycheedev_windows::filesystem::owned_cleanup::OwnedTemporary;
    inv.control.check()?;
    let path = inv.home.join("tmp/.lycheedev-write-probe");
    let opened = (|| {
        storage::guard_path_with_check(&inv.home, &path, || inv.control.check())?;
        Ok(OwnedTemporary::create(&path)?)
    })();
    let mut file = match opened {
        Ok(file) => file,
        Err(error) => {
            let error = observed::<()>(Err(error), &inv.control).unwrap_err();
            if inv.control.check().is_err() {
                return Err(error);
            }
            return Ok(check(
                "vault.writable",
                false,
                "error",
                "vault.workspace_readonly",
                format!("cannot create a probe file in tmp/: {}", error.message),
                "check filesystem permissions and re-run",
            ));
        }
    };
    let result = (|| {
        inv.control.check()?;
        file.file_mut().write_all(b"probe")?;
        inv.control.check()
    })();
    // Disposition is bound to CREATE_NEW's exact handle, including write/stop
    // failure. A substituted pathname never grants deletion authority. Marking
    // before checked close is the existing Windows ownership safety mechanism.
    let (file, removed) = file.discard();
    let closed = storage::close_file(file);
    // Preserve the Go public write/close/remove cause order although native
    // identity-bound disposal must be requested before releasing the handle.
    let mut errors = Vec::new();
    if let Err(e) = closed {
        errors.push(e.into())
    }
    if let Err(e) = removed {
        errors.push(e.into())
    }
    let result = diagnostic(cleanup(result, errors, &inv.control), &inv.control)?;
    Ok(match result {
        Ok(()) => check(
            "vault.writable",
            true,
            "ok",
            "",
            "workspace tmp/ accepts writes",
            "",
        ),
        Err(error) => check(
            "vault.writable",
            false,
            "error",
            "vault.workspace_readonly",
            format!("write probe failed: {}", error.message),
            "check filesystem permissions and re-run",
        ),
    })
}
pub(super) fn hygiene(inv: &Invocation) -> Result<Vec<Value>> {
    let mut locks = 0usize;
    let _ = cache::walk_files(&inv.home.join("locks"), &inv.control, |_, _| {
        locks += 1;
        Ok(())
    });
    inv.control.check()?;
    let locks = check(
        "vault.lock_hygiene",
        true,
        "ok",
        "",
        format!(
            "{locks} lock files present; lock files are retained on purpose so lock identity never splits"
        ),
        "",
    );
    let mut count = 0usize;
    let _ = cache::walk_files(&inv.home.join("tmp"), &inv.control, |_, _| {
        count += 1;
        Ok(())
    });
    inv.control.check()?;
    let mut oldest: Option<std::time::SystemTime> = None;
    let _ = cache::walk_files(&inv.home.join("tmp"), &inv.control, |_, info| {
        if let Ok(time) = info.and_then(|info| info.modified()) {
            if oldest.is_none_or(|old| time < old) {
                oldest = Some(time)
            }
        }
        Ok(())
    });
    inv.control.check()?;
    let _ = cache::walk_files(&inv.home.join("cache/tmp"), &inv.control, |_, _| {
        count += 1;
        Ok(())
    });
    inv.control.check()?;
    if count == 0 {
        return Ok(vec![
            locks,
            check(
                "vault.tmp_hygiene",
                true,
                "ok",
                "",
                "no leftover temporary files",
                "",
            ),
        ]);
    }
    let mut detail = format!("{count} leftover temporary file(s)");
    if let Some(oldest) = oldest {
        let time: chrono::DateTime<chrono::Utc> = oldest.into();
        detail.push_str(&format!(
            "; oldest modified {}",
            time.format("%Y-%m-%dT%H:%M:%SZ")
        ))
    }
    Ok(vec![
        locks,
        check(
            "vault.tmp_hygiene",
            false,
            "warn",
            "vault.stale_temporary_files",
            detail,
            "confirm no work is running, then remove stale temporary files or run cache prune --uncommitted",
        ),
    ])
}
pub(super) fn cache(inv: &Invocation) -> Result<Value> {
    let result = diagnostic(cache::verify(&inv.home, &inv.control), &inv.control)?;
    Ok(match result {
        Ok(report) => check(
            "vault.cache_integrity",
            report.ok,
            if report.ok { "ok" } else { "error" },
            if report.ok {
                ""
            } else {
                "vault.cache_integrity"
            },
            format!(
                "objects={} bytes={} missing={} corrupt={}",
                report.checked,
                report.bytes,
                report.missing.len(),
                report.corrupt.len()
            ),
            if report.ok {
                ""
            } else {
                "run cache verify for the exact objects and cache prune to reclaim damaged ordinary cache"
            },
        ),
        Err(error) => check(
            "vault.cache_integrity",
            false,
            "error",
            "vault.cache_integrity",
            error.message,
            "run cache verify and inspect the managed cache",
        ),
    })
}
pub(super) fn archive(inv: &Invocation) -> Result<Value> {
    let result = diagnostic(
        crate::archive::journal_controlled(&inv.home, &inv.control),
        &inv.control,
    )?;
    Ok(match result {
        Ok(None) => check(
            "vault.archive_state",
            true,
            "ok",
            "",
            "no archive switch recorded for this root",
            "",
        ),
        Ok(Some(plan)) => {
            let complete = plan.state == "complete";
            check(
                "vault.archive_state",
                complete,
                if complete { "ok" } else { "warn" },
                if complete {
                    ""
                } else {
                    "vault.archive_incomplete"
                },
                format!(
                    "archive switch state={} archivePath={}",
                    plan.state, plan.archive_path
                ),
                if complete {
                    ""
                } else {
                    "resume the interrupted switch (init fresh completes it) before writing to this root"
                },
            )
        }
        Err(error) => check(
            "vault.archive_state",
            false,
            "warn",
            "vault.archive_state",
            error.message,
            "inspect the archive-state journal next to the workspace root",
        ),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_keeps_identical_close_events_and_only_deduplicates_the_stop_observation() {
        let stop = crate::control::TrackedCancellation::new();
        let control = OperationControl::tracked(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            stop.clone(),
        );
        stop.cancel();
        let primary = control.check().unwrap_err();
        let close = ToolkitError::new("external.io", "same close failure", 5);
        let error = cleanup(Err(primary), vec![close.clone(), close], &control).unwrap_err();
        let causes = error.details["causes"].as_array().unwrap();
        assert_eq!(causes.len(), 3);
        assert_eq!(causes[0]["code"], "command.cancelled");
        assert_eq!(causes[1], causes[2]);
    }
}
