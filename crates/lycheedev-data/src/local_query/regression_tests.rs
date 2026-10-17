//! Review-driven boundaries for fixed Go sql.go admission and CheckSource.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell::U, Fixture, table};
use lycheedev_core::metadata::Metadata;
use std::fs;

#[test]
fn sql_go_admission_orders_table_count_names_size_before_numeric_conversion() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let file = fx.root.join("oversize-and-overflow.json");
    let raw = format!(
        r#"{{"sql":"SELECT :n,:text FROM Sample","parameters":{{"n":1e999,"text":"{}"}}}}"#,
        "<".repeat(200_000)
    );
    assert!(raw.len() < 1 << 20);
    fs::write(&file, raw).unwrap();
    let mut inv = fx.invocation("data sql", &[("--file", file.to_str().unwrap())]);
    inv.home = fx.root.join("must-not-create");
    assert_eq!(sql(&inv).unwrap_err().code, "query.budget_exceeded");
    assert!(!inv.home.exists());
    let sql_text = (0..33)
        .map(|index| format!("SELECT :missing FROM T{index}"))
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    assert_eq!(
        relational::Program::compile(&sql_text)
            .unwrap()
            .source_tables()
            .len(),
        33
    );
    let mut inv = fx.invocation("data sql", &[("--sql", &sql_text)]);
    inv.home = fx.root.join("must-not-create");
    assert_eq!(
        sql(&inv).unwrap_err().code,
        "query.budget_exceeded",
        "Physical source count precedes the missing parameter error"
    );
    assert!(!inv.home.exists());
}
#[test]
fn sql_checks_changed_casc_metadata_after_execution_before_capture_and_recovers() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
    let catalog = fx.client.parent().unwrap().join(".build.info");
    let original = fs::read(&catalog).unwrap();
    let before = Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 100)
        .unwrap()
        .len();
    let mut executed = false;
    let error = sql_inner_after_execute(&inv, &mut || {
        executed = true;
        let mut changed = original.clone();
        changed.push(b'\n');
        fs::write(&catalog, changed)?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        executed,
        "Mutation happens only after SQL has returned its result"
    );
    assert_eq!(error.code, "records.pinned_build_changed");
    assert_eq!(
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len(),
        before,
        "No query capture may precede final source verification"
    );
    fs::write(&catalog, original).unwrap();
    let out = sql(&inv).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[1]]));
    assert_eq!(out.provenance.len(), 1);
}

