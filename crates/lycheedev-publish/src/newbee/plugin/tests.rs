use super::*;
use crate::newbee::tests::{authenticated, provider};
use crate::test_support::{Reply, Server, TestDir};

fn detail() -> Value {
    json!({"t_id":719,"t_name":"Author addon","t_original":2,"t_content_format":1,"t_description":"short","t_description_v2":"long description","t_logo":"https://cdn.invalid/logo.png","screenshots":[{"media_url":"https://cdn.invalid/screen.png"}],"category_ids":[{"t_id":31,"label":"UI"}],"t_share":1,"t_subscribe_plan_level":0,"t_link_to_channel":1,"server_only":"never-post"})
}
fn edit(value: Value, dir: &TestDir) -> PluginEdit {
    PluginEdit::parse(&serde_json::to_vec(&value).unwrap(), &dir.0).unwrap()
}
fn update(dir: &TestDir) -> PluginUpdate {
    PluginUpdate::parse(&serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.newbee.plugin.update","mod_id":719,"version":"3.0","file":dir.zip(),"game_version_list":["12.0.1"],"changelog":"fixed","link_to_channel":false})).unwrap(),&dir.0).unwrap()
}
#[test]
fn image_upload_streams_original_bytes_and_reuses_the_original_public_url() {
    let dir = TestDir::new();
    let path = dir.0.join("screen.png");
    std::fs::write(&path, b"\x89PNG\r\n\x1a\noriginal-image").unwrap();
    let server = Server::new(authenticated(vec![Reply::Json(
        json!({"code":1,"data":{"media_url":"https://cdn.invalid/published.png"}}),
    )]));
    let mut provider = provider(&dir, &server, true);
    let uploaded = provider.upload_image(Resource::Plugin, &path).unwrap();
    assert_eq!(uploaded["url"], "https://cdn.invalid/published.png");
    assert_eq!(
        provider.upload_image(Resource::Plugin, &path).unwrap(),
        uploaded
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[3].line,
        "POST /creator/wow/mod/upload_media HTTP/1.1"
    );
    assert!(
        requests[3]
            .body
            .windows(22)
            .any(|bytes| bytes == b"\x89PNG\r\n\x1a\noriginal-image")
    );
    assert!(String::from_utf8_lossy(&requests[3].body).contains("Content-Type: image/png"));
}
#[test]
fn metadata_edit_preserves_omitted_form_fields_and_submits_explicit_clears() {
    let dir = TestDir::new();
    let mut changed = detail();
    changed["t_description"] = json!("");
    changed["t_share"] = json!(0);
    changed["t_link_to_channel"] = json!(false);
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":detail()})),
        Reply::Json(json!({"code":1})),
        Reply::Json(json!({"code":1,"data":changed})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let result=provider.edit_plugin(&edit(json!({"schema":"lycheedev.publish.v1.newbee.plugin.edit","id":719,"intro":"","public":false}),&dir)).unwrap();
    assert_eq!(result["author_writes"], 1);
    let requests = server.finish();
    assert_eq!(requests.len(), 6);
    assert_eq!(requests[4].line, "POST /creator/wow/mod/edit HTTP/1.1");
    let submitted: Value = serde_json::from_slice(&requests[4].body).unwrap();
    assert_eq!(
        submitted,
        json!({"id":719,"mod_categories":[31],"content_origin":2,"content_format":1,"name":"Author addon","description":"long description","intro":"","logo":"https://cdn.invalid/logo.png","screenshots":["https://cdn.invalid/screen.png"],"share_state":0,"subscribe_plan_level":0,"link_to_channel":false})
    );
}
#[test]
fn public_review_needs_explicit_intent_and_a_first_immutable_version() {
    let dir = TestDir::new();
    let missing =
        json!({"schema":"lycheedev.publish.v1.newbee.plugin.edit","id":719,"public":true});
    assert!(PluginEdit::parse(&serde_json::to_vec(&missing).unwrap(), &dir.0).is_err());
    let mut private = detail();
    private["t_share"] = json!(0);
    private["t_link_to_channel"] = json!(false);
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":private})),
        Reply::Json(json!({"code":1,"data":{"list":[],"total":0}})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let error=provider.edit_plugin(&edit(json!({"schema":"lycheedev.publish.v1.newbee.plugin.edit","id":719,"public":true,"submit_for_review":true}),&dir)).unwrap_err();
    assert_eq!(error.details["path"], "$.public");
    assert_eq!(server.finish().len(), 5);
}
#[test]
fn lost_metadata_response_is_read_back_without_repeating_the_original_edit() {
    let dir = TestDir::new();
    let mut changed = detail();
    changed["t_name"] = json!("Corrected addon");
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":detail()})),
        Reply::Drop,
        Reply::Json(json!({"code":1,"data":changed})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let request = edit(
        json!({"schema":"lycheedev.publish.v1.newbee.plugin.edit","id":719,"name":"Corrected addon"}),
        &dir,
    );
    let error = provider.edit_plugin(&request).unwrap_err();
    assert_eq!(error.details["submission"], "uncertain");
    let recovered = provider.edit_plugin(&request).unwrap();
    assert_eq!(recovered["recovered"], true);
    assert_eq!(recovered["author_writes"], 0);
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|r| r.line == "POST /creator/wow/mod/edit HTTP/1.1")
            .count(),
        1
    );
}
#[test]
fn old_version_on_the_second_page_is_rejected_before_archive_submission() {
    let dir = TestDir::new();
    let request = update(&dir);
    let first = (1..=100)
        .map(|id| json!({"t_id":id,"t_display_name":format!("old-{id}")}))
        .collect::<Vec<_>>();
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":{"list":first,"total":101}})),
        Reply::Json(
            json!({"code":1,"data":{"list":[{"t_id":991,"t_display_name":"3.0"}],"total":101}}),
        ),
    ]));
    let mut provider = provider(&dir, &server, true);
    assert_eq!(
        provider.update_plugin(&request).unwrap_err().details["path"],
        "$.version"
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 5);
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[4].body).unwrap()["pagenum"],
        2
    );
}
#[test]
fn archive_upload_keeps_exact_multipart_fields_and_recovers_missing_response_by_version() {
    let dir = TestDir::new();
    let request = update(&dir);
    let versions = json!({"code":1,"data":{"list":[{"t_id":991,"t_display_name":"3.0","versions":[{"version":"12.0.1"}]}],"total":1}});
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":{"list":[],"total":0}})),
        Reply::Json(json!({"game_version":[{"id":1,"name":"Retail","version":["12.0.1"]}]})),
        Reply::Json(json!({"code":1,"data":detail()})),
        Reply::Drop,
        Reply::Json(versions),
    ]));
    let mut provider = provider(&dir, &server, true);
    let error = provider.update_plugin(&request).unwrap_err();
    assert_eq!(error.details["submission"], "uncertain");
    let result = provider.update_plugin(&request).unwrap();
    assert_eq!(result["file_id"], 991);
    assert_eq!(result["recovered"], true);
    assert_eq!(result["remote_file_bytes_verification"], "not_available");
    assert_eq!(provider.update_plugin(&request).unwrap(), result);
    let requests = server.finish();
    assert_eq!(requests.len(), 8);
    let wire = String::from_utf8_lossy(&requests[6].body);
    for field in [
        "name=\"mod_id\"\r\n\r\n719",
        "name=\"version\"\r\n\r\n3.0",
        "name=\"game_version_list\"\r\n\r\n[\"12.0.1\"]",
        "name=\"link_to_channel\"\r\n\r\nfalse",
        "name=\"changelog\"\r\n\r\nfixed",
    ] {
        assert!(wire.contains(field), "{field}");
    }
    assert!(requests[6].body.windows(4).any(|v| v == b"PK\x03\x04"));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line == "POST /creator/wow/mod_file/upload_mod_file HTTP/1.1")
            .count(),
        1
    );
}
