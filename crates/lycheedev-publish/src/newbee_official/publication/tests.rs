use super::super::tests::{client, me, reply};
use super::*;
use crate::test_support::{Reply, Server, TestDir};
use std::fs;
fn id(n: u64) -> NonZeroU64 {
    NonZeroU64::new(n).unwrap()
}
fn plugin(name: &str) -> Value {
    json!({"mod_id":91,"name":name,"description":"original HTML","intro":null,"content_format":1,"categories":[{"id":7},{"id":3}],"logo":"https://cdn.invalid/icon.png","screenshots":[{"url":"https://cdn.invalid/shot.png"}],"visibility":0,"subscribe_plan_level":0,"link_to_channel":false})
}
fn wa(name: &str) -> Value {
    json!({"wa_id":92,"name":name,"description":"original description","intro":"keep intro","content_format":2,"categories":[{"id":7}],"thumbnail":"https://cdn.invalid/icon.png","images":[],"visibility":2,"price":"90071992547409.93","paid_time_range":"original period","subscribe_plan_level":0,"link_to_channel":false,"is_multi_string":false})
}
fn parse(dir: &TestDir, resource: Resource, create: bool, value: Value) -> Publication {
    Publication::parse(
        resource,
        create,
        &serde_json::to_vec(&value).unwrap(),
        &dir.0,
    )
    .unwrap()
}
#[test]
fn edits_preserve_complete_forms_null_zero_false_html_and_exact_decimal_prices() {
    let dir = TestDir::new();
    let p = parse(
        &dir,
        Resource::Plugin,
        false,
        json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.edit","id":91,"name":"new name"}),
    );
    let w = parse(
        &dir,
        Resource::Wa,
        false,
        json!({"schema":"lycheedev.publish.v1.newbee.official.wa.edit","id":92,"name":"new WA","images":[],"link_channel":false,"subscribe_level":0}),
    );
    let server = Server::new(vec![
        me(71),
        reply(plugin("old name")),
        reply(json!({})),
        reply(plugin("new name")),
        reply(plugin("new name")),
        reply(wa("old WA")),
        reply(json!({})),
        reply(wa("new WA")),
    ]);
    let mut c = client(&dir, &server);
    assert_eq!(
        c.publish(&p, None).unwrap()["verification"],
        "metadata_fields"
    );
    assert_eq!(c.publish(&p, None).unwrap()["reused"], true);
    assert_eq!(
        c.publish(&w, None).unwrap()["verification"],
        "metadata_fields"
    );
    let requests = server.finish();
    let body = input::value(&requests[2].body).unwrap();
    assert_eq!(
        body,
        json!({"id":91,"name":"new name","description":"original HTML","intro":null,"content_format":1,"mod_categories":[3,7],"logo":"https://cdn.invalid/icon.png","screenshots":["https://cdn.invalid/shot.png"],"share_state":0,"subscribe_plan_level":0,"link_to_channel":false})
    );
    let body = input::value(&requests[6].body).unwrap();
    assert_eq!(body["price"], 9007199254740993u64);
    assert_eq!(body["time_range"], "original period");
    assert_eq!(body["intro"], "keep intro");
    assert_eq!(body["share_state"], 2);
    assert_eq!(body["link_to_channel"], false);
    assert!(body.get("game_version_id").is_none());
}
#[test]
fn a_lost_edit_reads_the_original_full_form_without_rebasing_or_repeating() {
    let dir = TestDir::new();
    let request = parse(
        &dir,
        Resource::Plugin,
        false,
        json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.edit","id":91,"name":"new name"}),
    );
    let mut wrong = plugin("new name");
    wrong["description"] = json!("external replacement");
    let server = Server::new(vec![
        me(71),
        reply(plugin("old name")),
        Reply::Drop,
        reply(wrong),
        reply(plugin("new name")),
    ]);
    let mut c = client(&dir, &server);
    let error = c.publish(&request, None).unwrap_err();
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.details["submission"], "uncertain");
    assert_eq!(c.publish(&request, None).unwrap_err().exit_code, 6);
    let changed = parse(
        &dir,
        Resource::Plugin,
        false,
        json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.edit","id":91,"name":"different"}),
    );
    assert_eq!(c.publish(&changed, None).unwrap_err().exit_code, 6);
    assert_eq!(c.publish(&request, None).unwrap()["recovered"], true);
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.contains("/mod/edit "))
            .count(),
        1
    );
}
fn plugin_create(dir: &TestDir) -> Publication {
    parse(
        dir,
        Resource::Plugin,
        true,
        json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.create","name":"Created","description":{"file":"description.md"},"intro":"","categories":[7,3],"logo":"https://cdn.invalid/icon.png","screenshots":["https://cdn.invalid/shot.png"],"visibility":"public","origin":"localized","home_page":"upstream","home_page_link":"https://upstream.invalid","subscribe_level":0,"link_channel":false}),
    )
}
fn created() -> Value {
    let mut value = plugin("Created");
    value["description"] = json!("new Markdown");
    value["content_format"] = json!(2);
    value["intro"] = json!("server extracted summary");
    value
}
fn categories() -> Reply {
    Reply::Json(json!({"mod_category":[{"t_id":3},{"t_id":7}]}))
}
#[test]
fn creation_with_a_lost_response_requires_its_explicit_id_and_verifies_original_metadata() {
    let dir = TestDir::new();
    fs::write(dir.0.join("description.md"), "new Markdown").unwrap();
    let request = plugin_create(&dir);
    let server = Server::new(vec![
        me(71),
        categories(),
        Reply::Drop,
        reply({
            let mut v = created();
            v["mod_id"] = json!(99);
            v
        }),
        reply(created()),
    ]);
    let mut c = client(&dir, &server);
    let error = c.publish(&request, None).unwrap_err();
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.details["submission"], "uncertain");
    assert_eq!(c.publish(&request, None).unwrap_err().exit_code, 6);
    assert_eq!(c.publish(&request, Some(id(91))).unwrap_err().exit_code, 6);
    let result = c.publish(&request, Some(id(91))).unwrap();
    assert_eq!(result["id"], 91);
    assert_eq!(result["intro_verification"], "server_derived_not_available");
    assert_eq!(result["visibility_verification"], "pending_or_unavailable");
    assert_eq!(c.publish(&request, None).unwrap()["reused"], true);
    let requests = server.finish();
    let body = input::value(&requests[2].body).unwrap();
    assert_eq!(body["content_origin"], 3);
    assert_eq!(body["share_state"], 1);
    assert_eq!(body["home_page"], "upstream");
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.contains("/mod/create "))
            .count(),
        1
    );
    let records =
        fs::read_dir(dir.0.join("publish/newbee/records/official-plugin-create")).unwrap();
    for scope in records {
        for file in fs::read_dir(scope.unwrap().path()).unwrap() {
            let path = file.unwrap().path();
            if path.extension().is_some_and(|e| e == "json") {
                let bytes = fs::read_to_string(path).unwrap();
                assert!(!bytes.contains("new Markdown"));
                assert!(!bytes.contains("upstream"));
            }
        }
    }
}
#[test]
fn wa_creation_validates_current_choices_streams_media_and_reads_original_string_content() {
    let dir = TestDir::new();
    fs::write(dir.0.join("wa.txt"), "!WA:2!PRIVATE_OPAQUE").unwrap();
    fs::write(dir.0.join("cover.png"), b"original media bytes").unwrap();
    let request = parse(
        &dir,
        Resource::Wa,
        true,
        json!({"schema":"lycheedev.publish.v1.newbee.official.wa.create","name":"new WA","description":"original description","intro":"keep intro","wa_str":{"file":"wa.txt"},"wa_log":"initial log","game_version_id":2,"categories":[7],"thumbnail":"cover.png","visibility":"private","price_yuan":"1.005","link_channel":false}),
    );
    let mut detail = wa("new WA");
    detail["price"] = json!("1.01");
    detail["paid_time_range"] = json!("");
    detail["thumbnail"] = json!("https://cdn.invalid/upload.png");
    let server = Server::new(vec![
        me(71),
        Reply::Json(json!({"game_version":[{"id":2}]})),
        reply(json!([{"id":7}])),
        reply(json!({"media_url":"https://cdn.invalid/upload.png"})),
        reply(json!({"id":92})),
        reply(detail),
        reply(json!({"wa_id":92,"wa_str":"!WA:2!PRIVATE_OPAQUE","version":"1.0.0"})),
    ]);
    let mut c = client(&dir, &server);
    let result = c.publish(&request, None).unwrap();
    assert_eq!(result["id"], 92);
    assert!(!result.to_string().contains("PRIVATE_OPAQUE"));
    let requests = server.finish();
    assert!(
        requests[3]
            .body
            .windows(b"original media bytes".len())
            .any(|v| v == b"original media bytes")
    );
    let body = input::value(&requests[4].body).unwrap();
    assert_eq!(body["price"], 101);
    assert_eq!(body["wa_str"], "!WA:2!PRIVATE_OPAQUE");
    assert_eq!(body["thumbnail"], "https://cdn.invalid/upload.png");
}
fn backup() -> Value {
    json!({"backup_id":88,"addons":[{"mod_id":"9007199254740993","name":"known"},{"mod_id":0,"name":"unknown"}],"unknown_files":[{"name":"unknown"}],"materials":[{"name":"media"}],"fonts":[{"name":"font"}],"wtf_accounts":[{"servers":[{"roles":[{"role_id":"9007199254740995"}]}]}]})
}
#[test]
fn config_creation_selects_exact_backup_roles_and_keeps_independent_resource_identity() {
    let dir = TestDir::new();
    let base = json!({"schema":"lycheedev.publish.v1.newbee.official.config.create","backup_id":88,"title":"UI","content":"# body","intro":"summary","visibility":"private","role_id":9007199254740995u64,"price_yuan":"0"});
    let request = parse(&dir, Resource::Config, true, base.clone());
    let missing = parse(&dir, Resource::Config, true, {
        let mut v = base.clone();
        v.as_object_mut().unwrap().remove("role_id");
        v
    });
    let wrong = parse(&dir, Resource::Config, true, {
        let mut v = base;
        v["role_id"] = json!(1);
        v
    });
    let detail = json!({"config_id":99,"title":"UI","intro":"summary","visibility":0,"cloud_backup":{"backup_id":88},"shared_role":{"role_id":"9007199254740995"},"linked_mod_count":1,"price":0,"subscribe_plan_level":0,"screenshots":[],"link_to_channel":false});
    let server = Server::new(vec![
        me(71),
        reply(backup()),
        reply(backup()),
        reply(backup()),
        reply(json!({"config_id":99})),
        reply(detail),
        reply(json!({"config_id":99,"content_format":2,"content":"# body"})),
    ]);
    let mut c = client(&dir, &server);
    assert_eq!(c.publish(&missing, None).unwrap_err().exit_code, 4);
    assert_eq!(c.publish(&wrong, None).unwrap_err().exit_code, 4);
    assert_eq!(c.publish(&request, None).unwrap()["id"], 99);
    let requests = server.finish();
    let body = input::value(&requests[4].body).unwrap();
    assert_eq!(body["roleid"], 9007199254740995u64);
    assert_eq!(
        body["linked_mods"],
        json!([{"mod_id":9007199254740993u64,"mod_name":"known"}])
    );
    assert_eq!(body["ignored_fronts"], json!(["font"]));
    assert_eq!(body["sharing"], 0);
    assert_eq!(body["selected_eml"], json!([]));
}
#[test]
fn local_plans_reject_bad_fields_without_authentication_and_hold_original_files() {
    let dir = TestDir::new();
    fs::write(dir.0.join("description.md"), "new Markdown").unwrap();
    let request = plugin_create(&dir);
    let selected = request.select(&OperationControl::default()).unwrap();
    assert!(fs::write(dir.0.join("description.md"), "replacement").is_err());
    selected
        .finish(Ok(json!({})), &OperationControl::default())
        .unwrap();
    let plan = request.plan(&OperationControl::default()).unwrap();
    assert_eq!(plan["credential_reads"], false);
    assert!(!plan.to_string().contains("new Markdown"));
    for value in [
        json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.edit","id":91}),
        json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.edit","id":91,"name":null}),
        json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.edit","id":91,"unknown":true}),
        json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.edit","id":91,"categories":[3,3]}),
    ] {
        assert!(
            Publication::parse(
                Resource::Plugin,
                false,
                &serde_json::to_vec(&value).unwrap(),
                &dir.0
            )
            .is_err()
        );
    }
    let req = parse(
        &dir,
        Resource::Wa,
        false,
        json!({"schema":"lycheedev.publish.v1.newbee.official.wa.edit","id":92,"images":[],"price_yuan":"0","link_channel":false}),
    );
    assert_eq!(
        req.plan(&OperationControl::default()).unwrap()["author_writes"],
        0
    );
    let server = Server::new(vec![
        me(71),
        reply({
            let mut v = wa("old");
            v["is_multi_string"] = json!(true);
            v
        }),
    ]);
    assert_eq!(
        client(&dir, &server)
            .publish(&req, None)
            .unwrap_err()
            .exit_code,
        3
    );
    server.finish();
}
#[test]
fn yuan_conversion_uses_exact_round_half_up_and_detects_overflow() {
    for (text, expected) in [
        ("0", 0),
        ("0.0049", 0),
        ("0.005", 1),
        ("1.005", 101),
        ("90071992547409.93", 9007199254740993),
        ("184467440737095516.15", u64::MAX),
        ("0001.2", 120),
    ] {
        assert_eq!(money::yuan(text).unwrap(), expected);
        assert_eq!(money::wire(&json!(text)).unwrap(), expected);
    }
    for bad in [
        "-1",
        "NaN",
        "1.2.3",
        ".5",
        "1.",
        "184467440737095516.155",
        "1e2",
    ] {
        assert!(money::yuan(bad).is_err(), "{bad}");
    }
    assert_eq!(
        money::wire(&input::value(b"1.005e0").unwrap()).unwrap(),
        101
    );
    assert_eq!(money::wire(&input::value(b"5e-3").unwrap()).unwrap(), 1);
    assert_eq!(money::wire(&input::value(b"1e-100").unwrap()).unwrap(), 0);
    assert!(money::wire(&input::value(b"1e100").unwrap()).is_err());
}

#[test]
fn completed_edits_read_external_visibility_changes_before_applying_explicit_intent() {
    let dir = TestDir::new();
    let request = parse(
        &dir,
        Resource::Plugin,
        false,
        json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.edit","id":91,"visibility":"public"}),
    );
    let state = |visibility| {
        let mut v = plugin("same name");
        v["visibility"] = json!(visibility);
        v
    };
    let server = Server::new(vec![
        me(71),
        reply(state(2)),
        reply(json!({})),
        reply(state(1)),
        reply(state(2)),
        reply(json!({})),
        reply(state(1)),
    ]);
    let mut c = client(&dir, &server);
    assert_eq!(
        c.publish(&request, None).unwrap()["visibility_verification"],
        "readback"
    );
    assert_eq!(c.publish(&request, None).unwrap()["reused"], false);
    let requests = server.finish();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.line.contains("/mod/edit "))
            .count(),
        2
    );
}
