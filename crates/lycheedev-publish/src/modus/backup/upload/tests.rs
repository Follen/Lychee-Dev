use super::*;
use crate::{
    modus::{leveldb::tests::control, tests::api},
    test_support::{Reply, Server, TestDir},
};
use md5::Digest;
const KEY: &str = "modus/backups/original-manifest.json";
fn ok(v: Value) -> Reply {
    Reply::Json(json!({"code":200,"data":v}))
}
fn account() -> Reply {
    ok(json!({"user":{"userId":7}}))
}
fn signature() -> Reply {
    ok(
        json!({"key":KEY,"signedUrl":"https://objects.example.test/original?signature=one-use-capability"}),
    )
}
fn fixture(d: &Path) -> BackupUpload {
    let mut sections = Vec::new();
    for key in super::super::local::SECTIONS {
        sections.push(json!({"key":key,"items":if key=="addons"{vec![json!({"id":"known:123","section":"addons","name":"KnownAddon","meta":{"projectId":123,"fileId":456,"isLocked":true,"icon":"icon","author":"Author"}})]}else{vec![]}}));
    }
    let manifest = json!({"schemaVersion":"config-backup.v2","backupId":"local-1","backupName":"Author backup","source":{"origin":"modus","gameId":0},"storageTargets":["local"],"sections":sections});
    fs::create_dir_all(d).unwrap();
    storage::atomic_json(&d.join("manifest.v2.json"), &manifest).unwrap();
    BackupUpload::parse(&serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.modus.backup.upload","manifest_file":d.join("manifest.v2.json"),"server_type":0})).unwrap(),d).unwrap()
}
fn expected() -> Value {
    let empty = r#"{"list":[],"zipPath":"","md5":"","size":0,"layoutVersion":1}"#;
    json!({"serverType":0,"backupName":"Author backup","knownAddons":r#"{"list":["KnownAddon"],"displayList":[{"name":"KnownAddon","icon":"icon","author":"Author","isLocked":true}],"fileIds":"456","projectIds":"123","projectids":"123"}"#,"knownAddonsNum":1,"modifiedNum":0,"modifiedCosInfo":empty,"unknownAddons":empty,"unknownAddonsNum":0,"materialNum":0,"materialCosInfo":empty,"wtfNum":0,"wtfAccounts":"[]","fonts":"[]","fontNum":0,"manifestPath":KEY})
}
fn detail() -> Value {
    let mut v = expected();
    v["id"] = json!(719);
    v
}
fn run(input: &BackupUpload, home: &Path, server: &Server) -> Result<Value> {
    let s = Snapshot::open(&input.path, input.build, &control())?;
    input.run(home, &api(server, Client::Main), &s, &control())
}
fn record(home: &Path) -> Value {
    let directory = home.join("publish/modus/records/backup-upload");
    let scope = fs::read_dir(directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    serde_json::from_slice(&fs::read(scope.join("active.json")).unwrap()).unwrap()
}
fn normalized(mut v: Value) -> Value {
    for key in [
        "knownAddons",
        "modifiedCosInfo",
        "unknownAddons",
        "materialCosInfo",
        "wtfAccounts",
        "fonts",
    ] {
        v[key] = serde_json::from_str(v[key].as_str().unwrap()).unwrap();
    }
    v
}
#[test]
fn native_object_storage_and_backup_registration_recover_original_missing_response() {
    let d = TestDir::new();
    let input = fixture(&d.0.join("backup"));
    let home = d.0.join("home");
    let plan = input.plan(&control()).unwrap();
    assert_eq!(plan["server_type"], 0);
    assert!(!home.exists());
    let server = Server::new(vec![
        account(),
        signature(),
        ok(json!({})),
        ok(json!({})),
        ok(json!([{"id":7}])),
        Reply::Drop,
    ]);
    assert_eq!(run(&input, &home, &server).unwrap_err().exit_code, 6);
    let requests = server.finish();
    assert!(
        requests[1]
            .line
            .starts_with("POST /api/game/data/file/getSignature?")
    );
    assert!(requests[1].body.is_empty());
    assert!(requests[1].line.contains("fileType=2"));
    let sign = storage::sha256(b"7local-1_manifest.v2.jsonmodus_upload_sign_key_2026");
    assert!(requests[1].line.contains(&sign));
    assert!(requests[2].line.starts_with("PUT /original?signature="));
    assert!(
        !requests[2]
            .headers
            .to_lowercase()
            .contains("authorization:")
    );
    let cloud: Value = serde_json::from_slice(&requests[2].body).unwrap();
    assert_eq!(cloud["storageTargets"], json!(["local", "cloud"]));
    assert!(cloud["source"].get("cloudBackupId").is_none());
    let registered: Value = serde_json::from_slice(&requests[3].body).unwrap();
    assert_eq!(
        registered["md5"],
        hex::encode(md5::Md5::digest(&requests[2].body))
    );
    assert_eq!(registered["fileType"], 2);
    assert_eq!(registered["cosStoreUrl"], KEY);
    assert_eq!(
        normalized(serde_json::from_slice(&requests[5].body).unwrap()),
        normalized(expected())
    );
    assert!(
        requests[5]
            .headers
            .to_lowercase()
            .contains("x-server-type: 0")
    );
    let receipt = record(&home);
    assert!(!receipt.to_string().contains("one-use-capability"));
    let artifact = receipt["evidence"]["objects"][0]["file"].as_str().unwrap();
    let bytes = fs::read(artifact).unwrap();
    let server = Server::new(vec![
        account(),
        ok(json!([{"id":7},{"id":719}])),
        ok(detail()),
        ok(detail()),
    ]);
    let result = run(&input, &home, &server).unwrap();
    assert_eq!(result["backup_id"], 719);
    assert_eq!(result["recovered"], true);
    assert_eq!(result["objects"][0]["bytes_verified"], false);
    assert!(server.finish().iter().all(|r| r.line.starts_with("GET ")));
    assert_eq!(fs::read(artifact).unwrap(), bytes);
    let linked: Value = serde_json::from_slice(&fs::read(&input.path).unwrap()).unwrap();
    assert_eq!(linked["source"]["cloudBackupId"], "719");
    let server = Server::new(vec![account()]);
    let done = run(&input, &home, &server).unwrap();
    assert_eq!(done["reused"], true);
    assert_eq!(server.finish().len(), 1);
}
#[test]
fn unknown_put_reads_original_bytes_then_registers_once_without_resigning_or_rebuilding() {
    let d = TestDir::new();
    let input = fixture(&d.0.join("backup"));
    let home = d.0.join("home");
    let server = Server::new(vec![account(), signature(), Reply::Drop]);
    assert_eq!(run(&input, &home, &server).unwrap_err().exit_code, 6);
    let original = server.finish().remove(2).body;
    let server = Server::new(vec![account(), Reply::Bytes(b"old-cdn-bytes".to_vec())]);
    assert_eq!(run(&input, &home, &server).unwrap_err().exit_code, 6);
    assert!(server.finish().iter().all(|r| r.line.starts_with("GET ")));
    assert_eq!(record(&home)["evidence"]["objects"][0]["stage"], "putting");
    let server = Server::new(vec![
        account(),
        Reply::Bytes(original.clone()),
        ok(json!({})),
        ok(json!([])),
        ok(json!({"id":719})),
        ok(json!([{"id":719}])),
        ok(detail()),
    ]);
    let result = run(&input, &home, &server).unwrap();
    assert_eq!(result["objects"][0]["bytes_verified"], true);
    let r = server.finish();
    assert_eq!(r.iter().filter(|r| r.line.starts_with("PUT ")).count(), 0);
    assert!(!r.iter().any(|r| r.line.contains("getSignature")));
    assert_eq!(
        r.iter()
            .filter(|r| r
                .line
                .starts_with("POST /api/game/data/file/upload/data/storage "))
            .count(),
        1
    );
}
#[test]
fn unknown_storage_receipt_remains_pending_and_confirmed_rejection_reuses_put() {
    for unknown in [true, false] {
        let d = TestDir::new();
        let input = fixture(&d.0.join("backup"));
        let home = d.0.join("home");
        let rejection = if unknown {
            Reply::Drop
        } else {
            Reply::Json(json!({"code":403}))
        };
        let server = Server::new(vec![account(), signature(), ok(json!({})), rejection]);
        assert_eq!(run(&input, &home, &server).unwrap_err().exit_code, 6);
        server.finish();
        if unknown {
            let server = Server::new(vec![account()]);
            let error = run(&input, &home, &server).unwrap_err();
            assert_eq!(error.details["objects"][0]["stage"], "registering");
            assert_eq!(server.finish().len(), 1);
        } else {
            let server = Server::new(vec![
                account(),
                ok(json!({})),
                ok(json!([])),
                ok(json!({"id":719})),
                ok(json!([{"id":719}])),
                ok(detail()),
            ]);
            assert_eq!(run(&input, &home, &server).unwrap()["verified"], true);
            let r = server.finish();
            assert!(
                !r.iter()
                    .any(|r| r.line.starts_with("PUT ") || r.line.contains("getSignature"))
            );
        }
    }
}
#[test]
fn cloud_readback_mismatch_and_changed_original_manifest_do_not_create_another_backup() {
    let d = TestDir::new();
    let input = fixture(&d.0.join("backup"));
    let home = d.0.join("home");
    let mut wrong = detail();
    wrong["knownAddonsNum"] = json!(2);
    let server = Server::new(vec![
        account(),
        signature(),
        ok(json!({})),
        ok(json!({})),
        ok(json!([])),
        ok(json!({"id":719})),
        ok(json!([{"id":719}])),
        ok(wrong),
    ]);
    assert_eq!(run(&input, &home, &server).unwrap_err().exit_code, 6);
    server.finish();
    let mut doc: Value = serde_json::from_slice(&fs::read(&input.path).unwrap()).unwrap();
    doc["backupName"] = json!("different input");
    storage::atomic_json(&input.path, &doc).unwrap();
    let server = Server::new(vec![account()]);
    assert_eq!(run(&input, &home, &server).unwrap_err().exit_code, 6);
    assert_eq!(server.finish().len(), 1);
}

#[test]
fn native_categories_keep_distinct_object_types_and_existing_equal_fonts_upload_once() {
    let d = TestDir::new();
    let input = fixture(&d.0.join("backup"));
    let root = input.path.parent().unwrap();
    let home = d.0.join("home");
    let mut doc: Value = serde_json::from_slice(&fs::read(&input.path).unwrap()).unwrap();
    let put = |relative: &str, bytes: &[u8]| {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    };
    let font_md5 = hex::encode(md5::Md5::digest(b"font"));
    for (key, name, relative, bytes) in [
        (
            "modifiedAddons",
            "Modified",
            "Interface/AddOns/Modified/Modified.lua",
            b"modified".as_slice(),
        ),
        (
            "unknownAddons",
            "Unknown",
            "Interface/AddOns/Unknown/Unknown.lua",
            b"unknown".as_slice(),
        ),
        (
            "materials",
            "Icons",
            "Interface/Icons/Icon.blp",
            b"icon".as_slice(),
        ),
    ] {
        put(relative, bytes);
        let target = relative.rsplit_once('/').unwrap().0;
        let section = doc["sections"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|s| s["key"] == key)
            .unwrap();
        section["items"] = json!([{"id":key,"section":key,"name":name,"targetRelativePath":target,"sizeBytes":bytes.len(),"meta":{}}]);
    }
    for name in ["One.otf", "Two.ttf"] {
        put(&format!("Fonts/{name}"), b"font");
    }
    let section = doc["sections"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["key"] == "fonts")
        .unwrap();
    section["items"] = json!([{"id":"font1","section":"fonts","name":"One.otf","targetRelativePath":"Fonts/One.otf","sizeBytes":4,"meta":{"md5":font_md5}},{"id":"font2","section":"fonts","name":"Two.ttf","targetRelativePath":"Fonts/Two.ttf","sizeBytes":4,"meta":{"md5":font_md5}}]);
    storage::atomic_json(&input.path, &doc).unwrap();
    let s = Snapshot::open(&input.path, input.build, &control()).unwrap();
    assert_eq!(
        s.groups.iter().map(|g| g.kind).collect::<Vec<_>>(),
        vec![8, 9, 3, 4]
    );
    assert_eq!(s.fonts.len(), 2);
    let evidence = json!({"objects":[{"group":"modifiedAddons","cosPath":"modified.zip","md5":"m","size":21},{"group":"unknownAddons","cosPath":"unknown.zip","md5":"u","size":22},{"group":"materials","cosPath":"materials.zip","md5":"i","size":23},{"group":"manifest","cosPath":KEY}]});
    let wire = normalized(body(&s, &evidence).unwrap());
    assert_eq!(
        wire["modifiedCosInfo"],
        json!({"list":["Modified"],"zipPath":"modified.zip","md5":"m","size":21,"layoutVersion":1})
    );
    assert_eq!(wire["unknownAddons"]["list"], json!(["Unknown"]));
    assert_eq!(wire["materialCosInfo"]["list"], json!(["Icons"]));
    assert_eq!(
        wire["fonts"],
        json!([{"name":"One.otf","md5":font_md5,"size":4},{"name":"Two.ttf","md5":font_md5,"size":4}])
    );
    // Reuse the server's existing content-addressed font while retaining both
    // user filenames. Other category ZIP layout is exercised by local selection.
    for section in doc["sections"].as_array_mut().unwrap() {
        if ["modifiedAddons", "unknownAddons", "materials"]
            .iter()
            .any(|k| section["key"] == *k)
        {
            section["items"] = json!([]);
        }
    }
    storage::atomic_json(&input.path, &doc).unwrap();
    let server = Server::new(vec![
        account(),
        ok(json!({"notExist":[]})),
        signature(),
        ok(json!({})),
        ok(json!({})),
        ok(json!([])),
        Reply::Drop,
    ]);
    assert_eq!(run(&input, &home, &server).unwrap_err().exit_code, 6);
    let requests = server.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[1].body).unwrap(),
        json!({"fontMd5":[font_md5]})
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("PUT "))
            .count(),
        1
    );
    let body = normalized(serde_json::from_slice(&requests[6].body).unwrap());
    assert_eq!(body["fontNum"], 2);
    assert_eq!(body["fonts"].as_array().unwrap().len(), 2);
}
