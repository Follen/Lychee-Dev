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
fn list(ids: &[u64]) -> Reply {
    ok(json!({"total":ids.len(),"rows":ids.iter().map(|id|json!({"id":id})).collect::<Vec<_>>()}))
}
fn request(action: &str, mut value: Value) -> ProjectWrite {
    value["schema"] = json!(format!("lycheedev.publish.v1.modus.project.{action}"));
    ProjectWrite::parse(action, &serde_json::to_vec(&value).unwrap()).unwrap()
}
fn current(id: u64) -> Value {
    json!({"projectId":id,"name":"AuthorAddon","altName":"Old alias","summary":"Summary","description":"<p>Old</p>","repoUrl":"https://example.com/addon","categories":[{"id":1}],"license":"{\"type\":\"MIT\",\"holder\":\"Author\"}","synchronizationType":1,"requiredTierId":5,"requiredDependencies":"22","images":0})
}
#[test]
fn flat_project_creation_keeps_official_form_and_unknown_ack_recovers_unique_new_identity() {
    let d = TestDir::new();
    let input = request(
        "create",
        json!({"name":"AuthorAddon","summary":"Summary","categories":[1],"license":{"type":"MIT","holder":"Author"},"publish_platforms":["modus"],"required_tier_id":null}),
    );
    let server = Server::new(vec![
        account(),
        list(&[200000001]),
        ok(json!([{"id":1}])),
        Reply::Drop,
    ]);
    let e = input
        .run(&d.0, &api(&server, Client::Creator), &control())
        .unwrap_err();
    assert_eq!(e.details["submission"], "uncertain");
    let req = server.finish();
    let form: Value = serde_json::from_slice(&req[3].body).unwrap();
    assert_eq!(
        form,
        json!({"name":"AuthorAddon","summary":"Summary","categories":"1","license":"{\"holder\":\"Author\",\"type\":\"MIT\"}","synchronizationType":1,"images":0,"screenshotBase64sReqs":{"name":"logo.webp","screenshotBase64s":""}})
    );
    assert!(
        req[3]
            .line
            .starts_with("POST /api/game/data/author/project/release ")
    );
    let mut created = current(200000719);
    created["requiredTierId"] = Value::Null;
    let server = Server::new(vec![account(), list(&[200000001, 200000719]), ok(created)]);
    let result = input
        .run(&d.0, &api(&server, Client::Creator), &control())
        .unwrap();
    assert_eq!(result["project_id"], 200000719);
    assert_eq!(result["recovered"], true);
    assert_eq!(server.finish().len(), 3);
    let server = Server::new(vec![account()]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Creator), &control())
            .unwrap()["reused"],
        true
    );
    server.finish();
}
#[test]
fn sparse_edit_preserves_omissions_and_clearing_restricted_fields_are_checked_before_post() {
    let d = TestDir::new();
    let input = request(
        "edit",
        json!({"project_id":200000719,"alt_name":"","description":null,"repo_url":"","required_tier_id":null}),
    );
    let mut after = current(200000719);
    for k in ["altName", "description", "repoUrl", "requiredTierId"] {
        after[k] = Value::Null;
    }
    let server = Server::new(vec![
        account(),
        list(&[200000719]),
        ok(current(200000719)),
        ok(json!({})),
        list(&[200000719]),
        ok(after),
    ]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Creator), &control())
            .unwrap()["verified"],
        true
    );
    let req = server.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&req[3].body).unwrap(),
        json!({"projectId":200000719,"altName":"<null>","description":"<null>","repoUrl":"<null>","requiredTierId":"<null>"})
    );
    let input = request("edit", json!({"project_id":42,"name":"Changed"}));
    let server = Server::new(vec![account(), list(&[42]), ok(current(42))]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Creator), &control())
            .unwrap_err()
            .exit_code,
        4
    );
    assert_eq!(server.finish().len(), 3);
    let input = request("edit", json!({"project_id":42,"alt_name":"允许"}));
    let mut after = current(42);
    after["altName"] = json!("允许");
    let server = Server::new(vec![
        account(),
        list(&[42]),
        ok(current(42)),
        ok(json!({})),
        list(&[42]),
        ok(after),
    ]);
    input
        .run(&d.0, &api(&server, Client::Creator), &control())
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&server.finish()[3].body).unwrap(),
        json!({"projectId":42,"altName":"允许"})
    );
}
#[test]
fn project_delete_is_bodyless_and_unknown_media_edit_never_uses_metadata_as_byte_proof() {
    let d = TestDir::new();
    let input = request("delete", json!({"project_id":200000719,"confirm":"DELETE"}));
    let server = Server::new(vec![
        account(),
        list(&[200000719]),
        ok(current(200000719)),
        Reply::Drop,
    ]);
    assert!(
        input
            .run(&d.0, &api(&server, Client::Creator), &control())
            .is_err()
    );
    let req = server.finish();
    assert!(req[3].body.is_empty());
    assert!(req[3].line.contains("/project/delete/project/200000719"));
    let server = Server::new(vec![account(), list(&[])]);
    assert_eq!(
        input
            .run(&d.0, &api(&server, Client::Creator), &control())
            .unwrap()["deleted"],
        true
    );
    server.finish();
    let input = request(
        "edit",
        json!({"project_id":200000720,"images":0,"image_ops":[{"op":"rename","from":"2.webp","to":"1.webp"}]}),
    );
    let server = Server::new(vec![
        account(),
        list(&[200000720]),
        ok(current(200000720)),
        Reply::Drop,
    ]);
    assert!(
        input
            .run(&d.0, &api(&server, Client::Creator), &control())
            .is_err()
    );
    server.finish();
    let server = Server::new(vec![account()]);
    let e = input
        .run(&d.0, &api(&server, Client::Creator), &control())
        .unwrap_err();
    assert_eq!(e.exit_code, 6);
    assert!(e.to_string().contains("readback"));
    assert_eq!(server.finish().len(), 1);
    for mut value in [
        json!({"name":"N","summary":"S","categories":[998],"license":{"type":"MIT"},"publish_platforms":["modus"]}),
        json!({"name":"N","summary":"S","categories":[1],"license":{"type":"custom"},"publish_platforms":["modus"]}),
        json!({"name":"N","summary":"S","categories":[1],"license":{"type":"MIT"},"publish_platforms":["bigfoot"],"required_tier_id":1}),
    ] {
        value["schema"] = json!("lycheedev.publish.v1.modus.project.create");
        assert!(ProjectWrite::parse("create", &serde_json::to_vec(&value).unwrap()).is_err());
    }
}
