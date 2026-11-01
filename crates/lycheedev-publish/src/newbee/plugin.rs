use super::*;
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::Mutation,
    provider::Provider,
};
use lycheedev_core::storage;
use std::{
    collections::BTreeSet,
    io::{Seek, SeekFrom},
};

pub struct PluginEdit {
    doc: Value,
    cwd: PathBuf,
}
pub struct PluginUpdate {
    doc: Value,
    cwd: PathBuf,
}
impl PluginEdit {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        let doc = admit("edit", bytes, cwd)?;
        embedded_relationships(Resource::Plugin, doc["id"].as_u64().unwrap(), &doc)?;
        Ok(Self {
            doc,
            cwd: cwd.to_owned(),
        })
    }
    pub fn schema() -> Value {
        schema("edit")
    }
}
impl PluginUpdate {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        Ok(Self {
            doc: admit("update", bytes, cwd)?,
            cwd: cwd.to_owned(),
        })
    }
    pub fn schema() -> Value {
        schema("update")
    }
}
fn admit(action: &str, bytes: &[u8], cwd: &Path) -> Result<Value> {
    let doc = document(bytes)?;
    input_shape(Provider::Newbee, "plugin", action)?.admit(&doc)?;
    let doc = Value::Object(doc);
    let id_key = if action == "update" { "mod_id" } else { "id" };
    if !doc[id_key].as_u64().is_some_and(|n| n > 0) {
        return Err(invalid(
            &format!("$.{id_key}"),
            "resource identity must be positive",
        ));
    }
    admit_metadata(&doc, cwd)?;
    if let Some(file) = doc.get("file") {
        let path = cwd.join(file.as_str().unwrap());
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "zip" | "rar" | "7z")
            || !path.is_file()
            || path.metadata()?.len() > 300 * 1024 * 1024
        {
            return Err(invalid(
                "$.file",
                "plugin archive must be an existing .zip/.rar/.7z file no larger than 300 MiB",
            ));
        }
    }
    Ok(doc)
}
pub(super) fn admit_metadata(doc: &Value, cwd: &Path) -> Result<()> {
    if doc
        .get("content_format")
        .is_some_and(|value| value != 1 && value != 2)
    {
        return Err(invalid(
            "$.content_format",
            "content_format is 1 for HTML/rich text or 2 for Markdown",
        ));
    }
    if doc["public"] == true && doc["submit_for_review"] != true {
        return Err(invalid(
            "$.submit_for_review",
            "public=true requires submit_for_review=true",
        ));
    }
    if doc["submit_for_review"] == true && doc["public"] != true {
        return Err(invalid(
            "$.public",
            "submit_for_review=true requires public=true",
        ));
    }
    for field in ["mod_categories", "category_id_list"] {
        if let Some(items) = doc.get(field).and_then(Value::as_array) {
            if items
                .iter()
                .any(|item| !item.as_u64().is_some_and(|n| n > 0))
            {
                return Err(invalid(
                    &format!("$.{field}"),
                    "selected IDs must be positive integers",
                ));
            }
            let ids = items
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect::<BTreeSet<_>>();
            if ids.len() != items.len() {
                return Err(invalid(
                    &format!("$.{field}"),
                    "selected IDs must be unique",
                ));
            }
        }
    }
    for field in [
        "screenshots",
        "picture_urls",
        "images",
        "screenshot_files",
        "picture_files",
        "image_files",
        "game_version_list",
        "ignored_unknown_mods",
        "ignored_materials",
        "ignored_fronts",
        "wa_str_titles",
    ] {
        if let Some(items) = doc.get(field).and_then(Value::as_array) {
            for item in items {
                let text = item
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        invalid(&format!("$.{field}"), "items must be nonempty strings")
                    })?;
                if field.ends_with("_files") && !cwd.join(text).is_file() {
                    return Err(invalid(
                        &format!("$.{field}"),
                        "local image must be an existing regular file",
                    ));
                }
            }
        }
    }
    for field in ["logo_file", "thumbnail_file"] {
        if let Some(file) = doc.get(field) {
            if !file
                .as_str()
                .is_some_and(|s| !s.is_empty() && cwd.join(s).is_file())
            {
                return Err(invalid(
                    &format!("$.{field}"),
                    "local image must be an existing regular file",
                ));
            }
        }
    }
    Ok(())
}
fn schema(action: &str) -> Value {
    let mut schema = input_shape(Provider::Newbee, "plugin", action)
        .unwrap()
        .schema();
    schema.as_object_mut().unwrap().remove("x-validation-scope");
    let props = &mut schema["properties"];
    for field in ["id", "mod_id"] {
        if props.get(field).is_some() {
            props[field]["minimum"] = json!(1);
        }
    }
    for field in ["mod_categories"] {
        if props.get(field).is_some() {
            props[field]["items"] = json!({"type":"integer","minimum":1});
            props[field]["uniqueItems"] = json!(true);
        }
    }
    for field in ["screenshots", "screenshot_files", "game_version_list"] {
        if props.get(field).is_some() {
            props[field]["items"] = json!({"type":"string","minLength":1});
        }
    }
    relationship_schemas(props);
    if action == "edit" {
        schema["allOf"] = review_schema();
    }
    metadata_schema(&mut schema);
    schema
}
pub(super) fn metadata_schema(schema: &mut Value) {
    let props = &mut schema["properties"];
    for (field, description) in [
        (
            "id",
            "Exact existing author resource ID returned by list/get.",
        ),
        (
            "mod_id",
            "Existing plugin ID; uploading a version never changes visibility.",
        ),
        (
            "module_id",
            "Existing WA ID for the NewBee share-code service.",
        ),
        (
            "file_id",
            "Immutable plugin version file ID, distinct from its parent plugin ID.",
        ),
        (
            "cloud_id",
            "Cloud backup ID from config backups/backup-get. Changing it requires all component and role selections.",
        ),
        (
            "game_version_id",
            "Numeric NewBee game identity from plugin game-versions; distinct from build strings and TOC Interface numbers.",
        ),
        (
            "game_version_list",
            "Exact build strings from plugin game-versions; version archives must bind every selected build.",
        ),
        (
            "mod_categories",
            "At most five current IDs from plugin categories.",
        ),
        (
            "category_id_list",
            "Current WA category IDs for the selected numeric game_version_id.",
        ),
        (
            "content_format",
            "1 = HTML/rich text; 2 = Markdown. Descriptions are public author text, not serialized WTF data.",
        ),
        (
            "content_origin",
            "Select the current value from options content-origins.",
        ),
        (
            "subscribe_plan_level",
            "0 disables subscription; nonzero values must occur in options subscribe-plans.",
        ),
        (
            "price",
            "Nonnegative platform price. Zero clears time_range; preserve the current platform units when editing.",
        ),
        (
            "time_range",
            "Current value from options time-ranges; zero price clears the duration.",
        ),
        (
            "public",
            "Explicit visibility/review intent. true requires submit_for_review=true. Plugin creation stays private until a first version is uploaded and public review is explicitly requested.",
        ),
        (
            "submit_for_review",
            "Required true with public=true; omitted on edits preserves visibility.",
        ),
        (
            "link_to_channel",
            "Explicit boolean; a private resource always submits false.",
        ),
        (
            "file",
            "Existing local file, resolved from the invocation directory. Plugin archives accept ZIP/RAR/7z up to 300 MiB.",
        ),
        (
            "wa_str",
            "Opaque WA import content supplied through the JSON file or stdin. Results summarize it; never put it in argv.",
        ),
        (
            "wa_str_titles",
            "Nonempty titles required for collection mode; omitted on a version update preserves current titles.",
        ),
        (
            "wa_log",
            "Complete public version log; null clears it only on changelog edit.",
        ),
        (
            "changelog",
            "Complete public version log; null clears it only on changelog edit.",
        ),
        (
            "wa_id",
            "Parent WA ID for exact log readback. Without it an edit reports API acceptance; supply it to verify or recover the log.",
        ),
        (
            "linked_mods",
            "Select unique mod_id entries present in the cloud backup. Current backup fields replace stale caller file/version metadata.",
        ),
        (
            "roleid",
            "Exact string role ID from config backup-get; an empty string selects no role.",
        ),
        (
            "ignored_unknown_mods",
            "Names to exclude, selected from the chosen cloud backup's unknown_plugins.",
        ),
        (
            "ignored_materials",
            "Names to exclude, selected from the chosen cloud backup's materials.",
        ),
        (
            "ignored_fronts",
            "Font names to exclude, selected from the chosen cloud backup's fonts.",
        ),
        (
            "co_authors",
            "Complete replacement; [] clears. Unique positive user_id, share_percent in (0,1], total at most 1.",
        ),
        (
            "references",
            "Complete replacement; [] clears. Picker types: plugin=1, WA=2, config=3. Readback display types are a separate namespace.",
        ),
        (
            "attachments",
            "Reusable content references plus name, install_type, install_path and is_compressed. Select current destinations from wa attachment-paths.",
        ),
        (
            "kind",
            "image uploads a reusable WA image URL; attachment uses the native content-index/object-upload protocol.",
        ),
        (
            "install_type",
            "Current attachment destination type from wa attachment-paths.",
        ),
        (
            "install_path",
            "Destination paired with install_type in wa attachment-paths.",
        ),
    ] {
        if props.get(field).is_some() {
            props[field]["description"] = json!(description);
        }
    }
    if props.get("content_format").is_some() {
        props["content_format"]["enum"] = json!([1, 2]);
    }
    for (field, description) in [
        ("logo", "Existing reusable image URL."),
        ("thumbnail", "Existing reusable WA thumbnail URL."),
        (
            "screenshots",
            "Existing reusable image URLs; [] clears on edit.",
        ),
        (
            "picture_urls",
            "Existing reusable configuration image URLs; [] clears on edit.",
        ),
        (
            "images",
            "Existing reusable WA image URLs; [] clears on edit.",
        ),
        (
            "logo_file",
            "Local replacement logo, resolved from the invocation directory.",
        ),
        (
            "thumbnail_file",
            "Local replacement thumbnail, resolved from the invocation directory.",
        ),
        (
            "screenshot_files",
            "Local images appended after screenshots.",
        ),
        ("picture_files", "Local images appended after picture_urls."),
        ("image_files", "Local images appended after images."),
    ] {
        if props.get(field).is_some() {
            props[field]["description"] = json!(description);
        }
    }
}
pub(super) fn review_schema() -> Value {
    json!([
        {"if":{"properties":{"public":{"const":true}},"required":["public"]},"then":{"properties":{"submit_for_review":{"const":true}},"required":["submit_for_review"]}},
        {"if":{"properties":{"submit_for_review":{"const":true}},"required":["submit_for_review"]},"then":{"properties":{"public":{"const":true}},"required":["public"]}}
    ])
}
pub(super) fn relationship_schemas(props: &mut Value) {
    if props.get("co_authors").is_some() {
        props["co_authors"]["items"] = json!({"type":"object","properties":{"user_id":{"type":"integer","minimum":1},"share_percent":{"type":"number","exclusiveMinimum":0,"maximum":1}},"required":["user_id","share_percent"],"additionalProperties":false});
    }
    if props.get("references").is_some() {
        props["references"]["items"] = json!({"type":"object","properties":{"type":{"type":"integer","enum":[1,2,3]},"id":{"type":"integer","minimum":1}},"required":["type","id"],"additionalProperties":false});
    }
}
pub(super) fn embedded_relationships(
    resource: Resource,
    id: u64,
    doc: &Value,
) -> Result<Vec<RelationshipReplacement>> {
    let mut result = Vec::new();
    for (field, id_field, suffix, kind) in [
        (
            "co_authors",
            "content_id",
            "co-author",
            RelationshipKind::Coauthors,
        ),
        (
            "references",
            "source_id",
            "reference",
            RelationshipKind::References,
        ),
    ] {
        if let Some(value) = doc.get(field) {
            let shape = input_shape(
                Provider::Newbee,
                &format!("{}-{suffix}", resource.key()),
                "set",
            )?;
            let mut body = json!({"schema":shape.name});
            body[id_field] = json!(id);
            body[field] = value.clone();
            result.push(RelationshipReplacement::parse(
                resource,
                kind,
                &serde_json::to_vec(&body)?,
            )?);
        }
    }
    Ok(result)
}
pub(super) fn selected_ids(value: Value) -> Result<Value> {
    Ok(json!(
        optional_array(value)?
            .into_iter()
            .map(|item| wire_id(if item.is_object() {
                pick(
                    &item,
                    &["id", "t_id", "category_id", "t_category_id", "value"],
                )
            } else {
                item
            }))
            .collect::<Result<Vec<_>>>()?
    ))
}
pub(super) fn text(value: Value) -> Result<Value> {
    if value.is_null() {
        return Ok(json!(""));
    }
    if value.is_string() {
        Ok(value)
    } else {
        Err(remote("NewBeeBox text form field is invalid"))
    }
}
pub(super) fn boolean(value: Value) -> Result<Value> {
    match value {
        Value::Null => Ok(json!(false)),
        Value::Bool(_) => Ok(value),
        value if value == 0 || value == "0" => Ok(json!(false)),
        value if value == 1 || value == "1" => Ok(json!(true)),
        _ => Err(remote("NewBeeBox boolean form field is invalid")),
    }
}
pub(super) fn form(raw: &Value) -> Result<Value> {
    let mut result = json!({"id":wire_id(pick(raw,&["t_id","id"]))?,
        "mod_categories":selected_ids(pick(raw,&["category_ids","mod_categories"]))?,
        "content_origin":wire_uint(pick(raw,&["t_original","content_origin"]),0)?,
        "content_format":wire_uint(pick(raw,&["t_content_format","content_format"]),0)?,
        "share_state":wire_uint(pick(raw,&["t_share","share_state"]),0)?,
        "subscribe_plan_level":wire_uint(pick(raw,&["t_subscribe_plan_level","subscribe_plan_level"]),0)?,
        "screenshots":urls(pick(raw,&["screenshots"]))?,
        "link_to_channel":boolean(pick(raw,&["t_link_to_channel","link_to_channel"]))?});
    for (field, aliases) in [
        ("name", &["t_name", "name"][..]),
        (
            "description",
            &["t_description_v2", "description", "t_description"],
        ),
        ("intro", &["t_description", "intro"]),
        ("logo", &["t_logo", "logo"]),
    ] {
        result[field] = text(pick(raw, aliases))?;
    }
    Ok(result)
}
impl Newbee {
    pub(super) fn business_options(&mut self, doc: &Value) -> Result<()> {
        for (field, kind) in [
            ("content_origin", OptionKind::ContentOrigins),
            ("subscribe_plan_level", OptionKind::SubscribePlans),
            ("time_range", OptionKind::TimeRanges),
        ] {
            let Some(value) = doc.get(field) else {
                continue;
            };
            if field != "content_origin" && (value == 0 || value == "" || value.is_null()) {
                continue;
            }
            let options = self.read(&Read::Options(kind))?;
            let choices = options["items"].as_array().unwrap();
            if choices.is_empty() {
                return Err(remote("NewBeeBox returned no selectable business options"));
            }
            let scalar = |v: &Value| {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
            };
            if !choices
                .iter()
                .any(|row| scalar(&row["value"]) == scalar(value))
            {
                return Err(invalid(
                    &format!("$.{field}"),
                    "selection is unavailable in current business options",
                ));
            }
        }
        Ok(())
    }
    pub(super) fn categories(&mut self, selected: &Value) -> Result<()> {
        let categories = self.read(&Read::PluginCategories)?;
        let allowed = categories["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| wire_id(v["id"].clone()))
            .collect::<Result<BTreeSet<_>>>()?;
        if selected
            .as_array()
            .unwrap()
            .iter()
            .any(|v| !allowed.contains(&v.as_u64().unwrap()))
        {
            return Err(invalid(
                "$.mod_categories",
                "selection is unavailable in current plugin categories",
            ));
        }
        Ok(())
    }
    pub(super) fn versions(&mut self, id: u64) -> Result<Vec<Value>> {
        let mut all = Vec::new();
        let mut ids = BTreeSet::new();
        let mut expected = None;
        for page in 1..=10000 {
            let value = object(self.creator(
                "/creator/wow/mod_file/mod_file_list",
                json!({"mod_id":id,"game_version_id":0,"pagenum":page,"pagesize":100}),
            )?)?;
            let rows = array(pick(&value, &["list", "items"]))?;
            let total = pick(&value, &["total", "count"]);
            if !total.is_null() {
                let total = wire_uint(total, 0)?;
                if expected.is_some_and(|n| n != total) {
                    return Err(remote("plugin version total changed during pagination"));
                }
                expected = Some(total);
            }
            for row in &rows {
                let identity = pick(row, &["t_id", "id", "mod_file_id"]);
                let identity = if identity.is_null() {
                    pick(
                        row,
                        &["t_display_name", "display_name", "version", "t_version"],
                    )
                    .to_string()
                } else {
                    wire_id(identity)?.to_string()
                };
                if !row.is_object() || !ids.insert(identity) {
                    return Err(remote(
                        "plugin version pagination contains duplicate or invalid rows",
                    ));
                }
            }
            let count = rows.len();
            all.extend(rows);
            if let Some(total) = expected {
                if all.len() as u64 > total || (count == 0 && (all.len() as u64) < total) {
                    return Err(remote("plugin version pagination has a coverage gap"));
                }
                if all.len() as u64 == total {
                    return Ok(all);
                }
            } else if count < 100 {
                return Ok(all);
            }
        }
        Err(remote(
            "plugin version pagination exceeded the supported page bound",
        ))
    }
    pub fn edit_plugin(&mut self, request: &PluginEdit) -> Result<Value> {
        let doc = &request.doc;
        let id = doc["id"].as_u64().unwrap();
        let identity = media::request_identity(doc, &request.cwd, &self.control)?;
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "plugin",
            id,
            "edit",
            &identity,
            &self.control,
        )?;
        let before = form(&self.raw(Resource::Plugin, id)?)?;
        verify_pending_form(&mutation, &before)?;
        let mut desired = before.clone();
        for field in [
            "name",
            "mod_categories",
            "content_origin",
            "content_format",
            "intro",
            "description",
            "logo",
            "screenshots",
            "subscribe_plan_level",
            "link_to_channel",
        ] {
            if let Some(value) = doc.get(field) {
                desired[field] = value.clone();
            }
        }
        if let Some(file) = doc.get("logo_file") {
            desired["logo"] = self.image(
                Resource::Plugin,
                &request.cwd.join(file.as_str().unwrap()),
                !mutation.pending(),
            )?["url"]
                .clone();
        }
        desired["screenshots"] = self.resolve_images(
            Resource::Plugin,
            &desired["screenshots"],
            doc.get("screenshot_files"),
            &request.cwd,
            !mutation.pending(),
        )?;
        if let Some(public) = doc.get("public") {
            desired["share_state"] = json!(if public == true { 1 } else { 0 });
        }
        if desired["share_state"] != 1 {
            desired["link_to_channel"] = json!(false);
        }
        let mut writes = 0;
        let recovered = mutation.pending();
        if desired != before {
            if mutation.pending() {
                return Err(mutation.pending_error(
                    "the original plugin edit is not present in readback; no second edit was sent",
                ));
            }
            if doc["public"] == true && self.versions(id)?.is_empty() {
                return Err(invalid(
                    "$.public",
                    "a plugin needs its first version before public review",
                ));
            }
            if doc.get("mod_categories").is_some() {
                self.categories(&desired["mod_categories"])?;
            }
            self.business_options(doc)?;
            mutation.prepare(&self.control)?;
            remember_form(&mut mutation, &desired)?;
            self.post(
                Origin::NewbeeCreator,
                "/creator/wow/mod/edit",
                &desired,
                true,
            )
            .map_err(|error| mutation.failure(error))?;
            writes = 1;
            mutation.acknowledged().map_err(|_| {
                mutation.pending_error("plugin edit was accepted; finish saving its receipt")
            })?;
            let readback = self
                .raw(Resource::Plugin, id)
                .and_then(|raw| form(&raw))
                .map_err(|error| mutation.readback_error("plugin edit needs readback", error))?;
            if readback != desired {
                return Err(mutation.pending_error(
                    "plugin edit readback differs from the submitted complete form",
                ));
            }
            mutation.verified()?;
        } else if recovered {
            mutation.verified()?;
        }
        let operation = mutation.id().map(str::to_owned);
        drop(mutation);
        let relationships = self.apply_relationships(Resource::Plugin, id, doc)?;
        Ok(output::public(
            json!({"id":id,"operation_id":operation,"author_writes":writes,"recovered":recovered,"review_intent":doc["public"]==true,"readback":desired,"relationships":relationships}),
            &[],
        ))
    }
    pub(super) fn apply_relationships(
        &mut self,
        resource: Resource,
        id: u64,
        doc: &Value,
    ) -> Result<Vec<Value>> {
        embedded_relationships(resource, id, doc)?
            .iter()
            .map(|request| self.replace_relationships(request))
            .collect()
    }
    pub fn update_plugin(&mut self, request: &PluginUpdate) -> Result<Value> {
        let doc = &request.doc;
        let id = doc["mod_id"].as_u64().unwrap();
        let version = doc["version"].as_str().unwrap();
        let path = request.cwd.join(doc["file"].as_str().unwrap());
        let (mut file, digest) = storage::locked_sha256(&path, 300 * 1024 * 1024, &self.control)?;
        let result = (|| {
            if file.metadata()?.len() == 0 {
                return Err(invalid("$.file", "plugin archive must not be empty"));
            }
            file.seek(SeekFrom::Start(0))?;
            if path
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("zip"))
            {
                zip::ZipArchive::new(&mut file)
                    .map_err(|_| invalid("$.file", "invalid plugin ZIP archive"))?;
            }
            let mut identity = doc.clone();
            identity["file_sha256"] = json!(digest);
            let mut mutation = Mutation::open(
                &self.home,
                Provider::Newbee,
                "plugin",
                id,
                "update",
                &identity,
                &self.control,
            )?;
            if let Some(evidence) = mutation.evidence() {
                return Ok(evidence.clone());
            }
            let existing = self.versions(id)?;
            if let Some(uploaded) = find_version(&existing, version) {
                if !mutation.pending() {
                    return Err(invalid(
                        "$.version",
                        "plugin version already exists; overwrite is not allowed",
                    ));
                }
                require_bindings(uploaded, &doc["game_version_list"])?;
                let result = version_result(id, version, &digest, uploaded, mutation.id(), true);
                mutation.verified_with(result.clone())?;
                return Ok(result);
            }
            if mutation.pending() {
                return Err(mutation.pending_error(
                    "the original plugin version is not visible; no archive was sent again",
                ));
            }
            let options = self.read(&Read::GameVersions)?;
            let allowed = options["items"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|v| v["versions"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>();
            if doc["game_version_list"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| !allowed.contains(v.as_str().unwrap()))
            {
                return Err(invalid(
                    "$.game_version_list",
                    "game version is unavailable in current options",
                ));
            }
            let current = self.raw(Resource::Plugin, id)?;
            let link = doc.get("link_to_channel").cloned().unwrap_or(boolean(pick(
                &current,
                &["t_link_to_channel", "link_to_channel"],
            ))?);
            let mut fields = vec![
                ("mod_id", id.to_string()),
                ("version", version.to_owned()),
                (
                    "game_version_list",
                    serde_json::to_string(&doc["game_version_list"])?,
                ),
                ("link_to_channel", link.to_string()),
            ];
            if let Some(log) = doc.get("changelog") {
                fields.push(("changelog", log.as_str().unwrap().to_owned()));
            }
            let filename = path
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or_else(|| invalid("$.file", "archive basename must be UTF-8"))?;
            file.seek(SeekFrom::Start(0))?;
            let stream = file.try_clone()?;
            let headers = self.headers()?;
            mutation.prepare(&self.control)?;
            let response = self
                .http
                .multipart_file(
                    Origin::NewbeeCreator,
                    "/creator/wow/mod_file/upload_mod_file",
                    headers,
                    &fields,
                    filename,
                    media::mime(&path),
                    stream,
                )
                .map_err(|error| mutation.failure(error))?;
            envelope(response, "/creator/wow/mod_file/upload_mod_file", true)
                .map_err(|error| mutation.failure(error))?;
            mutation.acknowledged().map_err(|_| {
                mutation.pending_error("plugin archive was accepted; finish saving its receipt")
            })?;
            let versions = self.versions(id).map_err(|error| {
                mutation.readback_error("plugin version readback is pending", error)
            })?;
            let uploaded = find_version(&versions, version).ok_or_else(|| {
                mutation.pending_error("uploaded plugin version is not yet present in readback")
            })?;
            require_bindings(uploaded, &doc["game_version_list"]).map_err(|error| {
                mutation.readback_error("uploaded version bindings need verification", error)
            })?;
            let result = version_result(id, version, &digest, uploaded, mutation.id(), false);
            mutation.verified_with(result.clone())?;
            Ok(result)
        })();
        storage::finish_file(file, result, &self.control)
    }
}
pub(super) fn form_hash(value: &Value) -> Result<String> {
    Ok(storage::sha256(&storage::identity_json(value)?))
}
pub(super) fn remember_form(mutation: &mut Mutation, value: &Value) -> Result<()> {
    mutation.remember(json!({"expected_form_sha256":form_hash(value)?}))
}
pub(super) fn verify_pending_form(mutation: &Mutation, value: &Value) -> Result<()> {
    if mutation.pending()
        && mutation
            .evidence()
            .and_then(|evidence| evidence["expected_form_sha256"].as_str())
            != Some(form_hash(value)?.as_str())
    {
        return Err(mutation.pending_error(
            "the complete original author form is not present in readback; no write was repeated",
        ));
    }
    Ok(())
}
fn find_version<'a>(versions: &'a [Value], version: &str) -> Option<&'a Value> {
    versions.iter().find(|item| {
        pick(
            item,
            &["t_display_name", "display_name", "version", "t_version"],
        )
        .as_str()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case(version.trim()))
    })
}
fn require_bindings(uploaded: &Value, selected: &Value) -> Result<()> {
    let values = optional_array(pick(
        uploaded,
        &["versions", "game_version_list", "t_game_version_list"],
    ))?;
    let bindings = values
        .into_iter()
        .map(|value| {
            if value.is_object() {
                pick(&value, &["version", "build", "support_version", "value"])
            } else {
                value
            }
        })
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| remote("uploaded plugin has invalid game-version bindings"))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if selected
        .as_array()
        .unwrap()
        .iter()
        .any(|value| !bindings.contains(value.as_str().unwrap()))
    {
        return Err(remote(
            "uploaded plugin version does not contain all selected game-version bindings",
        ));
    }
    Ok(())
}
fn version_result(
    id: u64,
    version: &str,
    digest: &str,
    uploaded: &Value,
    operation: Option<&str>,
    recovered: bool,
) -> Value {
    output::public(
        json!({"id":id,"version":version,"file_sha256":digest,"file_id":pick(uploaded,&["t_id","id","mod_file_id"]),"operation_id":operation,"recovered":recovered,"verification":"version_and_bindings","remote_file_bytes_verification":"not_available","review_status":"not_checked"}),
        &[],
    )
}

#[cfg(test)]
mod tests;