#[test]
fn sql_normalizes_numeric_parameters_after_pin_before_any_source_resolution() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let file = fx.root.join("numeric-source-conflict.json");
    let missing_installation = fx.root.join("missing-installation");
    assert!(!missing_installation.exists());
    let before = Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 100)
        .unwrap()
        .len();
    for missing_definition in [false, true] {
        for missing_casc in [false, true] {
            let table = if missing_definition {
                "Missing"
            } else {
                "Sample"
            };
            let sql_text = format!("SELECT :n FROM effective.{table}");
            let mut inv = fx.invocation("data sql", &[("--file", file.to_str().unwrap())]);
            if missing_casc {
                inv.flags.insert(
                    "--installation".into(),
                    missing_installation.to_string_lossy().into_owned(),
                );
            }
            assert!(selection::resolve_pinned(&inv).unwrap().data.is_some());
            // With a canonical scalar, prove the later provider conflict exists.
            fs::write(
                &file,
                format!(r#"{{"sql":"{sql_text}","parameters":{{"n":1}}}}"#),
            )
            .unwrap();
            let source_error = sql(&inv).unwrap_err();
            if missing_definition {
                assert_eq!(source_error.code, "schema.definition_not_found");
            } else if missing_casc {
                assert_ne!(source_error.code, "records.hotfix_identity");
                assert_ne!(source_error.code, "query.numeric_range");
            } else {
                assert_eq!(source_error.code, "records.hotfix_identity");
            }
            fs::write(
                &file,
                format!(r#"{{"sql":"{sql_text}","parameters":{{"n":1e999}}}}"#),
            )
            .unwrap();
            assert_eq!(
                sql(&inv).unwrap_err().code,
                "query.numeric_range",
                "Numeric normalization precedes the provider error {} (missing definition={missing_definition}, missing CASC={missing_casc})",
                source_error.code
            );
        }
    }
    assert_eq!(
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len(),
        before
    );
}

#[test]
fn explain_rechecks_source_after_binding_before_capture() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let inv = fx.invocation("data sql", &[("--sql", "EXPLAIN SELECT ID FROM Sample")]);
    let catalog = fx.client.parent().unwrap().join(".build.info");
    let original = fs::read(&catalog).unwrap();
    let count = || {
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len()
    };
    let before = count();
    let error = sql_inner_after_execute(&inv, &mut || {
        let mut changed = original.clone();
        changed.push(b'\n');
        fs::write(&catalog, changed)?;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "records.pinned_build_changed");
    assert_eq!(count(), before);
    fs::write(catalog, original).unwrap();
    assert!(sql(&inv).unwrap().data["result"]["plan"].is_object());
}

#[test]
fn sql_checked_close_error_preserves_actual_committed_capture_identities() {
    let primary = fault("storage.close_failed", 5, "real close failure");
    let ids = vec!["CAP-query".to_owned(), "CAP-csv".to_owned()];
    let error = sql_committed_error(primary.clone(), &ids, true);
    assert_eq!(error.code, primary.code);
    assert_eq!(error.exit_code, primary.exit_code);
    assert_eq!(
        error.details["primary"],
        serde_json::to_value(primary).unwrap()
    );
    assert_eq!(error.details["completedCaptures"], json!(ids));
    assert_eq!(error.details["outputPublished"], true);
    assert!(error.message.contains("CAP-query, CAP-csv"));
    let early = fault("schema.definition_not_found", 3, "missing table");
    let unchanged = sql_committed_error(early.clone(), &[], false);
    assert_eq!(
        serde_json::to_value(unchanged).unwrap(),
        serde_json::to_value(early).unwrap()
    );
}

#[test]
fn sql_rehashes_original_index_path_after_execution_before_capture() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let inv = fx.invocation(
        "data sql",
        &[("--sql", "SELECT ID FROM Sample WHERE ID IN (1)")],
    );
    let path = fx.client.parent().unwrap().join("Data/data/0000000001.idx");
    let original = fs::read(&path).unwrap();
    let count = || {
        let metadata = Metadata::open(&fx.home, false).unwrap();
        let count = metadata.list::<Value>("capture/", 100).unwrap().len();
        metadata.close().unwrap();
        count
    };
    let before = count();
    let error = sql_inner_after_execute(&inv, &mut || {
        let mut changed = original.clone();
        changed[40] ^= 1;
        fs::write(&path, changed)?;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "records.pinned_build_changed");
    assert_eq!(count(), before);
    fs::write(&path, original).unwrap();
    assert_eq!(sql(&inv).unwrap().data["result"]["rows"], json!([[1]]));
}

#[test]
fn sql_detects_new_index_generation_after_explain_before_capture() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let inv = fx.invocation("data sql", &[("--sql", "EXPLAIN SELECT ID FROM Sample")]);
    let directory = fx.client.parent().unwrap().join("Data/data");
    let original = fs::read(directory.join("0000000001.idx")).unwrap();
    let path = directory.join("0000000002.idx");
    let error = sql_inner_after_execute(&inv, &mut || {
        fs::write(&path, &original)?;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "records.pinned_build_changed");
    fs::remove_file(path).unwrap();
    assert!(sql(&inv).unwrap().data["result"]["plan"].is_object());
}

fn root_read(fx: &Fixture, session: bool) -> Result<File> {
    let inv = fx.invocation("asset inspect", &[("--file-id", "201")]);
    let pin = selection::resolve_pinned(&inv)?.spec.data.unwrap();
    if session {
        let budget = Arc::new(Budget::controlled(
            Cost::defaults(),
            inv.control.deadline(),
            inv.control.clone(),
        ));
        let mut reader = sql_reader::Reader::new(&inv, &pin, &budget)?;
        let result = reader.read(201);
        reader.close_after(result)
    } else {
        Ok(read_files(&inv, &pin, &[201], inv.control.deadline())?.remove(0))
    }
}

#[test]
fn actual_root_reference_handoff_matches_cold_warm_cache_and_complete_body_in_both_consumers() {
    for session in [false, true] {
        let item = table("Sample", 201, &[("ID", "uint")], &[vec![U(7)]]);
        let expected = item.raw.clone();
        let fx = Fixture::new(&[item]);
        let mut reference = Value::Null;
        for warm in [false, true] {
            let file = root_read(&fx, session).unwrap();
            assert_eq!(&*file.bytes, expected.as_slice());
            let root = &file.reading["root"];
            let bytes = evidence::read_blob(&fx.home, root, 256 << 20).unwrap();
            assert_eq!(root["bytes"], bytes.len());
            assert_eq!(root["sha256"], lycheedev_core::storage::sha256(&bytes));
            let metadata = Metadata::open(&fx.home, false).unwrap();
            let aliases = metadata.list::<Value>("decoded-v1/", 100).unwrap();
            metadata.close().unwrap();
            assert_eq!(
                aliases
                    .iter()
                    .filter(|entry| entry["blob"] == *root)
                    .count(),
                1
            );
            assert_eq!(file.reading["decodedCache"]["contentHashCalls"], 2);
            assert_eq!(
                file.reading["decodedCache"]["contentDecodeCalls"],
                if warm { 0 } else { 2 }
            );
            assert_eq!(
                file.reading["decodedCache"]["hits"],
                if warm { 2 } else { 0 }
            );
            if warm {
                assert_eq!(*root, reference);
            } else {
                reference = root.clone();
            }
        }
        fs::remove_dir_all(fx.root).unwrap();
    }
}

#[test]
fn actual_root_reference_handoff_rejects_same_length_warm_digest_corruption_in_both_consumers() {
    for session in [false, true] {
        let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(7)]])]);
        let file = root_read(&fx, session).unwrap();
        let reference = &file.reading["root"];
        let path = evidence::blob_path(&fx.home, reference["sha256"].as_str().unwrap()).unwrap();
        let original = fs::read(&path).unwrap();
        let mut changed = original.clone();
        changed[0] ^= 1;
        fs::write(&path, changed).unwrap();
        let error = root_read(&fx, session).err().unwrap();
        assert_eq!(error.code, "vault.blob_integrity");
        fs::write(&path, original).unwrap();
        assert_eq!(root_read(&fx, session).unwrap().reading["root"], *reference);
        fs::remove_dir_all(fx.root).unwrap();
    }
}

