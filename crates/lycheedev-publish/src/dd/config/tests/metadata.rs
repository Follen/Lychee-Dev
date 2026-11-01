use super::*;
#[test]
fn legacy_root_accounts_keep_the_same_public_selectors_and_exact_wire_role_selection() {
    let d = TestDir::new();
    let home = d.0.join("home");
    let request = parse(WriteAction::Create, &create(), &d);
    let mut b = backup("backup-one", 10001);
    let accounts = b["wtf"]["accounts"].clone();
    b.as_object_mut().unwrap().remove("wtf");
    b["wtf_accounts"] = accounts;
    for account in b["wtf_accounts"].as_array_mut().unwrap() {
        let role = &mut account["servers"][0]["items"][0];
        role["id"] = json!(42);
        role["role_id"] = json!("");
    }
    let mut after = stored();
    after["wtf"]["accounts"][0]["servers"][0]["items"][0]["id"] = json!(42);
    after["wtf"]["accounts"][0]["servers"][0]["items"][0]["role_id"] = json!("");
    let catalog = read::backup_view(&b, "backup-one").unwrap();
    assert_eq!(catalog["wtf_roles"][0]["selector"], ROLE_A);
    assert_eq!(catalog["wtf_roles"][1]["selector"], ROLE_B);
    let mut replies = preflight(None, b);
    replies.extend([
        games(),
        reply(json!({"share_sn":"share-selected"})),
        reply(after.clone()),
        authored(),
    ]);
    let server = Server::new(replies);
    let mut a = author(&server, &home);
    run(&request, &mut a, None).unwrap();
    a.http.close().unwrap();
    let body = bodies(&server.finish()).remove(0);
    assert_eq!(body["wtf"], after["wtf"]);
}
#[test]
fn metadata_edit_preserves_backup_content_and_layouts_and_applies_exact_paid_private_controls() {
    let d = TestDir::new();
    let home = d.0.join("home");
    let request = parse(
        WriteAction::Edit,
        &json!({"schema":"lycheedev.publish.v1.dd.config.edit","share_sn":"share-selected","title":"新配置","scope":"private","share_code_life_type":"fourteen_day","need_buy":true,"price_fen":19999,"buy_life_type":"thirty_day","jump_room":false,"with_associate":false,"need_anchor_vip":false}),
        &d,
    );
    let mut current = stored();
    current["need_buy"] = json!(true);
    current["price_fen"] = json!(10);
    current["jump_room"] = json!(true);
    current["room_id"] = json!("old");
    current["channel_id"] = json!("old");
    current["channel_type"] = json!("2");
    current["sync_room"] = json!(true);
    current["need_anchor_vip"] = json!(true);
    current["vip_levels"] = json!([7]);
    current["with_associate"] = json!(true);
    current["associated_acts"] = json!([{"sn":"old","act_type":"wa"}]);
    let mut after = stored();
    after["title"] = json!("新配置");
    after["scope"] = json!("private");
    after["share_code_life_type"] = json!("fourteen_day");
    after["need_buy"] = json!(true);
    after["price_fen"] = json!(19999);
    after["buy_life_type"] = json!("thirty_day");
    let mut replies = preflight(Some(current), backup("backup-one", 10001));
    replies.extend([games(), reply(json!({})), reply(after), authored()]);
    let server = Server::new(replies);
    let mut a = author(&server, &home);
    run(&request, &mut a, None).unwrap();
    a.http.close().unwrap();
    let requests = server.finish();
    let body = bodies(&requests).remove(0);
    assert_eq!(body["backup_sn"], "backup-one");
    assert_eq!(body["retail_ui_config"], stored()["retail_ui_config"]);
    assert_eq!(body["known_addon"], stored()["known_addon"]);
    assert_eq!(body["price_fen"].to_string(), "19999");
    assert_eq!(body["need_buy"], 1);
    assert_eq!(body["room_id"], "");
    assert_eq!(body["vip_levels"], json!([]));
    assert_eq!(body["associated_acts"], json!([]));
}
#[test]
fn retail_spec_conflicts_and_nonretail_layouts_are_rejected_and_null_is_an_explicit_clear() {
    let d = TestDir::new();
    for game in [10001, 10002] {
        let mut doc = create();
        if game == 10001 {
            doc["retail_ui_config"]["cool_down_selectors"] =
                json!([COOL_A, "cd_526248af5ae75e997a32"]);
        }
        let request = parse(WriteAction::Create, &doc, &d);
        let server = Server::new(preflight(None, backup("backup-one", game)));
        let home = d.0.join(format!("bad-{game}"));
        let mut a = author(&server, &home);
        assert_eq!(run(&request, &mut a, None).unwrap_err().exit_code, 4);
        a.http.close().unwrap();
        assert!(bodies(&server.finish()).is_empty());
    }
    let mut doc = create();
    doc["retail_ui_config"] = Value::Null;
    let request = parse(WriteAction::Create, &doc, &d);
    let mut after = stored();
    after["game_type"] = json!(10002);
    after["retail_ui_config"] = Value::Null;
    let mut replies = preflight(None, backup("backup-one", 10002));
    replies.extend([
        games(),
        reply(json!({"share_sn":"share-selected"})),
        reply(after),
        authored(),
    ]);
    let server = Server::new(replies);
    let home = d.0.join("clear");
    let mut a = author(&server, &home);
    run(&request, &mut a, None).unwrap();
    a.http.close().unwrap();
    assert_eq!(bodies(&server.finish())[0]["retail_ui_config"], Value::Null);
}
