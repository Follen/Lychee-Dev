use super::*;
use crate::test_support::TestDir;
use std::io::Write;
fn write(root: &Path, relative: &str, bytes: &[u8]) {
    let p = root.join(relative);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}
fn request(root: &Path, output: &Path) -> Value {
    json!({"schema":"lycheedev.publish.v1.dd.backup.generate","game_dir":root,"game_type":10001,"output_dir":output,
        "addon_names":["Known"],"unknown_addons":["Extra"],"materials":["Textures"],"fonts":["FONT.ttf"],"include_wtf_root":true,
        "wtf_accounts":[{"account":"A","include_common":true,"roles":[{"server":"Server","name":"Chosen"}]}]})
}
fn fixture(root: &Path) {
    for (name, bytes) in [
        ("Interface/AddOns/Known/Main.lua", "known"),
        ("Interface/AddOns/Extra/Main.lua", "extra"),
        ("Interface/AddOns/NotChosen/Main.lua", "excluded"),
        ("Interface/Textures/texture.blp", "texture"),
        ("Fonts/FONT.ttf", "font"),
        ("WTF/Config.wtf", "root"),
        ("WTF/Account/A/SavedVariables/Common.lua", "common"),
        ("WTF/Account/A/Server/SavedVariables/Server.lua", "server"),
        (
            "WTF/Account/A/Server/Chosen/SavedVariables/Role.lua",
            "chosen",
        ),
        (
            "WTF/Account/A/Server/Other/SavedVariables/Role.lua",
            "excluded-role",
        ),
        (
            "WTF/Account/Other/Server/Chosen/Settings.wtf",
            "excluded-account",
        ),
    ] {
        write(root, name, bytes.as_bytes());
    }
}
#[test]
fn five_components_preserve_dd_paths_bytes_and_explicit_roles_and_reuse_the_original_bundle() {
    let temp = TestDir::new();
    let root = temp.0.join("game");
    fixture(&root);
    let output = temp.0.join("backup");
    let control = OperationControl::default();
    let scan = scan(&root, &control).unwrap();
    assert!(
        scan["wtf_accounts"][0]["roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "Chosen")
    );
    let input = request(&root, &output);
    let r = BackupGenerate::parse(&serde_json::to_vec(&input).unwrap(), &temp.0).unwrap();
    assert!(!output.exists());
    assert_eq!(r.plan()["local_writes"], false);
    let value = r.generate(&control).unwrap();
    assert_eq!(value["reused"], false);
    for (group, expected) in [
        ("known_addon", vec![("Known/Main.lua", "known")]),
        ("unknown_addon", vec![("Extra/Main.lua", "extra")]),
        ("material", vec![("Textures/texture.blp", "texture")]),
        ("font", vec![("Fonts/FONT.ttf", "font")]),
        (
            "wtf",
            vec![
                ("WTF/Config.wtf", "root"),
                ("WTF/Account/A/SavedVariables/Common.lua", "common"),
                ("WTF/Account/A/Server/SavedVariables/Server.lua", "server"),
                (
                    "WTF/Account/A/Server/Chosen/SavedVariables/Role.lua",
                    "chosen",
                ),
            ],
        ),
    ] {
        let file = File::open(output.join(format!("{group}.zip"))).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        assert_eq!(zip.len(), expected.len());
        for (name, bytes) in expected {
            let mut member = zip.by_name(name).unwrap();
            let mut actual = Vec::new();
            member.read_to_end(&mut actual).unwrap();
            assert_eq!(actual, bytes.as_bytes());
        }
    }
    let before = fs::read(output.join("backup-bundle.json")).unwrap();
    write(
        &root,
        "Interface/AddOns/Known/Main.lua",
        b"changed after original generation",
    );
    assert_eq!(r.generate(&control).unwrap()["reused"], true);
    assert_eq!(before, fs::read(output.join("backup-bundle.json")).unwrap());
    let other = temp.0.join("other-game");
    fixture(&other);
    let mut different = input;
    different["game_dir"] = json!(other);
    assert!(
        BackupGenerate::parse(&serde_json::to_vec(&different).unwrap(), &temp.0)
            .unwrap()
            .generate(&control)
            .is_err()
    );
}
#[test]
fn altered_archives_and_unselected_roles_are_rejected_even_with_updated_archive_hashes() {
    let temp = TestDir::new();
    let root = temp.0.join("game");
    fixture(&root);
    let output = temp.0.join("backup");
    let control = OperationControl::default();
    let r = BackupGenerate::parse(
        &serde_json::to_vec(&request(&root, &output)).unwrap(),
        &temp.0,
    )
    .unwrap();
    r.generate(&control).unwrap();
    let bundle = output.join("backup-bundle.json");
    let mut spec: Value = serde_json::from_slice(&fs::read(&bundle).unwrap()).unwrap();
    let archive = output.join("wtf.zip");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&archive)
        .unwrap();
    let mut zip = zip::ZipWriter::new_append(file).unwrap();
    zip.start_file(
        "WTF/Account/A/Server/Other/SavedVariables/Hidden.lua",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"unselected").unwrap();
    zip.finish().unwrap();
    assert!(Validated::open(&bundle, &control).is_err());
    let (mut file, sha) = storage::locked_sha256(&archive, u64::MAX, &control).unwrap();
    let digest = md5(&mut file, &control).unwrap();
    let size = file.metadata().unwrap().len();
    drop(file);
    let artifact = spec["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a["group"] == "wtf")
        .unwrap();
    artifact["sha256"] = json!(sha);
    artifact["md5"] = json!(digest);
    artifact["size"] = json!(size);
    artifact["files"] = json!(5);
    spec["manifest"]["wtf"]["md5"] = json!(digest);
    spec["manifest"]["wtf"]["size"] = json!(size);
    fs::write(&bundle, serde_json::to_vec(&spec).unwrap()).unwrap();
    let e = Validated::open(&bundle, &control).err().unwrap();
    assert!(e.message.contains("unselected"), "{}", e.message);
}
