use super::*;
use crate::test_support::TestDir;
fn make(dir: &TestDir, name: &str) {
    fs::create_dir_all(dir.0.join(name)).unwrap();
    fs::write(
        dir.0.join(name).join(format!("{name}.toc")),
        b"## Interface: 120000, 11508\n",
    )
    .unwrap();
    fs::write(dir.0.join(name).join("main.lua"), b"print('original')").unwrap();
}
fn request(dir: &TestDir, fields: Value) -> PluginPack {
    let mut value = fields;
    value["schema"] = json!("lycheedev.publish.v1.newbee.official.plugin.pack");
    PluginPack::parse(
        &serde_json::to_vec(&value).unwrap(),
        &dir.0,
        &OperationControl::default(),
    )
    .unwrap()
}
fn members(path: &Path) -> Vec<String> {
    let file = File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_owned())
        .collect()
}
#[test]
fn single_package_applies_ncc_ignore_precedence_and_reuses_the_original_zip() {
    let dir = TestDir::new();
    make(&dir, "Addon");
    let root = dir.0.join("Addon");
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".git/config"), b"private").unwrap();
    fs::write(root.join("old.zip"), b"previous artifact").unwrap();
    fs::write(root.join("asset.psd"), b"ignored").unwrap();
    fs::write(root.join("keep.txt"), b"keep").unwrap();
    fs::write(root.join(".gitignore"), b"keep.txt").unwrap();
    fs::write(
        root.join(".nccignore"),
        b"# precedence\nasset.psd\n!main.lua\n",
    )
    .unwrap();
    fs::write(
        root.join("ncc.json"),
        br#"{"game":"wow","type":"mod","id":91,"version":"1.2.3","ignore":["*.psd"]}"#,
    )
    .unwrap();
    let request = request(&dir, json!({"dir":"Addon"}));
    let control = OperationControl::default();
    let plan = request.plan(&control).unwrap();
    assert_eq!(plan["layout"], "single");
    assert_eq!(plan["file_count"], 3);
    assert_eq!(plan["archive_validation"], "not_built");
    assert!(!request.output.exists());
    let result = request.run(&control).unwrap();
    assert_eq!(result["reused"], false);
    let path = dir.0.join("Addon-1.2.3.zip");
    assert_eq!(
        members(&path),
        vec!["Addon/Addon.toc", "Addon/keep.txt", "Addon/main.lua"]
    );
    let before = fs::read(&path).unwrap();
    let (held, _) = storage::locked_sha256(&path, MAX_ZIP, &control).unwrap();
    assert_eq!(request.run(&control).unwrap()["reused"], true);
    storage::finish_file(held, Ok(()), &control).unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!dir.0.join("home").exists());
    fs::write(root.join("main.lua"), b"changed").unwrap();
    assert_eq!(request.run(&control).unwrap_err().exit_code, 4);
    assert_eq!(fs::read(path).unwrap(), before);
}
#[test]
fn collection_and_sibling_bundle_keep_their_correct_top_level_modules() {
    let dir = TestDir::new();
    make(&dir, "Main");
    make(&dir, "Options");
    fs::write(dir.0.join("notes.txt"), b"top notes").unwrap();
    let collection = request(
        &dir,
        json!({"dir":".","version":"release","out":"collection.zip"}),
    );
    let result = collection.run(&OperationControl::default()).unwrap();
    assert_eq!(result["layout"], "collection");
    assert_eq!(result["addon_count"], 2);
    assert_eq!(result["skipped_top_files"], json!(["notes.txt"]));
    assert_eq!(
        members(&dir.0.join("collection.zip")),
        vec![
            "Main/Main.toc",
            "Main/main.lua",
            "Options/Options.toc",
            "Options/main.lua"
        ]
    );
    fs::write(
        dir.0.join("Main/ncc.json"),
        br#"{"version":"bundle","bundle":["Options","Options","Main"],"ignore":[]}"#,
    )
    .unwrap();
    let bundle = request(&dir, json!({"dir":"Main","out":"bundle.zip"}));
    let result = bundle.run(&OperationControl::default()).unwrap();
    assert_eq!(result["layout"], "bundle");
    assert_eq!(result["addon_count"], 2);
    assert_eq!(
        members(&dir.0.join("bundle.zip")),
        members(&dir.0.join("collection.zip"))
    );
    fs::write(
        dir.0.join("Main/ncc.json"),
        br#"{"version":"bundle","bundle":["Options"],"ignore":["Options.toc"]}"#,
    )
    .unwrap();
    assert_eq!(
        bundle
            .plan(&OperationControl::default())
            .unwrap_err()
            .exit_code,
        4
    );
}
#[test]
fn selected_sources_stay_locked_and_bad_projects_fail_without_an_output() {
    let dir = TestDir::new();
    make(&dir, "Addon");
    let control = OperationControl::default();
    let selected = Package::select(&dir.0.join("Addon"), &control).unwrap();
    assert!(fs::write(dir.0.join("Addon/main.lua"), b"replacement").is_err());
    selected.finish(Ok(()), &control).unwrap();
    for manifest in [
        br#"{"version":"1","bundle":["../escape"]}"#.as_slice(),
        br#"{"version":"1","ignore":["*.toc"]}"#,
        br#"{"version":"1","version":"2"}"#,
    ] {
        fs::write(dir.0.join("Addon/ncc.json"), manifest).unwrap();
        let outcome = PluginPack::parse(
            br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin.pack","dir":"Addon"}"#,
            &dir.0,
            &control,
        )
        .and_then(|request| request.run(&control));
        assert!(outcome.is_err());
        assert!(!dir.0.join("Addon-1.zip").exists());
    }
    assert!(
        PluginPack::parse(
            br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin.pack","dir":null}"#,
            &dir.0,
            &control
        )
        .is_err()
    );
}
#[test]
fn reused_packages_reject_local_header_conflicts_dos_symlinks_and_bad_crc() {
    let dir = TestDir::new();
    make(&dir, "Addon");
    let request = request(&dir, json!({"dir":"Addon","version":"1"}));
    request.run(&OperationControl::default()).unwrap();
    let path = dir.0.join("Addon-1.zip");
    let original = fs::read(&path).unwrap();
    let central = original
        .windows(4)
        .position(|p| p == b"PK\x01\x02")
        .unwrap();
    let mut variants = Vec::new();
    let mut bytes = original.clone();
    bytes[30] ^= 1;
    variants.push(bytes);
    let mut bytes = original.clone();
    bytes[central + 5] = 0;
    bytes[central + 38..central + 42].copy_from_slice(&((0o120777u32) << 16).to_le_bytes());
    variants.push(bytes);
    let mut bytes = original.clone();
    let size = u16::from_le_bytes(bytes[28..30].try_into().unwrap()) as usize;
    let start = 30 + u16::from_le_bytes(bytes[26..28].try_into().unwrap()) as usize + size;
    bytes[start] ^= 0x40;
    variants.push(bytes);
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in [
        (
            "Addon/Addon.toc",
            b"## Interface: 120000, 11508\n".as_slice(),
        ),
        ("Addon/main.lua", b"print('original')"),
        ("Addon/", b"UNSELECTED_PRIVATE_DIRECTORY_PAYLOAD"),
    ] {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let mut bytes = writer.finish().unwrap().into_inner();
    let central = bytes.windows(4).rposition(|p| p == b"PK\x01\x02").unwrap();
    bytes[central + 38..central + 42].copy_from_slice(&((0o040755u32) << 16).to_le_bytes());
    variants.push(bytes);
    for bytes in variants {
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            request
                .run(&OperationControl::default())
                .unwrap_err()
                .exit_code,
            4
        );
    }
    fs::write(&path, &original).unwrap();
    assert_eq!(
        request.run(&OperationControl::default()).unwrap()["reused"],
        true
    );
}
#[test]
fn archive_interfaces_come_from_original_member_bytes_and_paths_remain_safe() {
    let dir = TestDir::new();
    make(&dir, "Addon");
    let request = request(&dir, json!({"dir":"Addon","version":"1"}));
    request.run(&OperationControl::default()).unwrap();
    let (mut file, _) =
        storage::locked_sha256(&request.output, MAX_ZIP, &OperationControl::default()).unwrap();
    let actual = inventory(&mut file, &OperationControl::default()).unwrap();
    assert_eq!(actual.interfaces, BTreeSet::from([11508, 120000]));
    storage::finish_file(file, Ok(()), &OperationControl::default()).unwrap();
    let path = dir.0.join("escape.zip");
    let mut writer = zip::ZipWriter::new(File::create(&path).unwrap());
    writer
        .start_file("../escape.lua", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"escape").unwrap();
    writer.finish().unwrap();
    let (mut file, _) =
        storage::locked_sha256(&path, MAX_ZIP, &OperationControl::default()).unwrap();
    assert_eq!(
        inventory(&mut file, &OperationControl::default())
            .err()
            .unwrap()
            .exit_code,
        4
    );
    storage::finish_file(file, Ok(()), &OperationControl::default()).unwrap();
}
