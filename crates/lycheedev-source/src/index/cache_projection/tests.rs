use super::*;
use crate::index::{self, facts_cache_tests::Fixture};
use lycheedev_core::{Invocation, Outcome, evidence, selection, storage};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf};
fn pin() -> SourcePin {
    persistence_wire::tests::fixture().1
}
fn manifest_key(pin: &SourcePin) -> String {
    format!("source/rust-index/{}", index::fingerprint(pin))
}
fn read_manifest(home: &Path, pin: &SourcePin) -> Value {
    crate::persistence::read(home, &manifest_key(pin), &OperationControl::default()).unwrap()
}
fn publish(home: &Path, pin: &SourcePin, bytes: &[u8]) -> (Value, PathBuf) {
    let control = OperationControl::default();
    let blob = lycheedev_core::cache::publish_derived_controlled(home, bytes, 128 << 20, &control)
        .unwrap();
    let digest = blob["sha256"].as_str().unwrap();
    let path = home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    let manifest =
        json!({"schema":persistence_wire::SCHEMA,"identity":index::fingerprint(pin),"blob":blob});
    crate::persistence::put_manifest(home, &manifest_key(pin), &manifest, &control).unwrap();
    (manifest, path)
}
fn error_value<T>(result: lycheedev_core::Result<T>) -> Value {
    serde_json::to_value(result.err().expect("expected error")).unwrap()
}
fn comparable_error(home: &Path, pin: &SourcePin, control: &OperationControl) -> Value {
    let full = error_value(index::build_with_control(home, pin, control));
    let summary = error_value(build_summary_with_control(home, pin, control));
    assert_eq!(summary, full);
    summary
}
fn no_elapsed(mut value: Value) -> Value {
    if let Some(io) = value.get_mut("io").and_then(Value::as_object_mut) {
        assert!(io.remove("elapsedMs").unwrap().is_u64());
    }
    value
}
fn assert_closed(path: &Path) {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let file = options.open(path).unwrap();
    storage::finish_file(file, Ok(()), &OperationControl::default()).unwrap();
}
#[test]
fn binary_warm_summary_authenticates_all_rows_and_matches_complete_owned_output() {
    let fixture = Fixture::new();
    let (index, pin) = persistence_wire::tests::fixture();
    let raw = persistence_wire::encode(
        &index,
        &index::fingerprint(&pin),
        &OperationControl::default(),
    )
    .unwrap();
    let (manifest, path) = publish(&fixture.home, &pin, &raw);
    fs::rename(
        crate::git::mirror(&fixture.home, &pin.repository),
        fixture.home.join("parked.git"),
    )
    .unwrap();
    let full =
        index::build_with_control(&fixture.home, &pin, &OperationControl::default()).unwrap();
    let (summary, coverage) =
        build_summary_with_control(&fixture.home, &pin, &OperationControl::default()).unwrap();
    assert_eq!(
        no_elapsed(summary.clone()),
        no_elapsed(serde_json::to_value(&full.summary).unwrap())
    );
    assert_eq!(no_elapsed(coverage), no_elapsed(full.coverage()));
    assert_eq!(summary["io"]["indexBytesRead"], raw.len());
    assert_eq!(summary["io"]["cachedFacts"], 2);
    assert_eq!(summary["io"]["cachedIndex"], true);
    assert_eq!(summary["io"]["sourceBytesRead"], 0);
    assert_eq!(summary["io"]["gitBlobProcesses"], 0);
    assert_eq!(read_manifest(&fixture.home, &pin), manifest);
    assert_eq!(fs::read(&path).unwrap(), raw);
    assert_closed(&path);
}
#[test]
fn binary_warm_summary_corruption_bad_length_digest_and_new_payload_never_fall_back() {
    let fixture = Fixture::new();
    let (index, pin) = persistence_wire::tests::fixture();
    let raw = persistence_wire::encode(
        &index,
        &index::fingerprint(&pin),
        &OperationControl::default(),
    )
    .unwrap();
    let (manifest, path) = publish(&fixture.home, &pin, &raw);
    let mut corrupt = raw.clone();
    corrupt[0] ^= 1;
    fs::write(&path, corrupt).unwrap();
    assert_eq!(
        comparable_error(&fixture.home, &pin, &OperationControl::default())["message"],
        "Cached index content differs"
    );
    assert_closed(&path);
    fs::write(&path, &raw).unwrap();
    for bad in [json!("invalid"), json!(null), json!("A".repeat(64))] {
        let mut changed = manifest.clone();
        changed["blob"]["sha256"] = bad;
        crate::persistence::put_manifest(
            &fixture.home,
            &manifest_key(&pin),
            &changed,
            &OperationControl::default(),
        )
        .unwrap();
        assert_eq!(
            comparable_error(&fixture.home, &pin, &OperationControl::default())["message"],
            "Invalid cached index digest"
        );
    }
    let mut wrong = manifest;
    wrong["blob"]["bytes"] = json!(raw.len() + 1);
    crate::persistence::put_manifest(
        &fixture.home,
        &manifest_key(&pin),
        &wrong,
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(
        comparable_error(&fixture.home, &pin, &OperationControl::default())["message"],
        "Cached index content differs"
    );
    for bytes in [b"{}".to_vec(), {
        let mut invalid = index;
        invalid.summary.documents += 1;
        persistence_wire::tests::unchecked(&invalid, &pin)
    }] {
        let (manifest, path) = publish(&fixture.home, &pin, &bytes);
        assert_eq!(
            comparable_error(&fixture.home, &pin, &OperationControl::default())["code"],
            "codebase.source_integrity"
        );
        assert_eq!(read_manifest(&fixture.home, &pin), manifest);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_closed(&path);
    }
}
#[test]
fn binary_warm_summary_timely_cancel_wins_over_unconsumed_bad_tail_and_checked_close_is_retained() {
    let fixture = Fixture::new();
    let (index, pin) = persistence_wire::tests::fixture();
    let mut raw = persistence_wire::encode(
        &index,
        &index::fingerprint(&pin),
        &OperationControl::default(),
    )
    .unwrap();
    raw.push(0xff);
    let bytes = (raw.len() - 44) as u32;
    raw[8..12].copy_from_slice(&bytes.to_le_bytes());
    let (manifest, path) = publish(&fixture.home, &pin, &raw);
    for summary in [false, true] {
        let control = OperationControl::default();
        let _stop = persistence_wire::CancelAtStep::new(12, &control);
        let error = if summary {
            error_value(build_summary_with_control(&fixture.home, &pin, &control))
        } else {
            error_value(index::build_with_control(&fixture.home, &pin, &control))
        };
        assert_eq!(error["code"], "command.cancelled");
        assert!(error["details"].is_null());
        assert_closed(&path);
    }
    assert_eq!(read_manifest(&fixture.home, &pin), manifest);
    assert_eq!(fs::read(&path).unwrap(), raw);
}
#[test]
fn binary_warm_summary_old_v6_is_a_miss_even_with_unmigrated_dynamic_values() {
    let fixture = Fixture::new();
    let pin = fixture.pin(&BTreeMap::from([(
        "Main.lua".into(),
        "function NewIndex() end".into(),
    )]));
    let old = br#"{"assets":[null,1,true],"summary":{"coverage":null}}"#;
    let (mut manifest, path) = publish(&fixture.home, &pin, old);
    manifest["schema"] = json!("lycheedev.source-index.rust.v6");
    crate::persistence::put_manifest(
        &fixture.home,
        &manifest_key(&pin),
        &manifest,
        &OperationControl::default(),
    )
    .unwrap();
    let (summary, _) =
        build_summary_with_control(&fixture.home, &pin, &OperationControl::default()).unwrap();
    assert_eq!(summary["documents"], 1);
    assert!(summary["io"].get("cachedIndex").is_none());
    assert_eq!(fs::read(path).unwrap(), old);
    assert_eq!(
        read_manifest(&fixture.home, &pin)["schema"],
        persistence_wire::SCHEMA
    );
}

fn invoke(home: &Path, snapshot: &str, command: &str) -> Invocation {
    Invocation {
        home: home.into(),
        cwd: home.into(),
        command: command.into(),
        positional: None,
        flags: BTreeMap::from([("--snapshot".into(), snapshot.into())]),
        control: OperationControl::default(),
    }
}
fn assert_capture(home: &Path, outcome: &Outcome, pin: &SourcePin, snapshot: &str, kind: &str) {
    assert_eq!(outcome.provenance.len(), 1);
    let capture = &outcome.provenance[0];
    assert_eq!(capture["provenance"]["kind"], kind);
    assert_eq!(capture["provenance"]["sourceCommit"], pin.exact_commit);
    assert_eq!(capture["provenance"]["snapshot"], snapshot);
    assert_eq!(capture["complete"], outcome.complete);
    let saved = evidence::read_capture(home, capture["id"].as_str().unwrap()).unwrap();
    assert_eq!(&saved, capture);
    let bytes =
        evidence::read_blob(home, &capture["blob"], evidence::DEFAULT_CAPTURE_BYTES).unwrap();
    assert_eq!(bytes, serde_json::to_vec(&outcome.data).unwrap());
    assert_eq!(capture["blob"]["sha256"], storage::sha256(&bytes));
}

#[test]
fn warm_summary_cli_cold_then_warm_keeps_full_cache_query_inspect_and_capture_consumers() {
    let fixture = Fixture::new();
    let source = fixture.pin(&BTreeMap::from([
        (
            "Main.lua".into(),
            "function SummaryProbe()\n return 1\nend\n".into(),
        ),
        ("Main.toc".into(), "## Interface: 120100\nMain.lua\n".into()),
    ]));
    let selected = selection::pin_selection(
        &fixture.home,
        selection::SelectionSpec {
            source: Some(source.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let inv = invoke(&fixture.home, &selected.id, "source index");
    let cold = crate::dispatch(&inv).unwrap();
    assert!(cold.data["io"].get("cachedIndex").is_none());
    assert_eq!(cold.data["io"]["parsedDocuments"], 2);
    assert_capture(&fixture.home, &cold, &source, &selected.id, "source-query");
    let manifest = read_manifest(&fixture.home, &source);
    let digest = manifest["blob"]["sha256"].as_str().unwrap();
    let path = fixture
        .home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    let bytes = fs::read(&path).unwrap();
    assert_eq!(storage::sha256(&bytes), digest);
    let persisted: Index = persistence_wire::decode(
        &bytes,
        &index::fingerprint(&source),
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(persisted.documents.len(), 2);
    assert!(
        persisted
            .symbols
            .iter()
            .any(|symbol| symbol.name == "SummaryProbe")
    );
    assert_eq!(serde_json::to_value(&persisted.summary).unwrap(), {
        let mut v = cold.data.clone();
        v.as_object_mut().unwrap().remove("io");
        v
    });
    let full =
        index::build_with_control(&fixture.home, &source, &OperationControl::default()).unwrap();
    let warm = crate::dispatch(&inv).unwrap();
    assert_eq!(
        no_elapsed(warm.data.clone()),
        no_elapsed(serde_json::to_value(&full.summary).unwrap())
    );
    assert_eq!(
        no_elapsed(warm.coverage.clone()),
        no_elapsed(full.coverage())
    );
    assert_eq!(warm.selection, cold.selection);
    assert_eq!(warm.complete, cold.complete);
    assert_capture(&fixture.home, &warm, &source, &selected.id, "source-query");
    let mut query = invoke(&fixture.home, &selected.id, "source query");
    query.positional = Some("SummaryProbe".into());
    query.flags.insert("--mode".into(), "precise".into());
    let searched = crate::dispatch(&query).unwrap();
    assert!(
        searched.data["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "SummaryProbe")
    );
    assert_capture(
        &fixture.home,
        &searched,
        &source,
        &selected.id,
        "source-query",
    );
    let mut inspect = invoke(&fixture.home, &selected.id, "source inspect");
    inspect
        .flags
        .insert("--symbol".into(), "SummaryProbe".into());
    let inspected = crate::dispatch(&inspect).unwrap();
    assert!(
        inspected.data["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["name"] == "SummaryProbe")
    );
    assert_capture(
        &fixture.home,
        &inspected,
        &source,
        &selected.id,
        "source-inspect",
    );
    assert_eq!(read_manifest(&fixture.home, &source), manifest);
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn warm_summary_missing_object_or_obsolete_manifest_keeps_original_cold_fallback() {
    let fixture = Fixture::new();
    let source = fixture.pin(&BTreeMap::from([(
        "Main.lua".into(),
        "function RecoverSummary() end\n".into(),
    )]));
    let (cold, _) =
        build_summary_with_control(&fixture.home, &source, &OperationControl::default()).unwrap();
    let expected = {
        let mut value = cold.clone();
        value.as_object_mut().unwrap().remove("io");
        value
    };
    for miss in ["object", "schema", "identity"] {
        let mut manifest = read_manifest(&fixture.home, &source);
        if miss == "object" {
            let digest = manifest["blob"]["sha256"].as_str().unwrap();
            fs::remove_file(
                fixture
                    .home
                    .join("cache/objects")
                    .join(&digest[..2])
                    .join(&digest[2..]),
            )
            .unwrap();
        } else {
            manifest[miss] = json!("obsolete");
            crate::persistence::put_manifest(
                &fixture.home,
                &manifest_key(&source),
                &manifest,
                &OperationControl::default(),
            )
            .unwrap();
        }
        let (rebuilt, _) =
            build_summary_with_control(&fixture.home, &source, &OperationControl::default())
                .unwrap();
        assert!(rebuilt["io"].get("cachedIndex").is_none());
        let mut actual = rebuilt;
        actual.as_object_mut().unwrap().remove("io");
        assert_eq!(actual, expected);
        let full = index::build_with_control(&fixture.home, &source, &OperationControl::default())
            .unwrap();
        assert_eq!(full.documents.len(), 1);
        assert!(
            full.symbols
                .iter()
                .any(|symbol| symbol.name == "RecoverSummary")
        );
    }
}

#[test]
fn warm_summary_actual_file_cap_precedes_binary_decode_and_never_publishes_fallback() {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let fixture = Fixture::new();
    let pin = pin();
    let (mut manifest, original_path) = publish(&fixture.home, &pin, b"not binary");
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&original_path)
        .unwrap();
    file.set_len((128 << 20) + 1).unwrap();
    storage::finish_file(file, Ok(()), &OperationControl::default()).unwrap();
    // Authenticate the actual oversized bytes. A stale small-blob digest would
    // hide a removed cap behind the unrelated SHA mismatch branch. Stream this
    // fixture hash with fixed scratch rather than allocate another 128 MiB.
    let mut file = fs::File::open(&original_path).unwrap();
    let mut hasher = Sha256::new();
    let mut buffer = [0; 65536];
    let mut bytes = 0usize;
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        bytes += read;
        hasher.update(&buffer[..read]);
    }
    assert_eq!(bytes, (128 << 20) + 1);
    storage::finish_file(file, Ok(()), &OperationControl::default()).unwrap();
    let digest = format!("{:x}", hasher.finalize());
    let path = fixture
        .home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::rename(original_path, &path).unwrap();
    manifest["blob"]["sha256"] = json!(digest);
    manifest["blob"]["bytes"] = json!((128 << 20) + 1);
    crate::persistence::put_manifest(
        &fixture.home,
        &manifest_key(&pin),
        &manifest,
        &OperationControl::default(),
    )
    .unwrap();
    let error = comparable_error(&fixture.home, &pin, &OperationControl::default());
    assert_eq!(error["code"], "codebase.source_integrity");
    assert_eq!(error["message"], "Cached index content differs");
    assert_eq!(read_manifest(&fixture.home, &pin), manifest);
    assert_eq!(fs::metadata(&path).unwrap().len(), (128 << 20) + 1);
    assert_closed(&path);
}
