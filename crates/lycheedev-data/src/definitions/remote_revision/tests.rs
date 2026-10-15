use super::*;
use std::{
    cell::Cell,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
fn home() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/remote-revision-fixtures")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    lycheedev_core::workspace::initialize(&home).unwrap();
    home
}
struct Reply {
    bytes: Vec<u8>,
    calls: Cell<usize>,
}
impl Fetch for Reply {
    fn fetch(&self, _: &str, control: &OperationControl, _: Option<&Budget>) -> Result<Vec<u8>> {
        control.check()?;
        self.calls.set(self.calls.get() + 1);
        Ok(self.bytes.clone())
    }
}
#[test]
fn exact_commit_needs_no_workspace_or_network_but_keeps_original_stop() {
    let control = OperationControl::default();
    let value = "a".repeat(40);
    let fetch = Reply {
        bytes: Vec::new(),
        calls: Cell::new(0),
    };
    assert_eq!(
        resolve(
            Path::new("does-not-exist"),
            &value,
            false,
            &control,
            None,
            &fetch
        )
        .unwrap(),
        value
    );
    assert_eq!(fetch.calls.get(), 0);
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        resolve(
            Path::new("does-not-exist"),
            &value,
            false,
            &control,
            None,
            &fetch
        )
        .unwrap_err()
        .code,
        "command.cancelled"
    );
}
#[test]
fn normalized_full_refs_and_go_path_segment_escaping_are_exact() {
    assert_eq!(normalized_reference("").unwrap(), "refs/heads/master");
    for bad in [
        "master",
        "refs/heads/",
        "refs/heads/.hidden",
        "refs/heads/a..b",
        "refs/heads/a.lock/b",
        "refs/heads/x\u{0085}",
    ] {
        assert!(normalized_reference(bad).is_err(), "{bad}");
    }
    assert_eq!(
        escaped("refs/heads/a+$&=@;,%中"),
        "refs%2Fheads%2Fa+$&=@%3B%2C%25%E4%B8%AD"
    );
}
#[test]
fn online_reference_refreshes_while_offline_uses_its_own_validated_alias() {
    let home = home();
    let control = OperationControl::default();
    let first = Reply {
        bytes: format!("{}\r\n", "a".repeat(40)).into_bytes(),
        calls: Cell::new(0),
    };
    assert_eq!(
        resolve(&home, "", false, &control, None, &first).unwrap(),
        "a".repeat(40)
    );
    let second = Reply {
        bytes: "b".repeat(40).into_bytes(),
        calls: Cell::new(0),
    };
    assert_eq!(
        resolve(&home, "", false, &control, None, &second).unwrap(),
        "b".repeat(40)
    );
    assert_eq!(
        resolve(&home, "", true, &control, None, &first).unwrap(),
        "b".repeat(40)
    );
    assert_eq!(first.calls.get(), 1);
    assert_eq!(second.calls.get(), 1);
    assert_eq!(
        resolve(&home, "refs/tags/absent", true, &control, None, &first)
            .unwrap_err()
            .code,
        "records.definition_unavailable_offline"
    );
}
#[test]
fn corrupt_alias_is_terminal_even_online_and_cannot_be_repaired_by_fetch() {
    let home = home();
    let control = OperationControl::default();
    let store = Store {
        home: &home,
        control: &control,
        budget: None,
    };
    let key = format!("definition-ref/{}", storage::sha256(b"refs/heads/master"));
    store
        .put(
            &key,
            &Remembered {
                reference: "refs/tags/wrong".into(),
                commit: "a".repeat(40),
            },
            None,
        )
        .unwrap();
    let fetch = Reply {
        bytes: "b".repeat(40).into_bytes(),
        calls: Cell::new(0),
    };
    assert_eq!(
        resolve(&home, "", false, &control, None, &fetch)
            .unwrap_err()
            .code,
        "records.definition_identity"
    );
    assert_eq!(fetch.calls.get(), 0);
}
#[test]
fn own_observed_commit_survives_a_valid_concurrent_alias_winner() {
    struct Race<'a> {
        home: &'a Path,
    }
    impl Fetch for Race<'_> {
        fn fetch(
            &self,
            reference: &str,
            control: &OperationControl,
            budget: Option<&Budget>,
        ) -> Result<Vec<u8>> {
            let store = Store {
                home: self.home,
                control,
                budget,
            };
            let key = format!("definition-ref/{}", storage::sha256(reference.as_bytes()));
            store.put(
                &key,
                &Remembered {
                    reference: reference.into(),
                    commit: "b".repeat(40),
                },
                None,
            )?;
            Ok("a".repeat(40).into_bytes())
        }
    }
    let home = home();
    let control = OperationControl::default();
    assert_eq!(
        resolve(&home, "", false, &control, None, &Race { home: &home }).unwrap(),
        "a".repeat(40)
    );
    assert_eq!(
        resolve(
            &home,
            "",
            true,
            &control,
            None,
            &Reply {
                bytes: vec![],
                calls: Cell::new(0)
            }
        )
        .unwrap(),
        "b".repeat(40)
    );
}
#[test]
fn revision_body_rejects_oversize_nonhex_and_non_crlf_suffixes() {
    let home = home();
    let control = OperationControl::default();
    for (bytes, code) in [
        (vec![b'a'; 129], "records.metadata_limit"),
        (
            format!("{} ", "a".repeat(40)).into_bytes(),
            "records.definition_identity",
        ),
        ("A".repeat(40).into_bytes(), "records.definition_identity"),
        (vec![255], "records.definition_identity"),
    ] {
        let fetch = Reply {
            bytes,
            calls: Cell::new(0),
        };
        assert_eq!(
            resolve(&home, "", false, &control, None, &fetch)
                .unwrap_err()
                .code,
            code
        );
    }
}

#[test]
fn cached_revision_uses_go_duplicate_case_unknown_and_null_field_rules() {
    let home = home();
    let control = OperationControl::default();
    let store = Store {
        home: &home,
        control: &control,
        budget: None,
    };
    let key = format!("definition-ref/{}", storage::sha256(b"refs/heads/master"));
    let raw = serde_json::value::RawValue::from_string(format!(r#"{{"Reference":"wrong","reference":"refs/heads/master","REFERENCE":null,"commit":"{}","Commit":null,"extra":"\ud800"}}"#, "a".repeat(40))).unwrap();
    store.put(&key, &raw, None).unwrap();
    let fetch = Reply {
        bytes: vec![],
        calls: Cell::new(0),
    };
    assert_eq!(
        resolve(&home, "", true, &control, None, &fetch).unwrap(),
        "a".repeat(40)
    );
    assert_eq!(fetch.calls.get(), 0);
    assert!(serde_json::from_str::<Remembered>(r#"{"reference":23}"#).is_err());
    let empty: Remembered = serde_json::from_str("null").unwrap();
    assert!(empty.reference.is_empty());
}

#[test]
fn cached_identity_mapping_preserves_complete_stop_and_cleanup_causes() {
    let pure = fault("records.remote_identity", 4, "records.remote_identity");
    assert_eq!(
        cache_error(pure.clone()).code,
        "records.definition_identity"
    );
    for cause in ["control", "budget", "cleanup"] {
        let mut composite = pure.clone();
        composite.details = serde_json::json!({"schema":"lycheedev.remote-cache-cleanup.v1", "primary":pure, cause: {"code":"command.cancelled","message":"after close","exitCode":7,"details":{"scope":cause}}});
        let expected = serde_json::to_value(&composite).unwrap();
        assert_eq!(
            serde_json::to_value(cache_error(composite)).unwrap(),
            expected
        );
    }
}
