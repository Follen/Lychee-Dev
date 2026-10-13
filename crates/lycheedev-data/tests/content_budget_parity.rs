//! Selected encoded/decoded allowance is independent from metadata caps.
//! Synthetic authenticated inputs. Large-content preparation is explicit so the
//! frozen Go and Rust public commands can observe the same authenticated body.
#[path = "support/catalog_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use fixture::{Cell, Fixture, table};
use lycheedev_core::{Invocation, selection};
use lycheedev_data::local_query;
use std::{
    io::{Read, Seek, SeekFrom, Write},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn input() -> (Fixture, Vec<u8>) {
    let table = table("ContentBudget", 501, &[("ID", "uint")], &[vec![Cell::U(7)]]);
    let bytes = table.raw.clone();
    (Fixture::new(&[table]), bytes)
}
fn read(inv: &Invocation) -> lycheedev_core::Result<Vec<local_query::File>> {
    let pin = selection::resolve_pinned(inv)?;
    local_query::read_files(
        inv,
        pin.data.as_ref().unwrap(),
        &[501],
        Instant::now() + Duration::from_secs(30),
    )
}

#[test]
fn maximum_content_allowance_small_file_cold_and_warm() {
    let (fx, expected) = input();
    let inv = fx.invocation("asset inspect", &[("--max-bytes", "536870912")]);
    let cold = read(&inv).unwrap().remove(0);
    assert_eq!(cold.bytes, expected);
    assert_eq!(cold.reading["decodedCache"]["hits"], 0);
    assert_eq!(cold.reading["decodedCache"]["contentDecodeCalls"], 2);
    let warm = read(&inv).unwrap().remove(0);
    assert_eq!(warm.bytes, expected);
    assert_eq!(warm.reading["decodedCache"]["hits"], 2);
    assert_eq!(warm.reading["decodedCache"]["contentDecodeCalls"], 0);
    assert_eq!(
        warm.reading["verification"]["contentWholeDecodedCKey"],
        true
    );
}

#[test]
fn sql_and_db2_capture_exact_fixture_rows_and_original_request_identity() {
    use lycheedev_core::{evidence, storage};
    use serde_json::{Value, json};
    let (fx, _) = input();
    let sql = lycheedev_data::dispatch(&fx.invocation(
        "data sql", &[("--sql", "SELECT ID FROM ContentBudget")],
    )).unwrap();
    assert_eq!(sql.data["result"]["rows"], json!([[7]]));
    assert_eq!(sql.data["query"], json!({"sql":"SELECT ID FROM ContentBudget"}));
    // Fixed SHA256 of the pre-existing ordinary JSON request encoding.
    let request = br#"{"sql":"SELECT ID FROM ContentBudget"}"#;
    let digest = "028bd8b6e327d0fff5ade1f22f946a3e2d08da062d4cba5fc3016d165b22aada";
    assert_eq!(storage::sha256(request), digest);
    assert_eq!(sql.provenance[0]["provenance"]["locator"], format!("sql:sha256:{digest}"));

    let parameterized = lycheedev_data::dispatch(&fx.invocation(
        "data sql", &[("--sql", "SELECT ID FROM ContentBudget WHERE ID=:id"), ("--param", "id=7")],
    )).unwrap();
    assert_eq!(parameterized.data["result"]["rows"], json!([[7]]));
    assert_eq!(parameterized.data["query"], json!({"sql":"SELECT ID FROM ContentBudget WHERE ID=:id","parameters":{"id":7}}));
    assert_eq!(parameterized.provenance[0]["provenance"]["locator"],
        "sql:sha256:a8e82349a8b7e4c5ef5e07154b4c06df66b8e35f5c9dc38d2da769a1a30284b8");

    let record = lycheedev_data::dispatch(&fx.invocation(
        "data db2", &[("--table", "ContentBudget"), ("--id", "7")],
    )).unwrap();
    assert_eq!(record.data["recordID"], 7);
    assert_eq!(record.data["row"], json!({"ID":7}));
    assert_eq!(record.provenance[0]["provenance"]["locator"], "db2:ContentBudget:7");
    let page = lycheedev_data::dispatch(&fx.invocation(
        "data db2", &[("--table", "ContentBudget"), ("--limit", "1")],
    )).unwrap();
    assert_eq!(page.data["page"]["ids"], json!([7]));
    assert_eq!(page.data["page"]["rows"], json!([{"ID":7}]));
    assert_eq!(page.data["page"]["more"], false);
    assert_eq!(page.provenance[0]["provenance"]["locator"], "db2:ContentBudget");

    for outcome in [&sql, &parameterized, &record, &page] {
        assert!(outcome.complete);
        assert_eq!(outcome.provenance.len(), 1);
        assert_eq!(outcome.data["resourceAccounting"]["wholeQueryAccountingComplete"], false);
        assert!(outcome.data["resourceAccounting"]["remainingGaps"].as_array().unwrap()
            .iter().all(|gap| gap != "evidence serialization allocations"));
        let capture = &outcome.provenance[0];
        assert_eq!(capture["complete"], true);
        assert_eq!(capture["truncated"], false);
        assert_eq!(evidence::read_capture(&fx.home, capture["id"].as_str().unwrap()).unwrap(), *capture);
        let raw = evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), outcome.data);
        assert_eq!(raw, serde_json::to_vec(&outcome.data).unwrap());
        assert_eq!(capture["blob"]["sha256"], storage::sha256(&raw));
    }
}