#[test]
fn actual_sql_profile_uses_one_recorder_through_prepare_payload_capture_and_checked_close() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(7)]])]);
    let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
    let (budget, profile) = Budget::controlled(
        Cost::defaults(),
        inv.control.deadline(),
        inv.control.clone(),
    )
    .with_test_phase_profile();
    let budget = Arc::new(budget);
    let out = sql_inner_budgeted(&inv, &budget, &mut || Ok(())).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[7]]));
    assert!(out.data.get("phases").is_none());
    let body = evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), out.data);
    assert!(profile.lock().unwrap().is_empty());
    drop(budget);
    let observations = profile.lock().unwrap();
    assert_eq!(observations.len(), 1);
    let phases = observations[0]["phases"].as_array().unwrap();
    for name in [
        "admission",
        "prepare_inclusive",
        "payload_inclusive",
        "source_capture_metadata_inclusive",
        "source_close",
    ] {
        assert!(
            phases.iter().find(|phase| phase["phase"] == name).unwrap()["calls"]
                .as_u64()
                .unwrap()
                > 0,
            "{name}"
        );
    }
    for name in [
        "texture_decode",
        "encode",
        "artifact_capture_metadata_inclusive",
        "output",
    ] {
        assert_eq!(
            phases.iter().find(|phase| phase["phase"] == name).unwrap()["calls"],
            0,
            "{name}"
        );
    }
    drop(observations);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn actual_verified_delivery_closes_owned_blob_after_late_source_and_reader_close_failures() {
    for source_change in [true, false] {
        let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(7)]])]);
        let inv = fx.invocation("asset inspect", &[("--file-id", "201")]);
        let pin = selection::resolve_pinned(&inv).unwrap().spec.data.unwrap();
        let (budget, profile) = Budget::controlled(
            Cost::defaults(),
            inv.control.deadline(),
            inv.control.clone(),
        )
        .with_test_phase_profile();
        let budget = Arc::new(budget);
        let catalog = fx.client.parent().unwrap().join(".build.info");
        let original = fs::read(&catalog).unwrap();
        let close = fault(
            "storage.close_failed",
            5,
            "injected after actual reader checked close",
        );
        let mut delivered = false;
        let mut closed = false;
        let error = read_verified_file_after_read(
            &inv,
            &pin,
            201,
            &budget,
            &mut || {
                delivered = true;
                if source_change {
                    let mut changed = original.clone();
                    changed.push(b'\n');
                    fs::write(&catalog, changed)?;
                }
                Ok(())
            },
            &mut |result| {
                closed = true;
                if source_change {
                    assert_eq!(
                        result.as_ref().unwrap_err().code,
                        "records.pinned_build_changed"
                    );
                    result
                } else {
                    result?;
                    Err(close.clone())
                }
            },
        )
        .err()
        .unwrap();
        assert!(delivered && closed);
        assert_eq!(
            error.code,
            if source_change {
                "records.pinned_build_changed"
            } else {
                "storage.close_failed"
            }
        );
        // The destination rename is a real Windows deny-delete-owner probe.
        // It happens only after the batch has failed and closed the delivered blob.
        let metadata = Metadata::open(&fx.home, false).unwrap();
        let aliases = metadata.list::<Value>("decoded-v1/", 100).unwrap();
        metadata.close().unwrap();
        for entry in aliases {
            let path =
                evidence::blob_path(&fx.home, entry["blob"]["sha256"].as_str().unwrap()).unwrap();
            let moved = path.with_extension("closed-probe");
            fs::rename(&path, &moved).unwrap();
            fs::rename(&moved, &path).unwrap();
        }
        drop(budget);
        let observations = profile.lock().unwrap();
        assert_eq!(observations.len(), 1);
        assert_eq!(
            observations[0]["phases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["phase"] == "source_close")
                .unwrap()["calls"],
            2
        );
        drop(observations);
        fs::write(catalog, original).unwrap();
        fs::remove_dir_all(fx.root).unwrap();
    }
}

