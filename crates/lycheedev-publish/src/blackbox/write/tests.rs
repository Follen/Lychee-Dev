use super::super::tests::{api as fixture_api, ok};
use super::*;
use crate::test_support::{Reply, Server, TestDir};
use std::time::{Duration, Instant};
fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn request(resource: &str, action: &str, mut input: Value, root: &Path) -> Write {
    input["schema"] = json!(format!("lycheedev.publish.v1.blackbox.{resource}.{action}"));
    Write::parse(resource, action, &serde_json::to_vec(&input).unwrap(), root).unwrap()
}
fn form(request: &crate::test_support::Request) -> Value {
    let url = reqwest::Url::parse(&format!(
        "https://local.invalid/?{}",
        String::from_utf8_lossy(&request.body)
    ))
    .unwrap();
    let value = url
        .query_pairs()
        .find(|(k, _)| k == "configuration")
        .unwrap()
        .1
        .into_owned();
    serde_json::from_str(&value).unwrap()
}
fn backup() -> Value {
    json!({"ID":"original-backup","IsClassic":false,"KnownAddons":[{"ID":719,"Name":"Addon","VersionID":17,"Strategy":1}],"WTFList":[{"Name":"ACCOUNT","RealmCharacters":[{"Realm":"Realm","Player":"Hero"}],"Characters":["Realm - Hero"],"Path":"account-original","Host":"wow.max-c.com","MD5":"original-md5","Layout":{"main":[1,2]}}],"UnknownAddons":{"Files":[{"Name":"Extra","Size":14}],"Host":"wow.max-c.com","Path":"archive-original","MD5":"archive-md5"}})
}
fn patch() -> Value {
    json!({"Title":"Original title","Article":"<p>author body</p>","Open":false,"ThumbList":[],"SubscribeLevel":0,"KnownAddons":[{"ID":719,"Name":"Addon","VersionID":17,"Strategy":1}],"WTFList":[],"UnknownAddons":{"Files":[{"Name":"Extra","Size":14}]},"Materials":{"Files":[]},"Fonts":{"Files":[]}})
}
#[test]
fn configuration_preserves_original_archives_roles_false_zero_and_rejects_unselected_content() {
    let backup = backup();
    let created = forms::config_form(&patch(), &backup, None).unwrap();
    assert_eq!(created["UnknownAddons"]["Path"], "archive-original");
    assert_eq!(created["UnknownAddons"]["MD5"], "archive-md5");
    assert!(created.get("Materials").is_none());
    assert_eq!(created["ClearFonts"], false);
    let mut current = created.clone();
    current["ShareID"] = json!("share");
    current["BackupID"] = json!("original-backup");
    current["SubscribeLevel"] = json!({"level":7});
    current["WTFList"] = backup["WTFList"].clone();
    let edited = forms::config_form(
        &json!({"Title":"New","Open":false,"SubscribeLevel":0,"Article":""}),
        &backup,
        Some(&current),
    )
    .unwrap();
    assert_eq!(edited["WTFList"], backup["WTFList"]);
    assert_eq!(edited["UnknownAddons"]["Host"], "wow.max-c.com");
    assert_eq!(edited["Open"], false);
    assert_eq!(edited["SubscribeLevel"], 0);
    assert_eq!(edited["Article"], "");
    let mut other = backup.clone();
    other["ID"] = json!("other-backup");
    assert!(forms::config_form(&json!({"Title":"New"}), &other, Some(&current)).is_err());
    let mut bad = patch();
    bad["KnownAddons"] = json!([{ "ID":720 }]);
    assert!(forms::config_form(&bad, &backup, None).is_err());
    let mut bad = patch();
    bad["WTFList"] =
        json!([{ "Name":"ACCOUNT","RealmCharacters":[{"Realm":"Realm","Player":"Unselected"}] }]);
    assert!(forms::config_form(&bad, &backup, None).is_err());
    let mut bad = patch();
    bad["Title"] = json!("😀".repeat(18));
    assert!(forms::validate_config(&bad, true).is_err());
    bad["Title"] = json!("😀".repeat(17));
    assert!(forms::validate_config(&bad, true).is_ok());
}
#[test]
fn config_creation_recovers_lost_ack_by_full_original_fields_without_resubmitting() {
    let dir = TestDir::new();
    let control = control();
    let backup = backup();
    let patch = patch();
    let req = request(
        "config",
        "create",
        json!({"backup_id":"original-backup","configuration":patch}),
        &dir.0,
    );
    let first = Server::new(vec![
        ok(json!({"backupList":[backup.clone()]})),
        ok(json!({"shares":[]})),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&first);
    let error = req.run(&dir.0, &api, &mut session, &control).unwrap_err();
    assert_eq!(error.details["submission"], "uncertain");
    let sent = first.finish();
    assert_eq!(
        sent.iter()
            .filter(|r| r.line.contains("create_config_share"))
            .count(),
        1
    );
    let mut actual = form(&sent[2]);
    assert_eq!(actual["UnknownAddons"]["Path"], "archive-original");
    actual["ShareID"] = json!("share-created");
    actual["BackupID"] = json!("original-backup");
    actual["SubscribeLevel"] = json!({"level":0});
    let second = Server::new(vec![
        ok(json!({"shares":[{"ShareID":"share-created"}]})),
        ok(json!({"shares":[]})),
        ok(actual),
    ]);
    let (api, mut session) = fixture_api(&second);
    let result = req.run(&dir.0, &api, &mut session, &control).unwrap();
    assert_eq!(result["id"], "share-created");
    assert_eq!(result["recovered"], true);
    assert!(
        second
            .finish()
            .iter()
            .all(|r| !r.line.contains("create_config_share"))
    );
    let result = req.run(&dir.0, &api, &mut session, &control).unwrap();
    assert_eq!(result["reused"], true);
}
fn wa_row() -> Value {
    json!({"Slug":"wa-original","HeyboxId":"123","Name":"Original","Description":"<p>preserved</p>","UpdateLog":"log","Categories":["raid"],"Encoded":"!WA:2!private-original","ThumbList":["https://wow.max-c.com/image.png"],"AuditState":0})
}
fn wa_summary() -> Value {
    json!({"Slug":"wa-original","HeyboxId":"123","LatestVersion":{"Version":"v1"}})
}
#[test]
fn wa_edit_recovers_original_import_and_image_metadata_without_secret_records_or_replay() {
    let dir = TestDir::new();
    let control = control();
    let req = request(
        "wa",
        "edit",
        json!({"slug":"wa-original","scope":"public","configuration":{"strappid":"wowretail","open":false,"subscribeLevel":0,"name":"Edited"}}),
        &dir.0,
    );
    let first = Server::new(vec![
        ok(json!({"heybox_id":"123"})),
        ok(json!({"waList":[wa_summary()]})),
        ok(json!({"waList":[]})),
        ok(wa_row()),
        ok(json!({"list":[{"TagList":[{"Key":"raid"}]}]})),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&first);
    assert!(req.run(&dir.0, &api, &mut session, &control).is_err());
    let sent = first.finish();
    let submitted = form(&sent[5]);
    assert_eq!(submitted["encoded"], "!WA:2!private-original");
    assert_eq!(submitted["descriptionHeybox"], "<p>preserved</p>");
    assert_eq!(submitted["open"], false);
    assert_eq!(submitted["subscribeLevel"], 0);
    assert_eq!(
        submitted["screens"][0]["thumb"],
        "https://wow.max-c.com/image.png"
    );
    let mut current = wa_row();
    current["Name"] = json!("Edited");
    let second = Server::new(vec![
        ok(json!({"heybox_id":"123"})),
        ok(json!({"waList":[wa_summary()]})),
        ok(json!({"waList":[]})),
        ok(current),
    ]);
    let (api, mut session) = fixture_api(&second);
    let result = req.run(&dir.0, &api, &mut session, &control).unwrap();
    assert_eq!(result["scope"], "public");
    assert_eq!(result["visibility"]["verified"], false);
    assert_eq!(
        result["unverified_fields"],
        json!(["subscribeLevel", "screens.thumb", "open"])
    );
    assert!(!result.to_string().contains("private-original"));
    assert!(
        second
            .finish()
            .iter()
            .all(|r| !r.line.contains("update_wa"))
    );
    for entry in std::fs::read_dir(dir.0.join("publish/blackbox/records/wa")).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path().join("active.json")).unwrap();
        assert!(!text.contains("private-original"));
    }
    for value in [
        json!({"scope":"private","configuration":{}}),
        json!({"scope":"public","configuration":{"strappid":"wowretail","open":false}}),
    ] {
        let mut value = value;
        value["schema"] = json!("lycheedev.publish.v1.blackbox.wa.create");
        assert!(
            Write::parse("wa", "create", &serde_json::to_vec(&value).unwrap(), &dir.0).is_err()
        );
    }
}
#[test]
fn module_edit_freezes_omitted_fields_and_keeps_changed_readback_pending() {
    let dir = TestDir::new();
    let control = control();
    let req = request(
        "plugin",
        "edit",
        json!({"id":719,"desc":"changed","category_ids":[],"official":""}),
        &dir.0,
    );
    let current = json!({"id":719,"name":"Addon","logoUrl":"logo","categoryIds":[1],"type":1,"desc":"before","official":"website","officialUrl":"https://example.invalid","coreFolders":["Addon","Lib"]});
    let first = Server::new(vec![ok(json!({"module":current.clone()})), Reply::Drop]);
    let (api, mut session) = fixture_api(&first);
    assert!(req.run(&dir.0, &api, &mut session, &control).is_err());
    let sent = first.finish();
    let wire = String::from_utf8_lossy(&sent[1].body);
    assert!(wire.contains("coreFolders=Addon%2CLib"));
    assert!(wire.contains("official=&"));
    assert!(!wire.contains("categoryIds="));
    let mut actual = current.clone();
    actual["desc"] = json!("changed");
    actual["official"] = json!("");
    actual["categoryIds"] = json!([]);
    actual["name"] = json!("concurrent edit");
    let second = Server::new(vec![ok(json!({"module":actual.clone()}))]);
    let (api, mut session) = fixture_api(&second);
    assert_eq!(
        req.run(&dir.0, &api, &mut session, &control)
            .unwrap_err()
            .exit_code,
        6
    );
    assert_eq!(second.finish().len(), 1);
    actual["name"] = json!("Addon");
    let third = Server::new(vec![ok(json!({"module":actual}))]);
    let (api, mut session) = fixture_api(&third);
    assert_eq!(
        req.run(&dir.0, &api, &mut session, &control).unwrap()["verified"],
        true
    );
    assert_eq!(third.finish().len(), 1);
}
#[test]
fn original_zip_native_put_and_lost_version_ack_recover_without_reupload() {
    let dir = TestDir::new();
    let control = control();
    let zip = dir.zip();
    let req = request(
        "plugin",
        "update",
        json!({"module_id":719,"name":"1.0","type":1,"game_versions":["12.0.0"],"file":zip}),
        &dir.0,
    );
    let options = json!([{"strappid":"wowretail","gameVersions":["12.0.0"]}]);
    let first = Server::new(vec![
        ok(json!({"versionList":[],"totalCount":0,"strappidInfos":options})),
        ok(
            json!({"info":{"region":"ap-shanghai","bucket":"fixture-123","host":"wow.max-c.com","Credentials":{"TmpSecretID":"fixtureKey","TmpSecretKey":"fixture-secret","Token":"fixture-token"}},"files":[{"key":"wow/module/original.zip"}]}),
        ),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&first);
    assert!(req.run(&dir.0, &api, &mut session, &control).is_err());
    let sent = first.finish();
    assert_eq!(sent[2].body, std::fs::read(&zip).unwrap());
    assert!(
        sent[2]
            .headers
            .to_ascii_lowercase()
            .contains("content-md5:")
    );
    assert!(sent[2].headers.contains("q-signature="));
    let object = Server::new(vec![
        Reply::Bytes(std::fs::read(&zip).unwrap()),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&object);
    assert!(req.run(&dir.0, &api, &mut session, &control).is_err());
    let readback = object.finish();
    assert!(readback[0].line.starts_with("GET /wow/module/original.zip"));
    assert!(readback[1].line.contains("module_version/upsert"));
    let second = Server::new(vec![ok(
        json!({"versionList":[{"id":18,"moduleId":719,"name":"1.0","type":1,"gameVersions":"12.0.0","fileUrlHeybox":"https://wow.max-c.com/wow/wow_module_file/wowretail/0123456789abcdef0123456789abcdef.zip"}],"totalCount":1,"strappidInfos":options}),
    )]);
    let (api, mut session) = fixture_api(&second);
    let result = req.run(&dir.0, &api, &mut session, &control).unwrap();
    assert_eq!(result["version_id"], "18");
    assert_eq!(
        result["archive_verification"]["mode"],
        "server_canonicalization"
    );
    assert_eq!(result["archive_verification"]["content_verified"], false);
    assert_eq!(second.finish().len(), 1);
    assert_eq!(
        req.run(&dir.0, &api, &mut session, &control).unwrap()["reused"],
        true
    );
}
#[test]
fn owned_wa_review_blocks_delete_and_unknown_delete_only_reads_complete_absence() {
    let dir = TestDir::new();
    let control = control();
    let req = request(
        "wa",
        "delete",
        json!({"slug":"wa-original","strappid":"wowretail","confirm_delete":true}),
        &dir.0,
    );
    let mut reviewed = wa_row();
    reviewed["AuditState"] = json!(1);
    let first = Server::new(vec![
        ok(json!({"heybox_id":"123"})),
        ok(json!({"waList":[wa_summary()]})),
        ok(json!({"waList":[]})),
        ok(reviewed),
    ]);
    let (api, mut session) = fixture_api(&first);
    assert_eq!(
        req.run(&dir.0, &api, &mut session, &control)
            .unwrap_err()
            .code,
        "publish.blackbox_review"
    );
    assert_eq!(first.finish().len(), 4);
    let second = Server::new(vec![
        ok(json!({"heybox_id":"123"})),
        ok(json!({"waList":[wa_summary()]})),
        ok(json!({"waList":[]})),
        ok(wa_row()),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&second);
    assert!(req.run(&dir.0, &api, &mut session, &control).is_err());
    assert!(second.finish()[4].line.contains("delete=1"));
    let third = Server::new(vec![
        ok(json!({"heybox_id":"123"})),
        ok(json!({"waList":[]})),
    ]);
    let (api, mut session) = fixture_api(&third);
    assert_eq!(
        req.run(&dir.0, &api, &mut session, &control).unwrap()["deleted_id"],
        "wa-original"
    );
    assert_eq!(third.finish().len(), 2);
}
