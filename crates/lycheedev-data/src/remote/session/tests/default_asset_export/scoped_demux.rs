use super::*;
fn record(name: &[u8; 4], raw: &[u8]) -> Vec<u8> {
    let mut b = name.to_vec();
    b.extend((raw.len() as u32).to_le_bytes());
    b.extend(raw);
    if raw.len() % 2 != 0 {
        b.push(0)
    }
    b
}
fn avi() -> Vec<u8> {
    let mut movi = b"movi".to_vec();
    movi.extend(record(b"00dc", b"first"));
    movi.extend(record(b"00db", b"second"));
    let mut body = b"AVI ".to_vec();
    body.extend(record(b"LIST", &movi));
    record(b"RIFF", &body)
}
#[test]
fn default_cdn_demux_uses_offline_authenticated_session_through_manifest_publication() {
    let f = AssetFixture::new(avi());
    let mut inv = f.invocation("unused");
    inv.command = "asset demux".into();
    inv.flags
        .insert("--output".into(), f.output.to_str().unwrap().into());
    let outcome = crate::dispatch(&inv).unwrap();
    assert_eq!(outcome.data["source"]["kind"], "casc");
    assert_eq!(outcome.data["source"]["fileDataID"], 7);
    assert_eq!(outcome.provenance.len(), 4);
    assert_eq!(
        std::fs::read(f.output.join("frame-000001.00dc")).unwrap(),
        b"first"
    );
    let manifest = std::fs::read(f.output.join("demux-manifest.json")).unwrap();
    let mut parsed: Value = serde_json::from_slice(&manifest).unwrap();
    parsed["frameRate"] = json!(parsed["frameRate"].as_f64().unwrap());
    for frame in parsed["frames"].as_array_mut().unwrap() {
        for key in ["timestamp", "duration"] {
            frame[key] = json!(frame[key].as_f64().unwrap());
        }
    }
    assert_eq!(parsed, outcome.data);
    assert_eq!(
        outcome.provenance[1]["dependencies"],
        json!([outcome.provenance[0]["id"]])
    );
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    assert_eq!(inv.control.stop_cause().unwrap(), None);
    f.close();
}
#[test]
fn default_cdn_demux_existing_manifest_precedes_all_source_and_capture_work() {
    let f = AssetFixture::new(avi());
    std::fs::write(f.output.join("demux-manifest.json"), b"existing").unwrap();
    let mut inv = f.invocation("unused");
    inv.command = "asset demux".into();
    inv.flags
        .insert("--output".into(), f.output.to_str().unwrap().into());
    let error = crate::dispatch(&inv).unwrap_err();
    assert_eq!(error.code, "records.export_conflict");
    assert!(f.captures().is_empty());
    assert!(f.source.backend.calls.lock().unwrap().is_empty());
    assert!(!f.output.join("frame-000001.00dc").exists());
    f.close();
}
