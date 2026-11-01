use super::*;
pub(super) const FIELDS: &[&str] = &[
    "game_type",
    "game_versions",
    "scope",
    "addon_type",
    "name",
    "description",
    "logo",
    "detail_imgs",
    "primary_category_id",
    "second_category_ids",
    "detail_url",
    "release_type",
    "version",
    "html_desc",
    "update_desc",
    "share_code_life_type",
    "need_buy",
    "price_fen",
    "buy_life_type",
    "jump_room",
    "room_id",
    "channel_id",
    "channel_type",
    "sync_room",
    "creation_statement",
    "with_associate",
    "associated_acts",
    "need_anchor_vip",
    "vip_levels",
];
pub(super) const VERSION_FIELDS: &[&str] = &[
    "game_versions",
    "detail_url",
    "release_type",
    "version",
    "update_desc",
];
const ID_ALIASES: &[&str] = &["c_id", "category_id", "id", "value"];
pub(super) fn nested(doc: &Map<String, Value>) -> Result<()> {
    forms::admit_controls(doc)?;
    if let Some(v) = doc.get("game_type") {
        if !v.as_u64().is_some_and(|n| n > 0) {
            return Err(invalid(
                "$.game_type",
                "game_type must be a positive integer",
            ));
        }
    }
    for key in ["game_versions", "detail_imgs", "detail_img_files"] {
        if let Some(rows) = doc.get(key) {
            let rows = rows
                .as_array()
                .ok_or_else(|| invalid(&format!("$.{key}"), "expected an array"))?;
            for row in rows {
                let text = row
                    .as_str()
                    .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
                    .ok_or_else(|| {
                        invalid(
                            &format!("$.{key}"),
                            "items must be nonblank scalar strings without control characters",
                        )
                    })?;
                if key == "detail_imgs" {
                    forms::permanent(text, "$.detail_imgs")?;
                }
            }
        }
    }
    if let Some(rows) = doc.get("second_category_ids") {
        if !rows
            .as_array()
            .is_some_and(|rows| rows.iter().all(Value::is_u64))
        {
            return Err(invalid(
                "$.second_category_ids",
                "child categories must be scalar nonnegative integer IDs",
            ));
        }
    }
    for key in ["logo", "detail_url"] {
        if let Some(v) = doc
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            forms::permanent(v, &format!("$.{key}"))?;
        }
    }
    if doc
        .get("detail_imgs")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
        + doc
            .get("detail_img_files")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
        > 8
    {
        return Err(invalid(
            "$.detail_img_files",
            "plugin detail images cannot exceed eight in total",
        ));
    }
    Ok(())
}
pub(super) fn schema(action: WriteAction) -> Value {
    let mut value = contract::input_shape(Provider::Dd, "plugin", action.name())
        .unwrap()
        .schema();
    value.as_object_mut().unwrap().remove("x-validation-scope");
    let properties = value["properties"].as_object_mut().unwrap();
    if let Some(file) = properties.get_mut("file") {
        file["description"] = json!(
            "Existing valid .zip archive. Both central-directory and local-header entry names must use forward slashes, without backslashes. Admission inspects original locked bytes without extracting or rebuilding the archive."
        );
    }
    for key in ["game_type", "primary_category_id"] {
        if let Some(v) = properties.get_mut(key) {
            v["minimum"] = json!(1);
        }
    }
    for key in ["game_versions", "detail_imgs", "detail_img_files"] {
        if let Some(v) = properties.get_mut(key) {
            v["items"] = json!({"type":"string","minLength":1});
        }
    }
    for key in ["second_category_ids", "vip_levels"] {
        if let Some(v) = properties.get_mut(key) {
            v["items"] = json!({"type":"integer","minimum":0});
        }
    }
    for key in ["share_code_life_type", "buy_life_type"] {
        if let Some(v) = properties.get_mut(key) {
            v["enum"] = json!([
                "seven_day",
                "fourteen_day",
                "thirty_day",
                "sixty_day",
                "ninety_day",
                "forever"
            ]);
        }
    }
    if let Some(v) = properties.get_mut("price_fen") {
        v["anyOf"] = json!([{"const":0},{"minimum":10,"maximum":20000}]);
    }
    if let Some(v) = properties.get_mut("associated_acts") {
        v["items"] = json!({"type":"object","additionalProperties":false,"required":["sn","act_type"],"properties":{"sn":{"type":"string","minLength":1,"maxLength":1024},"act_type":{"enum":["addon","share","wa"]}}});
        v["uniqueItems"] = json!(true);
    }
    if let Some(v) = properties.get_mut("sn") {
        v["maxLength"] = json!(1024);
    }
    if action == WriteAction::Create {
        value["allOf"] = json!([
            {"anyOf":[{"required":["logo"],"properties":{"logo":{"minLength":1}}},{"required":["logo_file"],"properties":{"logo_file":{"minLength":1}}}]},
            {"anyOf":[{"required":["detail_imgs"],"properties":{"detail_imgs":{"minItems":1}}},{"required":["detail_img_files"],"properties":{"detail_img_files":{"minItems":1}}}]},
            {"anyOf":[{"required":["detail_url"],"properties":{"detail_url":{"minLength":1}}},{"required":["file"],"properties":{"file":{"minLength":1}}}]}
        ]);
    }
    value["x-dynamic-options"] = json!({"game_type":"publish dd options game-types","game_versions":"publish dd plugin game-versions","primary_category_id":"publish dd plugin categories","second_category_ids":"children of primary_category_id","vip_levels":"publish dd options vip-levels","associated_acts":"publish dd options associated-acts","channel_id":"publish dd options channels"});
    value["x-execution"] = json!({"session":"explicit retained DDT","local_files":"original SHA256 and size bound before authentication; uploads recheck original bytes","edit":"preserve published author-list version; free/paid mode is locked","unknown_create":"recover original input with explicit --recover-sn; never create again","readback":"all submitted stable fields, with separate complete version projection for create/update"});
    forms::schema_conditions(&mut value, Resource::Plugin, action);
    value
}
pub(super) fn project(raw: &Value, author: Option<&Value>) -> Result<Value> {
    if !raw.is_object() {
        return Err(data("DD plugin projection is not an object"));
    }
    let mut form = Map::new();
    for &k in FIELDS {
        if let Some(v) = raw.get(k) {
            form.insert(k.into(), v.clone());
        }
    }
    for (key, latest_key) in [
        ("detail_url", "file_path"),
        ("release_type", "release_type"),
        ("version", "version"),
    ] {
        let v = author
            .and_then(|v| v.get("latest_version"))
            .and_then(|v| v.get(latest_key))
            .filter(|v| !v.is_null())
            .or_else(|| {
                raw.get("latest_version")
                    .and_then(|v| v.get(latest_key))
                    .filter(|v| !v.is_null())
            });
        if let Some(v) = v {
            form.insert(key.into(), v.clone());
        } else {
            form.remove(key);
        }
    }
    if raw.get("game_type").is_some_and(forms::truth)
        || raw
            .get("game_types")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    {
        form.insert("game_type".into(), json!(forms::game(raw)?.get()));
    } else {
        form.remove("game_type");
    }
    if !form.get("scope").is_some_and(forms::truth) {
        form.insert("scope".into(), json!("public"));
    }
    if !form.get("price_fen").is_some_and(forms::truth) {
        form.insert("price_fen".into(), json!(0));
    }
    form.insert(
        "vip_levels".into(),
        forms::remote_array(
            form.get("vip_levels"),
            &["level", "id", "value"],
            true,
            false,
        )?,
    );
    if form.contains_key("game_versions") {
        form.insert(
            "game_versions".into(),
            forms::remote_array(
                form.get("game_versions"),
                &["version", "build", "value"],
                false,
                false,
            )?,
        );
    }
    if form.contains_key("detail_imgs") {
        form.insert(
            "detail_imgs".into(),
            forms::remote_array(
                form.get("detail_imgs"),
                &["d_url", "url", "media_url", "value"],
                false,
                false,
            )?,
        );
    }
    if let Some(v) = form.get("primary_category_id") {
        form.insert("primary_category_id".into(), forms::integer(v, ID_ALIASES)?);
    }
    let mut children =
        forms::remote_array(form.get("second_category_ids"), ID_ALIASES, true, false)?;
    // The official editor removes the terminal category marker from detail.
    children.as_array_mut().unwrap().pop();
    form.insert("second_category_ids".into(), children);
    forms::normalize(&mut form, Resource::Plugin, false)?;
    Ok(json!(form))
}
pub(super) fn version(raw: &Value) -> Result<Value> {
    let mut form = Map::new();
    for (key, latest_key) in [
        ("game_versions", "game_versions"),
        ("detail_url", "file_path"),
        ("release_type", "release_type"),
        ("version", "version"),
        ("update_desc", "update_desc"),
    ] {
        if let Some(v) = raw
            .get("latest_version")
            .and_then(|v| v.get(latest_key))
            .filter(|v| !v.is_null())
            .or_else(|| raw.get(key).filter(|v| !v.is_null()))
        {
            let v = if key == "game_versions" {
                forms::remote_array(Some(v), &["version", "build", "value"], false, false)?
            } else {
                v.clone()
            };
            form.insert(key.into(), v);
        }
    }
    Ok(json!(form))
}
pub(super) fn validate(form: &Value, doc: &Value) -> Result<()> {
    for key in [
        "game_versions",
        "name",
        "description",
        "primary_category_id",
        "release_type",
        "version",
        "html_desc",
        "update_desc",
    ] {
        if !forms::truth(&form[key]) {
            return Err(invalid(
                &format!("$.{key}"),
                "field is required by the DD plugin editor",
            ));
        }
    }
    forms::game(form)?;
    for (key, file) in [("logo", "logo_file"), ("detail_url", "file")] {
        if !forms::truth(&form[key]) && !forms::truth(&doc[file]) {
            return Err(invalid(
                &format!("$.{key}"),
                "provide a permanent artifact URL or its local upload file",
            ));
        }
        if let Some(v) = form[key].as_str().filter(|s| !s.is_empty()) {
            forms::permanent(v, &format!("$.{key}"))?;
        }
    }
    let current = form["detail_imgs"].as_array().map_or(0, Vec::len);
    let added = doc["detail_img_files"].as_array().map_or(0, Vec::len);
    if current + added == 0 || current + added > 8 {
        return Err(invalid(
            "$.detail_imgs",
            "provide one to eight plugin detail images in total",
        ));
    }
    if let Some(rows) = form["detail_imgs"].as_array() {
        for row in rows {
            forms::permanent(
                row.as_str()
                    .ok_or_else(|| data("DD plugin image is not a scalar URL"))?,
                "$.detail_imgs",
            )?;
        }
    }
    if !matches!(form["release_type"].as_u64(), Some(1..=3)) {
        return Err(invalid("$.release_type", "release_type must be 1, 2 or 3"));
    }
    if !matches!(form["addon_type"].as_u64(), Some(0..=1)) {
        return Err(data("DD plugin has an invalid addon_type"));
    }
    for &key in FIELDS {
        if let Some(v) = form.get(key) {
            if key != "associated_acts"
                && (v.is_object()
                    || v.as_array()
                        .is_some_and(|a| a.iter().any(|r| r.is_object() || r.is_array())))
            {
                return Err(data(
                    "DD plugin wire projection contains an unexpected display object",
                ));
            }
        }
    }
    forms::validate_controls(form, Resource::Plugin)
}
