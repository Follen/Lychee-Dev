use super::super::tests::{client, reply};
use super::*;
use crate::test_support::{Reply, Server, TestDir};
use std::io::Write;
fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn detail(bytes: &[u8]) -> Reply {
    reply(
        json!({"mod_id":91,"name":"Online","description":"<p>PRIVATE_DESCRIPTION</p>","content_format":1,"latest_versions":["12.0.0"],"latest_file":{"version_name":"v2","file_size":bytes.len(),"download_url":"https://cdn.invalid/versions/v2.zip?Signature=PRIVATE_SIGNED_URL"}}),
    )
}
fn request(dir: &TestDir, init: bool, fields: Value) -> PluginWorkspace {
    let mut value = fields;
    value["schema"] = json!(format!(
        "lycheedev.publish.v1.newbee.official.plugin.{}",
        if init { "init" } else { "sync" }
    ));
    PluginWorkspace::parse(
        init,
        &serde_json::to_vec(&value).unwrap(),
        &dir.0,
        &OperationControl::default(),
    )
    .unwrap()
}
fn project(dir: &TestDir) -> PathBuf {
    let root = dir.0.join("Working");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("Working.toc"), b"## Interface: 120000\n").unwrap();
    fs::write(root.join("main.lua"), b"old code").unwrap();
    fs::write(root.join("ncc.json"),br#"{"game":"wow","type":"mod","id":91,"version":"local-next","game_versions":["1.15.8"],"ignore":["*.psd"],"custom":{"keep":true}}"#).unwrap();
    root
}
fn no_stages(dir: &TestDir) {
    assert!(!fs::read_dir(&dir.0).unwrap().any(|v| {
        v.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lycheedev-creator-")
    }));
}
#[test]
fn first_plugin_association_needs_no_existing_uploaded_file_and_preserves_local_code() {
    let dir = TestDir::new();
    let root = project(&dir);
    fs::remove_file(root.join("ncc.json")).unwrap();
    let request = request(
        &dir,
        true,
        json!({"id":9007199254740993u64,"dir":"Working"}),
    );
    assert_eq!(
        request.plan(&OperationControl::default()).unwrap()["clones_latest"],
        false
    );
    let before = fs::read(root.join("main.lua")).unwrap();
    let server = Server::new(vec![reply(
        json!({"mod_id":9007199254740993u64,"name":"New Plugin"}),
    )]);
    let result = client(&dir, &server).plugin_workspace(&request).unwrap();
    assert_eq!(result["cloned"], false);
    assert_eq!(result["game_versions_coverage"], "not_available");
    assert_eq!(fs::read(root.join("main.lua")).unwrap(), before);
    let manifest = fs::read_to_string(root.join("ncc.json")).unwrap();
    assert!(manifest.contains("9007199254740993"));
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["version"], "");
    assert_eq!(manifest["game_versions"], json!([]));
    assert_eq!(server.finish().len(), 1);
    no_stages(&dir);
}
#[test]
fn cloning_streams_the_original_archive_without_author_headers_and_keeps_description_format() {
    let dir = TestDir::new();
    let request = request(&dir, true, json!({"id":91,"dir":"new/project"}));
    assert_eq!(
        request.plan(&OperationControl::default()).unwrap()["local_writes"],
        0
    );
    assert!(!dir.0.join("new").exists());
    let bytes = archive(&[
        ("Addon/Addon.toc", b"## Interface: 120000\n"),
        ("Addon/main.lua", b"new code"),
        ("README.txt", b"top-level online notes"),
    ]);
    let server = Server::new(vec![detail(&bytes), Reply::Bytes(bytes)]);
    let result = client(&dir, &server).plugin_workspace(&request).unwrap();
    assert_eq!(result["cloned"], true);
    assert_eq!(result["file_count"], 3);
    assert!(!result.to_string().contains("PRIVATE_"));
    let root = dir.0.join("new/project");
    assert_eq!(fs::read(root.join("Addon/main.lua")).unwrap(), b"new code");
    assert_eq!(
        fs::read(root.join("README.txt")).unwrap(),
        b"top-level online notes"
    );
    assert_eq!(
        fs::read_to_string(root.join("description.html")).unwrap(),
        "<p>PRIVATE_DESCRIPTION</p>"
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("ncc.json")).unwrap()).unwrap();
    assert_eq!(manifest["id"], 91);
    assert_eq!(manifest["game_versions"], json!(["12.0.0"]));
    let requests = server.finish();
    assert!(
        requests[0]
            .headers
            .contains("authorization: Bearer ncc_SYNTHETIC_CREDENTIAL")
    );
    assert!(
        requests[1]
            .line
            .starts_with("GET /versions/v2.zip?Signature=PRIVATE_SIGNED_URL ")
    );
    assert!(!requests[1].headers.contains("authorization:"));
    assert!(!requests[1].headers.contains("x-ncc-cli-version:"));
    no_stages(&dir);
}
#[test]
fn single_sync_preserves_ignored_metadata_and_reuses_unchanged_files_while_removing_obsolete_content()
 {
    let dir = TestDir::new();
    let root = project(&dir);
    fs::write(root.join("obsolete.lua"), b"obsolete").unwrap();
    fs::write(root.join("art.psd"), b"preserve material").unwrap();
    fs::write(root.join(".private"), b"preserve hidden").unwrap();
    let (held, _) = storage::locked_sha256(
        &root.join("Working.toc"),
        u64::MAX,
        &OperationControl::default(),
    )
    .unwrap();
    let request = request(&dir, false, json!({"dir":"Working","confirm":true}));
    let bytes = archive(&[
        ("Remote/Working.toc", b"## Interface: 120000\n"),
        ("Remote/main.lua", b"new code"),
        ("Remote/added.lua", b"added"),
        ("Remote/art.psd", b"do not replace"),
        ("Remote/.private", b"do not replace"),
        ("Remote/ncc.json", b"do not replace"),
    ]);
    let server = Server::new(vec![detail(&bytes), Reply::Bytes(bytes)]);
    let result = client(&dir, &server).plugin_workspace(&request).unwrap();
    assert_eq!(result["unchanged_count"], 1);
    assert_eq!(result["replaced_count"], 2);
    assert_eq!(result["removed_count"], 1);
    assert_eq!(fs::read(root.join("main.lua")).unwrap(), b"new code");
    assert_eq!(fs::read(root.join("added.lua")).unwrap(), b"added");
    assert!(!root.join("obsolete.lua").exists());
    assert_eq!(
        fs::read(root.join("art.psd")).unwrap(),
        b"preserve material"
    );
    assert_eq!(fs::read(root.join(".private")).unwrap(), b"preserve hidden");
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("ncc.json")).unwrap()).unwrap();
    assert_eq!(manifest["game_versions"], json!(["12.0.0"]));
    assert_eq!(manifest["version"], "local-next");
    assert_eq!(manifest["custom"]["keep"], true);
    storage::finish_file(held, Ok(()), &OperationControl::default()).unwrap();
    assert_eq!(server.finish().len(), 2);
    no_stages(&dir);
}
#[test]
fn collection_sync_keeps_top_level_notes_and_removes_obsolete_empty_modules() {
    let dir = TestDir::new();
    let root = dir.0.join("Collection");
    fs::create_dir_all(root.join("Old")).unwrap();
    fs::write(root.join("Old/Old.toc"), b"## Interface: 120000").unwrap();
    fs::write(root.join("notes.txt"), b"preserve user notes").unwrap();
    fs::write(
        root.join("ncc.json"),
        br#"{"id":91,"version":"next","game_versions":["1.15.8"],"ignore":[]}"#,
    )
    .unwrap();
    let request = request(&dir, false, json!({"dir":"Collection","confirm":true}));
    let bytes = archive(&[
        ("New/New.toc", b"## Interface: 120000"),
        ("New/main.lua", b"new"),
        ("notes.txt", b"do not replace notes"),
    ]);
    let server = Server::new(vec![detail(&bytes), Reply::Bytes(bytes)]);
    let result = client(&dir, &server).plugin_workspace(&request).unwrap();
    assert_eq!(result["layout"], "collection");
    assert_eq!(result["removed_count"], 1);
    assert_eq!(result["empty_directories_removed"], 1);
    assert!(!root.join("Old").exists());
    assert_eq!(
        fs::read(root.join("notes.txt")).unwrap(),
        b"preserve user notes"
    );
    assert_eq!(fs::read(root.join("New/main.lua")).unwrap(), b"new");
    server.finish();
    no_stages(&dir);
}
#[test]
fn changed_content_after_initial_scan_is_rechecked_instead_of_being_misreported_as_unchanged() {
    let dir = TestDir::new();
    let root = project(&dir);
    let request = request(&dir, false, json!({"dir":"Working","confirm":true}));
    let bytes = archive(&[
        ("Addon/Working.toc", b"## Interface: 120000\n"),
        ("Addon/main.lua", b"old code"),
    ]);
    let target = root.join("main.lua");
    let server = Server::with_hook(vec![detail(&bytes), Reply::Bytes(bytes)], move |index| {
        if index == 1 {
            fs::write(&target, b"changed during download").unwrap();
        }
    });
    let result = client(&dir, &server).plugin_workspace(&request).unwrap();
    assert_eq!(result["unchanged_count"], 1);
    assert_eq!(result["replaced_count"], 1);
    assert_eq!(fs::read(root.join("main.lua")).unwrap(), b"old code");
    server.finish();
    no_stages(&dir);
}
#[test]
fn modified_ignore_selection_stops_before_deleting_newly_ignored_content() {
    let dir = TestDir::new();
    let root = project(&dir);
    fs::write(
        root.join("obsolete.lua"),
        b"preserve after selection change",
    )
    .unwrap();
    let request = request(&dir, false, json!({"dir":"Working","confirm":true}));
    let bytes = archive(&[
        ("Addon/Working.toc", b"## Interface: 120000\n"),
        ("Addon/main.lua", b"online code"),
    ]);
    let target = root.join(".nccignore");
    let server = Server::with_hook(vec![detail(&bytes), Reply::Bytes(bytes)], move |index| {
        if index == 1 {
            fs::write(&target, b"obsolete.lua\n").unwrap();
        }
    });
    let error = client(&dir, &server)
        .plugin_workspace(&request)
        .unwrap_err();
    assert_eq!(error.exit_code, 6);
    assert_eq!(
        fs::read(root.join("obsolete.lua")).unwrap(),
        b"preserve after selection change"
    );
    assert_eq!(fs::read(root.join("main.lua")).unwrap(), b"old code");
    server.finish();
    no_stages(&dir);
}
#[test]
fn partial_sync_retains_original_manifest_change_report_and_prepared_archive() {
    let dir = TestDir::new();
    let root = project(&dir);
    fs::write(root.join("obsolete.lua"), b"obsolete").unwrap();
    let profile = fs::read(root.join("ncc.json")).unwrap();
    let (held, _) = storage::locked_sha256(
        &root.join("main.lua"),
        u64::MAX,
        &OperationControl::default(),
    )
    .unwrap();
    let request = request(&dir, false, json!({"dir":"Working","confirm":true}));
    let bytes = archive(&[
        ("Addon/Working.toc", b"## Interface: 120000\n"),
        ("Addon/main.lua", b"new code"),
    ]);
    let server = Server::new(vec![detail(&bytes), Reply::Bytes(bytes.clone())]);
    let error = client(&dir, &server)
        .plugin_workspace(&request)
        .unwrap_err();
    assert!(
        !error.details["local_changes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!root.join("obsolete.lua").exists());
    assert_eq!(fs::read(root.join("main.lua")).unwrap(), b"old code");
    assert_eq!(fs::read(root.join("ncc.json")).unwrap(), profile);
    let recovery = PathBuf::from(error.details["recovery_directory"].as_str().unwrap());
    assert_eq!(fs::read(recovery.join("original.zip")).unwrap(), bytes);
    assert_eq!(
        fs::read(recovery.join("payload/Addon/main.lua")).unwrap(),
        b"new code"
    );
    storage::finish_file(held, Ok(()), &OperationControl::default()).unwrap();
    server.finish();
}
#[test]
fn malformed_downloads_and_redirects_leave_project_content_untouched() {
    let bad = archive(&[("../escape.lua", b"not selected")]);
    for (response, body, exit) in [
        (detail(&bad), Reply::Bytes(bad), 4),
        (
            reply(
                json!({"mod_id":91,"latest_versions":["12.0.0"],"latest_file":{"version_name":"v2","file_size":1,"download_url":"https://cdn.invalid/a"}}),
            ),
            Reply::Bytes(archive(&[("Addon/Addon.toc", b"## Interface: 120000")])),
            5,
        ),
        (
            detail(&[]),
            Reply::Raw(
                "HTTP/1.1 302 Found\r\nContent-Length: 0\r\nLocation: https://untrusted.invalid/PRIVATE_REDIRECT\r\nConnection: close\r\n\r\n",
            ),
            5,
        ),
    ] {
        let dir = TestDir::new();
        let root = project(&dir);
        let profile = fs::read(root.join("ncc.json")).unwrap();
        let request = request(&dir, false, json!({"dir":"Working","confirm":true}));
        let server = Server::new(vec![response, body]);
        let error = client(&dir, &server)
            .plugin_workspace(&request)
            .unwrap_err();
        assert_eq!(error.exit_code, exit);
        assert!(!error.to_string().contains("PRIVATE_"));
        assert_eq!(fs::read(root.join("main.lua")).unwrap(), b"old code");
        assert_eq!(fs::read(root.join("ncc.json")).unwrap(), profile);
        assert!(!dir.0.join("escape.lua").exists());
        assert_eq!(server.finish().len(), 2);
        no_stages(&dir);
    }
}
#[test]
fn initialization_and_sync_local_admission_never_overwrite_existing_projects_or_implicitly_confirm()
{
    let dir = TestDir::new();
    let root = project(&dir);
    assert!(PluginWorkspace::parse(true,br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin.init","id":91,"dir":"Working"}"#,&dir.0,&OperationControl::default()).is_err());
    let request = request(&dir, false, json!({"dir":"Working"}));
    assert_eq!(
        request.plan(&OperationControl::default()).unwrap()["confirmation_required"],
        true
    );
    let server = Server::new(vec![]);
    assert_eq!(
        client(&dir, &server)
            .plugin_workspace(&request)
            .unwrap_err()
            .exit_code,
        2
    );
    server.finish();
    fs::write(root.join("ncc.json"), br#"{"id":91,"bundle":["Other"]}"#).unwrap();
    assert!(PluginWorkspace::parse(false,br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin.sync","dir":"Working","confirm":true}"#,&dir.0,&OperationControl::default()).is_err());
}
