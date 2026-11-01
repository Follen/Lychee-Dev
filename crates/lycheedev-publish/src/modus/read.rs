use super::{
    protocol::{Api, data, identity},
    *,
};
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
pub enum Listing {
    Projects,
    Releases(u64),
    AddonHistory(u64),
    Configs,
    Was,
}
impl Listing {
    fn route(self) -> &'static str {
        match self {
            Self::Projects => "game/data/author/project/list",
            Self::Releases(_) => "game/data/author/project/file/list",
            Self::AddonHistory(_) => "plugin/project/history",
            Self::Configs => "system/user/share/list",
            Self::Was => "system/user/import/list",
        }
    }
    fn main(self) -> bool {
        matches!(self, Self::AddonHistory(_) | Self::Configs | Self::Was)
    }
    fn id(self, value: &Value) -> Option<String> {
        let names: &[&str] = match self {
            Self::Projects => &["projectId", "id"],
            Self::Releases(_) | Self::AddonHistory(_) => &["fileId", "id"],
            _ => &["id"],
        };
        names.iter().find_map(|n| value.get(*n).and_then(identity))
    }
}
pub enum Query {
    Builds,
    Account,
    SubscriptionCount,
    Statistics,
    Options {
        action: String,
        keys: Vec<String>,
        author: Option<u64>,
        game_type: Option<String>,
    },
    Addons {
        directories: Vec<String>,
        build: Option<u8>,
    },
    AddonProjects {
        projects: Vec<u64>,
        build: Option<u8>,
    },
    List {
        kind: Listing,
        build: Option<u8>,
        page: Option<u64>,
        size: u64,
        filters: Value,
    },
    Project(u64),
    Release {
        project: u64,
        file: u64,
    },
    Dependencies(Value),
    Config {
        id: String,
        build: Option<u8>,
    },
    Wa {
        id: String,
        build: Option<u8>,
    },
    Backups(Option<u8>),
    Backup {
        id: u64,
        build: Option<u8>,
    },
}
impl Query {
    pub(super) fn public(&self) -> bool {
        matches!(self,Self::Options{action,..} if ["config-tags","wa-tags","wa-support-addons"].contains(&action.as_str()))
    }
    pub(super) fn client(&self) -> Client {
        match self {
            Self::Builds
            | Self::Addons { .. }
            | Self::AddonProjects { .. }
            | Self::Config { .. }
            | Self::Wa { .. }
            | Self::Backups(_)
            | Self::Backup { .. } => Client::Main,
            Self::List { kind, .. } if kind.main() => Client::Main,
            Self::Options { action, .. }
                if ["subscription-game-types", "season-subscription"]
                    .contains(&action.as_str()) =>
            {
                Client::Main
            }
            _ => Client::Creator,
        }
    }
    pub(super) fn validate(&self) -> Result<()> {
        let err = || {
            ToolkitError::arguments(
                "invalid ModUs query; inspect this command's schema and current options",
            )
        };
        match self {
            Self::Options {
                action,
                keys,
                author,
                game_type,
            } => {
                if ![
                    "categories",
                    "game-versions",
                    "subscription-tiers",
                    "subscription-game-types",
                    "season-subscription",
                    "config-tags",
                    "wa-tags",
                    "wa-support-addons",
                ]
                .contains(&action.as_str())
                {
                    return Err(err());
                }
                if action == "game-versions"
                    && (keys.is_empty() || keys.iter().any(|s| s.trim().is_empty()))
                {
                    return Err(ToolkitError::arguments(
                        "--keys requires a nonempty JSON array of configuration keys",
                    ));
                }
                if author == &Some(0)
                    || action == "season-subscription"
                        && !game_type.as_ref().is_some_and(|s| !s.trim().is_empty())
                {
                    return Err(err());
                }
            }
            Self::Addons { directories, build } => {
                check_build(*build)?;
                if directories.is_empty()
                    || directories.iter().any(|s| {
                        s.is_empty()
                            || s == "."
                            || s == ".."
                            || s.chars().any(|c| "/\\:".contains(c) || c.is_control())
                    })
                {
                    return Err(ToolkitError::arguments(
                        "--directories requires a nonempty JSON array of addon folder names",
                    ));
                }
            }
            Self::AddonProjects { projects, build } => {
                check_build(*build)?;
                if projects.is_empty() || projects.contains(&0) {
                    return Err(err());
                }
            }
            Self::List {
                kind,
                build,
                page,
                size,
                filters,
            } => {
                check_build(*build)?;
                if *page == Some(0)
                    || *size == 0
                    || *size > 100
                    || !filters.is_object()
                    || matches!(kind, Listing::Releases(0) | Listing::AddonHistory(0))
                {
                    return Err(err());
                }
                for (key, value) in filters.as_object().unwrap() {
                    let allowed = match kind {
                        Listing::Projects => &["keyword"][..],
                        Listing::Configs => &[
                            "keyword",
                            "status",
                            "shareType",
                            "orderBy",
                            "mine",
                            "isPublic",
                            "isPaid",
                            "tags",
                        ][..],
                        Listing::Was => &[
                            "keyword",
                            "status",
                            "orderBy",
                            "mine",
                            "supportAddon",
                            "tags",
                            "isPaid",
                        ][..],
                        _ => &[],
                    };
                    if !allowed.contains(&key.as_str()) {
                        return Err(err());
                    }
                    let valid = match key.as_str() {
                        "mine" => value.is_boolean(),
                        "tags" => value
                            .as_array()
                            .is_some_and(|a| a.iter().all(Value::is_string)),
                        "status" | "shareType" => value.as_u64().is_some(),
                        "isPublic" | "isPaid" => matches!(value.as_u64(), Some(0 | 1)),
                        _ => value.is_string(),
                    };
                    if !valid {
                        return Err(err());
                    }
                }
            }
            Self::Project(0)
            | Self::Release { project: 0, .. }
            | Self::Release { file: 0, .. }
            | Self::Backup { id: 0, .. } => return Err(err()),
            Self::Config { id, build } | Self::Wa { id, build } => {
                check_build(*build)?;
                if identity(&json!(id)).is_none() {
                    return Err(err());
                }
            }
            Self::Backups(build) | Self::Backup { build, .. } => check_build(*build)?,
            Self::Dependencies(value) => {
                dependency(value)?;
            }
            _ => {}
        }
        Ok(())
    }
}
fn check_build(build: Option<u8>) -> Result<()> {
    if build.is_some_and(|b| b > 4) {
        Err(ToolkitError::arguments(
            "--server-type must be 0..4; use publish modus builds",
        ))
    } else {
        Ok(())
    }
}
pub(super) fn dependency(value: &Value) -> Result<Value> {
    if let Some(s) = value.as_str().filter(|s| !s.trim().is_empty()) {
        return Ok(json!({"name":s.trim()}));
    }
    let list = if let Some(object) = value.as_object() {
        if object.len() != 1 {
            return Err(ToolkitError::arguments(
                "--query accepts a name or one projectIds array",
            ));
        }
        if let Some(name) = object.get("name") {
            if name.is_string() {
                return dependency(name);
            }
        }
        object
            .get("projectIds")
            .or_else(|| object.get("project_ids"))
    } else {
        Some(value)
    };
    let ids = list
        .and_then(Value::as_array)
        .filter(|a| {
            !a.is_empty()
                && a.iter()
                    .all(|v| v.as_u64().is_some_and(|n| n > 0 && n <= i32::MAX as u64))
        })
        .ok_or_else(|| {
            ToolkitError::arguments(
                "--query project IDs must be a nonempty JSON array of integers 1..2147483647",
            )
        })?;
    Ok(json!({"projectIds":ids}))
}
pub(super) fn rows(value: &Value) -> Result<Vec<Value>> {
    let mut value = value;
    for _ in 0..5 {
        if let Some(a) = value.as_array().filter(|a| a.iter().all(Value::is_object)) {
            return Ok(a.clone());
        }
        if value.is_object() {
            if let Some(next) = ["rows", "list", "items", "records", "data"]
                .iter()
                .find_map(|k| value.get(*k))
            {
                value = next;
                continue;
            }
        }
        break;
    }
    Err(malformed("ModUs response omitted a recognized collection"))
}
fn total(value: &Value) -> Result<Option<u64>> {
    let mut value = value;
    for _ in 0..5 {
        if let Some(n) = value.get("total").or_else(|| value.get("totalCount")) {
            return n
                .as_u64()
                .map(Some)
                .ok_or_else(|| malformed("ModUs returned an invalid list total"));
        }
        let Some(next) = value.get("data") else {
            break;
        };
        value = next;
    }
    Ok(None)
}
fn partial(error: ToolkitError, items: &[Value], page: u64) -> ToolkitError {
    ToolkitError::new("publish.modus_partial","ModUs list is incomplete; retry the original query before deciding absence or deleting",error.exit_code).with_details(json!({"coverage":"partial","complete":false,"absence_proven":false,"next_page":page,"items":crate::output::public(json!(items),&[]),"cause":{"code":error.code,"details":error.details}}))
}
pub(super) fn pages(
    api: &Api,
    kind: Listing,
    build: Option<u8>,
    page: Option<u64>,
    size: u64,
    filters: &Value,
) -> Result<Value> {
    let branch = if kind.main() {
        Some(api.build(build)?)
    } else {
        None
    };
    let mut body = filters.clone();
    body["pageSize"] = json!(size);
    match kind {
        Listing::Projects => {}
        Listing::Releases(id) => body["projectId"] = json!(id),
        Listing::AddonHistory(id) => body["projectIds"] = json!([id]),
        Listing::Configs | Listing::Was => {
            let obj = body.as_object_mut().unwrap();
            obj.insert("server".into(), json!(branch));
            obj.insert("platform".into(), json!(0));
            obj.entry("mine").or_insert(json!(false));
            if matches!(kind, Listing::Configs) {
                obj.entry("shareType").or_insert(json!(0));
            } else {
                obj.entry("status").or_insert(json!(1));
            }
        }
    }
    let mut next = page.unwrap_or(1);
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    let mut expected = None;
    loop {
        let result = (|| {
            if next > 10000 {
                return Err(malformed("ModUs pagination exceeded 10000 pages"));
            }
            body["pageNum"] = json!(next);
            let payload = api.post(kind.route(), &body, branch, false)?;
            let count = total(&payload)?;
            if let (Some(old), Some(new)) = (expected, count) {
                if old != new {
                    return Err(malformed("ModUs list total changed during pagination"));
                }
            }
            if expected.is_some() && count.is_none() {
                return Err(malformed("ModUs list lost its total during pagination"));
            }
            expected = count;
            let batch = rows(&payload)?;
            let empty = batch.is_empty();
            for item in batch {
                let id = kind
                    .id(&item)
                    .ok_or_else(|| malformed("ModUs list omitted a resource identity"))?;
                if !seen.insert(id) {
                    return Err(malformed("ModUs list repeated a resource identity"));
                }
                if let Listing::Releases(project) = kind {
                    if item
                        .get("projectId")
                        .is_some_and(|p| identity(p) != Some(project.to_string()))
                    {
                        return Err(malformed("ModUs release list crossed its parent project"));
                    }
                }
                items.push(item);
            }
            if let Some(total) = expected {
                if page.is_none()
                    && (items.len() as u64 > total || empty && (items.len() as u64) < total)
                {
                    return Err(malformed("ModUs list does not match its reported total"));
                }
            }
            Ok(empty || expected == Some(items.len() as u64))
        })();
        let done = result.map_err(|e| partial(e, &items, next))?;
        if page.is_some() || done {
            return Ok(
                json!({"platform":"modus","items":items,"total":expected,"complete":page.is_none(),"coverage":if page.is_some(){"page"}else{"complete"},"page":page,"server_type":branch}),
            );
        }
        next += 1;
    }
}
pub(super) fn backups(api: &Api, build: Option<u8>) -> Result<Vec<Value>> {
    let branch = api.build(build)?;
    let items = rows(&api.get("system/user/backup/list", &[], Some(branch))?)?;
    let mut seen = BTreeSet::new();
    for item in &items {
        let id = identity(
            item.get("id")
                .or_else(|| item.get("backupId"))
                .unwrap_or(&Value::Null),
        )
        .ok_or_else(|| malformed("ModUs backup omitted its identity"))?;
        if !seen.insert(id) {
            return Err(malformed("ModUs backups repeated an identity"));
        }
    }
    Ok(items)
}
pub(super) fn detail(api: &Api, kind: &str, id: &str, build: Option<u8>) -> Result<Value> {
    optional_detail(api, kind, id, build)?
        .ok_or_else(|| malformed("ModUs detail did not return the requested resource"))
}
pub(super) fn optional_detail(
    api: &Api,
    kind: &str,
    id: &str,
    build: Option<u8>,
) -> Result<Option<Value>> {
    let branch = Some(api.build(build)?);
    let payload = if kind == "backup" {
        let Some(value) = api.backup_detail(id, branch.unwrap())? else {
            return Ok(None);
        };
        value
    } else {
        let key = if kind == "share" {
            "shareIds"
        } else {
            "importIds"
        };
        api.post(
            &format!("system/user/{kind}/detail"),
            &json!({key:[id]}),
            branch,
            false,
        )?
    };
    let mut item = data(payload);
    if item.is_array() || item.get("rows").is_some() {
        let mut items = rows(&item)?;
        if items.is_empty() {
            return Ok(None);
        }
        if items.len() != 1 {
            return Err(malformed(
                "ModUs detail did not return exactly one requested resource",
            ));
        }
        item = items.remove(0);
    }
    if identity(
        item.get("id")
            .or_else(|| item.get("backupId"))
            .unwrap_or(&Value::Null),
    )
    .as_deref()
        != Some(id)
    {
        return Err(malformed(
            "ModUs detail does not identify the requested resource",
        ));
    }
    Ok(Some(item))
}
pub(super) fn project(api: &Api, id: u64) -> Result<Value> {
    let item = data(api.get(&format!("game/data/author/project/detail/{id}"), &[], None)?);
    if identity(
        item.get("projectId")
            .or_else(|| item.get("id"))
            .unwrap_or(&Value::Null),
    ) != Some(id.to_string())
    {
        return Err(malformed("ModUs project detail changed its identity"));
    }
    Ok(item)
}
pub(super) fn execute(api: &Api, query: Query) -> Result<Value> {
    query.validate()?;
    let result = match query {
        Query::Builds => return Err(malformed("ModUs builds must use local discovery")),
        Query::Account => data(api.get("system/user/getInfo", &[], None)?),
        Query::SubscriptionCount => {
            data(api.get("user/author/subscription/active/count", &[], None)?)
        }
        Query::Statistics => data(api.get("game/data/author/project/statistics", &[], None)?),
        Query::Options {
            action,
            keys,
            author,
            game_type,
        } => match action.as_str() {
            "config-tags" => api.static_options("share_tags.json")?,
            "wa-tags" => api.static_options("imports_tags.json")?,
            "wa-support-addons" => api.static_options("imports_support_addons.json")?,
            "categories" => data(api.get("plugin/list/Categories", &[], None)?),
            "subscription-tiers" => data(api.get("user/author/subscription/tiers", &[], None)?),
            "subscription-game-types" => data(api.get("user/game-type/list", &[], None)?),
            "game-versions" => data(api.post(
                "game/data/config/detail",
                &json!({"keys":keys}),
                None,
                false,
            )?),
            "season-subscription" => {
                let author = match author {
                    Some(id) => id.to_string(),
                    None => api.account()?,
                };
                // This route deliberately has no X-Server-Type header.
                api.get(
                    "user/author/season-sub/detail",
                    &[("authorId", author), ("gameType", game_type.unwrap())],
                    None,
                )?
                .get("data")
                .cloned()
                .unwrap_or(Value::Null)
            }
            _ => unreachable!(),
        },
        Query::Addons { directories, build } => data(api.post(
            "plugin/list/info",
            &json!({"pluginList":directories}),
            Some(api.build(build)?),
            false,
        )?),
        Query::AddonProjects { projects, build } => data(api.post(
            "plugin/list/detail",
            &json!({"projectIds":projects}),
            Some(api.build(build)?),
            false,
        )?),
        Query::List {
            kind,
            build,
            page,
            size,
            filters,
        } => return pages(api, kind, build, page, size, &filters),
        Query::Project(id) => project(api, id)?,
        Query::Release { project, file } => {
            let item = data(api.get(
                &format!("game/data/author/project/file/detail/{file}"),
                &[],
                None,
            )?);
            if identity(
                item.get("fileId")
                    .or_else(|| item.get("id"))
                    .unwrap_or(&Value::Null),
            ) != Some(file.to_string())
            {
                return Err(malformed("ModUs release detail changed its file identity"));
            }
            if let Some(parent) = item.get("projectId") {
                if identity(parent) != Some(project.to_string()) {
                    return Err(malformed("ModUs release belongs to another project"));
                }
            } else {
                let list = pages(api, Listing::Releases(project), None, None, 100, &json!({}))?;
                if !list["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| Listing::Releases(project).id(v) == Some(file.to_string()))
                {
                    return Err(malformed(
                        "ModUs release was not found under its requested project",
                    ));
                }
            }
            item
        }
        Query::Dependencies(value) => data(api.post(
            "game/data/author/project/dependency/query",
            &dependency(&value)?,
            None,
            false,
        )?),
        Query::Config { id, build } => detail(api, "share", &id, build)?,
        Query::Wa { id, build } => detail(api, "import", &id, build)?,
        Query::Backups(build) => {
            return Ok(
                json!({"platform":"modus","items":backups(api,build)?,"complete":true,"coverage":"complete_account_all_builds"}),
            );
        }
        Query::Backup { id, build } => detail(api, "backup", &id.to_string(), build)?,
    };
    Ok(json!({"platform":"modus","data":result}))
}
