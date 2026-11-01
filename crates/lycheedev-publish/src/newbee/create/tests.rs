use super::*;
use crate::{
    newbee::tests::{authenticated, provider},
    test_support::{Reply, Server, TestDir},
};
fn plugin_doc() -> Value {
    json!({"schema":"lycheedev.publish.v1.newbee.plugin.create","name":"New addon","mod_categories":[31],"content_origin":2,"content_format":1,"intro":"short","description":"Details","logo":"https://cdn.invalid/logo.png","screenshots":["https://cdn.invalid/screen.png"],"public":true,"submit_for_review":true,"link_to_channel":true})
}
fn plugin_detail() -> Value {
    json!({"t_id":719,"t_name":"New addon","category_ids":[31],"t_original":2,"t_content_format":1,"t_description":"short","t_description_v2":"Details","t_logo":"https://cdn.invalid/logo.png","screenshots":["https://cdn.invalid/screen.png"],"t_share":0,"t_subscribe_plan_level":0,"t_link_to_channel":false})
}
fn plugin_preflight() -> Vec<Reply> {
    vec![
        Reply::Json(json!({"code":1})),
        Reply::Json(json!({"mod_category":[{"t_id":31,"t_name":"UI"}]})),
        Reply::Json(json!({"code":1,"data":[{"value":2,"label":"Original"}]})),
    ]
}
#[test]
fn identical_creation_input_is_partitioned_by_desktop_subject_and_reused_when_that_account_returns()
{
    let dir = TestDir::new();
    let request = AuthorCreate::parse(
        Resource::Plugin,
        &serde_json::to_vec(&plugin_doc()).unwrap(),
        &dir.0,
    )
    .unwrap();
    let mut replies = Vec::new();
    for id in [719, 720] {
        let mut current = plugin_detail();
        current["t_id"] = json!(id);
        let mut creation = plugin_preflight();
        creation.extend([
            Reply::Json(json!({"code":1,"data":{"id":id}})),
            Reply::Json(json!({"code":1,"data":current})),
        ]);
        replies.extend(authenticated(creation));
    }
    let server = Server::new(replies);
    for (subject, id) in [("author-A", 719), ("author-B", 720), ("author-A", 719)] {
        let mut client = provider(&dir, &server, true);
        crate::newbee::tests::set_subject(&dir, subject);
        let result = client.create(&request, None).unwrap();
        assert_eq!(result["id"], id);
    }
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line == "POST /creator/wow/mod/create HTTP/1.1")
            .count(),
        2
    );
    assert_eq!(requests.len(), 16);
}
#[test]
fn plugin_creation_stays_private_until_a_first_version_and_reuses_its_creation_receipt() {
    let dir = TestDir::new();
    let request = AuthorCreate::parse(
        Resource::Plugin,
        &serde_json::to_vec(&plugin_doc()).unwrap(),
        &dir.0,
    )
    .unwrap();
    let mut replies = plugin_preflight();
    replies.extend([
        Reply::Json(json!({"code":1,"data":{"id":719}})),
        Reply::Json(json!({"code":1,"data":plugin_detail()})),
    ]);
    let server = Server::new(authenticated(replies));
    let mut provider = provider(&dir, &server, true);
    let result = provider.create(&request, None).unwrap();
    assert_eq!(result["public"], false);
    assert_eq!(result["public_after_first_version"], true);
    assert_eq!(result["review_intent"], true);
    let reused = provider.create(&request, None).unwrap();
    assert_eq!(reused["id"], 719);
    assert_eq!(reused["reused"], true);
    let requests = server.finish();
    assert_eq!(requests.len(), 8);
    let body: Value = serde_json::from_slice(&requests[6].body).unwrap();
    assert_eq!(
        body,
        json!({"mod_categories":[31],"content_origin":2,"content_format":1,"name":"New addon","description":"Details","intro":"short","logo":"https://cdn.invalid/logo.png","screenshots":["https://cdn.invalid/screen.png"],"share_state":0,"subscribe_plan_level":0,"link_to_channel":false})
    );
}
#[test]
fn lost_creation_response_requires_explicit_resource_selection_and_never_recreates_by_title() {
    let dir = TestDir::new();
    let request = AuthorCreate::parse(
        Resource::Plugin,
        &serde_json::to_vec(&plugin_doc()).unwrap(),
        &dir.0,
    )
    .unwrap();
    let mut replies = plugin_preflight();
    replies.extend([
        Reply::Drop,
        Reply::Json(json!({"code":1,"data":plugin_detail()})),
        Reply::Json(json!({"code":1,"data":plugin_detail()})),
    ]);
    let server = Server::new(authenticated(replies));
    let mut provider = provider(&dir, &server, true);
    let error = provider.create(&request, None).unwrap_err();
    assert_eq!(error.details["submission"], "uncertain");
    assert_eq!(provider.create(&request, None).unwrap_err().exit_code, 6);
    assert_eq!(
        provider
            .create(&request, NonZeroU64::new(720))
            .unwrap_err()
            .exit_code,
        6
    );
    let result = provider.create(&request, NonZeroU64::new(719)).unwrap();
    assert_eq!(result["id"], 719);
    assert_eq!(result["recovered"], true);
    let requests = server.finish();
    assert_eq!(requests.len(), 9);
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line == "POST /creator/wow/mod/create HTTP/1.1")
            .count(),
        1
    );
    assert!(!requests.iter().any(|r| r.line.contains("publish_list")));
}
#[test]
fn configuration_creation_selects_the_backup_and_keeps_zero_payment_state() {
    let dir = TestDir::new();
    let doc = json!({"schema":"lycheedev.publish.v1.newbee.config.create","cloud_id":99,"title":"UI","content":"Details","content_format":1,"content_origin":2,"public":false,"linked_mods":[],"ignored_unknown_mods":[],"ignored_materials":[],"ignored_fronts":[],"roleid":"","picture_urls":["https://cdn.invalid/ui.png"],"price":0,"time_range":"unused","link_to_channel":true});
    let request =
        AuthorCreate::parse(Resource::Config, &serde_json::to_vec(&doc).unwrap(), &dir.0).unwrap();
    let raw = json!({"t_id":719,"t_cloudblackid":99,"t_title":"UI","t_content":"Details","t_content_format":1,"t_content_origin":2,"pic_url":["https://cdn.invalid/ui.png"],"t_sharing":0,"t_link_to_channel":false,"t_price":0,"t_time_range":"","t_selected_eml":[{"server_default":true}]});
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":[{"t_id":99}]})),
        Reply::Json(json!({"code":1,"data":[{"value":2}]})),
        Reply::Json(json!({"code":1,"data":{"id":719}})),
        Reply::Json(json!({"code":1,"data":raw})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let result = provider.create(&request, None).unwrap();
    assert_eq!(result["verification"], "metadata_readback");
    let requests = server.finish();
    assert_eq!(
        requests[5].line,
        "POST /creator/wow/share_config/release HTTP/1.1"
    );
    let body: Value = serde_json::from_slice(&requests[5].body).unwrap();
    assert_eq!(body["cloud_id"], 99);
    assert_eq!(body["time_range"], "");
    assert_eq!(body["link_to_channel"], false);
    assert!(body.get("selected_eml").is_none());
}
#[test]
fn wa_creation_checks_live_choices_and_reads_back_the_initial_opaque_content_without_emitting_it() {
    let dir = TestDir::new();
    let opaque = "!WA:2!PRIVATE_INITIAL_CONTENT";
    let doc = json!({"schema":"lycheedev.publish.v1.newbee.wa.create","game_version_id":1,"name":"Aura","description":"Details","content_format":1,"category_id_list":[11],"content_origin":2,"public":false,"thumbnail":"https://cdn.invalid/aura.png","wa_str":opaque,"wa_log":"Initial","string_mode":"single"});
    let request =
        AuthorCreate::parse(Resource::Wa, &serde_json::to_vec(&doc).unwrap(), &dir.0).unwrap();
    let raw = json!({"t_id":719,"t_game_version_id":1,"t_name":"Aura","t_description":"Details","t_content_format":1,"category_list":[{"t_id":11}],"t_content_origin":2,"t_thumbnail":"https://cdn.invalid/aura.png","t_share_state":2});
    let mut replies = vec![Reply::Json(
        json!({"game_version":[{"id":1,"version":["12.0.1"]}]}),
    )];
    replies.extend(authenticated(vec![
        Reply::Json(json!({"code":1,"data":[{"id":11}]})),
        Reply::Json(json!({"code":1,"data":[{"value":2}]})),
        Reply::Json(json!({"code":1,"data":{"wa_id":719}})),
        Reply::Json(json!({"code":1,"data":raw})),
        Reply::Json(json!({"code":1,"data":{"wa_id":719,"version":"1","wa_str":opaque}})),
    ]));
    let server = Server::new(replies);
    let mut provider = provider(&dir, &server, true);
    let result = provider.create(&request, None).unwrap();
    assert_eq!(result["wa_content_verification"], "readback");
    assert!(!result.to_string().contains(opaque));
    let requests = server.finish();
    assert_eq!(requests.len(), 9);
    assert_eq!(requests[6].line, "POST /creator/wow/wa/publish HTTP/1.1");
    let body: Value = serde_json::from_slice(&requests[6].body).unwrap();
    assert_eq!(body["wa_str"], opaque);
    assert_eq!(body["string_mode"], "single");
    assert_eq!(body["share_state"], 2);
    let scopes = std::fs::read_dir(provider.home.join("publish/newbee/records/wa-create"))
        .unwrap()
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(scopes.len(), 1);
    let receipt = std::fs::read_to_string(scopes[0].path().join("active.json")).unwrap();
    assert!(!receipt.contains(opaque));
}

#[test]
fn an_earlier_completed_creation_is_reused_after_creating_another_resource() {
    let dir = TestDir::new();
    let first = AuthorCreate::parse(
        Resource::Plugin,
        &serde_json::to_vec(&plugin_doc()).unwrap(),
        &dir.0,
    )
    .unwrap();
    let mut other_doc = plugin_doc();
    other_doc["name"] = json!("Another addon");
    let second = AuthorCreate::parse(
        Resource::Plugin,
        &serde_json::to_vec(&other_doc).unwrap(),
        &dir.0,
    )
    .unwrap();
    let mut replies = plugin_preflight();
    replies.extend([
        Reply::Json(json!({"code":1,"data":{"id":719}})),
        Reply::Json(json!({"code":1,"data":plugin_detail()})),
    ]);
    replies.extend(plugin_preflight());
    let mut other = plugin_detail();
    other["t_id"] = json!(720);
    other["t_name"] = json!("Another addon");
    replies.extend([
        Reply::Json(json!({"code":1,"data":{"id":720}})),
        Reply::Json(json!({"code":1,"data":other})),
    ]);
    let server = Server::new(authenticated(replies));
    let mut provider = provider(&dir, &server, true);
    assert_eq!(provider.create(&first, None).unwrap()["id"], 719);
    assert_eq!(provider.create(&second, None).unwrap()["id"], 720);
    let reused = provider.create(&first, None).unwrap();
    assert_eq!(reused["id"], 719);
    assert_eq!(reused["reused"], true);
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|request| request.line == "POST /creator/wow/mod/create HTTP/1.1")
            .count(),
        2
    );
}
