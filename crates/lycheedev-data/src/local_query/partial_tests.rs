//! Actual local CASC/Encoding/Root -> partial table -> public consumers.
use super::*;
#[path = "../../tests/support/partial_contract.rs"]
mod contract;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
const UNRELATED: &str = "0000000000000001 000102030405060708090a0b0c0d0e0f\n";
const CORRECT: &str = "1234567890abcdef 000102030405060708090a0b0c0d0e0f\n";
const WRONG: &str = "1234567890abcdef ffffffffffffffffffffffffffffffff\n";
fn invocation(fx: &fixture::Fixture) -> Invocation {
    let key = fx.root.join("keys.txt");
    std::fs::write(&key, UNRELATED).unwrap();
    let mut inv = fx.invocation("data db2 schema", &[]);
    inv.positional = Some("Sample".into());
    inv.flags
        .insert("--key-file".into(), key.to_string_lossy().into_owned());
    inv
}
#[test]
fn local_missing_key_preserves_readable_db2_sql_csv_stream_and_strict_assets() {
    let table = fixture::encrypted_sections(201);
    let encoded = fixture::encrypted_frame(&table.raw).1;
    let fx = fixture::Fixture::with_encoded_table(table, encoded);
    let inv = invocation(&fx);
    contract::exercise(&inv, &fx.root.join("partial.csv"));
    let mut public = inv.clone();
    public.flags.remove("--key-file");
    let partial = crate::db2::dispatch(&public).unwrap();
    assert_eq!(
        partial.data["tables"][0]["keySource"]["state"],
        "unavailable_offline"
    );
    assert!(!partial.complete);
    let key_blob = evidence::blob_path(&fx.home, keys::PUBLIC_SHA256).unwrap();
    std::fs::create_dir_all(key_blob.parent().unwrap()).unwrap();
    std::fs::write(&key_blob, b"corrupt public provider").unwrap();
    assert!(
        crate::db2::dispatch(&public).is_err(),
        "provider corruption became partial data"
    );
    let pin = selection::load_pinned(&fx.home, &fx.pin)
        .unwrap()
        .spec
        .data
        .unwrap();
    let error = read_files(&inv, &pin, &[201], inv.control.deadline())
        .err()
        .unwrap();
    assert_eq!(error.code, "container.key_unavailable");
    assert_eq!(error.details["missingKey"], true);
    let key = std::path::Path::new(inv.flag("--key-file").unwrap());
    std::fs::write(key, CORRECT).unwrap();
    contract::complete_page(&inv);
    contract::complete_page(&inv);
    assert_eq!(
        read_files(&inv, &pin, &[201], inv.control.deadline())
            .unwrap()
            .len(),
        1
    );
    std::fs::write(key, WRONG).unwrap();
    assert!(
        crate::db2::dispatch(&inv).is_err(),
        "changed key identity reused complete bytes"
    );
    std::fs::remove_dir_all(fx.root).unwrap();
}
#[test]
fn local_missing_key_never_downgrades_checksum_metadata_section_key_or_cancellation() {
    for mode in [
        "checksum",
        "zero-clear-digest",
        "zero-encrypted-digest",
        "metadata",
        "nested-metadata",
        "headerless",
        "section-key",
        "cancel",
        "bad-key-document",
    ] {
        let mut table = fixture::encrypted_sections(201);
        if mode == "section-key" {
            table.raw[112..120].copy_from_slice(&2u64.to_le_bytes());
        }
        let mut encoded = fixture::encrypted_frame(&table.raw).1;
        if mode == "checksum" {
            *encoded.last_mut().unwrap() ^= 1;
        }
        if mode == "zero-clear-digest" {
            encoded[20..36].fill(0);
            encoded[60 + 1 + table.raw.len() - 12] ^= 1;
        }
        if mode == "zero-encrypted-digest" {
            encoded[44..60].fill(0);
        }
        if mode == "metadata" {
            encoded =
                fixture::framed_chunks(&[(&encoded[encoded.len() - 25..], table.raw.len())]).1;
        }
        if mode == "headerless" {
            encoded = [b"BLTE\0\0\0\0".as_slice(), &encoded[encoded.len() - 25..]].concat();
        }
        if mode == "nested-metadata" {
            let nested = [b"F".as_slice(), encoded.as_slice()].concat();
            encoded = fixture::framed_chunks(&[(&nested, table.raw.len())]).1;
        }
        let fx = fixture::Fixture::with_encoded_table(table, encoded);
        let inv = invocation(&fx);
        if mode == "cancel" {
            inv.control
                .cancel_handle()
                .store(true, std::sync::atomic::Ordering::Release);
        }
        if mode == "bad-key-document" {
            std::fs::write(inv.flag("--key-file").unwrap(), b"bad source").unwrap();
        }
        let error = crate::db2::dispatch(&inv).unwrap_err();
        if mode == "cancel" {
            assert_eq!(error.exit_code, 7);
        }
        if mode == "headerless" {
            assert_eq!(error.code, "container.unsupported_encoding");
        }
        if matches!(mode, "zero-clear-digest" | "zero-encrypted-digest") {
            assert_eq!(error.code, "container.integrity_mismatch");
        }
        if matches!(mode, "metadata" | "nested-metadata" | "section-key") {
            assert_eq!(error.code, "container.key_unavailable");
        }
        let metadata = lycheedev_core::metadata::Metadata::open(&fx.home, false).unwrap();
        assert!(metadata.list::<Value>("capture/", 100).unwrap().is_empty());
        metadata.close().unwrap();
        std::fs::remove_dir_all(fx.root).unwrap();
    }
}

