//! Actual default dispatch over offline synthetic CDN bodies. Only priming uses
//! the fixture backend; command dispatch receives no alternate transport.
use super::*;
use lycheedev_core::{
    Invocation, Outcome, evidence,
    metadata::Metadata,
    selection::{self, SelectionSpec},
};
use serde_json::{Value, json};
struct AssetFixture {
    source: Fixture,
    snapshot: String,
    output: PathBuf,
}
impl AssetFixture {
    fn new(payload: Vec<u8>) -> Self {
        let source = Fixture::with_payload(false, None, payload);
        let snapshot = selection::pin_selection(
            &source.home,
            SelectionSpec {
                data: Some(source.pin.clone()),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        let output = source.home.parent().unwrap().join(format!(
            "{}-asset-output",
            source.home.file_name().unwrap().to_str().unwrap()
        ));
        std::fs::create_dir(&output).unwrap();
        let mut admission = source.admission();
        let mut session = Session::borrowed_with(
            &mut admission,
            &source.pin,
            source.options(),
            &source.budget,
            &source.backend,
            &source.backend,
        )
        .unwrap();
        session
            .read_batches(&[7], &mut |files| {
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].bytes(), source.payload);
                Ok(Vec::new())
            })
            .unwrap();
        session.close().unwrap();
        admission.finish(Ok(())).unwrap();
        source.backend.calls.lock().unwrap().clear();
        Self {
            source,
            snapshot,
            output,
        }
    }
    fn invocation(&self, name: &str) -> Invocation {
        Invocation {
            command: "asset export".into(),
            positional: None,
            flags: BTreeMap::from([
                ("--cdn".into(), "true".into()),
                ("--offline".into(), "true".into()),
                ("--snapshot".into(), self.snapshot.clone()),
                ("--file-id".into(), "7".into()),
                (
                    "--output".into(),
                    self.output.join(name).to_str().unwrap().into(),
                ),
            ]),
            home: self.source.home.clone(),
            cwd: self.source.home.clone(),
            control: OperationControl::default(),
        }
    }
    fn captures(&self) -> Vec<Value> {
        let metadata = Metadata::open(&self.source.home, false).unwrap();
        let values = metadata.list::<Value>("capture/", 100).unwrap();
        metadata.close().unwrap();
        values
    }
    fn verify(&self, result: &Outcome, name: &str) {
        assert_eq!(result.data["source"]["source"], "cdn");
        assert_eq!(result.data["source"]["contentVerified"], true);
        assert_eq!(result.selection["snapshot"], self.snapshot);
        let bytes = std::fs::read(self.output.join(name)).unwrap();
        assert_eq!(storage::sha256(&bytes), result.data["content"]["sha256"]);
        assert_eq!(
            bytes.len() as u64,
            result.data["content"]["bytes"].as_u64().unwrap()
        );
        for capture in &result.provenance {
            assert_eq!(capture["provenance"]["snapshot"], self.snapshot);
            assert_eq!(
                evidence::read_capture(&self.source.home, capture["id"].as_str().unwrap()).unwrap(),
                *capture
            );
            let raw = evidence::read_blob(&self.source.home, &capture["blob"], 2 << 20).unwrap();
            assert_eq!(storage::sha256(&raw), capture["blob"]["sha256"]);
        }
        let raw =
            evidence::read_blob(&self.source.home, &result.provenance[1]["blob"], 1 << 20).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), result.data);
        assert!(self.source.backend.calls.lock().unwrap().is_empty());
    }
    fn close(self) {
        std::fs::remove_dir_all(self.source.home).unwrap();
        std::fs::remove_dir_all(self.output).unwrap();
    }
}
fn blp() -> Vec<u8> {
    let mut bytes = vec![0; 148];
    bytes[..4].copy_from_slice(b"BLP2");
    bytes[4] = 1;
    bytes[8..12].copy_from_slice(&[3, 8, 0, 0x11]);
    bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&148u32.to_le_bytes());
    bytes[84..88].copy_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&[3, 5, 7, 255, 11, 13, 17, 128]);
    bytes
}
#[test]
fn default_cdn_raw_export_cold_then_warm_streams_authenticated_cache_and_dependencies() {
    let f = AssetFixture::new(vec![b'x'; 3 * 65536]);
    // Keep encoded fragments, remove only selected decoded body: default offline
    // dispatch must perform real decode and Core staged digest publication.
    std::fs::remove_file(
        evidence::blob_path(&f.source.home, &storage::sha256(&f.source.payload)).unwrap(),
    )
    .unwrap();
    let cold = crate::dispatch(&f.invocation("cold.bin")).unwrap();
    f.verify(&cold, "cold.bin");
    assert_eq!(
        std::fs::read(f.output.join("cold.bin")).unwrap(),
        f.source.payload
    );
    assert_eq!(cold.provenance.len(), 2);
    assert_eq!(
        cold.provenance[1]["dependencies"],
        json!([cold.provenance[0]["id"]])
    );
    assert_eq!(cold.data["source"]["decodedCache"]["contentDecodeCalls"], 1);
    let warm = crate::dispatch(&f.invocation("warm.bin")).unwrap();
    f.verify(&warm, "warm.bin");
    assert_eq!(warm.data["source"]["decodedCache"]["contentDecodeCalls"], 0);
    assert_eq!(warm.data["source"]["decodedCache"]["hits"], 2);
    assert_eq!(
        std::fs::read(f.output.join("warm.bin")).unwrap(),
        f.source.payload
    );
    f.close();
}
#[test]
fn default_cdn_png_and_webp_export_keep_ordered_source_artifact_manifest_dependencies() {
    let f = AssetFixture::new(blp());
    for encoding in ["png", "webp"] {
        let mut inv = f.invocation(encoding);
        inv.flags.insert("--encoding".into(), encoding.into());
        let result = crate::dispatch(&inv).unwrap();
        f.verify(&result, encoding);
        assert_eq!(result.provenance.len(), 3);
        let source = &result.provenance[0];
        let manifest = &result.provenance[1];
        let artifact = &result.provenance[2];
        assert_eq!(artifact["dependencies"], json!([source["id"]]));
        assert_eq!(
            manifest["dependencies"],
            json!([source["id"], artifact["id"]])
        );
        assert_eq!(result.data["artifactCapture"], *artifact);
        assert_eq!(result.data["sourceCapture"], *source);
        assert_eq!(
            result.data["image"]["pixelsSHA256"],
            storage::sha256(&[7, 5, 3, 255, 17, 13, 11, 128])
        );
        let bytes = std::fs::read(f.output.join(encoding)).unwrap();
        let (width, height, pixels) = if encoding == "png" {
            let mut decoder = png::Decoder::new(std::io::Cursor::new(&bytes))
                .read_info()
                .unwrap();
            let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
            let info = decoder.next_frame(&mut pixels).unwrap();
            pixels.truncate(info.buffer_size());
            assert_eq!(info.color_type, png::ColorType::Rgba);
            (info.width, info.height, pixels)
        } else {
            let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(&bytes)).unwrap();
            assert!(!decoder.is_lossy());
            assert!(decoder.has_alpha());
            let (width, height) = decoder.dimensions();
            let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
            decoder.read_image(&mut pixels).unwrap();
            (width, height, pixels)
        };
        assert_eq!((width, height), (2, 1));
        assert_eq!(pixels, [7, 5, 3, 255, 17, 13, 11, 128]);
    }
    f.close();
}
#[test]
fn default_cdn_export_conflict_precedes_snapshot_lookup_and_overwrite_preserves_old_hardlink() {
    let f = AssetFixture::new(b"new authenticated body".to_vec());
    let output = f.output.join("body");
    std::fs::write(&output, b"old bytes").unwrap();
    std::fs::hard_link(&output, f.output.join("old-hardlink")).unwrap();
    let mut inv = f.invocation("body");
    inv.flags
        .insert("--snapshot".into(), "invalid-snapshot".into());
    assert_eq!(
        crate::dispatch(&inv).unwrap_err().code,
        "records.export_conflict"
    );
    assert!(f.captures().is_empty());
    inv.flags.insert("--snapshot".into(), f.snapshot.clone());
    inv.flags.insert("--overwrite".into(), "true".into());
    let result = crate::dispatch(&inv).unwrap();
    f.verify(&result, "body");
    assert_eq!(std::fs::read(output).unwrap(), f.source.payload);
    assert_eq!(
        std::fs::read(f.output.join("old-hardlink")).unwrap(),
        b"old bytes"
    );
    f.close();
}
#[test]
fn default_cdn_export_corrupt_selected_cache_never_publishes_output_or_capture() {
    let f = AssetFixture::new(b"authenticated cache bytes".to_vec());
    let path = evidence::blob_path(&f.source.home, &storage::sha256(&f.source.payload)).unwrap();
    std::fs::write(path, b"corrupt cache bytes").unwrap();
    let error = crate::dispatch(&f.invocation("body")).unwrap_err();
    assert_eq!(error.code, "vault.blob_integrity");
    assert!(!f.output.join("body").exists());
    assert!(f.captures().is_empty());
    f.close();
}
#[test]
fn default_cdn_export_image_failure_keeps_one_committed_source_identity_without_output() {
    let f = AssetFixture::new(b"not a BLP image".to_vec());
    let mut inv = f.invocation("image");
    inv.flags.insert("--encoding".into(), "png".into());
    let error = crate::dispatch(&inv).unwrap_err();
    assert_eq!(error.code, "texture.unsupported_format");
    let captures = f.captures();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0]["provenance"]["kind"], "asset");
    assert!(error.message.contains(captures[0]["id"].as_str().unwrap()));
    assert!(error.message.contains("already committed"));
    assert!(!error.message.contains("output already published"));
    assert!(!f.output.join("image").exists());
    f.close();
}
#[test]
fn default_cdn_export_original_cancel_before_source_has_no_capture_or_output() {
    let f = AssetFixture::new(b"cancelled body".to_vec());
    let inv = f.invocation("body");
    inv.control.tracked_cancel_handle().unwrap().cancel();
    let expected = inv.control.check().unwrap_err();
    let actual = crate::dispatch(&inv).unwrap_err();
    assert_eq!(actual.code, expected.code);
    assert_eq!(actual.exit_code, expected.exit_code);
    assert!(f.captures().is_empty());
    assert!(!f.output.join("body").exists());
    f.close();
}

