//! Actual default local assets reuse the admitted query CASC owner. Synthetic
//! files only; no network, game or installed third-party input.
use super::*;
#[path = "catalog_fixture.rs"]
mod catalog;
#[test]
fn local_raw_export_uses_admitted_verified_stream_and_keeps_ordered_control() {
    let table = catalog::table("Asset", 201, &[("ID", "int")], &[vec![catalog::Cell::U(1)]]);
    let expected = table.raw.clone();
    let f = catalog::Fixture::new(&[table]);
    let output = f.root.join("body.bin");
    let inv = f.invocation(
        "asset export",
        &[("--file-id", "201"), ("--output", output.to_str().unwrap())],
    );
    let outcome = lycheedev_data::dispatch(&inv).unwrap();
    assert_eq!(fs::read(&output).unwrap(), expected);
    assert_eq!(outcome.data["source"]["source"], "installation");
    assert_eq!(outcome.data["source"]["contentVerified"], true);
    assert_eq!(outcome.provenance.len(), 2);
    for capture in &outcome.provenance {
        assert_eq!(
            evidence::read_capture(&f.home, capture["id"].as_str().unwrap()).unwrap(),
            *capture
        );
    }
    assert_eq!(inv.control.stop_cause().unwrap(), None);
    inv.control
        .ordered_child(inv.control.deadline())
        .unwrap()
        .check()
        .unwrap();
    fs::remove_dir_all(f.root).unwrap();
}
#[test]
fn local_casc_demux_reuses_same_query_and_records_only_source_identity_dependency() {
    let mut table = catalog::table("Movie", 201, &[("ID", "int")], &[vec![catalog::Cell::U(1)]]);
    table.raw = avi();
    let f = catalog::Fixture::new(&[table]);
    let output = f.root.join("frames");
    fs::create_dir(&output).unwrap();
    let inv = f.invocation(
        "asset demux",
        &[("--file-id", "201"), ("--output", output.to_str().unwrap())],
    );
    let result = lycheedev_data::dispatch(&inv).unwrap();
    assert_eq!(result.data["source"]["kind"], "casc");
    assert_eq!(result.data["source"]["snapshot"], f.pin);
    assert_eq!(result.data["frames"].as_array().unwrap().len(), 2);
    assert_eq!(result.provenance.len(), 4);
    assert_eq!(
        result.provenance[1]["dependencies"],
        json!([result.provenance[0]["id"]])
    );
    let manifest = fs::read(output.join("demux-manifest.json")).unwrap();
    assert_eq!(
        manifest,
        evidence::read_blob(&f.home, &result.provenance[1]["blob"], 16 << 20).unwrap()
    );
    assert_eq!(fs::read(output.join("frame-000002.00db")).unwrap(), [4, 5]);
    assert!(
        manifest.starts_with(b"{\n  \"schema\": \"lycheedev.asset-demux.v1\",\n  \"snapshot\":")
    );
    assert!(!manifest.ends_with(b"\n"));
    let pin_start = String::from_utf8(manifest).unwrap();
    let pin = &pin_start[pin_start.find("\"pin\": {").unwrap()..];
    assert!(pin.find("\"fullBuild\"").unwrap() < pin.find("\"language\"").unwrap());
    assert_eq!(inv.control.stop_cause().unwrap(), None);
    fs::remove_dir_all(f.root).unwrap();
}
#[test]
fn demux_path_manifest_has_go_typed_float_bytes_and_required_frames_block_removal() {
    let f = Fixture::new();
    let input = f.root.join("movie.avi");
    fs::write(&input, avi()).unwrap();
    let output = f.root.join("frames");
    fs::create_dir(&output).unwrap();
    let inv = f.invocation(
        "asset demux",
        &[
            ("--path", input.to_str().unwrap()),
            ("--output", output.to_str().unwrap()),
        ],
    );
    let result = lycheedev_data::dispatch(&inv).unwrap();
    let raw = fs::read(output.join("demux-manifest.json")).unwrap();
    let text = String::from_utf8(raw).unwrap();
    assert!(text.contains("\"frameRate\": 25,"));
    assert!(text.contains("\"timestamp\": 40000,"));
    assert!(!text.contains("40000.0"));
    assert_eq!(result.data["limits"]["maxInputBytes"], 128 << 20);
    assert_eq!(
        result.provenance[1]["dependencies"],
        json!([result.provenance[0]["id"]])
    );
    let mut remove = f.invocation("evidence remove", &[]);
    remove.positional = Some(result.provenance[2]["id"].as_str().unwrap().into());
    assert_eq!(
        evidence::dispatch(&remove).unwrap_err().code,
        "evidence.capture_referenced"
    );
    assert_eq!(
        evidence::read_capture(&f.home, result.provenance[2]["id"].as_str().unwrap()).unwrap(),
        result.provenance[2]
    );
}

// encoding/json emits integral binary64 without a .0 suffix. Restore only the
// three declared float positions for an internal Value comparison; raw artifact
// bytes are independently checked by the fixed-order wire fixture.
pub(super) fn assert_manifest_value(raw: &[u8], expected: &Value) {
    let mut parsed: Value = serde_json::from_slice(raw).unwrap();
    parsed["frameRate"] = json!(parsed["frameRate"].as_f64().unwrap());
    for frame in parsed["frames"].as_array_mut().unwrap() {
        for key in ["timestamp", "duration"] {
            frame[key] = json!(frame[key].as_f64().unwrap());
        }
    }
    assert_eq!(&parsed, expected);
}
