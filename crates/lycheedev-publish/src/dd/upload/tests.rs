use super::*;
use crate::{
    dd::tests::{Probe, transport},
    test_support::{Reply, Server, TestDir},
};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
fn authorization(id: &str, maximum: Value) -> Reply {
    Reply::Json(
        json!({"code":0,"result":{"url":format!("https://upload.invalid/object-{id}?X-Amz-Signature=synthetic-grant"),"d_url":format!("https://cdn.invalid/object-{id}"),"maxSize":maximum}}),
    )
}
fn clock() -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":1793500000}}))
}
fn upload_ok() -> Reply {
    Reply::Raw("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}
fn header<'a>(request: &'a crate::test_support::Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .lines()
        .filter_map(|s| s.split_once(':'))
        .find_map(|(k, v)| k.eq_ignore_ascii_case(name).then_some(v.trim()))
}
fn client(server: &Server, control: &OperationControl) -> (DdTransport, Probe) {
    let (mut client, probe) = transport(server, "synthetic-author-token");
    client.begin_operation(control).unwrap();
    (client, probe)
}
#[test]
fn all_five_purposes_keep_fixed_wire_names_mimes_and_original_bytes_without_author_headers_on_put()
{
    for (kind, suffix, file_type, business, name, mime) in [
        (
            UploadKind::PluginArchive,
            "zip",
            "a19-ui-res",
            "addon",
            Some("addon.zip"),
            "application/x-zip-compressed",
        ),
        (
            UploadKind::WaMaterials,
            "ZIP",
            "a19-ui-res",
            "wa",
            Some("wa_materials.zip"),
            "application/x-zip-compressed",
        ),
        (
            UploadKind::PluginImage,
            "PNG",
            "a19-ui-media",
            "img",
            Some(""),
            "image/png",
        ),
        (
            UploadKind::ConfigImage,
            "jpeg",
            "a19-ui-media",
            "img",
            None,
            "image/jpeg",
        ),
        (
            UploadKind::WaImage,
            "gif",
            "a19-ui-media",
            "img",
            None,
            "image/gif",
        ),
    ] {
        let dir = TestDir::new();
        let path = dir.0.join(format!("本地 [特殊] &+.{}", suffix));
        let bytes = b"independent-original-binary\x00\x01\xff";
        fs::write(&path, bytes).unwrap();
        let control = OperationControl::default();
        let selected = ObjectUpload::select(&path, kind, &control).unwrap();
        assert_eq!(selected.plan()["remote_write"], false);
        assert!(!dir.0.join("home").exists());
        let server = Server::new(vec![
            clock(),
            authorization("one", json!(1000)),
            upload_ok(),
        ]);
        let (mut author, _) = client(&server, &control);
        let receipt = send(
            &mut author,
            &dir.0.join("home"),
            "9007199254740993",
            &selected.request,
            &control,
        )
        .unwrap();
        assert_eq!(receipt["url"], "https://cdn.invalid/object-one");
        assert_eq!(receipt["reused"], false);
        assert_eq!(receipt["bytes"], bytes.len());
        assert_eq!(receipt["sha256"], storage::sha256(bytes));
        author.close().unwrap();
        let requests = server.finish();
        assert_eq!(requests.len(), 3);
        let query = Url::parse(&format!(
            "https://uiapi.w.163.com{}",
            requests[1].line.split(' ').nth(1).unwrap()
        ))
        .unwrap();
        assert_eq!(query.path(), "/file/upload");
        let values = query
            .query_pairs()
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(values.get("file_type").unwrap(), file_type);
        assert_eq!(values.get("business_id").unwrap(), business);
        assert_eq!(values.get("mime_type").unwrap(), mime);
        assert_eq!(values.get("file_name").map(|s| s.as_ref()), name);
        assert!(!requests[1].line.contains("%E6%9C%AC%E5%9C%B0"));
        assert!(header(&requests[1], "cookie").is_some());
        assert!(header(&requests[1], "x-w163-token").is_some());
        assert_eq!(requests[2].body, bytes);
        assert!(requests[2].line.starts_with("PUT /object-one?"));
        assert_eq!(header(&requests[2], "content-type"), Some(mime));
        assert_eq!(header(&requests[2], "x-amz-acl"), Some("public-read"));
        for key in [
            "cookie",
            "x-w163-token",
            "x-timestamp",
            "author-token",
            "authorization",
            "authentication",
            "origin",
            "referer",
        ] {
            assert_eq!(header(&requests[2], key), None);
        }
        fs::write(&path, b"file handles released").unwrap();
    }
}
#[test]
fn completed_objects_reuse_original_receipts_across_equal_byte_paths_and_a_b_a_account_switches() {
    let dir = TestDir::new();
    let path = dir.0.join("first.png");
    fs::write(&path, b"same-original-bytes").unwrap();
    let control = OperationControl::default();
    let selected = ObjectUpload::select(&path, UploadKind::PluginImage, &control).unwrap();
    let server = Server::new(vec![
        clock(),
        authorization("A", json!(0)),
        upload_ok(),
        authorization("B", json!("1000")),
        upload_ok(),
    ]);
    let (mut author, _) = client(&server, &control);
    let home = dir.0.join("home");
    let first = send(&mut author, &home, "account-A", &selected.request, &control).unwrap();
    let second = send(&mut author, &home, "account-B", &selected.request, &control).unwrap();
    assert_ne!(first["operation_id"], second["operation_id"]);
    let moved = dir.0.join("other-name.PNG");
    fs::write(&moved, b"same-original-bytes").unwrap();
    let same = ObjectUpload::select(&moved, UploadKind::PluginImage, &control).unwrap();
    let again = send(&mut author, &home, "account-A", &same.request, &control).unwrap();
    assert_eq!(again["operation_id"], first["operation_id"]);
    assert_eq!(again["url"], first["url"]);
    assert_eq!(again["reused"], true);
    author.close().unwrap();
    assert_eq!(server.finish().len(), 5);
    for entry in fs::read_dir(home.join("publish/dd/records/object")).unwrap() {
        for file in fs::read_dir(entry.unwrap().path()).unwrap() {
            let file = file.unwrap();
            if file.path().extension().is_some_and(|e| e == "json") {
                let text = fs::read_to_string(file.path()).unwrap();
                for private in [
                    "synthetic-grant",
                    "synthetic-author-token",
                    "synthetic-cookie",
                    "account-A",
                    "account-B",
                ] {
                    assert!(!text.contains(private));
                }
            }
        }
    }
}
#[test]
fn retained_borrowed_sessions_scope_uploads_by_their_actual_login_identity_without_an_extra_login()
{
    use crate::dd::{
        desktop::Desktop,
        session::{BorrowedSession, TaskSession},
    };
    let dir = TestDir::new();
    let path = dir.0.join("selected.png");
    fs::write(&path, b"same selected artifact").unwrap();
    let control = OperationControl::default();
    let selected = ObjectUpload::select(&path, UploadKind::PluginImage, &control).unwrap();
    let login =
        |id: u64, token: &str| Reply::Json(json!({"code":200,"result":{"uid":id,"token":token}}));
    let server = Server::new(vec![
        clock(),
        login(9007199254740993, "original-token-A"),
        authorization("A", json!(0)),
        upload_ok(),
        clock(),
        login(9007199254740994, "original-token-B"),
        authorization("B", json!(0)),
        upload_ok(),
        clock(),
        login(9007199254740993, "original-token-A"),
    ]);
    let home = dir.0.join("home");
    let mut receipts = Vec::new();
    for index in 0..3 {
        let (mut author, _) = transport(&server, "");
        author.cookie = format!("synthetic-cookie-{index}");
        let desktop = Desktop::synthetic(
            lycheedev_windows::ProcessIdentity {
                pid: 41,
                created: 42,
            },
            lycheedev_windows::ProcessIdentity {
                pid: 43,
                created: 44,
            },
        );
        let mut session =
            TaskSession::Borrowed(BorrowedSession::attach(author, desktop, home.clone()).unwrap());
        session.begin_operation(&control).unwrap();
        assert_eq!(session.identity()["native_login_count"], 0);
        receipts.push(session.upload(&home, &selected.request, &control).unwrap());
        session.close().unwrap();
    }
    assert_ne!(receipts[0]["operation_id"], receipts[1]["operation_id"]);
    assert_eq!(receipts[0]["operation_id"], receipts[2]["operation_id"]);
    assert_eq!(receipts[2]["reused"], true);
    let requests = server.finish();
    assert_eq!(requests.len(), 10);
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET /login/dflogin"))
            .count(),
        3
    );
    assert_eq!(
        header(&requests[2], "x-w163-token"),
        Some("original-token-A")
    );
    assert_eq!(
        header(&requests[6], "x-w163-token"),
        Some("original-token-B")
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("PUT "))
            .count(),
        2
    );
}
#[test]
fn changed_oversized_unsupported_or_cancelled_local_selection_never_authorizes_an_upload() {
    let dir = TestDir::new();
    let path = dir.0.join("selected.zip");
    fs::write(&path, b"original").unwrap();
    let control = OperationControl::default();
    let selected = ObjectUpload::select(&path, UploadKind::PluginArchive, &control).unwrap();
    fs::write(&path, b"changed").unwrap();
    let server = Server::new(vec![]);
    let (mut author, _) = client(&server, &control);
    let home = dir.0.join("absent");
    let error = send(&mut author, &home, "author", &selected.request, &control).unwrap_err();
    assert_eq!(error.details["submission"], "not_sent");
    assert!(!home.exists());
    let oversized = dir.0.join("oversized.png");
    let file = fs::File::create(&oversized).unwrap();
    file.set_len(10 * 1024 * 1024 + 1).unwrap();
    drop(file);
    assert!(ObjectUpload::select(&oversized, UploadKind::ConfigImage, &control).is_err());
    assert!(ObjectUpload::select(&path, UploadKind::ConfigImage, &control).is_err());
    let cancelled = OperationControl::default();
    cancelled.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        ObjectUpload::select(&path, UploadKind::PluginArchive, &cancelled)
            .err()
            .unwrap()
            .exit_code,
        7
    );
    author.close().unwrap();
    assert!(server.finish().is_empty());
    fs::remove_file(oversized).unwrap();
}
#[test]
fn malformed_grants_and_server_size_limits_stop_before_put_without_persisting_a_pending_upload() {
    for info in [
        json!({}),
        json!({"url":"http://object.invalid/a","d_url":"https://cdn.invalid/a"}),
        json!({"url":"https://object.invalid/a?X-Amz-Signature=private","d_url":"https://cdn.invalid/a?token=private"}),
        json!({"url":"https://object.invalid/a","d_url":"https://cdn.invalid/a","maxSize":1}),
        json!({"url":"https://object.invalid/a","d_url":"https://cdn.invalid/a","maxSize":true}),
    ] {
        let dir = TestDir::new();
        let path = dir.0.join("selected.zip");
        fs::write(&path, b"selected-bytes").unwrap();
        let control = OperationControl::default();
        let selected = ObjectUpload::select(&path, UploadKind::PluginArchive, &control).unwrap();
        let server = Server::new(vec![
            clock(),
            Reply::Json(json!({"code":0,"result":info})),
            authorization("corrected", json!(0)),
            upload_ok(),
        ]);
        let (mut author, _) = client(&server, &control);
        let home = dir.0.join("home");
        let error = send(&mut author, &home, "author", &selected.request, &control).unwrap_err();
        assert_eq!(error.details["submission"], "not_sent");
        assert!(!error.to_string().contains("token=private"));
        // A new explicit corrected attempt is permitted because zero PUTs ran.
        assert_eq!(
            send(&mut author, &home, "author", &selected.request, &control).unwrap()["reused"],
            false
        );
        author.close().unwrap();
        assert_eq!(server.finish().len(), 4);
    }
}
#[test]
fn lost_put_response_and_non_200_acknowledgement_keep_original_pending_intent_without_replay() {
    for reply in [
        Reply::Drop,
        Reply::Raw("HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        Reply::Raw(
            "HTTP/1.1 302 Found\r\nLocation: https://leak.invalid/replay\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ),
    ] {
        let dir = TestDir::new();
        let path = dir.0.join("selected.zip");
        fs::write(&path, b"original-file").unwrap();
        let control = OperationControl::default();
        let selected = ObjectUpload::select(&path, UploadKind::PluginArchive, &control).unwrap();
        let server = Server::new(vec![clock(), authorization("uncertain", json!(0)), reply]);
        let (mut author, _) = client(&server, &control);
        let home = dir.0.join("home");
        let first = send(&mut author, &home, "author", &selected.request, &control).unwrap_err();
        assert_eq!(first.details["submission"], "uncertain");
        assert!(!first.to_string().contains("synthetic-grant"));
        assert_eq!(first.details["replay_allowed"], false);
        let second = send(&mut author, &home, "author", &selected.request, &control).unwrap_err();
        assert_eq!(second.exit_code, 6);
        assert_eq!(
            second.details["operation_id"],
            first.details["operation_id"]
        );
        author.close().unwrap();
        let requests = server.finish();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.line.starts_with("PUT "))
                .count(),
            1
        );
        fs::write(path, b"released after uncertain PUT").unwrap();
    }
}
#[test]
fn original_file_is_locked_across_authorization_and_put_and_definitive_rejection_allows_an_explicit_new_attempt()
 {
    let dir = TestDir::new();
    let path = dir.0.join("selected.zip");
    fs::write(&path, b"unchanged original payload").unwrap();
    let control = OperationControl::default();
    let selected = ObjectUpload::select(&path, UploadKind::PluginArchive, &control).unwrap();
    let observed = Arc::new(AtomicUsize::new(0));
    let checks = observed.clone();
    let changed = path.clone();
    let server = Server::with_hook(
        vec![
            clock(),
            authorization("first", json!(0)),
            Reply::Raw("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            authorization("second", json!(0)),
            upload_ok(),
        ],
        move |index| {
            if index > 0 {
                assert!(fs::write(&changed, b"must-not-overwrite").is_err());
                assert!(fs::remove_file(&changed).is_err());
                checks.fetch_add(1, Ordering::SeqCst);
            }
        },
    );
    let (mut author, _) = client(&server, &control);
    let home = dir.0.join("home");
    let first = send(&mut author, &home, "author", &selected.request, &control).unwrap_err();
    assert_eq!(first.details["submission"], "rejected");
    let second = send(&mut author, &home, "author", &selected.request, &control).unwrap();
    assert_eq!(second["reused"], false);
    author.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 5);
    assert_eq!(observed.load(Ordering::SeqCst), 4);
    assert_eq!(requests[2].body, b"unchanged original payload");
    assert_eq!(requests[4].body, requests[2].body);
    fs::write(path, b"all handles released").unwrap();
}
#[test]
fn original_cancellation_during_authorization_sends_no_put_and_a_fresh_operation_keeps_the_same_author()
 {
    let dir = TestDir::new();
    let path = dir.0.join("selected.png");
    fs::write(&path, b"original").unwrap();
    let control = OperationControl::default();
    let signal = control.tracked_cancel_handle().unwrap();
    let selected = ObjectUpload::select(&path, UploadKind::ConfigImage, &control).unwrap();
    let server = Server::with_hook(
        vec![
            clock(),
            authorization("cancelled", json!(0)),
            authorization("fresh", json!(0)),
            upload_ok(),
        ],
        move |index| {
            if index == 1 {
                signal.cancel();
            }
        },
    );
    let (mut author, _) = client(&server, &control);
    let home = dir.0.join("home");
    let error = send(&mut author, &home, "author", &selected.request, &control).unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(error.details["submission"], "not_sent");
    let fresh = OperationControl::default();
    author.begin_operation(&fresh).unwrap();
    assert_eq!(
        send(&mut author, &home, "author", &selected.request, &fresh).unwrap()["reused"],
        false
    );
    author.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("PUT "))
            .count(),
        1
    );
}
#[test]
fn rejected_authorization_is_not_a_submitted_object_and_a_corrected_explicit_attempt_is_allowed() {
    let dir = TestDir::new();
    let path = dir.0.join("selected.zip");
    fs::write(&path, b"original").unwrap();
    let control = OperationControl::default();
    let selected = ObjectUpload::select(&path, UploadKind::PluginArchive, &control).unwrap();
    let server = Server::new(vec![
        clock(),
        Reply::Json(json!({"code":400,"msg":"private-auth-message"})),
        authorization("corrected", json!(0)),
        upload_ok(),
    ]);
    let (mut author, _) = client(&server, &control);
    let home = dir.0.join("home");
    let error = send(&mut author, &home, "author", &selected.request, &control).unwrap_err();
    assert_eq!(error.details["submission"], "not_sent");
    assert!(!error.to_string().contains("private-auth-message"));
    assert_eq!(
        send(&mut author, &home, "author", &selected.request, &control).unwrap()["reused"],
        false
    );
    author.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("PUT "))
            .count(),
        1
    );
}
