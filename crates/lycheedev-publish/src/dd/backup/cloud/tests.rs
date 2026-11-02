use super::*;
use crate::{
    dd::{DdTransport, tests::transport},
    test_support::{Reply, Server, TestDir},
};
impl Api for DdTransport {
    fn submit(&mut self, p: &str, b: &Value) -> Result<Value> {
        self.post(p, &[], b, true)
    }
}
fn clock() -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":1793500000}}))
}
fn reply(v: Value) -> Reply {
    Reply::Json(json!({"code":0,"result":v}))
}
fn row(sn: &str) -> Value {
    json!({"sn":sn,"name":format!("backup-{sn}"),"game_type":10001,"ctime":1793500000})
}
fn rows(v: Value) -> Reply {
    let n = v.as_array().unwrap().len();
    reply(json!({"items":v,"total":n}))
}
fn backups(ids: &[&str]) -> Reply {
    rows(json!(ids.iter().map(|s| row(s)).collect::<Vec<_>>()))
}
fn games() -> Reply {
    reply(json!([{"game_type":10001}]))
}
fn empty_refs() -> Reply {
    rows(json!([]))
}
fn preview_clean(replies: &mut Vec<Reply>, ids: &[&str]) {
    replies.extend([backups(ids), games(), empty_refs()]);
}
fn remove(ids: &[&str]) -> BackupDelete {
    BackupDelete::parse(&serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.dd.backup.delete","backup_ids":ids,"confirm_delete":true})).unwrap()).unwrap()
}
fn rename(sn: &str, name: &str) -> BackupRename {
    BackupRename::parse(
        &serde_json::to_vec(
            &json!({"schema":"lycheedev.publish.v1.dd.backup.rename","sn":sn,"name":name}),
        )
        .unwrap(),
    )
    .unwrap()
}
fn run_delete(r: &BackupDelete, h: &Path, t: &mut DdTransport) -> Result<Value> {
    let c = OperationControl::default();
    t.begin_operation(&c)?;
    r.execute_with(h, "actor", t, &c)
}
fn run_rename(r: &BackupRename, h: &Path, t: &mut DdTransport) -> Result<Value> {
    let c = OperationControl::default();
    t.begin_operation(&c)?;
    r.execute_with(h, "actor", t, &c)
}
#[test]
fn preview_covers_live_and_retired_game_types_all_pages_and_exact_fallback_config_identity() {
    let sn = "精确,+%SN";
    let server = Server::new(vec![
        clock(),
        rows(json!([{"sn":sn,"game_type":10003,"name":"原备份"}])),
        reply(json!([{"game_type":10001},{"game_type":10002}])),
        reply(json!({"items":[{"share_sn":"config-one","backup_sn":"other"}],"total":2})),
        reply(json!({"items":[{"share_sn":"config-two","title":"配置"}],"total":2})),
        reply(json!({"sn":"config-two","share_sn":"config-two","backup_sn":sn})),
        empty_refs(),
        empty_refs(),
    ]);
    let (mut t, _) = transport(&server, "synthetic-token");
    let c = OperationControl::default();
    t.begin_operation(&c).unwrap();
    let s = BackupSelection::new(vec![sn.into()], None, None, None, None).unwrap();
    let p = s.execute_with(&mut t, &c).unwrap();
    assert_eq!(p["references_complete"], true);
    assert_eq!(p["deletable"], false);
    assert_eq!(p["items"][0]["references"][0]["sn"], "config-two");
    t.close().unwrap();
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET /share/list?"))
            .count(),
        4
    );
    assert!(requests.iter().any(|r| r.line.contains("game_type=10003")));
    assert!(requests.iter().all(|r| !r.line.starts_with("POST ")));
}
#[test]
fn incomplete_reference_counts_and_contradictory_fallback_ids_block_without_a_delete() {
    for bad_total in [true, false] {
        let d = TestDir::new();
        let r = remove(&["one"]);
        let mut replies = vec![clock(), backups(&["one"]), games()];
        if bad_total {
            replies.push(reply(json!({"items":[]})));
        } else {
            replies.push(rows(json!([{"sn":"config"}])));
            replies.push(reply(
                json!({"sn":"config","share_sn":"other","backup_sn":"one"}),
            ));
        }
        let server = Server::new(replies);
        let (mut t, _) = transport(&server, "synthetic-token");
        let e = run_delete(&r, &d.0, &mut t).unwrap_err();
        assert_eq!(e.exit_code, 5);
        assert_eq!(e.details["stage"], "backup_reference_check");
        t.close().unwrap();
        assert!(server.finish().iter().all(|r| !r.line.starts_with("POST ")));
    }
    let d = TestDir::new();
    let server = Server::new(vec![
        clock(),
        backups(&["one"]),
        games(),
        reply(json!({"items":[{"sn":"cfg","backup_sn":"other"}],"total":2})),
        reply(json!({"items":[],"total":2})),
    ]);
    let (mut t, _) = transport(&server, "synthetic-token");
    assert!(run_delete(&remove(&["one"]), &d.0, &mut t).is_err());
    t.close().unwrap();
    assert!(server.finish().iter().all(|r| !r.line.starts_with("POST ")));
}
#[test]
fn rename_lost_response_recovers_original_unicode_name_by_fresh_reads_without_repeating_post() {
    let d = TestDir::new();
    let r = rename("opaque,+%中文", "新备份😀");
    assert_eq!(r.plan()["remote_write"], false);
    let mut before = row("opaque,+%中文");
    before["name"] = json!("old");
    let mut after = before.clone();
    after["name"] = json!("新备份😀");
    let server = Server::new(vec![
        clock(),
        backups(&["opaque,+%中文"]),
        reply(before),
        Reply::Drop,
        backups(&["opaque,+%中文"]),
        reply(after.clone()),
        backups(&["opaque,+%中文"]),
        reply(after),
    ]);
    let (mut t, _) = transport(&server, "synthetic-token");
    let e = run_rename(&r, &d.0, &mut t).unwrap_err();
    assert_eq!(e.details["submission"], "uncertain");
    let recovered = run_rename(&r, &d.0, &mut t).unwrap();
    assert_eq!(recovered["submitted_this_call"], false);
    assert_eq!(recovered["operation_id"], e.details["operation_id"]);
    assert_eq!(run_rename(&r, &d.0, &mut t).unwrap()["name"], "新备份😀");
    t.close().unwrap();
    let requests = server.finish();
    let post = requests
        .iter()
        .filter(|r| r.line.starts_with("POST /backup/modify_name?"))
        .collect::<Vec<_>>();
    assert_eq!(post.len(), 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&post[0].body).unwrap(),
        json!({"sn":"opaque,+%中文","name":"新备份😀"})
    );
}
#[test]
fn accepted_rename_stays_pending_for_stale_name_and_rejects_another_name_before_network() {
    let d = TestDir::new();
    let r = rename("one", "new");
    let server = Server::new(vec![
        clock(),
        backups(&["one"]),
        reply(row("one")),
        reply(json!({})),
        backups(&["one"]),
        reply(row("one")),
        backups(&["one"]),
        reply(row("one")),
    ]);
    let (mut t, _) = transport(&server, "synthetic-token");
    let e = run_rename(&r, &d.0, &mut t).unwrap_err();
    assert_eq!(e.exit_code, 6);
    assert_eq!(
        run_rename(&rename("one", "other"), &d.0, &mut t)
            .unwrap_err()
            .details["operation_id"],
        e.details["operation_id"]
    );
    assert_eq!(run_rename(&r, &d.0, &mut t).unwrap_err().exit_code, 6);
    t.close().unwrap();
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.line.starts_with("POST "))
            .count(),
        1
    );
}
#[test]
fn a_new_reference_before_the_second_delete_preserves_first_success_and_blocks_the_batch() {
    let d = TestDir::new();
    let r = remove(&["one", "two"]);
    let mut replies = vec![clock()];
    preview_clean(&mut replies, &["one", "two"]);
    preview_clean(&mut replies, &["one", "two"]);
    replies.extend([
        reply(json!({})),
        backups(&["two"]),
        backups(&["two"]),
        games(),
        rows(json!([{"sn":"cfg-new","backup_sn":"two"}])),
    ]);
    let server = Server::new(replies);
    let (mut t, _) = transport(&server, "synthetic-token");
    let e = run_delete(&r, &d.0, &mut t).unwrap_err();
    assert_eq!(e.code, "publish.dd_backup_delete_blocked");
    assert_eq!(e.details["completed"][0]["sn"], "one");
    assert_eq!(e.details["completed"][0]["deleted"], true);
    assert_eq!(e.details["sn"], "two");
    t.close().unwrap();
    let requests = server.finish();
    let posts = requests
        .iter()
        .filter(|r| r.line.starts_with("POST /backup/delete?"))
        .collect::<Vec<_>>();
    assert_eq!(posts.len(), 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&posts[0].body).unwrap(),
        json!({"sn":"one"})
    );
}
#[test]
fn unknown_second_delete_recovers_only_absence_and_does_not_recheck_or_resubmit_completed_items() {
    let d = TestDir::new();
    let r = remove(&["one", "two"]);
    let mut replies = vec![clock()];
    preview_clean(&mut replies, &["one", "two"]);
    preview_clean(&mut replies, &["one", "two"]);
    replies.extend([reply(json!({})), backups(&["two"])]);
    preview_clean(&mut replies, &["two"]);
    replies.extend([Reply::Drop, backups(&[]), backups(&[])]);
    let server = Server::new(replies);
    let (mut t, _) = transport(&server, "synthetic-token");
    let first = run_delete(&r, &d.0, &mut t).unwrap_err();
    assert_eq!(first.details["submission"], "uncertain");
    assert_eq!(first.details["completed"].as_array().unwrap().len(), 1);
    assert_eq!(first.details["sn"], "two");
    let out = run_delete(&r, &d.0, &mut t).unwrap();
    assert_eq!(out["deleted"], 2);
    assert_eq!(
        out["items"][1]["operation_id"],
        first.details["operation_id"]
    );
    assert!(
        out["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["submitted_this_call"] == false)
    );
    assert_eq!(run_delete(&r, &d.0, &mut t).unwrap()["deleted"], 2);
    t.close().unwrap();
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("POST /backup/delete?"))
            .count(),
        2
    );
}
#[test]
fn accepted_but_present_delete_stays_pending_and_missing_without_original_intent_is_blocked() {
    let d = TestDir::new();
    let r = remove(&["one"]);
    let mut replies = vec![clock()];
    preview_clean(&mut replies, &["one"]);
    preview_clean(&mut replies, &["one"]);
    replies.extend([reply(json!({})), backups(&["one"]), backups(&["one"])]);
    let server = Server::new(replies);
    let (mut t, _) = transport(&server, "synthetic-token");
    let first = run_delete(&r, &d.0, &mut t).unwrap_err();
    assert_eq!(first.exit_code, 6);
    assert_eq!(
        run_delete(&r, &d.0, &mut t).unwrap_err().details["operation_id"],
        first.details["operation_id"]
    );
    t.close().unwrap();
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.line.starts_with("POST "))
            .count(),
        1
    );
    let d = TestDir::new();
    let mut replies = vec![clock()];
    preview_clean(&mut replies, &[]);
    let server = Server::new(replies);
    let (mut t, _) = transport(&server, "synthetic-token");
    assert_eq!(
        run_delete(&r, &d.0, &mut t).unwrap_err().code,
        "publish.dd_backup_delete_blocked"
    );
    t.close().unwrap();
    assert!(server.finish().iter().all(|r| !r.line.starts_with("POST ")));
}
#[test]
fn local_confirmation_ids_dates_and_exact_filter_bounds_are_admitted_before_task_effects() {
    for ids in [
        json!([]),
        json!(["one", "one"]),
        json!([1]),
        json!(["private-secret\n"]),
    ] {
        let e=BackupDelete::parse(&serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.dd.backup.delete","backup_ids":ids,"confirm_delete":true})).unwrap()).err().unwrap();
        assert_eq!(e.exit_code, 4);
        assert!(!e.to_string().contains("private-secret"));
    }
    assert!(BackupDelete::parse(br#"{"schema":"lycheedev.publish.v1.dd.backup.delete","backup_ids":["one"],"confirm_delete":false}"#).is_err());
    assert!(BackupSelection::new(vec![], None, None, None, None).is_err());
    assert!(
        BackupSelection::new(vec![], Some("x".into()), None, Some("not-a-date"), None).is_err()
    );
    let s = BackupSelection::new(
        vec![],
        Some("chosen".into()),
        None,
        Some("2026-11-01T00:00:00.500Z"),
        Some("2026-11-01T00:00:00Z"),
    )
    .unwrap();
    let base = DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
        .unwrap()
        .timestamp();
    let rows = vec![
        json!({"sn":"at","name":"chosen","ctime":base}),
        json!({"sn":"inside","name":"chosen","ctime":serde_json::from_str::<Value>(&format!("{base}.499999999")).unwrap()}),
        json!({"sn":"boundary","name":"chosen","ctime":serde_json::from_str::<Value>(&format!("{base}.500")).unwrap()}),
        json!({"sn":"elsewhere","name":"other","ctime":base+1}),
    ];
    assert_eq!(s.select(&rows).unwrap(), vec!["inside"]);
    let server = Server::new(vec![clock(), backups(&["one"])]);
    let (mut t, _) = transport(&server, "synthetic-token");
    let c = OperationControl::default();
    t.begin_operation(&c).unwrap();
    let s = BackupSelection::new(vec![], Some("no-match".into()), None, None, None).unwrap();
    let p = s.execute_with(&mut t, &c).unwrap();
    assert_eq!(p["matched"], 0);
    assert_eq!(p["reference_check"], "not_needed_no_matches");
    t.close().unwrap();
    assert_eq!(server.finish().len(), 2);
}
