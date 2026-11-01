//! Exact content/role/layout selection from the chosen immutable backup.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
pub(super) const GROUPS: &[(&str, Option<&str>, &str, &str)] = &[
    (
        "known_addon",
        Some("addon_id"),
        "known_addon_ids",
        "known_addon_update_ids",
    ),
    (
        "unknown_addon",
        None,
        "unknown_addon_ids",
        "unknown_addon_update_ids",
    ),
    (
        "material",
        Some("name"),
        "material_names",
        "material_update_names",
    ),
    ("font", None, "font_names", "font_update_names"),
    (
        "known_wa",
        Some("uid"),
        "known_wa_ids",
        "known_wa_update_ids",
    ),
    (
        "unknown_wa",
        Some("uid"),
        "unknown_wa_ids",
        "unknown_wa_update_ids",
    ),
];
fn rows<'a>(v: &'a Value, key: &str) -> Result<&'a [Value]> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        _ => Err(data("DD backup selection group is not an array")),
    }
}
fn key(item: &Value, name: Option<&str>) -> Result<String> {
    let v = if let Some(k) = name {
        item.get(k).unwrap_or(&Value::Null)
    } else if item.is_object() {
        forms::first(item, &["name", "id"]).unwrap_or(&Value::Null)
    } else {
        item
    };
    forms::scalar(v)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| data("DD backup item omitted its exact content key"))
}
fn group<'a>(value: &'a Value, name: &str) -> Result<&'a [Value]> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(&[]),
        Some(v) if v.is_object() => rows(v, "items"),
        _ => Err(data("DD backup content group is not an object")),
    }
}
fn selected(
    doc: &Value,
    name: &str,
    current: &[Value],
    key_name: Option<&str>,
) -> Result<Vec<String>> {
    if let Some(a) = doc.get(name) {
        a.as_array()
            .unwrap()
            .iter()
            .map(|v| {
                forms::scalar(v)
                    .ok_or_else(|| invalid(&format!("$.{name}"), "invalid scalar selection"))
            })
            .collect()
    } else {
        current.iter().map(|v| key(v, key_name)).collect()
    }
}
fn old_version(current: &Value, group: &str, key: &str) -> Result<u64> {
    let Some(v) = current
        .get(group)
        .and_then(|v| v.get("inner_version"))
        .filter(|v| !v.is_null())
    else {
        return Ok(0);
    };
    let object = v
        .as_object()
        .ok_or_else(|| data("DD current inner_version is not an object"))?;
    match object.get(key) {
        None | Some(Value::Null) => Ok(0),
        Some(v) => v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| data("DD current inner_version is not a nonnegative integer")),
    }
}
fn build_group(
    backup: &Value,
    current: &Value,
    doc: &Value,
    name: &str,
    key_name: Option<&str>,
    selected_name: &str,
    update_name: &str,
    wa_account: &str,
    clear: bool,
) -> Result<Value> {
    let available = group(backup, name)?;
    let chosen = if clear {
        Vec::new()
    } else {
        selected(doc, selected_name, group(current, name)?, key_name)?
    };
    let updates = if clear {
        Vec::new()
    } else {
        doc.get(update_name)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let updates = updates
        .iter()
        .map(|v| {
            forms::scalar(v)
                .ok_or_else(|| invalid(&format!("$.{update_name}"), "invalid update marker"))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if updates.iter().any(|v| !chosen.contains(v)) {
        return Err(invalid(
            &format!("$.{update_name}"),
            "update markers must refer to selected content",
        ));
    }
    let mut items = BTreeMap::new();
    let mut versions = Map::new();
    for item in available {
        let k = key(item, key_name)?;
        if items.insert(k.clone(), item).is_some() {
            return Err(data("DD chosen backup repeats a content identity"));
        }
        let old = old_version(current, name, &k)?;
        let next = if updates.contains(&k) && old != 0 {
            old.checked_add(1)
                .ok_or_else(|| data("DD content version exceeds its integer range"))?
        } else {
            old.max(1)
        };
        versions.insert(k, json!(next));
    }
    let mut result = Vec::new();
    let wa = name == "known_wa" || name == "unknown_wa";
    let mapping = if wa {
        rows(&backup["extra"]["wa_account_info"], wa_account)?
    } else {
        &[]
    };
    if wa && !chosen.is_empty() && wa_account.is_empty() {
        return Err(invalid(
            "$.wtf_role_ids",
            "select one WTF account before choosing WA content",
        ));
    }
    for k in chosen {
        let mut item = (**items.get(&k).ok_or_else(|| {
            invalid(
                &format!("$.{selected_name}"),
                "selection is absent from the exact chosen backup",
            )
        })?)
        .clone();
        if wa {
            let matches = mapping
                .iter()
                .filter(|row| row.get("uid").and_then(forms::scalar).as_deref() == Some(&k))
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(invalid(
                    &format!("$.{selected_name}"),
                    "WA selection is unavailable or ambiguous for the selected WTF account",
                ));
            }
            if name == "unknown_wa" {
                item["id"] = matches[0].get("id").cloned().unwrap_or(Value::Null);
            }
        }
        result.push(item);
    }
    Ok(json!({"items":result,"inner_version":versions}))
}
struct Role<'a> {
    account: &'a str,
    server: &'a str,
    index: usize,
    id: String,
    selector: String,
}
fn roles(backup: &Value) -> Result<Vec<Role<'_>>> {
    let sn = backup
        .get("sn")
        .or_else(|| backup.get("backup_sn"))
        .and_then(forms::scalar)
        .ok_or_else(|| data("DD backup has no original SN"))?;
    let mut result = Vec::new();
    let accounts = read::selected_backup_accounts(backup)?;
    for account in accounts {
        let a = account["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| data("DD backup account name is missing"))?;
        for server in rows(account, "servers")? {
            let s = server["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| data("DD backup realm name is missing"))?;
            for (index, role) in rows(server, "items")?.iter().enumerate() {
                let id = read::selected_role_identity(role)?;
                let selector = read::selection_key(
                    "wtf_",
                    &[
                        sn.clone(),
                        a.into(),
                        s.into(),
                        index.to_string(),
                        id.clone(),
                    ],
                );
                result.push(Role {
                    account: a,
                    server: s,
                    index,
                    id,
                    selector,
                });
            }
        }
    }
    Ok(result)
}
fn wtf(backup: &Value, current: &Value, doc: &Value) -> Result<Value> {
    let available = roles(backup)?;
    let mut wanted = BTreeSet::new();
    if let Some(selected) = doc.get("wtf_role_ids").and_then(Value::as_array) {
        for s in selected {
            let s = s.as_str().unwrap();
            let exact = available
                .iter()
                .filter(|r| r.selector == s)
                .collect::<Vec<_>>();
            let matches = if exact.is_empty() {
                available.iter().filter(|r| r.id == s).collect::<Vec<_>>()
            } else {
                exact
            };
            if matches.len() != 1 {
                return Err(invalid(
                    "$.wtf_role_ids",
                    "role selector is absent or ambiguous; use the selector from this exact backup",
                ));
            }
            let r = matches[0];
            wanted.insert((r.account, r.server, r.index));
        }
    } else {
        for account in rows(&current["wtf"], "accounts")? {
            for server in rows(account, "servers")? {
                for role in rows(server, "items")? {
                    let id = read::selected_role_identity(role)?;
                    let matches = available
                        .iter()
                        .filter(|r| {
                            Some(r.account) == account["name"].as_str()
                                && Some(r.server) == server["name"].as_str()
                                && r.id == id
                        })
                        .collect::<Vec<_>>();
                    if matches.len() != 1 {
                        return Err(invalid(
                            "$.wtf_role_ids",
                            "current WTF role is absent or ambiguous in the chosen backup",
                        ));
                    }
                    let r = matches[0];
                    wanted.insert((r.account, r.server, r.index));
                }
            }
        }
    }
    let source = read::selected_backup_accounts(backup)?;
    let mut accounts = Vec::new();
    for a in source {
        let mut account = a.clone();
        let mut servers = Vec::new();
        for s in rows(a, "servers")? {
            let mut server = s.clone();
            let mut chosen = Vec::new();
            for (index, role) in rows(s, "items")?.iter().enumerate() {
                if wanted.contains(&(
                    a["name"].as_str().unwrap(),
                    s["name"].as_str().unwrap(),
                    index,
                )) {
                    chosen.push(role.clone());
                }
            }
            if !chosen.is_empty() {
                server["items"] = json!(chosen);
                servers.push(server);
            }
        }
        if !servers.is_empty() {
            account["servers"] = json!(servers);
            accounts.push(account);
        }
    }
    Ok(json!({"accounts":accounts}))
}
fn account(value: &Value) -> Result<String> {
    let a = rows(&value["wtf"], "accounts")?;
    Ok(if a.len() == 1 {
        a[0]["name"].as_str().unwrap_or("").into()
    } else {
        String::new()
    })
}
pub(super) fn groups(backup: &Value, current: &Value, doc: &Value, body: &mut Value) -> Result<()> {
    body["wtf"] = wtf(backup, current, doc)?;
    let selected_account = account(body)?;
    let changed = doc.get("wtf_role_ids").is_some() && selected_account != account(current)?;
    for &(name, key, selection, updates) in GROUPS {
        let clear =
            (name == "known_wa" || name == "unknown_wa") && changed && doc.get(selection).is_none();
        body[name] = build_group(
            backup,
            current,
            doc,
            name,
            key,
            selection,
            updates,
            &selected_account,
            clear,
        )?;
    }
    let has_roles = !rows(&body["wtf"], "accounts")?.is_empty();
    if (!group(body, "known_addon")?.is_empty() || !group(body, "unknown_addon")?.is_empty())
        && !has_roles
    {
        return Err(invalid(
            "$.wtf_role_ids",
            "addon content requires at least one selected WTF role",
        ));
    }
    if !has_roles
        && ["known_addon", "unknown_addon", "material", "font"]
            .iter()
            .all(|k| group(body, k).is_ok_and(|r| r.is_empty()))
    {
        return Err(invalid(
            "$.known_addon_ids",
            "configuration content cannot contain only WA selections",
        ));
    }
    Ok(())
}
pub(super) fn retail(backup: &Value, current: &Value, selection: &Value) -> Result<Value> {
    if selection.is_null() {
        return Ok(Value::Null);
    }
    if forms::game(backup)?.get() != 10001 {
        return Err(invalid(
            "$.retail_ui_config",
            "retail UI selections require a retail backup",
        ));
    }
    let sn = backup
        .get("sn")
        .or_else(|| backup.get("backup_sn"))
        .and_then(forms::scalar)
        .unwrap();
    let mut wire = current
        .get("retail_ui_config")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or(json!({}));
    for (field, section, prefix, output) in [
        ("edit_mode_selectors", "editMode", "em_", "edit_mode"),
        ("cool_down_selectors", "coolDown", "cd_", "cool_down"),
    ] {
        let Some(selected) = selection.get(field).and_then(Value::as_array) else {
            continue;
        };
        let mut map = BTreeMap::new();
        if let Some(accounts) = backup["retail_ui_config"][section].as_object() {
            for (a, entries) in accounts {
                let entries = entries
                    .as_array()
                    .ok_or_else(|| data("DD retail account entries are not an array"))?;
                for (index, item) in entries.iter().enumerate() {
                    if !item.is_object() {
                        return Err(data("DD retail entry is not an object"));
                    }
                    let key = read::selection_key(
                        prefix,
                        &[sn.clone(), section.into(), a.clone(), index.to_string()],
                    );
                    map.insert(key, (a, item));
                }
            }
        }
        let mut grouped = Map::new();
        let mut specs = BTreeSet::new();
        for s in selected {
            let s = s.as_str().unwrap();
            let (a, item) = map.get(s).ok_or_else(|| {
                invalid(
                    &format!("$.retail_ui_config.{field}"),
                    "selector is unavailable in the exact chosen backup",
                )
            })?;
            let mut item = (*item).clone();
            if section == "editMode" {
                item["is_default"] =
                    json!(selection["default_edit_mode_selector"].as_str() == Some(s));
            } else if !specs.insert(
                item.get("spec_tag")
                    .cloned()
                    .unwrap_or(Value::Null)
                    .to_string(),
            ) {
                return Err(invalid(
                    "$.retail_ui_config.cool_down_selectors",
                    "only one cooldown configuration may be selected per spec_tag",
                ));
            }
            grouped
                .entry((*a).clone())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap()
                .push(item);
        }
        wire[output] = json!(grouped);
    }
    if let Some(v) = selection.get("enable_dd_setup_wizard") {
        wire["enable_dd_setup_wizard"] = v.clone();
    }
    Ok(wire)
}
