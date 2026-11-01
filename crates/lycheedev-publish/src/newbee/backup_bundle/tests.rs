use super::*;
use crate::test_support::TestDir;
pub(in crate::newbee) fn fixture(dir: &TestDir) -> PathBuf {
    let root = dir.0.join("game");
    for (path, bytes) in [
        ("Interface/AddOns/One/One.lua", b"selected-addon".as_slice()),
        ("Interface/AddOns/Two/Two.lua", b"unselected-addon"),
        ("Interface/Textures/ui.tga", b"selected-texture"),
        ("Interface/Other/other.tga", b"unselected-texture"),
        ("Fonts/one.ttf", b"selected-font"),
        ("Fonts/two.ttf", b"unselected-font"),
        ("WTF/Account/A/config.wtf", b"common"),
        ("WTF/Account/A/SavedVariables/account.lua", b"common-vars"),
        ("WTF/Account/A/config.bak", b"excluded-backup"),
        ("WTF/Account/A/Realm/Chosen/AddOns.txt", b"chosen-marker"),
        (
            "WTF/Account/A/Realm/Chosen/SavedVariables/role.lua",
            b"chosen-role",
        ),
        (
            "WTF/Account/A/Realm/Chosen/old.bak",
            b"excluded-role-backup",
        ),
        ("WTF/Account/A/Realm/Other/AddOns.txt", b"unselected-marker"),
        (
            "WTF/Account/A/Realm/Other/secret.wtf",
            b"unselected-role-secret",
        ),
        (
            "WTF/Account/B/Realm/Other/AddOns.txt",
            b"unselected-account",
        ),
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    root
}
pub(in crate::newbee) fn doc(root: &Path, output: &Path) -> Value {
    json!({"schema":"lycheedev.publish.v1.newbee.backup.generate","game_dir":root,"game_version_id":2,"output_dir":output,
    "known_plugins":[{"name":"Remote identity","id":9007199254740993u64,"fileName":null,"fileUrl":null,"mod_file_id":991,"mod_version":"1.0","display_name":null,"updateType":1}],
    "unknown_plugins":["One"],"materials":["Textures"],"fonts":["one.ttf"],"wtf_accounts":[{"account":"A","include_common":true,"roles":[{"server":"Realm","name":"Chosen"}]}]})
}
fn members(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut result = Vec::new();
    for index in 0..archive.len() {
        let mut item = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut item, &mut bytes).unwrap();
        result.push((item.name().to_owned(), bytes));
    }
    result
}
#[test]
fn scan_reports_candidates_without_inventing_platform_identities() {
    let dir = TestDir::new();
    let root = fixture(&dir);
    let result = scan_backup(&root, &OperationControl::default()).unwrap();
    assert_eq!(result["automatic_addon_identification"], "not_performed");
    assert_eq!(result["coverage"], "complete");
    assert_eq!(
        result["unknown_plugins"],
        json!([{"name":"One"},{"name":"Two"}])
    );
    assert_eq!(
        result["wtf_accounts"][0]["roles"],
        json!([{"server":"Realm","name":"Chosen"},{"server":"Realm","name":"Other"}])
    );
    assert!(
        result["unknown_plugins"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.get("id").is_none())
    );
}
#[test]
fn selected_empty_component_directories_remain_present_in_the_generated_and_admitted_archive() {
    let dir = TestDir::new();
    let root = fixture(&dir);
    fs::create_dir_all(root.join("Interface/AddOns/Empty")).unwrap();
    let output = dir.0.join("empty-root");
    let mut input = doc(&root, &output);
    input["unknown_plugins"] = json!(["One", "Empty"]);
    BackupGenerate::parse(&serde_json::to_vec(&input).unwrap(), &dir.0)
        .unwrap()
        .generate(&OperationControl::default())
        .unwrap();
    assert!(
        members(&output.join("unknown_plug.zip"))
            .iter()
            .any(|(name, bytes)| name == "Empty/" && bytes.is_empty())
    );
    let bundle = validation::ValidatedBundle::open(
        &output.join("backup-bundle.json"),
        &OperationControl::default(),
    )
    .unwrap();
    bundle.finish(Ok(()), &OperationControl::default()).unwrap();
}
#[test]
fn generated_archives_include_only_selected_roots_roles_common_files_and_fonts() {
    let dir = TestDir::new();
    let root = fixture(&dir);
    let output = dir.0.join("bundle");
    let request =
        BackupGenerate::parse(&serde_json::to_vec(&doc(&root, &output)).unwrap(), &dir.0).unwrap();
    let result = request.generate(&OperationControl::default()).unwrap();
    assert_eq!(result["remote_writes"], 0);
    let bundle =
        crate::input::value(&fs::read(output.join("backup-bundle.json")).unwrap()).unwrap();
    assert_eq!(bundle["schema"], BUNDLE_SCHEMA);
    assert_eq!(
        bundle["manifest"]["knowplug_list"][0]["id"],
        9007199254740993u64
    );
    assert_eq!(
        bundle["manifest"]["wtf"][0]["server"][0]["roleList"],
        json!([{"name":"Chosen","zip":""}])
    );
    assert_eq!(bundle["manifest"]["t_WTF_list"], json!([]));
    assert_eq!(
        members(&output.join("unknown_plug.zip")),
        vec![("One/One.lua".into(), b"selected-addon".to_vec())]
    );
    assert_eq!(
        members(&output.join("material.zip")),
        vec![("Textures/ui.tga".into(), b"selected-texture".to_vec())]
    );
    let common = members(&output.join("wtf-0-common.zip"));
    assert_eq!(
        common
            .iter()
            .map(|row| row.0.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["config.wtf", "SavedVariables/account.lua"])
    );
    let role = members(&output.join("wtf-0-role-0.zip"));
    assert_eq!(
        role,
        vec![
            ("Chosen/AddOns.txt".into(), b"chosen-marker".to_vec()),
            (
                "Chosen/SavedVariables/role.lua".into(),
                b"chosen-role".to_vec()
            )
        ]
    );
    assert_eq!(
        fs::read(output.join("Fonts/one.ttf")).unwrap(),
        b"selected-font"
    );
    assert!(!output.join("Fonts/two.ttf").exists());
    assert_eq!(bundle["artifacts"].as_array().unwrap().len(), 4);
    assert_eq!(
        bundle["artifacts"][3]["target"],
        json!(["wtf", 0, "server", 0, "roleList", 0, "zip"])
    );
    for member in common.iter().chain(&role) {
        assert!(!member.0.ends_with(".bak"));
        assert!(!String::from_utf8_lossy(&member.1).contains("unselected"));
    }
    let second = request.generate(&OperationControl::default()).unwrap_err();
    assert_eq!(second.exit_code, 4);
    assert_eq!(
        fs::read(output.join("Fonts/one.ttf")).unwrap(),
        b"selected-font"
    );
}
#[test]
fn absent_or_unsafe_nested_selections_fail_before_creating_output() {
    let dir = TestDir::new();
    let root = fixture(&dir);
    for (index, field, value) in [
        (0, "unknown_plugins", json!(["Absent"])),
        (1, "materials", json!(["../Other"])),
        (2, "fonts", json!(["CON.ttf"])),
        (
            3,
            "wtf_accounts",
            json!([{"account":"A","include_common":false,"roles":[{"server":"Realm","name":"Absent"}]}]),
        ),
    ] {
        let output = dir.0.join(format!("bad-{index}"));
        let mut value_doc = doc(&root, &output);
        value_doc[field] = value;
        let result = BackupGenerate::parse(&serde_json::to_vec(&value_doc).unwrap(), &dir.0)
            .and_then(|request| request.generate(&OperationControl::default()));
        assert_eq!(result.unwrap_err().exit_code, 4);
        assert!(!output.exists());
    }
    let mut missing = doc(&root, &dir.0.join("missing"));
    missing["known_plugins"][0]
        .as_object_mut()
        .unwrap()
        .remove("fileUrl");
    assert!(BackupGenerate::parse(&serde_json::to_vec(&missing).unwrap(), &dir.0).is_err());
    let mut extra = doc(&root, &dir.0.join("extra"));
    extra["wtf_accounts"][0]["extra"] = json!(true);
    assert!(BackupGenerate::parse(&serde_json::to_vec(&extra).unwrap(), &dir.0).is_err());
}
#[test]
fn output_cannot_be_inside_the_game_or_overwrite_an_existing_directory() {
    let dir = TestDir::new();
    let root = fixture(&dir);
    let inside = doc(&root, &root.join("output"));
    assert_eq!(
        BackupGenerate::parse(&serde_json::to_vec(&inside).unwrap(), &dir.0)
            .err()
            .unwrap()
            .exit_code,
        4
    );
    let output = dir.0.join("occupied");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("keep"), b"original").unwrap();
    let occupied = doc(&root, &output);
    assert!(BackupGenerate::parse(&serde_json::to_vec(&occupied).unwrap(), &dir.0).is_err());
    assert_eq!(fs::read(output.join("keep")).unwrap(), b"original");
}
