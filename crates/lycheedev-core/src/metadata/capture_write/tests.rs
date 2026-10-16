use super::*;
use crate::control::TrackedCancellation;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-capture-write-{}-{}-{}",
            std::process::id(),
            crate::storage::utc_now().replace([':', '.'], ""),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        crate::workspace::initialize(&path).unwrap();
        Self(path)
    }
    fn open(&self) -> Metadata {
        Metadata::open_controlled(&self.0, true, &alive()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn alive() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn budget_stop() -> ToolkitError {
    ToolkitError::new(
        "records.query_resource_budget",
        "independent budget expired",
        3,
    )
    .with_details(serde_json::json!({"dimension":"deadline"}))
}
#[test]
fn capture_transaction_preserves_exact_serialized_bytes_and_generation() {
    let f = Fixture::new();
    let mut m = f.open();
    let state = Cell::new(CaptureWriteState::NotCommitted);
    let raw = br#"{ "literal":"<tag>&", "unknown": [1,2] }"#;
    for _ in 0..2 {
        m.put_capture_serialized(
            "capture/test",
            raw,
            &[],
            &alive(),
            || Ok(()),
            |_| Ok(()),
            |_, _| Ok(()),
            || Ok(()),
            &state,
        )
        .unwrap();
        assert_eq!(state.get(), CaptureWriteState::Committed);
    }
    let stored: (i64, Vec<u8>) = m
        .connection
        .query_row(
            "SELECT generation,value FROM documents WHERE key='capture/test'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, (1, raw.to_vec()));
    m.close().unwrap();
}
#[test]
fn capture_reference_failure_never_publishes_and_checked_rollback_releases_writer() {
    let f = Fixture::new();
    let mut m = f.open();
    let state = Cell::new(CaptureWriteState::NotCommitted);
    let published = Cell::new(0);
    let error = m
        .put_capture_serialized(
            "capture/owner",
            b"{}",
            &["capture/missing".into()],
            &alive(),
            || Ok(()),
            |_| Ok(()),
            |_, _| Ok(()),
            || {
                published.set(published.get() + 1);
                Ok(())
            },
            &state,
        )
        .unwrap_err();
    assert_eq!(error.code, "vault.record_missing");
    assert_eq!(published.get(), 0);
    assert_eq!(state.get(), CaptureWriteState::NotCommitted);
    assert!(m.connection.is_autocommit());
    m.put_capture_serialized(
        "capture/owner",
        b"{}",
        &[],
        &alive(),
        || Ok(()),
        |_| Ok(()),
        |_, _| Ok(()),
        || Ok(()),
        &state,
    )
    .unwrap();
    m.close().unwrap();
}
#[test]
fn capture_begin_and_commit_busy_observe_independent_budget_without_replaying_publish() {
    for commit in [false, true] {
        let f = Fixture::new();
        let mut m = f.open();
        m.put("seed", &true, None).unwrap();
        let blocker = Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
        blocker
            .execute_batch(if commit {
                "BEGIN; SELECT * FROM documents;"
            } else {
                "BEGIN IMMEDIATE;"
            })
            .unwrap();
        let invocation = alive();
        let stop = Cell::new(false);
        let attempts = Cell::new(0);
        let published = Cell::new(0);
        let state = Cell::new(CaptureWriteState::NotCommitted);
        let error = m
            .put_capture_serialized_observed(
                "capture/owner",
                b"{}",
                &[],
                &invocation,
                || {
                    if stop.get() {
                        Err(budget_stop())
                    } else {
                        Ok(())
                    }
                },
                |_| Ok(()),
                |_, _| Ok(()),
                || {
                    published.set(published.get() + 1);
                    Ok(())
                },
                &state,
                &mut || {
                    attempts.set(attempts.get() + 1);
                    stop.set(true)
                },
            )
            .unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("records.query_resource_budget", 3)
        );
        assert_eq!(attempts.get(), 1);
        assert_eq!(published.get(), usize::from(commit));
        assert_eq!(state.get(), CaptureWriteState::NotCommitted);
        assert!(m.connection.is_autocommit());
        invocation.check().unwrap();
        blocker.execute_batch("ROLLBACK").unwrap();
        blocker.close().unwrap();
        assert_eq!(m.generation("capture/owner").unwrap(), None);
        m.close().unwrap();
    }
}
#[test]
fn capture_commit_retry_only_retries_commit_not_publication() {
    let f = Fixture::new();
    let mut m = f.open();
    m.put("seed", &true, None).unwrap();
    let blocker = Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
    blocker
        .execute_batch("BEGIN; SELECT * FROM documents;")
        .unwrap();
    let state = Cell::new(CaptureWriteState::NotCommitted);
    let published = Cell::new(0);
    let busy = Cell::new(0);
    m.put_capture_serialized_observed(
        "capture/owner",
        b"{}",
        &[],
        &alive(),
        || Ok(()),
        |_| Ok(()),
        |_, _| Ok(()),
        || {
            published.set(published.get() + 1);
            Ok(())
        },
        &state,
        &mut || {
            busy.set(busy.get() + 1);
            blocker.execute_batch("ROLLBACK").unwrap();
        },
    )
    .unwrap();
    assert_eq!(published.get(), 1);
    assert_eq!(busy.get(), 1);
    assert_eq!(state.get(), CaptureWriteState::Committed);
    blocker.close().unwrap();
    m.close().unwrap();
}
#[test]
fn capture_successful_commit_is_recorded_before_post_commit_budget_stop() {
    let f = Fixture::new();
    let mut m = f.open();
    let state = Cell::new(CaptureWriteState::NotCommitted);
    let invocation = alive();
    let error = m
        .put_capture_serialized(
            "capture/owner",
            b"{}",
            &[],
            &invocation,
            || {
                if state.get() == CaptureWriteState::Committed {
                    Err(budget_stop())
                } else {
                    Ok(())
                }
            },
            |_| Ok(()),
            |_, _| Ok(()),
            || Ok(()),
            &state,
        )
        .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(state.get(), CaptureWriteState::Committed);
    assert_eq!(m.generation("capture/owner").unwrap(), Some(1));
    invocation.check().unwrap();
    m.close().unwrap();
}
#[test]
fn capture_invocation_cancel_and_independent_budget_keep_both_typed_causes() {
    let f = Fixture::new();
    let mut m = f.open();
    let cancel = TrackedCancellation::new();
    let invocation =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), cancel.clone());
    let state = Cell::new(CaptureWriteState::NotCommitted);
    let stop = Cell::new(false);
    let error = m
        .put_capture_serialized(
            "capture/owner",
            b"{}",
            &[],
            &invocation,
            || {
                if stop.get() {
                    Err(budget_stop())
                } else {
                    Ok(())
                }
            },
            |_| Ok(()),
            |_, _| Ok(()),
            || {
                cancel.cancel();
                stop.set(true);
                Ok(())
            },
            &state,
        )
        .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(
        error
            .details
            .to_string()
            .contains("records.query_resource_budget")
    );
    assert_eq!(state.get(), CaptureWriteState::NotCommitted);
    assert!(m.connection.is_autocommit());
    m.close().unwrap();
}
#[test]
fn capture_conflict_is_not_retried_or_misreported_as_committed() {
    let f = Fixture::new();
    let mut m = f.open();
    m.put("capture/owner", &false, None).unwrap();
    let state = Cell::new(CaptureWriteState::NotCommitted);
    let busy = Cell::new(0);
    let error = m
        .put_capture_serialized_observed(
            "capture/owner",
            b"true",
            &[],
            &alive(),
            || Ok(()),
            |_| Ok(()),
            |_, _| Ok(()),
            || Ok(()),
            &state,
            &mut || busy.set(busy.get() + 1),
        )
        .unwrap_err();
    assert_eq!(error.code, "vault.generation_conflict");
    assert_eq!(busy.get(), 0);
    assert_eq!(state.get(), CaptureWriteState::NotCommitted);
    assert!(m.connection.is_autocommit());
    m.close().unwrap();
}

#[test]
fn capture_busy_wait_observes_independent_deadline_without_new_invocation_deadline() {
    let f = Fixture::new();
    let mut metadata = f.open();
    let blocker = Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let invocation = alive();
    let deadline = Cell::new(None::<Instant>);
    let attempts = Cell::new(0);
    let state = Cell::new(CaptureWriteState::NotCommitted);
    let error = metadata
        .put_capture_serialized_observed(
            "capture/deadline",
            b"{}",
            &[],
            &invocation,
            || {
                if deadline.get().is_some_and(|end| Instant::now() >= end) {
                    Err(budget_stop())
                } else {
                    Ok(())
                }
            },
            |_| Ok(()),
            |_, _| Ok(()),
            || panic!("busy BEGIN must not publish"),
            &state,
            &mut || {
                attempts.set(attempts.get() + 1);
                if deadline.get().is_none() {
                    deadline.set(Some(Instant::now() + Duration::from_millis(20)));
                }
            },
        )
        .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    assert!(attempts.get() > 0);
    assert_eq!(state.get(), CaptureWriteState::NotCommitted);
    invocation.check().unwrap();
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    metadata.close().unwrap();
}
