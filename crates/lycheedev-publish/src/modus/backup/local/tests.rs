use super::*;
use crate::{modus::leveldb::tests::control, test_support::TestDir};
use std::io::Read;
fn put(root: &Path, path: &str, data: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}
fn request(action: &str, mut doc: Value, cwd: &Path) -> BackupLocal {
    doc["schema"] = json!(format!("lycheedev.publish.v1.modus.backup.{action}"));
    BackupLocal::parse(action, &serde_json::to_vec(&doc).unwrap(), cwd).unwrap()
}
#[test]
fn explicit_native_selection_reuses_manifest_and_keeps_other_characters_out_of_wtf_zip() {
    let d = TestDir::new();
    let root = d.0.join("Game");
    let out = d.0.join("Backup");
    let c = control();
    for (path, bytes) in [
        ("Interface/AddOns/One/One.toc", "## Interface: 120105"),
        ("Interface/AddOns/Two/private.lua", "unselected-addon"),
        ("Interface/Icons/custom.blp", "material"),
        ("Fonts/Custom.otf", "font"),
        ("WTF/Config.wtf", "config"),
        ("WTF/Account/ACC/SavedVariables/Shared.lua", "shared"),
        ("WTF/Account/ACC/SavedVariables/BigFootSync.lua", "excluded"),
        (
            "WTF/Account/ACC/Realm/Selected/config-cache.wtf",
            "selected-role",
        ),
        (
            "WTF/Account/ACC/Realm/Selected/settings.lua.bak",
            "excluded-backup",
        ),
        (
            "WTF/Account/ACC/Realm/Other/config-cache.wtf",
            "private-other-role",
        ),
    ] {
        put(&root, path, bytes.as_bytes());
    }
    let scan = request("scan", json!({"game_root":root}), &d.0)
        .plan(&c)
        .unwrap();
    assert_eq!(scan["font_names"], json!(["Custom.otf"]));
    assert_eq!(scan["roles"].as_array().unwrap().len(), 2);
    assert_eq!(scan["credential_reads"], false);
    let doc = json!({"game_root":root,"output_dir":out,"backup_name":" Author backup ","server_type":0,"addon_names":["One"],"font_names":["Custom.otf"],"material_names":["Icons"],"roles":[{"account":"ACC","server":"Realm","role":"Selected"}]});
    let input = request("generate", doc.clone(), &d.0);
    input.plan(&c).unwrap();
    assert!(!out.exists());
    let first = input.execute(&d.0.join("home"), &c).unwrap();
    assert_eq!(first["generated"], true);
    assert_eq!(first["server_type"], 0);
    assert!(!out.join("Interface/AddOns/Two").exists());
    assert!(!out.join("WTF/Account/ACC/Realm/Other").exists());
    assert!(
        !out.join("WTF/Account/ACC/SavedVariables/BigFootSync.lua")
            .exists()
    );
    assert!(
        !out.join("WTF/Account/ACC/Realm/Selected/settings.lua.bak")
            .exists()
    );
    let before = fs::read(out.join("manifest.v2.json")).unwrap();
    put(&root, "Interface/AddOns/One/One.toc", b"new-game-file");
    let reused = request("create", doc, &d.0)
        .generate(&input.root(&c).unwrap(), &c)
        .unwrap();
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["local_backup_id"], first["local_backup_id"]);
    assert_eq!(fs::read(out.join("manifest.v2.json")).unwrap(), before);
    assert_ne!(
        fs::read(out.join("Interface/AddOns/One/One.toc")).unwrap(),
        b"new-game-file"
    );
    let snapshot =
        super::super::manifest::Snapshot::open(&out.join("manifest.v2.json"), Some(0), &c).unwrap();
    assert_eq!(snapshot.manifest["schemaVersion"], "config-backup.v2");
    assert_eq!(snapshot.groups.len(), 4);
    let group = snapshot.groups.iter().find(|g| g.kind == 7).unwrap();
    let zip = d.0.join("selected.zip");
    local::archive_entries(&out, &group.entries, &zip, &c).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(zip).unwrap()).unwrap();
    let mut actual = BTreeMap::new();
    for i in 0..zip.len() {
        let mut item = zip.by_index(i).unwrap();
        let mut bytes = String::new();
        item.read_to_string(&mut bytes).unwrap();
        actual.insert(item.name().to_owned(), bytes);
    }
    assert_eq!(
        actual,
        BTreeMap::from([
            (
                "ACC/Realm/Selected/config-cache.wtf".into(),
                "selected-role".into()
            ),
            ("ACC/SavedVariables/Shared.lua".into(), "shared".into()),
            ("Config.wtf".into(), "config".into())
        ])
    );
    let mut cloud = snapshot.cloud_manifest();
    assert!(!cloud.to_string().contains("gameRoot"));
    assert_eq!(cloud["storageTargets"], json!(["local", "cloud"]));
    cloud["source"]["cloudBackupId"] = json!("719");
    snapshot.link(719, &c).unwrap();
    let linked =
        super::super::manifest::Snapshot::open(&out.join("manifest.v2.json"), Some(0), &c).unwrap();
    assert_eq!(linked.identity(), snapshot.identity());
    put(
        &out,
        "WTF/Account/ACC/Realm/Other/config-cache.wtf",
        b"unexpected-other-role",
    );
    let e = super::super::manifest::Snapshot::open(&out.join("manifest.v2.json"), Some(0), &c)
        .err()
        .unwrap();
    assert_eq!(e.details["path"], "$.manifest_file");
}
#[test]
fn local_admission_rejects_wrong_paths_duplicate_roles_and_different_build_before_writes() {
    let d = TestDir::new();
    let root = d.0.join("Game");
    put(&root, "Interface/AddOns/One/One.toc", b"one");
    let c = control();
    let base = json!({"schema":"lycheedev.publish.v1.modus.backup.generate","game_root":root,"output_dir":d.0.join("Backup"),"backup_name":"backup","server_type":0,"addon_names":["One"]});
    for (key, value) in [
        ("font_names", json!(["../Private.otf"])),
        ("addon_names", json!(["One", "one"])),
        (
            "roles",
            json!([{"account":"A","server":"R","role":"P"},{"account":"a","server":"r","role":"p"}]),
        ),
        (
            "roles",
            json!([{"account":"A","server":"R","role":"P","all":true}]),
        ),
        ("material_names", json!(["AddOns"])),
        ("backup_name", json!(" ")),
    ] {
        let mut doc = base.clone();
        doc[key] = value;
        assert!(BackupLocal::parse("generate", &serde_json::to_vec(&doc).unwrap(), &d.0).is_err());
    }
    let mut inside = base.clone();
    inside["output_dir"] = json!(root.join("Backup"));
    assert!(
        BackupLocal::parse("generate", &serde_json::to_vec(&inside).unwrap(), &d.0)
            .unwrap()
            .plan(&c)
            .is_err()
    );
    let input = BackupLocal::parse("generate", &serde_json::to_vec(&base).unwrap(), &d.0).unwrap();
    input.execute(&d.0.join("home"), &c).unwrap();
    assert!(
        super::super::manifest::Snapshot::open(&d.0.join("Backup/manifest.v2.json"), Some(3), &c)
            .is_err()
    );
    let mut different = base;
    different["backup_name"] = json!("different");
    assert!(
        BackupLocal::parse("generate", &serde_json::to_vec(&different).unwrap(), &d.0)
            .unwrap()
            .plan(&c)
            .is_err()
    );
    fs::remove_file(d.0.join("Backup/Interface/AddOns/One/One.toc")).unwrap();
    assert!(input.plan(&c).is_err());
}