#[test]
fn stream_publishes_manifest_before_end_and_has_no_end_or_new_capture_on_failure() {
    use lycheedev_core::{evidence, metadata::Metadata};
    use serde_json::Value;
    let (fx, _) = input();
    let mut inv = fx.invocation("data db2 stream", &[("--format", "jsonl"), ("--limit", "1")]);
    inv.positional = Some("ContentBudget".into());
    let captures = || Metadata::open(&fx.home, false).unwrap().list::<Value>("capture/", 100).unwrap();
    let mut frames = Vec::new();
    let outcome = lycheedev_data::stream::stream(&inv, &mut |frame| {
        if frame["frame"] == "end" {
            assert_eq!(captures().len(), 1, "capture must already be committed at success end");
        }
        frames.push(frame.clone());
        Ok(())
    }).unwrap();
    assert_eq!(frames.iter().map(|frame| frame["frame"].as_str().unwrap()).collect::<Vec<_>>(), ["begin", "record", "end"]);
    let body = evidence::read_blob(&fx.home, &outcome.provenance[0]["blob"], 128 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), serde_json::json!({"begin":frames[0]["begin"],"end":frames[2]["end"]}));
    let before = captures();
    let marker = fx.home.join("workspace.json");
    let original = std::fs::read(&marker).unwrap();
    frames.clear();
    let failed = lycheedev_data::stream::stream(&inv, &mut |frame| {
        if frame["frame"] == "record" {
            // The source has been fully read; fail the real publication boundary.
            std::fs::write(&marker, br#"{"schema":"unsupported"}"#).unwrap();
        }
        frames.push(frame.clone());
        Ok(())
    });
    std::fs::write(&marker, original).unwrap();
    assert_eq!(failed.unwrap_err().code, "vault.workspace_schema_newer");
    assert_eq!(frames.iter().map(|frame| frame["frame"].as_str().unwrap()).collect::<Vec<_>>(), ["begin", "record", "error"]);
    assert_eq!(captures(), before);
    frames.clear();
    let error = lycheedev_data::stream::stream(&inv, &mut |frame| {
        frames.push(frame.clone());
        if frame["frame"] == "record" {
            inv.control.cancel_handle().store(true, Ordering::Release);
        }
        Ok(())
    }).unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert!(!frames.iter().any(|frame| frame["frame"] == "end"));
    assert_eq!(frames.last().unwrap()["error"]["code"], "records.cancelled");
    assert_eq!(captures(), before);
}

