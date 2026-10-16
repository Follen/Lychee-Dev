use super::*;
use lycheedev_core::{ToolkitError, control::TrackedCancellation, workspace};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "lychee-source-persist-{}-{}-{}",
            std::process::id(),
            lycheedev_core::storage::sha256(lycheedev_core::storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn manifest_replace_keeps_key_and_advances_generation_without_replaying() {
    let f = Fixture::new();
    let c = OperationControl::default();
    put_manifest(
        &f.0,
        "source/cache",
        &json!({"identity":"same","blob":"one"}),
        &c,
    )
    .unwrap();
    put_manifest(
        &f.0,
        "source/cache",
        &json!({"identity":"same","blob":"two"}),
        &c,
    )
    .unwrap();
    assert_eq!(
        read::<Value>(&f.0, "source/cache", &c).unwrap()["blob"],
        "two"
    );
    let store = Metadata::open_controlled(&f.0, false, &c).unwrap();
    assert_eq!(
        store.generation_controlled("source/cache", &c).unwrap(),
        Some(2)
    );
    store.close().unwrap();
}
#[test]
fn source_intent_is_single_use_at_published_generation() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let g = create_intent(&f.0, "source/start", &json!({"id":1}), &c).unwrap();
    assert_eq!(
        consume::<Value, _>(&f.0, "source/start", Some(g), &c, |v| Ok(v["id"].clone())).unwrap(),
        1
    );
    assert_eq!(
        consume::<Value, ()>(&f.0, "source/start", Some(g), &c, |_| panic!("replay"))
            .unwrap_err()
            .code,
        "vault.record_missing"
    );
}
#[test]
fn changed_intent_generation_is_preserved() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let g = create_intent(&f.0, "source/start", &json!(1), &c).unwrap();
    put_manifest(&f.0, "source/start", &json!(2), &c).unwrap();
    assert_eq!(
        consume::<Value, ()>(&f.0, "source/start", Some(g), &c, |_| panic!(
            "foreign consume"
        ))
        .unwrap_err()
        .code,
        "vault.generation_conflict"
    );
    assert_eq!(read::<Value>(&f.0, "source/start", &c).unwrap(), 2);
}
#[test]
fn cancelled_cleanup_preserves_durable_intent_and_original_stop() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let g = create_intent(&f.0, "source/start", &json!(1), &c).unwrap();
    let token = TrackedCancellation::new();
    let stopped = OperationControl::tracked(Instant::now() + Duration::from_secs(5), token.clone());
    token.cancel();
    let first = stopped.check().unwrap_err();
    let error = consume::<Value, ()>(&f.0, "source/start", Some(g), &stopped, |_| {
        panic!("stopped cleanup")
    })
    .unwrap_err();
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        serde_json::to_value(first).unwrap()
    );
    assert_eq!(read::<Value>(&f.0, "source/start", &c).unwrap(), 1);
}
#[test]
fn pre_cancel_does_not_open_or_create_workspace() {
    let f = Fixture::new();
    let token = TrackedCancellation::new();
    let c = OperationControl::tracked(Instant::now() + Duration::from_secs(5), token.clone());
    token.cancel();
    let absent = f.0.join("absent");
    assert_eq!(
        read::<Value>(&absent, "source/cache", &c).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(
        create_intent(&absent, "source/start", &json!(1), &c)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert!(!absent.exists());
}
#[test]
fn terminal_composition_keeps_primary_close_and_distinct_stop() {
    let c = OperationControl::uncancelable(Instant::now());
    let primary = ToolkitError::new("source.primary", "original", 4)
        .with_details(json!({"blob":"authenticated"}));
    let close = ToolkitError::new("source.close", "close unconfirmed", 5);
    let error =
        finish_control::<()>(finish_historical(Err(primary), Err(close)), c.check()).unwrap_err();
    assert_eq!(error.code, "source.primary");
    let raw = serde_json::to_string(&error).unwrap();
    assert!(raw.contains("authenticated"));
    assert!(raw.contains("source.close"));
    assert!(raw.contains("command.deadline"));
}
#[test]
fn repeated_sticky_stop_is_not_a_second_cleanup_event() {
    let c = OperationControl::uncancelable(Instant::now());
    let first = c.check().unwrap_err();
    let got = finish_control::<()>(Err(first.clone()), c.check()).unwrap_err();
    assert_eq!(
        serde_json::to_value(got).unwrap(),
        serde_json::to_value(first).unwrap()
    );
}
