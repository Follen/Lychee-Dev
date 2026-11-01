//! DD author reads on an explicitly selected retained task. No acquisition,
//! reauthentication or mutation is available at this business boundary.
use super::task::Client;
use crate::output;
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde_json::{Value, json};
use std::{collections::BTreeSet, num::NonZeroU64};
mod projection;
use projection::*;

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resource {
    Plugin,
    Config,
    Wa,
}
impl Resource {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::Config => "config",
            Self::Wa => "wa",
        }
    }
    fn act(self) -> &'static str {
        match self {
            Self::Plugin => "addon",
            Self::Config => "share",
            Self::Wa => "wa",
        }
    }
}
pub struct Reference(String);
impl Reference {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
    pub fn parse(value: &str) -> Result<Self> {
        if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
            return Err(ToolkitError::arguments(
                "DD share/backup SN must be nonempty text of at most 1024 bytes without control characters",
            ));
        }
        Ok(Self(value.into()))
    }
}
#[derive(Clone, Copy)]
pub struct Page {
    number: u32,
    size: u32,
}
impl Page {
    pub fn new(number: u64, size: u64) -> Result<Self> {
        if number == 0 || number > u32::MAX as u64 || !(1..=100).contains(&size) {
            return Err(ToolkitError::arguments(
                "DD page must be a positive 32-bit integer and size must be 1..100",
            ));
        }
        Ok(Self {
            number: number as u32,
            size: size as u32,
        })
    }
}
pub enum Read {
    GameTypes,
    GameVersions(NonZeroU64),
    PluginCategories,
    WaCategories(NonZeroU64),
    VipLevels,
    AssociatedActs(NonZeroU64),
    List {
        resource: Resource,
        game_type: NonZeroU64,
        keyword: String,
        page: Page,
    },
    Detail {
        resource: Resource,
        reference: Reference,
    },
    PluginVersions {
        reference: Reference,
        game_type: NonZeroU64,
        page: NonZeroU64,
    },
    Backups,
    Backup(Reference),
}
pub fn life_types() -> Value {
    json!({"total":6,"items":[
        {"name":"7 days","value":"seven_day"},{"name":"14 days","value":"fourteen_day"},
        {"name":"30 days","value":"thirty_day"},{"name":"60 days","value":"sixty_day"},
        {"name":"90 days","value":"ninety_day"},{"name":"forever","value":"forever"}],
        "source":"NetEase DD official client enum","remote_read":false})
}
pub(super) trait Api {
    fn get(&mut self, path: &str, query: &[(&str, String)]) -> Result<Value>;
    fn post_read(&mut self, path: &str, body: &Value) -> Result<Value>;
}
impl Api for super::DdTransport {
    fn get(&mut self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        super::DdTransport::get(self, path, query)
    }
    fn post_read(&mut self, path: &str, body: &Value) -> Result<Value> {
        self.post(path, &[], body, false)
    }
}
pub(super) struct Attached<'a> {
    pub(super) client: &'a Client,
    pub(super) control: &'a OperationControl,
}
impl Api for Attached<'_> {
    fn get(&mut self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.client.get(path, query, self.control)
    }
    fn post_read(&mut self, path: &str, body: &Value) -> Result<Value> {
        self.client.post(path, &[], body, false, self.control)
    }
}
pub fn read(client: &Client, request: &Read, control: &OperationControl) -> Result<Value> {
    request.execute(&mut Attached { client, control }, control)
}
fn data(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_data", message, 5)
}
fn result(payload: &Value) -> Result<&Value> {
    payload
        .get("result")
        .ok_or_else(|| data("DD response omitted its documented result"))
}
fn rows<'a>(payload: &'a Value, names: &[&str]) -> Result<&'a [Value]> {
    let value = result(payload)?;
    if let Some(values) = value.as_array() {
        return Ok(values);
    }
    for name in names {
        if let Some(value) = value.get(*name) {
            return value
                .as_array()
                .map(Vec::as_slice)
                .ok_or_else(|| data("DD result list has an invalid shape"));
        }
    }
    Err(data(
        "DD response has no recognized result list; absence is unverified",
    ))
}
pub(super) fn items(payload: &Value) -> Result<&[Value]> {
    rows(
        payload,
        &["data_list", "list", "rows", "items", "wa_list", "shares"],
    )
}
fn options(payload: &Value) -> Result<&[Value]> {
    rows(
        payload,
        &[
            "data_list",
            "list",
            "rows",
            "items",
            "categories",
            "versions",
        ],
    )
}
pub(super) fn total(payload: &Value) -> Result<Option<u64>> {
    for value in [payload, result(payload)?] {
        for name in ["total", "total_count", "count"] {
            if let Some(value) = value.get(name).filter(|v| !v.is_null()) {
                return value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
                    .map(Some)
                    .ok_or_else(|| data("DD list total is not a nonnegative integer"));
            }
        }
    }
    Ok(None)
}
fn author_page(
    api: &mut impl Api,
    resource: Resource,
    game_type: NonZeroU64,
    keyword: &str,
    page: Page,
) -> Result<Value> {
    match resource {
        Resource::Plugin => api.post_read("/addon/addon_list",&json!({"game_type":game_type.get(),"origin":"created","page":page.number,"size":page.size,
            "category":0,"name_or_author_name_or_share_code":keyword,"sort_type":2})),
        Resource::Config | Resource::Wa => {
            let mut query=vec![("game_type",game_type.to_string()),("origin","created".into()),("page",page.number.to_string()),("size",page.size.to_string()),
                ("search_text",keyword.into()),("sort_type","mtime".into())];
            let path=if matches!(resource,Resource::Config) { "/share/list" } else { query.push(("category_id",String::new())); "/wa/list" };
            api.get(path,&query)
        }
    }
}
fn page_view(
    payload: &Value,
    page: Page,
    mut project: impl FnMut(&Value) -> Result<Value>,
) -> Result<Value> {
    let raw = items(payload)?;
    if raw.len() > page.size as usize {
        return Err(data("DD author page exceeds the requested size"));
    }
    let total = total(payload)?;
    let offset = (page.number as u64 - 1) * page.size as u64;
    if total.is_some_and(|n| !raw.is_empty() && offset + raw.len() as u64 > n) {
        return Err(data("DD page contradicts its reported total"));
    }
    let more = total
        .map(|n| offset + (raw.len() as u64) < n)
        .or_else(|| (raw.len() < page.size as usize).then_some(false));
    let complete = page.number == 1 && more == Some(false);
    Ok(
        json!({"items":raw.iter().map(&mut project).collect::<Result<Vec<_>>>()?,"returned":raw.len(),"total":total,
        "page":page.number,"page_size":page.size,"has_more":more,"complete_author_list":complete,"truncated":!complete,
        "next_page":if more!=Some(false) {Some(page.number as u64+1)} else {None}}),
    )
}
pub(super) fn all_author(
    api: &mut impl Api,
    resource: Resource,
    game_type: NonZeroU64,
    control: &OperationControl,
) -> Result<Vec<Value>> {
    author_all(api, resource, game_type, control, false)
}
pub(super) fn all_author_required_total(
    api: &mut impl Api,
    resource: Resource,
    game_type: NonZeroU64,
    control: &OperationControl,
) -> Result<Vec<Value>> {
    author_all(api, resource, game_type, control, true)
}
fn author_all(
    api: &mut impl Api,
    resource: Resource,
    game_type: NonZeroU64,
    control: &OperationControl,
    require_total: bool,
) -> Result<Vec<Value>> {
    let mut collected = Vec::new();
    let mut seen = BTreeSet::new();
    let mut known_total = None;
    for number in 1..=1000 {
        control.check()?;
        let payload = author_page(api, resource, game_type, "", Page { number, size: 100 })?;
        let rows = items(&payload)?;
        if rows.len() > 100 {
            return Err(data("DD author page exceeds its requested size"));
        }
        let current_total = total(&payload)?;
        if require_total && current_total.is_none() {
            return Err(data(
                "DD reference checks require a stable author-list total on every page",
            ));
        }
        if let (Some(old), Some(now)) = (known_total, current_total) {
            if old != now {
                return Err(data("DD author total changed during complete pagination"));
            }
        }
        if current_total.is_some() {
            known_total = current_total;
        }
        for row in rows {
            let id = identifier(row, &["sn", "share_sn", "id"])?;
            if !seen.insert(id) {
                return Err(data("DD author pagination repeated a resource identity"));
            }
            collected.push(row.clone());
        }
        if known_total.is_some_and(|n| collected.len() as u64 > n) {
            return Err(data("DD author pages exceeded their reported total"));
        }
        if known_total == Some(collected.len() as u64) {
            return Ok(collected);
        }
        if rows.is_empty() || (known_total.is_none() && rows.len() < 100) {
            return if known_total.is_none() {
                Ok(collected)
            } else {
                Err(data("DD author pagination ended before its reported total"))
            };
        }
    }
    Err(data("DD complete author list exceeded 1000 pages"))
}
pub(super) fn detail(
    api: &mut impl Api,
    resource: Resource,
    reference: &Reference,
) -> Result<Value> {
    let query = [("sn", reference.0.clone())];
    let mut payload = api.get(
        match resource {
            Resource::Plugin => "/addon/detail_v2",
            Resource::Config => "/share/detail",
            Resource::Wa => "/wa/detail",
        },
        &query,
    )?;
    if matches!(resource, Resource::Plugin) && !result(&payload)?.is_object() {
        payload = api.get("/addon/detail", &query)?;
    }
    let value = result(&payload)?
        .as_object()
        .map(|value| Value::Object(value.clone()))
        .ok_or_else(|| data("DD resource detail was not found"))?;
    let ids = ["sn", "share_sn"]
        .into_iter()
        .filter_map(|key| value.get(key))
        .filter(|v| !v.is_null() && v.as_str() != Some(""))
        .collect::<Vec<_>>();
    if !ids.is_empty()
        && !ids.iter().any(|id| {
            id.as_str()
                .map(str::to_owned)
                .or_else(|| id.as_u64().map(|v| v.to_string()))
                .as_deref()
                == Some(reference.0.as_str())
        })
    {
        return Err(data("DD resource detail does not match the selected SN"));
    }
    Ok(value)
}
pub(super) fn public_detail(value: Value) -> Value {
    output::public(safe_detail(value), &[])
}
pub(super) fn backup_view(value: &Value, sn: &str) -> Result<Value> {
    backup_detail(value, sn)
}
pub(super) fn selection_key(prefix: &str, parts: &[String]) -> String {
    projection::selector(prefix, parts)
}
pub(super) fn selected_backup_accounts(value: &Value) -> Result<&[Value]> {
    projection::backup_accounts(value)
}
pub(super) fn selected_role_identity(role: &Value) -> Result<String> {
    let id = projection::role_identity(role)?;
    Ok(id
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| id.to_string()))
}
pub(super) fn public_resource(resource: Resource, mut value: Value) -> Result<Value> {
    if resource == Resource::Config {
        if let Some(retail) = value.get_mut("retail_ui_config") {
            *retail = current_retail(retail)?;
        }
    }
    Ok(public_detail(value))
}
impl Read {
    pub(super) fn execute(&self, api: &mut impl Api, control: &OperationControl) -> Result<Value> {
        control.check()?;
        let value = match self {
            Self::GameTypes => {
                let payload = api.get("/game_type/list", &[])?;
                let values = items(&payload)?
                    .iter()
                    .map(|row| {
                        identifier(row, &["game_type"])?;
                        project(row, &["game_type", "name", "type", "def_game_version"])
                    })
                    .collect::<Result<Vec<_>>>()?;
                json!({"items":values,"total":values.len(),"dependencies":[{"parent":"game_type","children":["game_versions","associated_acts","category_ids"]}]})
            }
            Self::GameVersions(game_type) => game_versions(
                &api.get(
                    "/game_versions/list",
                    &[("game_type", game_type.to_string())],
                )?,
                *game_type,
            )?,
            Self::PluginCategories => plugin_categories(&api.get("/addon/category", &[])?)?,
            Self::WaCategories(game_type) => wa_categories(
                &api.get("/wa/categories", &[("game_type", game_type.to_string())])?,
                *game_type,
                control,
            )?,
            Self::VipLevels => {
                let payload =
                    api.get("/anchor_vip/level/list", &[("enrich_acts", "false".into())])?;
                let rows = options(&payload)?;
                json!({"items":safe_detail(json!(rows)),"total":rows.len()})
            }
            Self::AssociatedActs(game_type) => {
                let mut values = Vec::new();
                for resource in [Resource::Plugin, Resource::Config, Resource::Wa] {
                    for row in all_author(api, resource, *game_type, control)? {
                        values.push(json!({"sn":identifier(&row,&["sn","share_sn"] )?,"act_type":resource.act(),
                            "name":first(&row,&["name","title"]),"version":first(&row,&["version","current_version"])}));
                    }
                }
                json!({"items":values,"total":values.len(),"complete":true,"game_type":game_type.get()})
            }
            Self::List {
                resource,
                game_type,
                keyword,
                page,
            } => {
                if keyword.len() > 4096 || keyword.contains('\0') {
                    return Err(ToolkitError::arguments(
                        "DD keyword exceeds its supported text range",
                    ));
                }
                let payload = author_page(api, *resource, *game_type, keyword, *page)?;
                let mut value = page_view(&payload, *page, |row| author_item(*resource, row))?;
                value["game_type"] = json!(game_type.get());
                value["kind"] = json!(resource.name());
                value
            }
            Self::Detail {
                resource,
                reference,
            } => public_resource(*resource, detail(api, *resource, reference)?)?,
            Self::PluginVersions {
                reference,
                game_type,
                page,
            } => {
                let payload = api.get(
                    "/addon/addon_versions",
                    &[
                        ("sn", reference.0.clone()),
                        ("game_type", game_type.to_string()),
                        ("page", page.to_string()),
                    ],
                )?;
                let values = items(&payload)?;
                let total = total(&payload)?;
                if total.is_some_and(|n| n < values.len() as u64) {
                    return Err(data("DD version page contradicts its reported total"));
                }
                let complete = page.get() == 1
                    && (total == Some(values.len() as u64)
                        || (total.is_none() && values.is_empty()));
                json!({"items":safe_detail(json!(values)),"returned":values.len(),"total":total,"reference":reference.0,
                    "game_type":game_type.get(),"page":page.get(),"complete_version_list":complete,"truncated":!complete})
            }
            Self::Backups => {
                let payload = api.get("/backup/list", &[])?;
                let values = items(&payload)?
                    .iter()
                    .map(backup_item)
                    .collect::<Result<Vec<_>>>()?;
                let total = total(&payload)?;
                if total.is_some_and(|n| n < values.len() as u64) {
                    return Err(data("DD backup list contradicts its reported total"));
                }
                let complete = total.is_none_or(|n| n == values.len() as u64);
                json!({"items":values,"returned":values.len(),"total":total,"complete":complete,"truncated":!complete})
            }
            Self::Backup(reference) => {
                let payload = api.get("/backup/detail", &[("sn", reference.0.clone())])?;
                backup_detail(result(&payload)?, &reference.0)?
            }
        };
        control.check()?;
        Ok(output::public(value, &[]))
    }
}

#[cfg(test)]
mod tests;
