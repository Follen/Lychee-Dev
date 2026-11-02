//! Public DD metadata and backup selectors. Opaque content stays behind the
//! author boundary; selectors bind its original SN/account/position identity.
use super::*;
use serde_json::Map;
use std::collections::BTreeMap;

pub(super) fn first(value: &Value, names: &[&str]) -> Value {
    names
        .iter()
        .find_map(|name| {
            value
                .get(*name)
                .filter(|v| !v.is_null() && v.as_str() != Some(""))
                .cloned()
        })
        .unwrap_or(Value::Null)
}
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) if n.as_u64().is_some() => Some(n.to_string()),
        _ => None,
    }
}
pub(super) fn identifier(row: &Value, names: &[&str]) -> Result<String> {
    scalar(&first(row, names)).ok_or_else(|| data("DD item omitted a valid resource identity"))
}
fn category_id(row: &Value) -> Result<Option<String>> {
    if !row.is_object() {
        return Err(data("DD category is not an object"));
    }
    let value = first(row, &["id", "c_id", "category_id", "value"]);
    if value.is_null() {
        Ok(None)
    } else {
        scalar(&value)
            .map(Some)
            .ok_or_else(|| data("DD category identity has an invalid type"))
    }
}
pub(super) fn project(row: &Value, names: &[&str]) -> Result<Value> {
    if !row.is_object() {
        return Err(data("DD metadata item is not an object"));
    }
    Ok(Value::Object(
        names
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    row.get(*name).cloned().unwrap_or(Value::Null),
                )
            })
            .collect(),
    ))
}
pub(super) fn author_item(resource: Resource, row: &Value) -> Result<Value> {
    let mut version = first(row, &["version", "current_version"]);
    if version.is_null() {
        version = match row.get("latest_version") {
            Some(Value::Object(value)) => value.get("version").cloned().unwrap_or(Value::Null),
            Some(value) => value.clone(),
            None => Value::Null,
        };
    }
    Ok(
        json!({"kind":resource.name(),"reference":identifier(row,&["share_sn","sn"] )?,"name":first(row,&["title","name"]),
        "version":version,"scope":row.get("scope"),"status":first(row,&["status","audit_status","state"]),
        "game_type":row.get("game_type"),"updated_at":first(row,&["mtime","update_time"])}),
    )
}
pub(super) fn game_versions(payload: &Value, game_type: NonZeroU64) -> Result<Value> {
    let mut values = Vec::new();
    let mut seen = BTreeSet::new();
    for row in options(payload)? {
        let value = if row.is_object() {
            first(row, &["game_version", "version", "value", "id"])
        } else {
            row.clone()
        };
        let key = scalar(&value).ok_or_else(|| data("DD game version omitted its value"))?;
        if !seen.insert(key) {
            continue;
        }
        let mut name = first(row, &["name", "label"]);
        if name.is_null() {
            name = value.clone();
        }
        values.push(json!({"value":value,"name":name,"game_type":game_type.get()}));
    }
    Ok(json!({"parent":{"game_type":game_type.get()},"items":values,"total":values.len()}))
}
pub(super) fn plugin_categories(payload: &Value) -> Result<Value> {
    let mut values = Vec::new();
    for row in options(payload)? {
        let Some(id) = category_id(row)? else {
            continue;
        };
        let mut children = Vec::new();
        for child in array(row, "children")? {
            if let Some(child_id) = category_id(child)? {
                children.push(json!({"id":child_id,"name":first(child,&["name","label"]),"primary_category_id":id}));
            }
        }
        values.push(json!({"id":id,"name":first(row,&["name","label"]),"children":children}));
    }
    Ok(
        json!({"items":values,"total":values.len(),"dependencies":[{"parent":"primary_category_id","child":"second_category_ids"}]}),
    )
}
pub(super) fn wa_categories(
    payload: &Value,
    game_type: NonZeroU64,
    control: &OperationControl,
) -> Result<Value> {
    fn visit(
        rows: &[Value],
        parent: Option<&str>,
        game_type: NonZeroU64,
        values: &mut Vec<Value>,
        control: &OperationControl,
    ) -> Result<()> {
        for row in rows {
            control.check()?;
            let id = category_id(row)?;
            if let Some(id) = &id {
                values.push(json!({"id":id,"name":first(row,&["name","label"]),"parent_id":parent,"game_type":game_type.get()}));
            }
            for key in ["children", "items", "options"] {
                visit(
                    array(row, key)?,
                    id.as_deref().or(parent),
                    game_type,
                    values,
                    control,
                )?;
            }
        }
        Ok(())
    }
    let mut values = Vec::new();
    visit(options(payload)?, None, game_type, &mut values, control)?;
    Ok(json!({"parent":{"game_type":game_type.get()},"items":values,"total":values.len()}))
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value]> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(rows)) => Ok(rows),
        _ => Err(data("DD metadata group is not an array")),
    }
}
fn object<'a>(value: &'a Value, key: &str) -> Result<Option<&'a Map<String, Value>>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(value)) => Ok(Some(value)),
        _ => Err(data("DD metadata group is not an object")),
    }
}
fn group<'a>(value: &'a Value, key: &str) -> Result<&'a [Value]> {
    let Some(value) = object(value, key)? else {
        return Ok(&[]);
    };
    match value.get("items") {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(rows)) => Ok(rows.as_slice()),
        _ => Err(data("DD backup items are not an array")),
    }
}
fn counts(value: &Value) -> Result<Value> {
    let mut counts = Map::new();
    for name in [
        "known_addon",
        "unknown_addon",
        "material",
        "font",
        "known_wa",
        "unknown_wa",
    ] {
        counts.insert(name.into(), json!(group(value, name)?.len()));
    }
    let accounts = backup_accounts(value)?;
    counts.insert("wtf_accounts".into(), json!(accounts.len()));
    Ok(Value::Object(counts))
}
pub(super) fn backup_accounts(value: &Value) -> Result<&[Value]> {
    let accounts = match value.get("wtf") {
        None | Some(Value::Null) => &[][..],
        Some(v) if v.is_object() => array(v, "accounts")?,
        _ => return Err(data("DD backup WTF group is not an object")),
    };
    if !accounts.is_empty() {
        Ok(accounts)
    } else {
        array(value, "wtf_accounts")
    }
}
pub(super) fn role_identity(role: &Value) -> Result<Value> {
    let value = if role.is_object() {
        first(role, &["role_id", "id", "name"])
    } else {
        role.clone()
    };
    scalar(&value).ok_or_else(|| data("DD backup role omitted its identity"))?;
    Ok(value)
}
pub(super) fn backup_item(value: &Value) -> Result<Value> {
    let reference = identifier(value, &["sn", "backup_sn"])?;
    Ok(
        json!({"reference":reference,"sn":reference,"name":value.get("name"),"game_type":value.get("game_type"),"created_at":value.get("ctime"),"counts":counts(value)?}),
    )
}
pub(super) fn selector(prefix: &str, parts: &[String]) -> String {
    format!(
        "{prefix}{}",
        &lycheedev_core::storage::sha256(parts.join("\0").as_bytes())[..20]
    )
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| data("DD backup account or realm name is unavailable"))
}
pub(super) fn backup_detail(value: &Value, expected: &str) -> Result<Value> {
    let value = if let Some(values) = value.as_array() {
        if values.len() != 1 {
            return Err(data("DD backup detail is not one selected backup"));
        }
        &values[0]
    } else {
        value
    };
    if identifier(value, &["sn", "backup_sn"])? != expected {
        return Err(data("DD backup detail does not match the selected SN"));
    }
    let mut result = backup_item(value)?;
    let mut accounts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if let Some(extra) = object(value, "extra")? {
        match extra.get("wa_account_info") {
            None | Some(Value::Null) => {}
            Some(Value::Object(groups)) => {
                for (account, rows) in groups {
                    let rows = rows
                        .as_array()
                        .ok_or_else(|| data("DD backup WA account group is not an array"))?;
                    for row in rows {
                        let uid = identifier(row, &["uid"])?;
                        accounts.entry(uid).or_default().insert(account.clone());
                    }
                }
            }
            _ => return Err(data("DD backup WA account selection is not an object")),
        }
    }
    for (name, key) in [
        ("known_addon", "addon_id"),
        ("unknown_addon", "name"),
        ("material", "name"),
        ("font", "name"),
        ("known_wa", "uid"),
        ("unknown_wa", "uid"),
    ] {
        let wa = name == "known_wa" || name == "unknown_wa";
        let rows=group(value,name)?.iter().map(|item|{
            if item.is_object() {
                let reference=item.get(key).cloned().ok_or_else(||data("DD backup content omitted its selection key"))?;
                let selected=scalar(&reference).ok_or_else(||data("DD backup content has an invalid selection key"))?;
                let entries=if item.get("dirs").is_some_and(|value|!value.is_null()) {array(item,"dirs")?} else {array(item,"items")?};
                Ok(json!({"reference":reference,"name":item.get("name"),"content_reference":if name=="known_addon" {item.get("detail_sn")} else {None},
                    "accounts":if wa {Some(accounts.get(&selected).cloned().unwrap_or_default())} else {None},"release_type":item.get("release_type"),"entry_count":entries.len()}))
            } else {
                let reference=scalar(item).ok_or_else(||data("DD backup content is not selectable metadata"))?;
                Ok(json!({"reference":item,"name":reference}))
            }
        }).collect::<Result<Vec<_>>>()?;
        result[name] = json!(rows);
    }
    let mut roles = Vec::new();
    {
        for account in backup_accounts(value)? {
            let account_name = text(account, "name")?;
            for server in array(account, "servers")? {
                let realm = text(server, "name")?;
                for (index, role) in array(server, "items")?.iter().enumerate() {
                    let role_id = role_identity(role)?;
                    let role_key = scalar(&role_id)
                        .ok_or_else(|| data("DD backup role omitted its identity"))?;
                    let selected = selector(
                        "wtf_",
                        &[
                            expected.into(),
                            account_name.into(),
                            realm.into(),
                            index.to_string(),
                            role_key,
                        ],
                    );
                    roles.push(json!({"selector":selected,"account":account_name,"server":realm,
                        "name":if role.is_object() {role.get("name").cloned().unwrap_or(Value::Null)} else {role.clone()},
                        "role_id":first(role,&["role_id","id"])}));
                }
            }
        }
    }
    result["wtf_roles"] = json!(roles);
    if object(value, "retail_ui_config")?.is_some() {
        result["retail_ui_config"] = retail_catalog(value, expected)?;
    }
    result["dependencies"] = json!([{"parent":"backup_sn","children":["wtf_role_ids","content_groups","retail_ui_config"]},
        {"parent":"wtf_role_ids.account","children":["known_wa_ids","unknown_wa_ids"]}]);
    Ok(result)
}
fn retail_catalog(value: &Value, reference: &str) -> Result<Value> {
    let retail = &value["retail_ui_config"];
    let mut edits = Vec::new();
    let mut cool = Vec::new();
    for (section, prefix) in [("editMode", "em_"), ("coolDown", "cd_")] {
        if let Some(accounts) = object(retail, section)? {
            for (account, rows) in accounts {
                let rows = rows
                    .as_array()
                    .ok_or_else(|| data("DD retail account entries are not an array"))?;
                for (index, item) in rows.iter().enumerate() {
                    let mut entry = project(
                        item,
                        if section == "editMode" {
                            &["name"]
                        } else {
                            &[
                                "name",
                                "char",
                                "realm",
                                "class_name",
                                "spec_name",
                                "spec_tag",
                            ]
                        },
                    )?;
                    entry["selector"] = json!(selector(
                        prefix,
                        &[
                            reference.into(),
                            section.into(),
                            account.clone(),
                            index.to_string()
                        ]
                    ));
                    entry["account"] = json!(account);
                    if section == "editMode" {
                        edits.push(entry);
                    } else {
                        let character = entry.as_object_mut().unwrap().remove("char").unwrap();
                        entry["character"] = character;
                        cool.push(entry);
                    }
                }
            }
        }
    }
    Ok(
        json!({"edit_modes":edits,"cool_down":cool,"constraints":{"edit_mode_max":5,"edit_mode_default_required_when_selected":true,"cool_down_max_per_spec_tag":1}}),
    )
}
pub(super) fn current_retail(value: &Value) -> Result<Value> {
    if !value.is_object() && !value.is_null() {
        return Err(data("DD current retail configuration is not an object"));
    }
    let mut edits = Vec::new();
    let mut cool = Vec::new();
    for section in ["edit_mode", "cool_down"] {
        if let Some(accounts) = object(value, section)? {
            for (account, rows) in accounts {
                let rows = rows
                    .as_array()
                    .ok_or_else(|| data("DD current retail entries are not an array"))?;
                for item in rows {
                    let mut entry = project(
                        item,
                        if section == "edit_mode" {
                            &["name", "is_default"]
                        } else {
                            &[
                                "name",
                                "char",
                                "realm",
                                "class_name",
                                "spec_name",
                                "spec_tag",
                            ]
                        },
                    )?;
                    entry["account"] = json!(account);
                    if section == "edit_mode" {
                        edits.push(entry);
                    } else {
                        let character = entry.as_object_mut().unwrap().remove("char").unwrap();
                        entry["character"] = character;
                        cool.push(entry);
                    }
                }
            }
        }
    }
    Ok(
        json!({"edit_modes":edits,"cool_down":cool,"enable_dd_setup_wizard":value.get("enable_dd_setup_wizard")}),
    )
}
pub(super) fn safe_detail(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    let raw = [
                        "content",
                        "raw_content",
                        "wa_str",
                        "roleobj",
                        "wtflist",
                        "file_url",
                        "detail_url",
                        "upload_url",
                        "url",
                        "u_url",
                        "md5",
                        "hash",
                        "dir_md5",
                        "import_string",
                    ];
                    if raw.contains(&key.to_ascii_lowercase().as_str()) {
                        let summary = match value {
                            Value::String(text) => json!({"length":text.chars().count()}),
                            Value::Array(values) => json!({"items":values.len()}),
                            other => json!({"present":!other.is_null()}),
                        };
                        (format!("{key}_summary"), summary)
                    } else if ["file_path", "d_url"].contains(&key.as_str()) && value.is_string() {
                        let host = reqwest::Url::parse(value.as_str().unwrap())
                            .ok()
                            .and_then(|url| url.host_str().map(str::to_owned));
                        (format!("{key}_summary"), json!({"host":host}))
                    } else {
                        (key, safe_detail(value))
                    }
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(safe_detail).collect()),
        other => other,
    }
}
