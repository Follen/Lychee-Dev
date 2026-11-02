use super::*;
pub(super) const FIELDS: &[&str] = &[
    "game_type",
    "scope",
    "name",
    "game_version",
    "brief_desc",
    "display_imgs",
    "category_ids",
    "content",
    "desc",
    "update_desc",
    "version",
    "with_file",
    "file_path",
    "file_install_path",
    "parse_wa_uid",
    "parse_wa_id",
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
pub(super) fn nested(doc: &Map<String, Value>, control: &OperationControl) -> Result<()> {
    forms::admit_controls(doc)?;
    if let Some(v) = doc.get("game_type") {
        if !v.as_u64().is_some_and(|n| n > 0) {
            return Err(invalid("$.game_type", "game_type must be positive"));
        }
    }
    for key in ["display_imgs", "display_img_files"] {
        if let Some(rows) = doc.get(key).and_then(Value::as_array) {
            for row in rows {
                let s = row
                    .as_str()
                    .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
                    .ok_or_else(|| {
                        invalid(
                            &format!("$.{key}"),
                            "items must be nonblank scalar strings without controls",
                        )
                    })?;
                if key == "display_imgs" {
                    forms::permanent(s, "$.display_imgs")?;
                }
            }
        }
    }
    if doc
        .get("display_imgs")
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
        + doc
            .get("display_img_files")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
        > 8
    {
        return Err(invalid(
            "$.display_img_files",
            "WA images cannot exceed eight in total",
        ));
    }
    if let Some(rows) = doc.get("category_ids").and_then(Value::as_array) {
        for row in rows {
            if forms::scalar(row).is_none_or(|s| s.is_empty()) || row.is_i64() && !row.is_u64() {
                return Err(invalid(
                    "$.category_ids",
                    "categories require scalar string or nonnegative integer IDs",
                ));
            }
        }
    }
    if let Some(content) = doc.get("content").and_then(Value::as_str) {
        if content.len() > 32 << 20 {
            return Err(invalid("$.content", "WA content exceeds 32 MiB"));
        }
        if content.starts_with("!WA:2!") {
            super::super::wa::parse(content, control)?;
        }
    }
    Ok(())
}
pub(super) fn schema(action: WriteAction) -> Value {
    let mut schema = contract::input_shape(Provider::Dd, "wa", action.name())
        .unwrap()
        .schema();
    schema.as_object_mut().unwrap().remove("x-validation-scope");
    let p = schema["properties"].as_object_mut().unwrap();
    if let Some(v) = p.get_mut("game_type") {
        v["minimum"] = json!(1);
    }
    for k in ["display_imgs", "display_img_files"] {
        if let Some(v) = p.get_mut(k) {
            v["items"] = json!({"type":"string","minLength":1});
        }
    }
    if let Some(v) = p.get_mut("category_ids") {
        v["items"] =
            json!({"anyOf":[{"type":"string","minLength":1},{"type":"integer","minimum":0}]});
    }
    if let Some(v) = p.get_mut("sn") {
        v["maxLength"] = json!(1024);
    }
    if let Some(v) = p.get_mut("vip_levels") {
        v["items"] = json!({"type":"integer","minimum":0});
    }
    if let Some(v) = p.get_mut("price_fen") {
        v["anyOf"] = json!([{"const":0},{"minimum":10,"maximum":20000}]);
    }
    if let Some(v) = p.get_mut("associated_acts") {
        v["items"] = json!({"type":"object","additionalProperties":false,"required":["sn","act_type"],"properties":{"sn":{"type":"string","minLength":1,"maxLength":1024},"act_type":{"enum":["addon","share","wa"]}}});
        v["uniqueItems"] = json!(true);
    }
    for k in ["share_code_life_type", "buy_life_type"] {
        if let Some(v) = p.get_mut(k) {
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
    if action != WriteAction::Edit {
        schema["properties"]["version"]["pattern"] = json!("^[0-9]+$");
    }
    if action == WriteAction::Create {
        schema["allOf"] = json!([
            {"anyOf":[{"required":["display_imgs"],"properties":{"display_imgs":{"minItems":1}}},{"required":["display_img_files"],"properties":{"display_img_files":{"minItems":1}}}]},
            {"if":{"required":["with_file"],"properties":{"with_file":{"const":true}}},"then":{"required":["file","file_install_path"],"properties":{"file":{"minLength":1},"file_install_path":{"minLength":1}}}}
        ]);
    }
    schema["x-execution"] = json!({"session":"explicit retained DDT","content":"modern !WA:2! identity parsed as pure Rust data; older imports clear parse IDs; original content bytes are verified without exposing them","version_update":"ASCII digit version, exactly greater than current numeric value; no float conversion","materials":"file forces with_file=true; ZIP at most 50 MiB; original-byte author-scoped upload receipts","locked":"game type and outer free/paid usage mode","unknown_create":"recover original input with explicit --recover-sn"});
    forms::schema_conditions(&mut schema, Resource::Wa, action);
    schema
}
pub(super) fn project(raw: &Value) -> Result<Value> {
    if !raw.is_object() {
        return Err(data("DD WA detail is not an object"));
    }
    let mut body = Map::new();
    for &k in FIELDS {
        if let Some(v) = raw.get(k) {
            body.insert(k.into(), v.clone());
        }
    }
    if !body.get("scope").is_some_and(forms::truth) {
        body.insert("scope".into(), json!("public"));
    }
    if !body.get("price_fen").is_some_and(forms::truth) {
        body.insert("price_fen".into(), json!(0));
    }
    if !body.get("version").is_some_and(forms::truth) {
        body.insert("version".into(), json!("0"));
    }
    body.insert(
        "vip_levels".into(),
        forms::remote_array(
            body.get("vip_levels"),
            &["level", "id", "value"],
            true,
            false,
        )?,
    );
    if let Some(v) = body.get("game_type") {
        body.insert(
            "game_type".into(),
            forms::integer(v, &["game_type", "id", "value"])?,
        );
    }
    if body.contains_key("display_imgs") {
        body.insert(
            "display_imgs".into(),
            forms::remote_array(
                body.get("display_imgs"),
                &["d_url", "url", "media_url", "value"],
                false,
                false,
            )?,
        );
    }
    if body.contains_key("category_ids") {
        body.insert(
            "category_ids".into(),
            forms::remote_array(
                body.get("category_ids"),
                &["c_id", "category_id", "id", "value"],
                false,
                true,
            )?,
        );
    }
    forms::normalize(&mut body, Resource::Wa, false)?;
    Ok(json!(body))
}
pub(super) fn build(
    current: Option<&Value>,
    doc: &Value,
    action: WriteAction,
    control: &OperationControl,
) -> Result<Value> {
    let mut body = if let Some(current) = current {
        project(current)?
    } else {
        json!({"share_code_life_type":"seven_day","need_buy":false,"buy_life_type":"seven_day","category_ids":["ui_original"],"file_install_path":"Interface/Addons","vip_levels":[],"version":"0"})
    };
    for &key in FIELDS {
        if let Some(v) = doc.get(key) {
            body[key] = v.clone();
        }
    }
    if let Some(current) = current {
        if doc.get("game_type").is_some()
            && doc["game_type"].as_u64() != Some(forms::game(current)?.get())
        {
            return Err(invalid(
                "$.game_type",
                "game_type is locked after WA creation",
            ));
        }
        if forms::truth(&current["assign_user_sn"]) && body["scope"] != "public" {
            return Err(invalid(
                "$.scope",
                "assigned WA records require public scope",
            ));
        }
        if action == WriteAction::Update
            && !greater(
                body["version"].as_str().unwrap(),
                forms::scalar(&current["version"]).as_deref().unwrap_or("0"),
            )?
        {
            return Err(invalid(
                "$.version",
                "version must be strictly greater than the current numeric version",
            ));
        }
        body["sn"] = doc["sn"].clone();
    }
    if doc.get("file").is_some() {
        body["with_file"] = json!(true);
    }
    let content = body["content"].as_str().unwrap_or("");
    let identity = if content.starts_with("!WA:2!") {
        serde_json::to_value(super::super::wa::parse(content, control)?)?
    } else {
        json!({"parse_wa_uid":"","parse_wa_id":""})
    };
    for key in ["parse_wa_uid", "parse_wa_id"] {
        body[key] = identity[key].clone();
    }
    let categories = body["category_ids"]
        .as_array()
        .ok_or_else(|| data("DD WA categories are not an array"))?
        .iter()
        .map(|v| {
            forms::scalar(v)
                .map(Value::String)
                .ok_or_else(|| data("DD WA category has an invalid scalar identity"))
        })
        .collect::<Result<Vec<_>>>()?;
    body["category_ids"] = json!(categories);
    forms::normalize(
        body.as_object_mut().unwrap(),
        Resource::Wa,
        action == WriteAction::Create,
    )?;
    if let Some(current) = current {
        forms::locked_usage(current, &body, doc)?;
    }
    Ok(body)
}
pub(super) fn validate(body: &Value, doc: &Value) -> Result<()> {
    for k in [
        "name",
        "game_version",
        "brief_desc",
        "category_ids",
        "content",
        "desc",
        "update_desc",
    ] {
        if !forms::truth(&body[k]) {
            return Err(invalid(
                &format!("$.{k}"),
                "field is required by the DD WA editor",
            ));
        }
    }
    forms::game(body)?;
    let count = body["display_imgs"].as_array().map_or(0, Vec::len)
        + doc["display_img_files"].as_array().map_or(0, Vec::len);
    if count == 0 || count > 8 {
        return Err(invalid(
            "$.display_imgs",
            "provide one to eight display images in total",
        ));
    }
    if let Some(rows) = body["display_imgs"].as_array() {
        for row in rows {
            forms::permanent(
                row.as_str()
                    .ok_or_else(|| data("DD WA image is not a URL string"))?,
                "$.display_imgs",
            )?;
        }
    }
    if forms::truth(&body["with_file"]) {
        if !forms::truth(&body["file_path"]) && !forms::truth(&doc["file"]) {
            return Err(invalid(
                "$.file",
                "with_file requires an existing or new material ZIP",
            ));
        }
        if !matches!(
            body["file_install_path"].as_str(),
            Some("Interface" | "Interface/Addons")
        ) {
            return Err(invalid(
                "$.file_install_path",
                "enabled WA material requires Interface or Interface/Addons",
            ));
        }
    }
    if let Some(url) = body["file_path"].as_str().filter(|s| !s.is_empty()) {
        forms::permanent(url, "$.file_path")?;
    }
    for &k in FIELDS {
        if let Some(v) = body.get(k) {
            if k != "associated_acts"
                && (v.is_object()
                    || v.as_array()
                        .is_some_and(|a| a.iter().any(|v| v.is_object() || v.is_array())))
            {
                return Err(data(
                    "DD WA wire form contains an unexpected display object",
                ));
            }
        }
    }
    forms::validate_controls(body, Resource::Wa)
}
/// DD increments an integer version against the current Decimal value. Compare
/// decimal places directly so long versions and fractional old values stay exact.
pub(super) fn greater(candidate: &str, current: &str) -> Result<bool> {
    admit_version(candidate)?;
    let candidate = candidate.trim();
    let candidate = candidate.trim_start_matches('0');
    let current = current.trim();
    let (negative, current) = if let Some(v) = current.strip_prefix('-') {
        (true, v)
    } else {
        (false, current.strip_prefix('+').unwrap_or(current))
    };
    let (mantissa, exponent) = if let Some((m, e)) = current.split_once(['e', 'E']) {
        (
            m,
            e.parse::<i32>()
                .map_err(|_| data("current DD WA version has an invalid numeric exponent"))?
                as i64,
        )
    } else {
        (current, 0)
    };
    let mut parts = mantissa.split('.');
    let whole = parts.next().unwrap_or("");
    let fractional = parts.next().unwrap_or("");
    if parts.next().is_some()
        || whole.is_empty() && fractional.is_empty()
        || !whole
            .bytes()
            .chain(fractional.bytes())
            .all(|c| c.is_ascii_digit())
    {
        return Err(data("current DD WA version is not a finite decimal"));
    }
    let all = format!("{whole}{fractional}");
    let digits = all.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(!candidate.is_empty());
    }
    if negative {
        return Ok(true);
    }
    if candidate.is_empty() {
        return Ok(false);
    }
    let current_places = digits.len() as i64 + exponent - fractional.len() as i64;
    let order = (candidate.len() as i64).cmp(&current_places);
    if !order.is_eq() {
        return Ok(order.is_gt());
    }
    for (index, c) in candidate.bytes().enumerate() {
        let other = digits.as_bytes().get(index).copied().unwrap_or(b'0');
        if c != other {
            return Ok(c > other);
        }
    }
    Ok(false)
}
pub(super) fn admit_version(value: &str) -> Result<()> {
    let v = value;
    if v.is_empty() || !v.bytes().all(|c| c.is_ascii_digit()) {
        return Err(invalid(
            "$.version",
            "WA update version must contain only ASCII digits",
        ));
    }
    Ok(())
}
