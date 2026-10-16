use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-listfile-row-{}-{}-{}",
            std::process::id(),
            crate::storage::utc_now().replace(':', "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        crate::workspace::initialize(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        } else {
            eprintln!("retained listfile-row fixture {}", self.0.display());
        }
    }
}
#[test]
fn listfile_owner_read_returns_exact_raw_tokens_and_generation_without_decode() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let mut m = Metadata::open_controlled(&f.0, true, &control).unwrap();
    let raw =
        serde_json::value::RawValue::from_string(r#"{"a":1,"a":null,"n":-0}"#.into()).unwrap();
    m.create_controlled("listfile/test", &raw, &control)
        .unwrap();
    let document = m
        .read_document_on_owner_controlled("listfile/test", &control)
        .unwrap();
    assert_eq!(document.generation, 1);
    assert_eq!(document.bytes, raw.get().as_bytes());
    // Even malformed historical JSON remains raw at this boundary, just as the
    // generation-only Go refresh read; the actual cache decoder owns admission.
    m.connection
        .execute(
            "UPDATE documents SET value=?1,generation=2 WHERE key='listfile/test'",
            [b"not JSON".as_slice()],
        )
        .unwrap();
    let document = m
        .read_document_on_owner_controlled("listfile/test", &control)
        .unwrap();
    assert_eq!(document.generation, 2);
    assert_eq!(document.bytes, b"not JSON");
    m.close().unwrap();
}
#[test]
fn listfile_owner_row_read_observes_real_busy_cancel_and_keeps_handle_closeable() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let m = Metadata::open_controlled(&f.0, true, &control).unwrap();
    let lock = Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut busy = false;
    let error = m
        .read_document_on_owner_controlled_observed("x", &control, &mut || {
            busy = true;
            cancel.cancel();
        })
        .unwrap_err();
    assert!(busy);
    assert_eq!(error.code, "command.cancelled");
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    m.close().unwrap();
}
#[test]
fn listfile_owner_row_bound_and_original_deadline_precede_missing_classification() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let m = Metadata::open_controlled(&f.0, true, &control).unwrap();
    m.connection
        .execute(
            "INSERT INTO documents(key,generation,value) VALUES('large',1,?1)",
            [vec![b' '; 1048577]],
        )
        .unwrap();
    assert!(
        m.read_document_on_owner_controlled("large", &control)
            .unwrap_err()
            .message
            .contains("1 MiB")
    );
    assert_eq!(
        m.read_document_on_owner_controlled("missing", &control)
            .unwrap_err()
            .code,
        "vault.record_missing"
    );
    let expired = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        m.read_document_on_owner_controlled("missing", &expired)
            .unwrap_err()
            .code,
        "command.deadline"
    );
    m.close().unwrap();
}

#[test]
fn owner_read_busy_observes_independent_check_without_replacing_invocation() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let owner = Metadata::open_controlled(&f.0, true, &control).unwrap();
    let lock = Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let busy = std::cell::Cell::new(false);
    let error = owner
        .read_document_on_owner_controlled_impl(
            "x",
            &control,
            &mut || {
                if busy.get() {
                    Err(ToolkitError::new(
                        "records.query_resource_budget",
                        "independent deadline",
                        3,
                    ))
                } else {
                    Ok(())
                }
            },
            &mut || busy.set(true),
        )
        .unwrap_err();
    assert!(busy.get());
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    control.check().unwrap();
    assert!(control.stop_cause().unwrap().is_none());
    lock.execute_batch("ROLLBACK").unwrap();
    drop(lock);
    owner.close().unwrap();
}
#[test]
fn owner_read_preserves_original_stop_before_independent_stop() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let owner = Metadata::open_controlled(&f.0, true, &control).unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    let calls = std::cell::Cell::new(0);
    let error = owner
        .read_document_on_owner_controlled_with_check("missing", &control, || {
            calls.set(calls.get() + 1);
            Err(ToolkitError::new(
                "records.query_resource_budget",
                "independent",
                3,
            ))
        })
        .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(calls.get(), 0);
    owner.close().unwrap();
}
