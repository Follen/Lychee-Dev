use super::*;
use crate::{
    dd::{DdTransport, tests::transport, upload},
    test_support::{Reply, Server, TestDir},
};
use std::fs;
struct TestApi<'a> {
    author: DdTransport,
    home: &'a Path,
    subject: &'a str,
    control: OperationControl,
    channel_calls: usize,
}
impl read::Api for TestApi<'_> {
    fn get(&mut self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.author.get(path, query)
    }
    fn post_read(&mut self, path: &str, body: &Value) -> Result<Value> {
        self.author.post(path, &[], body, false)
    }
}
impl Api for TestApi<'_> {
    fn submit(&mut self, path: &str, body: &Value) -> Result<Value> {
        self.author.post(path, &[], body, true)
    }
    fn upload(&mut self, selection: &ObjectUpload) -> Result<Value> {
        upload::send(
            &mut self.author,
            self.home,
            self.subject,
            &selection.request,
            &self.control,
        )
    }
    fn channels(&mut self) -> Result<Value> {
        self.channel_calls += 1;
        Ok(
            json!({"items":[{"room_id":"room","channel_id":"selected","channel_type":"2","room_name":"room","channel_name":"channel"}]}),
        )
    }
}
fn api<'a>(server: &Server, home: &'a Path, subject: &'a str) -> TestApi<'a> {
    let (mut author, _) = transport(server, "synthetic-author-token");
    let control = OperationControl::default();
    author.begin_operation(&control).unwrap();
    TestApi {
        author,
        home,
        subject,
        control,
        channel_calls: 0,
    }
}
fn clock() -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":1793500000}}))
}
fn reply(value: Value) -> Reply {
    Reply::Json(json!({"code":0,"result":value}))
}
fn listing(rows: Value) -> Reply {
    let n = rows.as_array().unwrap().len();
    reply(json!({"items":rows,"total":n}))
}
fn options() -> Vec<Reply> {
    vec![
        reply(json!([{"game_version":"12.0.0"}])),
        reply(json!([{"c_id":10,"children":[{"c_id":11}]}])),
        reply(json!([{"game_type":10001,"name":"正式服"}])),
    ]
}
fn create() -> Value {
    json!({"schema":"lycheedev.publish.v1.dd.plugin.create","game_type":10001,"scope":"public","addon_type":0,
    "name":"独立插件","description":"原简介","logo":"https://cdn.invalid/logo.png","detail_imgs":["https://cdn.invalid/detail.png"],
    "primary_category_id":10,"second_category_ids":[11],"html_desc":"<p>原详情</p>","need_buy":false,"jump_room":false,"creation_statement":"original",
    "with_associate":false,"need_anchor_vip":false,"game_versions":["12.0.0"],"detail_url":"https://cdn.invalid/v2.zip","release_type":2,"version":"2.0","update_desc":"第二版"})
}
fn detail(sn: &str, published: &str) -> Value {
    json!({"sn":sn,"is_owner":true,"game_type":0,"game_types":[10001],"scope":"public","addon_type":0,
    "name":"独立插件","description":"原简介","logo":"https://cdn.invalid/logo.png","detail_imgs":[{"d_url":"https://cdn.invalid/detail.png","display":"only"}],
    "primary_category_id":{"c_id":10,"name":"分类"},"second_category_ids":[{"c_id":11},{"c_id":999}],"html_desc":"<p>原详情</p>","need_buy":false,"jump_room":false,
    "creation_statement":"original","buy_life_type":"seven_day","with_associate":false,"need_anchor_vip":false,"game_versions":[{"version":"12.0.0","label":"界面"}],"update_desc":"第二版",
    "latest_version":{"file_path":format!("https://cdn.invalid/v{published}.zip"),"release_type":if published=="1" {1}else{2},"version":format!("{published}.0"),"game_versions":["12.0.0"],"update_desc":"第二版"},
    "token":"private-token","content":"private-import","status":"review-pending"})
}
fn author(sn: &str, published: &str) -> Value {
    json!({"sn":sn,"name":"独立插件","latest_version":{"file_path":format!("https://cdn.invalid/v{published}.zip"),"release_type":if published=="1"{1}else{2},"version":format!("{published}.0"),"game_versions":["12.0.0"],"update_desc":"第二版"}})
}
fn parse(action: WriteAction, doc: &Value, dir: &TestDir) -> PluginWrite {
    PluginWrite::parse(
        action,
        &serde_json::to_vec(doc).unwrap(),
        &dir.0,
        &OperationControl::default(),
    )
    .unwrap()
}
fn run(request: &PluginWrite, api: &mut TestApi<'_>, recover: Option<&str>) -> Result<Value> {
    let control = OperationControl::default();
    api.control = control.clone();
    api.author.begin_operation(&control)?;
    request.execute_with(api.home, api.subject, api, recover, &control)
}
fn mutation_bodies(requests: &[crate::test_support::Request]) -> Vec<Value> {
    requests
        .iter()
        .filter(|r| {
            r.line.starts_with("POST /addon/create?") || r.line.starts_with("POST /addon/modify?")
        })
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}
fn receipt_json(home: &Path) -> String {
    fn visit(root: &Path, out: &mut String) {
        if root.is_dir() {
            for entry in fs::read_dir(root).unwrap() {
                visit(&entry.unwrap().path(), out);
            }
        } else if root.extension().is_some_and(|e| e == "json") {
            out.push_str(&fs::read_to_string(root).unwrap());
        }
    }
    let mut out = String::new();
    visit(home, &mut out);
    out
}
#[test]
fn create_sends_full_scalar_wire_form_and_reuses_the_original_creation_with_fresh_readback() {
    let dir = TestDir::new();
    let request = parse(WriteAction::Create, &create(), &dir);
    let home = dir.0.join("home");
    assert!(!home.exists());
    assert_eq!(request.plan()["remote_write"], false);
    let mut replies = vec![clock(), listing(json!([]))];
    replies.extend(options());
    replies.extend([
        reply(json!({"sn":"created"})),
        reply(detail("created", "2")),
        listing(json!([author("created", "2")])),
        reply(detail("created", "2")),
        listing(json!([author("created", "2")])),
    ]);
    let server = Server::new(replies);
    let mut api = api(&server, &home, "author-A");
    let first = run(&request, &mut api, None).unwrap();
    let again = run(&request, &mut api, None).unwrap();
    assert_eq!(first["reference"], "created");
    assert_eq!(first["submitted_this_call"], true);
    assert_eq!(again["submitted_this_call"], false);
    assert_eq!(first["operation_id"], again["operation_id"]);
    assert_eq!(first["readback"]["version_source"], "detail_v2");
    assert!(!first.to_string().contains("private-token"));
    assert!(!first.to_string().contains("private-import"));
    assert_eq!(api.channel_calls, 0);
    api.author.close().unwrap();
    let requests = server.finish();
    let bodies = mutation_bodies(&requests);
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        bodies[0],
        json!({"game_type":10001,"scope":"public","addon_type":0,"name":"独立插件","description":"原简介","logo":"https://cdn.invalid/logo.png",
        "detail_imgs":["https://cdn.invalid/detail.png"],"primary_category_id":10,"second_category_ids":[11],"html_desc":"<p>原详情</p>","need_buy":false,"price_fen":0,
        "buy_life_type":"seven_day","share_code_life_type":"forever","jump_room":false,"room_id":"","channel_id":"","channel_type":"","sync_room":false,"creation_statement":"original",
        "with_associate":false,"associated_acts":[],"need_anchor_vip":false,"vip_levels":[],"game_versions":["12.0.0"],"detail_url":"https://cdn.invalid/v2.zip","release_type":2,"version":"2.0","update_desc":"第二版"})
    );
    let persisted = receipt_json(&home);
    for secret in [
        "private-token",
        "private-import",
        "原简介",
        "<p>原详情</p>",
        "https://cdn.invalid/v2.zip",
        "author-A",
    ] {
        assert!(!persisted.contains(secret));
    }
}
#[test]
fn metadata_edit_preserves_the_published_author_version_and_projects_display_objects_before_upload()
{
    let dir = TestDir::new();
    fs::write(dir.0.join("logo.png"), b"original-image-bytes").unwrap();
    let home = dir.0.join("home");
    let request = parse(
        WriteAction::Edit,
        &json!({"schema":"lycheedev.publish.v1.dd.plugin.edit","sn":"selected","description":"新简介","html_desc":"<p>新详情</p>","logo_file":"logo.png"}),
        &dir,
    );
    let mut old = detail("selected", "1");
    old.as_object_mut().unwrap().remove("is_owner");
    let mut after = detail("selected", "1");
    after["description"] = json!("新简介");
    after["html_desc"] = json!("<p>新详情</p>");
    let mut author_after = author("selected", "2");
    author_after["logo"] = json!("https://cdn.invalid/new-logo.png");
    let mut replies = vec![
        clock(),
        reply(old),
        listing(json!([author("selected", "2")])),
    ];
    replies.extend(options());
    replies.extend([reply(json!({"url":"https://upload.invalid/logo?X-Amz-Signature=synthetic-grant","d_url":"https://cdn.invalid/new-logo.png","maxSize":1024})),
        Reply::Raw("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),reply(json!({})),reply(after),listing(json!([author_after]))]);
    let server = Server::new(replies);
    let mut api = api(&server, &home, "author-A");
    let value = run(&request, &mut api, None).unwrap();
    assert_eq!(value["readback"]["sources"]["logo"], "author_list");
    assert_eq!(value["readback"]["sources"]["description"], "detail_v2");
    api.author.close().unwrap();
    let requests = server.finish();
    let body = mutation_bodies(&requests).remove(0);
    assert_eq!(body["version"], "2.0");
    assert_eq!(body["detail_url"], "https://cdn.invalid/v2.zip");
    assert_eq!(body["release_type"], 2);
    assert_eq!(body["game_type"], 10001);
    assert_eq!(body["game_versions"], json!(["12.0.0"]));
    assert_eq!(body["primary_category_id"], 10);
    assert_eq!(body["second_category_ids"], json!([11]));
    assert_eq!(
        body["detail_imgs"],
        json!(["https://cdn.invalid/detail.png"])
    );
    assert_eq!(body["logo"], "https://cdn.invalid/new-logo.png");
    assert_eq!(body["description"], "新简介");
    assert!(body.get("token").is_none());
    assert!(body.get("status").is_none());
    assert!(body.get("logo_file").is_none());
    let put = requests
        .iter()
        .find(|r| r.line.starts_with("PUT "))
        .unwrap();
    assert_eq!(put.body, b"original-image-bytes");
    assert!(!put.headers.to_ascii_lowercase().contains("x-w163-token"));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET /file/upload?"))
            .count(),
        1
    );
    assert!(!receipt_json(&home).contains("synthetic-grant"));
}
#[test]
fn version_update_preserves_saved_optional_controls_without_querying_them_and_uses_one_matching_version_projection()
 {
    let dir = TestDir::new();
    let home = dir.0.join("home");
    let request = parse(
        WriteAction::Update,
        &json!({"schema":"lycheedev.publish.v1.dd.plugin.update","sn":"selected","game_versions":["12.0.0"],"version":"2.0","update_desc":"第二版","release_type":2,"detail_url":"https://cdn.invalid/v2.zip"}),
        &dir,
    );
    let mut current = detail("selected", "1");
    current["need_anchor_vip"] = json!(true);
    current["vip_levels"] = json!([{"level":7}]);
    current["jump_room"] = json!(true);
    current["room_id"] = json!("old-room");
    current["channel_id"] = json!("old-channel");
    current["channel_type"] = json!("2");
    current["with_associate"] = json!(true);
    current["associated_acts"] =
        json!([{"sn":"old-reference","act_type":"wa","name":"display only"}]);
    let mut replies = vec![
        clock(),
        reply(current),
        listing(json!([author("selected", "1")])),
        reply(json!({"items":[{"version":"0.9"}],"total":2})),
        reply(json!({"items":[{"version":"1.0"}],"total":2})),
    ];
    replies.extend(options());
    replies.extend([
        reply(json!({})),
        reply(detail("selected", "1")),
        listing(json!([author("selected", "2")])),
    ]);
    let server = Server::new(replies);
    let mut api = api(&server, &home, "author");
    let value = run(&request, &mut api, None).unwrap();
    assert_eq!(value["readback"]["version_source"], "author_list");
    assert_eq!(api.channel_calls, 0);
    api.author.close().unwrap();
    let requests = server.finish();
    let body = mutation_bodies(&requests).remove(0);
    assert_eq!(body["vip_levels"], json!([7]));
    assert_eq!(body["need_anchor_vip"], true);
    assert_eq!(body["room_id"], "old-room");
    assert_eq!(
        body["associated_acts"],
        json!([{"sn":"old-reference","act_type":"wa"}])
    );
    assert!(requests.iter().all(|r| !r.line.contains("anchor_vip")
        && !r.line.contains("/wa/list")
        && !r.line.contains("/share/list")));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET /addon/addon_versions?"))
            .count(),
        2
    );
}
#[test]
fn history_collisions_and_incomplete_history_fail_before_any_mutation_or_upload() {
    for history_rows in [
        vec![reply(json!({"items":[{"version":"2.0"}],"total":1}))],
        vec![
            reply(json!({"items":[{"version":"0.8"}],"total":2})),
            reply(json!({"items":[],"total":2})),
        ],
    ] {
        let dir = TestDir::new();
        let home = dir.0.join("home");
        let request = parse(
            WriteAction::Update,
            &json!({"schema":"lycheedev.publish.v1.dd.plugin.update","sn":"selected","game_versions":["12.0.0"],"version":"2.0","update_desc":"第二版"}),
            &dir,
        );
        let mut replies = vec![
            clock(),
            reply(detail("selected", "1")),
            listing(json!([author("selected", "1")])),
        ];
        replies.extend(history_rows);
        let server = Server::new(replies);
        let mut api = api(&server, &home, "author");
        assert!(run(&request, &mut api, None).is_err());
        api.author.close().unwrap();
        let requests = server.finish();
        assert!(mutation_bodies(&requests).is_empty());
        assert!(requests.iter().all(|r| !r.line.contains("/file/upload")));
    }
}
#[test]
fn unknown_creation_requires_explicit_new_sn_and_recovers_original_fields_without_another_post() {
    let dir = TestDir::new();
    let home = dir.0.join("home");
    let request = parse(WriteAction::Create, &create(), &dir);
    let mut replies = vec![clock(), listing(json!([author("preexisting", "2")]))];
    replies.extend(options());
    replies.extend([
        Reply::Drop,
        reply(detail("created", "2")),
        listing(json!([author("preexisting", "2"), author("created", "2")])),
    ]);
    let server = Server::new(replies);
    let mut api = api(&server, &home, "author");
    let error = run(&request, &mut api, None).unwrap_err();
    assert_eq!(error.details["submission"], "uncertain");
    assert_eq!(run(&request, &mut api, None).unwrap_err().exit_code, 6);
    assert_eq!(
        run(&request, &mut api, Some("preexisting"))
            .unwrap_err()
            .exit_code,
        6
    );
    let result = run(&request, &mut api, Some("created")).unwrap();
    assert_eq!(result["operation_id"], error.details["operation_id"]);
    assert_eq!(result["submitted_this_call"], false);
    assert!(run(&request, &mut api, Some("other")).is_err());
    api.author.close().unwrap();
    let requests = server.finish();
    assert_eq!(mutation_bodies(&requests).len(), 1);
    assert_eq!(requests.len(), 8);
}
#[test]
fn accepted_but_stale_edit_stays_pending_and_recovery_performs_only_fresh_reads() {
    let dir = TestDir::new();
    let home = dir.0.join("home");
    let request = parse(
        WriteAction::Edit,
        &json!({"schema":"lycheedev.publish.v1.dd.plugin.edit","sn":"selected","description":"新简介"}),
        &dir,
    );
    let mut replies = vec![
        clock(),
        reply(detail("selected", "2")),
        listing(json!([author("selected", "2")])),
    ];
    replies.extend(options());
    replies.push(reply(json!({})));
    for _ in 0..6 {
        replies.push(reply(detail("selected", "2")));
        replies.push(listing(json!([author("selected", "2")])));
    }
    let mut fresh = detail("selected", "2");
    fresh["description"] = json!("新简介");
    replies.push(reply(fresh));
    replies.push(listing(json!([author("selected", "2")])));
    let server = Server::new(replies);
    let mut api = api(&server, &home, "author");
    let first = run(&request, &mut api, None).unwrap_err();
    assert_eq!(first.exit_code, 6);
    assert_eq!(first.details["submission"], "pending");
    let result = run(&request, &mut api, None).unwrap();
    assert_eq!(result["recovered"], true);
    assert_eq!(result["operation_id"], first.details["operation_id"]);
    api.author.close().unwrap();
    let requests = server.finish();
    assert_eq!(mutation_bodies(&requests).len(), 1);
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET /game_type/list?"))
            .count(),
        1
    );
}
#[test]
fn selected_dynamic_controls_require_exact_channel_and_author_namespaces_and_preserve_integer_price()
 {
    let dir = TestDir::new();
    let home = dir.0.join("home");
    let mut doc = create();
    doc["need_buy"] = json!(true);
    doc["price_fen"] = json!(19999);
    doc["buy_life_type"] = json!("forever");
    doc["jump_room"] = json!(true);
    doc["room_id"] = json!("room");
    doc["channel_id"] = json!("selected");
    doc["channel_type"] = json!("2");
    doc["with_associate"] = json!(true);
    doc["associated_acts"] = json!([{"act_type":"wa","sn":"associated"}]);
    doc["need_anchor_vip"] = json!(true);
    doc["vip_levels"] = json!([7]);
    let request = parse(WriteAction::Create, &doc, &dir);
    let mut readback = detail("created", "2");
    for key in [
        "need_buy",
        "price_fen",
        "buy_life_type",
        "jump_room",
        "room_id",
        "channel_id",
        "channel_type",
        "with_associate",
        "associated_acts",
        "need_anchor_vip",
        "vip_levels",
    ] {
        readback[key] = doc[key].clone();
    }
    let replies = vec![
        clock(),
        listing(json!([])),
        reply(json!([{"version":"12.0.0"}])),
        reply(json!([{"c_id":10,"children":[{"c_id":11}]}])),
        reply(json!([{"level":7}])),
        listing(json!([])),
        listing(json!([])),
        listing(json!([{"sn":"associated"}])),
        reply(json!([{"game_type":10001}])),
        reply(json!({"sn":"created"})),
        reply(readback),
        listing(json!([author("created", "2")])),
    ];
    let server = Server::new(replies);
    let mut api = api(&server, &home, "author");
    run(&request, &mut api, None).unwrap();
    assert_eq!(api.channel_calls, 1);
    api.author.close().unwrap();
    let requests = server.finish();
    let body = mutation_bodies(&requests).remove(0);
    assert_eq!(body["price_fen"].to_string(), "19999");
    assert_eq!(body["vip_levels"], json!([7]));
    assert_eq!(
        body["associated_acts"],
        json!([{"sn":"associated","act_type":"wa"}])
    );
    let vip = requests
        .iter()
        .find(|r| r.line.starts_with("GET /anchor_vip/level/list?"))
        .unwrap();
    assert!(vip.line.contains("enrich_acts=false"));
}
#[test]
fn local_nested_contracts_and_recovery_selection_fail_before_home_or_remote_effects() {
    let dir = TestDir::new();
    for (field, value) in [
        ("need_buy", json!(true)),
        ("jump_room", json!(true)),
        ("with_associate", json!(true)),
        ("scope", json!("private")),
        ("game_versions", json!([{"version":"12.0.0"}])),
        ("second_category_ids", json!([true])),
        ("associated_acts", json!([{"sn":"x","act_type":"plugin"}])),
        (
            "associated_acts",
            json!([{"sn":"x","act_type":"wa","token":"private-secret"}]),
        ),
        ("vip_levels", json!([7.5])),
        ("price_fen", json!(9)),
        (
            "logo",
            json!("https://cdn.invalid/image?token=private-secret"),
        ),
        ("detail_imgs", json!([12])),
        ("game_type", json!(0)),
    ] {
        let mut value_doc = create();
        value_doc[field] = value;
        let error = PluginWrite::parse(
            WriteAction::Create,
            &serde_json::to_vec(&value_doc).unwrap(),
            &dir.0,
            &OperationControl::default(),
        )
        .err()
        .unwrap();
        assert_eq!(error.exit_code, 4);
        assert!(!error.to_string().contains("private-secret"));
    }
    let mut doc = create();
    doc.as_object_mut().unwrap().remove("logo");
    assert!(
        PluginWrite::parse(
            WriteAction::Create,
            &serde_json::to_vec(&doc).unwrap(),
            &dir.0,
            &OperationControl::default()
        )
        .is_err()
    );
    let request = parse(WriteAction::Create, &create(), &dir);
    let home = dir.0.join("home");
    let server = Server::new(vec![]);
    let mut api = api(&server, &home, "author");
    assert_eq!(
        run(&request, &mut api, Some("selected"))
            .unwrap_err()
            .exit_code,
        2
    );
    api.author.close().unwrap();
    assert!(server.finish().is_empty());
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        PluginWrite::parse(
            WriteAction::Create,
            &serde_json::to_vec(&create()).unwrap(),
            &dir.0,
            &control
        )
        .err()
        .unwrap()
        .exit_code,
        7
    );
}
#[test]
fn plugin_zip_admission_rejects_invalid_archives_and_backslashes_in_either_stored_header() {
    let dir = TestDir::new();
    let valid = fs::read(dir.zip()).unwrap();
    let source = dir.0.join("selected.zip");
    let mut doc = create();
    doc.as_object_mut().unwrap().remove("detail_url");
    doc["file"] = json!("selected.zip");
    let mut local = valid.clone();
    let local_header = local.windows(4).position(|b| b == b"PK\x03\x04").unwrap();
    let offset = local_header + 30 + "AuthorAddon".len();
    assert_eq!(local[offset], b'/');
    local[offset] = b'\\';
    let mut central = valid.clone();
    let header = central.windows(4).position(|b| b == b"PK\x01\x02").unwrap();
    let offset = header + 46 + "AuthorAddon".len();
    assert_eq!(central[offset], b'/');
    central[offset] = b'\\';
    for bytes in [b"invalid ZIP".to_vec(), local, central] {
        fs::write(&source, bytes).unwrap();
        let error = PluginWrite::parse(
            WriteAction::Create,
            &serde_json::to_vec(&doc).unwrap(),
            &dir.0,
            &OperationControl::default(),
        )
        .err()
        .unwrap();
        assert_eq!(error.exit_code, 4);
        assert_eq!(error.details["path"], "$.file");
    }
    fs::write(&source, &valid).unwrap();
    let request = parse(WriteAction::Create, &doc, &dir);
    assert_eq!(
        request.plan()["uploads"][0]["selection"]["bytes"],
        valid.len()
    );
    fs::rename(&source, dir.0.join("released.zip")).unwrap();
}
