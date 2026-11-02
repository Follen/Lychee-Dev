use super::*;
use crate::{
    dd::{DdTransport, tests::transport, upload},
    test_support::{Reply, Server, TestDir},
};
use std::fs;
const MODERN: &str = include_str!("../wa/fixtures/modern.txt");
struct Author<'a> {
    http: DdTransport,
    home: &'a Path,
    control: OperationControl,
    channel_calls: usize,
}
impl read::Api for Author<'_> {
    fn get(&mut self, p: &str, q: &[(&str, String)]) -> Result<Value> {
        self.http.get(p, q)
    }
    fn post_read(&mut self, p: &str, b: &Value) -> Result<Value> {
        self.http.post(p, &[], b, false)
    }
}
impl Api for Author<'_> {
    fn submit(&mut self, p: &str, b: &Value) -> Result<Value> {
        self.http.post(p, &[], b, true)
    }
    fn upload(&mut self, u: &ObjectUpload) -> Result<Value> {
        upload::send(
            &mut self.http,
            self.home,
            "author",
            &u.request,
            &self.control,
        )
    }
    fn channels(&mut self) -> Result<Value> {
        self.channel_calls += 1;
        Ok(json!({"items":[{"room_id":"room","channel_id":"selected","channel_type":"2"}]}))
    }
}
fn author<'a>(server: &Server, home: &'a Path) -> Author<'a> {
    let (mut http, _) = transport(server, "synthetic-token");
    let control = OperationControl::default();
    http.begin_operation(&control).unwrap();
    Author {
        http,
        home,
        control,
        channel_calls: 0,
    }
}
fn run(request: &WaWrite, author: &mut Author<'_>, recover: Option<&str>) -> Result<Value> {
    let control = OperationControl::default();
    author.http.begin_operation(&control)?;
    author.control = control.clone();
    publication::execute(request, author.home, "author", author, recover, &control)
}
fn parse(action: WriteAction, doc: &Value, dir: &TestDir) -> WaWrite {
    WaWrite::parse(
        action,
        &serde_json::to_vec(doc).unwrap(),
        &dir.0,
        &OperationControl::default(),
    )
    .unwrap()
}
fn clock() -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":1793500000}}))
}
fn reply(value: Value) -> Reply {
    Reply::Json(json!({"code":0,"result":value}))
}
fn list(rows: Value) -> Reply {
    let total = rows.as_array().unwrap().len();
    reply(json!({"items":rows,"total":total}))
}
fn options() -> Vec<Reply> {
    vec![
        reply(json!([{"version":"12.0.0"}])),
        reply(json!([{"name":"group without ID","children":[{"c_id":7,"name":"分类"}]}])),
        reply(json!([{"game_type":10001}])),
    ]
}
fn create() -> Value {
    json!({"schema":"lycheedev.publish.v1.dd.wa.create","game_type":10001,"scope":"public","name":"独立WA","game_version":"12.0.0","brief_desc":"原简介","display_imgs":["https://cdn.invalid/wa.png"],
    "category_ids":["ui_original",7],"desc":"<p>说明</p>","need_buy":false,"jump_room":false,"creation_statement":"original","with_associate":false,"need_anchor_vip":false,"content":MODERN,"update_desc":"更新说明","version":"1","with_file":false})
}
fn detail(version: Value) -> Value {
    json!({"sn":"selected","is_owner":true,"game_type":10001,"scope":"public","name":"独立WA","game_version":"12.0.0","brief_desc":"原简介","display_imgs":[{"d_url":"https://cdn.invalid/wa.png","name":"display"}],
    "category_ids":[{"c_id":"ui_original"},{"c_id":7}],"desc":"<p>说明</p>","need_buy":false,"jump_room":false,"creation_statement":"original","with_associate":false,"need_anchor_vip":false,"content":MODERN,
    "update_desc":"更新说明","version":version,"with_file":false,"file_install_path":"Interface/Addons","buy_life_type":"seven_day","parse_wa_uid":"SYNTHETIC-uid-001","parse_wa_id":"Synthetic WA 中文 ^ ~ 😀","token":"private-token"})
}
fn bodies(requests: &[crate::test_support::Request]) -> Vec<Value> {
    requests
        .iter()
        .filter(|r| {
            r.line.starts_with("POST /wa/create?") || r.line.starts_with("POST /wa/modify?")
        })
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}
#[test]
fn modern_creation_parses_identifiers_locally_sends_original_import_and_verifies_every_field() {
    let dir = TestDir::new();
    let home = dir.0.join("home");
    let request = parse(WriteAction::Create, &create(), &dir);
    let plan = request.plan();
    assert_eq!(plan["remote_write"], false);
    assert!(plan["fields"]["content"].is_object());
    assert!(!plan.to_string().contains(MODERN));
    assert!(!home.exists());
    let mut replies = vec![clock(), list(json!([]))];
    replies.extend(options());
    replies.extend([
        reply(json!({"sn":"selected"})),
        reply(detail(json!("1"))),
        list(json!([{"sn":"selected"}])),
    ]);
    let server = Server::new(replies);
    let mut author = author(&server, &home);
    let output = run(&request, &mut author, None).unwrap();
    assert_eq!(output["reference"], "selected");
    assert_eq!(output["verification"], "stable_fields_and_original_content");
    assert!(!output.to_string().contains(MODERN));
    assert!(!output.to_string().contains("private-token"));
    author.http.close().unwrap();
    let requests = server.finish();
    let body = bodies(&requests).remove(0);
    assert_eq!(body["content"], MODERN);
    assert_eq!(body["parse_wa_uid"], "SYNTHETIC-uid-001");
    assert_eq!(body["parse_wa_id"], "Synthetic WA 中文 ^ ~ 😀");
    assert_eq!(body["category_ids"], json!(["ui_original", "7"]));
    assert_eq!(body["display_imgs"], json!(["https://cdn.invalid/wa.png"]));
    assert_eq!(body["version"], "1");
    assert_eq!(body["with_file"], false);
    assert_eq!(body["share_code_life_type"], "forever");
    assert_eq!(body["file_install_path"], "Interface/Addons");
    let active = fs::read_dir(home.join("publish/dd/records/wa-create"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("active.json");
    let saved = fs::read_to_string(active).unwrap();
    assert!(!saved.contains(MODERN));
    assert!(!saved.contains("更新说明"));
    assert!(!saved.contains("author"));
}
#[test]
fn exact_large_version_update_clears_legacy_parse_ids_uploads_original_materials_and_blocks_other_pending_writes()
 {
    let dir = TestDir::new();
    let home = dir.0.join("home");
    fs::write(dir.0.join("materials.zip"), b"original-wa-materials").unwrap();
    let request = parse(
        WriteAction::Update,
        &json!({"schema":"lycheedev.publish.v1.dd.wa.update","sn":"selected","content":"legacy opaque export","update_desc":"新版本","version":"9007199254740993","with_file":false,"file":"materials.zip"}),
        &dir,
    );
    let blocked = parse(
        WriteAction::Edit,
        &json!({"schema":"lycheedev.publish.v1.dd.wa.edit","sn":"selected","brief_desc":"another action"}),
        &dir,
    );
    let mut after = detail(json!(9007199254740993u64));
    after["content"] = json!("legacy opaque export");
    after["update_desc"] = json!("新版本");
    after["parse_wa_uid"] = json!("");
    after["parse_wa_id"] = json!("");
    after["with_file"] = json!(true);
    after["file_path"] = json!("https://cdn.invalid/materials.zip");
    let mut replies = vec![
        clock(),
        reply(detail(json!("9007199254740992"))),
        list(json!([{"sn":"selected"}])),
    ];
    replies.extend(options());
    replies.extend([reply(json!({"url":"https://upload.invalid/materials?X-Amz-Signature=synthetic","d_url":"https://cdn.invalid/materials.zip","maxSize":52428800})),Reply::Raw("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),Reply::Drop,reply(after),list(json!([{"sn":"selected"}]))]);
    let server = Server::new(replies);
    let mut author = author(&server, &home);
    let first = run(&request, &mut author, None).unwrap_err();
    assert_eq!(first.details["submission"], "uncertain");
    let conflict = run(&blocked, &mut author, None).unwrap_err();
    assert_eq!(conflict.exit_code, 6);
    assert_eq!(
        conflict.details["operation_id"],
        first.details["operation_id"]
    );
    fs::remove_file(dir.0.join("materials.zip")).unwrap();
    let recovered = run(&request, &mut author, None).unwrap();
    assert_eq!(recovered["submitted_this_call"], false);
    assert_eq!(recovered["operation_id"], first.details["operation_id"]);
    author.http.close().unwrap();
    let requests = server.finish();
    let mutations = bodies(&requests);
    assert_eq!(mutations.len(), 1);
    assert_eq!(mutations[0]["version"], "9007199254740993");
    assert_eq!(mutations[0]["parse_wa_uid"], "");
    assert_eq!(mutations[0]["parse_wa_id"], "");
    assert_eq!(mutations[0]["with_file"], true);
    let grant = requests
        .iter()
        .find(|r| r.line.starts_with("GET /file/upload?"))
        .unwrap();
    assert!(grant.line.contains("file_name=wa_materials.zip"));
    assert!(grant.line.contains("business_id=wa"));
    let put = requests
        .iter()
        .find(|r| r.line.starts_with("PUT "))
        .unwrap();
    assert_eq!(put.body, b"original-wa-materials");
    assert!(requests.iter().all(|r| !r.line.contains("/login")));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET /file/upload?"))
            .count(),
        1
    );
}
#[test]
fn metadata_edit_preserves_content_and_material_reference_and_applies_explicit_commercial_disables()
{
    let dir = TestDir::new();
    let home = dir.0.join("home");
    let request = parse(
        WriteAction::Edit,
        &json!({"schema":"lycheedev.publish.v1.dd.wa.edit","sn":"selected","scope":"private","jump_room":false,"with_associate":false,"need_anchor_vip":false,"price_fen":19999,"brief_desc":"新简介"}),
        &dir,
    );
    let mut current = detail(json!("1"));
    current["need_buy"] = json!(true);
    current["price_fen"] = json!(10);
    current["jump_room"] = json!(true);
    current["room_id"] = json!("old-room");
    current["channel_id"] = json!("old-channel");
    current["channel_type"] = json!("2");
    current["sync_room"] = json!(true);
    current["need_anchor_vip"] = json!(true);
    current["vip_levels"] = json!([{"level":7}]);
    current["with_associate"] = json!(true);
    current["associated_acts"] = json!([{"sn":"old","act_type":"wa"}]);
    current["file_path"] = json!("https://cdn.invalid/existing.zip");
    let mut after = current.clone();
    after["scope"] = json!("private");
    after["share_code_life_type"] = json!("forever");
    after["brief_desc"] = json!("新简介");
    after["price_fen"] = json!(19999);
    after["jump_room"] = json!(false);
    after["sync_room"] = json!(false);
    after["need_anchor_vip"] = json!(false);
    after["vip_levels"] = json!([]);
    after["with_associate"] = json!(false);
    after["associated_acts"] = json!([]);
    for k in ["room_id", "channel_id", "channel_type"] {
        after[k] = json!("");
    }
    let mut replies = vec![clock(), reply(current), list(json!([{"sn":"selected"}]))];
    replies.extend(options());
    replies.extend([
        reply(json!({})),
        reply(after),
        list(json!([{"sn":"selected"}])),
    ]);
    let server = Server::new(replies);
    let mut author = author(&server, &home);
    run(&request, &mut author, None).unwrap();
    assert_eq!(author.channel_calls, 0);
    author.http.close().unwrap();
    let requests = server.finish();
    let body = bodies(&requests).remove(0);
    assert_eq!(body["content"], MODERN);
    assert_eq!(body["version"], "1");
    assert_eq!(body["file_path"], "https://cdn.invalid/existing.zip");
    assert_eq!(body["price_fen"].to_string(), "19999");
    assert_eq!(body["associated_acts"], json!([]));
    assert_eq!(body["vip_levels"], json!([]));
    assert_eq!(body["channel_id"], "");
    assert_eq!(body["sync_room"], false);
    assert!(
        requests
            .iter()
            .all(|r| !r.line.contains("anchor_vip") && !r.line.contains("/file/upload"))
    );
}
#[test]
fn invalid_modern_import_nested_shapes_and_noninteger_update_versions_fail_locally() {
    let dir = TestDir::new();
    for (field, value) in [
        ("content", json!("!WA:2!broken")),
        ("category_ids", json!([{"id":7}])),
        ("display_imgs", json!([{"url":"https://cdn.invalid/a.png"}])),
        ("vip_levels", json!([true])),
        ("price_fen", json!(10.5)),
        (
            "display_imgs",
            json!(["https://cdn.invalid/a?token=private-secret"]),
        ),
    ] {
        let mut doc = create();
        doc[field] = value;
        let e = WaWrite::parse(
            WriteAction::Create,
            &serde_json::to_vec(&doc).unwrap(),
            &dir.0,
            &OperationControl::default(),
        )
        .err()
        .unwrap();
        assert_eq!(e.exit_code, 4);
        assert!(!e.to_string().contains("private-secret"));
    }
    for version in ["1.5", "-1", "١٢", " ", " 2 "] {
        let doc = json!({"schema":"lycheedev.publish.v1.dd.wa.update","sn":"selected","content":"legacy","update_desc":"notes","version":version,"with_file":false});
        assert_eq!(
            WaWrite::parse(
                WriteAction::Update,
                &serde_json::to_vec(&doc).unwrap(),
                &dir.0,
                &OperationControl::default()
            )
            .err()
            .unwrap()
            .exit_code,
            4
        );
    }
    for field in ["with_file", "need_buy"] {
        let mut doc = create();
        doc[field] = json!(true);
        let e = WaWrite::parse(
            WriteAction::Create,
            &serde_json::to_vec(&doc).unwrap(),
            &dir.0,
            &OperationControl::default(),
        )
        .err()
        .unwrap();
        assert_eq!(e.exit_code, 4);
    }
    let mut doc = create();
    doc["version"] = json!("1.5");
    assert_eq!(
        WaWrite::parse(
            WriteAction::Create,
            &serde_json::to_vec(&doc).unwrap(),
            &dir.0,
            &OperationControl::default()
        )
        .err()
        .unwrap()
        .exit_code,
        4
    );
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        WaWrite::parse(
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
fn current_version_and_locked_game_or_usage_changes_are_rejected_before_options_or_post() {
    let dir = TestDir::new();
    for doc in [
        json!({"schema":"lycheedev.publish.v1.dd.wa.update","sn":"selected","content":"legacy","update_desc":"notes","version":"1","with_file":false}),
        json!({"schema":"lycheedev.publish.v1.dd.wa.edit","sn":"selected","game_type":10002}),
        json!({"schema":"lycheedev.publish.v1.dd.wa.edit","sn":"selected","need_buy":true,"price_fen":10,"buy_life_type":"seven_day"}),
    ] {
        let action = if doc["schema"].as_str().unwrap().ends_with(".update") {
            WriteAction::Update
        } else {
            WriteAction::Edit
        };
        let request = parse(action, &doc, &dir);
        let home = dir.0.join(format!("home-{}", action.name()));
        let server = Server::new(vec![
            clock(),
            reply(detail(json!("1"))),
            list(json!([{"sn":"selected"}])),
        ]);
        let mut author = author(&server, &home);
        assert_eq!(run(&request, &mut author, None).unwrap_err().exit_code, 4);
        author.http.close().unwrap();
        assert!(bodies(&server.finish()).is_empty());
    }
}
#[test]
fn numeric_comparison_is_exact_for_long_integer_versions_and_current_decimal_values() {
    for (left, right, expected) in [
        ("9007199254740993", "9007199254740992", true),
        ("9007199254740992", "9007199254740993", false),
        ("2", "1.5", true),
        ("1", "1.5", false),
        ("100", "1e2", false),
        ("101", "1e2", true),
        ("0", "-0.1", true),
        ("000", "0.0", false),
    ] {
        assert_eq!(form::greater(left, right).unwrap(), expected);
    }
}