#[test]
fn selected_encoded_and_decoded_lengths_are_bounded_on_cold_and_warm_reads() {
    let (fx, expected) = input();
    // The selected headerless BLTE adds nine bytes. Frozen Go applies max to
    // both lengths, while Encoding/Root metadata have a separate allowance.
    let max = (expected.len() + 9).to_string();
    let inv = fx.invocation("asset inspect", &[("--max-bytes", &max)]);
    for _ in 0..2 {
        assert_eq!(read(&inv).unwrap()[0].bytes, expected);
    }
    for too_small in [expected.len() - 1, expected.len(), expected.len() + 8] {
        let maximum = too_small.to_string();
        let inv = fx.invocation("asset inspect", &[("--max-bytes", &maximum)]);
        assert_eq!(read(&inv).err().unwrap().code, "records.metadata_limit");
        let fresh = input().0;
        let inv = fresh.invocation("asset inspect", &[("--max-bytes", &maximum)]);
        assert_eq!(read(&inv).err().unwrap().code, "records.metadata_limit");
    }
}

#[test]
#[ignore = "prepare a shared real CASC fixture for explicit frozen-Go/Rust public cold/warm boundary calls"]
fn prepare_content_budget_public_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LYCHEEDEV_CONTENT_BUDGET_FIXTURE").expect("fixture output directory"),
    );
    let table = table("ContentBudget", 501, &[("ID", "uint")], &[vec![Cell::U(7)]]);
    let decoded = table.raw.len();
    let fx = Fixture::at(root, &[table], "zhCN", 0x40);
    std::fs::write(
        fx.root.join("content-boundaries.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "fileDataID":501,"decodedBytes":decoded,"encodedBytes":decoded+9,
            "cases":[
                {"name":"below-decoded","maximum":decoded-1,"expectedExit":3},
                {"name":"decoded-only","maximum":decoded,"expectedExit":3},
                {"name":"below-encoded","maximum":decoded+8,"expectedExit":3},
                {"name":"exact-encoded","maximum":decoded+9,"expectedExit":0},
                {"name":"maximum-allowance","maximum":536870912,"expectedExit":0}
            ],
            "errorCode":"records.metadata_limit",
            "runs":"asset inspect in json and jsonl; independent cold homes and warm homes primed at maximum-allowance; use input-identity.json snapshot/installation; verify complete capture bytes on success"
        })).unwrap(),
    ).unwrap();
}

#[test]
fn maximum_allowance_preserves_cancellation_and_rechecks_warm_encoded_source() {
    use lycheedev_core::metadata::Metadata;
    let (fx, _) = input();
    let mut inv = fx.invocation("asset inspect", &[("--max-bytes", "536870912")]);
    read(&inv).unwrap();
    let captures = || Metadata::open(&fx.home, false).unwrap()
        .list::<serde_json::Value>("capture/", 100).unwrap();
    let before = captures();
    inv.control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(read(&inv).err().unwrap().exit_code, 7);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code, 7);
    assert_eq!(captures(), before, "cancelled content must not publish a capture");
    inv.control = Default::default();
    // First object is the content: corrupt its payload without altering extent.
    let archive = fx.root.join("game/Data/data/data.000");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(archive)
        .unwrap();
    file.seek(SeekFrom::Start(39)).unwrap();
    file.write_all(b"X").unwrap();
    drop(file);
    assert_eq!(read(&inv).err().unwrap().code, "archive.index_integrity");
}

#[test]
fn malformed_content_allowances_are_arguments_errors() {
    let (fx, _) = input();
    for max in ["0", "536870913", "-1", "bad"] {
        let inv = fx.invocation("asset inspect", &[("--max-bytes", max)]);
        assert_eq!(read(&inv).err().unwrap().exit_code, 2);
    }
}

