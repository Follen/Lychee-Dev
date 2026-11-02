use super::*;
use crate::{
    modus::{leveldb::tests::control, tests::api},
    test_support::{Reply, Server, TestDir},
};
fn ok(v: Value) -> Reply {
    Reply::Json(json!({"code":200,"data":v}))
}
fn account() -> Reply {
    ok(json!({"user":{"userId":7}}))
}
fn detail(id: u64) -> Value {
    json!({"id":id,"serverType":0,"backupName":"Original"})
}
fn request(action: &str, mut doc: Value) -> BackupCloud {
    doc["schema"] = json!(format!("lycheedev.publish.v1.modus.backup.{action}"));
    BackupCloud::parse(action, &serde_json::to_vec(&doc).unwrap()).unwrap()
}
#[test]
fn rename_lost_response_reads_original_build_and_never_renames_again() {
    let d = TestDir::new();
    let input = request(
        "rename",
        json!({"backup_id":719,"backup_name":"Changed","server_type":0}),
    );
    let server = Server::new(vec![
        account(),
        ok(json!([detail(719)])),
        ok(detail(719)),
        Reply::Drop,
    ]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap_err()
            .exit_code,
        5
    );
    let r = server.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&r[3].body).unwrap(),
        json!({"id":719,"backupName":"Changed"})
    );
    let server = Server::new(vec![
        account(),
        ok(json!({"id":719,"serverType":0,"backupName":"Changed"})),
    ]);
    let result = input
        .run(&d.0, &api(&server, Client::Main), &control())
        .unwrap();
    assert_eq!(result["renamed"], true);
    assert_eq!(result["recovered"], true);
    let r = server.finish();
    assert!(r.iter().all(|r| r.line.starts_with("GET ")));
    assert!(r[1].headers.to_lowercase().contains("x-server-type: 0"));
    let server = Server::new(vec![
        account(),
        ok(json!([detail(719)])),
        ok(json!({"id":719,"serverType":0,"backupName":"Changed"})),
    ]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap()["reused"],
        true
    );
    assert!(server.finish().iter().all(|r| r.line.starts_with("GET ")));
}
#[test]
fn complete_references_across_builds_protect_backup_and_incomplete_totals_block_preview() {
    let d = TestDir::new();
    let input = request(
        "delete-preview",
        json!({"backup_ids":[719],"server_type":0}),
    );
    let mut replies = vec![
        ok(json!({"total":1,"rows":[{"id":"share"}]})),
        ok(json!([{"id":"share","backupId":719,"title":"Uses backup"}])),
    ];
    for _ in 1..5 {
        replies.push(ok(json!({"total":0,"rows":[]})));
    }
    replies.push(ok(
        json!([detail(719),{"id":720,"serverType":3,"backupName":"Other"}]),
    ));
    let server = Server::new(replies);
    let result = input
        .run(&d.0, &api(&server, Client::Main), &control())
        .unwrap();
    assert_eq!(result["items"][0]["deletable"], false);
    assert_eq!(result["items"][0]["references"][0]["share_id"], "share");
    let r = server.finish();
    for build in 0..5 {
        assert!(r.iter().any(|r| {
            r.headers
                .to_lowercase()
                .contains(&format!("x-server-type: {build}"))
        }));
    }
    assert!(!r.iter().any(|r| r.line.starts_with("DELETE ")));
    let server = Server::new(vec![ok(json!({"rows":[]}))]);
    assert!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .is_err()
    );
    assert_eq!(server.finish().len(), 1);
    let server = Server::new(vec![
        ok(json!({"total":1,"rows":[{"id":"share"}]})),
        ok(json!([])),
    ]);
    assert!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .is_err()
    );
    assert_eq!(server.finish().len(), 2);
}
#[test]
fn delete_batch_retains_done_ids_and_unknown_delete_reads_only_exact_absence() {
    let d = TestDir::new();
    let input = request(
        "delete",
        json!({"backup_ids":[719,720],"server_type":0,"confirm":"DELETE"}),
    );
    let mut replies = vec![
        account(),
        ok(json!([detail(719), detail(720)])),
        ok(detail(719)),
        ok(detail(720)),
        ok(detail(719)),
    ];
    for _ in 0..5 {
        replies.push(ok(json!({"total":0,"rows":[]})));
    }
    replies.extend([
        ok(json!(null)),
        ok(json!([detail(720)])),
        Reply::Json(json!({"code":500,"msg":"没有备份信息，获取失败"})),
        ok(detail(720)),
        Reply::Drop,
    ]);
    let server = Server::new(replies);
    let error = input
        .run(&d.0, &api(&server, Client::Main), &control())
        .unwrap_err();
    assert_eq!(error.exit_code, 6);
    assert_eq!(error.details["items"][0]["status"], "done");
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.line.starts_with("DELETE "))
            .count(),
        2
    );
    let server = Server::new(vec![account(), ok(json!([])), ok(json!([]))]);
    let result = input
        .run(&d.0, &api(&server, Client::Main), &control())
        .unwrap();
    assert_eq!(result["completed"], true);
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["deleted"] == true)
    );
    assert!(server.finish().iter().all(|r| r.line.starts_with("GET ")));
    let server = Server::new(vec![account()]);
    let result = input
        .run(&d.0, &api(&server, Client::Main), &control())
        .unwrap();
    assert_eq!(result["reused"], true);
    assert_eq!(server.finish().len(), 1);
}
#[test]
fn missing_detail_requires_native_absence_and_wrong_build_blocks_before_delete() {
    let server = Server::new(vec![
        Reply::Json(json!({"code":500,"msg":"permission failure"})),
        ok(json!({})),
        Reply::Raw("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
    ]);
    let a = api(&server, Client::Main);
    assert!(read::optional_detail(&a, "backup", "719", Some(0)).is_err());
    assert!(read::optional_detail(&a, "backup", "719", Some(0)).is_err());
    assert!(
        read::optional_detail(&a, "backup", "719", Some(0))
            .unwrap()
            .is_none()
    );
    server.finish();
    let d = TestDir::new();
    let input = request(
        "delete",
        json!({"backup_ids":[719],"server_type":0,"confirm":"DELETE"}),
    );
    let server = Server::new(vec![
        account(),
        ok(json!([{"id":719,"serverType":3}])),
        ok(json!({"id":719,"serverType":3})),
    ]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap_err()
            .exit_code,
        4
    );
    assert!(
        !server
            .finish()
            .iter()
            .any(|r| r.line.starts_with("DELETE "))
    );
}

#[test]
fn referenced_delete_stays_protected_and_resumes_after_actual_reference_removal() {
    let d = TestDir::new();
    let input = request(
        "delete",
        json!({"backup_ids":[719],"server_type":0,"confirm":"DELETE"}),
    );
    let mut replies = vec![
        account(),
        ok(json!([detail(719)])),
        ok(detail(719)),
        ok(detail(719)),
        ok(json!({"total":1,"rows":[{"id":"share"}]})),
        ok(json!([{"id":"share","backupId":719}])),
    ];
    for _ in 1..5 {
        replies.push(ok(json!({"total":0,"rows":[]})));
    }
    let server = Server::new(replies);
    let error = input
        .run(&d.0, &api(&server, Client::Main), &control())
        .unwrap_err();
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.details["items"][0]["status"], "protected");
    assert!(
        !server
            .finish()
            .iter()
            .any(|r| r.line.starts_with("DELETE "))
    );
    let mut replies = vec![account(), ok(detail(719))];
    for _ in 0..5 {
        replies.push(ok(json!({"total":0,"rows":[]})));
    }
    replies.extend([ok(json!(null)), ok(json!([])), ok(json!([]))]);
    let server = Server::new(replies);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Main), &control())
            .unwrap()["completed"],
        true
    );
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.line.starts_with("DELETE "))
            .count(),
        1
    );
}
