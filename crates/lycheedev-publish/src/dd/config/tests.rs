use super::*;
mod metadata;
use crate::{
    dd::{DdTransport, tests::transport, upload},
    test_support::{Reply, Server, TestDir},
};
use std::fs;
// Independently computed with Node crypto SHA256 over the documented NUL parts.
const ROLE_A: &str = "wtf_9f9e22ad3fc6faa04b17";
const ROLE_B: &str = "wtf_6eb5fe50a7ecfef1066b";
const EDIT_A: &str = "em_1c822a00e0220469132d";
const COOL_A: &str = "cd_9d1f8f247851ff9701a9";
const ADDON: u64 = 9007199254740993;
struct Author<'a> {
    http: DdTransport,
    home: &'a Path,
    control: OperationControl,
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
        panic!("unchanged/disabled channel must not be queried")
    }
}
fn author<'a>(s: &Server, h: &'a Path) -> Author<'a> {
    let (mut http, _) = transport(s, "synthetic-token");
    let control = OperationControl::default();
    http.begin_operation(&control).unwrap();
    Author {
        http,
        home: h,
        control,
    }
}
fn run(r: &ConfigWrite, a: &mut Author<'_>, recover: Option<&str>) -> Result<Value> {
    let control = OperationControl::default();
    a.control = control.clone();
    a.http.begin_operation(&control)?;
    publication::execute(r, a.home, "author", a, recover, &control)
}
fn parse(action: WriteAction, doc: &Value, d: &TestDir) -> ConfigWrite {
    ConfigWrite::parse(
        action,
        &serde_json::to_vec(doc).unwrap(),
        &d.0,
        &OperationControl::default(),
    )
    .unwrap()
}
fn clock() -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":1793500000}}))
}
fn reply(v: Value) -> Reply {
    Reply::Json(json!({"code":0,"result":v}))
}
fn list(v: Value) -> Reply {
    let n = v.as_array().unwrap().len();
    reply(json!({"items":v,"total":n}))
}
fn authored() -> Reply {
    list(json!([{"share_sn":"share-selected","title":"原配置"}]))
}
fn games() -> Reply {
    reply(json!([{"game_type":10001},{"game_type":10002}]))
}
fn addon(id: u64) -> Value {
    json!({"addon_id":id,"name":"AuthorAddon","dirs":["AuthorAddon"],"content":"private-addon-content"})
}
fn role(account: &str) -> Value {
    json!({"name":account,"account_metadata":"kept","servers":[{"name":"Realm","region":"CN","items":[{"role_id":42,"name":"同名角色","content":"private-role-content"}]}]})
}
fn backup(sn: &str, game: u64) -> Value {
    json!({"sn":sn,"name":"原备份","game_type":game,
    "known_addon":{"items":[addon(ADDON),addon(2)]},"unknown_addon":{"items":["Loose"]},"material":{"items":[{"name":"SharedMedia","content":"private-material"}]},"font":{"items":["Arial"]},
    "known_wa":{"items":[{"uid":"K","id":"original-K","content":"private-known-wa"}]},"unknown_wa":{"items":[{"uid":"U","id":"original-U","content":"private-unknown-wa"}]},
    "extra":{"wa_account_info":{"A":[{"uid":"K","id":"account-K-A"},{"uid":"U","id":"account-U-A"}],"B":[{"uid":"U","id":"account-U-B"}]}},"wtf":{"accounts":[role("A"),role("B")]},
    "retail_ui_config":{"editMode":{"A":[{"name":"同名布局","data":"private-layout-code"},{"name":"同名布局","data":"private-second-layout"}]},"coolDown":{"A":[{"name":"CD","spec_tag":"Mage-Arcane","data":"private-cooldown"},{"name":"CD2","spec_tag":"Mage-Arcane","data":"private-cooldown-2"}]}}})
}
fn create() -> Value {
    json!({"schema":"lycheedev.publish.v1.dd.config.create","scope":"public","title":"原配置","brief_desc":"原简介","desc":"<p>原说明</p>","display_imgs":["https://cdn.invalid/config.png"],"need_buy":false,"jump_room":false,"creation_statement":"original","with_associate":false,"need_anchor_vip":false,
    "backup_sn":"backup-one","known_addon_ids":[ADDON],"unknown_addon_ids":["Loose"],"wtf_role_ids":[ROLE_A],"material_names":["SharedMedia"],"font_names":["Arial"],"known_wa_ids":["K"],"unknown_wa_ids":["U"],
    "retail_ui_config":{"edit_mode_selectors":[EDIT_A],"default_edit_mode_selector":EDIT_A,"cool_down_selectors":[COOL_A],"enable_dd_setup_wizard":false}})
}
fn stored() -> Value {
    json!({"share_sn":"share-selected","is_owner":true,"game_type":10001,"backup_sn":"backup-one","scope":"public","title":"原配置","brief_desc":"原简介","desc":"<p>原说明</p>","update_desc":"","display_imgs":["https://cdn.invalid/config.png"],"need_buy":false,"price_fen":0,"buy_life_type":"seven_day","jump_room":false,"room_id":"","channel_id":"","channel_type":"","sync_room":false,"creation_statement":"original","with_associate":false,"associated_acts":[],"need_anchor_vip":false,"vip_levels":[],
    "known_addon":{"items":[addon(ADDON)],"inner_version":{ADDON.to_string():1,"2":1}},"unknown_addon":{"items":["Loose"],"inner_version":{"Loose":1}},"material":{"items":[{"name":"SharedMedia","content":"private-material"}],"inner_version":{"SharedMedia":1}},"font":{"items":["Arial"],"inner_version":{"Arial":1}},
    "known_wa":{"items":[{"uid":"K","id":"original-K","content":"private-known-wa"}],"inner_version":{"K":1}},"unknown_wa":{"items":[{"uid":"U","id":"account-U-A","content":"private-unknown-wa"}],"inner_version":{"U":1}},"wtf":{"accounts":[role("A")]},
    "retail_ui_config":{"edit_mode":{"A":[{"name":"同名布局","data":"private-layout-code","is_default":true}]},"cool_down":{"A":[{"name":"CD","spec_tag":"Mage-Arcane","data":"private-cooldown"}]},"enable_dd_setup_wizard":false}})
}
fn preflight(current: Option<Value>, b: Value) -> Vec<Reply> {
    let mut r = vec![clock()];
    if let Some(c) = current {
        r.push(reply(c));
    }
    r.push(list(json!([{"sn":b["sn"],"game_type":b["game_type"]}])));
    r.push(reply(b));
    r.push(if r.len() == 3 {
        list(json!([]))
    } else {
        authored()
    });
    r
}
fn bodies(r: &[crate::test_support::Request]) -> Vec<Value> {
    r.iter()
        .filter(|r| {
            r.line.starts_with("POST /share/create?") || r.line.starts_with("POST /share/modify?")
        })
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}
#[test]
fn creation_binds_exact_role_wa_account_layouts_and_full_wire_groups_without_exposing_raw_data() {
    let d = TestDir::new();
    let home = d.0.join("home");
    let request = parse(WriteAction::Create, &create(), &d);
    assert_eq!(request.plan()["remote_write"], false);
    assert!(!home.exists());
    let mut replies = preflight(None, backup("backup-one", 10001));
    replies.extend([
        games(),
        reply(json!({"share_sn":"share-selected"})),
        reply(stored()),
        authored(),
    ]);
    let server = Server::new(replies);
    let mut a = author(&server, &home);
    let out = run(&request, &mut a, None).unwrap();
    assert_eq!(out["reference"], "share-selected");
    assert_eq!(
        out["verification"],
        "stable_fields_and_selected_backup_content"
    );
    for raw in [
        "private-layout-code",
        "private-cooldown",
        "private-role-content",
        "private-material",
        "private-known-wa",
    ] {
        assert!(!out.to_string().contains(raw), "{raw}");
    }
    a.http.close().unwrap();
    let requests = server.finish();
    let all = bodies(&requests);
    assert_eq!(all.len(), 1);
    let body = &all[0];
    assert_eq!(
        body["known_addon"]["items"][0]["addon_id"].to_string(),
        "9007199254740993"
    );
    assert_eq!(body["unknown_wa"]["items"][0]["id"], "account-U-A");
    assert_eq!(body["known_wa"]["items"][0]["id"], "original-K");
    assert_eq!(body["wtf"]["accounts"].as_array().unwrap().len(), 1);
    assert_eq!(body["wtf"]["accounts"][0]["name"], "A");
    assert_eq!(
        body["retail_ui_config"]["edit_mode"]["A"][0]["data"],
        "private-layout-code"
    );
    assert_eq!(
        body["retail_ui_config"]["edit_mode"]["A"][0]["is_default"],
        true
    );
    assert_eq!(body["need_buy"], 0);
    assert!(body.get("share_code_life_type").is_none());
    assert!(body.get("game_type").is_none());
    assert!(body.get("known_addon_ids").is_none());
}
#[test]
fn same_backup_update_preserves_omitted_groups_and_layouts_and_increments_only_selected_markers() {
    let d = TestDir::new();
    let home = d.0.join("home");
    let request = parse(
        WriteAction::Update,
        &json!({"schema":"lycheedev.publish.v1.dd.config.update","share_sn":"share-selected","backup_sn":"backup-one","update_desc":"修订","known_addon_update_ids":[ADDON],"unknown_wa_update_ids":["U"]}),
        &d,
    );
    let mut current = stored();
    current["known_addon"]["inner_version"] = json!({ADDON.to_string():7,"2":4});
    current["unknown_wa"]["inner_version"]["U"] = json!(5);
    let mut after = current.clone();
    after["known_addon"]["inner_version"][ADDON.to_string()] = json!(8);
    after["unknown_wa"]["inner_version"]["U"] = json!(6);
    after["update_desc"] = json!("修订");
    let mut replies = preflight(Some(current), backup("backup-one", 10001));
    replies.extend([games(), reply(json!({})), reply(after), authored()]);
    let server = Server::new(replies);
    let mut a = author(&server, &home);
    run(&request, &mut a, None).unwrap();
    a.http.close().unwrap();
    let body = bodies(&server.finish()).remove(0);
    assert_eq!(body["known_addon"]["inner_version"][ADDON.to_string()], 8);
    assert_eq!(body["known_addon"]["inner_version"]["2"], 4);
    assert_eq!(body["unknown_wa"]["inner_version"]["U"], 6);
    assert_eq!(body["retail_ui_config"], stored()["retail_ui_config"]);
    assert_eq!(body["title"], "原配置");
}
#[test]
fn changing_role_account_clears_unselected_wa_and_explicit_old_account_wa_is_rejected() {
    for explicit in [false, true] {
        let d = TestDir::new();
        let home = d.0.join("home");
        let mut doc = json!({"schema":"lycheedev.publish.v1.dd.config.update","share_sn":"share-selected","backup_sn":"backup-one","update_desc":"切换角色","wtf_role_ids":[ROLE_B]});
        if explicit {
            doc["known_wa_ids"] = json!(["K"]);
        }
        let request = parse(WriteAction::Update, &doc, &d);
        let mut replies = preflight(Some(stored()), backup("backup-one", 10001));
        if !explicit {
            let mut after = stored();
            after["update_desc"] = json!("切换角色");
            after["wtf"] = json!({"accounts":[role("B")]});
            after["known_wa"]["items"] = json!([]);
            after["unknown_wa"]["items"] = json!([]);
            replies.extend([games(), reply(json!({})), reply(after), authored()]);
        }
        let server = Server::new(replies);
        let mut a = author(&server, &home);
        let result = run(&request, &mut a, None);
        if explicit {
            assert_eq!(result.unwrap_err().exit_code, 4);
        } else {
            result.unwrap();
        }
        a.http.close().unwrap();
        let requests = server.finish();
        let b = bodies(&requests);
        if explicit {
            assert!(b.is_empty());
        } else {
            assert_eq!(b[0]["wtf"]["accounts"][0]["name"], "B");
            assert_eq!(b[0]["known_wa"]["items"], json!([]));
            assert_eq!(b[0]["unknown_wa"]["items"], json!([]));
        }
    }
}
#[test]
fn backup_changes_require_complete_reselection_and_explicit_retail_choice() {
    let d = TestDir::new();
    let home = d.0.join("home");
    let doc = json!({"schema":"lycheedev.publish.v1.dd.config.update","share_sn":"share-selected","backup_sn":"backup-two","update_desc":"迁移"});
    let request = parse(WriteAction::Update, &doc, &d);
    let server = Server::new(vec![clock(), reply(stored())]);
    let mut a = author(&server, &home);
    let e = run(&request, &mut a, None).unwrap_err();
    assert_eq!(e.details["path"], "$.known_addon_ids");
    a.http.close().unwrap();
    assert!(bodies(&server.finish()).is_empty());
    let mut doc = create();
    doc["backup_sn"] = json!("backup-two");
    doc["wtf_role_ids"] = json!(["wtf_9791670a804a644d2063"]);
    doc.as_object_mut().unwrap().remove("retail_ui_config");
    let request = parse(WriteAction::Create, &doc, &d);
    let server = Server::new(preflight(None, backup("backup-two", 10001)));
    let mut a = author(&server, &home);
    assert_eq!(
        run(&request, &mut a, None).unwrap_err().details["path"],
        "$.retail_ui_config"
    );
    a.http.close().unwrap();
    assert!(bodies(&server.finish()).is_empty());
}
#[test]
fn unknown_creation_reuses_original_image_receipt_and_recovers_only_owned_full_field_readback() {
    let d = TestDir::new();
    let home = d.0.join("home");
    fs::write(d.0.join("selected.png"), b"original-config-image").unwrap();
    let mut doc = create();
    doc["display_imgs"] = json!([]);
    doc["display_img_files"] = json!(["selected.png"]);
    let request = parse(WriteAction::Create, &doc, &d);
    let mut after = stored();
    after["display_imgs"] = json!(["https://cdn.invalid/uploaded.png"]);
    let mut replies = preflight(None, backup("backup-one", 10001));
    replies.extend([games(),reply(json!({"url":"https://upload.invalid/image?X-Amz-Signature=synthetic","d_url":"https://cdn.invalid/uploaded.png","maxSize":1024})),Reply::Raw("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),Reply::Drop,reply(after),authored()]);
    let server = Server::new(replies);
    let mut a = author(&server, &home);
    let first = run(&request, &mut a, None).unwrap_err();
    assert_eq!(first.details["submission"], "uncertain");
    assert_eq!(run(&request, &mut a, None).unwrap_err().exit_code, 6);
    let result = run(&request, &mut a, Some("share-selected")).unwrap();
    assert_eq!(result["operation_id"], first.details["operation_id"]);
    assert_eq!(result["submitted_this_call"], false);
    a.http.close().unwrap();
    let requests = server.finish();
    assert_eq!(bodies(&requests).len(), 1);
    let grant = requests
        .iter()
        .find(|r| r.line.starts_with("GET /file/upload?"))
        .unwrap();
    assert!(!grant.line.contains("file_name="));
    assert_eq!(
        requests
            .iter()
            .find(|r| r.line.starts_with("PUT "))
            .unwrap()
            .body,
        b"original-config-image"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET /backup/detail?"))
            .count(),
        1
    );
}
#[test]
fn malformed_nested_selections_retail_defaults_and_update_markers_fail_at_their_business_boundary()
{
    let d = TestDir::new();
    for (field, v) in [
        ("known_addon_ids", json!([true])),
        ("wtf_role_ids", json!([ROLE_A, ROLE_B])),
        ("retail_ui_config", json!({"edit_mode_selectors":[EDIT_A]})),
        (
            "retail_ui_config",
            json!({"edit_mode_selectors":[EDIT_A,EDIT_A],"default_edit_mode_selector":EDIT_A}),
        ),
        ("retail_ui_config", json!({"enable_dd_setup_wizard":1})),
        ("retail_ui_config", json!({"raw_code":"private-secret"})),
    ] {
        let mut doc = create();
        doc[field] = v;
        let e = ConfigWrite::parse(
            WriteAction::Create,
            &serde_json::to_vec(&doc).unwrap(),
            &d.0,
            &OperationControl::default(),
        )
        .err()
        .unwrap();
        assert_eq!(e.exit_code, 4);
        assert!(!e.to_string().contains("private-secret"));
    }
    for doc in [
        json!({"schema":"lycheedev.publish.v1.dd.config.update","share_sn":"share-selected","backup_sn":"backup-one","update_desc":"bad","known_addon_update_ids":[2]}),
        json!({"schema":"lycheedev.publish.v1.dd.config.update","share_sn":"share-selected","backup_sn":"backup-one","update_desc":"bad","wtf_role_ids":["42"]}),
    ] {
        let request = parse(WriteAction::Update, &doc, &d);
        let server = Server::new(preflight(Some(stored()), backup("backup-one", 10001)));
        let home = d.0.join("home");
        let mut a = author(&server, &home);
        assert_eq!(run(&request, &mut a, None).unwrap_err().exit_code, 4);
        a.http.close().unwrap();
        assert!(bodies(&server.finish()).is_empty());
    }
}
