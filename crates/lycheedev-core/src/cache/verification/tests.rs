use super::*;
use crate::control::{StopCause, TrackedCancellation};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Fixture {
    home: PathBuf,
    control: OperationControl,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-cache-verify-r35-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self {
            home,
            control: OperationControl::uncancelable(Instant::now() + Duration::from_secs(60)),
        }
    }
    fn object(&self, key: &str, body: &[u8]) -> PathBuf {
        let path = self
            .home
            .join("cache/objects")
            .join(&key[..2])
            .join(&key[2..]);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }
    fn marker(&self, key: &str, body: &[u8]) -> PathBuf {
        let path = self.home.join("cache/retain").join(format!("{key}.json"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.home).unwrap()
    }
}
#[test]
fn verify_streams_objects_sorts_all_errors_and_never_mutates_them() {
    let f = Fixture::new();
    let body = vec![17u8; 3 * 65536 + 19];
    let good = storage::sha256(&body);
    let good_path = f.object(&good, &body);
    let bad = "c".repeat(64);
    let bad_path = f.object(&bad, b"broken");
    let missing = "b".repeat(64);
    let marker = f.marker(
        &missing,
        format!(r#"{{"sha256":"{missing}","owners":[{{"kind":"pinned","owner":"fixture"}}]}}"#)
            .as_bytes(),
    );
    let foreign = f.home.join("cache/objects/foreign");
    fs::write(&foreign, b"ignored name").unwrap();
    let report = verify(&f.home, &f.control).unwrap();
    assert!(!report.ok);
    assert_eq!(report.checked, 2);
    assert_eq!(report.bytes, body.len() as u64 + 6);
    assert_eq!(report.corrupt, vec![bad]);
    assert_eq!(report.missing, vec![missing]);
    assert_eq!(report.errors, vec!["unexpected cache file: foreign"]);
    assert_eq!(fs::read(&good_path).unwrap(), body);
    assert_eq!(fs::read(&bad_path).unwrap(), b"broken");
    assert!(marker.exists());
    fs::write(good_path, b"after checked close").unwrap();
    f.cleanup();
}
#[test]
fn retention_duplicate_slice_null_and_zero_time_follow_go_reader() {
    let f = Fixture::new();
    let key = "a".repeat(64);
    let marker=f.marker(&key,format!(r#"{{"SHA256":"{key}","sha256":null,"owners":[{{"kind":"pinned","owner":"before"}}],"OWNERS":[{{"owner":"after","retainedAt":null}}]}}"#).as_bytes());
    let report = verify(&f.home, &f.control).unwrap();
    assert_eq!(report.missing, vec![key.clone()]);
    assert!(report.errors.is_empty());
    fs::write(
        &marker,
        format!(
            r#"{{"sha256":"{key}","owners":[{{"kind":"pinned","owner":"x"}}],"owners":[null]}}"#
        ),
    )
    .unwrap();
    assert_eq!(
        verify(&f.home, &f.control).unwrap().missing,
        vec![key.clone()]
    );
    fs::write(
        &marker,
        format!(r#"{{"sha256":"{key}","owners":[{{"kind":"pinned","owner":"x"}}],"owners":null}}"#),
    )
    .unwrap();
    assert!(verify(&f.home, &f.control).unwrap().ok);
    f.cleanup();
}
#[test]
fn retention_raw_timestamp_escape_is_not_json_unescaped_into_valid_time() {
    let f = Fixture::new();
    let key = "a".repeat(64);
    let marker=f.marker(&key,format!(r#"{{"sha256":"{key}","owners":[{{"kind":"evidence","owner":"x","retainedAt":"2026-10-15T00:00:00\u005a"}}]}}"#).as_bytes());
    let report = verify(&f.home, &f.control).unwrap();
    assert!(report.missing.is_empty());
    assert_eq!(report.errors.len(), 1);
    fs::write(marker,format!(r#"{{"sha256":"{key}","owners":[{{"kind":"evidence","owner":"x","retainedAt":"2026-10-15T00:00:00,123Z"}}]}}"#)).unwrap();
    assert_eq!(verify(&f.home, &f.control).unwrap().missing, vec![key]);
    f.cleanup();
}
#[test]
fn ignored_retention_values_use_go_depth_and_actual_64k_limit() {
    let f = Fixture::new();
    let key = "a".repeat(64);
    let value = |depth: usize| {
        format!(
            r#"{{"sha256":"{key}","owners":[],"ignored":{}0{}}}"#,
            "[".repeat(depth),
            "]".repeat(depth)
        )
    };
    let path = f.marker(&key, value(9999).as_bytes());
    assert!(verify(&f.home, &f.control).unwrap().ok);
    fs::write(&path, value(10000)).unwrap();
    assert_eq!(verify(&f.home, &f.control).unwrap().errors.len(), 1);
    let mut bytes = format!(r#"{{"sha256":"{key}","owners":[]}}"#).into_bytes();
    bytes.resize(65536, b' ');
    fs::write(&path, &bytes).unwrap();
    assert!(verify(&f.home, &f.control).unwrap().ok);
    bytes.push(b' ');
    fs::write(path, bytes).unwrap();
    assert_eq!(verify(&f.home, &f.control).unwrap().errors.len(), 1);
    f.cleanup();
}
#[test]
fn empty_directory_names_do_not_become_cache_objects_or_hygiene_files() {
    let f = Fixture::new();
    fs::create_dir_all(f.home.join("cache/objects/foreign/empty")).unwrap();
    let report = verify(&f.home, &f.control).unwrap();
    assert!(report.ok);
    assert_eq!(report.checked, 0);
    assert!(report.errors.is_empty());
    f.cleanup();
}
#[test]
fn cancelled_cache_dispatch_keeps_original_stop_and_does_not_scan() {
    let f = Fixture::new();
    let stop = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    stop.cancel();
    let inv = Invocation {
        command: "cache verify".into(),
        positional: None,
        flags: BTreeMap::new(),
        home: f.home.clone(),
        cwd: f.home.clone(),
        control,
    };
    let error = super::super::dispatch(&inv).unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        inv.control.stop_cause().unwrap(),
        Some(StopCause::Cancelled)
    );
    assert!(!f.home.join("cache/objects").exists());
    f.cleanup();
}
#[test]
fn cache_dispatch_integrity_preserves_report_and_existing_warning_slot() {
    let f = Fixture::new();
    f.object(&"d".repeat(64), b"bad");
    let inv = Invocation {
        command: "cache verify".into(),
        positional: None,
        flags: BTreeMap::new(),
        home: f.home.clone(),
        cwd: f.home.clone(),
        control: f.control.clone(),
    };
    let error = super::super::dispatch(&inv).unwrap_err();
    assert_eq!(error.code, "vault.cache_integrity");
    assert_eq!(error.details["result"]["checked"], 1);
    assert_eq!(error.details["warnings"].as_array().unwrap().len(), 1);
    f.cleanup();
}

#[test]
fn unused_malformed_reference_does_not_trigger_object_path_validation() {
    let f = Fixture::new();
    let path = f.marker("x", br#"{"sha256":"x","owners":[]}"#);
    assert!(verify(&f.home, &f.control).unwrap().ok);
    fs::write(
        path,
        br#"{"sha256":"x","owners":[{"kind":"pinned","owner":"fixture"}]}"#,
    )
    .unwrap();
    assert_eq!(verify(&f.home, &f.control).unwrap().errors.len(), 1);
    f.cleanup();
}
