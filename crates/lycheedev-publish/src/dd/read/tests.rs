use super::*;
use crate::{
    dd::{DdTransport, tests::transport},
    test_support::{Reply, Server},
};
use std::collections::BTreeMap;
fn reply(value: Value) -> Reply {
    Reply::Json(json!({"code":0,"result":value}))
}
fn clock() -> Reply {
    Reply::Json(json!({"code":200,"result":{"ts":1793500000}}))
}
fn game() -> NonZeroU64 {
    NonZeroU64::new(10001).unwrap()
}
fn request_url(request: &crate::test_support::Request) -> reqwest::Url {
    reqwest::Url::parse(&format!(
        "http://loopback.test{}",
        request.line.split_whitespace().nth(1).unwrap()
    ))
    .unwrap()
}
fn run(client: &mut DdTransport, request: Read) -> Result<Value> {
    let control = OperationControl::default();
    client.begin_operation(&control)?;
    request.execute(client, &control)
}

#[test]
fn author_pages_send_the_selected_search_and_report_page_coverage_without_claiming_a_full_list() {
    let server = Server::new(vec![
        clock(),
        reply(
            json!({"items":[{"sn":"chosen-sn","title":"标题","latest_version":{"version":"v2"},"scope":false,"status":0,"game_type":10001,"token":"private-token"}],"total":3}),
        ),
        reply(json!({"shares":[],"total":0})),
    ]);
    let (mut client, _) = transport(&server, "synthetic-token");
    let value = run(
        &mut client,
        Read::List {
            resource: Resource::Plugin,
            game_type: game(),
            keyword: "姓名 &?".into(),
            page: Page::new(2, 1).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(value["items"][0]["reference"], "chosen-sn");
    assert_eq!(value["items"][0]["version"], "v2");
    assert_eq!(value["items"][0]["scope"], false);
    assert_eq!(value["items"][0]["status"], 0);
    assert_eq!(value["total"], 3);
    assert_eq!(value["returned"], 1);
    assert_eq!(value["has_more"], true);
    assert_eq!(value["next_page"], 3);
    assert_eq!(value["complete_author_list"], false);
    assert_eq!(value["truncated"], true);
    assert!(!value.to_string().contains("private-token"));
    let empty = run(
        &mut client,
        Read::List {
            resource: Resource::Config,
            game_type: game(),
            keyword: "配置 +".into(),
            page: Page::new(1, 50).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(empty["complete_author_list"], true);
    assert_eq!(empty["truncated"], false);
    assert_eq!(empty["total"], 0);
    client.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 3);
    assert_eq!(request_url(&requests[1]).path(), "/addon/addon_list");
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[1].body).unwrap(),
        json!({"game_type":10001,"origin":"created","page":2,"size":1,"category":0,"name_or_author_name_or_share_code":"姓名 &?","sort_type":2})
    );
    let url = request_url(&requests[2]);
    assert_eq!(url.path(), "/share/list");
    let query = url.query_pairs().collect::<BTreeMap<_, _>>();
    assert_eq!(query["origin"], "created");
    assert_eq!(query["search_text"], "配置 +");
    assert_eq!(query["sort_type"], "mtime");
}
#[test]
fn associated_options_collect_complete_author_pages_and_preserve_act_namespaces() {
    let plugins=(0..100).map(|n|json!({"sn":format!("p-{n}"),"name":format!("插件{n}"),"version":"v1","content":"private-content"})).collect::<Vec<_>>();
    let server = Server::new(vec![
        clock(),
        reply(json!({"items":plugins,"total":101})),
        reply(json!({"items":[{"sn":"p-100","name":"最后插件"}],"total":101})),
        reply(
            json!({"shares":[{"share_sn":"c-1","title":"配置","version":9007199254740993u64}],"total":1}),
        ),
        reply(json!({"wa_list":[]})),
    ]);
    let (mut client, _) = transport(&server, "synthetic-token");
    let value = run(&mut client, Read::AssociatedActs(game())).unwrap();
    assert_eq!(value["total"], 102);
    assert_eq!(value["complete"], true);
    assert_eq!(value["items"][100]["sn"], "p-100");
    assert_eq!(value["items"][0]["act_type"], "addon");
    assert_eq!(value["items"][101]["act_type"], "share");
    assert_eq!(
        value["items"][101]["version"].as_u64(),
        Some(9007199254740993)
    );
    assert!(!value.to_string().contains("private-content"));
    client.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 5);
    for (index, page) in [(1, 1), (2, 2)] {
        let body: Value = serde_json::from_slice(&requests[index].body).unwrap();
        assert_eq!(body["page"], page);
        assert_eq!(body["size"], 100);
    }
    assert_eq!(request_url(&requests[4]).path(), "/wa/list");
    assert_eq!(
        request_url(&requests[4])
            .query_pairs()
            .find(|(key, _)| key == "category_id")
            .unwrap()
            .1,
        ""
    );
}
#[test]
fn complete_options_reject_repeated_changing_and_prematurely_empty_author_pages_without_other_requests()
 {
    for next in [
        json!({"items":[{"sn":"one"}],"total":2}),
        json!({"items":[{"sn":"two"}],"total":3}),
        json!({"items":[],"total":2}),
    ] {
        let server = Server::new(vec![
            clock(),
            reply(json!({"items":[{"sn":"one"}],"total":2})),
            reply(next),
        ]);
        let (mut client, _) = transport(&server, "synthetic-token");
        let error = run(&mut client, Read::AssociatedActs(game())).unwrap_err();
        assert_eq!(error.exit_code, 5);
        client.close().unwrap();
        let requests = server.finish();
        assert_eq!(requests.len(), 3);
        assert!(
            requests[1..]
                .iter()
                .all(|r| request_url(r).path() == "/addon/addon_list")
        );
    }
}
#[test]
fn dynamic_options_keep_exact_values_and_category_dependencies_and_static_life_choices_need_no_client()
 {
    let server = Server::new(vec![
        clock(),
        reply(
            json!([{"game_type":9007199254740993u64,"name":"新客户端","def_game_version":"12.1.5","type":0}]),
        ),
        reply(
            json!({"versions":["12.1.0",{"value":"12.1.5","label":"正式"},{"game_version":"12.1.5"}]}),
        ),
        reply(
            json!({"categories":[{"c_id":1,"name":"主类","children":[{"category_id":2,"label":"子类"}]}]}),
        ),
        reply(
            json!({"categories":[{"name":"非选择组","items":[{"id":9,"name":"职业","options":[{"value":"九-1","label":"专精"}]}]}]}),
        ),
        reply(
            json!([{"level":0,"name":"基础","url":"https://objects.invalid/private-key","token":"private-token"}]),
        ),
    ]);
    let (mut client, _) = transport(&server, "synthetic-token");
    let types = run(&mut client, Read::GameTypes).unwrap();
    assert_eq!(
        types["items"][0]["game_type"].as_u64(),
        Some(9007199254740993)
    );
    assert_eq!(types["items"][0]["type"], 0);
    let versions = run(&mut client, Read::GameVersions(game())).unwrap();
    assert_eq!(versions["total"], 2);
    assert_eq!(versions["items"][1]["value"], "12.1.5");
    let categories = run(&mut client, Read::PluginCategories).unwrap();
    assert_eq!(
        categories["items"][0]["children"][0]["primary_category_id"],
        "1"
    );
    let wa = run(&mut client, Read::WaCategories(game())).unwrap();
    assert_eq!(wa["total"], 2);
    assert_eq!(wa["items"][0]["parent_id"], Value::Null);
    assert_eq!(wa["items"][1]["parent_id"], "9");
    let vip = run(&mut client, Read::VipLevels).unwrap();
    assert_eq!(vip["items"][0]["level"], 0);
    assert!(!vip.to_string().contains("private-key"));
    assert!(!vip.to_string().contains("private-token"));
    client.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 6);
    let paths = requests[1..]
        .iter()
        .map(|request| request_url(request).path().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "/game_type/list",
            "/game_versions/list",
            "/addon/category",
            "/wa/categories",
            "/anchor_vip/level/list"
        ]
    );
    assert!(
        request_url(&requests[5])
            .query_pairs()
            .any(|(key, value)| key == "enrich_acts" && value == "false")
    );
    assert_eq!(
        life_types()["items"],
        json!([{"name":"7 days","value":"seven_day"},{"name":"14 days","value":"fourteen_day"},{"name":"30 days","value":"thirty_day"},{"name":"60 days","value":"sixty_day"},{"name":"90 days","value":"ninety_day"},{"name":"forever","value":"forever"}])
    );
}
#[test]
fn detail_reads_keep_metadata_and_original_selection_while_summarizing_raw_imports_and_retained_retail_data()
 {
    let server = Server::new(vec![
        clock(),
        reply(Value::Null),
        reply(
            json!({"sn":"plugin &+","name":"插件","scope":false,"content":"private-description-body","roleobj":{"private":"value"},"wtflist":["private-wtf"],"detail_url":"https://cdn.invalid/private-archive","file_path":"https://cdn.invalid/path?token=private-token","token":"private-token"}),
        ),
        reply(
            json!({"share_sn":"config","name":"配置","retail_ui_config":{"edit_mode":{"account-a":[{"name":"Raid","is_default":false,"import_string":"private-edit"}]},"cool_down":{"account-a":[{"name":"Fire","char":"角色","spec_tag":63,"import_string":"private-cool"}]},"enable_dd_setup_wizard":false}}),
        ),
        reply(
            json!({"sn":"wa","name":"WA","wa_str":"!WA:2!private-opaque","raw_content":null,"value":9007199254740993u64}),
        ),
    ]);
    let (mut client, _) = transport(&server, "synthetic-token");
    let plugin = run(
        &mut client,
        Read::Detail {
            resource: Resource::Plugin,
            reference: Reference::parse("plugin &+").unwrap(),
        },
    )
    .unwrap();
    assert_eq!(plugin["scope"], false);
    assert_eq!(plugin["roleobj_summary"]["present"], true);
    assert_eq!(plugin["wtflist_summary"]["items"], 1);
    assert_eq!(plugin["file_path_summary"]["host"], "cdn.invalid");
    assert!(!plugin.to_string().contains("private-"));
    let config = run(
        &mut client,
        Read::Detail {
            resource: Resource::Config,
            reference: Reference::parse("config").unwrap(),
        },
    )
    .unwrap();
    assert_eq!(
        config["retail_ui_config"]["edit_modes"][0]["is_default"],
        false
    );
    assert_eq!(
        config["retail_ui_config"]["cool_down"][0]["character"],
        "角色"
    );
    assert_eq!(config["retail_ui_config"]["enable_dd_setup_wizard"], false);
    assert!(!config.to_string().contains("private-"));
    let wa = run(
        &mut client,
        Read::Detail {
            resource: Resource::Wa,
            reference: Reference::parse("wa").unwrap(),
        },
    )
    .unwrap();
    assert_eq!(wa["value"].as_u64(), Some(9007199254740993));
    assert_eq!(wa["raw_content_summary"]["present"], false);
    assert!(!wa.to_string().contains("private-"));
    client.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 5);
    assert_eq!(request_url(&requests[1]).path(), "/addon/detail_v2");
    assert_eq!(request_url(&requests[2]).path(), "/addon/detail");
    assert!(
        request_url(&requests[2])
            .query_pairs()
            .any(|(key, value)| key == "sn" && value == "plugin &+")
    );
}
#[test]
fn backup_metadata_returns_account_bound_content_and_independent_stable_role_and_retail_selectors()
{
    let backup = json!({"sn":"backup-A","name":"备份","ctime":1793500000,"game_type":10001,
        "known_addon":{"items":[{"addon_id":9007199254740993u64,"name":"Known","detail_sn":"public-child","dirs":["private-dir"]}]},
        "unknown_addon":{"items":["LooseAddon"]},"known_wa":{"items":[{"uid":"uid-a","name":"Known WA","content":"private-content"}]},
        "extra":{"wa_account_info":{"account-b":[{"uid":"uid-a"}],"account-a":[{"uid":"uid-a"}]}},
        "wtf":{"accounts":[{"name":"account-a","servers":[{"name":"Realm","items":[{"role_id":"role-a","name":"角色"},{"role_id":"role-a","name":"重复角色"}]}]}]},
        "retail_ui_config":{"editMode":{"account-a":[{"name":"Raid","import_string":"private-edit"}]},"coolDown":{"account-a":[{"name":"Fire","char":"角色","realm":"Realm","spec_tag":63,"import_string":"private-cool"}]}}});
    let server = Server::new(vec![
        clock(),
        reply(json!({"items":[backup.clone()],"total":1})),
        reply(backup),
    ]);
    let (mut client, _) = transport(&server, "synthetic-token");
    let list = run(&mut client, Read::Backups).unwrap();
    assert_eq!(list["complete"], true);
    assert_eq!(list["items"][0]["counts"]["known_addon"], 1);
    assert_eq!(list["items"][0]["created_at"], 1793500000);
    let detail = run(
        &mut client,
        Read::Backup(Reference::parse("backup-A").unwrap()),
    )
    .unwrap();
    assert_eq!(
        detail["known_addon"][0]["reference"].as_u64(),
        Some(9007199254740993)
    );
    assert_eq!(
        detail["known_addon"][0]["content_reference"],
        "public-child"
    );
    assert_eq!(detail["known_addon"][0]["entry_count"], 1);
    assert_eq!(
        detail["known_wa"][0]["accounts"],
        json!(["account-a", "account-b"])
    );
    // SHA-256 expectations were independently computed with Node crypto from
    // the documented NUL-separated SN/account/realm/position identities.
    assert_eq!(
        detail["wtf_roles"][0]["selector"],
        "wtf_d3234d58ba2eb6f31b52"
    );
    assert_eq!(
        detail["wtf_roles"][1]["selector"],
        "wtf_96d22bc2b3c52afeebea"
    );
    assert_eq!(
        detail["retail_ui_config"]["edit_modes"][0]["selector"],
        "em_66e32fe74d717eb6f9cb"
    );
    assert_eq!(
        detail["retail_ui_config"]["cool_down"][0]["selector"],
        "cd_b0438e8578e1faca65ca"
    );
    assert_eq!(
        detail["retail_ui_config"]["cool_down"][0]["character"],
        "角色"
    );
    assert!(!list.to_string().contains("private-"));
    assert!(!detail.to_string().contains("private-"));
    client.close().unwrap();
    assert_eq!(server.finish().len(), 3);
}
#[test]
fn version_pages_expose_observed_coverage_and_never_publish_download_capabilities() {
    let server = Server::new(vec![
        clock(),
        reply(
            json!({"items":[{"id":9007199254740993u64,"version":"next","file_path":"https://cdn.invalid/private?sig=private-capability","hash":"private-hash"}],"total":2}),
        ),
        reply(json!({"items":[],"total":0})),
    ]);
    let (mut client, _) = transport(&server, "synthetic-token");
    let request = || Read::PluginVersions {
        reference: Reference::parse("chosen").unwrap(),
        game_type: game(),
        page: NonZeroU64::new(1).unwrap(),
    };
    let value = run(&mut client, request()).unwrap();
    assert_eq!(value["complete_version_list"], false);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["items"][0]["id"].as_u64(), Some(9007199254740993));
    assert!(!value.to_string().contains("private-"));
    assert_eq!(
        run(&mut client, request()).unwrap()["complete_version_list"],
        true
    );
    client.close().unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), 3);
    assert!(
        request_url(&requests[1])
            .query_pairs()
            .any(|(key, value)| key == "page" && value == "1")
    );
}
#[test]
fn malformed_or_wrong_selected_data_is_not_an_empty_success_and_a_failed_dependency_post_is_not_repeated()
 {
    for (request, payload) in [
        (Read::GameTypes, json!({"unknown":[]})),
        (
            Read::WaCategories(game()),
            json!({"categories":"private-raw"}),
        ),
        (
            Read::Detail {
                resource: Resource::Wa,
                reference: Reference::parse("selected").unwrap(),
            },
            json!({"sn":"different","content":"private-raw"}),
        ),
        (
            Read::Backup(Reference::parse("selected").unwrap()),
            json!({"sn":"different","wtf":"private-raw"}),
        ),
    ] {
        let server = Server::new(vec![clock(), reply(payload)]);
        let (mut client, _) = transport(&server, "synthetic-token");
        let error = run(&mut client, request).unwrap_err();
        assert_eq!(error.exit_code, 5);
        assert!(!error.to_string().contains("private-raw"));
        client.close().unwrap();
        assert_eq!(server.finish().len(), 2);
    }
    let server = Server::new(vec![clock(), Reply::Drop]);
    let (mut client, _) = transport(&server, "synthetic-token");
    run(
        &mut client,
        Read::List {
            resource: Resource::Plugin,
            game_type: game(),
            keyword: "chosen".into(),
            page: Page::new(1, 50).unwrap(),
        },
    )
    .unwrap_err();
    client.close().unwrap();
    assert_eq!(server.finish().len(), 2);
    let server = Server::new(vec![]);
    let (mut client, _) = transport(&server, "synthetic-token");
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        Read::GameTypes
            .execute(&mut client, &control)
            .unwrap_err()
            .exit_code,
        7
    );
    client.close().unwrap();
    assert!(server.finish().is_empty());
    assert!(Page::new(0, 50).is_err());
    assert!(Page::new(1, 101).is_err());
    assert!(Reference::parse("").is_err());
    assert!(Reference::parse("secret\0input").is_err());
}
