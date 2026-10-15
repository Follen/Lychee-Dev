use super::*;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    home: PathBuf,
    path: PathBuf,
    control: OperationControl,
}
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-spec-file-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        crate::workspace::initialize(&home).unwrap();
        let path = home.join("selection.json");
        Self {
            home,
            path,
            control: OperationControl::uncancelable(Instant::now() + Duration::from_secs(60)),
        }
    }
    fn run(&self, raw: &[u8]) -> Result<PinnedSet> {
        fs::write(&self.path, raw).unwrap();
        resolve_selection_file_controlled(&self.home, &self.path, &self.control)
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.home).unwrap();
    }
}
fn source() -> Value {
    json!({"repository":"wow-ui-source","product":"retail","requestedRef":"live","exactCommit":"a".repeat(40),"parserRevision":"spec-fixture"})
}
fn spec() -> Vec<u8> {
    serde_json::to_vec(&json!({"source":source()})).unwrap()
}
#[test]
fn real_selection_file_publishes_exact_reusable_pin_and_reauthenticates_winner() {
    let f = Fixture::new();
    let pin = f.run(&spec()).unwrap();
    let again = f.run(&spec()).unwrap();
    assert_eq!(pin.id, again.id);
    assert_eq!(
        load_pinned_controlled(&f.home, &pin.id, &f.control)
            .unwrap()
            .source,
        pin.source
    );
    let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
    db.execute(
        "UPDATE documents SET value=?2 WHERE key=?1",
        rusqlite::params![format!("pin/{}", pin.id), br#"{"schema":"bad"}"#.as_slice()],
    )
    .unwrap();
    db.close().unwrap();
    assert!(f.run(&spec()).is_err());
    let m = Metadata::open(&f.home, false).unwrap();
    assert_eq!(
        m.read::<Value>(&format!("pin/{}", pin.id)).unwrap()["schema"],
        "bad"
    );
    m.close().unwrap();
    f.cleanup();
}
#[test]
fn file_spec_merges_duplicate_objects_case_fields_and_pointer_null_without_project_rules() {
    let f = Fixture::new();
    let raw = format!(
        r#"{{"source":{{"repository":"wow-ui-source","product":"wrong","parserRevision":"spec-fixture"}},"SOURCE":{{"PRODUCT":"retail","product":null,"requestedRef":"live","exactCommit":"{}"}}}}"#,
        "a".repeat(40)
    );
    assert_eq!(
        f.run(raw.as_bytes()).unwrap().id,
        f.run(&spec()).unwrap().id
    );
    let reset = format!(
        r#"{{"source":{{"repository":"discard"}},"source":null,"source":{}}}"#,
        source()
    );
    assert_eq!(
        f.run(reset.as_bytes()).unwrap().id,
        f.run(&spec()).unwrap().id
    );
    f.cleanup();
}
#[test]
fn spec_unknown_nested_fields_are_rejected_before_trailing_data_and_without_metadata() {
    let f = Fixture::new();
    for raw in [
        br#"{"source":{"unknown":true}} {}"#.as_slice(),
        br#"{"unknown":true} malformed"#.as_slice(),
    ] {
        let error = f.run(raw).unwrap_err();
        assert!(error.message.contains("unknown field"));
        assert!(!f.home.join("state/toolkit.sqlite").exists());
    }
    let mut raw = spec();
    raw.extend_from_slice(b" garbage");
    assert_eq!(
        f.run(&raw).unwrap_err().message,
        "selection.trailing_spec_data"
    );
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    f.cleanup();
}
#[test]
fn input_limit_is_actual_sixty_four_kib_plus_one_probe_not_metadata_length() {
    struct Source {
        remaining: usize,
        delivered: usize,
    }
    impl Read for Source {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = self.remaining.min(out.len());
            out[..n].fill(b' ');
            self.remaining -= n;
            self.delivered += n;
            Ok(n)
        }
    }
    let control = OperationControl::default();
    let mut input = Source {
        remaining: 1 << 20,
        delivered: 0,
    };
    assert_eq!(
        read(&mut input, &control).unwrap_err().message,
        "selection.spec_limit"
    );
    assert_eq!(input.delivered, 65537);
    let f = Fixture::new();
    let mut raw = spec();
    raw.resize(65536, b' ');
    assert!(f.run(&raw).is_ok());
    raw.push(b' ');
    assert_eq!(f.run(&raw).unwrap_err().message, "selection.spec_limit");
    f.cleanup();
}
#[test]
fn invalid_utf8_in_string_uses_go_replacement_instead_of_project_utf8_rejection() {
    let f = Fixture::new();
    let mut raw = spec();
    let position = raw.windows(4).position(|v| v == b"live").unwrap();
    raw.splice(position..position + 4, [0xff]);
    let pin = f.run(&raw).unwrap();
    assert_eq!(pin.source.as_ref().unwrap().requested_ref, "\u{fffd}");
    f.cleanup();
}
#[test]
fn first_json_shared_scanner_keeps_full_admission_and_go_ten_thousand_depth() {
    for raw in [
        b"{} garbage".as_slice(),
        b"null true".as_slice(),
        b"[]{}".as_slice(),
    ] {
        assert!(crate::raw_json::first_value_end(raw).is_ok());
        assert!(!crate::raw_json::is_valid(raw));
    }
    for depth in [9999usize, 10000] {
        let mut raw = br#"{"unknown":"#.to_vec();
        raw.extend(std::iter::repeat_n(b'[', depth));
        raw.extend(std::iter::repeat_n(b']', depth));
        raw.push(b'}');
        let result = wire::decode(&raw, &OperationControl::default());
        if depth == 9999 {
            assert!(result.err().unwrap().message.contains("unknown field"));
        } else {
            assert!(
                result
                    .err()
                    .unwrap()
                    .message
                    .contains("invalid Go raw JSON")
            );
        }
    }
}
#[test]
fn spec_time_raw_tokens_null_merge_zero_and_custom_error_priority_follow_go() {
    let control = OperationControl::default();
    let good =
        br#"{"changes":{"fetchedAt":"2026-10-15T1:02:03,1200Z"},"CHANGES":{"FETCHEDAT":null}}"#;
    let decoded = wire::decode(good, &control).unwrap();
    assert_eq!(
        decoded.spec.changes.unwrap().fetched_at,
        "2026-10-15T01:02:03.12Z"
    );
    for raw in [
        br#"{"changes":{"fetchedAt":"2026-10-15T00:00:00\u005a"}}"#.as_slice(),
        br#"{"unknown":true,"changes":{"bad":true,"fetchedAt":"bad"}}"#.as_slice(),
    ] {
        assert_eq!(
            wire::decode(raw, &control).err().unwrap().message,
            "invalid Go selection timestamp"
        );
    }
    let decoded = wire::decode(
        br#"{"changes":{"fetchedAt":"0001-01-01T01:00:00+01:00"}}"#,
        &control,
    )
    .unwrap();
    assert!(decoded.spec.changes.unwrap().fetched_at.is_empty());
    let decoded = wire::decode(
        br#"{"changes":{"fetchedAt":"2026-10-15T00:00:00+24:00"},"changes":null}"#,
        &control,
    )
    .unwrap();
    assert!(!decoded.time_unmarshal_only);
    assert!(decoded.spec.changes.is_none());
}
#[test]
fn valid_decoded_spec_opens_metadata_before_pin_validation_and_parent_precedes_bad_build() {
    let f = Fixture::new();
    fs::create_dir_all(f.home.join("state")).unwrap();
    fs::write(f.home.join("state/toolkit.sqlite"), b"not sqlite").unwrap();
    assert_eq!(f.run(b"null").unwrap_err().code, "vault.metadata");
    f.cleanup();
    let f = Fixture::new();
    let raw = serde_json::to_vec(&json!({"parent":"PIN-missing","data":{"fullBuild":"+1.2.3.4"}}))
        .unwrap();
    assert_eq!(f.run(&raw).unwrap_err().code, "vault.record_missing");
    f.cleanup();
}
#[test]
fn original_stop_before_open_and_during_real_read_preserves_ordered_control_and_observed_bytes() {
    let f = Fixture::new();
    let token = crate::control::TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(60), token.clone());
    token.cancel();
    assert_eq!(
        resolve_selection_file_controlled(&f.home, &f.path, &control)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert!(control.stop_cause().unwrap().is_some());
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    struct StopRead {
        token: crate::control::TrackedCancellation,
        delivered: usize,
    }
    impl Read for StopRead {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            out[..3].copy_from_slice(b"abc");
            self.delivered += 3;
            self.token.cancel();
            Ok(3)
        }
    }
    let token = crate::control::TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(60), token.clone());
    let mut input = StopRead {
        token,
        delivered: 0,
    };
    assert_eq!(
        read(&mut input, &control).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(input.delivered, 3);
    f.cleanup();
}
#[test]
fn checked_file_close_failure_keeps_primary_and_original_stop_after_committed_work() {
    let token = crate::control::TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(60), token.clone());
    token.cancel();
    let result: Result<()> = finish(
        Err(failed("primary")),
        Err(ToolkitError::new("external.io", "close", 5)),
        &control,
    );
    let error = result.unwrap_err();
    assert_eq!(
        error.message,
        format!("primary\nclose\n{}", control.check().unwrap_err().message)
    );
    let error = completed::<()>(Err(error), Some("PIN-committed".into())).unwrap_err();
    assert_eq!(error.details["completedPin"], "PIN-committed");
    assert!(error.message.contains("primary\nclose"));
    assert!(
        error
            .message
            .contains("selection pin already committed: PIN-committed")
    );
    assert!(!error.details.is_null());
    let stopped = control.check().unwrap_err();
    let error = finish::<()>(
        Err(stopped.clone()),
        Err(ToolkitError::new("external.io", "close", 5)),
        &control,
    )
    .unwrap_err();
    assert_eq!(error.message, format!("{}\nclose", stopped.message));
}
#[test]
fn default_workspace_file_dispatch_retains_relative_cwd_without_added_snapshot_context() {
    let f = Fixture::new();
    fs::write(&f.path, spec()).unwrap();
    let inv = Invocation {
        command: "target resolve".into(),
        positional: None,
        flags: BTreeMap::from([("--file".into(), "selection.json".into())]),
        home: f.home.clone(),
        cwd: f.home.clone(),
        control: f.control.clone(),
    };
    let out = crate::workspace::dispatch(&inv).unwrap();
    assert!(out.data["id"].as_str().unwrap().starts_with("PIN-"));
    assert_eq!(out.selection, json!({}));
    assert!(out.provenance.is_empty());
    f.cleanup();
}
#[test]
fn inherited_pin_uses_typed_historical_merge_and_ignores_unknown_fields() {
    let f = Fixture::new();
    let parent = f.run(&spec()).unwrap();
    let raw = format!(
        r#"{{"schema":"wrong","SCHEMA":"lycheedev.selection.v1","id":"{}","source":{{"repository":"wow-ui-source","product":"retail","requestedRef":"live"}},"source":{{"exactCommit":"{}","parserRevision":"spec-fixture","oldExtension":{{}}}},"unknown":[]}}"#,
        parent.id,
        "a".repeat(40)
    );
    Metadata::with_historical_write(&f.home, &f.control, |writer| {
        writer.commit(&[crate::metadata::historical_write::HistoricalMutation {
            key: &format!("pin/{}", parent.id),
            expected_generation: 1,
            bytes: raw.as_bytes(),
        }])
    })
    .unwrap();
    let child = f
        .run(&serde_json::to_vec(&json!({"parent":parent.id})).unwrap())
        .unwrap();
    assert_eq!(child.source, parent.source);
    assert_eq!(child.parent, parent.id);
    assert_ne!(child.id, parent.id);
    let mut conflicting = source();
    conflicting["requestedRef"] = json!("other");
    let error = f
        .run(&serde_json::to_vec(&json!({"parent":parent.id,"source":conflicting})).unwrap())
        .unwrap_err();
    assert_eq!(error.message, "selection.fixed_source_conflict");
    assert_eq!(error.exit_code, 5);
    f.cleanup();
}
#[test]
fn validation_precedence_and_unmarshal_only_time_precede_publication() {
    let f = Fixture::new();
    let error = f
        .run(br#"{"source":{},"data":{"fullBuild":"+1.2.3.4"}}"#)
        .unwrap_err();
    assert_eq!(error.message, "selection.unresolved_source");
    let raw=serde_json::to_vec(&json!({"changes":{"provider":"fixture","product":"retail","fullBuild":"1.2.3.4","region":"us","scope":"all","fetchedAt":"2026-10-15T00:00:00+24:00","sha256":"f".repeat(64)}})).unwrap();
    assert_eq!(
        f.run(&raw).unwrap_err().message,
        "Time.MarshalJSON: timezone hour outside of range [0,23]"
    );
    let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM documents WHERE key LIKE 'pin/%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    db.close().unwrap();
    f.cleanup();
}
#[test]
fn real_metadata_schema_lease_wait_observes_original_deadline_and_releases_input() {
    let f = Fixture::new();
    fs::write(&f.path, spec()).unwrap();
    let lease =
        crate::metadata::historical_write::historical_lease(&f.home, "metadata-schema", &f.control)
            .unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(30));
    let error = resolve_selection_file_controlled(&f.home, &f.path, &control).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.deadline", 5)
    );
    assert!(control.stop_cause().unwrap().is_some());
    assert!(
        control
            .ordered_child(Instant::now() + Duration::from_secs(1))
            .is_ok()
    );
    lease.close().unwrap();
    // Delete/recreate is also the Windows proof that the failed invocation did
    // not retain its input File. A later fresh invocation can acquire metadata.
    fs::remove_file(&f.path).unwrap();
    assert!(f.run(&spec()).is_ok());
    f.cleanup();
}
