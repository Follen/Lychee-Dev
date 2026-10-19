//! F4 reproductions through authenticated local providers, not SDK-only limits.
//! Header-only inputs isolate count admission before intentionally absent bodies.
//! Small compressed content isolates the actual request's decoded-byte boundary.
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod fixture;

use fixture::{Cell::U, Fixture, Table, table};
use lycheedev_core::{Invocation, Outcome, Result, evidence, metadata::Metadata};
use lycheedev_data::{dispatch, stream};
use serde_json::Value;
use std::{fs, io::Write, sync::atomic::Ordering};

fn tables() -> Vec<Table> {
    vec![
        table(
            "Sample",
            201,
            &[("ID", "uint"), ("Value", "uint")],
            &[vec![U(1), U(42)]],
        ),
        table(
            "SpellEffect",
            303,
            &[
                ("ID", "uint"),
                ("SpellID", "uint"),
                ("EffectIndex", "uint"),
                ("Effect", "uint"),
                ("EffectMiscValue", "uint"),
            ],
            &[vec![U(1), U(7), U(0), U(6), U(0)]],
        ),
        fixture::decors(&fixture::decor_rows()),
        table(
            "JournalEncounterSection",
            331,
            &[
                ("ID", "uint"),
                ("JournalEncounterID", "uint"),
                ("ParentSectionID", "uint"),
                ("SpellID", "uint"),
            ],
            &[vec![U(1), U(7), U(0), U(0)]],
        ),
    ]
}

fn invocations(fx: &Fixture) -> Vec<Invocation> {
    let mut requests = vec![
        fx.invocation("data db2", &[("--table", "Sample"), ("--id", "1")]),
        fx.invocation("data db2", &[("--table", "Sample"), ("--limit", "1")]),
        fx.invocation("data db2 schema", &[]),
        fx.invocation(
            "data db2 search",
            &[("--field", "Value"), ("--query", "42"), ("--limit", "1")],
        ),
        fx.invocation(
            "data db2 foreign-key",
            &[("--field", "Value"), ("--value", "42"), ("--limit", "1")],
        ),
        fx.invocation(
            "data db2 stream",
            &[("--format", "jsonl"), ("--limit", "1")],
        ),
        fx.invocation("data spell auras", &[("--spell-id", "7")]),
        fx.invocation("data decor list", &[("--limit", "1")]),
        fx.invocation("data encounter get", &[("--journal-encounter-id", "7")]),
        fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample LIMIT 1")]),
    ];
    for request in &mut requests {
        if matches!(
            request.command.as_str(),
            "data db2 schema" | "data db2 search" | "data db2 foreign-key" | "data db2 stream"
        ) {
            request.positional = Some("Sample".into());
        }
    }
    requests
}

fn run(inv: &Invocation, frames: &mut Vec<Value>) -> Result<Outcome> {
    if inv.command == "data db2 stream" {
        stream::stream(inv, &mut |frame| {
            frames.push(frame.clone());
            Ok(())
        })
    } else {
        dispatch(inv)
    }
}

fn no_capture(fx: &Fixture) {
    let metadata = Metadata::open(&fx.home, false).unwrap();
    assert!(metadata.list::<Value>("capture/", 100).unwrap().is_empty());
    metadata.close().unwrap();
}

fn header_boundary(offset: usize, maximum: u32) {
    for count in [maximum - 1, maximum, maximum + 1] {
        let mut inputs = tables();
        for input in &mut inputs {
            input.raw.truncate(72);
            input.raw[offset..offset + 4].copy_from_slice(&count.to_le_bytes());
        }
        let fx = Fixture::new(&inputs);
        for inv in invocations(&fx) {
            let mut frames = Vec::new();
            let error = run(&inv, &mut frames).unwrap_err();
            assert_eq!(
                error.code,
                if count > maximum {
                    "table.limit_exceeded"
                } else {
                    "table.invalid_format"
                },
                "{} offset={offset} count={count}",
                inv.command
            );
            assert_eq!(error.exit_code, if count > maximum { 3 } else { 4 });
            assert!(frames.is_empty(), "preparation failure must precede Begin");
            assert!(error.details.get("completedCaptures").is_none());
        }
        no_capture(&fx);
        fs::remove_dir_all(fx.root).unwrap();
    }
}

#[test]
fn public_table_consumers_enforce_one_million_physical_rows() {
    header_boundary(4, 1_000_000);
}
#[test]
fn public_table_consumers_enforce_4096_physical_columns() {
    header_boundary(8, 4096);
}
#[test]
fn public_table_consumers_enforce_4096_total_columns() {
    header_boundary(44, 4096);
}
#[test]
fn public_table_consumers_enforce_4096_partitions() {
    header_boundary(68, 4096);
}

#[test]
fn actual_max_bytes_boundary_preserves_complete_captures_and_stream_terminal() {
    let input = tables().remove(0);
    let bytes = input.raw.len();
    let mut compressed = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    compressed.write_all(&input.raw).unwrap();
    let mut encoded = b"BLTE\0\0\0\0Z".to_vec();
    encoded.extend(compressed.finish().unwrap());
    assert!(
        encoded.len() < bytes,
        "decoded size must be the controlling boundary"
    );
    let fx = Fixture::with_encoded_table(input, encoded);
    let requests = invocations(&fx)
        .into_iter()
        .filter(|inv| inv.command.starts_with("data db2") || inv.command == "data sql")
        .collect::<Vec<_>>();
    for mut inv in requests {
        inv.flags
            .insert("--max-bytes".into(), (bytes - 1).to_string());
        let mut frames = Vec::new();
        let error = run(&inv, &mut frames).unwrap_err();
        assert_eq!(error.code, "records.metadata_limit", "{}", inv.command);
        assert_eq!(error.exit_code, 3);
        assert!(frames.is_empty());
        assert!(error.details.get("completedCaptures").is_none());
        for maximum in [bytes, bytes + 1, 512 << 20] {
            inv.flags.insert("--max-bytes".into(), maximum.to_string());
            let mut frames = Vec::new();
            let outcome = run(&inv, &mut frames).unwrap();
            assert!(outcome.complete, "{}", inv.command);
            assert!(!outcome.provenance.is_empty());
            for capture in &outcome.provenance {
                let stored =
                    evidence::read_capture(&fx.home, capture["id"].as_str().unwrap()).unwrap();
                assert_eq!(&stored, capture);
                evidence::read_blob(&fx.home, &stored["blob"], 16 << 20).unwrap();
            }
            if inv.command == "data db2 stream" {
                assert_eq!(frames.len(), 3);
                assert_eq!(frames[0]["frame"], "begin");
                assert_eq!(frames[1]["record"]["row"]["Value"], 42);
                assert_eq!(frames[2]["frame"], "end");
                assert_eq!(frames[2]["end"]["complete"], true);
            }
        }
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn cancelled_public_table_requests_keep_original_stop_before_caps_and_capture() {
    let mut inputs = tables();
    for input in &mut inputs {
        input.raw.truncate(72);
        input.raw[4..8].copy_from_slice(&1_000_001u32.to_le_bytes());
    }
    let fx = Fixture::new(&inputs);
    for inv in invocations(&fx) {
        inv.control.cancel_handle().store(true, Ordering::Release);
        let mut frames = Vec::new();
        let error = run(&inv, &mut frames).unwrap_err();
        assert_eq!(error.exit_code, 7, "{}", inv.command);
        assert_ne!(error.code, "table.limit_exceeded");
        assert!(frames.is_empty());
        assert!(error.details.get("completedCaptures").is_none());
    }
    no_capture(&fx);
    fs::remove_dir_all(fx.root).unwrap();
}
