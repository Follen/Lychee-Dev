use super::*;
use std::collections::BTreeMap;

fn invocation() -> Invocation {
    Invocation {
        command: "asset export".into(),
        positional: None,
        flags: BTreeMap::from([
            ("--snapshot".into(), "PIN-invalid-but-present".into()),
            ("--cdn".into(), "true".into()),
            ("--file-id".into(), "7".into()),
            ("--output".into(), "unused-output".into()),
        ]),
        home: "unopened-asset-workspace".into(),
        cwd: "unopened-asset-workspace".into(),
        control: Default::default(),
    }
}
#[test]
fn default_asset_arguments_reject_missing_snapshot_zero_id_and_source_before_workspace() {
    for (flag, value) in [("--snapshot", ""), ("--file-id", "0"), ("--cdn", "false")] {
        let mut inv = invocation();
        inv.flags.insert(flag.into(), value.into());
        let error = run(&inv).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert!(error.message.contains("requires --snapshot"), "{error:?}");
    }
    let mut inv = invocation();
    inv.flags.insert("--file-id".into(), "4294967296".into());
    assert_eq!(run(&inv).unwrap_err().message, "invalid --file-id");
    inv.flags.insert("--file-id".into(), "7".into());
    inv.flags.insert("--installation".into(), "source".into());
    assert!(run(&inv).unwrap_err().message.contains("choose either"));
}
#[test]
fn default_asset_image_options_and_original_stop_precede_destination_open() {
    let mut inv = invocation();
    inv.flags.insert("--channels".into(), "r".into());
    assert_eq!(run(&inv).unwrap_err().code, "records.export_options");
    inv.control.tracked_cancel_handle().unwrap().cancel();
    let stopped = inv.control.check().unwrap_err();
    let actual = run(&inv).unwrap_err();
    assert_eq!(actual.code, stopped.code);
    assert_eq!(actual.exit_code, stopped.exit_code);
    assert_eq!(actual.details, stopped.details);
}
#[test]
fn committed_capture_context_preserves_all_typed_cleanup_and_does_not_replay_work() {
    let mut progress = Progress::default();
    let ids: Vec<_> = ['a', 'b', 'c']
        .into_iter()
        .map(|ch| format!("CAP-{}", ch.to_string().repeat(64)))
        .collect();
    for id in &ids {
        progress.committed(&json!({"id":id}));
    }
    progress.output_published = true;
    let mut primary = ToolkitError::new("command.cancelled", "cancelled after publish", 7);
    primary.details = json!({"schema":"lycheedev.admitted-workspace-errors.v1", "causes":[
        {"code":"command.cancelled","message":"cancelled after publish","exit_code":7},
        {"code":"command.io","message":"same cleanup","exit_code":5},
        {"code":"command.io","message":"same cleanup","exit_code":5}
    ]});
    let expected = primary.clone();
    let actual = progress.finish::<()>(Err(primary)).unwrap_err();
    assert_eq!(actual.code, expected.code);
    assert_eq!(actual.exit_code, expected.exit_code);
    assert_eq!(
        actual.details["schema"],
        "lycheedev.asset-committed-error.v1"
    );
    assert_eq!(
        actual.details["primary"],
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(actual.details["completedCaptures"], json!(ids));
    assert_eq!(actual.details["outputPublished"], true);
    assert!(actual.message.starts_with(&expected.message));
    for id in &ids {
        assert_eq!(actual.message.matches(id).count(), 1);
    }
    assert!(actual.message.contains("asset output already published"));
    assert_eq!(progress.count, 3);
    assert_eq!(progress.finish(Ok(19)).unwrap(), 19);
}

#[allow(dead_code)]
#[path = "../../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;

#[test]
fn actual_profiled_image_export_covers_consumer_phases_success_and_codec_failure() {
    use crate::resource::{Budget, Cost};
    use catalog_fixture::{Cell::U, Fixture, table};
    use lycheedev_core::storage;
    use std::{fs, io::Cursor, sync::Arc};

    for encoding in ["png", "webp"] {
        for codec_failure in [false, true] {
            let mut item = table("Image", 201, &[("ID", "uint")], &[vec![U(7)]]);
            let mut raw = vec![0; 148];
            raw[..4].copy_from_slice(b"BLP2");
            raw[4] = 1;
            raw[8..12].copy_from_slice(&[3, 8, 0, 0x11]);
            raw[12..16].copy_from_slice(&2u32.to_le_bytes());
            raw[16..20].copy_from_slice(&1u32.to_le_bytes());
            raw[20..24].copy_from_slice(&148u32.to_le_bytes());
            raw[84..88].copy_from_slice(&8u32.to_le_bytes());
            raw.extend_from_slice(&[3, 2, 1, 255, 6, 5, 4, 128]);
            item.raw = raw;
            let fx = Fixture::new(&[item]);
            let path = fx.root.join(format!("profile.{encoding}"));
            let inv = fx.invocation(
                "asset export",
                &[
                    ("--file-id", "201"),
                    ("--encoding", encoding),
                    ("--output", path.to_str().unwrap()),
                ],
            );
            let (budget, profile) = Budget::controlled(
                Cost::defaults(),
                inv.control.deadline(),
                inv.control.clone(),
            )
            .with_test_phase_profile();
            let budget = Arc::new(budget);
            let destination = output::destination_scoped(&inv, &path, false).unwrap();
            let options = Options {
                encoding,
                mipmap: 0,
                channels: "rgba".into(),
                pixels: 16,
                maximum: if codec_failure { 1 } else { 1 << 20 },
            };
            let mut progress = Progress::default();
            let result = input::with_casc_verified_budgeted(&inv, 201, &budget, &mut |source| {
                progress.committed(source.capture);
                publish(&inv, &destination, source, &options, &mut progress)
            });
            let result = output::finish_closes(result, destination.close(), &inv.control);
            let result = progress.finish(result);
            if codec_failure {
                let error = result.unwrap_err();
                assert_eq!(error.code, "records.image_output_limit");
                assert_eq!(
                    error.details["completedCaptures"].as_array().unwrap().len(),
                    1
                );
                assert_eq!(error.details["outputPublished"], false);
                assert!(!path.exists());
            } else {
                let outcome = result.unwrap();
                assert_eq!(outcome.provenance.len(), 3);
                let encoded = fs::read(&path).unwrap();
                assert_eq!(outcome.data["content"]["sha256"], storage::sha256(&encoded));
                assert!(outcome.data.get("phases").is_none());
                let pixels = if encoding == "png" {
                    let mut decoder = png::Decoder::new(Cursor::new(&encoded))
                        .read_info()
                        .unwrap();
                    let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
                    let frame = decoder.next_frame(&mut pixels).unwrap();
                    assert_eq!((frame.width, frame.height), (2, 1));
                    pixels.truncate(frame.buffer_size());
                    pixels
                } else {
                    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(&encoded)).unwrap();
                    assert!(!decoder.is_lossy());
                    assert_eq!(decoder.dimensions(), (2, 1));
                    let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
                    decoder.read_image(&mut pixels).unwrap();
                    pixels
                };
                assert_eq!(pixels, [1, 2, 3, 255, 4, 5, 6, 128]);
                assert_eq!(
                    outcome.data["image"]["pixelsSHA256"],
                    storage::sha256(&pixels)
                );
                for saved in outcome.provenance {
                    assert_eq!(
                        evidence::read_capture(&fx.home, saved["id"].as_str().unwrap()).unwrap(),
                        saved
                    );
                }
            }
            assert!(profile.lock().unwrap().is_empty());
            drop(budget);
            let observations = profile.lock().unwrap();
            assert_eq!(observations.len(), 1);
            let phases = observations[0]["phases"].as_array().unwrap();
            for name in [
                "admission",
                "prepare_inclusive",
                "payload_inclusive",
                "texture_decode",
                "pixel_transform_and_hash",
                "encode",
                "source_capture_metadata_inclusive",
            ] {
                assert_eq!(
                    phases.iter().find(|row| row["phase"] == name).unwrap()["calls"],
                    1,
                    "{name}"
                );
            }
            for name in [
                "artifact_capture_metadata_inclusive",
                "manifest_capture_metadata_inclusive",
                "output",
            ] {
                assert_eq!(
                    phases.iter().find(|row| row["phase"] == name).unwrap()["calls"],
                    if codec_failure { 0 } else { 1 },
                    "{name}"
                );
            }
            assert_eq!(
                phases
                    .iter()
                    .find(|row| row["phase"] == "source_close")
                    .unwrap()["calls"],
                3
            );
            drop(observations);
            fs::remove_dir_all(fx.root).unwrap();
        }
    }
}