#[test]
fn local_table_policy_skips_full_public_alias_when_keys_are_unavailable_offline() {
    use md5::{Digest, Md5};
    let table = fixture::encrypted_sections(201);
    let complete_bytes = table.raw.clone();
    let encoded = fixture::encrypted_frame(&complete_bytes).1;
    let content_key = casc::hex(&Md5::digest(&complete_bytes));
    let mut identity = sha2::Sha256::new();
    identity.update(format!(
        "decoded-v1\0{content_key}\0public\0{}\0",
        keys::PUBLIC_SHA256
    ));
    identity.update(&encoded);
    let cache_key = format!("decoded-v1/{}", casc::hex(&identity.finalize()));
    let fx = fixture::Fixture::with_encoded_table(table, encoded);
    // Simulate a previous complete read whose public key document is no longer
    // cached. The full alias and blob use the real fixture's authenticated CKey.
    let full_blob = evidence::publish_blob(&fx.home, &complete_bytes, 4 << 20).unwrap();
    let entry = json!({"contentKey":content_key, "blob":full_blob, "keys":{"kind":"public","state":"loaded","commit":keys::PUBLIC_COMMIT,"sha256":keys::PUBLIC_SHA256}});
    let mut metadata = lycheedev_core::metadata::Metadata::open(&fx.home, true).unwrap();
    metadata.put(&cache_key, &entry, None).unwrap();
    metadata.close().unwrap();
    assert!(
        !evidence::blob_path(&fx.home, keys::PUBLIC_SHA256)
            .unwrap()
            .try_exists()
            .unwrap()
    );
    let inv = fx.invocation("data db2", &[("--table", "Sample"), ("--limit", "10")]);
    let page = crate::db2::dispatch(&inv).unwrap();
    assert!(!page.complete);
    assert_eq!(page.data["page"]["rows"], json!([{"ID":1,"Value":42}]));
    assert_eq!(
        page.data["file"]["keySource"]["state"],
        "unavailable_offline"
    );
    assert_eq!(page.data["file"]["contentVerified"], false);
    assert_eq!(page.data["file"]["content"], json!({"sha256":"","bytes":0}));
    assert_ne!(page.data["file"]["partialContent"], full_blob);
    let metadata = lycheedev_core::metadata::Metadata::open(&fx.home, false).unwrap();
    assert_eq!(metadata.read::<Value>(&cache_key).unwrap(), entry);
    metadata.close().unwrap();
    let pin = selection::load_pinned(&fx.home, &fx.pin)
        .unwrap()
        .spec
        .data
        .unwrap();
    assert_eq!(
        read_files(&inv, &pin, &[201], inv.control.deadline())
            .err()
            .unwrap()
            .code,
        "container.key_unavailable"
    );
    let public_blob = evidence::blob_path(&fx.home, keys::PUBLIC_SHA256).unwrap();
    std::fs::create_dir_all(public_blob.parent().unwrap()).unwrap();
    std::fs::write(public_blob, b"corrupt public provider").unwrap();
    assert!(
        crate::db2::dispatch(&inv).is_err(),
        "corrupt key provider became a cache miss"
    );
    std::fs::remove_dir_all(fx.root).unwrap();
}

#[path = "../../tests/support/partial_string_contract.rs"]
mod string_contract;
#[path = "../../tests/support/partial_strings.rs"]
mod strings_fixture;
#[test]
fn local_partial_cross_partition_string_stops_at_nul_before_unknown_bytes() {
    for gap in [16, 512, 4] {
        let table = strings_fixture::table(201, gap, b"visible\0");
        let start = strings_fixture::pool(&table.raw);
        let encoded = strings_fixture::frame_at(&table.raw, gap).1;
        let fx = fixture::Fixture::with_encoded_table(table, encoded);
        let inv = invocation(&fx);
        string_contract::exercise(
            &inv,
            &fx.root.join("partial-string.csv"),
            start + gap,
            gap != 4,
        );
        let pin = selection::load_pinned(&fx.home, &fx.pin)
            .unwrap()
            .spec
            .data
            .unwrap();
        assert_eq!(
            read_files(&inv, &pin, &[201], inv.control.deadline())
                .err()
                .unwrap()
                .code,
            "container.key_unavailable"
        );
        if gap != 4 {
            std::fs::write(inv.flag("--key-file").unwrap(), CORRECT).unwrap();
            string_contract::complete(&inv);
            read_files(&inv, &pin, &[201], inv.control.deadline()).unwrap();
        }
        std::fs::remove_dir_all(fx.root).unwrap();
    }
}
