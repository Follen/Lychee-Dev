//! Selected encoded/decoded allowance is independent from metadata caps.
//! Synthetic authenticated inputs. Large-content preparation is explicit so the
//! frozen Go and Rust public commands can observe the same authenticated body.
#[path = "support/data_cache.rs"]
mod data_cache;
#[path = "support/catalog_fixture.rs"]
#[allow(dead_code)]
mod fixture;

use fixture::{Cell, Fixture, table};
use lycheedev_data::{assets::{self, request::ExportRequest}, request::{DataContext, SourceOptions}};
use lycheedev_data::query_source;
use std::{
    io::{Read, Seek, SeekFrom, Write},
    time::{Duration, Instant},
};

fn input() -> (Fixture, Vec<u8>) {
    let table = table("ContentBudget", 501, &[("ID", "uint")], &[vec![Cell::U(7)]]);
    let bytes = table.raw.clone();
    (Fixture::new(&[table]), bytes)
}
fn context(fx: &Fixture, maximum: usize) -> DataContext {
    DataContext { home: fx.home.clone(), cwd: fx.root.clone(), control: Default::default(),
        source: SourceOptions { snapshot: Some(fx.pin.clone()), installation: Some(fx.client.clone()), offline: true,
            max_file_bytes: maximum, ..Default::default() } }
}
fn read(ctx: &DataContext) -> lycheedev_core::Result<Vec<query_source::File>> {
    let pin = ctx.resolve_pinned()?;
    query_source::read_files(
        ctx,
        pin.data.as_ref().unwrap(),
        &[501],
        Instant::now() + Duration::from_secs(30),
    )
}

#[test]
fn maximum_content_allowance_small_file_cold_and_warm() {
    let (fx, expected) = input();
    let ctx = context(&fx, 512 << 20);
    let cold = read(&ctx).unwrap().remove(0);
    assert_eq!(cold.bytes.as_ref(), expected.as_slice());
    assert_eq!(cold.reading["decodedCache"]["hits"], 0);
    assert_eq!(cold.reading["decodedCache"]["contentDecodeCalls"], 2);
    let warm = read(&ctx).unwrap().remove(0);
    assert_eq!(warm.bytes.as_ref(), expected.as_slice());
    assert_eq!(warm.reading["decodedCache"]["hits"], 2);
    assert_eq!(warm.reading["decodedCache"]["contentDecodeCalls"], 0);
    assert_eq!(
        warm.reading["verification"]["contentWholeDecodedCKey"],
        true
    );
}

