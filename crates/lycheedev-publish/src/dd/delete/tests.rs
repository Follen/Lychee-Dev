use super::*;
use crate::{
    dd::{DdTransport, tests::transport},
    test_support::{Reply, Server, TestDir},
};
impl Api for DdTransport {
    fn delete(&mut self, path: &str, body: &Value) -> Result<Value> {
        self.post(path, &[], body, true)
    }
}
fn reply(result: Value) -> Reply {
    Reply::Json(json!({"code":0,"result":result}))
}
fn clock() -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":1793500000}}))
}
fn input(resource: Resource, sn: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema":format!("lycheedev.publish.v1.dd.{}.delete",resource.name()),"sn":sn,"confirm_delete":true})).unwrap()
}
fn list(rows: Value, total: u64) -> Reply {
    reply(json!({"items":rows,"total":total}))
}
fn absent() -> Reply {
    list(json!([]), 0)
}
fn run(request: &Delete, home: &Path, author: &str, transport: &mut DdTransport) -> Result<Value> {
    let control = OperationControl::default();
    transport.begin_operation(&control)?;
    request.execute_with(home, author, transport, &control)
}
#[test]
fn three_resource_deletions_keep_exact_sn_body_check_owner_and_verify_full_author_list_absence() {
    for resource in [Resource::Plugin, Resource::Config, Resource::Wa] {
        let dir = TestDir::new();
        let reference = "opaque+%中文";
        let request = Delete::parse(resource, &input(resource, reference)).unwrap();
        assert_eq!(request.plan()["remote_write"], false);
        let home = dir.0.join("home");
        assert!(!home.exists());
        let mut before = json!({"sn":reference,"game_type":10001,"is_owner":true,"name":"作者资源","content":"private-original-config","token":"private-token"});
        if resource == Resource::Config {
            before["retail_ui_config"] = json!({"edit_mode":{"A":[{"name":"layout","is_default":true,"data":"private-layout-code"}]}});
        }
        let server = Server::new(vec![
            clock(),
            reply(before),
            list(json!([{"sn":reference,"name":"作者资源"}]), 1),
            reply(json!({"removed":true,"token":"private-server-token"})),
            absent(),
        ]);
        let (mut author, _) = transport(&server, "synthetic-token");
        let output = run(&request, &home, "exact-author", &mut author).unwrap();
        assert_eq!(output["deleted"], true);
        assert_eq!(output["reference"], reference);
        assert_eq!(output["submitted_this_call"], true);
        assert_eq!(output["readback"]["complete_author_list"], true);
        assert!(!output.to_string().contains("private-original-config"));
        assert!(!output.to_string().contains("private-token"));
        assert!(!output.to_string().contains("private-layout-code"));
        author.close().unwrap();
        let requests = server.finish();
        let path = match resource {
            Resource::Plugin => "/addon/delete",
            Resource::Config => "/share/delete",
            Resource::Wa => "/wa/delete",
        };
        let mutations = requests
            .iter()
            .filter(|r| r.line.starts_with(&format!("POST {path}?")))
            .collect::<Vec<_>>();
        assert_eq!(mutations.len(), 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&mutations[0].body).unwrap(),
            json!({"sn":reference})
        );
        assert_eq!(requests.len(), 5);
    }
}
#[test]
fn ownership_requires_exact_detail_or_complete_author_membership_and_config_uses_bound_backup_game_type()
 {
    let dir = TestDir::new();
    let request = Delete::parse(Resource::Config, &input(Resource::Config, "selected")).unwrap();
    let server = Server::new(vec![
        clock(),
        reply(json!({"sn":"selected","backup_sn":"backup-original","name":"配置"})),
        reply(json!({"sn":"backup-original","game_type":"10001"})),
        list(json!([{"share_sn":"selected"}]), 1),
        reply(json!({})),
        absent(),
    ]);
    let (mut author, _) = transport(&server, "synthetic-token");
    assert_eq!(
        run(&request, &dir.0, "author", &mut author).unwrap()["game_type"],
        10001
    );
    author.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 6);
    assert!(requests[2].line.contains("sn=backup-original"));
    for before in [
        json!({"sn":"selected","game_type":10001,"is_owner":false}),
        json!({"sn":"selected","game_type":10001}),
    ] {
        let dir = TestDir::new();
        let denied = before["is_owner"] == false;
        let mut replies = vec![clock(), reply(before)];
        if !denied {
            replies.push(absent());
        }
        let server = Server::new(replies);
        let (mut author, _) = transport(&server, "synthetic-token");
        assert!(run(&request, &dir.0, "author", &mut author).is_err());
        author.close().unwrap();
        assert!(
            server
                .finish()
                .iter()
                .all(|r| !r.line.starts_with("POST /share/delete"))
        );
    }
}
#[test]
fn unknown_delete_result_recovers_original_game_and_sn_by_readback_without_detail_or_another_post()
{
    let dir = TestDir::new();
    let request = Delete::parse(Resource::Wa, &input(Resource::Wa, "selected")).unwrap();
    let server = Server::new(vec![
        clock(),
        reply(json!({"sn":"selected","game_type":0,"game_types":[10001],"is_owner":true})),
        list(json!([{"sn":"selected"}]), 1),
        Reply::Drop,
        absent(),
        absent(),
    ]);
    let (mut author, _) = transport(&server, "synthetic-token");
    let first = run(&request, &dir.0, "author", &mut author).unwrap_err();
    assert_eq!(first.details["submission"], "uncertain");
    let recovered = run(&request, &dir.0, "author", &mut author).unwrap();
    assert_eq!(recovered["recovered"], true);
    assert_eq!(recovered["submitted_this_call"], false);
    assert_eq!(recovered["operation_id"], first.details["operation_id"]);
    assert_eq!(
        run(&request, &dir.0, "author", &mut author).unwrap()["deleted"],
        true
    );
    author.close().unwrap();
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("GET /wa/detail"))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.starts_with("POST /wa/delete"))
            .count(),
        1
    );
    assert_eq!(requests.len(), 6);
}
#[test]
fn accepted_but_present_target_stays_pending_and_explicit_recovery_only_reads() {
    let dir = TestDir::new();
    let request = Delete::parse(Resource::Plugin, &input(Resource::Plugin, "selected")).unwrap();
    let selected = json!([{"sn":"selected"}]);
    let server = Server::new(vec![
        clock(),
        reply(json!({"sn":"selected","game_type":10001,"is_owner":true})),
        list(selected.clone(), 1),
        reply(json!({})),
        list(selected.clone(), 1),
        list(selected, 1),
    ]);
    let (mut author, _) = transport(&server, "synthetic-token");
    let first = run(&request, &dir.0, "author", &mut author).unwrap_err();
    assert_eq!(first.exit_code, 6);
    let again = run(&request, &dir.0, "author", &mut author).unwrap_err();
    assert_eq!(again.exit_code, 6);
    assert_eq!(first.details["operation_id"], again.details["operation_id"]);
    author.close().unwrap();
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.line.starts_with("POST /addon/delete"))
            .count(),
        1
    );
}
#[test]
fn incomplete_or_repeated_author_pages_cannot_authorize_ownership_or_prove_deleted() {
    for after_post in [false, true] {
        let dir = TestDir::new();
        let request = Delete::parse(Resource::Wa, &input(Resource::Wa, "selected")).unwrap();
        let before = reply(json!({"sn":"selected","game_type":10001,"is_owner":true}));
        let mut replies = vec![clock(), before];
        if after_post {
            replies.push(list(json!([{"sn":"selected"}]), 1));
            replies.push(reply(json!({})));
        }
        replies.push(list(json!([{"sn":"unrelated"}]), 2));
        replies.push(list(json!([{"sn":"unrelated"}]), 2));
        let server = Server::new(replies);
        let (mut author, _) = transport(&server, "synthetic-token");
        let error = run(&request, &dir.0, "author", &mut author).unwrap_err();
        assert_eq!(error.exit_code, if after_post { 6 } else { 5 });
        author.close().unwrap();
        let requests = server.finish();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.line.starts_with("POST /wa/delete"))
                .count(),
            usize::from(after_post)
        );
    }
}
#[test]
fn definitive_business_rejection_allows_a_new_explicit_delete_and_shape_errors_never_open_home() {
    let dir = TestDir::new();
    let resource = Resource::Wa;
    let request = Delete::parse(resource, &input(resource, "selected")).unwrap();
    let detail = json!({"sn":"selected","game_type":10001,"is_owner":true});
    let row = json!([{"sn":"selected"}]);
    let server = Server::new(vec![
        clock(),
        reply(detail.clone()),
        list(row.clone(), 1),
        Reply::Json(json!({"code":400,"msg":"private token"})),
        reply(detail),
        list(row, 1),
        reply(json!({})),
        absent(),
    ]);
    let (mut author, _) = transport(&server, "synthetic-token");
    let rejected = run(&request, &dir.0, "author", &mut author).unwrap_err();
    assert_eq!(rejected.details["submission"], "rejected");
    assert!(!rejected.to_string().contains("private token"));
    assert_eq!(
        run(&request, &dir.0, "author", &mut author).unwrap()["deleted"],
        true
    );
    author.close().unwrap();
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.line.starts_with("POST /wa/delete"))
            .count(),
        2
    );
    for value in [
        json!({"schema":"lycheedev.publish.v1.dd.wa.delete","sn":"selected","confirm_delete":false}),
        json!({"schema":"lycheedev.publish.v1.dd.wa.delete","sn":"bad\u{1f}","confirm_delete":true}),
        json!({"schema":"lycheedev.publish.v1.dd.plugin.delete","sn":"selected","confirm_delete":true}),
        json!({"schema":"lycheedev.publish.v1.dd.wa.delete","sn":"selected","confirm_delete":true,"cookie":"private"}),
    ] {
        assert!(Delete::parse(resource, &serde_json::to_vec(&value).unwrap()).is_err());
    }
}
