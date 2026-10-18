//! Actual authenticated local DB2/navigation/stream consumers. These synthetic
//! CASC fixtures exercise the public application callbacks, not a fresh source
//! opened after mutation, and are not evidence for an installed game client.
use crate::{capture::committed_tests, db2, navigate, stream};
use committed_tests::fixture::{self, Cell, Fixture, table};
use lycheedev_core::{Invocation, Outcome, Result, evidence, metadata::Metadata};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::PathBuf};

fn captures(fx: &Fixture) -> BTreeSet<String> {
    let metadata = Metadata::open(&fx.home, false).unwrap();
    let records = metadata.list::<Value>("capture/", 10000).unwrap();
    metadata.close().unwrap();
    records
        .into_iter()
        .map(|record| record["id"].as_str().unwrap().to_owned())
        .collect()
}
fn after_read(inv: &Invocation, callback: &mut impl FnMut() -> Result<()>) -> Result<Outcome> {
    if inv.command.starts_with("data db2") {
        db2::dispatch_after_read(inv, callback)
    } else {
        navigate::dispatch_after_read(inv, callback)
    }
}

#[derive(Clone, Copy, Debug)]
enum Change {
    ClientBuild,
    IndexBytes,
    IndexGeneration,
}
impl Change {
    fn path(self, fx: &Fixture) -> PathBuf {
        match self {
            Self::ClientBuild => fx.client.join("version.txt"),
            Self::IndexBytes => fx.client.parent().unwrap().join("Data/data/0000000001.idx"),
            Self::IndexGeneration => fx.client.parent().unwrap().join("Data/data/0000000002.idx"),
        }
    }
    fn apply(self, fx: &Fixture) -> Result<Option<Vec<u8>>> {
        let path = self.path(fx);
        if matches!(self, Self::IndexGeneration) {
            assert!(!path.exists());
            fs::copy(Self::IndexBytes.path(fx), &path)?;
            return Ok(None);
        }
        let original = fs::read(&path)?;
        let changed = match self {
            Self::ClientBuild => b"12.1.0.69934".to_vec(),
            Self::IndexBytes => {
                let mut changed = original.clone();
                changed[40] ^= 1;
                assert_eq!(changed.len(), original.len());
                changed
            }
            Self::IndexGeneration => unreachable!(),
        };
        fs::write(&path, changed)?;
        Ok(Some(original))
    }
    fn restore(self, fx: &Fixture, original: Option<Vec<u8>>) {
        match original {
            Some(original) => fs::write(self.path(fx), original).unwrap(),
            None => fs::remove_file(self.path(fx)).unwrap(),
        }
    }
}

