use super::{
    protocol::{Api, Session, identifier},
    *,
};
use std::collections::BTreeSet;

pub enum Query {
    GameTypes,
    Plugins,
    Plugin(u64),
    PluginVersions(u64),
    Configs(String),
    Config(String),
    Backups,
    Backup(String),
    Was(String),
    Wa { branch: String, slug: String },
    WaCategories,
}
impl Query {
    pub(super) fn validate(&self) -> Result<()> {
        match self {
            Self::Plugin(0) | Self::PluginVersions(0) => {
                Err(ToolkitError::arguments("--module-id must be positive"))
            }
            Self::Configs(branch) | Self::Was(branch) | Self::Wa { branch, .. } => {
                if !branch.strip_prefix("wow").is_some_and(|s| {
                    !s.is_empty()
                        && s.bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                }) {
                    return Err(ToolkitError::arguments(
                        "--strappid must be a branch ID returned by publish blackbox options game-types",
                    ));
                }
                if let Self::Wa { slug, .. } = self {
                    reference(slug)?;
                }
                Ok(())
            }
            Self::Config(id) | Self::Backup(id) => reference(id),
            _ => Ok(()),
        }
    }
}
pub(super) fn reference(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        Err(ToolkitError::arguments(
            "resource identity must be an exact nonempty ID from its list command",
        ))
    } else {
        Ok(())
    }
}
fn rows(value: &Value, field: &str) -> Result<Vec<Value>> {
    value
        .get(field)
        .and_then(Value::as_array)
        .filter(|a| a.iter().all(Value::is_object))
        .cloned()
        .ok_or_else(|| protocol_error("Heybox list response is incomplete"))
}
fn list(kind: &str, rows: Vec<Value>) -> Value {
    json!({"platform":"blackbox","resource":kind,"total_count":rows.len(),"complete":true,"coverage":"complete","items":rows})
}
pub(super) fn execute(api: &Api, session: &mut Session, query: Query) -> Result<Value> {
    match query {
        Query::GameTypes => {
            let items = rows(
                &api.data(session, "/wow/v2/dynamic_conf/", &[], false)?,
                "strappidInfos",
            )?;
            if items.is_empty() {
                return Err(protocol_error("Heybox returned no dynamic game branches"));
            }
            Ok(list("game-types", items))
        }
        Query::Plugins => Ok(list(
            "plugin",
            rows(
                &api.data(session, "/wow/open_platform/module/list/", &[], false)?,
                "moduleList",
            )?,
        )),
        Query::Plugin(id) => {
            let module = module(api, session, id)?;
            let (versions, options) = versions(api, session, id)?;
            Ok(
                json!({"platform":"blackbox","module":module,"versions":versions,"branch_options":options,"complete":true}),
            )
        }
        Query::PluginVersions(id) => {
            let (items, options) = versions(api, session, id)?;
            let mut result = list("plugin-version", items);
            result["branch_options"] = options;
            Ok(result)
        }
        Query::Configs(branch) => Ok(list(
            "config",
            collect(api, session, Listing::Configs(branch))?,
        )),
        Query::Config(id) => {
            let detail = api.data(
                session,
                "/wow/v2/share/detail/",
                &[("share_id", id.clone())],
                true,
            )?;
            if identifier(&detail["ShareID"]).as_deref() != Some(&id)
                || identifier(&detail["BackupID"]).is_none()
            {
                return Err(protocol_error(
                    "Heybox config detail is incomplete or identifies another share",
                ));
            }
            Ok(json!({"platform":"blackbox","config":detail}))
        }
        Query::Backups => Ok(list("backup", backups(api, session)?)),
        Query::Backup(id) => {
            let item = backups(api, session)?
                .into_iter()
                .find(|v| identifier(&v["ID"]).as_deref() == Some(&id));
            match item {
                Some(item) => Ok(json!({"platform":"blackbox","backup":item})),
                None => Err(ToolkitError::new(
                    "publish.blackbox_not_found",
                    "backup was not found in the complete current account list",
                    5,
                )),
            }
        }
        Query::Was(branch) => Ok(list("wa", collect(api, session, Listing::Was(branch))?)),
        Query::Wa { branch, slug } => {
            let summary = collect(api, session, Listing::Was(branch))?
                .into_iter()
                .find(|v| v["Slug"] == slug)
                .ok_or_else(|| {
                    ToolkitError::new(
                        "publish.blackbox_not_found",
                        "WA was not found in the current author's branch",
                        5,
                    )
                })?;
            let detail = wa_detail(api, session, &summary)?;
            Ok(json!({"platform":"blackbox","wa":detail}))
        }
        Query::WaCategories => {
            let items = rows(
                &api.data(session, "/wow/get_wa_tag_list/", &[], true)?,
                "list",
            )?;
            if items.iter().any(|r| !r["TagList"].is_array()) {
                return Err(protocol_error("Heybox WA categories are incomplete"));
            }
            Ok(list("wa-categories", items))
        }
    }
}
pub(super) fn module(api: &Api, session: &mut Session, id: u64) -> Result<Value> {
    let detail = api.data(
        session,
        "/wow/open_platform/module/detail/",
        &[("moduleId", id.to_string())],
        false,
    )?;
    let module = detail.get("module").unwrap_or(&detail);
    if !module.is_object()
        || module.as_object().unwrap().is_empty()
        || module
            .get("id")
            .is_some_and(|v| identifier(v).as_deref() != Some(id.to_string().as_str()))
    {
        return Err(protocol_error(
            "Heybox plugin detail is incomplete or identifies another plugin",
        ));
    }
    Ok(module.clone())
}
pub(super) fn wa_detail(api: &Api, session: &mut Session, summary: &Value) -> Result<Value> {
    let slug = summary["Slug"]
        .as_str()
        .ok_or_else(|| protocol_error("WA summary omitted its slug"))?;
    let version = summary
        .pointer("/LatestVersion/Version")
        .and_then(identifier)
        .ok_or_else(|| protocol_error("Heybox WA summary omitted its latest version identity"))?;
    let detail = api.data(
        session,
        "/wow/get_wa_detail/",
        &[("slug", slug.into()), ("version_id", version)],
        true,
    )?;
    if detail["Slug"] != slug
        || identifier(&detail["HeyboxId"]).as_deref() != Some(session.identity()?.as_str())
    {
        return Err(protocol_error(
            "Heybox WA detail changed its author or identity",
        ));
    }
    Ok(detail)
}
pub(super) fn backups(api: &Api, session: &mut Session) -> Result<Vec<Value>> {
    let rows = rows(
        &api.data(
            session,
            "/wow/get_user_configuration_backup_list/",
            &[],
            true,
        )?,
        "backupList",
    )?;
    let mut seen = BTreeSet::new();
    for row in &rows {
        let id = identifier(&row["ID"])
            .ok_or_else(|| protocol_error("Heybox backup list omitted an identity"))?;
        if !seen.insert(id) {
            return Err(protocol_error("Heybox backup list repeated an identity"));
        }
    }
    Ok(rows)
}
pub(super) enum Listing {
    Versions(u64),
    Configs(String),
    Was(String),
}
pub(super) fn collect(api: &Api, session: &mut Session, kind: Listing) -> Result<Vec<Value>> {
    collect_options(api, session, kind).map(|(items, _)| items)
}
pub(super) fn versions(api: &Api, session: &mut Session, id: u64) -> Result<(Vec<Value>, Value)> {
    collect_options(api, session, Listing::Versions(id))
}
fn collect_options(api: &Api, session: &mut Session, kind: Listing) -> Result<(Vec<Value>, Value)> {
    let (path, field, id, limit, post, maximum) = match &kind {
        Listing::Versions(_) => (
            "/wow/open_platform/module_version/list/",
            "versionList",
            "id",
            100,
            false,
            100,
        ),
        Listing::Configs(_) => ("/wow/v2/share/list/", "shares", "ShareID", 20, true, 1000),
        Listing::Was(_) => ("/wow/get_wa_list/", "waList", "Slug", 30, true, 1000),
    };
    let owner = if matches!(kind, Listing::Was(_)) {
        let data = api.data(session, "/wow/v2/get_user_info/", &[], true)?;
        let owner = identifier(&data["heybox_id"])
            .ok_or_else(|| protocol_error("Heybox did not identify the WA author"))?;
        if owner != session.identity()? {
            return Err(protocol_error(
                "Heybox WA author differs from the current account",
            ));
        }
        Some(owner)
    } else {
        None
    };
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    let mut total = None;
    let mut options = Value::Null;
    for _ in 0..maximum {
        let offset = items.len();
        let step = (|| -> Result<bool> {
            let mut query = vec![("offset", offset.to_string()), ("limit", limit.to_string())];
            match &kind {
                Listing::Versions(module) => query.push(("moduleId", module.to_string())),
                Listing::Configs(branch) => query.extend([
                    ("strappid", branch.clone()),
                    ("sort_by", "weekDownloadNum".into()),
                    ("my_publish", "1".into()),
                ]),
                Listing::Was(branch) => query.extend([
                    ("strappid", branch.clone()),
                    ("sortBy", "finalScore".into()),
                    ("my_publish", "1".into()),
                ]),
            }
            let data = api.data(session, path, &query, post)?;
            let page = rows(&data, field)?;
            if matches!(kind, Listing::Versions(_)) {
                if let Some(value) = data.get("strappidInfos") {
                    if !value.is_array() || (!options.is_null() && options != *value) {
                        return Err(protocol_error(
                            "Heybox version branches changed during pagination",
                        ));
                    }
                    options = value.clone();
                }
                for key in ["totalCount", "total_count", "total", "count"] {
                    if let Some(v) = data.get(key) {
                        let n = v
                            .as_u64()
                            .ok_or_else(|| protocol_error("Heybox version total is invalid"))?;
                        if total.is_some_and(|old| old != n) {
                            return Err(protocol_error(
                                "Heybox version total changed during pagination",
                            ));
                        }
                        total = Some(n);
                    }
                }
            }
            if page.is_empty() {
                if total.is_some_and(|n| n != items.len() as u64) {
                    return Err(protocol_error("Heybox pagination ended before its total"));
                }
                return Ok(true);
            }
            for row in page {
                let identity = identifier(&row[id])
                    .ok_or_else(|| protocol_error("Heybox pagination omitted an identity"))?;
                reference(&identity)
                    .map_err(|_| protocol_error("Heybox list identity is invalid"))?;
                if matches!(kind, Listing::Versions(_))
                    && identity.parse::<u64>().ok().is_none_or(|n| n == 0)
                {
                    return Err(protocol_error("Heybox version identity is invalid"));
                }
                if owner
                    .as_ref()
                    .is_some_and(|o| identifier(&row["HeyboxId"]).as_ref() != Some(o))
                {
                    return Err(protocol_error("Heybox WA list contains another author"));
                }
                if !seen.insert(identity) {
                    return Err(protocol_error("Heybox pagination repeated an identity"));
                }
                items.push(row);
            }
            if total.is_some_and(|n| items.len() as u64 > n) {
                return Err(protocol_error(
                    "Heybox version list exceeded its reported total",
                ));
            }
            Ok(total == Some(items.len() as u64))
        })();
        match step {
            Ok(true) => return Ok((items, options)),
            Ok(false) => {}
            Err(error) => return Err(partial(error, items, offset)),
        }
    }
    let offset = items.len();
    Err(partial(
        protocol_error("Heybox pagination exceeded its page limit"),
        items,
        offset,
    ))
}
fn partial(mut error: ToolkitError, items: Vec<Value>, offset: usize) -> ToolkitError {
    error.details = json!({"complete":false,"coverage":"partial","items":crate::output::public(json!(items),&[]),"next_offset":offset,"absence_proven":false});
    error
}
