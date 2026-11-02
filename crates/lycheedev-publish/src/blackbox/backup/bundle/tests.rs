use super::*;
use crate::test_support::TestDir;
use std::{
    io::Write,
    time::{Duration, Instant},
};
fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn fixture(root: &Path) -> Value {
    let game = root.join("game");
    for (name, bytes) in [
        ("Interface/AddOns/Custom/Custom.toc", "toc"),
        (
            "Interface/AddOns/Custom/heyboxplayerinfo.lua",
            "private telemetry",
        ),
        ("Interface/Textures/a.blp", "texture"),
        ("Fonts/Font.TTF", "font"),
        ("WTF/Account/Account/config-cache.wtf", "account"),
        ("WTF/Account/Account/SavedVariables/common.lua", "common"),
        (
            "WTF/Account/Account/SavedVariables/heyboxplayerinfo.lua.bak",
            "telemetry",
        ),
        ("WTF/Account/Account/Realm/Hero/chat-cache.txt", "hero"),
        (
            "WTF/Account/Account/Realm/Hero/SavedVariables/a.lua",
            "selected",
        ),
        ("WTF/Account/Account/Realm/Other/addons.txt", "unselected"),
    ] {
        let p = game.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    json!({"schema":"lycheedev.publish.v1.blackbox.backup.generate","game_dir":game,"output_dir":root.join("output"),"strappid":"wowretail","known_addons":[{"ID":719,"Name":"Known","Strategy":3,"VersionID":9007199254740993u64}],"unknown_addons":["Custom"],"materials":["Textures"],"fonts":["Font.TTF"],"wtf_accounts":[{"name":"Account","characters":[{"realm":"Realm","player":"Hero"}]}],"strategies":[{"ID":719,"Strategy":3}]})
}
#[test]
fn native_bundle_preserves_exact_groups_characters_and_reuses_original_bytes() {
    let dir = TestDir::new();
    let c = control();
    let doc = fixture(&dir.0);
    let r = BackupGenerate::parse(&serde_json::to_vec(&doc).unwrap(), &dir.0).unwrap();
    let scan = scan(&dir.0.join("game"), "wowretail", &c).unwrap();
    assert_eq!(
        scan["wtf_accounts"][0]["characters"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(r.plan(&c).unwrap()["archive_count"], 4);
    assert!(!dir.0.join("output").exists());
    let result = r.generate(&c).unwrap();
    let p = Path::new(result["bundle_file"].as_str().unwrap());
    let b = Validated::open(p, &c).unwrap();
    assert_eq!(b.spec.archives.len(), 4);
    assert_eq!(
        b.spec.configuration["KnownAddons"][0]["VersionID"],
        9007199254740993u64
    );
    let expected = [
        vec!["Custom/Custom.toc"],
        vec!["Textures/a.blp"],
        vec!["Font.TTF"],
        vec![
            "Account/SavedVariables/common.lua",
            "Account/config-cache.wtf",
            "Account/Realm/Hero/SavedVariables/a.lua",
            "Account/Realm/Hero/chat-cache.txt",
        ],
    ];
    for (a, wanted) in b.spec.archives.iter().zip(expected) {
        let mut z =
            zip::ZipArchive::new(File::open(dir.0.join("output").join(&a.file)).unwrap()).unwrap();
        let mut names = Vec::new();
        for i in 0..z.len() {
            names.push(z.by_index(i).unwrap().name().to_owned());
        }
        let mut wanted = wanted.into_iter().map(str::to_owned).collect::<Vec<_>>();
        names.sort();
        wanted.sort();
        assert_eq!(names, wanted);
    }
    let original_sha = b.sha256.clone();
    let original_bytes = b
        .spec
        .archives
        .iter()
        .map(|a| a.sha256.clone())
        .collect::<Vec<_>>();
    b.finish(Ok(()), &c).unwrap();
    fs::write(dir.0.join("game/Fonts/Font.TTF"), "new game bytes").unwrap();
    assert_eq!(r.generate(&c).unwrap()["reused"], true);
    let b = Validated::open(p, &c).unwrap();
    assert_eq!(b.sha256, original_sha);
    assert_eq!(
        b.spec
            .archives
            .iter()
            .map(|a| a.sha256.clone())
            .collect::<Vec<_>>(),
        original_bytes
    );
    b.finish(Ok(()), &c).unwrap();
}
#[test]
fn rehashed_bundle_cannot_smuggle_an_unselected_character() {
    let dir = TestDir::new();
    let c = control();
    let doc = fixture(&dir.0);
    let r = BackupGenerate::parse(&serde_json::to_vec(&doc).unwrap(), &dir.0).unwrap();
    let result = r.generate(&c).unwrap();
    let path = Path::new(result["bundle_file"].as_str().unwrap());
    let mut spec: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let zip_path = dir.0.join("output/account_Account.zip");
    {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&zip_path)
            .unwrap();
        let mut zip = zip::ZipWriter::new_append(file).unwrap();
        zip.start_file(
            "Account/Realm/Other/addons.txt",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"rogue").unwrap();
        zip.finish().unwrap();
    }
    let (mut file, hash) = storage::locked_sha256(&zip_path, u64::MAX, &c).unwrap();
    let md5 = cos::md5(&mut file, &c).unwrap();
    let bytes = file.metadata().unwrap().len();
    storage::finish_file(file, Ok(()), &c).unwrap();
    spec["archives"][3]["sha256"] = json!(hash);
    spec["archives"][3]["md5"] = json!(md5);
    spec["archives"][3]["bytes"] = json!(bytes);
    spec["configuration"]["WTFList"][0]["MD5"] = json!(md5);
    spec["configuration"]["WTFList"][0]["Size"] = json!(bytes);
    spec["configuration"]["WTFList"][0]["Files"][0]["Size"] = json!(
        spec["configuration"]["WTFList"][0]["Files"][0]["Size"]
            .as_u64()
            .unwrap()
            + 5
    );
    spec["source_bytes"] = json!(spec["source_bytes"].as_u64().unwrap() + 5);
    fs::write(path, serde_json::to_vec(&spec).unwrap()).unwrap();
    assert!(
        matches!(Validated::open(path,&c),Err(e) if e.message.contains("unselected realm/character"))
    );
}
