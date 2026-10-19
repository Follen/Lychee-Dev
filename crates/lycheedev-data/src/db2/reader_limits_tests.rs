//! Exact request propagation and real secondary parsers on small authenticated
//! fixtures. These do not claim acceptance of an actual >256 MiB table.
use super::*;
use crate::{capture::committed_tests, stream};
use committed_tests::fixture::{self, Cell::U, Fixture, table};
use serde_json::Value;
use std::fs;

#[allow(dead_code)]
#[path = "../../tests/support/partial_contract.rs"]
mod partial_contract;

fn assert_limits(limits: table::TableLimits, maximum: usize) {
    // Independent fixed-Go query constants; do not derive expected values from
    // the constructor under test or change the general SDK defaults.
    assert_eq!(limits.file_bytes, maximum);
    assert_eq!(limits.metadata_bytes, 64 << 20);
    assert_eq!(limits.rows, 1_000_000);
    assert_eq!(limits.columns, 4096);
    assert_eq!(limits.partitions, 4096);
    assert_eq!(limits.decode_work, 100_000_000);
}

fn budget(inv: &Invocation) -> Arc<Budget> {
    Arc::new(Budget::query_controlled(
        inv.control.deadline(),
        inv.control.clone(),
    ))
}

// Exercise the real secondary parser with the state that each production
// consumer receives. Public application paths are exercised independently below.
fn reopen<'a>(prepared: &'a Prepared, budget: &Arc<Budget>) -> Result<table::Table<'a>> {
    table::open_budgeted(
        &prepared.file.bytes,
        &prepared.definition,
        prepared.limits,
        &prepared.availability.skipped,
        &prepared.availability.ranges,
        budget.clone(),
    )
}

#[test]
fn actual_preparation_retains_exact_request_limits_without_changing_sdk_defaults() {
    let fx = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![U(1), U(42)]],
    )]);
    for maximum in [None, Some((256 << 20) + 1), Some(512 << 20)] {
        let mut inv = fx.invocation("data db2", &[("--table", "Sample"), ("--id", "1")]);
        if let Some(maximum) = maximum {
            inv.flags.insert("--max-bytes".into(), maximum.to_string());
        }
        let budget = budget(&inv);
        let prepared = prepare(&inv, "Sample", &budget).unwrap();
        assert_limits(prepared.limits, maximum.unwrap_or(128 << 20));
        assert_eq!(prepared.file.bytes.len(), 177);
        let view = reopen(&prepared, &budget).unwrap();
        assert_eq!(view.logical_count(), 1);
        assert_eq!(
            view.row(1, 1 << 20).unwrap()["Value"],
            table::Value::UInt(42)
        );
    }
    let sdk = table::TableLimits::default();
    assert_eq!(sdk.file_bytes, 256 << 20);
    assert_eq!(sdk.rows, 10_000_000);
    assert_eq!(sdk.columns, 65536);
    assert_eq!(sdk.partitions, 65536);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn real_secondary_open_uses_retained_byte_and_logical_copy_caps() {
    let mut input = table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![U(1), U(42)], vec![U(2), U(43)]],
    );
    input.raw[108..112].copy_from_slice(&1u32.to_le_bytes());
    input.raw.extend(3u32.to_le_bytes());
    input.raw.extend(1u32.to_le_bytes());
    let fx = Fixture::new(&[input]);
    let inv = fx.invocation(
        "data db2",
        &[("--table", "Sample"), ("--max-bytes", "536870912")],
    );
    let budget = budget(&inv);
    let mut prepared = prepare(&inv, "Sample", &budget).unwrap();
    assert_limits(prepared.limits, 512 << 20);
    let original = prepared.limits;
    prepared.limits.file_bytes = prepared.file.bytes.len() - 1;
    assert_eq!(
        reopen(&prepared, &budget).err().unwrap().code,
        "table.limit_exceeded"
    );
    prepared.limits.file_bytes += 1;
    assert_eq!(reopen(&prepared, &budget).unwrap().logical_count(), 3);
    // Physical inspection accepts two rows; only the secondary open can reject
    // the third logical row introduced by the tiny copy map.
    prepared.limits.rows = 2;
    table::inspect_budgeted(&prepared.file.bytes, prepared.limits, budget.clone()).unwrap();
    assert_eq!(
        reopen(&prepared, &budget).err().unwrap().code,
        "table.limit_exceeded"
    );
    prepared.limits.rows = 3;
    let view = reopen(&prepared, &budget).unwrap();
    assert_eq!(view.logical_count(), 3);
    assert_eq!(view.row(3, 1 << 20).unwrap()["ID"], table::Value::UInt(3));
    assert_eq!(
        view.row(3, 1 << 20).unwrap()["Value"],
        table::Value::UInt(42)
    );
    drop(view);
    prepared.limits = original;
    assert_limits(prepared.limits, 512 << 20);
    fs::remove_dir_all(fx.root).unwrap();
}

fn verify_captures(inv: &Invocation, outcome: &Outcome) {
    for capture in &outcome.provenance {
        assert_eq!(
            evidence::read_capture(&inv.home, capture["id"].as_str().unwrap()).unwrap(),
            *capture
        );
        let raw = evidence::read_blob(&inv.home, &capture["blob"], 16 << 20).unwrap();
        serde_json::from_slice::<Value>(&raw).unwrap();
    }
}

