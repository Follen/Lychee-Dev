use super::*;
use std::{
    cell::Cell,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-listfile-r43-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        lycheedev_core::workspace::initialize(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        } else {
            eprintln!("retained failed listfile fixture: {}", self.0.display());
        }
    }
}
struct Fetch {
    calls: Cell<usize>,
}
impl Fetcher for Fetch {
    fn fetch(&self, _: &str, _: usize, control: &OperationControl) -> Result<Vec<u8>> {
        control.check()?;
        self.calls.set(self.calls.get() + 1);
        Ok(b"1;A.blp\n".to_vec())
    }
}
fn seed(f: &Fixture) -> serde_json::Value {
    let fetch = Fetch {
        calls: Cell::new(0),
    };
    prepare(
        &f.0,
        "community-csv",
        false,
        false,
        Default::default(),
        &Default::default(),
        &fetch,
    )
    .unwrap();
    let m = Metadata::open_controlled(&f.0, false, &Default::default()).unwrap();
    let doc = m
        .read_controlled("listfile/community-csv", &Default::default())
        .unwrap();
    m.close().unwrap();
    doc
}
fn write(f: &Fixture, raw: &str) {
    let control = OperationControl::default();
    let mut m = Metadata::open_controlled(&f.0, true, &control).unwrap();
    let generation = m
        .generation_controlled("listfile/community-csv", &control)
        .unwrap();
    let raw = serde_json::value::RawValue::from_string(raw.into()).unwrap();
    m.put_controlled("listfile/community-csv", &raw, generation, &control)
        .unwrap();
    m.close().unwrap();
}
fn offline(f: &Fixture) -> Result<Prepared> {
    prepare(
        &f.0,
        "community-csv",
        true,
        false,
        Default::default(),
        &Default::default(),
        &Fetch {
            calls: Cell::new(0),
        },
    )
}
#[test]
fn listfile_validation_precedes_workspace_and_offline_opens_real_metadata_owner() {
    let path = Path::new("nonexistent-listfile-invalid-request");
    assert_eq!(
        prepare(
            path,
            "bad",
            false,
            false,
            Default::default(),
            &Default::default(),
            &Fetch {
                calls: Cell::new(0)
            }
        )
        .err()
        .unwrap()
        .code,
        "records.listfile_query"
    );
    let f = Fixture::new();
    assert_eq!(
        offline(&f).err().unwrap().code,
        "records.listfile_unavailable"
    );
    let m = Metadata::open_controlled(&f.0, false, &Default::default()).unwrap();
    m.close().unwrap();
}
#[test]
fn listfile_refresh_samples_generation_after_fetch_and_keeps_original_control() {
    let f = Fixture::new();
    seed(&f);
    struct Concurrent<'a>(&'a Fixture, Cell<usize>);
    impl Fetcher for Concurrent<'_> {
        fn fetch(&self, _: &str, _: usize, control: &OperationControl) -> Result<Vec<u8>> {
            assert!(control.tracked_cancel_handle().is_ok());
            self.1.set(self.1.get() + 1);
            write(self.0, "{\"intervening\":true}");
            Ok(b"2;B.blp\n".to_vec())
        }
    }
    let fetch = Concurrent(&f, Cell::new(0));
    let control = OperationControl::default();
    let result = prepare(
        &f.0,
        "community-csv",
        false,
        true,
        Default::default(),
        &control,
        &fetch,
    )
    .unwrap();
    assert_eq!(result.index.entries[0].file_id, 2);
    assert_eq!(fetch.1.get(), 1);
    assert!(control.stop_cause().is_ok());
    assert_eq!(offline(&f).unwrap().index.entries[0].file_id, 2);
}
#[test]
fn listfile_cache_duplicate_null_case_time_and_signed_fields_follow_go_typed_reader() {
    let f = Fixture::new();
    let doc = seed(&f);
    let provenance = serde_json::to_string(&doc["provenance"]).unwrap();
    let raw = format!(
        r#"{{"schema":"wrong","SCHEMA":"{SCHEMA}","schema":null,"provenance":{provenance},"Provenance":{{"fetchedAt":"2026-10-14T2:00:00,125Z","entryCount":null}},"provenance":null}}"#
    );
    write(&f, &raw);
    let prepared = offline(&f).unwrap();
    assert_eq!(
        serde_json::to_value(&prepared.provenance).unwrap()["fetchedAt"],
        "2026-10-14T02:00:00.125Z"
    );
    for token in ["1.0", "1e0", "-1", "9223372036854775808"] {
        let raw = format!(
            r#"{{"schema":"{SCHEMA}","provenance":{provenance},"Provenance":{{"entryCount":{token}}}}}"#
        );
        write(&f, &raw);
        assert_eq!(offline(&f).err().unwrap().code, "records.listfile_format");
    }
    let raw = format!(
        r#"{{"schema":"{SCHEMA}","provenance":{provenance},"Provenance":{{"fetchedAt":"2026-10-14T22:00:00\u005a"}}}}"#
    );
    write(&f, &raw);
    assert_eq!(offline(&f).err().unwrap().code, "records.listfile_format");
}
#[test]
fn listfile_cache_depth_admission_uses_original_bytes_and_go_boundary() {
    let f = Fixture::new();
    let doc = serde_json::to_string(&seed(&f)).unwrap();
    for (depth, valid) in [(9999, true), (10000, false)] {
        let raw = format!(
            "{},\"ignored\":{}0{}}}",
            &doc[..doc.len() - 1],
            "[".repeat(depth),
            "]".repeat(depth)
        );
        write(&f, &raw);
        assert_eq!(offline(&f).is_ok(), valid);
    }
}
#[test]
fn listfile_cache_invalid_source_reloads_same_origin_online_without_swallowing_stop() {
    let f = Fixture::new();
    let mut doc = seed(&f);
    doc["provenance"]["files"][0]["url"] = serde_json::json!("https://other.test/file");
    write(&f, &doc.to_string());
    let fetch = Fetch {
        calls: Cell::new(0),
    };
    let control = OperationControl::default();
    prepare(
        &f.0,
        "community-csv",
        false,
        false,
        Default::default(),
        &control,
        &fetch,
    )
    .unwrap();
    assert_eq!(fetch.calls.get(), 1);
    control.tracked_cancel_handle().unwrap().cancel();
    let error = prepare(
        &f.0,
        "community-csv",
        false,
        false,
        Default::default(),
        &control,
        &fetch,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(fetch.calls.get(), 1);
}
#[test]
fn listfile_unknown_timezone_is_accepted_on_read_and_fails_only_public_serialization() {
    let f = Fixture::new();
    let mut doc = seed(&f);
    doc["provenance"]["fetchedAt"] = serde_json::json!("2026-10-14T22:00:00+24:00");
    write(&f, &doc.to_string());
    let prepared = offline(&f).unwrap();
    assert!(serde_json::to_value(&prepared.provenance).is_err());
}
