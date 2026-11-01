//! NewBeeBox Creator/Next API. Resource identities and the two relationship
//! namespaces are explicit; API authentication stays private to this adapter.
use crate::{
    newbee_session::CreatorSession,
    output,
    transport::{Http, Origin},
};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::{
    num::NonZeroU64,
    path::{Path, PathBuf},
};
mod backup_bundle;
mod backup_upload;
mod changelog;
mod cloud;
mod config;
mod create;
mod font_sdk;
mod media;
mod operations;
mod plugin;
mod relationships;
mod wa_write;
pub use backup_bundle::{BackupGenerate, scan_backup};
pub use backup_upload::{BackupCreate, BackupUpload};
pub use changelog::PluginChangelogEdit;
pub use cloud::{BackupDelete, BackupRead, BackupRename, BackupSelection};
pub use config::{ConfigEdit, ConfigUpdate};
pub use create::AuthorCreate;
pub use media::WaMediaUpload;
pub use operations::{AuthorDelete, WaChangelogEdit, WaShareCode};
pub use plugin::{PluginEdit, PluginUpdate};
pub use relationships::{RelationshipKind, RelationshipReplacement};
pub use wa_write::{WaEdit, WaUpdate};

#[derive(Clone, Copy)]
pub enum Resource {
    Plugin,
    Config,
    Wa,
}
impl Resource {
    fn key(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::Config => "config",
            Self::Wa => "wa",
        }
    }
    fn coauthor_type(self) -> u64 {
        match self {
            Self::Plugin => 1,
            Self::Config => 4,
            Self::Wa => 3,
        }
    }
    fn reference_type(self) -> u64 {
        match self {
            Self::Plugin => 1,
            Self::Config => 3,
            Self::Wa => 2,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Page {
    number: u32,
    size: u32,
}
impl Page {
    pub fn new(number: u32, size: u32) -> Result<Self> {
        if number == 0 || !(1..=100).contains(&size) {
            return Err(ToolkitError::arguments(
                "NewBeeBox page must be positive and page size must be 1..100",
            ));
        }
        Ok(Self { number, size })
    }
}
pub enum OptionKind {
    ContentOrigins,
    SubscribePlans,
    TimeRanges,
}
pub enum Read {
    Doctor,
    PluginCategories,
    GameVersions,
    Options(OptionKind),
    PluginList {
        keyword: String,
        page: Page,
    },
    ConfigList {
        keyword: String,
        offset: u64,
        size: u32,
    },
    WaList {
        keyword: String,
        offset: u64,
        size: u32,
    },
    Get {
        resource: Resource,
        id: NonZeroU64,
    },
    PluginVersions {
        id: NonZeroU64,
        page: Page,
    },
    PluginChangelogs {
        id: NonZeroU64,
        page: Page,
    },
    PluginChangelog {
        id: NonZeroU64,
    },
    WaCategories {
        game_version: NonZeroU64,
    },
    WaAttachmentPaths,
    ConfigBackups,
    ConfigBackup {
        id: NonZeroU64,
    },
    WaLatest {
        id: NonZeroU64,
    },
    WaChangelogs {
        id: NonZeroU64,
        page: Page,
    },
    CoauthorSearch {
        keyword: String,
    },
    Coauthors {
        resource: Resource,
        id: NonZeroU64,
    },
    ReferenceSearch {
        resource: Resource,
        keyword: String,
    },
    References {
        resource: Resource,
        id: NonZeroU64,
    },
}
pub struct Newbee {
    home: PathBuf,
    http: Http,
    session: Option<CreatorSession>,
    control: OperationControl,
}
impl Newbee {
    pub fn open(home: &Path, control: &OperationControl) -> Result<Self> {
        Ok(Self {
            home: home.to_owned(),
            http: Http::new(control)?,
            session: None,
            control: control.clone(),
        })
    }
    fn headers(&mut self) -> Result<HeaderMap> {
        if self.session.is_none() {
            self.session = Some(CreatorSession::open(&self.control)?);
        }
        self.session
            .as_mut()
            .expect("selected Creator session")
            .headers(&self.http, &self.control)
    }
    fn post(&mut self, origin: Origin, path: &str, body: &Value, mutation: bool) -> Result<Value> {
        let headers = self.headers()?;
        let payload = self.http.post(origin, path, headers, body, mutation)?;
        envelope(payload, path, mutation)
    }
    fn creator(&mut self, path: &str, body: Value) -> Result<Value> {
        self.post(Origin::NewbeeCreator, path, &body, false)
    }
    fn subject(&mut self) -> Result<String> {
        if self.session.is_none() {
            self.session = Some(CreatorSession::open(&self.control)?);
        }
        self.session
            .as_mut()
            .unwrap()
            .subject(&self.http, &self.control)
    }
    fn raw(&mut self, resource: Resource, id: u64) -> Result<Value> {
        let path = match resource {
            Resource::Plugin => "/creator/wow/mod/publish_detail",
            Resource::Config => "/creator/wow/share_config/details_aps",
            Resource::Wa => "/creator/wow/wa/detail_aps",
        };
        let value = object(self.creator(path, json!({"id":id}))?)?;
        if wire_id(pick(&value, &["t_id", "id"]))? != id {
            return Err(remote(
                "NewBeeBox detail belongs to a different resource identity",
            ));
        }
        Ok(value)
    }
    pub fn read(&mut self, request: &Read) -> Result<Value> {
        self.control.check()?;
        let value = match request {
            Read::Doctor => {
                self.headers()?;
                self.session.as_ref().unwrap().report()
            }
            Read::PluginCategories => {
                let metadata = self.http.get(
                    Origin::NewbeeMetadata,
                    "/modconfig.json",
                    &[],
                    HeaderMap::new(),
                )?;
                let items=array(metadata.get("mod_category").cloned().ok_or_else(||remote("plugin metadata has no category array"))?)?.into_iter().map(|item|json!({"id":pick(&item,&["t_id"]),"name":pick(&item,&["t_name"]),"parent_id":pick(&item,&["t_parent_category_id"]),"sort_index":pick(&item,&["t_show_index"])})).collect::<Vec<_>>();
                json!({"total":items.len(),"items":items})
            }
            Read::GameVersions => {
                let metadata = self.http.get(
                    Origin::NewbeeMetadata,
                    "/modconfig.json",
                    &[],
                    HeaderMap::new(),
                )?;
                let items=array(metadata.get("game_version").cloned().ok_or_else(||remote("plugin metadata has no game-version array"))?)?.into_iter().map(|item|json!({"id":pick(&item,&["id"]),"name":pick(&item,&["name"]),"search_enabled":pick(&item,&["search_enable"]),"versions":pick(&item,&["version"])})).collect::<Vec<_>>();
                json!({"total":items.len(),"items":items})
            }
            Read::Options(kind) => {
                let value = match kind {
                    OptionKind::ContentOrigins => {
                        self.creator("/v3/sys/content_origin_list", json!({}))?
                    }
                    OptionKind::SubscribePlans => {
                        self.creator("/creator/author_subscribe/plan_level_preset", json!({}))?
                    }
                    OptionKind::TimeRanges => self.post(
                        Origin::NewbeeNext,
                        "/cloudsaveserver/GameCloudSavePublish/GetTimeRangeList",
                        &json!({}),
                        false,
                    )?,
                };
                let items = option_rows(value)?
                    .into_iter()
                    .filter(|item| item.get("value").is_some_and(|v| !v.is_null()))
                    .map(
                        |item| json!({"label":pick(&item,&["label","name"]),"value":item["value"]}),
                    )
                    .collect::<Vec<_>>();
                json!({"total":items.len(),"items":items})
            }
            Read::PluginList { keyword, page } => {
                let raw=self.creator("/creator/wow/mod/publish_list",json!({"keyword":keyword,"game_version_id":0,"sort_by":"t_last_update","sort_order":"DESC","pagenum":page.number,"pagesize":page.size}))?;
                let known=!pick(&raw,&["total","count"]).is_null();
                let (total, items) = paged(raw)?;
                let items = items.iter().map(plugin).collect::<Result<Vec<_>>>()?;
                let offset=(page.number as u64-1)*page.size as u64;
                let more=offset.saturating_add(items.len() as u64)<total;
                json!({"total":if known{json!(total)}else{Value::Null},"coverage":if !known{"unknown"}else if offset==0&&items.len() as u64==total{"complete"}else{"partial"},"truncated":if known{json!(more)}else{Value::Null},"items":items,"page":page.number,"page_size":page.size,"next_page":if known&&more{Some(page.number as u64+1)}else{None}})
            }
            Read::ConfigList {
                keyword,
                offset,
                size,
            }
            | Read::WaList {
                keyword,
                offset,
                size,
            } => {
                if !(1..=100).contains(size) {
                    return Err(ToolkitError::arguments(
                        "NewBeeBox page size must be 1..100",
                    ));
                }
                let (path, config) = if matches!(request, Read::ConfigList { .. }) {
                    ("/creator/wow/share_config/publish_list", true)
                } else {
                    ("/creator/wow/wa/mtg_uc_publish_list", false)
                };
                let raw=self.creator(path,json!({"keyword":keyword,"game_version_id":0,"sort":3,"offset":offset,"pagesize":size}))?;
                let next = raw.get("next_offset").cloned();
                let known=!pick(&raw,&["total","count"]).is_null();
                let (total, items) = paged(raw)?;
                let items = items
                    .into_iter()
                    .map(|v| if config { configuration(&v) } else { wa(&v) })
                    .collect::<Result<Vec<_>>>()?;
                let end=offset.saturating_add(items.len() as u64);let more=end<total;
                let next=next.filter(|value|!value.is_null()).or_else(||(known&&more).then_some(json!(end)));
                json!({"total":if known{json!(total)}else{Value::Null},"coverage":if !known{"unknown"}else if *offset==0&&items.len() as u64==total{"complete"}else{"partial"},"truncated":if known{json!(more)}else{Value::Null},"items":items,"offset":offset,"page_size":size,"next_offset":next})
            }
            Read::Get { resource, id } => {
                let raw = self.raw(*resource, id.get())?;
                match resource {
                    Resource::Plugin => plugin(&raw)?,
                    Resource::Config => configuration(&raw)?,
                    Resource::Wa => wa(&raw)?,
                }
            }
            Read::PluginVersions { id, page } => history_page(self.creator(
                "/creator/wow/mod_file/mod_file_list",
                json!({"mod_id":id,"game_version_id":0,"pagenum":page.number,"pagesize":page.size}),
            )?,*page)?,
            Read::PluginChangelogs { id, page } => history_page(self.creator(
                "/creator/wow/mod_file/changelog_list",
                json!({"mod_id":id,"pagenum":page.number,"pagesize":page.size}),
            )?,*page)?,
            Read::PluginChangelog { id } => {
                self.creator("/creator/wow/mod_file/get_changelog", json!({"file_id":id}))?
            }
            Read::WaCategories { game_version } => self.creator(
                "/creator/wow/wa/category",
                json!({"game_version":game_version}),
            )?,
            Read::WaAttachmentPaths => {
                self.creator("/creator/wow/wa/attachment_install_path_list", json!({}))?
            }
            Read::ConfigBackups => self.config_backups()?,
            Read::ConfigBackup { id } => self.config_backup(id.get())?,
            Read::WaLatest { id } => {
                self.creator("/creator/wow/wa_log/latest_str_info", json!({"wa_id":id}))?
            }
            Read::WaChangelogs { id, page } => history_page(self.creator(
                "/creator/wow/wa_log/list",
                json!({"wa_id":id,"pagenum":page.number,"pagesize":page.size}),
            )?,*page)?,
            Read::CoauthorSearch { keyword } => {
                self.creator("/creator/co_author/search_user", json!({"keyword":keyword}))?
            }
            Read::Coauthors { resource, id } => self.creator(
                "/creator/co_author/list",
                json!({"content_type":resource.coauthor_type(),"content_id":id}),
            )?,
            Read::ReferenceSearch { resource, keyword } => self.creator(
                "/creator/content_reference/search",
                json!({"keyword":keyword,"limit":20,"target_types":[resource.reference_type()]}),
            )?,
            Read::References { resource, id } => self.creator(
                "/creator/content_reference/list",
                json!({"content_type":resource.reference_type(),"content_id":id}),
            )?,
        };
        let public_text = if matches!(
            request,
            Read::ConfigList { .. }
                | Read::Get {
                    resource: Resource::Config,
                    ..
                }
        ) {
            &["content"][..]
        } else {
            &[]
        };
        Ok(output::public(value, public_text))
    }
}
fn envelope(payload: Value, path: &str, mutation: bool) -> Result<Value> {
    let Some(code) = payload.get("code").and_then(Value::as_i64) else {
        return Err(remote("NewBeeBox response omitted a valid business outcome").with_details(json!({"endpoint":path,"submission":if mutation{"uncertain"}else{"not_applicable"},"replay_allowed":!mutation})));
    };
    if code != 1 {
        return Err(ToolkitError::new("publish.newbee_business","NewBeeBox rejected the operation",5)
        .with_details(json!({"endpoint":path,"business_code":code,"verification_required":mutation,"replay_allowed":!mutation})));
    }
    Ok(payload.get("data").cloned().unwrap_or(Value::Null))
}
/// Describe already admitted author input without emitting opaque imports.
/// Admission itself remains with the typed operation constructors.
pub fn input_summary(bytes: &[u8]) -> Result<Value> {
    Ok(output::public(
        Value::Object(crate::input::document(bytes)?),
        &["content"],
    ))
}
fn remote(message: &str) -> ToolkitError {
    ToolkitError::new("publish.newbee_data", message, 5)
}
fn object(value: Value) -> Result<Value> {
    match value {
        Value::Object(_) => Ok(value),
        Value::Array(items) => items
            .into_iter()
            .next()
            .filter(Value::is_object)
            .ok_or_else(|| remote("NewBeeBox detail is missing")),
        _ => Err(remote("NewBeeBox detail is not an object")),
    }
}
fn array(value: Value) -> Result<Vec<Value>> {
    match value {
        Value::Array(items) => Ok(items),
        Value::String(text) => match crate::input::value(text.as_bytes())
            .map_err(|_| remote("NewBeeBox encoded array is invalid"))?
        {
            Value::Array(items) => Ok(items),
            _ => Err(remote("NewBeeBox encoded value is not an array")),
        },
        _ => Err(remote("NewBeeBox response is not an array")),
    }
}
fn pick(value: &Value, names: &[&str]) -> Value {
    names
        .iter()
        .find_map(|key| value.get(*key).filter(|v| !v.is_null()))
        .cloned()
        .unwrap_or(Value::Null)
}
fn option_rows(value: Value) -> Result<Vec<Value>> {
    if value.is_array() {
        return array(value);
    }
    for key in ["list", "items", "rows", "options", "data"] {
        if let Some(items) = value.get(key).filter(|v| v.is_array()) {
            return array(items.clone());
        }
    }
    Err(remote("NewBeeBox option response has no option array"))
}
fn paged(value: Value) -> Result<(u64, Vec<Value>)> {
    let value = object(value)?;
    let rows = array(pick(&value, &["list", "items"]))?;
    let total = wire_uint(pick(&value, &["total", "count"]), rows.len() as u64)?;
    if total < rows.len() as u64 || rows.iter().any(|v| !v.is_object()) {
        return Err(remote(
            "NewBeeBox page does not match its total or item shape",
        ));
    }
    Ok((total, rows))
}
fn history_page(value: Value, page: Page) -> Result<Value> {
    let mut value = object(value)?;
    let known = !pick(&value, &["total", "count"]).is_null();
    let (total, rows) = paged(value.clone())?;
    let offset = (page.number as u64 - 1) * page.size as u64;
    let more = offset.saturating_add(rows.len() as u64) < total;
    value["coverage"] = json!(if !known {
        "unknown"
    } else if offset == 0 && rows.len() as u64 == total {
        "complete"
    } else {
        "partial"
    });
    value["truncated"] = if known { json!(more) } else { Value::Null };
    value["page"] = json!(page.number);
    value["page_size"] = json!(page.size);
    value["next_page"] = if known && more {
        json!(page.number as u64 + 1)
    } else {
        Value::Null
    };
    Ok(value)
}
fn plugin(raw: &Value) -> Result<Value> {
    let mut result = json!({"id":wire_id(pick(raw,&["t_id","id"]))?,"name":pick(raw,&["t_name","name"]),"public":wire_uint(pick(raw,&["t_share","share_state"]),0)?==1,
        "review_status":pick(raw,&["t_check","review_status"]),"intro":pick(raw,&["t_description","intro"]),"description":pick(raw,&["t_description_v2","description"])});
    for (target, fields) in [
        (
            "content_format",
            &["t_content_format", "content_format"][..],
        ),
        ("content_origin", &["t_original", "content_origin"]),
        ("logo", &["t_logo", "logo"]),
        ("screenshots", &["screenshots"]),
        ("game_versions", &["game_versions"]),
        ("category_ids", &["category_ids", "mod_categories"]),
        (
            "subscribe_plan_level",
            &["t_subscribe_plan_level", "subscribe_plan_level"],
        ),
        ("link_to_channel", &["t_link_to_channel", "link_to_channel"]),
        ("updated_at", &["t_last_update", "updated_at"]),
    ] {
        result[target] = pick(raw, fields);
    }
    result["screenshots"] = json!(urls(result["screenshots"].take())?);
    let versions=optional_array(result["game_versions"].take())?.into_iter().map(|version|json!({"id":version.get("id"),"name":version.get("name"),"support_version":version.get("support_version"),"tag":version.get("tag")})).collect::<Vec<_>>();
    result["game_versions"] = json!(versions);
    result["category_ids"] = json!(optional_array(result["category_ids"].take())?);
    Ok(result)
}
fn configuration(raw: &Value) -> Result<Value> {
    let mut result =
        json!({"id":wire_id(pick(raw,&["t_id","id"]))?,"title":pick(raw,&["t_title","title"])});
    for (target, fields) in [
        ("cloud_id", &["t_cloudblackid", "cloud_id"][..]),
        ("public", &["t_sharing", "sharing"]),
        ("review_status", &["t_check", "review_status"]),
        ("content", &["t_content", "content"]),
        ("content_format", &["t_content_format", "content_format"]),
        ("intro", &["t_intro", "intro"]),
        ("picture_urls", &["pic_url", "picture_urls", "piclist"]),
        ("content_origin", &["t_content_origin", "content_origin"]),
        ("link_to_channel", &["t_link_to_channel", "link_to_channel"]),
        (
            "subscribe_plan_level",
            &["t_subscribe_plan_level", "subscribe_plan_level"],
        ),
        ("price", &["t_price", "price"]),
        ("time_range", &["t_time_range", "time_range"]),
        ("linked_mods", &["t_linked_mods", "linked_mods"]),
        (
            "ignored_unknown_mods",
            &["t_ignored_unknown_mods", "ignored_unknown_mods"],
        ),
        (
            "ignored_materials",
            &["t_ignored_materials", "ignored_materials"],
        ),
        ("ignored_fronts", &["t_ignored_fronts", "ignored_fronts"]),
        ("roleid", &["t_roleid", "roleid", "role_id"]),
        ("selected_eml", &["t_selected_eml", "selected_eml"]),
    ] {
        result[target] = pick(raw, fields);
    }
    result["public"] = json!(wire_uint(result["public"].take(), 0)? != 0);
    result["cloud_id"] = json!(wire_uint(result["cloud_id"].take(), 0)?);
    result["picture_urls"] = json!(urls(result["picture_urls"].take())?);
    for key in [
        "linked_mods",
        "ignored_unknown_mods",
        "ignored_materials",
        "ignored_fronts",
        "selected_eml",
    ] {
        result[key] = json!(optional_array(result[key].take())?);
    }
    result["roleid"] = match result["roleid"].take() {
        Value::Null => json!(""),
        Value::String(text) => json!(text),
        Value::Number(number) if number.as_u64() == Some(0) => json!(""),
        Value::Number(number) if number.as_u64().is_some_and(|n| n > 0) => {
            json!(number.to_string())
        }
        _ => {
            return Err(remote(
                "NewBeeBox role identity must be a string or positive integer",
            ));
        }
    };
    Ok(result)
}
fn wa(raw: &Value) -> Result<Value> {
    let mut result = json!({"id":wire_id(pick(raw,&["t_id","id"]))?,"name":pick(raw,&["t_name","name"]),"public":wire_uint(pick(raw,&["t_share_state","share_state"]),2)?==1});
    for (target, fields) in [
        ("version", &["t_version", "version"][..]),
        ("review_status", &["t_check_status", "review_status"]),
        ("game_version_id", &["t_game_version_id", "game_version_id"]),
        ("thumbnail", &["t_thumbnail", "thumbnail"]),
        ("images", &["t_images", "images"]),
        ("categories", &["category_list"]),
        ("attachments", &["t_attachments", "attachments"]),
        ("content_format", &["t_content_format", "content_format"]),
        ("content_origin", &["t_content_origin", "content_origin"]),
        (
            "subscribe_plan_level",
            &["t_subscribe_plan_level", "subscribe_plan_level"],
        ),
        ("price", &["price", "t_price"]),
        ("time_range", &["t_time_range", "time_range"]),
        ("link_to_channel", &["t_link_to_channel", "link_to_channel"]),
        ("intro", &["t_intro", "intro"]),
        ("description", &["t_description", "description"]),
        ("wa_str_titles", &["t_wa_str_titles", "wa_str_titles"]),
        ("updated_at", &["t_update_time", "updated_at"]),
    ] {
        result[target] = pick(raw, fields);
    }
    result["images"] = json!(urls(result["images"].take())?);
    for key in ["attachments", "wa_str_titles"] {
        result[key] = json!(optional_array(result[key].take())?);
    }
    result["categories"] = json!(
        optional_array(result["categories"].take())?
            .into_iter()
            .map(|v| json!({"id":v.get("t_id"),"name":pick(&v,&["t_show_name","t_name"])}))
            .collect::<Vec<_>>()
    );
    Ok(result)
}
fn optional_array(value: Value) -> Result<Vec<Value>> {
    if value.is_null() {
        Ok(Vec::new())
    } else {
        array(value)
    }
}
fn wire_uint(value: Value, default: u64) -> Result<u64> {
    match value {
        Value::Null => Ok(default),
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| remote("NewBeeBox integer field is invalid")),
        Value::String(text) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => text
            .parse()
            .map_err(|_| remote("NewBeeBox integer field exceeds its range")),
        _ => Err(remote("NewBeeBox integer field is invalid")),
    }
}
fn wire_id(value: Value) -> Result<u64> {
    let id = wire_uint(value, 0)?;
    if id == 0 {
        return Err(remote("NewBeeBox resource identity is missing or invalid"));
    }
    Ok(id)
}
fn urls(value: Value) -> Result<Vec<String>> {
    optional_array(value)?
        .into_iter()
        .map(|value| {
            let value = if value.is_object() {
                pick(&value, &["media_url", "url", "name"])
            } else {
                value
            };
            value
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| remote("NewBeeBox image reference is invalid"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Reply, Server, TestDir};
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    pub(super) fn provider(dir: &TestDir, server: &Server, authenticate: bool) -> Newbee {
        let control = OperationControl::default();
        let session = if authenticate {
            let expiry = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 600;
            let access = format!(
                "head.{}.sig",
                URL_SAFE_NO_PAD.encode(
                    serde_json::to_vec(&json!({"exp":expiry,"sub":"fixture-author"})).unwrap()
                )
            );
            std::fs::write(dir.0.join("access-token"), access).unwrap();
            std::fs::write(dir.0.join("refresh-token"), "FixtureRefresh").unwrap();
            Some(CreatorSession::fixture(dir.0.clone()))
        } else {
            None
        };
        Newbee {
            home: dir.0.join("managed-home"),
            http: Http::loopback(&control, server.url.clone()),
            session,
            control,
        }
    }
    pub(super) fn authenticated(mut replies: Vec<Reply>) -> Vec<Reply> {
        let mut start = vec![
            Reply::Json(json!({"code":1,"data":{"code":"fixture-code"}})),
            Reply::Json(json!({"code":1,"data":{"token":"FixtureAuthor","jwtToken":"FixtureJWT"}})),
            Reply::Json(json!({"code":1,"data":{"resource_token":"FixtureResource"}})),
        ];
        start.append(&mut replies);
        start
    }
    pub(super) fn set_subject(dir: &TestDir, subject: &str) {
        let expiry = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 600;
        let access = format!(
            "head.{}.sig",
            URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&json!({"exp":expiry,"sub":subject})).unwrap())
        );
        std::fs::write(dir.0.join("access-token"), access).unwrap();
    }
    #[test]
    fn public_metadata_reads_need_no_desktop_credentials() {
        let dir = TestDir::new();
        let server = Server::new(vec![
            Reply::Json(
                json!({"mod_category":[{"t_id":31,"t_name":"UI","t_parent_category_id":0,"t_show_index":2}]}),
            ),
            Reply::Json(
                json!({"game_version":[{"id":5,"name":"Classic","search_enable":true,"version":["5.5.4"]}]}),
            ),
        ]);
        let mut provider = provider(&dir, &server, false);
        let categories = provider.read(&Read::PluginCategories).unwrap();
        let versions = provider.read(&Read::GameVersions).unwrap();
        assert_eq!(
            categories["items"][0],
            json!({"id":31,"name":"UI","parent_id":0,"sort_index":2})
        );
        assert_eq!(versions["items"][0]["versions"], json!(["5.5.4"]));
        assert!(provider.session.is_none());
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        for request in requests {
            assert_eq!(request.line, "GET /modconfig.json HTTP/1.1");
            assert!(!request.headers.contains("authorization"));
        }
    }
    #[test]
    fn selected_author_pages_keep_partial_and_unknown_coverage_with_their_cursor() {
        let dir = TestDir::new();
        let server = Server::new(authenticated(vec![
            Reply::Json(
                json!({"code":1,"data":{"list":[{"t_id":719,"t_title":"First"}],"total":3}}),
            ),
            Reply::Json(json!({"code":1,"data":{"list":[{"t_id":720,"t_name":"Unknown total"}]}})),
        ]));
        let mut provider = provider(&dir, &server, true);
        let page = provider
            .read(&Read::ConfigList {
                keyword: "".into(),
                offset: 0,
                size: 1,
            })
            .unwrap();
        assert_eq!(page["coverage"], "partial");
        assert_eq!(page["truncated"], true);
        assert_eq!(page["next_offset"], 1);
        assert_eq!(page["total"], 3);
        let unknown = provider
            .read(&Read::PluginList {
                keyword: "".into(),
                page: Page::new(1, 1).unwrap(),
            })
            .unwrap();
        assert_eq!(unknown["coverage"], "unknown");
        assert!(unknown["total"].is_null());
        assert!(unknown["truncated"].is_null());
        assert_eq!(server.finish().len(), 5);
    }
    #[test]
    fn relationship_reads_keep_the_independent_resource_namespaces() {
        let dir = TestDir::new();
        let server = Server::new(authenticated(
            (0..6)
                .map(|_| Reply::Json(json!({"code":1,"data":[]})))
                .collect(),
        ));
        let mut provider = provider(&dir, &server, true);
        let id = NonZeroU64::new(719).unwrap();
        for resource in [Resource::Plugin, Resource::Config, Resource::Wa] {
            provider.read(&Read::Coauthors { resource, id }).unwrap();
            provider.read(&Read::References { resource, id }).unwrap();
        }
        let requests = server.finish();
        let bodies = requests[3..]
            .iter()
            .map(|request| serde_json::from_slice::<Value>(&request.body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            bodies
                .iter()
                .map(|v| v["content_type"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 1, 4, 3, 3, 2]
        );
        assert!(bodies.iter().all(|value| value["content_id"] == 719));
        assert!(
            requests[3..]
                .iter()
                .all(|r| r.headers.contains("authorization: Bearer FixtureResource"))
        );
    }
    #[test]
    fn creator_detail_decodes_selected_arrays_and_preserves_exact_role_and_layout_identity() {
        let dir = TestDir::new();
        let selected =
            json!([{"server_owned_layout_id":9007199254740993u64,"all_members":[1,2,3]}]);
        let server = Server::new(authenticated(vec![Reply::Json(
            json!({"code":1,"data":[{"t_id":"719","t_title":"UI set","t_cloudblackid":17,"t_sharing":1,"t_content":"Public author description","t_roleid":9007199254740993u64,
            "pic_url":"[{\"media_url\":\"https://cdn2.newbeebox.com/image.png\"}]","t_linked_mods":"[{\"mod_id\":31}]","t_selected_eml":selected.to_string(),"token":"never-output","raw_wtf":"never-output-wtf"}]}),
        )]));
        let mut provider = provider(&dir, &server, true);
        let output = provider
            .read(&Read::Get {
                resource: Resource::Config,
                id: NonZeroU64::new(719).unwrap(),
            })
            .unwrap();
        assert_eq!(output["id"], 719);
        assert_eq!(output["public"], true);
        assert_eq!(output["roleid"], "9007199254740993");
        assert_eq!(output["selected_eml"], selected);
        assert_eq!(
            output["picture_urls"],
            json!(["https://cdn2.newbeebox.com/image.png"])
        );
        assert_eq!(output["linked_mods"], json!([{"mod_id":31}]));
        assert_eq!(output["content"], "Public author description");
        assert!(!output.to_string().contains("never-output"));
        assert_eq!(server.finish().len(), 4);
    }
    #[test]
    fn wa_history_summarizes_raw_strings_and_does_not_emit_signed_capabilities() {
        let dir = TestDir::new();
        let server = Server::new(authenticated(vec![Reply::Json(
            json!({"code":1,"data":{"version":"8","t_wa_str":"!WA:2!private","content":"raw-import-content","download_url":"https://cdn.invalid/file?Signature=private","token":"HiddenToken"}}),
        )]));
        let mut provider = provider(&dir, &server, true);
        let output = provider
            .read(&Read::WaLatest {
                id: NonZeroU64::new(71).unwrap(),
            })
            .unwrap();
        assert_eq!(output["version"], "8");
        assert_eq!(output["t_wa_str"]["length"], 13);
        let wire = output.to_string();
        for secret in [
            "!WA:2!private",
            "raw-import-content",
            "Signature=private",
            "HiddenToken",
        ] {
            assert!(!wire.contains(secret));
        }
        let requests = server.finish();
        assert_eq!(
            requests[3].line,
            "POST /creator/wow/wa_log/latest_str_info HTTP/1.1"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[3].body).unwrap(),
            json!({"wa_id":71})
        );
    }
    #[test]
    fn full_detail_rejects_a_different_record_before_form_projection() {
        let dir = TestDir::new();
        let server = Server::new(authenticated(vec![Reply::Json(
            json!({"code":1,"data":{"t_id":720,"t_title":"unrelated record"}}),
        )]));
        let mut provider = provider(&dir, &server, true);
        let error = provider
            .read(&Read::Get {
                resource: Resource::Config,
                id: NonZeroU64::new(719).unwrap(),
            })
            .unwrap_err();
        assert_eq!(error.code, "publish.newbee_data");
        assert_eq!(server.finish().len(), 4);
    }
    fn reference_document(target: u64) -> Value {
        json!({"schema":"lycheedev.publish.v1.newbee.config-reference.set","source_id":719,"references":[{"type":2,"id":target}]})
    }
    fn reference_replacement(target: u64) -> RelationshipReplacement {
        RelationshipReplacement::parse(
            Resource::Config,
            RelationshipKind::References,
            &serde_json::to_vec(&reference_document(target)).unwrap(),
        )
        .unwrap()
    }
    fn reference_readback(target: u64) -> Value {
        json!({"code":1,"data":{"source_type":3,"source_id":719,"references":[{"type":3,"id":target}]}})
    }
    #[test]
    fn complete_reference_replacement_uses_picker_types_and_verifies_display_types() {
        let dir = TestDir::new();
        let server = Server::new(authenticated(vec![
            Reply::Json(json!({"code":1,"data":[]})),
            Reply::Json(json!({"code":1,"data":{}})),
            Reply::Json(reference_readback(42)),
        ]));
        let mut provider = provider(&dir, &server, true);
        let result = provider
            .replace_relationships(&reference_replacement(42))
            .unwrap();
        assert_eq!(result["changed"], true);
        assert_eq!(result["author_writes"], 1);
        assert_eq!(result["recovered"], false);
        assert_eq!(result["operation_id"].as_str().unwrap().len(), 64);
        let requests = server.finish();
        assert_eq!(
            requests[4].line,
            "POST /creator/content_reference/set HTTP/1.1"
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[4].body).unwrap(),
            json!({"source_type":3,"source_id":719,"references":[{"type":2,"id":42}]})
        );
        let state: Value = lycheedev_core::storage::read_json(
            &dir.0
                .join("managed-home/publish/newbee/records/config/719/active.json"),
        )
        .unwrap();
        assert_eq!(state["state"], "verified");
        assert!(!state.to_string().contains("FixtureAuthor"));
        let records = dir.0.join("managed-home/publish/newbee/records/config/719");
        let json_files = std::fs::read_dir(records)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "json")
            })
            .count();
        assert_eq!(
            json_files, 1,
            "one atomic current receipt; no duplicate operation archive"
        );
    }
    #[test]
    fn uncertain_reference_write_recovers_by_readback_without_a_second_set() {
        let dir = TestDir::new();
        let server = Server::new(authenticated(vec![
            Reply::Json(json!({"code":1,"data":[]})),
            Reply::Drop,
            Reply::Json(reference_readback(42)),
        ]));
        let mut provider = provider(&dir, &server, true);
        let replacement = reference_replacement(42);
        let error = provider.replace_relationships(&replacement).unwrap_err();
        assert_eq!(error.details["submission"], "uncertain");
        let original = error.details["operation_id"].clone();
        let recovered = provider.replace_relationships(&replacement).unwrap();
        assert_eq!(recovered["operation_id"], original);
        assert_eq!(recovered["recovered"], true);
        assert_eq!(recovered["author_writes"], 0);
        let requests = server.finish();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.line == "POST /creator/content_reference/set HTTP/1.1")
                .count(),
            1
        );
    }
    #[test]
    fn changed_pending_replacement_is_rejected_and_matching_unverified_input_is_not_resent() {
        let dir = TestDir::new();
        let server = Server::new(authenticated(vec![
            Reply::Json(json!({"code":1,"data":[]})),
            Reply::Drop,
            Reply::Json(json!({"code":1,"data":[]})),
            Reply::Json(reference_readback(42)),
        ]));
        let mut provider = provider(&dir, &server, true);
        let original = reference_replacement(42);
        provider.replace_relationships(&original).unwrap_err();
        assert_eq!(
            provider
                .replace_relationships(&reference_replacement(43))
                .unwrap_err()
                .exit_code,
            6
        );
        assert_eq!(
            provider
                .replace_relationships(&original)
                .unwrap_err()
                .exit_code,
            6
        );
        assert_eq!(
            provider.replace_relationships(&original).unwrap()["recovered"],
            true
        );
        let requests = server.finish();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.line == "POST /creator/content_reference/set HTTP/1.1")
                .count(),
            1
        );
    }
    #[test]
    fn coauthor_admission_rejects_ambiguous_shares_and_unchanged_state_sends_no_set() {
        let dir = TestDir::new();
        let base = json!({"schema":"lycheedev.publish.v1.newbee.wa-co-author.set","content_id":71,"co_authors":[{"user_id":42,"share_percent":0.5}]});
        for authors in [
            json!([{"user_id":42,"share_percent":true}]),
            json!([{"user_id":42,"share_percent":{"$serde_json::private::Number":"0.5"}}]),
            json!([{"user_id":42,"share_percent":0.6},{"user_id":43,"share_percent":0.5}]),
            json!([{"user_id":42,"share_percent":0.2},{"user_id":42,"share_percent":0.2}]),
        ] {
            let mut value = base.clone();
            value["co_authors"] = authors;
            assert!(
                RelationshipReplacement::parse(
                    Resource::Wa,
                    RelationshipKind::Coauthors,
                    &serde_json::to_vec(&value).unwrap()
                )
                .is_err()
            );
        }
        let replacement = RelationshipReplacement::parse(
            Resource::Wa,
            RelationshipKind::Coauthors,
            &serde_json::to_vec(&base).unwrap(),
        )
        .unwrap();
        let server = Server::new(authenticated(vec![Reply::Json(
            json!({"code":1,"data":{"co_authors":[{"t_user_id":"42","t_share_percent":"0.5"}]}}),
        )]));
        let mut provider = provider(&dir, &server, true);
        let result = provider.replace_relationships(&replacement).unwrap();
        assert_eq!(result["changed"], false);
        assert_eq!(result["author_writes"], 0);
        let requests = server.finish();
        assert_eq!(requests.len(), 4);
        assert_eq!(
            serde_json::from_slice::<Value>(&requests[3].body).unwrap(),
            json!({"content_type":3,"content_id":71})
        );
        assert!(
            !dir.0
                .join("managed-home/publish/newbee/records/wa/71/active.json")
                .exists()
        );
    }
    #[test]
    fn coauthor_write_preserves_decimal_input_and_verifies_the_complete_replacement() {
        let dir = TestDir::new();
        let replacement=RelationshipReplacement::parse(Resource::Config,RelationshipKind::Coauthors,br#"{"schema":"lycheedev.publish.v1.newbee.config-co-author.set","content_id":719,"co_authors":[{"user_id":9007199254740993,"share_percent":0.30}]}"#).unwrap();
        let server = Server::new(authenticated(vec![
            Reply::Json(json!({"code":1,"data":[]})),
            Reply::Json(json!({"code":1})),
            Reply::Json(
                json!({"code":1,"data":{"co_authors":[{"user_id":"9007199254740993","share_percent":"0.3"}]}}),
            ),
        ]));
        let mut provider = provider(&dir, &server, true);
        let report = provider.replace_relationships(&replacement).unwrap();
        assert_eq!(report["author_writes"], 1);
        let requests = server.finish();
        assert_eq!(requests[4].line, "POST /creator/co_author/set HTTP/1.1");
        let body = String::from_utf8(requests[4].body.clone()).unwrap();
        assert!(body.contains("\"share_percent\":0.30"));
        assert!(body.contains("\"user_id\":9007199254740993"));
        assert!(body.contains("\"content_type\":4"));
    }
    #[test]
    fn plugin_changelog_clear_waits_for_its_readback_without_repeating_the_edit() {
        let dir = TestDir::new();
        let edit=PluginChangelogEdit::parse(br#"{"schema":"lycheedev.publish.v1.newbee.plugin-changelog.edit","file_id":991,"changelog":null}"#).unwrap();
        let old = json!({"code":1,"data":{"file_id":"991","changelog":"old content"}});
        let server = Server::new(authenticated(vec![
            Reply::Json(old.clone()),
            Reply::Json(json!({"code":1})),
            Reply::Json(old),
            Reply::Json(json!({"code":1,"data":{"file_id":991,"changelog":null}})),
        ]));
        let mut provider = provider(&dir, &server, true);
        let result = provider.edit_plugin_changelog(&edit).unwrap();
        assert_eq!(result["read_attempts"], 2);
        assert_eq!(result["verification"], "readback");
        let requests = server.finish();
        let writes = requests
            .iter()
            .filter(|r| r.line == "POST /creator/wow/mod_file/edit_changelog HTTP/1.1")
            .collect::<Vec<_>>();
        assert_eq!(writes.len(), 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&writes[0].body).unwrap(),
            json!({"file_id":991,"changelog":""})
        );
    }
    #[test]
    fn changelog_wrong_file_readback_retains_the_original_operation_for_recovery() {
        let dir = TestDir::new();
        let edit=PluginChangelogEdit::parse(br#"{"schema":"lycheedev.publish.v1.newbee.plugin-changelog.edit","file_id":991,"changelog":"new content"}"#).unwrap();
        let server = Server::new(authenticated(vec![
            Reply::Json(json!({"code":1,"data":{"file_id":991,"changelog":"old content"}})),
            Reply::Json(json!({"code":1})),
            Reply::Json(json!({"code":1,"data":{"file_id":992,"changelog":"new content"}})),
            Reply::Json(json!({"code":1,"data":{"file_id":991,"changelog":"new content"}})),
        ]));
        let mut provider = provider(&dir, &server, true);
        let error = provider.edit_plugin_changelog(&edit).unwrap_err();
        assert_eq!(error.exit_code, 6);
        let operation = error.details["operation_id"].clone();
        let report = provider.edit_plugin_changelog(&edit).unwrap();
        assert_eq!(report["operation_id"], operation);
        assert_eq!(report["recovered"], true);
        assert_eq!(report["author_writes"], 0);
        let requests = server.finish();
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.line == "POST /creator/wow/mod_file/edit_changelog HTTP/1.1")
                .count(),
            1
        );
    }
}
