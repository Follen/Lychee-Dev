use super::*;
use crate::{
    newbee::tests::{authenticated, provider},
    test_support::{Reply, Server, TestDir},
};
fn linked(version: &str) -> Value {
    json!([{"mod_id":31,"mod_name":"Known","mod_file_id":991,"mod_version":version,"display_name":null,"updateType":1}])
}
fn original() -> Value {
    json!({"t_id":719,"t_cloudblackid":99,"t_title":"UI","t_content":"Description","t_content_format":1,"t_intro":"short","pic_url":["https://cdn.invalid/ui.png"],"t_content_origin":2,"t_sharing":1,"t_link_to_channel":true,"t_subscribe_plan_level":0,"t_price":0,"t_time_range":"","t_linked_mods":linked("1.0"),"t_ignored_unknown_mods":[{"name":"Unknown"}],"t_ignored_materials":["Material"],"t_ignored_fronts":["Font"],"t_roleid":9007199254740993u64,"t_selected_eml":"[{\"name\":\"layout\",\"opaque\":{\"new_server_field\":[0,false,null]}}]"})
}
fn backup(version: &str) -> Value {
    json!({"t_id":99,"t_name":"Backup","t_Versionid":1,"t_Known_plug":[{"id":31,"name":"Known","mod_file_id":991,"mod_version":version,"display_name":null,"updateType":1}],"t_unKnown_list":[{"name":"Unknown"}],"t_material_list":[{"name":"Material"}],"t_font_list":["Font"],"wtflist":[{"account":"Fixture","server":[{"serverName":"Realm","roleList":[{"name":"Role","role_id":9007199254740993u64}]}]}]})
}
fn edit(doc: Value, dir: &TestDir) -> ConfigEdit {
    ConfigEdit::parse(&serde_json::to_vec(&doc).unwrap(), &dir.0).unwrap()
}
#[test]
fn configuration_full_form_keeps_exact_role_and_unknown_layout_objects() {
    let dir = TestDir::new();
    let mut changed = original();
    changed["t_title"] = json!("Corrected UI");
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":original()})),
        Reply::Json(json!({"code":1,"data":[backup("1.0")]})),
        Reply::Json(json!({"code":1})),
        Reply::Json(json!({"code":1,"data":changed})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let result=provider.edit_config(&edit(json!({"schema":"lycheedev.publish.v1.newbee.config.edit","id":719,"title":"Corrected UI"}),&dir)).unwrap();
    assert_eq!(result["author_writes"], 1);
    assert_eq!(result["readback"]["roleid"], "9007199254740993");
    let requests = server.finish();
    let body: Value = serde_json::from_slice(&requests[5].body).unwrap();
    assert_eq!(
        requests[5].line,
        "POST /creator/wow/share_config/update HTTP/1.1"
    );
    assert_eq!(
        body["selected_eml"],
        json!([{"name":"layout","opaque":{"new_server_field":[0,false,null]}}])
    );
    assert_eq!(body["tid"], 719);
    assert_eq!(body["linked_mods"], linked("1.0"));
    assert_eq!(body["ignored_unknown_mods"], json!(["Unknown"]));
    assert_eq!(
        body,
        json!({"tid":719,"cloud_id":99,"title":"Corrected UI","content":"Description","content_format":1,"intro":"short","pic_url":["https://cdn.invalid/ui.png"],"content_origin":2,"sharing":1,"link_to_channel":true,"subscribe_plan_level":0,"price":0,"time_range":"","linked_mods":linked("1.0"),"ignored_unknown_mods":["Unknown"],"ignored_materials":["Material"],"ignored_fronts":["Font"],"roleid":"9007199254740993","selected_eml":[{"name":"layout","opaque":{"new_server_field":[0,false,null]}}]})
    );
}
#[test]
fn configuration_backup_selection_uses_the_current_backup_plugin_identity() {
    let dir = TestDir::new();
    let mut changed = original();
    changed["t_linked_mods"] = linked("2.0");
    changed["t_ignored_unknown_mods"] = json!([]);
    changed["t_ignored_materials"] = json!([]);
    changed["t_ignored_fronts"] = json!([]);
    changed["t_roleid"] = json!("");
    let request=ConfigUpdate::parse(&serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.newbee.config.update","id":719,"cloud_id":99,"linked_mods":linked("stale"),"ignored_unknown_mods":[],"ignored_materials":[],"ignored_fronts":[],"roleid":""})).unwrap(),&dir.0).unwrap();
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":original()})),
        Reply::Json(json!({"code":1,"data":[backup("2.0")]})),
        Reply::Json(json!({"code":1})),
        Reply::Json(json!({"code":1,"data":changed})),
    ]));
    let mut provider = provider(&dir, &server, true);
    provider.update_config(&request).unwrap();
    let requests = server.finish();
    let body: Value = serde_json::from_slice(&requests[5].body).unwrap();
    assert_eq!(body["linked_mods"], linked("2.0"));
    assert_eq!(body["roleid"], "");
    assert_eq!(body["ignored_fronts"], json!([]));
    assert!(!String::from_utf8_lossy(&requests[5].body).contains("stale"));
}
#[test]
fn changing_cloud_requires_every_selection_and_rejects_unknown_selected_members() {
    let dir = TestDir::new();
    let missing =
        json!({"schema":"lycheedev.publish.v1.newbee.config.update","id":719,"cloud_id":100});
    assert_eq!(
        ConfigUpdate::parse(&serde_json::to_vec(&missing).unwrap(), &dir.0)
            .err()
            .unwrap()
            .details["path"],
        "$.linked_mods"
    );
    let mut bad = json!({"schema":"lycheedev.publish.v1.newbee.config.update","id":719,"linked_mods":linked("1.0")});
    bad["linked_mods"][0]["unexpected"] = json!(true);
    assert!(ConfigUpdate::parse(&serde_json::to_vec(&bad).unwrap(), &dir.0).is_err());
    let bad_selection = json!({"schema":"lycheedev.publish.v1.newbee.config.update","id":719,"ignored_materials":["Absent"]});
    let request =
        ConfigUpdate::parse(&serde_json::to_vec(&bad_selection).unwrap(), &dir.0).unwrap();
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":original()})),
        Reply::Json(json!({"code":1,"data":[backup("1.0")]})),
    ]));
    let mut provider = provider(&dir, &server, true);
    assert_eq!(
        provider.update_config(&request).unwrap_err().details["path"],
        "$.ignored_materials"
    );
    assert_eq!(server.finish().len(), 5);
}
#[test]
fn layout_loss_in_configuration_readback_stays_pending_until_original_form_is_visible() {
    let dir = TestDir::new();
    let mut missing = original();
    missing.as_object_mut().unwrap().remove("t_selected_eml");
    missing["t_title"] = json!("Changed");
    let mut correct = original();
    correct["t_title"] = json!("Changed");
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":original()})),
        Reply::Json(json!({"code":1,"data":[backup("1.0")]})),
        Reply::Json(json!({"code":1})),
        Reply::Json(json!({"code":1,"data":missing.clone()})),
        Reply::Json(json!({"code":1,"data":missing})),
        Reply::Json(json!({"code":1,"data":correct})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let request = edit(
        json!({"schema":"lycheedev.publish.v1.newbee.config.edit","id":719,"title":"Changed"}),
        &dir,
    );
    let error = provider.edit_config(&request).unwrap_err();
    assert_eq!(error.exit_code, 6);
    assert_eq!(provider.edit_config(&request).unwrap_err().exit_code, 6);
    let result = provider.edit_config(&request).unwrap();
    assert_eq!(result["recovered"], true);
    assert_eq!(result["author_writes"], 0);
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|request| request.line == "POST /creator/wow/share_config/update HTTP/1.1")
            .count(),
        1
    );
}