#[cfg(windows)]
#[test]
fn asset_inspect_cold_and_warm_with_existing_shared_archive_writer() {
    use lycheedev_core::{evidence, storage};
    use lycheedev_data::assets::request::AssetRequest;
    use md5::Digest;
    use std::os::windows::fs::OpenOptionsExt;

    let (fx, expected) = input();
    let ctx = context(&fx, 1 << 20);
    let sha256 = storage::sha256(&expected);
    let content_key = lycheedev_data::casc::hex(&md5::Md5::digest(&expected));
    // Encoding, Root and selected content share this real synthetic archive.
    // Keep an actual write-access handle for both cold decode and cache reuse.
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .share_mode(1 | 2 | 4)
        .open(fx.root.join("game/Data/data/data.000"))
        .unwrap();
    for pass in 0..2 {
        let found = assets::dispatch(&ctx, &AssetRequest::Inspect { file_id: 501 }).unwrap();
        assert_eq!(found.data["contentVerified"], true);
        assert_eq!(found.data["content"]["bytes"], expected.len() as u64);
        assert_eq!(found.data["content"]["sha256"], sha256);
        assert_eq!(found.data["entry"]["contentKey"], content_key);
        assert_eq!(found.data["verification"]["contentWholeDecodedCKey"], true);
        assert_eq!(found.provenance.len(), 1);
        let capture = &found.provenance[0];
        assert_eq!(capture["blob"], found.data["content"]);
        assert_eq!(capture["complete"], true);
        assert_eq!(capture["truncated"], false);
        assert_eq!(capture["provenance"]["snapshot"], fx.pin);
        assert_eq!(capture["provenance"]["dataBuild"], fixture::BUILD);
        assert_eq!(
            evidence::read_capture(&fx.home, capture["id"].as_str().unwrap()).unwrap(),
            *capture
        );
        assert_eq!(
            evidence::read_blob(&fx.home, &capture["blob"], 1 << 20).unwrap(),
            expected
        );
        assert_eq!(
            found.data["decodedCache"]["hits"],
            if pass == 0 { 0 } else { 2 }
        );
        assert_eq!(
            found.data["decodedCache"]["contentDecodeCalls"],
            if pass == 0 { 2 } else { 0 }
        );
        ctx.control.check().unwrap();
    }
    storage::close_file(writer).unwrap();
    std::fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn selected_encoded_and_decoded_lengths_are_bounded_on_cold_and_warm_reads() {
    let (fx, expected) = input();
    // The selected headerless BLTE adds nine bytes. Frozen Go applies max to
    // both lengths, while Encoding/Root metadata have a separate allowance.
    let maximum = expected.len() + 9;
    let ctx = context(&fx, maximum);
    for _ in 0..2 {
        assert_eq!(read(&ctx).unwrap()[0].bytes.as_ref(), expected.as_slice());
    }
    for too_small in [expected.len() - 1, expected.len(), expected.len() + 8] {
        let ctx = context(&fx, too_small);
        assert_eq!(read(&ctx).err().unwrap().code, "records.metadata_limit");
        let fresh = input().0;
        let ctx = context(&fresh, too_small);
        assert_eq!(read(&ctx).err().unwrap().code, "records.metadata_limit");
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
    let mut ctx = context(&fx, 512 << 20);
    read(&ctx).unwrap();
    let captures = || {
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<serde_json::Value>("capture/", 100)
            .unwrap()
    };
    let before = captures();
    ctx.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(read(&ctx).err().unwrap().exit_code, 7);
    assert_eq!(query_source::asset(&ctx, 501).unwrap_err().exit_code, 7);
    assert_eq!(
        captures(),
        before,
        "cancelled content must not publish a capture"
    );
    ctx.control = Default::default();
    // First object is the content: corrupt its payload without altering extent.
    let archive = fx.root.join("game/Data/data/data.000");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(archive)
        .unwrap();
    file.seek(SeekFrom::Start(39)).unwrap();
    file.write_all(b"X").unwrap();
    drop(file);
    assert_eq!(read(&ctx).err().unwrap().code, "archive.index_integrity");
}

#[test]
#[ignore = "prepare shared compressed CASC input for explicit public Go/Rust observation"]
fn prepare_compressed_content_public_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LYCHEEDEV_CONTENT_BUDGET_FIXTURE").expect("fixture output directory"),
    );
    let decoded = std::env::var("LYCHEEDEV_CONTENT_BUDGET_BYTES")
        .expect("explicit decoded content size")
        .parse()
        .unwrap();
    Fixture::compressed_at(root, decoded);
}

