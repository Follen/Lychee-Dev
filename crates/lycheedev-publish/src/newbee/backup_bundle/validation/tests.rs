use super::*;
use crate::{
    newbee::backup_bundle::tests::{doc, fixture},
    test_support::TestDir,
};
fn generated(dir: &TestDir, label: &str) -> (PathBuf, Value) {
    let root = fixture(dir);
    let output = dir.0.join(label);
    BackupGenerate::parse(&serde_json::to_vec(&doc(&root, &output)).unwrap(), &dir.0)
        .unwrap()
        .generate(&OperationControl::default())
        .unwrap();
    let path = output.join("backup-bundle.json");
    let value = crate::input::value(&fs::read(&path).unwrap()).unwrap();
    (path, value)
}
fn save(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn identity(path: &Path, value: &mut Value, index: usize) {
    let artifact = path
        .parent()
        .unwrap()
        .join(value["artifacts"][index]["file"].as_str().unwrap());
    let bytes = fs::read(&artifact).unwrap();
    value["artifacts"][index]["sha256"] = json!(storage::sha256(&bytes));
    value["artifacts"][index]["size"] = json!(bytes.len());
    let mut archive = zip::ZipArchive::new(File::open(artifact).unwrap()).unwrap();
    let files = (0..archive.len())
        .filter(|index| !archive.by_index(*index).unwrap().is_dir())
        .count();
    value["artifacts"][index]["files"] = json!(files);
}
fn rejected(path: &Path, value: &Value) {
    save(path, value);
    let error = ValidatedBundle::open(path, &OperationControl::default())
        .err()
        .unwrap();
    assert_eq!(error.exit_code, 4, "{error:?}");
}
#[test]
fn admission_binds_and_holds_the_original_manifest_archives_and_font_bytes() {
    let dir = TestDir::new();
    let (path, value) = generated(&dir, "valid");
    let bundle = ValidatedBundle::open(&path, &OperationControl::default()).unwrap();
    assert_eq!(bundle.version.get(), 2);
    assert_eq!(bundle.artifacts.len(), 4);
    assert_eq!(bundle.fonts.len(), 1);
    for held in [
        &path,
        &path
            .parent()
            .unwrap()
            .join(value["artifacts"][0]["file"].as_str().unwrap()),
        &path.parent().unwrap().join("Fonts/one.ttf"),
    ] {
        assert!(OpenOptions::new().write(true).open(held).is_err());
    }
    bundle.finish(Ok(()), &OperationControl::default()).unwrap();
    assert!(OpenOptions::new().write(true).open(&path).is_ok());
}
#[test]
fn self_consistent_archive_hashes_cannot_authorize_unselected_components_or_roles() {
    let dir = TestDir::new();
    for (index, member) in [
        (0, "Two/Two.lua"),
        (1, "Other/other.tga"),
        (2, "Other/"),
        (3, "Other/AddOns.txt"),
    ] {
        let (path, mut value) = generated(&dir, &format!("extra-{index}"));
        let artifact = path
            .parent()
            .unwrap()
            .join(value["artifacts"][index]["file"].as_str().unwrap());
        let mut writer = zip::ZipWriter::new_append(
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(artifact)
                .unwrap(),
        )
        .unwrap();
        let options = zip::write::SimpleFileOptions::default();
        if member.ends_with('/') {
            writer.add_directory(member, options).unwrap();
        } else {
            writer.start_file(member, options).unwrap();
            writer.write_all(b"unselected data").unwrap();
        }
        writer.finish().unwrap();
        identity(&path, &mut value, index);
        rejected(&path, &value);
    }
    let (path, mut value) = generated(&dir, "changed-role");
    value["manifest"]["wtf"][0]["server"][0]["roleList"][0]["name"] = json!("Other");
    rejected(&path, &value);
}
#[test]
fn windows_paths_local_header_identity_link_attributes_and_crc_are_checked() {
    let dir = TestDir::new();
    for variant in [
        "local-name",
        "both-names",
        "case-duplicate",
        "unix-link",
        "dos-link",
        "reparse",
        "crc",
    ] {
        let (path, mut value) = generated(&dir, variant);
        let archive_path = path
            .parent()
            .unwrap()
            .join(value["artifacts"][3]["file"].as_str().unwrap());
        if variant == "case-duplicate" {
            let mut zip = zip::ZipWriter::new_append(
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&archive_path)
                    .unwrap(),
            )
            .unwrap();
            zip.start_file(
                "Chosen/addons.txt",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"ambiguous").unwrap();
            zip.finish().unwrap();
        } else {
            let mut zip = zip::ZipArchive::new(File::open(&archive_path).unwrap()).unwrap();
            let member = zip.by_index(0).unwrap();
            let central = member.central_header_start() as usize;
            let payload = member.data_start() as usize;
            drop(member);
            drop(zip);
            let mut bytes = fs::read(&archive_path).unwrap();
            match variant {
                "local-name" | "both-names" => {
                    let original = b"Chosen/AddOns.txt";
                    let at = bytes
                        .windows(original.len())
                        .position(|part| part == original)
                        .unwrap();
                    bytes[at + 6] = b'\\';
                    if variant == "both-names" {
                        let at = bytes
                            .windows(original.len())
                            .position(|part| part == original)
                            .unwrap();
                        bytes[at + 6] = b'\\';
                    }
                }
                "unix-link" | "dos-link" => {
                    bytes[central + 38..central + 42]
                        .copy_from_slice(&(0o120777u32 << 16).to_le_bytes());
                    if variant == "dos-link" {
                        bytes[central + 5] = 0;
                    }
                }
                "reparse" => {
                    let flags =
                        u32::from_le_bytes(bytes[central + 38..central + 42].try_into().unwrap())
                            | 0x400;
                    bytes[central + 38..central + 42].copy_from_slice(&flags.to_le_bytes());
                }
                "crc" => bytes[payload] ^= 0xff,
                _ => unreachable!(),
            }
            fs::write(&archive_path, bytes).unwrap();
        }
        identity(&path, &mut value, 3);
        rejected(&path, &value);
    }
}
#[test]
fn bundle_selection_files_targets_and_font_directory_must_all_agree() {
    let dir = TestDir::new();
    for variant in [
        "hash",
        "file-count",
        "missing-role",
        "target-object",
        "extra-font",
        "font-bytes",
        "pending-reference",
    ] {
        let (path, mut value) = generated(&dir, variant);
        match variant {
            "hash" => value["artifacts"][0]["sha256"] = json!("0".repeat(64)),
            "file-count" => value["artifacts"][3]["files"] = json!(99),
            "missing-role" => {
                value["artifacts"].as_array_mut().unwrap().remove(3);
            }
            "target-object" => {
                value["artifacts"][3]["target"][1] = json!({"$serde_json::private::Number":"0"})
            }
            "extra-font" => fs::write(
                path.parent().unwrap().join("Fonts/two.ttf"),
                b"not selected",
            )
            .unwrap(),
            "font-bytes" => fs::write(
                path.parent().unwrap().join("Fonts/one.ttf"),
                b"changed bytes",
            )
            .unwrap(),
            "pending-reference" => {
                value["manifest"]["unknown_plug"] = json!("cloud/already-uploaded.zip")
            }
            _ => unreachable!(),
        }
        rejected(&path, &value);
    }
}
