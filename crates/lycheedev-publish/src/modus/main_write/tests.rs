use super::*;
use crate::{
    modus::{leveldb::tests::control, tests::api},
    test_support::{Reply, Server, TestDir},
};
fn ok(value: Value) -> Reply {
    Reply::Json(json!({"code":200,"data":value}))
}
fn account() -> Reply {
    ok(json!({"user":{"userId":7}}))
}
fn list(ids: &[&str]) -> Reply {
    ok(json!({"total":ids.len(),"rows":ids.iter().map(|id|json!({"id":id})).collect::<Vec<_>>()}))
}
fn request(resource: &str, action: &str, mut doc: Value) -> MainWrite {
    doc["schema"] = json!(format!("lycheedev.publish.v1.modus.{resource}.{action}"));
    MainWrite::parse(resource, action, &serde_json::to_vec(&doc).unwrap()).unwrap()
}
fn detail(config: bool, id: &str) -> Value {
    let mut value = json!({"id":id,"title":"Original author title","content":"<p>Original author description with enough characters.</p>","imageUrl":"original-image","isPaid":0,"isPublic":0,"price":0,"shareType":0,"tags":"1","subType":0,"platform":1,"synchronizationType":1,"requiredTierId":5,"addonsId":"22"});
    if config {
        value["backupId"] = json!(719);
        value["excludeWtf"] = json!(0);
        value["accountName"] = json!("AuthorAccount");
        value["roleName"] = json!("Realm-Role");
    } else {
        value["codeText"] = json!("!WA:2!private-original");
        value["filePath"] = json!("");
        value["supportAddon"] = json!("WeakAuras");
        value["version"] = json!("1.0.0");
        value["versionList"] = json!([]);
    }
    value
}
#[test]
fn sparse_config_update_rebuilds_form_and_unknown_reply_recovers_in_original_build() {
    let dir = TestDir::new();
    let request = request(
        "config",
        "edit",
        json!({"share_id":"9007199254740993","content":"<p>Updated &amp; sufficiently long author text.</p>","required_tier_id":null,"exclude_wtf":1}),
    );
    let current = detail(true, "9007199254740993");
    let server = Server::new(vec![
        account(),
        ok(json!([current])),
        ok(json!([{"id":719,"serverType":3,"knownAddons":"{\"projectids\":\"22\"}"}])),
        Reply::Drop,
    ]);
    let error = request
        .run(&dir.0, &api(&server, Client::Main), &control())
        .unwrap_err();
    assert_eq!(error.details["submission"], "uncertain");
    let captured = server.finish();
    assert!(
        captured[3]
            .line
            .starts_with("PUT /api/system/user/share/update ")
    );
    let wire: Value = serde_json::from_slice(&captured[3].body).unwrap();
    assert_eq!(wire["isPublic"], 0);
    assert_eq!(wire["backupId"], 719);
    assert_eq!(wire["imageUrl"], "original-image");
    assert_eq!(wire["accountName"], "");
    assert_eq!(wire["roleName"], "");
    assert_eq!(wire["excludeWtf"], 1);
    assert!(wire.get("requiredTierId").is_none());
    assert_eq!(
        wire["contentText"],
        "Updated & sufficiently long author text."
    );
    let mut after = wire.clone();
    after.as_object_mut().unwrap().remove("contentText");
    let server = Server::new(vec![account(), ok(json!([after]))]);
    let mut changed = api(&server, Client::Main);
    changed.session.as_mut().unwrap().build = Some(0);
    let result = request.run(&dir.0, &changed, &control()).unwrap();
    assert_eq!(result["verified"], true);
    assert_eq!(result["recovered"], true);
    assert_eq!(result["server_type"], 3);
    let captured = server.finish();
    assert!(captured[1].headers.contains("x-server-type: 3"));
    assert!(!result.to_string().contains("Updated &"));
    let state_root = dir.0.join("publish/modus/records/main-config");
    let state = std::fs::read_to_string(
        std::fs::read_dir(state_root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
            .join("active.json"),
    )
    .unwrap();
    assert!(!state.contains("sufficiently"));
}
#[test]
fn main_creation_recovers_unique_new_id_without_republishing_or_exposing_wa() {
    let dir = TestDir::new();
    let input = request(
        "wa",
        "create",
        json!({"title":"Original author title","content":"<p>Original author description with enough characters.</p>","image_url":"original-image","tags":"1","addons_id":"22","support_addon":"WeakAuras","version":"1.0.0","code_text":"!WA:2!private-original"}),
    );
    let server = Server::new(vec![
        account(),
        list(&["700"]),
        ok(json!([{"id":1}])),
        ok(json!([{"id":22,"name":"WeakAuras"}])),
        Reply::Drop,
    ]);
    assert_eq!(
        input
            .run(&dir.0, &api(&server, Client::Main), &control())
            .unwrap_err()
            .details["submission"],
        "uncertain"
    );
    let captured = server.finish();
    assert!(
        captured[4]
            .line
            .starts_with("POST /api/system/user/import/create ")
    );
    let wire: Value = serde_json::from_slice(&captured[4].body).unwrap();
    assert_eq!(wire["isPublic"], 1);
    assert_eq!(wire["filePath"], "");
    assert_eq!(wire["synchronizationType"], 1);
    assert_eq!(wire["codeText"], "!WA:2!private-original");
    let mut after = detail(false, "9007199254740993");
    after["requiredTierId"] = Value::Null;
    after["isPublic"] = json!(1);
    let server = Server::new(vec![
        account(),
        list(&["700", "9007199254740993"]),
        ok(json!([after])),
    ]);
    let result = input
        .run(&dir.0, &api(&server, Client::Main), &control())
        .unwrap();
    assert_eq!(result["id"], "9007199254740993");
    assert_eq!(result["verified"], true);
    assert!(!result.to_string().contains("private-original"));
    assert_eq!(server.finish().len(), 3);
    let server = Server::new(vec![account()]);
    assert_eq!(
        input
            .run(&dir.0, &api(&server, Client::Main), &control())
            .unwrap()["reused"],
        true
    );
    server.finish();
}
#[test]
fn configuration_linkage_checks_exact_backup_build_addons_account_and_role() {
    let d = TestDir::new();
    let input = request(
        "config",
        "create",
        json!({"title":"Author configuration","content":"<p>A complete author description with enough characters.</p>","image_url":"image-key","tags":"1","addons_id":"22,23","backup_id":719,"exclude_wtf":0,"account_name":"SelectedAccount","role_name":"Realm-Role"}),
    );
    let backup = json!({"id":719,"serverType":3,"knownAddons":"{\"projectids\":\"22,23\"}","wtfAccounts":"[{\"id\":\"SelectedAccount\",\"roles\":[\"Realm-Role\"]}]"});
    let mut after = detail(true, "720");
    after["title"] = json!("Author configuration");
    after["content"] = json!("<p>A complete author description with enough characters.</p>");
    after["imageUrl"] = json!("image-key");
    after["addonsId"] = json!("22,23");
    after["accountName"] = json!("SelectedAccount");
    after["requiredTierId"] = Value::Null;
    after["isPublic"] = json!(1);
    let server = Server::new(vec![
        account(),
        list(&[]),
        ok(json!([{"id":1}])),
        ok(json!([backup])),
        ok(json!({"id":"720"})),
        ok(json!([after])),
    ]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap()["verified"],
        true
    );
    server.finish();
    let update = request(
        "config",
        "update",
        json!({"share_id":"720","backup_id":721,"addons_id":"22","exclude_wtf":0,"account_name":"WrongAccount","role_name":"Realm-Role"}),
    );
    let server = Server::new(vec![
        account(),
        ok(json!([detail(true, "720")])),
        ok(json!([{"id":721,"serverType":0}])),
    ]);
    assert_eq!(
        update
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap_err()
            .details["path"],
        "$.backup_id"
    );
    server.finish();
    let server = Server::new(vec![
        account(),
        ok(json!([detail(true, "720")])),
        ok(
            json!([{"id":721,"serverType":3,"knownAddons":"{\"projectids\":\"22\"}","wtfAccounts":[{"id":"SelectedAccount","roles":["Realm-Role"]}]}]),
        ),
    ]);
    assert_eq!(
        update
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap_err()
            .details["path"],
        "$.account_name"
    );
    server.finish();
}
#[test]
fn sparse_wa_post_preserves_paired_support_and_checks_current_season() {
    let d = TestDir::new();
    let input = request(
        "wa",
        "edit",
        json!({"import_id":"719","support_addon":"WeakAuras","required_tier_id":9,"sub_type":1}),
    );
    let mut after = detail(false, "719");
    after["requiredTierId"] = json!(9);
    after["subType"] = json!(1);
    let server = Server::new(vec![
        account(),
        ok(json!([detail(false, "719")])),
        ok(json!([{"gameCode":"wow_classic_titan"}])),
        ok(json!({"id":9,"isEnabled":1,"platformSeasonStatus":"1"})),
        ok(json!([{"id":22,"name":"WeakAuras"}])),
        ok(json!({})),
        ok(json!([after])),
    ]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap()["verified"],
        true
    );
    let requests = server.finish();
    assert!(
        requests[3]
            .line
            .contains("authorId=7&gameType=wow_classic_titan")
    );
    assert!(!requests[3].headers.contains("x-server-type"));
    assert!(
        requests[5]
            .line
            .starts_with("POST /api/system/user/import/update ")
    );
    let wire: Value = serde_json::from_slice(&requests[5].body).unwrap();
    assert_eq!(wire["codeText"], "!WA:2!private-original");
    assert_eq!(wire["isPublic"], 0);
    assert_eq!(wire["addonsId"], "22");
}
#[test]
fn version_publish_and_history_delete_recover_through_original_parent_without_replay() {
    let d = TestDir::new();
    let input = request(
        "wa",
        "version-publish",
        json!({"import_id":"719","version":" 1.0.1 ","code_text":" !WA:2!private-new ","changelog":" Fixed layout "}),
    );
    let server = Server::new(vec![
        account(),
        ok(json!([detail(false, "719")])),
        Reply::Drop,
    ]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap_err()
            .details["submission"],
        "uncertain"
    );
    let captured = server.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&captured[2].body).unwrap(),
        json!({"importId":"719","version":"1.0.1","codeText":"!WA:2!private-new","changelog":"Fixed layout"})
    );
    let mut after = detail(false, "719");
    after["version"] = json!("1.0.2");
    after["codeText"] = json!("later-version");
    after["versionList"] = json!([{"versionId":"9007199254740993","version":"1.0.1","codeText":"!WA:2!private-new","changelog":"Fixed layout"}]);
    let server = Server::new(vec![account(), ok(json!([after.clone()]))]);
    let result = input
        .run(&d.0, &api(&server, Client::Main), &control())
        .unwrap();
    assert_eq!(result["version_id"], "9007199254740993");
    server.finish();
    let delete = request(
        "wa",
        "version-delete",
        json!({"version_id":"9007199254740993","confirm":"DELETE"}),
    );
    let server = Server::new(vec![
        account(),
        list(&["719"]),
        ok(json!([after.clone()])),
        Reply::Drop,
    ]);
    assert_eq!(
        delete
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap_err()
            .details["submission"],
        "uncertain"
    );
    let captured = server.finish();
    assert!(
        captured[3].line.starts_with(
            "DELETE /api/system/user/import/version/delete?versionId=9007199254740993 "
        )
    );
    assert!(captured[3].body.is_empty());
    after["versionList"] = json!([]);
    let server = Server::new(vec![account(), ok(json!([after]))]);
    assert_eq!(
        delete
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap()["deleted"],
        true
    );
    assert_eq!(server.finish().len(), 2);
}
#[test]
fn exact_detail_absence_proves_delete_and_malformed_or_still_visible_records_stay_pending() {
    let d = TestDir::new();
    let input = request(
        "config",
        "delete",
        json!({"share_id":"719","confirm":"DELETE"}),
    );
    let server = Server::new(vec![
        account(),
        ok(json!([detail(true, "719")])),
        ok(json!({})),
        ok(json!([detail(true, "719")])),
    ]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap_err()
            .exit_code,
        6
    );
    let captured = server.finish();
    assert!(
        captured[2]
            .line
            .starts_with("DELETE /api/system/user/share/delete/719 ")
    );
    assert!(captured[2].body.is_empty());
    let server = Server::new(vec![account(), ok(json!({}))]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap_err()
            .exit_code,
        6
    );
    server.finish();
    let server = Server::new(vec![account(), ok(json!([]))]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap()["deleted"],
        true
    );
    server.finish();
}
#[test]
fn local_main_admission_rejects_incomplete_choices_and_derives_changed_text() {
    for patch in [
        json!({"title":"short"}),
        json!({"content_text":"too short"}),
        json!({"content":null}),
        json!({"tags":"1,1"}),
        json!({"image_url":"one,,two"}),
        json!({"price":-0.01}),
        json!({"price":1}),
        json!({"is_paid":1}),
        json!({"platform":3}),
        json!({"platform":3,"synchronization_type":1}),
        json!({"platform":3,"synchronization_type":3,"required_tier_id":5}),
        json!({"exclude_wtf":1,"account_name":"Account"}),
    ] {
        let mut doc = patch;
        doc["schema"] = json!("lycheedev.publish.v1.modus.config.edit");
        doc["share_id"] = json!("719");
        assert!(
            MainWrite::parse("config", "edit", &serde_json::to_vec(&doc).unwrap()).is_err(),
            "{doc}"
        );
    }
    let input = request("config", "edit", json!({"share_id":"719","backup_id":721}));
    assert!(form::form(&input.doc, true, Some(&detail(true, "719"))).is_err());
    let input = request(
        "wa",
        "edit",
        json!({"import_id":"719","title":"Changed title"}),
    );
    let mut bad = detail(false, "719");
    bad["contentText"] = json!(123);
    assert!(form::form(&input.doc, false, Some(&bad)).is_err());
    assert_eq!(input.plan()["credential_reads"], false);
}
