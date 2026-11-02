//! Explicit current-fixture importer. Never copies a private SQLite image.
use lycheedev_core::{
    control::OperationControl,
    metadata::DataCacheMetadata,
    selection::{self, DataPin, SelectionSpec},
    workspace::data_disk::publish_shared_blob_with_check,
};
use serde::Deserialize;
use serde_json::Value;
use std::{fs, path::{Path, PathBuf}, time::{Duration, Instant}};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document { key: String, value: Value, blob_path: PathBuf }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    owner_receipt: PathBuf,
    home: PathBuf,
    fixture_home: PathBuf,
    snapshot: String,
    pin: DataPin,
    documents: Vec<Document>,
}
fn bounded(path: &Path, maximum: u64) -> Vec<u8> {
    let metadata = fs::symlink_metadata(path).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= maximum);
    let bytes = fs::read(path).unwrap();
    assert_eq!(bytes.len() as u64, metadata.len());
    bytes
}
fn below(root: &Path, path: &Path) {
    assert!(root.is_absolute() && path.is_absolute());
    assert!(!fs::symlink_metadata(root).unwrap().file_type().is_symlink());
    let base = fs::canonicalize(root).unwrap();
    let actual = fs::canonicalize(path).unwrap();
    assert!(actual.starts_with(&base) && actual != base);
    let mut current = path.to_path_buf();
    while current != root {
        assert!(!fs::symlink_metadata(&current).unwrap().file_type().is_symlink());
        assert!(current.pop());
    }
}
#[test]
#[ignore]
fn import_owned_agent_fixture() {
    let path = PathBuf::from(std::env::var_os("LYCHEEDEV_AGENT_IMPORT").expect("explicit importer request"));
    let request: Request = serde_json::from_slice(&bounded(&path, 1 << 20)).unwrap();
    assert_eq!(request.schema, "lycheedev.paired-agent-seed.recovered.v1");
    let owner: Value = serde_json::from_slice(&bounded(&request.owner_receipt, 4 << 20)).unwrap();
    assert_eq!(owner["schema"], "lycheedev.disk-owner.v1");
    assert!(matches!(owner["owner"].as_str(), Some("paired-agent" | "skill-data-usage")));
    assert_eq!(owner["state"], "active");
    assert_eq!(owner["category"], "scratch");
    assert_eq!(owner["retention"], "rebuildable");
    assert!(owner["activePid"].as_u64().is_some_and(|pid| pid > 0));
    let id = owner["id"].as_str().unwrap();
    assert_eq!(id.len(), 36);
    assert!(id.bytes().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) { byte == b'-' }
        else { byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) }
    }));
    let workspace = request.owner_receipt.ancestors().nth(5).unwrap();
    assert_eq!(request.owner_receipt, workspace.join(".tmp/rust-disk/evidence").join(id).join("receipt.json"));
    below(workspace, &request.owner_receipt);
    let root = Path::new(owner["directory"].as_str().unwrap());
    let expected_root = workspace.join(".tmp/rust-scratch").join(id);
    assert_eq!(root, expected_root.as_path());
    assert_eq!(owner["commandSHA256"], lycheedev_core::storage::sha256(&serde_json::to_vec(&owner["command"]).unwrap()));
    below(root, &request.home);
    below(root, &path);
    assert!(!request.documents.is_empty() && request.documents.len() <= 64);
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(60));
    let pin = selection::pin_selection_controlled(&request.home, SelectionSpec { data: Some(request.pin), ..Default::default() }, &control).unwrap();
    assert_eq!(pin.id, request.snapshot);
    for document in request.documents {
        assert!(document.key.starts_with("definition/") && document.key.len() == 75);
        assert_eq!(document.value.as_object().unwrap().len(), 2);
        below(&request.fixture_home, &document.blob_path);
        let bytes = bounded(&document.blob_path, 1 << 20);
        let blob = publish_shared_blob_with_check(&request.home, &bytes, 1 << 20, &control, &|| Ok(())).unwrap();
        assert_eq!(serde_json::to_value(&blob).unwrap(), document.value["blob"]);
        let url = document.value["url"].as_str().unwrap();
        assert!(url.starts_with("https://raw.githubusercontent.com/wowdev/WoWDBDefs/"));
        assert_eq!(document.key, format!("definition/{}", lycheedev_core::storage::sha256(url.as_bytes())));
        DataCacheMetadata::put_controlled_with_check(&request.home, &document.key, &document.value, None, &control, || Ok(())).unwrap();
    }
}
