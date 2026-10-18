//! A dedicated retained-input exporter, not an alternate product consumer.
#[allow(dead_code)]
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Fixture, encrypted_frame, encrypted_sections};
use lycheedev_core::storage;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

#[test]
#[ignore = "explicit active Skill owner and fresh caller root required"]
fn prepare_skill_partial_fixture() {
    let root = PathBuf::from(std::env::var_os("LYCHEEDEV_SKILL_PARTIAL_FIXTURE").expect("fresh root"));
    let receipt = PathBuf::from(std::env::var_os("LYCHEEDEV_SKILL_OWNER_RECEIPT").expect("active owner receipt"));
    let metadata = fs::symlink_metadata(&receipt).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 1 << 20);
    let owner: Value = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
    assert_eq!(owner["schema"], "lycheedev.disk-owner.v1");
    assert_eq!(owner["owner"], "skill-data-usage");
    assert_eq!(owner["category"], "scratch");
    assert_eq!(owner["state"], "active");
    let directory = PathBuf::from(owner["directory"].as_str().unwrap());
    let metadata = fs::symlink_metadata(&directory).unwrap();
    assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
    assert_eq!(root, directory.join("partial-fixture"));
    assert_eq!(fs::symlink_metadata(&root).unwrap_err().kind(), std::io::ErrorKind::NotFound);
    let table = encrypted_sections(201);
    let raw = table.raw.clone();
    let (encoding_key, encoded) = encrypted_frame(&raw);
    let descriptor = json!({
        "schema":"lycheedev.skill-partial-fixture.v2",
        "rawBytes":raw.len(), "rawSHA256":storage::sha256(&raw),
        "encodedBytes":encoded.len(), "encodedSHA256":storage::sha256(&encoded),
        "encodingKey":encoding_key,
        "encryptedChunkHex":"4508efcdab9078563412041122334453c6ab63b7daa05cd383",
        "keyID":"1234567890abcdef", "key":"000102030405060708090a0b0c0d0e0f",
        "expectedRows":[[1,42],[2,99]]
    });
    let fixture = Fixture::with_encoded_table_at(root, table, encoded);
    fs::write(fixture.root.join("sample.wdc"), raw).unwrap();
    fs::write(fixture.root.join("partial-identity.json"), serde_json::to_vec_pretty(&descriptor).unwrap()).unwrap();
    // Fixture has no cleanup Drop. Only the outer owner retires this root after
    // its process tree is reaped and complete evidence is sealed.
}
