use super::*;
use crate::{
    newbee::tests::{authenticated, provider},
    test_support::{Reply, Server, TestDir},
};
fn row() -> Value {
    json!({"t_id":7,"t_name":"Backup","t_Versionid":2,"t_create_time":"2026-10-30T00:00:00Z","t_unKnown_list":[{"name":"Addon"}],"wtflist":[{"account":"Fixture","server":[{"serverName":"Realm","roleList":[{"name":"Role","role_id":9007199254740993u64}]}]}]})
}
fn clouds(rows: Vec<Value>) -> Reply {
    Reply::Json(json!({"code":1,"data":rows}))
}
fn no_refs() -> Reply {
    Reply::Json(json!({"code":1,"data":{"list":[],"total":0}}))
}
#[test]
fn preview_filters_intersect_ids_case_sensitive_names_and_utc_creation_dates_before_reference_checks()
 {
    let dir = TestDir::new();
    let mut earlier = row();
    earlier["t_id"] = json!(8);
    earlier["t_create_time"] = json!(1793145600000i64);
    let mut unrelated = row();
    unrelated["t_id"] = json!(9);
    unrelated["t_name"] = json!("Other");
    unrelated["t_create_time"] = json!("bad date outside name selection");
    let mut replies = vec![clouds(vec![row(), earlier, unrelated])];
    replies.extend(authenticated(vec![no_refs()]));
    let server = Server::new(replies);
    let mut provider = provider(&dir, &server, true);
    let selection = BackupSelection::new(
        vec![NonZeroU64::new(7).unwrap(), NonZeroU64::new(8).unwrap()],
        Some("Back".into()),
        Some("Backup".into()),
        Some("2026-10-31"),
        Some("2026-10-29T00:00:00+08:00"),
    )
    .unwrap();
    let result = provider
        .read_backup(&BackupRead::DeletePreview(selection))
        .unwrap();
    assert_eq!(result["matched"], 1);
    assert_eq!(result["items"][0]["id"], 7);
    assert_eq!(result["deletable"], true);
    assert_eq!(server.finish().len(), 5);
    assert!(BackupSelection::new(Vec::new(), None, None, Some("invalid"), None).is_err());
    assert!(BackupSelection::new(Vec::new(), None, None, None, None).is_err());
    let zero =
        BackupSelection::new(Vec::new(), None, None, None, Some("2026-10-30T00:00:00Z")).unwrap();
    assert!(zero.select(&[row()]).unwrap().is_empty());
    let mut bad = row();
    bad["t_create_time"] = json!("not a date");
    assert_eq!(zero.select(&[bad]).unwrap_err().exit_code, 5);
}
fn delete() -> BackupDelete {
    BackupDelete::parse(br#"{"schema":"lycheedev.publish.v1.newbee.backup.delete","backup_ids":[7],"confirm_delete":true}"#).unwrap()
}
#[test]
fn desktop_backup_queries_keep_opaque_configuration_data_out_and_role_ids_exact() {
    let dir = TestDir::new();
    let mut raw = row();
    raw["raw_wtf"] = json!("PRIVATE_WTF_DATA");
    raw["t_unKnown_plug"] = json!("https://objects.invalid/?signature=PRIVATE_CAPABILITY");
    let server = Server::new(vec![clouds(vec![raw.clone()]), clouds(vec![raw])]);
    let mut provider = provider(&dir, &server, true);
    let list = provider.read_backup(&BackupRead::List).unwrap();
    assert_eq!(list["items"][0]["unknown_plugins"], 1);
    let detail = provider
        .read_backup(&BackupRead::Get(NonZeroU64::new(7).unwrap()))
        .unwrap();
    assert_eq!(detail["roles"][0]["role_id"], "9007199254740993");
    assert!(!detail.to_string().contains("PRIVATE_"));
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.line == "POST /v3/share/list HTTP/1.1"
                && request.headers.contains("authorization: Bearer head.")
                && !request.headers.contains("token:"))
    );
}
#[test]
fn uncertain_backup_rename_reads_the_original_id_without_repeating_update() {
    let dir = TestDir::new();
    let mut changed = row();
    changed["t_name"] = json!("Renamed");
    let server = Server::new(vec![
        clouds(vec![row()]),
        Reply::Drop,
        clouds(vec![changed]),
    ]);
    let mut provider = provider(&dir, &server, true);
    let request = BackupRename::parse(
        br#"{"schema":"lycheedev.publish.v1.newbee.backup.rename","id":7,"name":"Renamed"}"#,
    )
    .unwrap();
    assert_eq!(
        provider.rename_backup(&request).unwrap_err().details["submission"],
        "uncertain"
    );
    let recovered = provider.rename_backup(&request).unwrap();
    assert_eq!(recovered["recovered"], true);
    assert_eq!(recovered["author_writes"], 0);
    let requests = server.finish();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[1].body).unwrap(),
        json!({"t_id":7,"t_name":"Renamed"})
    );
}
#[test]
fn a_referenced_backup_is_blocked_before_any_deletion() {
    let dir = TestDir::new();
    let mut replies = vec![clouds(vec![row()])];
    replies.extend(authenticated(vec![Reply::Json(json!({"code":1,"data":{"list":[{"t_id":719,"t_cloudblackid":7,"t_title":"Published UI"}],"total":1}}))]));
    let server = Server::new(replies);
    let mut provider = provider(&dir, &server, true);
    let error = provider.delete_backups(&delete()).unwrap_err();
    assert_eq!(error.code, "publish.backup_delete_blocked");
    assert_eq!(error.details["references_complete"], true);
    assert_eq!(error.details["items"][0]["references"][0]["id"], 719);
    assert_eq!(server.finish().len(), 5);
}
#[test]
fn missing_association_cannot_use_another_configurations_detail_as_reference_proof() {
    let dir = TestDir::new();
    let mut replies = vec![clouds(vec![row()])];
    replies.extend(authenticated(vec![
        Reply::Json(json!({"code":1,"data":{"list":[{"t_id":719}],"total":1}})),
        Reply::Json(json!({"code":1,"data":{"t_id":720,"t_cloudblackid":8}})),
    ]));
    let server = Server::new(replies);
    let mut provider = provider(&dir, &server, true);
    assert_eq!(
        provider.delete_backups(&delete()).unwrap_err().code,
        "publish.newbee_data"
    );
    assert_eq!(server.finish().len(), 6);
}
#[test]
fn a_reference_created_after_preview_blocks_the_actual_backup_delete() {
    let dir = TestDir::new();
    let mut replies = vec![clouds(vec![row()])];
    replies.extend(authenticated(vec![no_refs()]));
    replies.extend([
        clouds(vec![row()]),
        Reply::Json(json!({"code":1,"data":{"list":[{"t_id":719,"t_cloudblackid":7}],"total":1}})),
    ]);
    let server = Server::new(replies);
    let mut provider = provider(&dir, &server, true);
    let error = provider.delete_backups(&delete()).unwrap_err();
    assert_eq!(error.code, "publish.backup_delete_blocked");
    assert_eq!(error.details["completed"], json!([]));
    assert!(
        server
            .finish()
            .iter()
            .all(|request| !request.line.contains("/v3/share/delete"))
    );
}
#[test]
fn lost_backup_delete_response_is_resolved_by_absence_without_another_delete() {
    let dir = TestDir::new();
    let mut replies = vec![clouds(vec![row()])];
    replies.extend(authenticated(vec![no_refs()]));
    replies.extend([
        clouds(vec![row()]),
        no_refs(),
        Reply::Drop,
        clouds(vec![]),
        no_refs(),
        clouds(vec![]),
        no_refs(),
    ]);
    let server = Server::new(replies);
    let mut provider = provider(&dir, &server, true);
    let uncertain = provider.delete_backups(&delete()).unwrap_err();
    assert_eq!(uncertain.details["submission"], "uncertain");
    assert_eq!(uncertain.details["completed"], json!([]));
    let result = provider.delete_backups(&delete()).unwrap();
    assert_eq!(result["items"][0]["recovered"], true);
    assert_eq!(result["items"][0]["author_writes"], 0);
    let requests = server.finish();
    assert_eq!(requests.len(), 12);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line == "POST /v3/share/delete HTTP/1.1")
            .count(),
        1
    );
}
