//! Native adapter for the official creator CLI protocol. This channel uses
//! NCC credentials and normalized CLI forms, independently of desktop Creator.
use crate::{
    input, output,
    transport::{Http, Origin, secret_header},
};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::Read,
    num::NonZeroU64,
    path::{Path, PathBuf},
};
mod config_text;
mod plugin_version;
mod plugin_workspace;
mod project;
mod project_files;
mod publication;
mod wa_update;
mod writes;
pub use config_text::ConfigTextUpdate;
pub use plugin_version::PluginVersion;
pub use plugin_workspace::PluginWorkspace;
pub use project::PluginPack;
pub use project_files::TextProject;
pub use publication::Publication;
pub use wa_update::WaFileUpdate;
pub use writes::{ChangelogEdit, ReviewReply};

// Wire compatibility, fixed from the official 0.1.29 package. This is not the
// Lychee package/update version and never selects an npm installation channel.
const PROTOCOL_VERSION: &str = "0.1.29";
const API: &str = "/creator/cli/v1";
#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Resource {
    Plugin,
    Wa,
    Config,
}
impl Resource {
    fn route(self) -> &'static str {
        match self {
            Self::Plugin => "mod",
            Self::Wa => "wa",
            Self::Config => "share_config",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::Wa => "wa",
            Self::Config => "config",
        }
    }
    fn review_type(self) -> u8 {
        match self {
            Self::Plugin => 1,
            Self::Wa => 2,
            Self::Config => 3,
        }
    }
}
pub struct AuthorList {
    resource: Resource,
    page: NonZeroU64,
    size: Option<u16>,
    keyword: String,
    paid: Option<bool>,
}
impl AuthorList {
    pub fn new(
        resource: Resource,
        page: NonZeroU64,
        size: Option<u16>,
        keyword: String,
        paid: Option<bool>,
    ) -> Result<Self> {
        if matches!(resource, Resource::Plugin) {
            if size.is_some_and(|size| !(10..=2000).contains(&size)) || paid.is_some() {
                return Err(ToolkitError::arguments(
                    "official plugin lists accept size 10..2000 and no paid/free filter",
                ));
            }
        } else if size.is_some() {
            return Err(ToolkitError::arguments(
                "official WA/config lists use the platform page size",
            ));
        }
        Ok(Self {
            resource,
            page,
            size,
            keyword,
            paid,
        })
    }
}
#[derive(Clone, Copy)]
pub enum ReplyStatus {
    All,
    Unreplied,
    Replied,
}
pub enum Query {
    Doctor,
    List(AuthorList),
    Detail {
        resource: Resource,
        id: NonZeroU64,
    },
    PluginVersions {
        id: NonZeroU64,
        page: NonZeroU64,
        size: u16,
    },
    PluginChangelog {
        file_id: NonZeroU64,
    },
    WaCategories {
        game_version: NonZeroU64,
    },
    Backups,
    Backup {
        id: NonZeroU64,
    },
    Comments {
        resource: Resource,
        id: NonZeroU64,
        page: NonZeroU64,
        status: ReplyStatus,
    },
    Reference {
        resource: Resource,
        id: NonZeroU64,
        link: bool,
        game_version: Option<NonZeroU64>,
    },
}
pub struct MediaUpload {
    resource: Resource,
    path: PathBuf,
}
impl MediaUpload {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Doc {
            schema: String,
            resource: Resource,
            file: String,
        }
        let raw = input::document(bytes)?;
        let doc: Doc = serde_json::from_value(json!(raw)).map_err(|_| {
            input::invalid(
                "$",
                "official media input requires schema, resource and file",
            )
        })?;
        if doc.schema != "lycheedev.publish.v1.newbee.official.media.upload" {
            return Err(input::invalid(
                "$.schema",
                "select the official media upload schema",
            ));
        }
        let path = cwd.join(doc.file);
        media_type(&path)?;
        if !path.is_file() {
            return Err(input::invalid("$.file", "select an existing media file"));
        }
        Ok(Self {
            resource: doc.resource,
            path,
        })
    }
    pub fn schema() -> Value {
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"schema":{"const":"lycheedev.publish.v1.newbee.official.media.upload"},"resource":{"enum":["plugin","wa","config"]},"file":{"type":"string","minLength":1,"description":"PNG/JPG/JPEG/GIF/WEBP/BMP/ICO image or MP4 video. Relative paths resolve from the invocation directory."}},"required":["schema","resource","file"],"additionalProperties":false})
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let (file, sha256) = storage::locked_sha256(&self.path, u64::MAX, control)?;
        let size = file.metadata()?.len();
        storage::finish_file(
            file,
            Ok(
                json!({"provider":"newbee_official","resource":self.resource,"file_sha256":sha256,"size":size,"content_type":media_type(&self.path)?,"credential_reads":false,"author_writes":0,"remote_validation":"not_run"}),
            ),
            control,
        )
    }
}
struct Session {
    headers: HeaderMap,
    source: &'static str,
}
fn auth() -> ToolkitError {
    ToolkitError::new(
        "publish.ncc_auth",
        "official NewBee CLI authentication is unavailable; configure NCC_TOKEN locally or sign in with the official ncc client",
        3,
    )
}
impl Session {
    fn load(control: &OperationControl) -> Result<Self> {
        let token = match std::env::var("NCC_TOKEN") {
            Ok(value) => Some(value).filter(|value| !value.is_empty()),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err(auth()),
        };
        if let Some(token) = token {
            return Self::token(&token, "environment");
        }
        let profile = std::env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(auth)?;
        Self::profile(&profile, control)
    }
    fn profile(root: &Path, control: &OperationControl) -> Result<Self> {
        let path = root.join(".ncc/config.json");
        storage::guard_path_with_check(root, &path, || control.check())?;
        let (mut file, _) = storage::locked_sha256(&path, 65536, control).map_err(|error| {
            if matches!(
                error.code.as_str(),
                "command.cancelled" | "command.deadline"
            ) {
                error
            } else {
                auth()
            }
        })?;
        let result = (|| {
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::Start(0)).map_err(|_| auth())?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map_err(|_| auth())?;
            control.check()?;
            let value = input::value(&bytes).map_err(|_| auth())?;
            let token = value["token"].as_str().ok_or_else(auth)?;
            Self::token(token, "official_config")
        })();
        storage::finish_file(file, result, control)
    }
    fn token(token: &str, source: &'static str) -> Result<Self> {
        if !token.starts_with("ncc_")
            || token.len() > 65536
            || token.len() <= 4
            || !token.is_ascii()
            || token
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b' ')
        {
            return Err(auth());
        }
        let mut headers =
            secret_header("authorization", &format!("Bearer {token}")).map_err(|_| auth())?;
        headers.insert(
            "x-ncc-cli-version",
            HeaderValue::from_static(PROTOCOL_VERSION),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&format!("ncc-cli/{PROTOCOL_VERSION}"))
                .expect("fixed protocol user agent"),
        );
        Ok(Self { headers, source })
    }
}
pub struct Official {
    home: PathBuf,
    http: Http,
    control: OperationControl,
    session: Option<Session>,
    author: Option<u64>,
}
impl Official {
    pub fn open(home: &Path, control: &OperationControl) -> Result<Self> {
        Ok(Self {
            home: home.to_owned(),
            http: Http::new(control)?,
            control: control.clone(),
            session: None,
            author: None,
        })
    }
    fn headers(&mut self) -> Result<HeaderMap> {
        if self.session.is_none() {
            self.session = Some(Session::load(&self.control)?);
        }
        self.control.check()?;
        Ok(self.session.as_ref().unwrap().headers.clone())
    }
    fn call(&mut self, path: &str, body: &Value, mutation: bool) -> Result<Value> {
        let headers = self.headers()?;
        let response = self
            .http
            .post(
                Origin::NewbeeCreator,
                &format!("{API}{path}"),
                headers,
                body,
                mutation,
            )
            .map_err(|mut error| {
                if matches!(error.details["http_status"].as_u64(), Some(401 | 403)) {
                    error.message = auth().message;
                    error.exit_code = 3;
                }
                error
            })?;
        envelope(response, mutation)
    }
    fn me(&mut self) -> Result<Value> {
        let raw = self.call("/me", &json!({}), false)?;
        let uid = positive(&raw["uid"])?;
        self.author = Some(uid);
        // Token name/prefix/creation/expiry metadata is intentionally absent.
        let mut result = json!({"uid":uid,"username":raw.get("username"),"credential_source":self.session.as_ref().unwrap().source,"protocol_compatibility":PROTOCOL_VERSION});
        for key in ["modules", "permissions"] {
            if let Some(value) = raw.get(key) {
                result[key] = module_permissions(value)?;
            }
        }
        Ok(result)
    }
    fn author(&mut self) -> Result<u64> {
        if self.author.is_none() {
            self.me()?;
        }
        Ok(self.author.unwrap())
    }
    fn game_version(&self, id: NonZeroU64) -> Result<()> {
        let metadata = self.http.get(
            Origin::NewbeeMetadata,
            "/modconfig.json",
            &[],
            HeaderMap::new(),
        )?;
        let rows = metadata["game_version"]
            .as_array()
            .ok_or_else(|| data("official game version metadata is incomplete"))?;
        if rows
            .iter()
            .any(|row| positive(&row["id"]).is_ok_and(|value| value == id.get()))
        {
            Ok(())
        } else {
            Err(input::invalid(
                "$.game_version_id",
                "game version is absent from current platform metadata; inspect publish newbee plugin game-versions",
            ))
        }
    }
    pub fn upload_media(&mut self, request: &MediaUpload) -> Result<Value> {
        let (mut file, sha256) = storage::locked_sha256(&request.path, u64::MAX, &self.control)?;
        let control = self.control.clone();
        let result = (|| {
            let author = self.author()?;
            let size = file.metadata()?.len();
            let content_type = media_type(&request.path)?;
            let identity =
                json!({"author":author,"resource":request.resource,"sha256":sha256,"size":size});
            let hash = storage::sha256(&storage::identity_json(&identity)?);
            let mut mutation = crate::mutation::Mutation::open(
                &self.home,
                crate::provider::Provider::Newbee,
                &format!("official-{}-media", request.resource.key()),
                u64::from_str_radix(&hash[..16], 16).unwrap().max(1),
                "upload",
                &identity,
                &self.control,
            )?;
            if let Some(value) = mutation
                .evidence()
                .filter(|value| value["verification"] == "upload_response")
            {
                let mut value = value.clone();
                value["reused"] = json!(true);
                return Ok(value);
            }
            if mutation.pending() {
                return Err(mutation.pending_error("the original official media upload is uncertain; retain its receipt and do not upload it again"));
            }
            let filename = request
                .path
                .file_name()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| input::invalid("$.file", "media filename must be UTF-8"))?;
            let headers = self.headers()?;
            let stream = file.try_clone()?;
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::Start(0))?;
            mutation.prepare(&self.control)?;
            let raw = self
                .http
                .multipart_file(
                    Origin::NewbeeCreator,
                    &format!("{API}/{}/upload_media", request.resource.route()),
                    headers,
                    &[],
                    filename,
                    content_type,
                    stream,
                )
                .and_then(|value| envelope(value, true))
                .map_err(|error| mutation.failure(error))?;
            let media = raw["media_url"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    mutation.pending_error("official media upload omitted its reusable media URL")
                })?;
            let url = reqwest::Url::parse(media).map_err(|_| {
                mutation.pending_error("official media upload returned an invalid reusable URL")
            })?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
                || output::public(json!(media), &[]) != json!(media)
            {
                return Err(mutation.pending_error("official media upload returned a private capability rather than a reusable media URL"));
            }
            let result = json!({"uploaded":true,"resource":request.resource,"media_url":media,"is_image":content_type.starts_with("image/"),"sha256":sha256,"size":size,"operation_id":mutation.id(),"verification":"upload_response","remote_byte_verification":"not_available","reused":false});
            mutation.verified_with(result.clone()).map_err(|_| {
                mutation.pending_error(
                    "official media was accepted; finish saving its original response",
                )
            })?;
            Ok(result)
        })();
        storage::finish_file(file, result, &control)
    }
    pub fn query(&mut self, query: &Query) -> Result<Value> {
        let result = match query {
            Query::Doctor => self.me()?,
            Query::List(request) => {
                let mut body = json!({"keyword":request.keyword,"pagenum":request.page});
                if let Some(size) = request.size {
                    body["pagesize"] = json!(size);
                }
                if let Some(paid) = request.paid {
                    body["is_paid"] = json!(paid);
                }
                let raw = self.call(
                    &format!("/{}/publish_list", request.resource.route()),
                    &body,
                    false,
                )?;
                selected_page(raw, request.page)
            }
            Query::Detail { resource, id } => self.call(
                &format!("/{}/publish_detail", resource.route()),
                &json!({"id":id}),
                false,
            )?,
            Query::PluginVersions { id, page, size } => {
                if !(10..=2000).contains(size) {
                    return Err(ToolkitError::arguments(
                        "official plugin version size must be 10..2000",
                    ));
                }
                let raw = self.call(
                    "/mod_file/mod_file_list",
                    &json!({"mod_id":id,"pagenum":page,"pagesize":size}),
                    false,
                )?;
                selected_page(raw, *page)
            }
            Query::PluginChangelog { file_id } => self.call(
                "/mod_file/get_changelog",
                &json!({"file_id":file_id}),
                false,
            )?,
            Query::WaCategories { game_version } => {
                self.game_version(*game_version)?;
                self.call("/wa/category", &json!({"game_version":game_version}), false)?
            }
            Query::Backups => self.call("/cloudbackup/list", &json!({}), false)?,
            Query::Backup { id } => self.call("/cloudbackup/detail", &json!({"id":id}), false)?,
            Query::Comments {
                resource,
                id,
                page,
                status,
            } => {
                let reply_status = match status {
                    ReplyStatus::All => 0,
                    ReplyStatus::Unreplied => 1,
                    ReplyStatus::Replied => 2,
                };
                let raw=self.call("/content_review/list",&json!({"content_type":resource.review_type(),"content_id":id,"reply_status":reply_status,"page":page}),false)?;
                selected_page(raw, *page)
            }
            Query::Reference {
                resource,
                id,
                link,
                game_version,
            } => {
                if !matches!(resource, Resource::Plugin) && game_version.is_some() {
                    return Err(ToolkitError::arguments(
                        "game version applies only to plugin references",
                    ));
                }
                if let Some(game_version) = game_version {
                    self.game_version(*game_version)?;
                }
                let raw = self.call(
                    &format!("/{}/publish_detail", resource.route()),
                    &json!({"id":id}),
                    false,
                )?;
                if positive(&raw["id"])? != id.get() {
                    return Err(data("reference detail differs from its requested identity"));
                }
                let title = raw[if matches!(resource, Resource::Config) {
                    "title"
                } else {
                    "name"
                }]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| data("reference detail has no title"))?;
                let shape = if *link { 2 } else { 1 };
                let code = match resource {
                    Resource::Plugin => format!(
                        "{{[plugin=-={id}=-={title}=-={shape}=-={}]}}",
                        game_version.map(NonZeroU64::get).unwrap_or(0)
                    ),
                    Resource::Wa => format!("{{[wa=-={id}=-={title}=-={shape}]}}"),
                    Resource::Config => format!("{{[shareConfig=-={id}=-={title}=-={shape}]}}"),
                };
                json!({"id":id,"resource":resource.key(),"name":title,"shape":if *link{"link"}else{"card"},"reference_code":code})
            }
        };
        Ok(output::public(result, &[]))
    }
}
fn positive(value: &Value) -> Result<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .filter(|value| *value > 0)
        .ok_or_else(|| data("official creator response omitted a positive identity"))
}
fn data(message: &str) -> ToolkitError {
    ToolkitError::new("publish.ncc_data", message, 5)
}
fn media_type(path: &Path) -> Result<&'static str> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        "bmp" => Ok("image/bmp"),
        "ico" => Ok("image/x-icon"),
        "mp4" => Ok("video/mp4"),
        _ => Err(input::invalid(
            "$.file",
            "official media supports PNG/JPG/JPEG/GIF/WEBP/BMP/ICO or MP4",
        )),
    }
}
fn module_permissions(value: &Value) -> Result<Value> {
    match value {
        Value::Array(values) => Ok(json!(
            values.iter().filter_map(Value::as_str).collect::<Vec<_>>()
        )),
        Value::Object(values) => Ok(Value::Object(
            values
                .iter()
                .filter(|(_, value)| value.is_boolean())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )),
        _ => Err(data("official creator module permissions are malformed")),
    }
}
fn selected_page(raw: Value, page: NonZeroU64) -> Value {
    // Preserve the API's own pagination. A selected page is not a complete
    // author enumeration; missing metadata must not become an invented total.
    json!({"page":page,"coverage":"unknown","platform_result":raw})
}
fn envelope(value: Value, mutation: bool) -> Result<Value> {
    let code = value.get("code").and_then(Value::as_i64);
    if code == Some(1) {
        return value
            .get("data")
            .cloned()
            .ok_or_else(|| data("official creator response omitted data"));
    }
    let known = [
        "cli_version_outdated",
        "quota_exceeded",
        "cli_disabled",
        "permission_denied",
        "not_found",
        "invalid_param",
        "api_error",
    ];
    let kind = value["data"]["error"]
        .as_str()
        .filter(|value| known.contains(value))
        .unwrap_or("api_error");
    let exit = if code == Some(-3)
        || matches!(
            kind,
            "cli_version_outdated" | "cli_disabled" | "permission_denied"
        ) {
        3
    } else {
        5
    };
    Err(ToolkitError::new(if code.is_some(){"publish.ncc_business"}else{"publish.ncc_protocol"},if code==Some(-3){"official creator token is invalid; sign in locally or replace NCC_TOKEN locally"}else{"official creator request was rejected or incomplete; retain the original request and inspect the declared correction"},exit)
        .with_details(json!({"business_code":code,"api_error":kind,"submission":if !mutation{"not_applicable"}else if code.is_some(){"rejected"}else{"uncertain"},"automatic_retry":false,"correction":match kind{"cli_version_outdated"=>"update Lychee CLI protocol support through its normal delivery workflow","quota_exceeded"|"cli_disabled"=>"stop automated submissions and use the official creator web interface",_=>"inspect this command schema and official session doctor"}})))
}
#[cfg(test)]
mod tests;