#[test]
fn actual_remote_captured_consumer_cancel_is_terminal_and_releases_admission() {
    let f = AssetFixture::new(b"cancel after committed capture".to_vec());
    let inv = f.invocation("unused");
    let mut calls = 0;
    let mut id = String::new();
    let error = crate::assets::input::with_casc_verified(&inv, 7, &mut |source| {
        calls += 1;
        id = source.capture["id"].as_str().unwrap().to_owned();
        let mut bytes = [0; 2];
        assert_eq!(source.blob.read_checked(&mut bytes)?, 2);
        inv.control.tracked_cancel_handle().unwrap().cancel();
        let read = source.blob.read_checked(&mut bytes).unwrap_err();
        assert_eq!(read.code, "command.cancelled");
        Err::<(), _>(read)
    })
    .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        inv.control.stop_cause().unwrap(),
        Some(lycheedev_core::control::StopCause::Cancelled)
    );
    let captures = f.captures();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0]["id"], id);
    assert!(!f.output.join("unused").exists());
    // A fresh bounded scope can acquire all measurement slots only after the
    // stopped consumer's ordinary owner has really released its resources.
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(10));
    let probe =
        AdmittedWorkspace::acquire(&f.source.home, Policy::Measurement, &control, None).unwrap();
    probe.finish(Ok(())).unwrap();
    f.close();
}

mod scoped_demux;