#[test]
fn actual_verified_batch_closes_every_constructed_owner_on_later_error_and_delivery_stop() {
    use std::sync::atomic::Ordering;
    for delivery_stop in [false, true] {
        let fx = Fixture::new(&[
            table("First", 201, &[("ID", "uint")], &[vec![U(7)]]),
            table("Second", 202, &[("ID", "uint")], &[vec![U(8)]]),
        ]);
        let inv = fx.invocation("asset inspect", &[("--file-id", "201")]);
        let pin = selection::resolve_pinned(&inv).unwrap().spec.data.unwrap();
        let (budget, profile) = Budget::controlled(
            Cost::defaults(),
            inv.control.deadline(),
            inv.control.clone(),
        )
        .with_test_phase_profile();
        let budget = Arc::new(budget);
        let ids: &[u32] = if delivery_stop {
            &[201, 202]
        } else {
            &[201, 202, 999]
        };
        let mut transferred = false;
        let error = read_batches(&inv, &pin, ids, &budget, ReadMode::Verified, &mut |files| {
            assert!(
                delivery_stop,
                "a partial batch must never reach its callback"
            );
            assert_eq!(files.len(), 2);
            // Exercise the same final-check/transfer consumer with both real
            // verified owners, stopping before it can transfer either one.
            inv.control.cancel_handle().store(true, Ordering::Release);
            deliver_resolved_files(files, &budget, &mut |_| {
                transferred = true;
                Ok(Vec::new())
            })
        })
        .unwrap_err();
        assert!(!transferred);
        assert_eq!(
            error.code,
            if delivery_stop {
                "command.cancelled"
            } else {
                "records.content_missing"
            }
        );
        let metadata = Metadata::open(&fx.home, false).unwrap();
        let aliases = metadata.list::<Value>("decoded-v1/", 100).unwrap();
        metadata.close().unwrap();
        assert_eq!(
            aliases.len(),
            3,
            "Root and both actual selected payloads were constructed"
        );
        for entry in aliases {
            let path =
                evidence::blob_path(&fx.home, entry["blob"]["sha256"].as_str().unwrap()).unwrap();
            let moved = path.with_extension("closed-probe");
            fs::rename(&path, &moved).unwrap();
            fs::rename(&moved, &path).unwrap();
        }
        drop(budget);
        let observations = profile.lock().unwrap();
        assert_eq!(observations.len(), 1);
        assert_eq!(
            observations[0]["phases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["phase"] == "source_close")
                .unwrap()["calls"],
            3
        );
        drop(observations);
        fs::remove_dir_all(fx.root).unwrap();
    }
}
