use super::{
    protocol::{Api, data, identity},
    *,
};
use crate::input::invalid;
use std::collections::BTreeSet;

pub(super) const FIELDS: &[(&str, &str)] = &[
    ("share_id", "id"),
    ("import_id", "id"),
    ("addons_id", "addonsId"),
    ("backup_id", "backupId"),
    ("account_name", "accountName"),
    ("content", "content"),
    ("content_text", "contentText"),
    ("image_url", "imageUrl"),
    ("is_paid", "isPaid"),
    ("is_public", "isPublic"),
    ("price", "price"),
    ("share_type", "shareType"),
    ("tags", "tags"),
    ("title", "title"),
    ("exclude_wtf", "excludeWtf"),
    ("role_name", "roleName"),
    ("required_tier_id", "requiredTierId"),
    ("sub_type", "subType"),
    ("platform", "platform"),
    ("synchronization_type", "synchronizationType"),
    ("code_text", "codeText"),
    ("file_path", "filePath"),
    ("support_addon", "supportAddon"),
    ("version", "version"),
];
fn csv<'a>(value: &'a Value, path: &str, max: usize) -> Result<Vec<&'a str>> {
    let items = value
        .as_str()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>();
    if items.iter().any(|s| s.is_empty()) || items.len() > max {
        return Err(invalid(
            path,
            &format!("supply 1..{max} nonempty comma-delimited values"),
        ));
    }
    Ok(items)
}
pub(super) fn validate(doc: &Value) -> Result<()> {
    for key in ["content", "code_text", "version", "support_addon"] {
        if doc
            .get(key)
            .is_some_and(|v| v.as_str().is_some_and(|s| s.trim().is_empty()))
        {
            return Err(invalid(&format!("$.{key}"), "value must not be blank"));
        }
    }
    if doc
        .get("title")
        .is_some_and(|v| v.as_str().unwrap().trim().chars().count() < 6)
    {
        return Err(invalid(
            "$.title",
            "title must contain at least 6 characters",
        ));
    }
    if doc
        .get("content_text")
        .is_some_and(|v| v.as_str().unwrap().trim().chars().count() <= 20)
    {
        return Err(invalid(
            "$.content_text",
            "editor plain text must contain more than 20 characters",
        ));
    }
    if let Some(tags) = doc.get("tags") {
        let tags = csv(tags, "$.tags", 3)?;
        if tags.iter().collect::<BTreeSet<_>>().len() != tags.len() {
            return Err(invalid("$.tags", "duplicate tag IDs"));
        }
    }
    if let Some(images) = doc.get("image_url") {
        csv(images, "$.image_url", 10)?;
    }
    for key in ["is_paid", "price", "share_type"] {
        if doc.get(key).is_some_and(|v| v.as_f64() != Some(0.0)) {
            return Err(invalid(
                &format!("$.{key}"),
                "the current ModUs main-client form requires 0",
            ));
        }
    }
    let platform = doc.get("platform");
    let sync = doc.get("synchronization_type");
    if platform.is_some() != sync.is_some() || platform != sync {
        return Err(invalid(
            "$.synchronization_type",
            "platform and synchronization_type must be supplied together with the same 1 (ModUs) or 3 (ModUs + BigFoot)",
        ));
    }
    if platform == Some(&json!(3)) && doc.get("required_tier_id").is_some_and(|v| !v.is_null()) {
        return Err(invalid(
            "$.required_tier_id",
            "BigFoot synchronization requires no tier",
        ));
    }
    if doc["exclude_wtf"] == 1
        && ["account_name", "role_name"].iter().any(|k| {
            doc.get(*k)
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
        })
    {
        return Err(invalid(
            "$.account_name",
            "exclude_wtf=1 requires empty account_name and role_name",
        ));
    }
    Ok(())
}
pub(super) fn form(doc: &Value, config: bool, current: Option<&Value>) -> Result<Value> {
    let create = current.is_none();
    let mut form = json!({});
    if let Some(current) = current {
        let current = normalize(current)?;
        for (_, wire) in FIELDS {
            if let Some(v) = current.get(*wire).filter(|v| !v.is_null()) {
                form[*wire] = v.clone();
            }
        }
        if config
            && doc
                .get("backup_id")
                .is_some_and(|v| identity(v) != identity(&form["backupId"]))
            && ["addons_id", "exclude_wtf", "account_name", "role_name"]
                .iter()
                .any(|k| doc.get(*k).is_none())
        {
            return Err(invalid(
                "$.backup_id",
                "changing backup_id requires explicit addons_id, exclude_wtf, account_name and role_name",
            ));
        }
    } else {
        form = json!({"isPaid":0,"isPublic":1,"price":0,"shareType":0,"subType":0,"platform":1,"synchronizationType":1});
        if config {
            form["accountName"] = json!("");
            form["roleName"] = json!("");
        } else {
            form["filePath"] = json!("");
        }
    }
    for (local, wire) in FIELDS {
        if let Some(v) = doc.get(*local) {
            if *wire == "requiredTierId" && v.is_null() {
                form.as_object_mut().unwrap().remove(*wire);
            } else {
                form[*wire] = v.clone();
            }
        }
    }
    if doc.get("content_text").is_none()
        && (doc.get("content").is_some() || form.get("contentText").is_none())
    {
        form["contentText"] =
            json!(plaintext::text(form["content"].as_str().ok_or_else(
                || malformed("current main-client detail omitted content")
            )?));
    }
    if config && form["excludeWtf"] == 1 {
        form["accountName"] = json!("");
        form["roleName"] = json!("");
    }
    let mut required = vec![
        "addonsId",
        "content",
        "contentText",
        "imageUrl",
        "isPaid",
        "isPublic",
        "price",
        "shareType",
        "tags",
        "title",
    ];
    required.extend(if config {
        vec!["backupId", "excludeWtf"]
    } else {
        vec!["filePath", "supportAddon", "version", "codeText"]
    });
    if required.iter().any(|k| form.get(*k).is_none()) {
        return Err(malformed(
            "current main-client detail lacks fields needed to preserve omitted values; inspect config/wa get before editing",
        ));
    }
    if form["requiredTierId"] != Value::Null
        && (form["platform"] == 3 || form["synchronizationType"] == 3)
    {
        return Err(invalid(
            "$.required_tier_id",
            "clear the current tier explicitly before BigFoot synchronization",
        ));
    }
    // Validate the effective body, including reconstructed plaintext, without
    // requiring an omitted synchronizationType on older stored records.
    let shape = crate::contract::input_shape(
        crate::provider::Provider::Modus,
        if config { "config" } else { "wa" },
        if create { "create" } else { "edit" },
    )?;
    let mut effective = json!({"schema":shape.name});
    for (local, wire) in FIELDS {
        if shape.fields.contains_key(*local) {
            if let Some(v) = form.get(*wire) {
                effective[*local] = v.clone();
            }
        }
    }
    shape.admit(effective.as_object().unwrap())?;
    if !create && doc.get("platform").is_none() {
        effective.as_object_mut().unwrap().remove("platform");
        effective
            .as_object_mut()
            .unwrap()
            .remove("synchronization_type");
    }
    validate(&effective)?;
    if config
        && form["excludeWtf"] == 0
        && form["accountName"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
    {
        return Err(invalid(
            "$.account_name",
            "exclude_wtf=0 requires a selected backup account",
        ));
    }
    Ok(form)
}
pub(super) fn normalize(value: &Value) -> Result<Value> {
    let mut out = value.clone();
    if !out.is_object() {
        return Err(malformed("ModUs main-client detail must be an object"));
    }
    for key in [
        "backupId",
        "isPaid",
        "isPublic",
        "shareType",
        "excludeWtf",
        "requiredTierId",
        "subType",
        "platform",
        "synchronizationType",
    ] {
        if let Some(n) = out[key].as_str().and_then(|s| s.parse::<u64>().ok()) {
            out[key] = json!(n);
        }
    }
    if out.get("requiredTierId").is_none() {
        out["requiredTierId"] = Value::Null;
    }
    if out.get("contentText").is_none() {
        if let Some(content) = out["content"].as_str() {
            out["contentText"] = json!(plaintext::text(content));
        }
    }
    if out["addonsId"].is_number() {
        out["addonsId"] = json!(out["addonsId"].to_string());
    }
    if out["price"].as_f64() == Some(0.0) {
        out["price"] = json!(0);
    }
    if let Some(id) = identity(&out["id"]) {
        out["id"] = json!(id);
    }
    Ok(out)
}
pub(super) fn options(
    api: &Api,
    doc: &Value,
    form: &Value,
    config: bool,
    build: u8,
    account: &str,
) -> Result<()> {
    if let Some(tags) = doc.get("tags") {
        let rows = read::rows(&api.static_options(if config {
            "share_tags.json"
        } else {
            "imports_tags.json"
        })?)?;
        for tag in csv(tags, "$.tags", 3)? {
            if !rows
                .iter()
                .any(|r| identity(&r["id"]).as_deref() == Some(tag))
            {
                return Err(invalid(
                    "$.tags",
                    "tag is absent from current config-tags/wa-tags options",
                ));
            }
        }
    }
    if ["required_tier_id", "sub_type"]
        .iter()
        .any(|k| doc.get(*k).is_some())
        && !form["requiredTierId"].is_null()
    {
        let selected = identity(&form["requiredTierId"]);
        let allowed = if form["subType"] == 1 {
            let code = format!("wow_{}", auth::BUILD_CODES[usize::from(build)]);
            let games = read::rows(&api.get("user/game-type/list", &[], None)?)?;
            if !games.iter().any(|r| r["gameCode"] == code) {
                return Err(invalid(
                    "$.sub_type",
                    "this build has no current subscription game type",
                ));
            }
            let season = data(api.get(
                "user/author/season-sub/detail",
                &[("authorId", account.into()), ("gameType", code)],
                None,
            )?);
            identity(&season["id"]) == selected
                && season["isEnabled"] == 1
                && season["platformSeasonStatus"] != "2"
                && season["platformSeasonStatus"] != 2
        } else {
            read::rows(&api.get("user/author/subscription/tiers", &[], None)?)?
                .iter()
                .any(|r| {
                    identity(&r["id"]) == selected
                        && r.get("isEnabled")
                            .or_else(|| r.get("is_enabled"))
                            .is_none_or(|v| v != &json!(0) && v != &json!(false))
                })
        };
        if !allowed {
            return Err(invalid(
                "$.required_tier_id",
                "select a current enabled author tier or unfinished season subscription",
            ));
        }
    }
    if !config && (doc.get("support_addon").is_some() || doc.get("addons_id").is_some()) {
        let rows = read::rows(&api.static_options("imports_support_addons.json")?)?;
        if !rows.iter().any(|r| {
            r["name"] == form["supportAddon"] && identity(&r["id"]) == identity(&form["addonsId"])
        }) {
            return Err(invalid(
                "$.support_addon",
                "support_addon and addons_id must identify the same wa-support-addons option",
            ));
        }
    }
    if config
        && [
            "backup_id",
            "addons_id",
            "account_name",
            "role_name",
            "exclude_wtf",
        ]
        .iter()
        .any(|k| doc.get(*k).is_some())
    {
        let rows = read::backups(api, Some(build))?;
        let backup = rows
            .iter()
            .find(|r| identity(&r["id"]) == identity(&form["backupId"]))
            .ok_or_else(|| invalid("$.backup_id", "backup is absent from the current account"))?;
        let branch = backup["serverType"]
            .as_u64()
            .or_else(|| backup["serverType"].as_str().and_then(|s| s.parse().ok()));
        if branch != Some(u64::from(build)) {
            return Err(invalid(
                "$.backup_id",
                "selected backup belongs to a different build",
            ));
        }
        if let Some(raw) = backup["knownAddons"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
        {
            let known: Value = serde_json::from_str(raw)
                .map_err(|_| malformed("backup knownAddons JSON is invalid"))?;
            if let Some(ids) = known.get("projectids") {
                let ids = ids
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| ids.to_string());
                if form["addonsId"] != ids {
                    return Err(invalid(
                        "$.addons_id",
                        "addons_id must match backup knownAddons.projectids",
                    ));
                }
            }
        }
        if form["excludeWtf"] == 1 {
            return Ok(());
        }
        let accounts = match &backup["wtfAccounts"] {
            Value::String(s) => serde_json::from_str::<Value>(s)
                .map_err(|_| malformed("backup WTF account JSON is invalid"))?,
            other => other.clone(),
        };
        let accounts = accounts
            .as_array()
            .ok_or_else(|| malformed("backup omitted its WTF account choices"))?;
        let selected = form["accountName"].as_str().unwrap_or("");
        let account = accounts
            .iter()
            .find(|r| {
                ["id", "accountId", "name", "characterName"]
                    .iter()
                    .any(|k| {
                        r[*k].as_str() == Some(selected)
                            || identity(&r[*k]).as_deref() == Some(selected)
                    })
            })
            .ok_or_else(|| {
                invalid(
                    "$.account_name",
                    "account is absent from the selected backup",
                )
            })?;
        let roles = account.get("roles").and_then(Value::as_array);
        let role = form["roleName"].as_str().unwrap_or("");
        if !role.is_empty() && !roles.is_some_and(|a| a.contains(&json!(role)))
            || role.is_empty() && roles.is_some_and(|a| !a.is_empty())
        {
            return Err(invalid(
                "$.role_name",
                "select a role present in this backup account",
            ));
        }
    }
    Ok(())
}