fn check_compressed_inspect_and_export(fx: &Fixture) {
    use lycheedev_core::evidence;
    use sha2::{Digest, Sha256};
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fx.root.join("input-identity.json")).unwrap())
            .unwrap();
    let content = &identity["content"];
    let maximum = usize::try_from(content["decodedBytes"].as_u64().unwrap()).unwrap();
    let ctx = context(fx, maximum);
    for pass in 0..2 {
        let found = query_source::asset(&ctx, 501).unwrap();
        assert_eq!(found.provenance.len(), 1);
        assert_eq!(found.data["content"]["bytes"], content["decodedBytes"]);
        assert_eq!(found.data["content"]["sha256"], content["sha256"]);
        assert_eq!(found.provenance[0]["blob"], found.data["content"]);
        assert_eq!(found.provenance[0]["complete"], true);
        assert_eq!(found.provenance[0]["truncated"], false);
        assert_eq!(
            evidence::read_capture(&fx.home, found.provenance[0]["id"].as_str().unwrap()).unwrap(),
            found.provenance[0]
        );
        let mut observed = 0u64;
        let blob = evidence::open_verified_blob_bounded(
            &fx.home,
            &found.data["content"],
            content["decodedBytes"].as_u64().unwrap(),
            &ctx.control,
            |block| {
                assert!(block.iter().all(|byte| *byte == b'x'));
                observed += block.len() as u64;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(observed, content["decodedBytes"]);
        drop(blob);
        if pass == 1 {
            assert_eq!(found.data["decodedCache"]["contentDecodeCalls"], 0);
            assert_eq!(found.data["decodedCache"]["hits"], 2);
        }
    }
    let output = fx.root.join("compressed-raw.bin");
    let mut ctx = context(fx, maximum);
    let project = fx.root.join("project");
    std::fs::create_dir_all(project.join("nested")).unwrap();
    let pin = lycheedev_core::selection::load_pinned(&fx.home, &fx.pin).unwrap();
    std::fs::write(project.join("lycheedev.json"), br#"{"schema":"lycheedev.project.v1","product":"retail"}"#).unwrap();
    std::fs::write(project.join("lycheedev.lock.json"), serde_json::to_vec(&serde_json::json!({
        "schema":"lycheedev.project-lock.v1", "selection":pin,
    })).unwrap()).unwrap();
    ctx.source.snapshot = None;
    ctx.cwd = project.join("nested");
    let request = ExportRequest { maximum_bytes: maximum, ..ExportRequest::raw(501, output.clone()) };
    let exported = assets::export::run(&ctx, &request).unwrap();
    assert_eq!(exported.selection["snapshot"], fx.pin);
    assert_eq!(exported.data["content"]["sha256"], content["sha256"]);
    assert_eq!(
        exported.data["sourceCapture"]["blob"],
        exported.data["content"]
    );
    let mut raw = std::fs::File::open(&output).unwrap();
    let mut sha = Sha256::new();
    let mut buffer = [0; 65536];
    let mut bytes = 0u64;
    loop {
        let n = raw.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
        bytes += n as u64;
    }
    assert_eq!(bytes, content["decodedBytes"]);
    assert_eq!(
        lycheedev_data::casc::hex(&sha.finalize()),
        content["sha256"]
    );
    let manifest = evidence::read_blob(&fx.home, &exported.provenance[1]["blob"], 1 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&manifest).unwrap(),
        exported.data
    );
}

#[test]
fn compressed_multichunk_inspect_cold_warm_and_raw_export() {
    let fx = Fixture::compressed_at(stream_fixture_root(), 17 << 20);
    check_compressed_inspect_and_export(&fx);
}

fn stream_fixture_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/rust-content-streaming-fixtures")
        .join(format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
}

#[test]
fn streaming_rejects_changed_warm_source_and_bad_ekey_without_new_captures() {
    use lycheedev_core::metadata::Metadata;
    let fx = Fixture::compressed_at(stream_fixture_root(), 1 << 20);
    let ctx = context(&fx, 1 << 20);
    query_source::asset(&ctx, 501).unwrap();
    let documents = || {
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<serde_json::Value>("", 100)
            .unwrap()
    };
    let before = documents();
    let archive = fx.root.join("game/Data/data/data.000");
    let original = std::fs::read(&archive).unwrap();
    // Framed EKey covers the header. Changed payload still has the same EKey,
    // but its chunk MD5 must fail even after a successful decoded-cache fill.
    let mut changed = original.clone();
    changed[30 + 36 + 8] ^= 1;
    std::fs::write(&archive, &changed).unwrap();
    assert_eq!(
        query_source::asset(&ctx, 501).unwrap_err().code,
        "container.integrity_mismatch"
    );
    assert_eq!(documents(), before);
    // A header digest change is rejected at the selected archive boundary.
    changed = original;
    changed[30 + 20] ^= 1;
    std::fs::write(&archive, &changed).unwrap();
    assert_eq!(
        query_source::asset(&ctx, 501).unwrap_err().code,
        "archive.index_integrity"
    );
    assert_eq!(documents(), before);
}

#[test]
fn streaming_whole_ckey_failure_publishes_no_selected_blob_cache_or_capture() {
    use lycheedev_core::{evidence, metadata::Metadata};
    let fx = Fixture::compressed_with_wrong_ckey_at(stream_fixture_root(), 1 << 20);
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fx.root.join("input-identity.json")).unwrap())
            .unwrap();
    let output = fx.root.join("failed-raw.bin");
    let ctx = context(&fx, 1 << 20);
    let request = ExportRequest { maximum_bytes: 1 << 20, ..ExportRequest::raw(501, output.clone()) };
    assert_eq!(
        assets::export::run(&ctx, &request).unwrap_err().code,
        "records.content_integrity"
    );
    assert!(!output.exists());
    let metadata = Metadata::open(&fx.home, false).unwrap();
    assert!(
        metadata
            .list::<serde_json::Value>("capture/", 100)
            .unwrap()
            .is_empty()
    );
    let decoded = data_cache::Cache::new(&fx.home)
        .list::<serde_json::Value>("decoded-v1/", 100)
        .unwrap();
    assert!(
        decoded
            .iter()
            .all(|entry| entry["contentKey"] != identity["content"]["contentKey"])
    );
    assert!(
        !evidence::blob_path(&fx.home, identity["content"]["sha256"].as_str().unwrap())
            .unwrap()
            .exists()
    );
}

