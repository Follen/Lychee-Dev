use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
struct Fixture {
    root: PathBuf,
    home: PathBuf,
    request: TargetRequest,
    control: OperationControl,
    key: String,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-local-target-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let home = root.join("home");
        lycheedev_core::workspace::initialize(&home).unwrap();
        let game = root.join("game");
        let client = game.join("_retail_");
        std::fs::create_dir_all(&client).unwrap();
        std::fs::write(client.join(".flavor.info"), b"wow\n").unwrap();
        let raw=b"root = 01234567890123456789012345678901\nencoding = 01234567890123456789012345678901 abcdef01234567890123456789012345\nencoding-size = 20 30\n";
        let key = casc::hex(&Md5::digest(raw));
        let directory = game.join(format!("Data/config/{}/{}", &key[..2], &key[2..4]));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(&key), raw).unwrap();
        std::fs::write(game.join(".build.info"),format!("Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|12.1.5.70000|{key}|{key}|1\n")).unwrap();
        let control = OperationControl::default();
        Self {
            root,
            home,
            request: TargetRequest {
                installation: client,
                product: "retail".into(),
                region: "cn".into(),
                locale: "zhCN".into(),
                definitions: "a".repeat(40),
                offline: true,
                ..Default::default()
            },
            control,
            key,
        }
    }
    fn cleanup(self) {
        std::fs::remove_dir_all(self.root).unwrap();
    }
}
#[test]
fn local_target_publishes_exact_capture_body_and_original_scope() {
    let f = Fixture::new();
    let result = prepare_target(&f.home, &f.request, &f.control).unwrap();
    assert_eq!(result.pin.data.as_ref().unwrap().build_config, f.key);
    assert_eq!(
        result.installation,
        // Go resolves the selected directory before taking its parent. TEMP
        // may contain an 8.3 alias; display_path alone does not resolve it.
        client::display_path(&std::fs::canonicalize(f.root.join("game")).unwrap())
    );
    let capture = serde_json::to_value(&result.capture).unwrap();
    let raw = evidence::read_blob(&f.home, &capture["blob"], 65536).unwrap();
    let body: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(body["pin"], serde_json::to_value(&result.pin).unwrap());
    assert_eq!(body["client"], result.client);
    assert_eq!(body["installation"], result.installation);
    assert_eq!(body["configurations"].as_array().unwrap().len(), 2);
    assert!(body["catalogSHA256"].as_str().unwrap().len() == 64);
    assert!(matches!(body["client"].as_object().unwrap().len(), 7 | 8));
    for extra in ["interfaceKnown", "displayName", "channel", "metadataBranch"] {
        assert!(body["client"].get(extra).is_none());
    }
    let selected_directory =
        client::display_path(&std::fs::canonicalize(&f.request.installation).unwrap());
    assert_eq!(result.client["directory"], selected_directory);
    // Fixed Go uses client.Directory, after selection/canonicalization, rather
    // than the original spelling (TEMP can include a Windows 8.3 alias).
    assert_eq!(capture["provenance"]["locator"], selected_directory);
    assert_eq!(capture["provenance"]["kind"], "local-target");
    assert!(f.control.stop_cause().unwrap().is_none());
    f.control.ordered_child(f.control.deadline()).unwrap();
    f.cleanup();
}
#[test]
fn local_target_inherits_parent_definition_after_installation_identity() {
    let mut f = Fixture::new();
    let first = prepare_target(&f.home, &f.request, &f.control).unwrap();
    f.request.parent = first.pin.id.clone();
    f.request.definitions.clear();
    let next = prepare_target(&f.home, &f.request, &f.control).unwrap();
    assert_eq!(next.pin.parent, first.pin.id);
    assert_eq!(
        next.pin.data.as_ref().unwrap().definition_commit,
        "a".repeat(40)
    );
    f.cleanup();
}
#[test]
fn local_target_config_change_reaches_shared_pin_conflict_without_rewriting_parent() {
    let mut f = Fixture::new();
    let first = prepare_target(&f.home, &f.request, &f.control).unwrap();
    let pin_key = format!("pin/{}", first.pin.id);
    let before = Metadata::read_document_raw_controlled(&f.home, &pin_key, &f.control).unwrap();
    let original_capture = serde_json::to_value(&first.capture).unwrap();
    // Another valid configuration for the same product/build is new input;
    // the original DataPin remains fixed after the new metadata is admitted.
    let raw = b"build-uid = wow\nroot = ffffffffffffffffffffffffffffffff\nencoding = 01234567890123456789012345678901 abcdef01234567890123456789012345\nencoding-size = 20 30\n";
    let key = casc::hex(&Md5::digest(raw));
    assert_ne!(key, f.key);
    let game = f.root.join("game");
    let directory = game.join(format!("Data/config/{}/{}", &key[..2], &key[2..4]));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(&key), raw).unwrap();
    std::fs::write(game.join(".build.info"), format!("Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|12.1.5.70000|{key}|{key}|1\n")).unwrap();
    f.request.parent = first.pin.id.clone();
    f.request.definitions.clear();
    let error = prepare_target(&f.home, &f.request, &f.control).unwrap_err();
    assert_eq!(error.code, "selection.fixed_data_conflict");
    assert_eq!(error.exit_code, 4);
    assert_eq!(error.message, "cannot change inherited data");
    assert_eq!(
        selection::load_pinned_controlled(&f.home, &first.pin.id, &f.control)
            .unwrap()
            .data,
        first.pin.data
    );
    let after = Metadata::read_document_raw_controlled(&f.home, &pin_key, &f.control).unwrap();
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.bytes, before.bytes);
    let metadata = Metadata::open_controlled(&f.home, false, &f.control).unwrap();
    assert_eq!(metadata.list::<Value>("pin/", 100).unwrap().len(), 1);
    let captures = metadata.list::<Value>("capture/", 100).unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0], original_capture);
    metadata.close().unwrap();
    f.cleanup();
}
#[test]
fn local_target_build_failure_precedes_bad_definition_and_does_not_publish() {
    let mut f = Fixture::new();
    f.request.full_build = "12.1.5.70001".into();
    f.request.definitions = "invalid-definition".into();
    let error = prepare_target(&f.home, &f.request, &f.control).unwrap_err();
    assert_eq!(error.code, "selection.target_build_unavailable");
    let m = Metadata::open(&f.home, false).unwrap();
    assert!(m.list::<Value>("pin/", 100).unwrap().is_empty());
    assert!(m.list::<Value>("capture/", 100).unwrap().is_empty());
    m.close().unwrap();
    f.cleanup();
}