fn rejects_changed_source(change: Change) {
    let mut tables = committed_tests::tables();
    tables.retain(|table| table.name != "JournalEncounterSection");
    tables.extend([
        table(
            "JournalEncounterSection",
            331,
            &[
                ("ID", "uint"),
                ("JournalEncounterID", "uint"),
                ("ParentSectionID", "uint"),
                ("SpellID", "uint"),
            ],
            &[vec![Cell::U(1), Cell::U(132), Cell::U(0), Cell::U(7)]],
        ),
        table(
            "SpellName",
            332,
            &[("ID", "uint"), ("Name_lang", "text")],
            &[vec![Cell::U(7), Cell::Text("Named spell".into())]],
        ),
        table(
            "SpellMisc",
            333,
            &[
                ("ID", "uint"),
                ("SpellID", "uint"),
                ("SpellIconFileDataID", "uint"),
            ],
            &[vec![Cell::U(1), Cell::U(7), Cell::U(0)]],
        ),
    ]);
    tables.extend(fixture::items());
    let fx = Fixture::new(&tables);
    let mut invocations = committed_tests::invocations(&fx.invocation("", &[]));
    invocations.push((fx.invocation("data item get", &[("--item-id", "7")]), 1));
    for (inv, expected_captures) in invocations {
        let before = captures(&fx);
        let mut calls = 0;
        let mut original = None;
        let result = after_read(&inv, &mut || {
            calls += 1;
            assert_eq!(calls, 1, "must not replay after_read");
            original = change.apply(&fx)?;
            Ok(())
        });
        assert_eq!(calls, 1, "{}: {change:?}", inv.command);
        change.restore(&fx, original);
        let error = result.unwrap_err();
        assert_eq!(
            error.code, "records.pinned_build_changed",
            "{}: {change:?}",
            inv.command
        );
        assert_eq!(error.exit_code, 4);
        assert!(error.details.get("completedCaptures").is_none());
        assert_eq!(
            captures(&fx),
            before,
            "No stale result may acquire a CAP identity"
        );

        // The same pinned request succeeds once the original source is restored.
        // Verify the persisted bytes, not merely the number of returned IDs.
        let out = after_read(&inv, &mut || Ok(())).unwrap();
        assert!(out.complete, "{}", inv.command);
        assert_eq!(out.provenance.len(), expected_captures);
        if inv.command == "data encounter get" {
            assert_eq!(out.data["tables"].as_array().unwrap().len(), 3);
            assert_eq!(out.data["relatedSpells"][0]["name"], "Named spell");
        }
        if inv.command == "data item get" {
            assert_eq!(out.data["tables"].as_array().unwrap().len(), 2);
            assert_eq!(out.data["item"]["id"], 7);
        }
        let first = &out.provenance[0];
        assert_eq!(
            evidence::read_capture(&fx.home, first["id"].as_str().unwrap()).unwrap(),
            *first
        );
        let raw = evidence::read_blob(&fx.home, &first["blob"], 16 << 20).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), out.data);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn local_db2_and_navigation_recheck_original_client_after_actual_consumer() {
    rejects_changed_source(Change::ClientBuild);
}

#[test]
fn local_db2_and_navigation_rehash_original_same_size_index_after_actual_consumer() {
    rejects_changed_source(Change::IndexBytes);
}

#[test]
fn local_db2_and_navigation_reject_new_index_generation_after_actual_consumer() {
    rejects_changed_source(Change::IndexGeneration);
}