#[test]
fn streamed_raw_export_conflict_overwrite_and_cancel_preserve_existing_output() {
    let fx = Fixture::compressed_at(stream_fixture_root(), 1 << 20);
    let output = fx.root.join("overwrite.bin");
    let alias = fx.root.join("prior-hardlink.bin");
    std::fs::write(&output, b"previous output").unwrap();
    std::fs::hard_link(&output, &alias).unwrap();
    let ctx = context(&fx, 1 << 20);
    let mut request = ExportRequest { maximum_bytes: 1 << 20, ..ExportRequest::raw(501, output.clone()) };
    assert_eq!(
        assets::export::run(&ctx, &request).unwrap_err().code,
        "records.export_conflict"
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"previous output");
    request.overwrite = true;
    let found = assets::export::run(&ctx, &request).unwrap();
    assert_eq!(std::fs::read(&alias).unwrap(), b"previous output");
    let raw = std::fs::read(&output).unwrap();
    assert_eq!(raw.len(), 1 << 20);
    assert!(raw.iter().all(|byte| *byte == b'x'));
    assert_eq!(
        found.data["content"]["sha256"],
        lycheedev_core::storage::sha256(&raw)
    );
    ctx.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(assets::export::run(&ctx, &request).unwrap_err().exit_code, 7);
    assert_eq!(std::fs::read(&output).unwrap(), raw);
}

#[test]
#[ignore = "run inspect/raw against an explicitly prepared real large-content CASC fixture"]
fn prepared_compressed_content_inspect_and_raw_export() {
    let root = std::path::PathBuf::from(
        std::env::var_os("LYCHEEDEV_CONTENT_BUDGET_FIXTURE").expect("fixture directory"),
    );
    let identity: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("input-identity.json")).unwrap()).unwrap();
    let fx = Fixture {
        home: identity["home"].as_str().unwrap().into(),
        client: identity["installation"].as_str().unwrap().into(),
        pin: identity["snapshot"].as_str().unwrap().into(),
        root,
    };
    check_compressed_inspect_and_export(&fx);
}