#[test]
#[ignore = "prepare shared compressed CASC input for explicit public Go/Rust observation"]
fn prepare_compressed_content_public_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LYCHEEDEV_CONTENT_BUDGET_FIXTURE").expect("fixture output directory"),
    );
    let decoded = std::env::var("LYCHEEDEV_CONTENT_BUDGET_BYTES")
        .expect("explicit decoded content size").parse().unwrap();
    Fixture::compressed_at(root, decoded);
}

fn check_compressed_inspect_and_export(fx: &Fixture) {
    use lycheedev_core::evidence;
    use sha2::{Digest, Sha256};
    let identity: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fx.root.join("input-identity.json")).unwrap(),
    ).unwrap();
    let content = &identity["content"];
    let maximum = content["decodedBytes"].as_u64().unwrap().to_string();
    let inv = fx.invocation("asset inspect", &[("--file-id", "501"), ("--max-bytes", &maximum)]);
    for pass in 0..2 {
        let found = lycheedev_data::dispatch(&inv).unwrap();
        assert_eq!(found.provenance.len(), 1);
        assert_eq!(found.data["content"]["bytes"], content["decodedBytes"]);
        assert_eq!(found.data["content"]["sha256"], content["sha256"]);
        assert_eq!(found.provenance[0]["blob"], found.data["content"]);
        assert_eq!(found.provenance[0]["complete"], true);
        assert_eq!(found.provenance[0]["truncated"], false);
        assert_eq!(evidence::read_capture(&fx.home, found.provenance[0]["id"].as_str().unwrap()).unwrap(), found.provenance[0]);
        let mut observed = 0u64;
        let blob = evidence::open_verified_blob_bounded(&fx.home, &found.data["content"],
            content["decodedBytes"].as_u64().unwrap(), &inv.control, |block| {
                assert!(block.iter().all(|byte| *byte == b'x'));
                observed += block.len() as u64;
                Ok(())
            }).unwrap();
        assert_eq!(observed, content["decodedBytes"]);
        drop(blob);
        if pass == 1 {
            assert_eq!(found.data["decodedCache"]["contentDecodeCalls"], 0);
            assert_eq!(found.data["decodedCache"]["hits"], 2);
        }
    }
    let output = fx.root.join("compressed-raw.bin");
    let inv = fx.invocation("asset export", &[("--file-id", "501"), ("--max-bytes", &maximum),
        ("--output", output.to_str().unwrap())]);
    let exported = lycheedev_data::dispatch(&inv).unwrap();
    assert_eq!(exported.data["content"]["sha256"], content["sha256"]);
    assert_eq!(exported.data["sourceCapture"]["blob"], exported.data["content"]);
    let mut raw = std::fs::File::open(&output).unwrap();
    let mut sha = Sha256::new();
    let mut buffer = [0; 65536];
    let mut bytes = 0u64;
    loop {
        let n = raw.read(&mut buffer).unwrap();
        if n == 0 { break; }
        sha.update(&buffer[..n]);
        bytes += n as u64;
    }
    assert_eq!(bytes, content["decodedBytes"]);
    assert_eq!(lycheedev_data::casc::hex(&sha.finalize()), content["sha256"]);
    let manifest = evidence::read_blob(&fx.home, &exported.provenance[1]["blob"], 1 << 20).unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&manifest).unwrap(), exported.data);
}

#[test]
fn compressed_multichunk_inspect_cold_warm_and_raw_export() {
    let fx = Fixture::compressed_at(stream_fixture_root(), 17 << 20);
    check_compressed_inspect_and_export(&fx);
}

