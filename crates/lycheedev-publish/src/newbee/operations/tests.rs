use super::*;
use crate::{
    newbee::tests::{authenticated, provider},
    test_support::{Reply, Server, TestDir},
};
#[test]
fn uncertain_delete_uses_complete_author_absence_without_repeating_the_deletion() {
    let dir = TestDir::new();
    let present = json!({"code":1,"data":{"list":[{"t_id":719,"t_title":"UI"}],"total":1}});
    let server = Server::new(authenticated(vec![
        Reply::Json(present.clone()),
        Reply::Json(json!({"code":1,"data":{"t_id":719}})),
        Reply::Drop,
        Reply::Json(present),
        Reply::Json(json!({"code":1,"data":{"list":[],"total":0}})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let request = AuthorDelete::parse(
        Resource::Config,
        br#"{"schema":"lycheedev.publish.v1.newbee.config.delete","id":719,"confirm":"DELETE"}"#,
    )
    .unwrap();
    assert_eq!(
        provider.delete(&request).unwrap_err().details["submission"],
        "uncertain"
    );
    assert_eq!(provider.delete(&request).unwrap_err().exit_code, 6);
    let recovered = provider.delete(&request).unwrap();
    assert_eq!(recovered["recovered"], true);
    assert_eq!(recovered["author_writes"], 0);
    assert_eq!(recovered["verification"], "complete_author_list");
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line == "POST /creator/wow/share_config/delete HTTP/1.1")
            .count(),
        1
    );
}
#[test]
fn deletion_cannot_use_a_page_without_total_as_proof_of_author_absence() {
    let dir = TestDir::new();
    let server = Server::new(authenticated(vec![Reply::Json(
        json!({"code":1,"data":{"list":[]}}),
    )]));
    let mut provider = provider(&dir, &server, true);
    let request = AuthorDelete::parse(
        Resource::Plugin,
        br#"{"schema":"lycheedev.publish.v1.newbee.plugin.delete","id":719,"confirm":"DELETE"}"#,
    )
    .unwrap();
    assert_eq!(
        provider.delete(&request).unwrap_err().code,
        "publish.newbee_data"
    );
    assert_eq!(server.finish().len(), 4);
}
#[test]
fn wa_changelog_clear_uses_the_original_log_and_parent_then_checks_complete_text() {
    let dir = TestDir::new();
    let server = Server::new(authenticated(vec![
        Reply::Json(
            json!({"code":1,"data":{"list":[{"wa_log_id":991,"wa_id":719,"content":"old"}]}}),
        ),
        Reply::Json(json!({"code":1})),
        Reply::Json(
            json!({"code":1,"data":{"list":[{"wa_log_id":991,"wa_id":719,"content":null}]}}),
        ),
    ]));
    let mut provider = provider(&dir, &server, true);
    let request=WaChangelogEdit::parse(br#"{"schema":"lycheedev.publish.v1.newbee.wa-changelog.edit","id":991,"wa_id":719,"wa_log":null}"#).unwrap();
    let result = provider.edit_wa_changelog(&request).unwrap();
    assert_eq!(result["verification"], "readback");
    let requests = server.finish();
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[4].body).unwrap(),
        json!({"wa_log_id":991,"content":""})
    );
    for request in [&requests[3], &requests[5]] {
        assert_eq!(
            serde_json::from_slice::<Value>(&request.body).unwrap()["wa_id"],
            719
        );
    }
}
#[test]
fn acknowledged_share_code_recovers_its_identity_readback_without_repeating_set() {
    let dir = TestDir::new();
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":{"t_id":719}})),
        Reply::Json(json!({"code":1,"data":{"shareCode":"PUBLIC-CODE"}})),
        Reply::Drop,
        Reply::Json(json!({"code":1,"data":{"t_id":719}})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let request = WaShareCode::parse(
        br#"{"schema":"lycheedev.publish.v1.newbee.wa-share-code.set","module_id":719}"#,
    )
    .unwrap();
    assert_eq!(
        provider.set_wa_share_code(&request).unwrap_err().exit_code,
        6
    );
    let result = provider.set_wa_share_code(&request).unwrap();
    assert_eq!(result["share_code"], "PUBLIC-CODE");
    assert_eq!(result["verification"], "api_acceptance");
    assert_eq!(result["share_code_verification"], "not_available");
    assert_eq!(provider.set_wa_share_code(&request).unwrap(), result);
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.line == "POST /bannerserver/ShareCode/Set HTTP/1.1")
            .count(),
        1
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[4].body).unwrap(),
        json!({"gameId":1,"moduleId":719,"moduleType":3})
    );
}
#[test]
fn missing_business_outcome_is_uncertain_and_does_not_authorize_a_second_post() {
    let dir = TestDir::new();
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":{"t_id":719,"t_name":"Old"}})),
        Reply::Json(json!({"data":{"id":719},"message":"Bearer raw-credential"})),
        Reply::Json(json!({"code":1,"data":{"t_id":719,"t_name":"Old"}})),
        Reply::Json(json!({"code":1,"data":{"t_id":719,"t_name":"Changed"}})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let request = PluginEdit::parse(
        br#"{"schema":"lycheedev.publish.v1.newbee.plugin.edit","id":719,"name":"Changed"}"#,
        &dir.0,
    )
    .unwrap();
    let error = provider.edit_plugin(&request).unwrap_err();
    assert_eq!(error.details["submission"], "uncertain");
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains("raw-credential")
    );
    assert_eq!(provider.edit_plugin(&request).unwrap_err().exit_code, 6);
    assert_eq!(provider.edit_plugin(&request).unwrap()["recovered"], true);
    assert_eq!(
        server
            .finish()
            .iter()
            .filter(|request| request.line == "POST /creator/wow/mod/edit HTTP/1.1")
            .count(),
        1
    );
}
