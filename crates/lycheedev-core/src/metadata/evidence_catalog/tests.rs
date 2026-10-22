use super::*;
use crate::{evidence, storage, workspace};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-evidence-catalog-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], ""),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn capture(&self, bytes: &[u8], data: bool, dependencies: &[&str]) -> serde_json::Value {
        struct Meter(bool);
        impl evidence::CaptureAccounting for Meter {
            fn evidence_domain(&self) -> EvidenceDomain {
                if self.0 {
                    EvidenceDomain::Data
                } else {
                    EvidenceDomain::General
                }
            }
            fn check(&self) -> Result<()> {
                Ok(())
            }
            fn admit_allocation(&self, _: usize) -> Result<()> {
                Ok(())
            }
            fn admit_work(&self, _: usize) -> Result<()> {
                Ok(())
            }
        }
        evidence::commit_capture_budgeted_with_dependencies(
            &self.0,
            evidence::CaptureInput {
                bytes,
                maximum_bytes: bytes.len(),
                media_type: "text/plain",
                provenance: serde_json::json!({"kind":"fixture", "locator":"catalog"}),
                complete: true,
                truncated: false,
            },
            dependencies,
            &OperationControl::default(),
            &Meter(data),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn id(capture: &serde_json::Value) -> &str {
    capture["id"].as_str().unwrap()
}

#[test]
fn data_capture_is_authoritative_separate_from_rebuildable_and_historical_databases() {
    let f = Fixture::new();
    let capture = f.capture(b"new Data", true, &[]);
    assert!(f.0.join("state/data/evidence-v2.sqlite").is_file());
    assert!(!f.0.join("state/toolkit.sqlite").exists());
    assert!(!f.0.join("state/data/cache-v1.sqlite").exists());
    let (decoded, bytes) =
        evidence::fetch_capture_controlled(&f.0, id(&capture), 1024, &OperationControl::default())
            .unwrap();
    assert_eq!(decoded.id, id(&capture));
    assert_eq!(bytes, b"new Data");
    let metadata = Metadata::open(&f.0, false).unwrap();
    assert_eq!(
        metadata
            .read::<serde_json::Value>(&format!("capture/{}", id(&capture)))
            .unwrap(),
        capture
    );
    assert_eq!(
        metadata.list::<serde_json::Value>("capture/", 100).unwrap(),
        vec![capture]
    );
    metadata.close().unwrap();
    assert!(!f.0.join("state/toolkit.sqlite").exists());
}
#[test]
fn merged_capture_pages_use_one_global_limit_cursor_and_key_order() {
    let f = Fixture::new();
    let mut expected = vec![
        f.capture(b"old one", false, &[]),
        f.capture(b"data two", true, &[]),
        f.capture(b"old three", false, &[]),
        f.capture(b"data four", true, &[]),
    ];
    expected.sort_by(|a, b| id(a).cmp(id(b)));
    let metadata = Metadata::open(&f.0, false).unwrap();
    let mut after = "capture/".to_owned();
    let mut ids = Vec::new();
    loop {
        let (count, last) = metadata
            .visit_raw_page_controlled_with_check(
                "capture/",
                &after,
                1,
                &OperationControl::default(),
                || Ok(()),
                |key, _, _| {
                    ids.push(key.to_owned());
                    Ok(())
                },
            )
            .unwrap();
        if count == 0 {
            break;
        }
        assert_eq!(count, 1);
        assert!(last > after);
        after = last;
    }
    assert_eq!(
        ids,
        expected
            .iter()
            .map(|capture| format!("capture/{}", id(capture)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        metadata.list::<serde_json::Value>("capture/", 2).unwrap(),
        expected[..2]
    );
    metadata.close().unwrap();
}
#[test]
fn historical_to_data_and_data_to_historical_dependencies_block_removal() {
    let f = Fixture::new();
    let old = f.capture(b"historical dependency", false, &[]);
    let data = f.capture(b"Data owner", true, &[id(&old)]);
    assert!(
        evidence::remove_capture_controlled(&f.0, id(&old), &OperationControl::default()).is_err()
    );
    let historical_owner = f.capture(b"historical owner", false, &[id(&data)]);
    assert!(
        evidence::remove_capture_controlled(&f.0, id(&data), &OperationControl::default()).is_err()
    );
    assert_eq!(
        evidence::read_capture(&f.0, id(&historical_owner)).unwrap(),
        historical_owner
    );
}
#[test]
fn data_removal_receipt_and_generation_history_stay_in_same_store() {
    let f = Fixture::new();
    let capture = f.capture(b"Data remove", true, &[]);
    let removed =
        evidence::remove_capture_controlled(&f.0, id(&capture), &OperationControl::default())
            .unwrap();
    assert_eq!(removed["manifestRemoved"], true);
    // Independently owned Data CAS remains retained, even after CAP removal.
    assert_eq!(removed["blobShared"], true);
    let connection = Connection::open(f.0.join("state/data/evidence-v2.sqlite")).unwrap();
    let generations: i64 = connection
        .query_row(
            "SELECT generation FROM deleted_generations WHERE key=?1",
            [format!("capture/{}", id(&capture))],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(generations, 1);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM documents WHERE key=?1",
                [format!("evidence/remove/{}", id(&capture))],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    connection.close().unwrap();
    assert!(!f.0.join("state/toolkit.sqlite").exists());
    assert_eq!(
        evidence::remove_capture_controlled(&f.0, id(&capture), &OperationControl::default())
            .unwrap(),
        removed
    );
}
#[test]
fn read_only_carrier_observes_later_owned_writes_without_creating_itself() {
    let f = Fixture::new();
    f.capture(b"Data first", true, &[]);
    let reader = Metadata::open(&f.0, false).unwrap();
    let mut writer = Metadata::open(&f.0, true).unwrap();
    writer.put("ordinary/new", &42, None).unwrap();
    writer.close().unwrap();
    assert_eq!(reader.read::<i64>("ordinary/new").unwrap(), 42);
    reader.close().unwrap();
}
#[test]
fn divergent_duplicate_capture_fails_closed_and_preserves_both_stores() {
    let f = Fixture::new();
    let capture = f.capture(b"duplicate", true, &[]);
    let key = format!("capture/{}", id(&capture));
    // Deliberate corruption bypasses production mutation routing only here.
    let old = Metadata::open(&f.0, true).unwrap();
    old.connection
        .execute(
            INSERT_DOCUMENT,
            params![key, br#"{"invalid":true}"#.as_slice()],
        )
        .unwrap();
    old.close().unwrap();
    assert_eq!(
        read(&f.0, &key, &OperationControl::default(), &|| Ok(()))
            .unwrap_err()
            .code,
        "evidence.catalog_integrity"
    );
    assert!(f.0.join("state/data/evidence-v2.sqlite").is_file());
    assert!(f.0.join("state/toolkit.sqlite").is_file());
}
#[test]
fn cache_metadata_never_accepts_authoritative_evidence_keys() {
    let f = Fixture::new();
    let capture = f.capture(b"purpose", true, &[]);
    assert!(
        DataCacheMetadata::put_controlled_with_check(
            &f.0,
            &format!("capture/{}", id(&capture)),
            &capture,
            None,
            &OperationControl::default(),
            || Ok(())
        )
        .is_err()
    );
    assert_eq!(evidence::read_capture(&f.0, id(&capture)).unwrap(), capture);
}

#[test]
fn ordinary_metadata_writes_wait_for_catalog_gate_but_ordinary_reads_do_not() {
    use std::sync::mpsc;
    use std::time::Duration;
    let f = Fixture::new();
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    metadata.put("ordinary/value", &1, None).unwrap();
    metadata.close().unwrap();
    let gate = acquire_gate(&f.0, &OperationControl::default(), || Ok(())).unwrap();
    let reader = Metadata::open(&f.0, false).unwrap();
    assert_eq!(reader.read::<i64>("ordinary/value").unwrap(), 1);
    reader.close().unwrap();
    let home = f.0.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut metadata = Metadata::open(&home, true).unwrap();
        ready_tx.send(()).unwrap();
        let result = metadata.put("ordinary/second", &2, None);
        let result = finish_owned(result, metadata.close());
        done_tx.send(result).unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
    gate.close().unwrap();
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
}
#[test]
fn cancelled_catalog_wait_never_mutates_either_store() {
    use crate::control::TrackedCancellation;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let capture = f.capture(b"cancellation", true, &[]);
    let stop = TrackedCancellation::new();
    stop.cancel();
    // A cancelled root is sufficient: it must stop before quota/gate/database.
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop);
    assert!(
        read(
            &f.0,
            &format!("capture/{}", id(&capture)),
            &control,
            &|| Ok(())
        )
        .is_err()
    );
    assert_eq!(evidence::read_capture(&f.0, id(&capture)).unwrap(), capture);
}
#[test]
fn generic_reserved_key_update_targets_data_store_without_duplicate_history() {
    let f = Fixture::new();
    let capture = f.capture(b"generic mutation", true, &[]);
    let mut historical = Metadata::open(&f.0, true).unwrap();
    let key = format!("capture/{}", id(&capture));
    let generation = historical.generation(&key).unwrap();
    assert_eq!(generation, Some(1));
    historical.put(&key, &capture, generation).unwrap();
    assert_eq!(historical.generation(&key).unwrap(), Some(2));
    assert_eq!(
        historical
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM documents WHERE key=?1), (SELECT count(*) FROM deleted_generations WHERE key=?1)",
                [&key],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            )
            .unwrap(),
        (0, 0)
    );
    assert_eq!(historical.read::<serde_json::Value>(&key).unwrap(), capture);
    historical.close().unwrap();
    let data = Connection::open(f.0.join("state/data/evidence-v2.sqlite")).unwrap();
    assert_eq!(
        data.query_row(
            "SELECT generation FROM documents WHERE key=?1",
            [&key],
            |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
        2
    );
    data.close().unwrap();
}
#[test]
fn purpose_bound_database_rejects_unrelated_documents_without_deleting_them() {
    let f = Fixture::new();
    f.capture(b"purpose corruption", true, &[]);
    let connection = Connection::open(f.0.join("state/data/evidence-v2.sqlite")).unwrap();
    connection
        .execute(INSERT_DOCUMENT, params!["foreign/key", b"{}".as_slice()])
        .unwrap();
    connection.close().unwrap();
    assert_eq!(
        read(
            &f.0,
            "capture/CAP-any",
            &OperationControl::default(),
            &|| Ok(())
        )
        .unwrap_err()
        .code,
        "evidence.catalog_integrity"
    );
    let connection = Connection::open(f.0.join("state/data/evidence-v2.sqlite")).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM documents WHERE key='foreign/key'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    connection.close().unwrap();
}

#[test]
fn list_includes_exact_prefix_and_zero_limit_stays_empty() {
    let f = Fixture::new();
    let capture = f.capture(b"inclusive list", true, &[]);
    let reader = Metadata::open(&f.0, false).unwrap();
    let key = format!("capture/{}", id(&capture));
    assert_eq!(
        reader.list::<serde_json::Value>(&key, 1).unwrap(),
        vec![capture]
    );
    assert!(
        reader
            .list::<serde_json::Value>(&key, 0)
            .unwrap()
            .is_empty()
    );
    reader.close().unwrap();
}

#[test]
fn generic_delete_recreate_preserves_data_owner_and_generation_history() {
    let f = Fixture::new();
    let capture = f.capture(b"history remains Data", true, &[]);
    let key = format!("capture/{}", id(&capture));
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    assert!(metadata.delete(&key).unwrap());
    with_catalog(
        &f.0,
        None,
        &OperationControl::default(),
        &|| Ok(()),
        |catalog, _| {
            for related in [
                key.clone(),
                format!("evidence/keep/{}", id(&capture)),
                format!("evidence/remove/{}", id(&capture)),
            ] {
                assert_eq!(
                    catalog.owner(&related, &OperationControl::default(), &|| Ok(()))?,
                    Some(EvidenceDomain::Data),
                );
            }
            Ok(())
        },
    )
    .unwrap();
    metadata.create(&key, &capture).unwrap();
    assert_eq!(metadata.generation(&key).unwrap(), Some(2));
    assert_eq!(
        metadata
            .connection
            .query_row(
                "SELECT count(*) FROM documents WHERE key=?1",
                [&key],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    let consumed: serde_json::Value = metadata.consume(&key, |value| Ok(value)).unwrap();
    assert_eq!(consumed, capture);
    metadata.create(&key, &capture).unwrap();
    assert_eq!(metadata.generation(&key).unwrap(), Some(3));
    metadata.close().unwrap();
}

#[test]
fn cancellation_interrupts_catalog_wait_without_waiting_for_gate_release() {
    use crate::control::TrackedCancellation;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    let capture = f.capture(b"wait cancellation", true, &[]);
    let gate = acquire_gate(&f.0, &OperationControl::default(), || Ok(())).unwrap();
    let stop = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(10), stop.clone());
    let home = f.0.clone();
    let key = format!("capture/{}", id(&capture));
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx
            .send(read(&home, &key, &control, &|| Ok(())))
            .unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
    stop.cancel();
    let error = done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    gate.close().unwrap();
    worker.join().unwrap();
    assert_eq!(evidence::read_capture(&f.0, id(&capture)).unwrap(), capture);
}

#[test]
fn cross_domain_dependency_publication_serializes_before_removal_scan() {
    use std::{sync::mpsc, time::Duration};
    struct PausedDependency {
        ready: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl evidence::CaptureAccounting for PausedDependency {
        fn check(&self) -> Result<()> {
            Ok(())
        }
        fn admit_allocation(&self, _: usize) -> Result<()> {
            Ok(())
        }
        fn admit_work(&self, _: usize) -> Result<()> {
            Ok(())
        }
        fn admit_dependency(&self, _: usize) -> Result<()> {
            self.ready.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        }
    }
    let f = Fixture::new();
    let source = f.capture(b"Data dependency", true, &[]);
    let source_id = id(&source).to_owned();
    let home = f.0.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer_id = source_id.clone();
    let writer = std::thread::spawn(move || {
        evidence::commit_capture_budgeted_with_dependencies(
            &home,
            evidence::CaptureInput {
                bytes: b"historical owner",
                maximum_bytes: 64,
                media_type: "text/plain",
                provenance: serde_json::json!({"kind":"fixture","locator":"concurrent owner"}),
                complete: true,
                truncated: false,
            },
            &[&writer_id],
            &OperationControl::default(),
            &PausedDependency {
                ready: ready_tx,
                release: release_rx,
            },
        )
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let home = f.0.clone();
    let (removed_tx, removed_rx) = mpsc::channel();
    let removal = std::thread::spawn(move || {
        removed_tx
            .send(evidence::remove_capture_controlled(
                &home,
                &source_id,
                &OperationControl::default(),
            ))
            .unwrap();
    });
    assert!(removed_rx.recv_timeout(Duration::from_millis(100)).is_err());
    release_tx.send(()).unwrap();
    let owner = writer.join().unwrap().unwrap();
    let error = removed_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "evidence.capture_referenced");
    removal.join().unwrap();
    assert_eq!(evidence::read_capture(&f.0, id(&source)).unwrap(), source);
    assert_eq!(evidence::read_capture(&f.0, id(&owner)).unwrap(), owner);
}

#[test]
fn read_only_historical_and_carrier_handles_cannot_reopen_catalog_writers() {
    let f = Fixture::new();
    let capture = f.capture(b"readonly capability", true, &[]);
    let key = format!("capture/{}", id(&capture));
    for historical_present in [false, true] {
        if historical_present {
            let mut writer = Metadata::open(&f.0, true).unwrap();
            writer.put("ordinary/value", &1, None).unwrap();
            writer.close().unwrap();
        }
        let mut reader = Metadata::open(&f.0, false).unwrap();
        assert!(reader.put(&key, &capture, None).is_err());
        assert!(reader.delete(&key).is_err());
        assert!(
            reader
                .consume::<serde_json::Value, ()>(&key, |_| Ok(()))
                .is_err()
        );
        assert!(reader.put("ordinary/value", &2, None).is_err());
        assert!(reader.delete("ordinary/value").is_err());
        reader.close().unwrap();
        assert_eq!(evidence::read_capture(&f.0, id(&capture)).unwrap(), capture);
    }
}

#[test]
fn completed_removal_retry_remains_readable_after_quota_is_lowered() {
    let f = Fixture::new();
    let capture = f.capture(b"completed fact", true, &[]);
    let first =
        evidence::remove_capture_controlled(&f.0, id(&capture), &OperationControl::default())
            .unwrap();
    let mut config = workspace::default_config();
    config["dataCache"]["maxBytes"] = serde_json::json!(1);
    storage::atomic_json(&f.0.join("config.json"), &config).unwrap();
    let second =
        evidence::remove_capture_controlled(&f.0, id(&capture), &OperationControl::default())
            .unwrap();
    assert_eq!(first, second);
}

#[test]
fn checked_delete_reads_both_stores_but_mutates_one_owner() {
    let f = Fixture::new();
    let capture = f.capture(b"checked delete", true, &[]);
    let key = format!("capture/{}", id(&capture));
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    metadata.put("ordinary/other", &17, None).unwrap();
    let recovery = format!("evidence/remove/{}", id(&capture));
    metadata
        .delete_checked_record(&[key.clone()], &[(key.clone(), 1)], |documents| {
            assert!(documents.iter().any(|row| row.key == "ordinary/other"));
            assert!(documents.iter().any(|row| row.key == key));
            Ok((
                (),
                Some((recovery.clone(), serde_json::json!({"test":"same owner"}))),
            ))
        })
        .unwrap();
    assert_eq!(metadata.generation(&key).unwrap(), None);
    assert_eq!(metadata.generation(&recovery).unwrap(), Some(1));
    assert_eq!(
        metadata
            .connection
            .query_row(
                "SELECT count(*) FROM documents WHERE key=?1",
                [&recovery],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    metadata.close().unwrap();
}

#[test]
fn checked_delete_rejects_cross_store_recovery_before_any_mutation() {
    let f = Fixture::new();
    let capture = f.capture(b"checked split", true, &[]);
    let key = format!("capture/{}", id(&capture));
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    let error = metadata.delete_checked_record(&[key.clone()], &[(key.clone(), 1)], |_| {
        Ok((
            (),
            Some(("ordinary/recovery".into(), serde_json::json!(true))),
        ))
    });
    assert!(error.is_err());
    assert_eq!(metadata.generation(&key).unwrap(), Some(1));
    assert_eq!(metadata.generation("ordinary/recovery").unwrap(), None);
    assert_eq!(metadata.read::<serde_json::Value>(&key).unwrap(), capture);
    metadata.close().unwrap();
}

#[test]
fn cancelled_removal_rollback_preserves_original_failure_and_manifest() {
    use crate::control::TrackedCancellation;
    use crate::metadata::owned_write::removal::RemovalCommit;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let capture = f.capture(b"first error rollback", true, &[]);
    let stop = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    let mut state = RemovalCommit::NotCommitted;
    let error = with_existing_writer(&f.0, id(&capture), &control, |writer| {
        writer.with_write_lease(|writer| {
            writer.removal_transaction::<()>(&mut state, |tx| {
                tx.delete_manifest(&format!("capture/{}", id(&capture)), 1)?;
                stop.cancel();
                Err(ToolkitError::new("fixture.primary", "original failure", 4))
            })
        })
    })
    .unwrap_err();
    assert_eq!(error.code, "fixture.primary");
    assert_eq!(state, RemovalCommit::NotCommitted);
    assert_eq!(evidence::read_capture(&f.0, id(&capture)).unwrap(), capture);
}

#[test]
fn on_owner_raw_read_resolves_data_capture_from_carrier_and_historical_handle() {
    let f = Fixture::new();
    let capture = f.capture(b"owned raw read", true, &[]);
    let key = format!("capture/{}", id(&capture));
    for historical in [false, true] {
        if historical {
            Metadata::open(&f.0, true).unwrap().close().unwrap();
        }
        let reader = Metadata::open(&f.0, false).unwrap();
        let row = reader
            .read_document_on_owner_controlled(&key, &OperationControl::default())
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&row.bytes).unwrap(),
            capture
        );
        reader.close().unwrap();
    }
}

#[test]
fn read_quota_capability_cannot_publish_or_attribute_a_body() {
    let f = Fixture::new();
    let reference = evidence::BlobRef {
        sha256: storage::sha256(b"read-only body"),
        bytes: 14,
    };
    let error = with_catalog(
        &f.0,
        None,
        &OperationControl::default(),
        &|| Ok(()),
        |_, owner| owner.publish_memory(b"read-only body", 14, &reference),
    )
    .unwrap_err();
    assert!(error.message.contains("read-only evidence owner"));
    assert!(
        !evidence::blob_path(&f.0, &reference.sha256)
            .unwrap()
            .exists()
    );
    assert!(
        !f.0.join("indexes/data-derived-v1/shared-blobs-v1")
            .join(format!("{}.intent", reference.sha256))
            .exists()
    );
}

#[test]
fn historical_main_identity_is_pinned_through_catalog_sqlite_scope() {
    let f = Fixture::new();
    let capture = f.capture(b"historical", false, &[]);
    with_catalog(
        &f.0,
        Some(EvidenceDomain::General),
        &OperationControl::default(),
        &|| Ok(()),
        |catalog, _| {
            let key = format!("capture/{}", capture["id"].as_str().unwrap());
            assert!(
                catalog
                    .document(&key, &OperationControl::default(), &|| Ok(()))?
                    .is_some()
            );
            assert!(
                std::fs::rename(
                    f.0.join("state/toolkit.sqlite"),
                    f.0.join("state/replaced.sqlite")
                )
                .is_err()
            );
            catalog.general.as_mut().unwrap().put(
                "ordinary-pinned-test",
                &serde_json::json!({"ok":true}),
                None,
            )?;
            Ok(())
        },
    )
    .unwrap();
    std::fs::rename(
        f.0.join("state/toolkit.sqlite"),
        f.0.join("state/replaced.sqlite"),
    )
    .unwrap();
    std::fs::rename(
        f.0.join("state/replaced.sqlite"),
        f.0.join("state/toolkit.sqlite"),
    )
    .unwrap();
}

#[test]
fn historical_creation_is_pinned_before_sqlite_initialization() {
    let f = Fixture::new();
    assert!(!f.0.join("state/toolkit.sqlite").exists());
    with_catalog(
        &f.0,
        Some(EvidenceDomain::General),
        &OperationControl::default(),
        &|| Ok(()),
        |catalog, _| {
            assert!(catalog.shared_anchor.as_ref().unwrap().present());
            assert!(std::fs::remove_file(f.0.join("state/toolkit.sqlite")).is_err());
            catalog.general.as_mut().unwrap().put(
                "ordinary-created-test",
                &serde_json::json!({"ok":true}),
                None,
            )?;
            Ok(())
        },
    )
    .unwrap();
    let metadata = Metadata::open(&f.0, false).unwrap();
    assert_eq!(
        metadata
            .read::<serde_json::Value>("ordinary-created-test")
            .unwrap(),
        serde_json::json!({"ok":true})
    );
    metadata.close().unwrap();
}

#[test]
fn data_main_pin_preserves_read_miss_and_precedes_sqlite_creation() {
    let f = Fixture::new();
    let path = f.0.join("state/data/evidence-v2.sqlite");
    with_catalog(
        &f.0,
        None,
        &OperationControl::default(),
        &|| Ok(()),
        |catalog, owner| {
            assert!(catalog.data.is_none());
            assert!(owner.pin_existing_main()?.is_none());
            assert!(!path.exists());
            Ok(())
        },
    )
    .unwrap();
    with_catalog(
        &f.0,
        Some(EvidenceDomain::Data),
        &OperationControl::default(),
        &|| Ok(()),
        |catalog, owner| {
            catalog.data =
                store::open(&f.0, owner, true, &OperationControl::default(), &|| Ok(()))?;
            assert!(owner.pin_existing_main()?.is_some());
            assert!(std::fs::remove_file(&path).is_err());
            assert!(std::fs::rename(&path, path.with_extension("replaced")).is_err());
            Ok(())
        },
    )
    .unwrap();
    std::fs::rename(&path, path.with_extension("replaced")).unwrap();
    std::fs::rename(path.with_extension("replaced"), &path).unwrap();
}

#[test]
fn existing_data_main_remains_pinned_across_catalog_read_scope() {
    let f = Fixture::new();
    let capture = f.capture(b"pinned data", true, &[]);
    let path = f.0.join("state/data/evidence-v2.sqlite");
    with_catalog(
        &f.0,
        None,
        &OperationControl::default(),
        &|| Ok(()),
        |catalog, owner| {
            assert!(owner.pin_existing_main()?.is_some());
            assert!(std::fs::remove_file(&path).is_err());
            assert!(
                catalog
                    .document(
                        &format!("capture/{}", capture["id"].as_str().unwrap()),
                        &OperationControl::default(),
                        &|| Ok(())
                    )?
                    .is_some()
            );
            Ok(())
        },
    )
    .unwrap();
    std::fs::rename(&path, path.with_extension("replaced")).unwrap();
    std::fs::rename(path.with_extension("replaced"), &path).unwrap();
}
