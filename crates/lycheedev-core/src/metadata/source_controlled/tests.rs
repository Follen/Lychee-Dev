use super::*;
use crate::control::TrackedCancellation;
use serde_json::{Value, json};
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
            "lychee-source-metadata-{}-{}-{}",
            std::process::id(),
            crate::storage::sha256(crate::storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        crate::workspace::initialize(&path).unwrap();
        Self(path)
    }
    fn store(&self) -> Metadata {
        Metadata::open_controlled(&self.0, true, &OperationControl::default()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn tracked() -> (OperationControl, TrackedCancellation) {
    let token = TrackedCancellation::new();
    (
        OperationControl::tracked(Instant::now() + Duration::from_secs(10), token.clone()),
        token,
    )
}
#[test]
fn generation_uses_original_sticky_control_and_does_not_mutate() {
    let f = Fixture::new();
    let mut store = f.store();
    let (control, token) = tracked();
    store
        .put_controlled("source/test", &json!({"v":1}), None, &control)
        .unwrap();
    assert_eq!(
        store
            .generation_controlled("source/test", &control)
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        store.generation_controlled("missing", &control).unwrap(),
        None
    );
    token.cancel();
    let first = store
        .generation_controlled("source/test", &control)
        .unwrap_err();
    assert_eq!(first.code, "command.cancelled");
    assert_eq!(store.generation("source/test").unwrap(), Some(1));
    assert_eq!(control.check().unwrap_err().message, first.message);
    store.close().unwrap();
}
#[test]
fn consume_is_exactly_once_and_does_not_decode_unrelated_rows() {
    let f = Fixture::new();
    let mut store = f.store();
    let control = OperationControl::default();
    store
        .put_controlled("source/intent", &json!({"token":"one"}), None, &control)
        .unwrap();
    store
        .connection
        .execute(
            "INSERT INTO documents VALUES('unrelated',1,?1)",
            [b"{".as_slice()],
        )
        .unwrap();
    let mut calls = 0;
    let value = store
        .consume_controlled::<Value, _>("source/intent", &control, |value| {
            calls += 1;
            Ok(value)
        })
        .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(value["token"], "one");
    assert_eq!(
        store
            .generation_controlled("source/intent", &control)
            .unwrap(),
        None
    );
    assert_eq!(
        store.generation_controlled("unrelated", &control).unwrap(),
        Some(1)
    );
    let error = store
        .consume_controlled::<Value, ()>("source/intent", &control, |_| panic!("replayed"))
        .unwrap_err();
    assert_eq!(error.code, "vault.record_missing");
    store.close().unwrap();
}
#[test]
fn rejected_intent_remains_and_first_typed_cause_survives() {
    let f = Fixture::new();
    let mut store = f.store();
    let (control, token) = tracked();
    store
        .put_controlled("source/intent", &json!({"token":"one"}), None, &control)
        .unwrap();
    let error = store
        .consume_controlled::<Value, ()>("source/intent", &control, |_| {
            token.cancel();
            Err(ToolkitError::new(
                "source.intent_changed",
                "bad identity",
                4,
            ))
        })
        .unwrap_err();
    assert_eq!(error.code, "source.intent_changed");
    assert!(error.details.to_string().contains("command.cancelled"));
    assert_eq!(store.generation("source/intent").unwrap(), Some(1));
    store.close().unwrap();
}
#[test]
fn changed_generation_never_consumes_or_runs_validator() {
    let f = Fixture::new();
    let mut store = f.store();
    let control = OperationControl::default();
    store
        .put_controlled("source/intent", &json!(1), None, &control)
        .unwrap();
    store
        .put_controlled("source/intent", &json!(1), Some(1), &control)
        .unwrap();
    let error = store
        .consume_generation_controlled::<Value, ()>("source/intent", 1, &control, |_| {
            panic!("changed intent consumed")
        })
        .unwrap_err();
    assert_eq!(error.code, "vault.generation_conflict");
    assert_eq!(store.generation("source/intent").unwrap(), Some(2));
    store.close().unwrap();
}
#[test]
fn begin_contention_stops_before_business_callback() {
    let f = Fixture::new();
    let mut store = f.store();
    let (control, token) = tracked();
    store
        .put_controlled("source/intent", &json!(1), None, &control)
        .unwrap();
    let blocker = Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut busy = false;
    let error = store
        .consume_controlled_observed::<Value, ()>(
            "source/intent",
            None,
            &control,
            |_| panic!("callback before BEGIN"),
            &mut || {
                busy = true;
                token.cancel();
            },
        )
        .unwrap_err();
    assert!(busy);
    assert_eq!(error.code, "command.cancelled");
    assert!(error.details.is_null());
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    assert_eq!(store.generation("source/intent").unwrap(), Some(1));
    store.close().unwrap();
}
#[test]
fn commit_contention_does_not_replay_validator_and_rolls_back_delete() {
    let f = Fixture::new();
    let mut store = f.store();
    let (control, token) = tracked();
    store
        .put_controlled("source/intent", &json!(1), None, &control)
        .unwrap();
    let blocker = Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
    blocker
        .execute_batch("BEGIN; SELECT * FROM documents")
        .unwrap();
    let mut calls = 0;
    let mut busy = false;
    let error = store
        .consume_controlled_observed::<Value, ()>(
            "source/intent",
            None,
            &control,
            |_| {
                calls += 1;
                Ok(())
            },
            &mut || {
                busy = true;
                token.cancel();
            },
        )
        .unwrap_err();
    assert!(busy);
    assert_eq!(calls, 1);
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.details["key"], "source/intent");
    assert_eq!(error.details["state"], "unconfirmed");
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    assert_eq!(store.generation("source/intent").unwrap(), Some(1));
    store.close().unwrap();
}
#[test]
fn expired_original_deadline_does_not_consume() {
    let f = Fixture::new();
    let mut store = f.store();
    store.put("source/intent", &json!(1), None).unwrap();
    let control = OperationControl::uncancelable(Instant::now());
    let error = store
        .consume_controlled::<Value, ()>("source/intent", &control, |_| panic!("expired callback"))
        .unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert_eq!(store.generation("source/intent").unwrap(), Some(1));
    store.close().unwrap();
}
#[test]
fn malformed_intent_fails_without_deletion() {
    let f = Fixture::new();
    let mut store = f.store();
    store
        .connection
        .execute(
            "INSERT INTO documents VALUES('source/intent',1,?1)",
            [b"{".as_slice()],
        )
        .unwrap();
    assert!(
        store
            .consume_controlled::<Value, ()>(
                "source/intent",
                &OperationControl::default(),
                |_| panic!("malformed callback")
            )
            .is_err()
    );
    assert_eq!(store.generation("source/intent").unwrap(), Some(1));
    store.close().unwrap();
}
