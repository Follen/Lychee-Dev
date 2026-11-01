use super::*;
use crate::{
    newbee::{
        backup_bundle::tests::{doc, fixture},
        tests::provider,
    },
    test_support::{Reply, Server, TestDir},
};
use std::fs;

fn generated(dir: &TestDir) -> (BackupUpload, Value, Vec<Vec<u8>>) {
    let root = fixture(dir);
    let output = dir.0.join("bundle");
    let mut input = doc(&root, &output);
    input["fonts"] = json!([]);
    BackupGenerate::parse(&serde_json::to_vec(&input).unwrap(), &dir.0)
        .unwrap()
        .generate(&OperationControl::default())
        .unwrap();
    let bundle = output.join("backup-bundle.json");
    let value = crate::input::value(&fs::read(&bundle).unwrap()).unwrap();
    let bytes = value["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artifact| fs::read(output.join(artifact["file"].as_str().unwrap())).unwrap())
        .collect::<Vec<_>>();
    (BackupUpload { bundle, name: None }, value, bytes)
}
fn clouds(rows: Vec<Value>) -> Reply {
    Reply::Json(json!({"code":1,"data":rows}))
}
fn metadata() -> Reply {
    Reply::Json(json!({"game_version":[{"id":2,"name":"Fixture"}]}))
}
fn objects() -> Vec<Reply> {
    let mut replies = Vec::new();
    for i in 0..4 {
        replies.push(Reply::Json(json!({"code":1,"data":{"file_path":format!("backups/object-{i}.zip"),"upload_url":format!("http://wclboxdata.cn-hangzhou.oss.aliyuncs.com/object-{i}?signature=PRIVATE_CAPABILITY")}})));
        replies.push(Reply::Raw(
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ));
    }
    replies
}
fn cloud() -> Value {
    json!({"t_id":91,"t_name":"Automatic name","t_Versionid":"2","t_create_time":"2026-11-01T00:00:00Z",
    "t_Known_plug":[{"name":"Remote identity","id":9007199254740993u64,"fileName":null,"fileUrl":"","mod_file_id":991,"mod_version":"1.0","display_name":null,"updateType":1}],
    "t_unKnown_list":"[{\"name\":\"One\"}]","t_material_list":[{"name":"Textures"}],"t_font_list":"[]","t_unKnown_plug":"https://objects.invalid/backups/object-0.zip","t_material":"backups/object-1.zip","t_font_ncs_hash":null,
    "wtflist":[{"account":"A","commonConfig":"backups/object-2.zip","server":[{"serverName":"Realm","roleList":[{"name":"Chosen","zip":"backups/object-3.zip"}]}]}]})
}
#[test]
fn backup_creation_and_object_upload_receipts_are_partitioned_by_the_current_desktop_subject() {
    let dir = TestDir::new();
    let (request, _, _) = generated(&dir);
    let mut replies = Vec::new();
    for id in [91, 92] {
        let mut row = cloud();
        row["t_id"] = json!(id);
        replies.extend([metadata(), clouds(vec![])]);
        replies.extend(objects());
        replies.extend([Reply::Json(json!({"code":1,"data":id})), clouds(vec![row])]);
    }
    let server = Server::new(replies);
    for (subject, id) in [("author-A", 91), ("author-B", 92), ("author-A", 91)] {
        let mut client = provider(&dir, &server, true);
        crate::newbee::tests::set_subject(&dir, subject);
        assert_eq!(client.upload_backup(&request, None).unwrap()["id"], id);
    }
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line.starts_with("PUT "))
            .count(),
        8
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line == "POST /v3/share/create HTTP/1.1")
            .count(),
        2
    );
    assert_eq!(requests.len(), 24);
}
fn count(requests: &[crate::test_support::Request], path: &str) -> usize {
    requests
        .iter()
        .filter(|request| request.line == path)
        .count()
}
#[test]
fn native_backup_upload_streams_original_admitted_objects_and_reuses_the_creation_receipt() {
    let dir = TestDir::new();
    let (request, bundle, bytes) = generated(&dir);
    let mut replies = vec![metadata(), clouds(vec![])];
    replies.extend(objects());
    replies.extend([
        Reply::Json(json!({"code":1,"data":null})),
        clouds(vec![cloud()]),
    ]);
    let server = Server::new(replies);
    let mut client = provider(&dir, &server, true);
    let plan = request.plan(&OperationControl::default()).unwrap();
    assert_eq!(plan["archives"], 4);
    assert!(!dir.0.join("home/publish").exists());
    let result = client.upload_backup(&request, None).unwrap();
    assert_eq!(result["id"], 91);
    assert_eq!(result["verification"], "manifest_readback");
    assert_eq!(result["remote_byte_verification"], "not_available");
    let reused = client.upload_backup(&request, None).unwrap();
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["author_writes"], 0);
    let requests = server.finish();
    assert_eq!(requests.len(), 12);
    assert_eq!(count(&requests, "POST /v3/share/create HTTP/1.1"), 1);
    for i in 0..4 {
        let prepared = &requests[2 + i * 2];
        let put = &requests[3 + i * 2];
        assert_eq!(
            serde_json::from_slice::<Value>(&prepared.body).unwrap(),
            json!({"file_type":(["unknown_plug","material","wtfserve","wtfrole"][i]),"file_size":bytes[i].len()})
        );
        assert_eq!(put.body, bytes[i]);
        assert!(
            put.headers
                .contains("content-type: application/x-www-form-urlencoded")
        );
        assert!(!put.headers.contains("authorization:"));
        assert!(!put.headers.contains("token:"));
        assert!(!put.headers.contains("x-oss-callback:"));
    }
    let wire = crate::input::value(&requests[10].body).unwrap();
    assert_eq!(wire["knowplug_list"], bundle["manifest"]["knowplug_list"]);
    assert_eq!(
        wire["wtf"][0]["server"][0]["roleList"][0]["zip"],
        "backups/object-3.zip"
    );
    assert_eq!(wire["unknown_plug"], "backups/object-0.zip");
    assert_eq!(wire["font_hash"], Value::Null);
    assert!(!result.to_string().contains("PRIVATE_"));
}
#[test]
fn a_lost_backup_creation_is_recovered_by_explicit_id_without_reupload_or_create() {
    let dir = TestDir::new();
    let (request, _, _) = generated(&dir);
    let mut replies = vec![metadata(), clouds(vec![])];
    replies.extend(objects());
    replies.extend([Reply::Drop, clouds(vec![cloud()])]);
    let server = Server::new(replies);
    let mut client = provider(&dir, &server, true);
    assert_eq!(
        client.upload_backup(&request, None).unwrap_err().details["submission"],
        "uncertain"
    );
    let pending = client.upload_backup(&request, None).unwrap_err();
    assert_eq!(pending.exit_code, 6);
    assert!(!pending.to_string().contains("PRIVATE_"));
    let recovered = client.upload_backup(&request, NonZeroU64::new(91)).unwrap();
    assert_eq!(recovered["recovered"], true);
    assert_eq!(recovered["author_writes"], 0);
    let requests = server.finish();
    assert_eq!(requests.len(), 12);
    assert_eq!(count(&requests, "POST /v3/share/create HTTP/1.1"), 1);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line.starts_with("PUT "))
            .count(),
        4
    );
}
#[test]
fn uncertain_object_put_stops_before_backup_creation_and_is_never_repeated() {
    let dir = TestDir::new();
    let (request, _, _) = generated(&dir);
    let mut replies = vec![metadata(), clouds(vec![])];
    let mut prepared = objects();
    replies.push(prepared.remove(0));
    replies.extend([Reply::Drop, metadata(), clouds(vec![])]);
    let server = Server::new(replies);
    let mut client = provider(&dir, &server, true);
    assert_eq!(
        client.upload_backup(&request, None).unwrap_err().details["submission"],
        "uncertain"
    );
    assert_eq!(
        client.upload_backup(&request, None).unwrap_err().exit_code,
        6
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 6);
    assert_eq!(count(&requests, "POST /v3/share/create HTTP/1.1"), 0);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line.starts_with("PUT "))
            .count(),
        1
    );
}
#[test]
fn acknowledged_object_uploads_are_reused_after_an_explicit_backup_creation_rejection() {
    let dir = TestDir::new();
    let (request, _, _) = generated(&dir);
    let mut replies = vec![metadata(), clouds(vec![])];
    replies.extend(objects());
    replies.extend([
        Reply::Json(json!({"code":0,"message":"PRIVATE_ECHO"})),
        metadata(),
        clouds(vec![]),
        Reply::Json(json!({"code":1,"data":{"t_id":91}})),
        clouds(vec![cloud()]),
    ]);
    let server = Server::new(replies);
    let mut client = provider(&dir, &server, true);
    let rejected = client.upload_backup(&request, None).unwrap_err();
    assert_eq!(rejected.details["submission"], "rejected");
    assert!(!rejected.to_string().contains("PRIVATE_"));
    assert_eq!(client.upload_backup(&request, None).unwrap()["id"], 91);
    let requests = server.finish();
    assert_eq!(requests.len(), 15);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line.starts_with("PUT "))
            .count(),
        4
    );
    assert_eq!(count(&requests, "POST /v3/share/create HTTP/1.1"), 2);
}
#[test]
fn verified_creation_survives_a_pending_rename_and_the_retry_performs_only_rename_readback() {
    let dir = TestDir::new();
    let (mut request, _, _) = generated(&dir);
    request.name = Some("Selected name".into());
    let mut renamed = cloud();
    renamed["t_name"] = json!("Selected name");
    let mut replies = vec![metadata(), clouds(vec![])];
    replies.extend(objects());
    replies.extend([
        Reply::Json(json!({"code":1,"data":91})),
        clouds(vec![cloud()]),
        clouds(vec![cloud()]),
        Reply::Drop,
        clouds(vec![renamed]),
    ]);
    let server = Server::new(replies);
    let mut client = provider(&dir, &server, true);
    let pending = client.upload_backup(&request, None).unwrap_err();
    assert_eq!(pending.details["created_backup"]["id"], 91);
    assert_eq!(
        pending.details["created_backup"]["verification"],
        "manifest_readback"
    );
    assert_eq!(pending.details["rename_pending"], true);
    let recovered = client.upload_backup(&request, None).unwrap();
    assert_eq!(recovered["reused"], true);
    assert_eq!(recovered["rename"]["recovered"], true);
    let requests = server.finish();
    assert_eq!(requests.len(), 15);
    assert_eq!(count(&requests, "POST /v3/share/create HTTP/1.1"), 1);
    assert_eq!(count(&requests, "POST /share/update HTTP/1.1"), 1);
}
#[test]
fn independent_backup_readback_rejects_unselected_rows_wrong_refs_roles_and_known_file_versions() {
    let dir = TestDir::new();
    let (_, bundle, _) = generated(&dir);
    let mut expected = bundle["manifest"].clone();
    for (i, target) in [
        super::super::backup_bundle::validation::Target::Unknown,
        super::super::backup_bundle::validation::Target::Material,
        super::super::backup_bundle::validation::Target::Common(0),
        super::super::backup_bundle::validation::Target::Role(0, 0, 0),
    ]
    .iter()
    .enumerate()
    {
        target.apply(&mut expected, json!(format!("backups/object-{i}.zip")));
    }
    assert!(manifest_matches(&expected, &cloud()));
    for field in [
        "plugin-version",
        "extra-plugin",
        "role",
        "object",
        "font",
        "game-version",
    ] {
        let mut row = cloud();
        match field {
            "plugin-version" => row["t_Known_plug"][0]["mod_file_id"] = json!(992),
            "extra-plugin" => row["t_unKnown_list"] = json!([{"name":"One"},{"name":"Two"}]),
            "role" => row["wtflist"][0]["server"][0]["roleList"][0]["name"] = json!("Other"),
            "object" => row["t_material"] = json!("backups/wrong.zip"),
            "font" => row["t_font_ncs_hash"] = json!("unexpected_font"),
            _ => row["t_Versionid"] = json!(3),
        }
        assert!(!manifest_matches(&expected, &row), "{field}");
    }
    assert!(contains(&json!({"name":"123"}), &json!({"name":"123"})));
    assert!(!contains(&json!({"name":"null"}), &json!({"name":null})));
}
#[test]
fn bundle_and_generate_creation_inputs_are_exclusive_and_upload_plans_reject_changed_archives() {
    let dir = TestDir::new();
    let (request, _, _) = generated(&dir);
    let upload = json!({"schema":"lycheedev.publish.v1.newbee.backup.create","bundle_file":request.bundle,"game_dir":"ignored"});
    assert_eq!(
        BackupCreate::parse(&serde_json::to_vec(&upload).unwrap(), &dir.0)
            .err()
            .unwrap()
            .exit_code,
        4
    );
    let artifact = request.bundle.parent().unwrap().join("unknown_plug.zip");
    fs::write(artifact, b"changed archive").unwrap();
    assert_eq!(
        request
            .plan(&OperationControl::default())
            .unwrap_err()
            .exit_code,
        4
    );
}