fn stream_fixture_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/rust-content-streaming-fixtures")
        .join(format!("{}-{}", std::process::id(), std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
}

#[test]
fn streaming_rejects_changed_warm_source_and_bad_ekey_without_new_captures() {
    use lycheedev_core::metadata::Metadata;
    let fx = Fixture::compressed_at(stream_fixture_root(), 1 << 20);
    let inv = fx.invocation("asset inspect", &[("--file-id", "501"), ("--max-bytes", "1048576")]);
    lycheedev_data::dispatch(&inv).unwrap();
    let documents = || Metadata::open(&fx.home, false).unwrap()
        .list::<serde_json::Value>("", 100).unwrap();
    let before = documents();
    let archive = fx.root.join("game/Data/data/data.000");
    let original = std::fs::read(&archive).unwrap();
    // Framed EKey covers the header. Changed payload still has the same EKey,
    // but its chunk MD5 must fail even after a successful decoded-cache fill.
    let mut changed = original.clone();
    changed[30 + 36 + 8] ^= 1;
    std::fs::write(&archive, &changed).unwrap();
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code, "container.integrity_mismatch");
    assert_eq!(documents(), before);
    // A header digest change is rejected at the selected archive boundary.
    changed = original;
    changed[30 + 20] ^= 1;
    std::fs::write(&archive, &changed).unwrap();
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code, "archive.index_integrity");
    assert_eq!(documents(), before);
}

#[test]
fn streaming_whole_ckey_failure_publishes_no_selected_blob_cache_or_capture() {
    use lycheedev_core::{evidence, metadata::Metadata};
    let fx = Fixture::compressed_with_wrong_ckey_at(stream_fixture_root(), 1 << 20);
    let identity: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fx.root.join("input-identity.json")).unwrap(),
    ).unwrap();
    let output = fx.root.join("failed-raw.bin");
    let inv = fx.invocation("asset export", &[("--file-id", "501"), ("--max-bytes", "1048576"),
        ("--output", output.to_str().unwrap())]);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code, "records.content_integrity");
    assert!(!output.exists());
    let metadata = Metadata::open(&fx.home, false).unwrap();
    assert!(metadata.list::<serde_json::Value>("capture/", 100).unwrap().is_empty());
    let decoded = metadata.list::<serde_json::Value>("decoded-v1/", 100).unwrap();
    assert!(decoded.iter().all(|entry| entry["contentKey"] != identity["content"]["contentKey"]));
    assert!(!evidence::blob_path(&fx.home, identity["content"]["sha256"].as_str().unwrap()).unwrap().exists());
}

#[test]
fn streamed_raw_export_conflict_overwrite_and_cancel_preserve_existing_output() {
    let fx = Fixture::compressed_at(stream_fixture_root(), 1 << 20);
    let output = fx.root.join("overwrite.bin");
    let alias = fx.root.join("prior-hardlink.bin");
    std::fs::write(&output, b"previous output").unwrap();
    std::fs::hard_link(&output, &alias).unwrap();
    let mut inv = fx.invocation("asset export", &[("--file-id", "501"), ("--max-bytes", "1048576"),
        ("--output", output.to_str().unwrap())]);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().code, "records.export_conflict");
    assert_eq!(std::fs::read(&output).unwrap(), b"previous output");
    inv.flags.insert("--overwrite".into(), "true".into());
    let found = lycheedev_data::dispatch(&inv).unwrap();
    assert_eq!(std::fs::read(&alias).unwrap(), b"previous output");
    let raw = std::fs::read(&output).unwrap();
    assert_eq!(raw.len(), 1 << 20);
    assert!(raw.iter().all(|byte| *byte == b'x'));
    assert_eq!(found.data["content"]["sha256"], lycheedev_core::storage::sha256(&raw));
    inv.control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(lycheedev_data::dispatch(&inv).unwrap_err().exit_code, 7);
    assert_eq!(std::fs::read(&output).unwrap(), raw);
}

#[test]
#[ignore = "run inspect/raw against an explicitly prepared real large-content CASC fixture"]
fn prepared_compressed_content_inspect_and_raw_export() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LYCHEEDEV_CONTENT_BUDGET_FIXTURE").expect("fixture directory"),
    );
    let identity: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("input-identity.json")).unwrap(),
    ).unwrap();
    let fx = Fixture { home: identity["home"].as_str().unwrap().into(),
        client: identity["installation"].as_str().unwrap().into(),
        pin: identity["snapshot"].as_str().unwrap().into(), root };
    check_compressed_inspect_and_export(&fx);
}
