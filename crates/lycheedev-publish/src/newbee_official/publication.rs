//! Normalized official author forms. Desktop Creator forms are a distinct API.
use super::*;
use crate::{input::invalid, mutation::Mutation, provider::Provider};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
};
mod forms;
mod money;
use forms::*;

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Text {
    Inline(String),
    File(TextFile),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextFile {
    file: String,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Visibility {
    Public,
    Private,
}
impl Visibility {
    fn share(self) -> u8 {
        match self {
            Self::Public => 1,
            Self::Private => 2,
        }
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum ContentOrigin {
    #[default]
    Original,
    Repost,
    Localized,
}
impl ContentOrigin {
    fn code(self) -> u8 {
        match self {
            Self::Original => 1,
            Self::Repost => 2,
            Self::Localized => 3,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginCreate {
    name: String,
    description: Text,
    categories: Vec<NonZeroU64>,
    logo: String,
    screenshots: Vec<String>,
    visibility: Visibility,
    intro: Option<String>,
    home_page: Option<String>,
    home_page_link: Option<String>,
    #[serde(default)]
    origin: ContentOrigin,
    subscribe_level: Option<u64>,
    #[serde(default)]
    link_channel: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaCreate {
    name: String,
    description: Text,
    wa_str: Text,
    game_version_id: NonZeroU64,
    categories: Vec<NonZeroU64>,
    thumbnail: String,
    visibility: Visibility,
    intro: Option<String>,
    #[serde(default)]
    images: Vec<String>,
    wa_log: Option<Text>,
    price_yuan: Option<String>,
    #[serde(default)]
    subscribe_level: u64,
    #[serde(default)]
    time_range: String,
    #[serde(default)]
    origin: ContentOrigin,
    #[serde(default)]
    link_channel: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigCreate {
    backup_id: NonZeroU64,
    title: String,
    content: Text,
    visibility: Visibility,
    intro: Option<String>,
    role_id: Option<NonZeroU64>,
    #[serde(default)]
    images: Vec<String>,
    price_yuan: Option<String>,
    #[serde(default)]
    subscribe_level: u64,
    #[serde(default)]
    time_range: String,
    #[serde(default)]
    origin: ContentOrigin,
    #[serde(default)]
    link_channel: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginEdit {
    id: NonZeroU64,
    name: Option<String>,
    description: Option<Text>,
    intro: Option<String>,
    categories: Option<Vec<NonZeroU64>>,
    logo: Option<String>,
    screenshots: Option<Vec<String>>,
    visibility: Option<Visibility>,
    subscribe_level: Option<u64>,
    link_channel: Option<bool>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WaEdit {
    id: NonZeroU64,
    name: Option<String>,
    description: Option<Text>,
    intro: Option<String>,
    categories: Option<Vec<NonZeroU64>>,
    thumbnail: Option<String>,
    images: Option<Vec<String>>,
    visibility: Option<Visibility>,
    price_yuan: Option<String>,
    subscribe_level: Option<u64>,
    link_channel: Option<bool>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "schema")]
enum Document {
    #[serde(rename = "lycheedev.publish.v1.newbee.official.plugin.create")]
    PluginCreate(PluginCreate),
    #[serde(rename = "lycheedev.publish.v1.newbee.official.wa.create")]
    WaCreate(WaCreate),
    #[serde(rename = "lycheedev.publish.v1.newbee.official.config.create")]
    ConfigCreate(ConfigCreate),
    #[serde(rename = "lycheedev.publish.v1.newbee.official.plugin.edit")]
    PluginEdit(PluginEdit),
    #[serde(rename = "lycheedev.publish.v1.newbee.official.wa.edit")]
    WaEdit(WaEdit),
}
pub struct Publication {
    doc: Document,
    cwd: PathBuf,
}
impl Publication {
    pub fn parse(resource: Resource, create: bool, bytes: &[u8], cwd: &Path) -> Result<Self> {
        let raw = input::document(bytes)?;
        if raw.values().any(Value::is_null) {
            return Err(invalid(
                "$",
                "omit optional fields to preserve/default them; use empty text or arrays for supported clears, not null",
            ));
        }
        let doc: Document = serde_json::from_value(json!(raw)).map_err(|_| {
            invalid(
                "$",
                "official author input differs from this command's typed schema",
            )
        })?;
        if doc.resource() != resource || doc.create() != create {
            return Err(invalid(
                "$.schema",
                "select the schema for this official resource and action",
            ));
        }
        doc.admit()?;
        Ok(Self {
            doc,
            cwd: cwd.to_owned(),
        })
    }
    pub fn schema(resource: Resource, create: bool) -> Value {
        forms::schema(resource, create)
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let selected = self.select(control)?;
        let result = json!({"provider":"newbee_official","resource":self.doc.resource(),"action":if self.doc.create(){"create"}else{"edit"},"id":self.doc.id(),"input_sha256":selected.hash()?,"files":selected.summaries,"credential_reads":false,"author_writes":0,"remote_validation":"not_run"});
        selected.finish(Ok(result), control)
    }
    fn select(&self, control: &OperationControl) -> Result<Selected> {
        let mut selected = Selected {
            identity: serde_json::to_value(&self.doc)?,
            ..Default::default()
        };
        match &self.doc {
            Document::PluginCreate(doc) => {
                selected.text("description", &doc.description, &self.cwd, control)?;
                selected.media(&doc.logo, &self.cwd, control)?;
                for value in &doc.screenshots {
                    selected.media(value, &self.cwd, control)?;
                }
            }
            Document::WaCreate(doc) => {
                selected.text("description", &doc.description, &self.cwd, control)?;
                selected.text("wa_str", &doc.wa_str, &self.cwd, control)?;
                if let Some(value) = &doc.wa_log {
                    selected.text("wa_log", value, &self.cwd, control)?;
                }
                selected.media(&doc.thumbnail, &self.cwd, control)?;
                for value in &doc.images {
                    selected.media(value, &self.cwd, control)?;
                }
            }
            Document::ConfigCreate(doc) => {
                selected.text("content", &doc.content, &self.cwd, control)?;
                for value in &doc.images {
                    selected.media(value, &self.cwd, control)?;
                }
            }
            Document::PluginEdit(doc) => {
                if let Some(value) = &doc.description {
                    selected.text("description", value, &self.cwd, control)?;
                }
                if let Some(value) = &doc.logo {
                    selected.media(value, &self.cwd, control)?;
                }
                for value in doc.screenshots.iter().flatten() {
                    selected.media(value, &self.cwd, control)?;
                }
            }
            Document::WaEdit(doc) => {
                if let Some(value) = &doc.description {
                    selected.text("description", value, &self.cwd, control)?;
                }
                if let Some(value) = &doc.thumbnail {
                    selected.media(value, &self.cwd, control)?;
                }
                for value in doc.images.iter().flatten() {
                    selected.media(value, &self.cwd, control)?;
                }
            }
        }
        control.check()?;
        Ok(selected)
    }
}
impl Document {
    fn resource(&self) -> Resource {
        match self {
            Self::PluginCreate(_) | Self::PluginEdit(_) => Resource::Plugin,
            Self::WaCreate(_) | Self::WaEdit(_) => Resource::Wa,
            Self::ConfigCreate(_) => Resource::Config,
        }
    }
    fn create(&self) -> bool {
        matches!(
            self,
            Self::PluginCreate(_) | Self::WaCreate(_) | Self::ConfigCreate(_)
        )
    }
    fn id(&self) -> Option<NonZeroU64> {
        match self {
            Self::PluginEdit(d) => Some(d.id),
            Self::WaEdit(d) => Some(d.id),
            _ => None,
        }
    }
    fn admit(&self) -> Result<()> {
        match self {
            Self::PluginCreate(d) => {
                nonblank("name", &d.name)?;
                ids(&d.categories, false)?;
                if d.screenshots.is_empty() {
                    return Err(invalid("$.screenshots", "provide at least one screenshot"));
                }
                if let Some(url) = &d.home_page_link {
                    public_url(url)?;
                }
            }
            Self::WaCreate(d) => {
                nonblank("name", &d.name)?;
                ids(&d.categories, true)?;
                money::yuan(d.price_yuan.as_deref().unwrap_or("0"))?;
            }
            Self::ConfigCreate(d) => {
                nonblank("title", &d.title)?;
                money::yuan(d.price_yuan.as_deref().unwrap_or("0"))?;
            }
            Self::PluginEdit(d) => {
                if let Some(value) = &d.name {
                    nonblank("name", value)?;
                }
                if let Some(value) = &d.categories {
                    ids(value, false)?;
                }
                if d.name.is_none()
                    && d.description.is_none()
                    && d.intro.is_none()
                    && d.categories.is_none()
                    && d.logo.is_none()
                    && d.screenshots.is_none()
                    && d.visibility.is_none()
                    && d.subscribe_level.is_none()
                    && d.link_channel.is_none()
                {
                    return Err(invalid("$", "select at least one plugin edit field"));
                }
            }
            Self::WaEdit(d) => {
                if let Some(value) = &d.name {
                    nonblank("name", value)?;
                }
                if let Some(value) = &d.categories {
                    ids(value, true)?;
                }
                if let Some(value) = &d.price_yuan {
                    money::yuan(value)?;
                }
                if d.name.is_none()
                    && d.description.is_none()
                    && d.intro.is_none()
                    && d.categories.is_none()
                    && d.thumbnail.is_none()
                    && d.images.is_none()
                    && d.visibility.is_none()
                    && d.price_yuan.is_none()
                    && d.subscribe_level.is_none()
                    && d.link_channel.is_none()
                {
                    return Err(invalid("$", "select at least one WA edit field"));
                }
            }
        }
        Ok(())
    }
}
fn nonblank(key: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(invalid(&format!("$.{key}"), "text must not be blank"))
    } else {
        Ok(())
    }
}
fn ids(values: &[NonZeroU64], wa: bool) -> Result<()> {
    if values.is_empty()
        || (wa && values.len() > 5)
        || values.iter().collect::<BTreeSet<_>>().len() != values.len()
    {
        return Err(invalid(
            "$.categories",
            "provide unique positive category IDs, with one to five for WA",
        ));
    }
    Ok(())
}
fn public_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).map_err(|_| {
        invalid(
            "$.media",
            "provide a reusable public HTTP(S) media URL or local media file",
        )
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || output::public(json!(value), &[]) != json!(value)
    {
        return Err(invalid(
            "$.media",
            "media must be a reusable public HTTP(S) URL without credentials or signed capabilities",
        ));
    }
    Ok(())
}
#[derive(Default)]
struct Selected {
    identity: Value,
    files: Vec<File>,
    texts: BTreeMap<&'static str, String>,
    media: BTreeMap<String, PathBuf>,
    summaries: Vec<Value>,
}
impl Selected {
    fn text(
        &mut self,
        key: &'static str,
        text: &Text,
        cwd: &Path,
        control: &OperationControl,
    ) -> Result<()> {
        let value = match text {
            Text::Inline(value) => {
                if key != "wa_log" {
                    nonblank(key, value)?;
                }
                value.clone()
            }
            Text::File(value) => {
                let (file, hash, text) = input::locked_text(&cwd.join(&value.file), control)?;
                self.files.push(file);
                self.identity[key] = json!({"sha256":hash});
                self.summaries
                    .push(json!({"field":key,"sha256":hash,"length":text.chars().count()}));
                text
            }
        };
        self.texts.insert(key, value);
        Ok(())
    }
    fn media(&mut self, value: &str, cwd: &Path, control: &OperationControl) -> Result<()> {
        if value.to_ascii_lowercase().starts_with("http://")
            || value.to_ascii_lowercase().starts_with("https://")
        {
            return public_url(value);
        }
        nonblank("media", value)?;
        if self.media.contains_key(value) {
            return Ok(());
        }
        let path = cwd.join(value);
        media_type(&path)?;
        let (file, hash) = storage::locked_sha256(&path, u64::MAX, control)?;
        self.summaries.push(json!({"field":"media","sha256":hash,"size":file.metadata()?.len(),"content_type":media_type(&path)?}));
        self.files.push(file);
        self.identity["selected_media"][value] = json!(hash);
        self.media.insert(value.to_owned(), path);
        Ok(())
    }
    fn hash(&self) -> Result<String> {
        Ok(storage::sha256(&storage::identity_json(&self.identity)?))
    }
    fn finish(self, mut result: Result<Value>, control: &OperationControl) -> Result<Value> {
        for file in self.files {
            result = storage::finish_file(file, result, control);
        }
        result
    }
}

impl Official {
    pub fn publish(
        &mut self,
        request: &Publication,
        recover_id: Option<NonZeroU64>,
    ) -> Result<Value> {
        let selected = request.select(&self.control)?;
        let result = self.publish_selected(request, &selected, recover_id);
        selected.finish(result, &self.control)
    }
    fn publish_selected(
        &mut self,
        request: &Publication,
        selected: &Selected,
        recover_id: Option<NonZeroU64>,
    ) -> Result<Value> {
        let resource = request.doc.resource();
        let create = request.doc.create();
        let author = self.author()?;
        let identity = json!({"author":author,"request":selected.hash()?});
        let hash = storage::sha256(&storage::identity_json(&identity)?);
        let target = request
            .doc
            .id()
            .map(NonZeroU64::get)
            .unwrap_or_else(|| u64::from_str_radix(&hash[..16], 16).unwrap().max(1));
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            &format!(
                "official-{}-{}",
                resource.key(),
                if create { "create" } else { "edit" }
            ),
            target,
            if create { "create" } else { "edit" },
            &identity,
            &self.control,
        )?;
        let recovered = mutation.pending();
        if !create && recover_id.is_some() {
            return Err(ToolkitError::arguments(
                "--id is only for original creation recovery; edits select their id in the input",
            ));
        }
        if create {
            if let Some(value) = mutation
                .evidence()
                .filter(|value| value["verification"] == "metadata_fields")
            {
                if recover_id.is_some_and(|id| json!(id) != value["id"]) {
                    return Err(invalid(
                        "$.id",
                        "recovery ID differs from the original creation receipt",
                    ));
                }
                let mut result = value.clone();
                if recovered {
                    mutation.verified()?;
                }
                result["reused"] = json!(true);
                result["recovered"] = json!(recovered);
                result["metadata_writes"] = json!(0);
                return Ok(result);
            }
            if !recovered && recover_id.is_some() {
                return Err(ToolkitError::arguments(
                    "--id selects a pending original creation; omit it for a new creation",
                ));
            }
        }
        let mut current = if let Some(id) = request.doc.id() {
            Some(
                self.publication_state(resource, id, false)
                    .map_err(|error| {
                        if recovered {
                            mutation.readback_error(
                                "original official author metadata readback is pending",
                                error,
                            )
                        } else {
                            error
                        }
                    })?,
            )
        } else {
            None
        };
        if !create {
            if let Some(value) = mutation
                .evidence()
                .filter(|value| value["verification"] == "metadata_fields")
            {
                let matches = value["observed_sha256"] == observed_hash(current.as_ref().unwrap())?;
                if recovered || matches {
                    let mut result = value.clone();
                    if recovered {
                        mutation.verified()?;
                    }
                    result["reused"] = json!(true);
                    result["current_state_matches"] = json!(matches);
                    result["metadata_writes"] = json!(0);
                    return Ok(result);
                }
            }
        }
        let id = if recovered {
            let saved = mutation
                .evidence()
                .and_then(|value| value["created_id"].as_u64())
                .and_then(NonZeroU64::new);
            if saved.is_some_and(|saved| recover_id.is_some_and(|given| given != saved)) {
                return Err(invalid(
                    "$.id",
                    "recovery ID differs from the acknowledged original creation",
                ));
            }
            request.doc.id().or(saved).or(recover_id).ok_or_else(||mutation.pending_error("original official creation outcome is uncertain; inspect the author list and supply its explicit --id with the original input"))?
        } else {
            let body = self.publication_body(&request.doc, selected, current.as_ref())?;
            let ignore_intro = body.get("intro").is_none()
                || matches!(
                    &request.doc,
                    Document::PluginCreate(_) | Document::PluginEdit(_)
                ) && body["intro"] == "";
            let expected = expected_state(resource, &body)?;
            if !create
                && state_hash(current.as_ref().unwrap(), ignore_intro)?
                    == state_hash(&expected, ignore_intro)?
                && current.as_ref().unwrap()["share_state"] == body["share_state"]
                && body["link_to_channel"] != true
                && !ignore_intro
            {
                return Ok(
                    json!({"id":request.doc.id(),"resource":resource,"changed":false,"verification":"metadata_fields","metadata_writes":0}),
                );
            }
            mutation.prepare(&self.control)?;
            let mut evidence = json!({"expected_sha256":state_hash(&expected,ignore_intro)?,"ignore_intro":ignore_intro,"requested_visibility":body.get("share_state").or_else(||body.get("sharing"))});
            mutation.remember(evidence.clone())?;
            let endpoint = match &request.doc {
                Document::PluginCreate(_) => "/mod/create",
                Document::WaCreate(_) => "/wa/publish",
                Document::ConfigCreate(_) => "/share_config/release",
                Document::PluginEdit(_) => "/mod/edit",
                Document::WaEdit(_) => "/wa/update",
            };
            let response = self
                .call(endpoint, &body, true)
                .map_err(|error| mutation.failure(error))?;
            let id = if create {
                let key = match resource {
                    Resource::Plugin => "mod_id",
                    Resource::Wa => "id",
                    Resource::Config => "config_id",
                };
                NonZeroU64::new(positive(&response[key]).map_err(|_|mutation.pending_error("official creation was accepted without its positive resource ID; recover using the original input and explicit --id"))?).unwrap()
            } else {
                request.doc.id().unwrap()
            };
            if create {
                evidence["created_id"] = json!(id);
                mutation.remember(evidence).map_err(|_| {
                    mutation.pending_error(
                        "official creation was accepted; finish saving its original resource ID",
                    )
                })?;
            }
            mutation.acknowledged().map_err(|_|mutation.pending_error("official author write was accepted; finish saving its original acknowledgement"))?;
            if !create {
                current = Some(
                    self.publication_state(resource, id, false)
                        .map_err(|error| {
                            mutation.readback_error(
                                "official author metadata readback is pending",
                                error,
                            )
                        })?,
                );
            }
            id
        };
        if create {
            current = Some(
                self.publication_state(resource, id, true)
                    .map_err(|error| {
                        mutation.readback_error(
                            "created official author resource readback is pending",
                            error,
                        )
                    })?,
            );
        }
        let current = current.unwrap();
        let evidence = mutation.evidence().ok_or_else(|| {
            mutation.pending_error("original official author form hash is missing")
        })?;
        let ignore_intro = evidence["ignore_intro"]
            .as_bool()
            .ok_or_else(|| mutation.pending_error("original official summary policy is missing"))?;
        if evidence["expected_sha256"] != state_hash(&current, ignore_intro)? {
            return Err(mutation.pending_error("official author metadata differs from the original requested and preserved form; the write was not repeated"));
        }
        let result = json!({"id":id,"resource":resource,"operation_id":mutation.id(),"verification":"metadata_fields","observed_sha256":observed_hash(&current)?,"intro_verification":if ignore_intro{"server_derived_not_available"}else{"readback"},"visibility_verification":if current["share_state"]==evidence["requested_visibility"]{"readback"}else{"pending_or_unavailable"},"observed_visibility":current["share_state"],"channel_delivery_verification":"not_available","review_status":"not_checked","recovered":recovered,"reused":false,"metadata_writes":if recovered{0}else{1}});
        let mut result = result;
        if create {
            result["creation_fields_without_readback"] = json!(match resource {
                Resource::Plugin => vec!["content_origin", "home_page", "home_page_link"],
                Resource::Wa => vec!["content_origin", "game_version_id", "wa_log"],
                Resource::Config => vec![
                    "content_origin",
                    "time_range",
                    "ignored_unknown_mods",
                    "ignored_materials",
                    "ignored_fronts",
                    "selected_eml"
                ],
            });
        }
        mutation.verified_with(result.clone()).map_err(|_| {
            mutation.pending_error(
                "official author metadata matched; finish saving its original result",
            )
        })?;
        Ok(result)
    }
    fn resolve_media(
        &mut self,
        resource: Resource,
        value: &str,
        selected: &Selected,
    ) -> Result<String> {
        if let Some(path) = selected.media.get(value) {
            let result = self.upload_media(&MediaUpload {
                resource,
                path: path.clone(),
            })?;
            Ok(result["media_url"]
                .as_str()
                .ok_or_else(|| data("official media result omitted URL"))?
                .to_owned())
        } else {
            public_url(value)?;
            Ok(value.to_owned())
        }
    }
    fn resolve_media_list(
        &mut self,
        resource: Resource,
        values: &[String],
        selected: &Selected,
    ) -> Result<Vec<String>> {
        values
            .iter()
            .map(|value| self.resolve_media(resource, value, selected))
            .collect()
    }
}
fn state_hash(value: &Value, ignore_intro: bool) -> Result<String> {
    let mut value = value.clone();
    let map = value
        .as_object_mut()
        .ok_or_else(|| data("official author metadata is not an object"))?;
    map.remove("share_state");
    if ignore_intro {
        map.remove("intro");
    }
    Ok(storage::sha256(&storage::identity_json(&value)?))
}
fn observed_hash(value: &Value) -> Result<String> {
    Ok(storage::sha256(&storage::identity_json(value)?))
}
#[cfg(test)]
mod tests;
