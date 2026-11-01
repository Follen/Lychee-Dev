use super::*;
use crate::{
    newbee::tests::{authenticated, provider},
    test_support::{Reply, Server, TestDir},
};
fn detail() -> Value {
    json!({"t_id":719,"t_game_version_id":1,"t_name":"Aura","t_intro":"short","t_description":"Details","t_content_format":1,"t_thumbnail":"https://cdn.invalid/aura.png","t_images":["https://cdn.invalid/screen.png"],"category_list":[{"t_id":11,"t_name":"UI"}],"t_content_origin":2,"t_subscribe_plan_level":0,"t_price":5,"t_time_range":"month","t_share_state":1,"t_link_to_channel":true,"t_attachments":[],"t_version":"9007199254740993","t_wa_str":"!WA:2!private-current"})
}
#[test]
fn wa_full_form_clears_explicit_fields_and_preserves_omitted_metadata_without_raw_imports() {
    let dir = TestDir::new();
    let mut changed = detail();
    changed["t_intro"] = json!("");
    changed["t_images"] = json!([]);
    changed["t_share_state"] = json!(2);
    changed["t_link_to_channel"] = json!(false);
    changed["t_price"] = json!(0);
    changed["t_time_range"] = json!("");
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":detail()})),
        Reply::Json(json!({"game_version":[{"id":1,"name":"Retail","version":["12.0.1"]}]})),
        Reply::Json(json!({"code":1,"data":[{"id":1,"children":[{"id":11}]}]})),
        Reply::Json(json!({"code":1})),
        Reply::Json(json!({"code":1,"data":changed})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let request=WaEdit::parse(&serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.newbee.wa.edit","id":719,"intro":"","images":[],"public":false,"price":0})).unwrap(),&dir.0).unwrap();
    let result = provider.edit_wa(&request).unwrap();
    assert_eq!(result["author_writes"], 1);
    let requests = server.finish();
    let body: Value = serde_json::from_slice(&requests[6].body).unwrap();
    assert_eq!(
        body,
        json!({"id":719,"game_version_id":1,"name":"Aura","intro":"","description":"Details","content_format":1,"thumbnail":"https://cdn.invalid/aura.png","images":[],"category_id_list":[11],"content_origin":2,"subscribe_plan_level":0,"price":0,"time_range":"","share_state":2,"link_to_channel":false,"attachments":[],"wa_log":""})
    );
    assert!(!String::from_utf8_lossy(&requests[6].body).contains("private-current"));
}
#[test]
fn wa_category_and_nested_attachment_types_are_checked_before_a_write() {
    let dir = TestDir::new();
    let invalid = json!({"schema":"lycheedev.publish.v1.newbee.wa.edit","id":719,"attachments":[{"name":"file","install_type":true,"install_path":"Interface","value":"index","is_compressed":true}]});
    assert!(WaEdit::parse(&serde_json::to_vec(&invalid).unwrap(), &dir.0).is_err());
    let request=WaEdit::parse(&serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.newbee.wa.edit","id":719,"category_id_list":[12]})).unwrap(),&dir.0).unwrap();
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":detail()})),
        Reply::Json(json!({"game_version":[{"id":1,"version":["12.0.1"]}]})),
        Reply::Json(json!({"code":1,"data":[{"id":11}]})),
    ]));
    let mut provider = provider(&dir, &server, true);
    assert_eq!(
        provider.edit_wa(&request).unwrap_err().details["path"],
        "$.category_id_list"
    );
    assert_eq!(server.finish().len(), 6);
}
#[test]
fn automatic_wa_version_is_retained_across_lost_response_and_wrong_identity_readback() {
    let dir = TestDir::new();
    let version = "9007199254740994";
    let opaque = "!WA:2!ORIGINAL_PRIVATE_IMPORT";
    let request=WaUpdate::parse(&serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.newbee.wa.update","id":719,"wa_str":opaque,"wa_log":"Changes"})).unwrap()).unwrap();
    let server = Server::new(authenticated(vec![
        Reply::Json(json!({"code":1,"data":{"next_version":version}})),
        Reply::Json(json!({"code":1,"data":detail()})),
        Reply::Drop,
        Reply::Json(json!({"code":1,"data":{"wa_id":720,"version":version,"wa_str":opaque}})),
        Reply::Json(json!({"code":1,"data":{"wa_id":719,"version":version,"wa_str":opaque}})),
    ]));
    let mut provider = provider(&dir, &server, true);
    let uncertain = provider.update_wa(&request).unwrap_err();
    assert_eq!(uncertain.details["submission"], "uncertain");
    let wrong = provider.update_wa(&request).unwrap_err();
    assert_eq!(wrong.exit_code, 6);
    assert!(!serde_json::to_string(&wrong).unwrap().contains(opaque));
    let result = provider.update_wa(&request).unwrap();
    assert_eq!(result["version"], version);
    assert_eq!(result["wa_content_verification"], "readback");
    assert_eq!(result["recovered"], true);
    let record = provider
        .home
        .join("publish/newbee/records/wa/719/active.json");
    assert!(!std::fs::read_to_string(record).unwrap().contains(opaque));
    let requests = server.finish();
    assert_eq!(requests.len(), 8);
    let next = requests
        .iter()
        .filter(|r| r.line == "POST /creator/wow/wa/get_next_version HTTP/1.1")
        .count();
    assert_eq!(next, 1);
    let writes = requests
        .iter()
        .filter(|r| r.line == "POST /creator/wow/wa/update_wa_str HTTP/1.1")
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 1);
    let body: Value = serde_json::from_slice(&writes[0].body).unwrap();
    assert_eq!(body["version"], version);
    assert_eq!(body["wa_str"], opaque);
    assert_eq!(body["wa_str_titles"], json!([]));
}
