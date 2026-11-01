//! Workshop editor fields and preservation rules; no platform policy in Skill.
use super::*;
use crate::input::invalid;
use std::collections::BTreeSet;

pub(super) const CONFIG: &[&str] = &[
    "Open",
    "Title",
    "Article",
    "UpdateLog",
    "KnownAddons",
    "UnknownAddons",
    "WTFList",
    "Materials",
    "Fonts",
    "ClearKnownAddons",
    "ClearUnknownAddons",
    "ClearMaterials",
    "ClearFonts",
    "ThumbList",
    "SubscribeLevel",
    "ChatInviteCode",
    "NeedsPush",
];
const SELECTION: &[&str] = &[
    "KnownAddons",
    "UnknownAddons",
    "WTFList",
    "Materials",
    "Fonts",
];
const WA: &[&str] = &[
    "categories",
    "strappid",
    "open",
    "name",
    "descriptionHeybox",
    "updateLog",
    "screens",
    "encoded",
    "subscribeLevel",
];

pub(super) fn text<'a>(value: &'a Value, path: &str, nonempty: bool) -> Result<&'a str> {
    value
        .as_str()
        .filter(|s| !nonempty || !s.trim().is_empty())
        .ok_or_else(|| {
            invalid(
                path,
                if nonempty {
                    "expected a nonblank string"
                } else {
                    "expected a string"
                },
            )
        })
}
fn title(value: &Value, path: &str) -> Result<()> {
    let text = text(value, path, true)?;
    let width: usize = text
        .chars()
        .map(|c| {
            if c as u32 <= 255 {
                1
            } else if c as u32 <= 65535 {
                2
            } else {
                4
            }
        })
        .sum();
    if width > 70 {
        return Err(invalid(
            path,
            "exceeds the official weighted limit of 70; Latin-1 counts 1, other BMP characters 2, astral characters 4",
        ));
    }
    Ok(())
}
pub(super) fn branch(value: &Value, path: &str) -> Result<String> {
    let s = text(value, path, true)?;
    if !s.strip_prefix("wow").is_some_and(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    }) {
        return Err(invalid(
            path,
            "select strappid from publish blackbox options game-types",
        ));
    }
    Ok(s.into())
}
pub(super) fn url(value: &Value, path: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(text(value, path, true)?)
        .map_err(|_| invalid(path, "expected an HTTPS URL"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.port().is_some_and(|p| p != 443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            path,
            "expected a credential-free HTTPS URL without a fragment",
        ));
    }
    Ok(url)
}
pub(super) fn strings(value: &Value, path: &str, nonempty: bool) -> Result<Vec<String>> {
    let rows = value
        .as_array()
        .filter(|r| !nonempty || !r.is_empty())
        .ok_or_else(|| invalid(path, "expected an explicit string array"))?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for item in rows {
        let s = text(item, path, true)?;
        if !seen.insert(s) {
            return Err(invalid(path, "array contains duplicate values"));
        }
        result.push(s.into());
    }
    Ok(result)
}
fn require(value: &Value, fields: &[&str]) -> Result<()> {
    if fields.iter().any(|f| value.get(*f).is_none()) {
        return Err(invalid(
            "$.configuration",
            "required editor fields are missing; consult schema for this action",
        ));
    }
    Ok(())
}
pub(super) fn validate_wa(value: &Value, create: bool) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("$.configuration", "expected the official WA editor object"))?;
    if object.keys().any(|k| !WA.contains(&k.as_str())) {
        return Err(invalid("$.configuration", "unknown WA editor field"));
    }
    require(
        value,
        if create {
            &[
                "categories",
                "strappid",
                "open",
                "name",
                "screens",
                "encoded",
                "subscribeLevel",
            ]
        } else {
            &["strappid", "open", "subscribeLevel"]
        },
    )?;
    for (key, item) in object {
        let path = format!("$.configuration.{key}");
        match key.as_str() {
            "strappid" => {
                branch(item, &path)?;
            }
            "open" => {
                if !item.is_boolean() {
                    return Err(invalid(
                        &path,
                        "expected a boolean; open is not a privacy control",
                    ));
                }
            }
            "subscribeLevel" => {
                if item.as_u64().is_none() {
                    return Err(invalid(&path, "expected a nonnegative integer"));
                }
            }
            "name" => title(item, &path)?,
            "encoded" => {
                text(item, &path, true)?;
            }
            "descriptionHeybox" | "updateLog" => {
                text(item, &path, false)?;
            }
            "categories" => {
                strings(item, &path, true)?;
            }
            "screens" => {
                let rows = item
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| invalid(&path, "at least one {src,thumb} image is required"))?;
                for row in rows {
                    if row.as_object().is_none_or(|o| {
                        o.len() != 2 || !o.contains_key("src") || !o.contains_key("thumb")
                    }) {
                        return Err(invalid(
                            &path,
                            "each image must contain exactly src and thumb",
                        ));
                    }
                    url(&row["src"], &path)?;
                    url(&row["thumb"], &path)?;
                }
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}
pub(super) fn wa_form(patch: &Value, current: Option<&Value>) -> Result<Value> {
    let mut form = if let Some(current) = current {
        require(
            current,
            &[
                "Name",
                "Description",
                "UpdateLog",
                "ThumbList",
                "Categories",
                "Encoded",
            ],
        )?;
        let thumbs = current["ThumbList"]
            .as_array()
            .ok_or_else(|| protocol_error("WA detail has no image array"))?;
        json!({"name":current["Name"],"descriptionHeybox":current["Description"],"updateLog":current["UpdateLog"],"categories":current["Categories"],"encoded":current["Encoded"],"screens":thumbs.iter().map(|v|json!({"src":v,"thumb":v})).collect::<Vec<_>>()})
    } else {
        json!({})
    };
    form.as_object_mut()
        .unwrap()
        .extend(patch.as_object().unwrap().clone());
    validate_wa(&form, true)?;
    Ok(form)
}
pub(super) fn wa_expected(form: &Value) -> Value {
    let mut expected = json!({"Name":form["name"],"Categories":form["categories"],"Encoded":form["encoded"],"ThumbList":form["screens"].as_array().unwrap().iter().map(|r|r["src"].clone()).collect::<Vec<_>>()});
    for (wire, detail) in [
        ("descriptionHeybox", "Description"),
        ("updateLog", "UpdateLog"),
    ] {
        if let Some(v) = form.get(wire) {
            expected[detail] = v.clone();
        }
    }
    expected
}
pub(super) fn validate_config(value: &Value, create: bool) -> Result<()> {
    let object = value
        .as_object()
        .filter(|o| !o.is_empty())
        .ok_or_else(|| invalid("$.configuration", "expected a nonempty configuration patch"))?;
    if object.keys().any(|k| !CONFIG.contains(&k.as_str())) {
        return Err(invalid("$.configuration", "unknown configuration field"));
    }
    if create {
        require(
            value,
            &[
                "Title",
                "Article",
                "Open",
                "ThumbList",
                "SubscribeLevel",
                "KnownAddons",
                "UnknownAddons",
                "Materials",
                "Fonts",
                "WTFList",
            ],
        )?;
    }
    for (key, item) in object {
        let path = format!("$.configuration.{key}");
        match key.as_str() {
            "Title" => title(item, &path)?,
            "Article" | "UpdateLog" | "ChatInviteCode" => {
                text(item, &path, false)?;
            }
            "Open" | "ClearKnownAddons" | "ClearUnknownAddons" | "ClearMaterials"
            | "ClearFonts" | "NeedsPush" => {
                if !item.is_boolean() {
                    return Err(invalid(&path, "expected a boolean"));
                }
            }
            "KnownAddons" | "WTFList" => {
                if item
                    .as_array()
                    .is_none_or(|a| a.iter().any(|v| !v.is_object()))
                {
                    return Err(invalid(
                        &path,
                        "expected an array of selected backup objects",
                    ));
                }
            }
            "ThumbList" => {
                if item
                    .as_array()
                    .is_none_or(|a| a.iter().any(|v| !v.is_string()))
                {
                    return Err(invalid(&path, "expected image URL strings"));
                }
            }
            "SubscribeLevel" => {
                if item.as_u64().is_none() {
                    return Err(invalid(&path, "expected a nonnegative integer"));
                }
            }
            "UnknownAddons" | "Materials" | "Fonts" => {
                if item["Files"]
                    .as_array()
                    .is_none_or(|a| a.iter().any(|v| !v.is_object()))
                {
                    return Err(invalid(
                        &path,
                        "expected a backup container with Files objects",
                    ));
                }
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}
fn set(form: &mut Value, key: &str, value: &Value) {
    if ["UnknownAddons", "Materials", "Fonts"].contains(&key)
        && form[key].is_object()
        && value.is_object()
    {
        form[key]
            .as_object_mut()
            .unwrap()
            .extend(value.as_object().unwrap().clone());
    } else {
        form[key] = value.clone();
    }
}
pub(super) fn config_form(patch: &Value, backup: &Value, current: Option<&Value>) -> Result<Value> {
    let mut form = backup.clone();
    if let Some(current) = current {
        let changed =
            protocol::identifier(&backup["ID"]) != protocol::identifier(&current["BackupID"]);
        if changed {
            require(patch, SELECTION)?;
        }
        require(
            current,
            &[
                "Title",
                "Article",
                "Open",
                "KnownAddons",
                "WTFList",
                "ThumbList",
                "SubscribeLevel",
            ],
        )?;
        for key in CONFIG.iter().filter(|k| **k != "NeedsPush") {
            if changed && SELECTION.contains(key) {
                continue;
            }
            if let Some(value) = current.get(*key) {
                set(&mut form, key, value);
            } else if ["UnknownAddons", "Materials", "Fonts"].contains(key) {
                form.as_object_mut().unwrap().remove(*key);
            }
        }
        if form["SubscribeLevel"].is_object() {
            form["SubscribeLevel"] = form["SubscribeLevel"]["level"].clone();
        }
    } else {
        form["UpdateLog"] = json!("<p><br></p>");
        for key in [
            "ClearKnownAddons",
            "ClearUnknownAddons",
            "ClearMaterials",
            "ClearFonts",
        ] {
            form[key] = json!(false);
        }
    }
    for (k, v) in patch.as_object().unwrap() {
        set(&mut form, k, v);
    }
    // Validate only editor fields; the complete original backup carries server metadata.
    let editor = Value::Object(
        form.as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| CONFIG.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    );
    validate_config(&editor, false)?;
    require(
        &form,
        &[
            "Title",
            "Article",
            "Open",
            "KnownAddons",
            "WTFList",
            "ThumbList",
            "SubscribeLevel",
        ],
    )?;
    if form["Open"] == true && form["ThumbList"].as_array().is_none_or(Vec::is_empty) {
        return Err(invalid(
            "$.configuration.ThumbList",
            "public shares require at least one image",
        ));
    }
    let mut content = false;
    for field in ["KnownAddons", "WTFList"] {
        let available = backup[field]
            .as_array()
            .ok_or_else(|| protocol_error("backup selection metadata is incomplete"))?;
        for row in form[field].as_array().unwrap() {
            content = true;
            let key = if field == "KnownAddons" { "ID" } else { "Name" };
            let original = available
                .iter()
                .find(|r| {
                    protocol::identifier(&r[key]).is_some()
                        && protocol::identifier(&r[key]) == protocol::identifier(&row[key])
                })
                .ok_or_else(|| {
                    invalid(
                        "$.configuration",
                        "selected addon/account is absent from the bound backup",
                    )
                })?;
            if field == "WTFList" {
                let selected = row["RealmCharacters"]
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .ok_or_else(|| {
                        invalid(
                            "$.configuration.WTFList",
                            "every selected account needs explicit RealmCharacters",
                        )
                    })?;
                for r in selected {
                    let realm = text(&r["Realm"], "$.configuration.WTFList", true)?;
                    let player = text(&r["Player"], "$.configuration.WTFList", true)?;
                    let found = original["RealmCharacters"].as_array().is_some_and(|a| {
                        a.iter()
                            .any(|v| v["Realm"] == realm && v["Player"] == player)
                    }) || original["Characters"]
                        .as_array()
                        .is_some_and(|a| a.contains(&json!(format!("{realm} - {player}"))));
                    if !found {
                        return Err(invalid(
                            "$.configuration.WTFList",
                            "selected character is absent from the bound backup",
                        ));
                    }
                }
            }
        }
    }
    for field in ["UnknownAddons", "Materials", "Fonts"] {
        if let Some(rows) = form[field]["Files"].as_array() {
            for row in rows {
                content = true;
                let name = text(&row["Name"], "$.configuration", true)?;
                if !backup[field]["Files"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v["Name"] == name))
                {
                    return Err(invalid(
                        "$.configuration",
                        "selected file is absent from the bound backup",
                    ));
                }
            }
            if rows.is_empty() {
                form.as_object_mut().unwrap().remove(field);
            }
        }
    }
    if !content {
        return Err(invalid(
            "$.configuration",
            "select at least one content item from the bound backup",
        ));
    }
    Ok(form)
}
pub(super) fn config_expected(form: &Value) -> Value {
    let mut result = json!({"BackupID":protocol::identifier(&form["ID"])});
    for key in CONFIG.iter().filter(|k| **k != "NeedsPush") {
        if let Some(value) = form.get(*key) {
            result[*key] = value.clone();
        }
    }
    for key in ["UnknownAddons", "Materials", "Fonts"] {
        if result.get(key).is_none() {
            result[key] = Value::Null;
        }
    }
    result
}
pub(super) fn normalize_config(mut row: Value) -> Value {
    row["BackupID"] = json!(protocol::identifier(&row["BackupID"]));
    if row["SubscribeLevel"].is_object() {
        row["SubscribeLevel"] = row["SubscribeLevel"]["level"].clone();
    }
    for key in ["UnknownAddons", "Materials", "Fonts"] {
        if row[key].is_null() || row[key]["Files"].as_array().is_some_and(Vec::is_empty) {
            row[key] = Value::Null;
        }
    }
    row
}
pub(super) fn backup_branch(row: &Value) -> Result<String> {
    if row["IsClassic"] == false {
        return Ok("wowretail".into());
    }
    if row.get("Strappid").is_some() {
        return branch(&row["Strappid"], "$.strappid");
    }
    if row["IsClassic"] == true {
        return Ok("wowclassicera".into());
    }
    Err(protocol_error(
        "backup/share omitted its game branch; references cannot be checked",
    ))
}
pub(super) fn schema(resource: &str, create: bool) -> Value {
    let mut properties = serde_json::Map::new();
    let required;
    if resource == "wa" {
        required = if create {
            vec![
                "categories",
                "strappid",
                "open",
                "name",
                "screens",
                "encoded",
                "subscribeLevel",
            ]
        } else {
            vec!["strappid", "open", "subscribeLevel"]
        };
        properties.insert("categories".into(),json!({"type":"array","minItems":1,"uniqueItems":true,"items":{"type":"string","minLength":1},"description":"Exact Key values from publish blackbox wa categories."}));
        properties.insert("strappid".into(),json!({"type":"string","pattern":"^wow[a-z0-9_]+$","description":"Select from publish blackbox options game-types."}));
        properties.insert("open".into(),json!({"type":"boolean","description":"Native editor setting, not a privacy control; explicit on every write."}));
        properties.insert("subscribeLevel".into(),json!({"type":"integer","minimum":0,"description":"Explicit subscription level; readback does not verify this setting."}));
        for key in ["name", "descriptionHeybox", "updateLog", "encoded"] {
            properties.insert(key.into(),json!({"type":"string","description":if key=="name" {"Nonblank title; weighted width <=70 (Latin-1=1, BMP=2, astral=4)."}else if key=="encoded" {"Nonblank opaque original WA import; never printed."}else{"HTML; omission preserves, empty string clears."}}));
        }
        properties.insert("screens".into(),json!({"type":"array","minItems":1,"items":{"type":"object","required":["src","thumb"],"additionalProperties":false,"properties":{"src":{"type":"string","format":"uri","pattern":"^https://"},"thumb":{"type":"string","format":"uri","pattern":"^https://"}}},"description":"Credential-free HTTPS references. Omission preserves; at least one image is required."}));
    } else {
        required = if create {
            vec![
                "Title",
                "Article",
                "Open",
                "ThumbList",
                "SubscribeLevel",
                "KnownAddons",
                "UnknownAddons",
                "WTFList",
                "Materials",
                "Fonts",
            ]
        } else {
            vec![]
        };
        for key in ["Title", "Article", "UpdateLog", "ChatInviteCode"] {
            properties.insert(key.into(),json!({"type":"string","description":if key=="Title" {"Nonblank; weighted width <=70 (Latin-1=1, BMP=2, astral=4)."}else{"HTML/text; omission preserves, empty string clears."}}));
        }
        for key in [
            "Open",
            "ClearKnownAddons",
            "ClearUnknownAddons",
            "ClearMaterials",
            "ClearFonts",
            "NeedsPush",
        ] {
            properties.insert(key.into(),json!({"type":"boolean","description":if key=="Open" {"False hides listing, not access by share ID. True requires ThumbList."}else{"Explicit false is retained; omission preserves."}}));
        }
        properties.insert(
            "SubscribeLevel".into(),
            json!({"type":"integer","minimum":0}),
        );
        properties.insert(
            "ThumbList".into(),
            json!({"type":"array","items":{"type":"string"}}),
        );
        properties.insert("KnownAddons".into(),json!({"type":"array","items":{"type":"object","required":["ID"],"properties":{"ID":{"type":["string","integer"]},"Name":{"type":"string"},"Strategy":{"type":"integer"},"VersionID":{"type":["string","integer"]}},"additionalProperties":true},"description":"Selected original backup objects; retain backend fields. Empty clears this selection."}));
        let role = json!({"type":"object","required":["Realm","Player"],"properties":{"Realm":{"type":"string","minLength":1},"Player":{"type":"string","minLength":1}},"additionalProperties":true});
        properties.insert("WTFList".into(),json!({"type":"array","items":{"type":"object","required":["Name","RealmCharacters"],"properties":{"Name":{"type":"string","minLength":1},"RealmCharacters":{"type":"array","minItems":1,"items":role},"Characters":{"type":"array","items":{"type":"string"}}},"additionalProperties":true},"description":"Original account objects from backup get with explicit selected RealmCharacters. Preserve original archive metadata."}));
        for key in ["UnknownAddons", "Materials", "Fonts"] {
            properties.insert(key.into(),json!({"type":"object","required":["Files"],"properties":{"Files":{"type":"array","items":{"type":"object","required":["Name"],"properties":{"Name":{"type":"string","minLength":1},"Size":{"type":"integer","minimum":0}},"additionalProperties":true}}},"additionalProperties":true,"description":"Files selected from bound backup. Original archive Host/Path/MD5 and metadata are retained; empty Files clears this selection."}));
        }
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false,"description":"Omitted editable fields preserve current values. Explicit empties follow each field. Changing config backup_id requires all five selection fields. At least one config content selection is required."})
}
