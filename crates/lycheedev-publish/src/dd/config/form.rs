use super::*;
use std::collections::BTreeSet;
pub(super) const FIELDS: &[&str] = &[
    "backup_sn",
    "scope",
    "title",
    "brief_desc",
    "desc",
    "update_desc",
    "display_imgs",
    "share_code_life_type",
    "creation_statement",
    "need_buy",
    "price_fen",
    "buy_life_type",
    "jump_room",
    "room_id",
    "channel_id",
    "channel_type",
    "sync_room",
    "with_associate",
    "associated_acts",
    "need_anchor_vip",
    "vip_levels",
    "known_addon",
    "unknown_addon",
    "wtf",
    "material",
    "font",
    "known_wa",
    "unknown_wa",
    "retail_ui_config",
];
pub(super) fn nested(doc: &Map<String, Value>) -> Result<()> {
    forms::admit_controls(doc)?;
    for &(group, _, selection, updates) in selection::GROUPS {
        for key in [selection, updates] {
            if let Some(rows) = doc.get(key).and_then(Value::as_array) {
                for v in rows {
                    let valid = if group == "known_addon" {
                        v.is_i64() || v.is_u64()
                    } else {
                        v.as_str().is_some_and(|s| !s.is_empty())
                    };
                    if !valid {
                        return Err(invalid(
                            &format!("$.{key}"),
                            if group == "known_addon" {
                                "addon IDs must be integers"
                            } else {
                                "content selectors must be nonempty strings"
                            },
                        ));
                    }
                }
            }
        }
    }
    if let Some(rows) = doc.get("wtf_role_ids").and_then(Value::as_array) {
        if rows.len() > 1
            || rows
                .iter()
                .any(|v| !v.as_str().is_some_and(|s| !s.is_empty()))
        {
            return Err(invalid(
                "$.wtf_role_ids",
                "select at most one nonempty WTF role selector",
            ));
        }
    }
    for key in ["display_imgs", "display_img_files"] {
        if let Some(rows) = doc.get(key).and_then(Value::as_array) {
            for v in rows {
                let text = v
                    .as_str()
                    .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control))
                    .ok_or_else(|| {
                        invalid(
                            &format!("$.{key}"),
                            "image entries must be nonblank scalar strings",
                        )
                    })?;
                if key == "display_imgs" {
                    forms::permanent(text, "$.display_imgs")?;
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
            "configuration images cannot exceed eight in total",
        ));
    }
    if let Some(value) = doc.get("retail_ui_config").filter(|v| !v.is_null()) {
        admit_retail(value)?;
    }
    Ok(())
}
fn admit_retail(value: &Value) -> Result<()> {
    let o = value
        .as_object()
        .ok_or_else(|| invalid("$.retail_ui_config", "expected a selector object or null"))?;
    if o.keys().any(|k| {
        !matches!(
            k.as_str(),
            "edit_mode_selectors"
                | "default_edit_mode_selector"
                | "cool_down_selectors"
                | "enable_dd_setup_wizard"
        )
    }) {
        return Err(invalid(
            "$.retail_ui_config",
            "unknown retail selector field",
        ));
    }
    for key in ["edit_mode_selectors", "cool_down_selectors"] {
        if let Some(v) = o.get(key) {
            let a = v.as_array().ok_or_else(|| {
                invalid("$.retail_ui_config", "selector collections must be arrays")
            })?;
            let mut seen = BTreeSet::new();
            for s in a {
                let s = s.as_str().filter(|s| !s.is_empty()).ok_or_else(|| {
                    invalid("$.retail_ui_config", "selectors must be nonempty strings")
                })?;
                if !seen.insert(s) {
                    return Err(invalid("$.retail_ui_config", "duplicate selector"));
                }
            }
            if key == "edit_mode_selectors" && a.len() > 5 {
                return Err(invalid(
                    "$.retail_ui_config.edit_mode_selectors",
                    "at most five edit modes may be selected",
                ));
            }
        }
    }
    if let Some(v) = o.get("enable_dd_setup_wizard") {
        if !v.is_boolean() {
            return Err(invalid(
                "$.retail_ui_config.enable_dd_setup_wizard",
                "expected a boolean",
            ));
        }
    }
    if let Some(v) = o.get("default_edit_mode_selector") {
        if !v.is_string() {
            return Err(invalid(
                "$.retail_ui_config.default_edit_mode_selector",
                "expected a string",
            ));
        }
    }
    if let Some(edits) = o.get("edit_mode_selectors").and_then(Value::as_array) {
        let default = o
            .get("default_edit_mode_selector")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !edits.is_empty() && default.is_empty()
            || !default.is_empty() && !edits.iter().any(|v| v.as_str() == Some(default))
        {
            return Err(invalid(
                "$.retail_ui_config.default_edit_mode_selector",
                "a selected edit mode must be the explicit default",
            ));
        }
    } else if o.contains_key("default_edit_mode_selector") {
        return Err(invalid(
            "$.retail_ui_config.default_edit_mode_selector",
            "default selector requires edit_mode_selectors",
        ));
    }
    Ok(())
}
pub(super) fn schema(action: WriteAction) -> Value {
    let mut value = contract::input_shape(Provider::Dd, "config", action.name())
        .unwrap()
        .schema();
    value.as_object_mut().unwrap().remove("x-validation-scope");
    let p = value["properties"].as_object_mut().unwrap();
    for &(group, _, selected, updated) in selection::GROUPS {
        for key in [selected, updated] {
            if let Some(v) = p.get_mut(key) {
                v["items"] = if group == "known_addon" {
                    json!({"type":"integer"})
                } else {
                    json!({"type":"string","minLength":1})
                };
            }
        }
    }
    if let Some(v) = p.get_mut("wtf_role_ids") {
        v["items"] = json!({"type":"string","minLength":1});
        v["maxItems"] = json!(1);
    }
    for key in ["display_imgs", "display_img_files"] {
        if let Some(v) = p.get_mut(key) {
            v["items"] = json!({"type":"string","minLength":1});
        }
    }
    for key in ["backup_sn", "share_sn"] {
        if let Some(v) = p.get_mut(key) {
            v["maxLength"] = json!(1024);
        }
    }
    if let Some(v) = p.get_mut("vip_levels") {
        v["items"] = json!({"type":"integer","minimum":0});
    }
    if let Some(v) = p.get_mut("price_fen") {
        v["anyOf"] = json!([{"const":0},{"minimum":10,"maximum":20000}]);
    }
    for key in ["share_code_life_type", "buy_life_type"] {
        if let Some(v) = p.get_mut(key) {
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
    if let Some(v) = p.get_mut("associated_acts") {
        v["items"] = json!({"type":"object","required":["sn","act_type"],"additionalProperties":false,"properties":{"sn":{"type":"string","minLength":1},"act_type":{"enum":["addon","share","wa"]}}});
        v["uniqueItems"] = json!(true);
    }
    if let Some(v) = p.get_mut("retail_ui_config") {
        *v = json!({"type":["object","null"],"additionalProperties":false,"properties":{"edit_mode_selectors":{"type":"array","maxItems":5,"uniqueItems":true,"items":{"type":"string","minLength":1}},"default_edit_mode_selector":{"type":"string"},"cool_down_selectors":{"type":"array","uniqueItems":true,"items":{"type":"string","minLength":1}},"enable_dd_setup_wizard":{"type":"boolean"}},"dependentRequired":{"default_edit_mode_selector":["edit_mode_selectors"]},"allOf":[{"if":{"required":["edit_mode_selectors"],"properties":{"edit_mode_selectors":{"minItems":1}}},"then":{"required":["default_edit_mode_selector"],"properties":{"default_edit_mode_selector":{"minLength":1}}}}]});
    }
    if action == WriteAction::Create {
        value["allOf"] = json!([
            {"anyOf":[{"required":["display_imgs"],"properties":{"display_imgs":{"minItems":1}}},{"required":["display_img_files"],"properties":{"display_img_files":{"minItems":1}}}]},
            {"if":{"required":["scope"],"properties":{"scope":{"const":"private"}}},"then":{"anyOf":[{"required":["need_buy"],"properties":{"need_buy":{"const":true}}},{"required":["share_code_life_type"]}]}}
        ]);
    }
    forms::schema_conditions(&mut value, Resource::Config, action);
    value["x-dynamic-options"] = json!({"backup_sn":"publish dd config backups","content_and_role_selectors":"publish dd config backup-get --sn EXACT_BACKUP_SN","retail_ui_config":"exact em_/cd_ selectors from the selected backup; null clears; omitted fields preserve the same backup's current wire object"});
    value["x-execution"] = json!({"session":"explicit retained DDT","backup_change":"complete reselection of all seven content groups/role fields; retail backup also requires explicit retail_ui_config, including null","roles":"at most one explicit role; old role values accepted only when unique; WA belongs to its WTF account","update_markers":"must be a subset of the effective selection; existing per-content versions increment exactly","unknown_create":"recover the original input with explicit --recover-sn","readback":"all submitted metadata, complete content groups and original layout data verified by digest; public results contain summaries"});
    value
}
pub(super) fn build(current: &Value, backup: &Value, doc: &Value, changed: bool) -> Result<Value> {
    let mut body = json!({"scope":"public","backup_sn":"","desc":"","update_desc":"","title":"","display_imgs":[],"share_code_life_type":"seven_day","brief_desc":"","price_fen":0,"need_buy":0,"buy_life_type":"seven_day",
        "jump_room":false,"room_id":"","channel_id":"","channel_type":"","sync_room":false,"creation_statement":"","with_associate":false,"associated_acts":[],"need_anchor_vip":false,"vip_levels":[]});
    let keys = body
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        if let Some(v) = current.get(&key).filter(|v| !v.is_null()) {
            body[&key] = v.clone();
        }
        if let Some(v) = doc.get(&key) {
            body[&key] = v.clone();
        }
    }
    selection::groups(backup, current, doc, &mut body)?;
    if let Some(v) = current.get("retail_ui_config").filter(|v| !v.is_null()) {
        body["retail_ui_config"] = v.clone();
    }
    let game = forms::game(backup)?;
    if changed && game.get() == 10001 && doc.get("retail_ui_config").is_none() {
        return Err(invalid(
            "$.retail_ui_config",
            "a selected retail backup requires explicit retail_ui_config, including null to clear it",
        ));
    }
    if let Some(v) = doc.get("retail_ui_config") {
        body["retail_ui_config"] =
            selection::retail(backup, if changed { &Value::Null } else { current }, v)?;
    } else if game.get() != 10001 {
        body.as_object_mut().unwrap().remove("retail_ui_config");
    }
    body["display_imgs"] = forms::remote_array(
        body.get("display_imgs"),
        &["d_url", "url", "media_url", "value"],
        false,
        false,
    )?;
    body["vip_levels"] = forms::remote_array(
        body.get("vip_levels"),
        &["level", "id", "value"],
        true,
        false,
    )?;
    forms::normalize(
        body.as_object_mut().unwrap(),
        Resource::Config,
        current.is_null(),
    )?;
    if !current.is_null() {
        forms::locked_usage(current, &body, doc)?;
    }
    Ok(body)
}
pub(super) fn validate(body: &Value, doc: &Value) -> Result<()> {
    for key in ["backup_sn", "title", "brief_desc", "desc"] {
        if !forms::truth(&body[key]) {
            return Err(invalid(
                &format!("$.{key}"),
                "field is required by the DD configuration editor",
            ));
        }
    }
    let count = body["display_imgs"].as_array().map_or(0, Vec::len)
        + doc["display_img_files"].as_array().map_or(0, Vec::len);
    if count == 0 || count > 8 {
        return Err(invalid(
            "$.display_imgs",
            "provide one to eight display images in total",
        ));
    }
    for v in body["display_imgs"]
        .as_array()
        .ok_or_else(|| data("DD display images are not an array"))?
    {
        forms::permanent(
            v.as_str()
                .ok_or_else(|| data("DD image is not a scalar URL"))?,
            "$.display_imgs",
        )?;
    }
    for &key in FIELDS {
        if let Some(v) = body.get(key) {
            if !matches!(
                key,
                "associated_acts"
                    | "known_addon"
                    | "unknown_addon"
                    | "wtf"
                    | "material"
                    | "font"
                    | "known_wa"
                    | "unknown_wa"
                    | "retail_ui_config"
            ) && (v.is_object()
                || v.as_array()
                    .is_some_and(|a| a.iter().any(|v| v.is_object() || v.is_array())))
            {
                return Err(data(
                    "DD configuration wire form contains an unexpected display object",
                ));
            }
        }
    }
    forms::validate_controls(body, Resource::Config)
}
pub(super) fn project(raw: &Value) -> Result<Value> {
    if !raw.is_object() {
        return Err(data("DD configuration readback is not an object"));
    }
    let mut value = raw.clone();
    if value.get("need_buy").is_some() {
        value["need_buy"] = json!(u8::from(forms::truth(&value["need_buy"])));
    }
    for &(key, _, _, _) in selection::GROUPS {
        if value.get(key).is_none_or(Value::is_null) {
            value[key] = json!({"items":[],"inner_version":{}});
        }
    }
    if value.get("wtf").is_none_or(Value::is_null) {
        value["wtf"] = json!({"accounts":[]});
    }
    if value.get("vip_levels").is_none_or(Value::is_null) {
        value["vip_levels"] = json!([]);
    }
    Ok(value)
}
