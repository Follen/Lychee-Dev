use super::*;
use crate::{
    modus::{leveldb::tests::control, tests::api},
    test_support::{Reply, Server, TestDir},
};
use md5::Digest;
use std::io::Write;
const PROJECT: u64 = 200000719;
fn ok(value: Value) -> Reply {
    Reply::Json(json!({"code":200,"data":value}))
}
fn account() -> Reply {
    ok(json!({"user":{"userId":7}}))
}
fn projects() -> Reply {
    ok(json!({"total":1,"rows":[{"projectId":PROJECT}]}))
}
fn choices() -> Reply {
    ok(
        json!([{"key":"wow_builds","value":"{\"wow_retail\":{\"versions\":[\"12.1.5\",\"12.1.6\"]},\"wow_classic_era\":{\"versions\":[\"1.15.9\"]}}"}]),
    )
}
fn zip(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join("AuthorAddon.zip");
    let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
    for (name, data) in entries {
        zip.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    path
}
fn request(action: &str, mut doc: Value, cwd: &Path) -> PluginWrite {
    doc["schema"] = json!(format!("lycheedev.publish.v1.modus.plugin.{action}"));
    PluginWrite::parse(action, &serde_json::to_vec(&doc).unwrap(), cwd).unwrap()
}
fn execute(
    input: &PluginWrite,
    home: &Path,
    server: &Server,
    recover: Option<u64>,
) -> Result<Value> {
    let mut package = input.package(&control())?;
    let result = input.run(
        home,
        &api(server, Client::Creator),
        package.as_mut().map(|(f, v)| (f, &*v)),
        recover,
        &control(),
    );
    if let Some((file, _)) = package {
        storage::finish_file(file, result, &control())
    } else {
        result
    }
}
fn saved(path: &Path, file: u64) -> Value {
    let bytes = std::fs::read(path).unwrap();
    json!({"projectId":PROJECT,"fileId":file,"version":"1.0.0","type":"release","md5":hex::encode(md5::Md5::digest(&bytes)),"zipSize":bytes.len(),"unzipSize":24,"tocVersion":"120105","path":format!("modus/wow/addons/{PROJECT}/files/{file}.zip"),"supportedGameVersions":[{"gameVersion":"12.1.5","server":"wow_retail"}]})
}
#[test]
fn toc_flavors_libraries_and_dynamic_choices_preserve_real_client_coverage() {
    let d = TestDir::new();
    let c = control();
    let path = zip(
        &d.0,
        &[
            ("Addon/Addon_Mainline.toc", b"## Interface: 120105\n"),
            (
                "Addon/Addon_Vanilla.toc",
                b"## Interface: 11509\n## Title: \xe9\n",
            ),
            ("Addon/Libs/Library/Library.toc", b"## Interface: 999999\n"),
        ],
    );
    let (file, info) = package::open(&path, &c).unwrap();
    assert_eq!(info["toc_version"], "11509,120105");
    assert_eq!(
        info["derived_games"],
        json!([{"gameVersion":"1.15.9","server":"wow_classic_era"},{"gameVersion":"12.1.5","server":"wow_retail"}])
    );
    drop(file);
    let config = json!([{"key":"wow_builds","value":"{\"wow_retail\":{\"versions\":[\"12.1.5\",\"12.1.6\"]},\"wow_classic_era\":{\"versions\":[\"1.15.9\"]}}"}]);
    let games = package::select(&info["derived_games"], &config, None).unwrap();
    assert!(!games.to_string().contains("12.1.6"));
    assert_eq!(
        package::select(
            &info["derived_games"],
            &config,
            Some(&json!([{"game_version":"12.1.6","server":"wow_retail"}]))
        )
        .unwrap(),
        json!([{"gameVersion":"12.1.6","server":"wow_retail"}])
    );
    assert!(
        package::select(
            &json!([{"gameVersion":"12.1.5","server":"wow_retail"}]),
            &config,
            Some(&json!([{"game_version":"1.15.9","server":"wow_classic_era"}]))
        )
        .is_err()
    );
    let path = zip(
        &d.0,
        &[
            ("One/One.toc", b"## Interface: 120105\n"),
            ("Two/Two.toc", b"## Interface: 11509\n"),
        ],
    );
    assert!(package::open(&path, &c).is_err());
    let path = zip(&d.0, &[("One/One.toc", b"## Interface: 120000\n")]);
    assert!(package::open(&path, &c).is_err());
    let path = zip(&d.0, &[("One/One.toc", b"## Interface: 16001,30405\n")]);
    let (file, info) = package::open(&path, &c).unwrap();
    assert_eq!(
        info["derived_games"],
        json!([{"gameVersion":"1.60.1","server":"wow_forever"}])
    );
    drop(file);
}
#[test]
fn unknown_metadata_reads_original_release_then_uploads_once_and_verifies_cdn_bytes() {
    let d = TestDir::new();
    let path = zip(&d.0, &[("Addon/Addon.toc", b"## Interface: 120105\nabc")]);
    let input = request(
        "create",
        json!({"project_id":PROJECT,"file":path,"version":"1.0.0","type":"release"}),
        &d.0,
    );
    let server = Server::new(vec![
        account(),
        projects(),
        choices(),
        ok(json!(720)),
        Reply::Drop,
    ]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap_err().exit_code,
        6
    );
    let captured = server.finish();
    assert!(
        captured[3]
            .line
            .starts_with("GET /api/game/data/author/project/fileId/200000719 ")
    );
    assert!(
        captured[4]
            .line
            .starts_with("POST /api/game/data/author/project/upload ")
    );
    let body: Value = serde_json::from_slice(&captured[4].body).unwrap();
    assert!(body.get("fileId").is_none());
    assert_eq!(body["path"], "modus/wow/addons/200000719/files/720.zip");
    assert_eq!(
        body["supportedGameVersionsReqs"],
        json!([{"gameVersion":"12.1.5","server":"wow_retail"}])
    );
    let bytes = std::fs::read(&path).unwrap();
    let actual = saved(&path, 720);
    let server = Server::new(vec![
        account(),
        ok(actual.clone()),
        ok(json!({"signedUrl":"https://storage.modus.cool/object?signature=synthetic-private"})),
        Reply::Bytes(vec![]),
        ok(actual),
        Reply::Bytes(bytes.clone()),
    ]);
    let result = execute(&input, &d.0, &server, None).unwrap();
    assert_eq!(result["archive_bytes_verified"], true);
    assert_eq!(result["file_id"], 720);
    assert!(!result.to_string().contains("synthetic-private"));
    let captured = server.finish();
    assert!(captured[3].line.starts_with("PUT /object?signature="));
    assert_eq!(captured[3].body, bytes);
    assert!(
        captured[5]
            .line
            .starts_with("GET /modus/wow/addons/200000719/files/720.zip ")
    );
    assert!(!captured[5].headers.contains("authorization"));
    let server = Server::new(vec![account()]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap()["reused"],
        true
    );
    server.finish();
}
#[test]
fn uncertain_put_and_stale_cdn_do_not_repeat_metadata_signature_or_binary_upload() {
    let d = TestDir::new();
    let path = zip(&d.0, &[("Addon/Addon.toc", b"## Interface: 120105\nabc")]);
    let input = request(
        "upload",
        json!({"project_id":PROJECT,"file":path,"version":"1.0.0","type":"release"}),
        &d.0,
    );
    let server = Server::new(vec![
        account(),
        projects(),
        choices(),
        ok(json!(720)),
        ok(json!({})),
        ok(json!({"signedUrl":"https://storage.modus.cool/object?signature=synthetic-private"})),
        Reply::Drop,
    ]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap_err().exit_code,
        6
    );
    server.finish();
    let bytes = std::fs::read(&path).unwrap();
    let actual = saved(&path, 720);
    let server = Server::new(vec![
        account(),
        ok(actual.clone()),
        Reply::Bytes(vec![0; bytes.len()]),
    ]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap_err().exit_code,
        6
    );
    let captured = server.finish();
    assert!(captured.iter().all(|r| r.line.starts_with("GET ")));
    let server = Server::new(vec![account(), ok(actual), Reply::Bytes(bytes)]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap()["archive_bytes_verified"],
        true
    );
    assert!(server.finish().iter().all(|r| r.line.starts_with("GET ")));
}
#[test]
fn metadata_only_update_and_delete_keep_original_file_identity_and_omit_zip_work() {
    let d = TestDir::new();
    let input = request(
        "edit",
        json!({"project_id":PROJECT,"file_id":720,"changelog":null}),
        &d.0,
    );
    let current = json!({"projectId":PROJECT,"fileId":720,"version":"kept","type":"beta","changelog":"old","path":"original.zip"});
    let mut after = current.clone();
    after["changelog"] = Value::Null;
    let server = Server::new(vec![
        account(),
        projects(),
        ok(current),
        ok(json!({})),
        ok(after.clone()),
    ]);
    let result = execute(&input, &d.0, &server, None).unwrap();
    assert_eq!(result["archive_bytes_verified"], false);
    let captured = server.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&captured[3].body).unwrap(),
        json!({"projectId":PROJECT,"fileId":720,"changelog":"<null>"})
    );
    let delete = request(
        "delete",
        json!({"project_id":PROJECT,"file_id":720,"confirm":"DELETE"}),
        &d.0,
    );
    let server = Server::new(vec![account(), projects(), ok(after), Reply::Drop]);
    assert_eq!(
        execute(&delete, &d.0, &server, None).unwrap_err().exit_code,
        6
    );
    let captured = server.finish();
    assert!(
        captured[3]
            .line
            .starts_with("POST /api/game/data/author/project/delete ")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&captured[3].body).unwrap(),
        json!({"projectId":PROJECT,"fileId":720})
    );
    let server = Server::new(vec![account(), ok(json!({"total":0,"rows":[]}))]);
    assert_eq!(
        execute(&delete, &d.0, &server, None).unwrap()["deleted"],
        true
    );
    server.finish();
}
#[test]
fn unknown_allocation_is_not_repeated_and_unproved_recovery_stays_pending() {
    let d = TestDir::new();
    let path = zip(&d.0, &[("Addon/Addon.toc", b"## Interface: 120105\nabc")]);
    let input = request(
        "create",
        json!({"project_id":PROJECT,"file":path,"version":"1.0.0","type":"release"}),
        &d.0,
    );
    let server = Server::new(vec![account(), projects(), choices(), Reply::Drop]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap_err().details["submission"],
        "uncertain"
    );
    server.finish();
    let server = Server::new(vec![account()]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap_err().exit_code,
        6
    );
    server.finish();
    let mut wrong = saved(&path, 720);
    wrong["version"] = json!("different");
    let server = Server::new(vec![account(), ok(wrong)]);
    assert_eq!(
        execute(&input, &d.0, &server, Some(720))
            .unwrap_err()
            .exit_code,
        6
    );
    server.finish();
    assert!(resource_url("https://foreign.invalid/author.zip").is_err());
    assert!(resource_url("https://cdn.modus.cool/author.zip?signature=private").is_err());
}