#[test]
fn public_secondary_owners_keep_rows_captures_and_stream_terminal_at_maximum_request() {
    let fx = Fixture::new(&committed_tests::tables());
    let base = fx.invocation("", &[("--max-bytes", "536870912")]);
    for (inv, count) in committed_tests::invocations(&base) {
        let out = crate::dispatch(&inv).unwrap();
        assert!(out.complete, "{}", inv.command);
        assert_eq!(out.provenance.len(), count, "{}", inv.command);
        verify_captures(&inv, &out);
        if inv.command == "data db2" && inv.flag("--id").is_some() {
            assert_eq!(out.data["row"]["Value"], 42);
        }
        if inv.command == "data spell auras" {
            assert_eq!(out.data["hasAura"], json!([7]));
        }
    }
    let mut inv = fx.invocation(
        "data db2 stream",
        &[
            ("--max-bytes", "536870912"),
            ("--format", "jsonl"),
            ("--limit", "10"),
        ],
    );
    inv.positional = Some("Sample".into());
    let mut frames = Vec::new();
    let out = stream::stream(&inv, &mut |frame| {
        frames.push(frame.clone());
        Ok(())
    })
    .unwrap();
    assert!(out.complete);
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[1]["record"]["row"]["Value"], 42);
    assert_eq!(frames[2]["frame"], "end");
    assert_eq!(frames[2]["end"]["complete"], true);
    verify_captures(&inv, &out);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn dynamic_encounter_batches_retain_identical_limits_and_real_related_spell_capture() {
    let fx = Fixture::new(&[
        table(
            "JournalEncounterSection",
            331,
            &[
                ("ID", "uint"),
                ("JournalEncounterID", "uint"),
                ("ParentSectionID", "uint"),
                ("SpellID", "uint"),
            ],
            &[vec![U(1), U(132), U(0), U(7)]],
        ),
        table(
            "SpellName",
            332,
            &[("ID", "uint"), ("Name_lang", "text")],
            &[vec![U(7), fixture::Cell::Text("Named spell".into())]],
        ),
        table(
            "SpellMisc",
            333,
            &[
                ("ID", "uint"),
                ("SpellID", "uint"),
                ("SpellIconFileDataID", "uint"),
            ],
            &[vec![U(1), U(7), U(0)]],
        ),
    ]);
    let inv = fx.invocation(
        "data encounter get",
        &[
            ("--journal-encounter-id", "132"),
            ("--max-bytes", "536870912"),
        ],
    );
    let budget = budget(&inv);
    let mut names = Vec::new();
    let prepared = prepare_following(&inv, &["JournalEncounterSection"], &budget, &mut |batch| {
        assert_eq!(batch.len(), 1);
        assert_limits(batch[0].limits, 512 << 20);
        assert_eq!(reopen(&batch[0], &budget)?.logical_count(), 1);
        names.push(batch[0].bundle.identity.name.clone());
        Ok(match batch[0].bundle.identity.name.as_str() {
            "JournalEncounterSection" => vec!["SpellName"],
            "SpellName" => vec!["SpellMisc"],
            "SpellMisc" => Vec::new(),
            other => panic!("unexpected follow-up table: {other}"),
        })
    })
    .unwrap();
    assert_eq!(names, ["JournalEncounterSection", "SpellName", "SpellMisc"]);
    for prepared in &prepared {
        assert_limits(prepared.limits, 512 << 20);
        assert_eq!(reopen(prepared, &budget).unwrap().logical_count(), 1);
    }
    let out = crate::dispatch(&inv).unwrap();
    assert!(out.complete);
    assert_eq!(out.provenance.len(), 2);
    assert_eq!(out.data["tables"].as_array().unwrap().len(), 3);
    assert_eq!(out.data["relatedSpells"][0]["name"], "Named spell");
    verify_captures(&inv, &out);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn maximum_request_preserves_partial_coverage_rows_and_capture_contracts() {
    let input = fixture::encrypted_sections(201);
    let encoded = fixture::encrypted_frame(&input.raw).1;
    let fx = Fixture::with_encoded_table(input, encoded);
    let key_file = fx.root.join("unrelated-keys.txt");
    fs::write(
        &key_file,
        b"0000000000000001 000102030405060708090a0b0c0d0e0f\n",
    )
    .unwrap();
    let mut inv = fx.invocation("data db2 schema", &[("--max-bytes", "536870912")]);
    inv.flags
        .insert("--key-file".into(), key_file.to_string_lossy().into_owned());
    inv.positional = Some("Sample".into());
    let budget = budget(&inv);
    let prepared = prepare(&inv, "Sample", &budget).unwrap();
    assert_limits(prepared.limits, 512 << 20);
    let view = reopen(&prepared, &budget).unwrap();
    assert!(!view.is_complete());
    assert_eq!(
        view.row(1, 1 << 20).unwrap()["Value"],
        table::Value::UInt(42)
    );
    assert_eq!(prepared.availability.skipped.len(), 1);
    drop(view);
    partial_contract::exercise(&inv, &fx.root.join("partial.csv"));
    fs::remove_dir_all(fx.root).unwrap();
}
