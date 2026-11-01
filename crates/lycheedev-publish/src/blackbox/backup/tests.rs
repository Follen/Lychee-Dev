use super::super::tests::{api as fixture_api, ok};
use super::*;
use crate::test_support::{Reply, Server, TestDir};
use std::time::{Duration, Instant};
fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn row(id: &str) -> Value {
    json!({"ID":id,"Name":"author backup","SaveTime":1700000000,"IsClassic":false})
}
fn request(action: &str, mut doc: Value) -> BackupWrite {
    doc["schema"] = json!(format!("lycheedev.publish.v1.blackbox.backup.{action}"));
    BackupWrite::parse(action, &serde_json::to_vec(&doc).unwrap()).unwrap()
}
#[test]
fn preview_resolves_share_bindings_and_failed_pagination_never_authorizes_delete() {
    let dir = TestDir::new();
    let c = control();
    let preview = request(
        "delete-preview",
        json!({"name_contains":"author","created_before":1800000000}),
    );
    let server = Server::new(vec![
        ok(json!({"backupList":[row("backup-a")]})),
        ok(json!({"shares":[{"ShareID":"share"}]})),
        ok(json!({"shares":[]})),
        ok(json!({"ShareID":"share","BackupID":"backup-a"})),
    ]);
    let (api, mut session) = fixture_api(&server);
    let result = preview.run(&dir.0, &api, &mut session, &c).unwrap();
    assert_eq!(result["allowed"], false);
    assert_eq!(result["items"][0]["referenced_by"], json!(["share"]));
    assert_eq!(server.finish().len(), 4);
    let delete = request(
        "delete",
        json!({"backup_ids":["backup-a"],"confirm_delete":true}),
    );
    let server = Server::new(vec![
        ok(json!({"backupList":[row("backup-a")]})),
        ok(json!({"shares":[{"ShareID":"first"}]})),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&server);
    let error = delete.run(&dir.0, &api, &mut session, &c).unwrap_err();
    assert_eq!(error.details["absence_proven"], false);
    assert!(
        server
            .finish()
            .iter()
            .all(|r| !r.line.contains("remove_user_configuration_backup"))
    );
}
#[test]
fn batch_deletion_retains_completed_items_and_reads_unknown_original_without_expanding_selection() {
    let dir = TestDir::new();
    let c = control();
    let req = request(
        "delete",
        json!({"backup_ids":["a","b"],"confirm_delete":true}),
    );
    let server = Server::new(vec![
        ok(json!({"backupList":[row("a"),row("b")]})),
        ok(json!({"shares":[]})),
        ok(json!({"backupList":[row("a"),row("b")]})),
        ok(json!({"shares":[]})),
        ok(json!({})),
        ok(json!({"backupList":[row("b")]})),
        ok(json!({"backupList":[row("b")]})),
        ok(json!({"shares":[]})),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&server);
    let error = req.run(&dir.0, &api, &mut session, &c).unwrap_err();
    assert_eq!(error.details["items"][0]["state"], "verified");
    assert_eq!(error.details["items"][1]["state"], "submitted");
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.line.contains("remove_user_configuration_backup"))
            .count(),
        2
    );
    let server = Server::new(vec![ok(json!({"backupList":[row("new-backup")]}))]);
    let (api, mut session) = fixture_api(&server);
    let result = req.run(&dir.0, &api, &mut session, &c).unwrap();
    assert_eq!(result["items"].as_array().unwrap().len(), 2);
    assert_eq!(result["items"][1]["state"], "verified");
    assert_eq!(server.finish().len(), 1);
    assert_eq!(
        req.run(&dir.0, &api, &mut session, &c).unwrap()["reused"],
        true
    );
}
#[test]
fn rename_unknown_response_recovers_exact_opaque_id_and_name() {
    let dir = TestDir::new();
    let c = control();
    let req = request(
        "rename",
        json!({"backup_id":"9007199254740993","name":"中文备份"}),
    );
    let first = Server::new(vec![
        ok(json!({"backupList":[row("9007199254740993")]})),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&first);
    assert!(req.run(&dir.0, &api, &mut session, &c).is_err());
    assert!(
        first.finish()[1]
            .line
            .contains("backup_id=9007199254740993")
    );
    let mut renamed = row("9007199254740993");
    renamed["Name"] = json!("中文备份");
    let second = Server::new(vec![ok(json!({"backupList":[renamed]}))]);
    let (api, mut session) = fixture_api(&second);
    assert_eq!(
        req.run(&dir.0, &api, &mut session, &c).unwrap()["verified"],
        true
    );
    assert_eq!(second.finish().len(), 1);
}