#[test]
fn confirmed_metadata_or_put_rejections_retain_the_reserved_id_and_completed_stages() {
    let d = TestDir::new();
    let path = zip(&d.0, &[("Addon/Addon.toc", b"## Interface: 120105\nabc")]);
    let input = request(
        "create",
        json!({"project_id":PROJECT,"file":path,"version":"1.0.0","type":"release"}),
        &d.0,
    );
    let server = Server::new(vec![
        account(),
        projects(),
        choices(),
        ok(json!(720)),
        Reply::Json(json!({"code":403,"msg":"not allowed"})),
    ]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap_err().exit_code,
        6
    );
    server.finish();
    let server = Server::new(vec![
        account(),
        ok(json!({})),
        ok(json!({"signedUrl":"https://storage.modus.cool/object?signature=expired"})),
        Reply::Raw("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
    ]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap_err().exit_code,
        6
    );
    let requests = server.finish();
    assert!(
        requests[1]
            .line
            .starts_with("POST /api/game/data/author/project/upload ")
    );
    assert!(requests.iter().all(|r| !r.line.contains("/fileId/")));
    let server = Server::new(vec![
        account(),
        ok(json!({"signedUrl":"https://storage.modus.cool/object?signature=fresh"})),
        Reply::Bytes(vec![]),
        ok(saved(&path, 720)),
        Reply::Bytes(std::fs::read(&path).unwrap()),
    ]);
    assert_eq!(
        execute(&input, &d.0, &server, None).unwrap()["archive_bytes_verified"],
        true
    );
    let requests = server.finish();
    assert!(
        requests
            .iter()
            .all(|r| !r.line.starts_with("POST ") && !r.line.contains("/fileId/"))
    );
}