#[test]
fn local_stream_source_changes_keep_delivered_rows_without_end_capture_or_replay() {
    for change in [
        Change::ClientBuild,
        Change::IndexBytes,
        Change::IndexGeneration,
    ] {
        let fx = Fixture::new(&[table(
            "Sample",
            201,
            &[("ID", "uint"), ("Value", "uint")],
            &[vec![Cell::U(1), Cell::U(42)], vec![Cell::U(2), Cell::U(43)]],
        )]);
        let mut inv = fx.invocation(
            "data db2 stream",
            &[("--format", "jsonl"), ("--limit", "50")],
        );
        inv.positional = Some("Sample".into());
        let before = captures(&fx);
        let mut frames = Vec::new();
        let mut mutated = false;
        let mut original = None;
        let result = stream::stream(&inv, &mut |frame| {
            frames.push(frame.clone());
            if frame["frame"] == "record" && !mutated {
                mutated = true;
                original = change.apply(&fx)?;
            }
            Ok(())
        });
        assert!(mutated);
        change.restore(&fx, original);
        let error = result.unwrap_err();
        assert_eq!(error.code, "records.pinned_build_changed", "{change:?}");
        assert_eq!(error.exit_code, 4);
        assert!(error.details.get("completedCaptures").is_none());
        assert_eq!(captures(&fx), before);
        assert_eq!(
            frames
                .iter()
                .map(|f| f["frame"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["begin", "record", "record", "error"]
        );
        assert_eq!(frames[1]["record"]["row"], json!({"ID":1,"Value":42}));
        assert_eq!(frames[2]["record"]["row"], json!({"ID":2,"Value":43}));
        assert_eq!(frames[3]["error"]["code"], "records.pinned_build_changed");

        let mut recovered = Vec::new();
        let out = stream::stream(&inv, &mut |frame| {
            recovered.push(frame.clone());
            Ok(())
        })
        .unwrap();
        assert!(out.complete);
        assert_eq!(out.provenance.len(), 1);
        assert_eq!(
            recovered
                .iter()
                .map(|f| f["frame"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["begin", "record", "record", "end"]
        );
        assert_eq!(&recovered[1..3], &frames[1..3]);
        let raw = evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&raw).unwrap(),
            json!({"begin":recovered[0]["begin"],"end":recovered[3]["end"]})
        );
        fs::remove_dir_all(fx.root).unwrap();
    }
}

fn array_fixture(name: &'static str, field: &'static str, bad_string: bool) -> Fixture {
    Fixture::new(&[table(
        name,
        201,
        &[("ID", "uint"), (field, "array3"), ("Unused", "text")],
        &[vec![
            Cell::U(1),
            Cell::Array(vec![Cell::U(42), Cell::U(7), Cell::U(8)]),
            if bad_string {
                Cell::BadString
            } else {
                Cell::Text("readable".into())
            },
        ]],
    )])
}
fn assert_type_error_before_scan(fx: &Fixture, inv: &Invocation) {
    let before = captures(fx);
    let error = after_read(inv, &mut || {
        panic!("numeric array rejection must precede consumer completion")
    })
    .unwrap_err();
    assert_eq!(error.code, "records.field_type_mismatch");
    assert_eq!(error.exit_code, 2);
    assert_eq!(captures(fx), before);
}

#[test]
fn local_db2_foreign_key_rejects_typed_integer_array_before_scan_or_capture() {
    let fx = array_fixture("Sample", "Value", true);
    let mut inv = fx.invocation(
        "data db2 foreign-key",
        &[("--field", "Value"), ("--value", "42"), ("--limit", "50")],
    );
    inv.positional = Some("Sample".into());
    assert_type_error_before_scan(&fx, &inv);
    let error = db2::dispatch(&inv).unwrap_err();
    assert_eq!(error.details, json!({"stage":"query"}));
    // Prove that scanning this fixture really encounters an unrelated bad row.
    inv.command = "data db2 search".into();
    inv.flags.insert("--query".into(), "42".into());
    assert_eq!(
        db2::dispatch(&inv).unwrap_err().code,
        "table.invalid_format"
    );
    assert!(captures(&fx).is_empty());
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn local_db2_array_search_and_scalar_foreign_key_keep_successful_results() {
    let fx = array_fixture("Sample", "Value", false);
    let mut inv = fx.invocation(
        "data db2 search",
        &[("--field", "Value"), ("--query", "42 7"), ("--limit", "50")],
    );
    inv.positional = Some("Sample".into());
    let out = db2::dispatch(&inv).unwrap();
    assert!(out.complete);
    assert_eq!(out.data["count"], 1);
    assert_eq!(out.data["rows"][0]["Value"], json!([42, 7, 8]));
    fx.verify(&out);
    inv.command = "data db2 foreign-key".into();
    inv.flags.insert("--field".into(), "ID".into());
    inv.flags.insert("--value".into(), "1".into());
    let out = db2::dispatch(&inv).unwrap();
    assert!(out.complete);
    assert_eq!(out.data["count"], 1);
    assert_eq!(out.data["rows"][0]["ID"], 1);
    fx.verify(&out);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn local_spell_profiles_reject_integer_array_relation_before_scan_or_capture() {
    let fx = array_fixture("SpellEffect", "SpellID", true);
    for command in ["data spell auras", "data spell summons"] {
        let inv = fx.invocation(command, &[("--spell-id", "42")]);
        assert_type_error_before_scan(&fx, &inv);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn local_catalog_navigation_rejects_integer_array_relation_before_scan_or_capture() {
    let fx = array_fixture("HouseDecor", "ItemID", true);
    let inv = fx.invocation("data decor get", &[("--item-id", "42")]);
    assert_type_error_before_scan(&fx, &inv);
    // Preserve the independent scalar relation success through the same path.
    let valid = Fixture::new(&[fixture::decors(&fixture::decor_rows())]);
    let inv = valid.invocation("data decor get", &[("--item-id", "42")]);
    let out = navigate::dispatch(&inv).unwrap();
    assert_eq!(out.data["item"]["itemID"], 42);
    assert_eq!(out.provenance.len(), 1);
    valid.verify(&out);
    fs::remove_dir_all(fx.root).unwrap();
    fs::remove_dir_all(valid.root).unwrap();
}
