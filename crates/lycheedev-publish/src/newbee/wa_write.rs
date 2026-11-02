use super::config::{commercial, names};
use super::plugin::{
    admit_metadata, boolean, embedded_relationships, relationship_schemas, remember_form,
    review_schema, selected_ids, text, verify_pending_form,
};
use super::*;
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::Mutation,
    provider::Provider,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub struct WaEdit {
    doc: Value,
    cwd: PathBuf,
}
pub struct WaUpdate {
    doc: Value,
}
impl WaEdit {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        Ok(Self {
            doc: admit("edit", bytes, cwd)?,
            cwd: cwd.to_owned(),
        })
    }
    pub fn schema() -> Value {
        schema("edit")
    }
}
impl WaUpdate {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            doc: admit("update", bytes, Path::new(""))?,
        })
    }
    pub fn schema() -> Value {
        schema("update")
    }
}
fn admit(action: &str, bytes: &[u8], cwd: &Path) -> Result<Value> {
    let doc = document(bytes)?;
    input_shape(Provider::Newbee, "wa", action)?.admit(&doc)?;
    let doc = Value::Object(doc);
    let id = doc["id"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| invalid("$.id", "WA identity must be positive"))?;
    admit_metadata(&doc, cwd)?;
    embedded_relationships(Resource::Wa, id, &doc)?;
    if let Some(attachments) = doc.get("attachments") {
        attachment_values(attachments.clone(), true)?;
    }
    if doc
        .get("game_version_id")
        .is_some_and(|value| !value.as_u64().is_some_and(|n| n > 0))
    {
        return Err(invalid(
            "$.game_version_id",
            "game identity must be positive",
        ));
    }
    Ok(doc)
}
fn schema(action: &str) -> Value {
    let mut value = input_shape(Provider::Newbee, "wa", action)
        .unwrap()
        .schema();
    value.as_object_mut().unwrap().remove("x-validation-scope");
    let props = &mut value["properties"];
    props["id"]["minimum"] = json!(1);
    for field in ["category_id_list"] {
        if props.get(field).is_some() {
            props[field]["items"] = json!({"type":"integer","minimum":1});
            props[field]["uniqueItems"] = json!(true);
        }
    }
    for field in ["images", "image_files", "wa_str_titles"] {
        if props.get(field).is_some() {
            props[field]["items"] = json!({"type":"string","minLength":1});
        }
    }
    if props.get("attachments").is_some() {
        props["attachments"]["items"] = attachment_schema();
    }
    relationship_schemas(props);
    if action == "edit" {
        value["allOf"] = review_schema();
    }
    plugin::metadata_schema(&mut value);
    value
}
pub(super) fn attachment_schema() -> Value {
    json!({"type":"object","properties":{"name":{"type":"string","minLength":1},"install_type":{"type":"integer"},"install_path":{"type":"string"},"value":{"type":"string","minLength":1},"is_compressed":{"type":"boolean"},"timestamp":{"type":"integer"}},"required":["name","install_type","install_path","value","is_compressed"],"additionalProperties":false})
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attachment {
    name: String,
    install_type: i64,
    install_path: String,
    value: String,
    is_compressed: bool,
    #[serde(default)]
    timestamp: i64,
}
pub(super) fn attachment_values(value: Value, strict: bool) -> Result<Value> {
    let rows = optional_array(value)?;
    let mut result = Vec::new();
    for item in rows {
        let item = if strict {
            item
        } else {
            json!({"name":pick(&item,&["name","display_name","filename"]),"install_type":item.get("install_type"),"install_path":item.get("install_path"),"value":pick(&item,&["value","url"]),"is_compressed":item.get("is_compressed").cloned().unwrap_or(json!(true)),"timestamp":item.get("timestamp").cloned().unwrap_or(json!(0))})
        };
        let attachment: Attachment = serde_json::from_value(item).map_err(|_| {
            invalid(
                "$.attachments",
                "attachment fields do not match the declared nested types",
            )
        })?;
        if attachment.name.is_empty() || attachment.value.is_empty() {
            return Err(invalid(
                "$.attachments",
                "attachment name and value must be nonempty",
            ));
        }
        result.push(serde_json::to_value(attachment)?);
    }
    Ok(json!(result))
}
pub(super) fn form(raw: &Value) -> Result<Value> {
    let mut value = json!({"id":wire_id(pick(raw,&["t_id","id"]))?,"game_version_id":wire_id(pick(raw,&["game_version_id","t_game_version_id"]))?,
        "content_format":wire_uint(pick(raw,&["content_format","t_content_format"]),0)?,"content_origin":wire_uint(pick(raw,&["content_origin","t_content_origin"]),0)?,
        "subscribe_plan_level":wire_uint(pick(raw,&["subscribe_plan_level","t_subscribe_plan_level"]),0)?,"price":wire_uint(pick(raw,&["price","t_price"]),0)?,
        "share_state":wire_uint(pick(raw,&["share_state","t_share_state"]),2)?,"link_to_channel":boolean(pick(raw,&["link_to_channel","t_link_to_channel"]))?,
        "images":urls(pick(raw,&["images","t_images"]))?,"category_id_list":selected_ids(pick(raw,&["category_id_list","category_ids","category_list"]))?,
        "attachments":attachment_values(pick(raw,&["attachments","t_attachments"]),false)?,"wa_log":""});
    for (field, aliases) in [
        ("name", &["name", "t_name"][..]),
        ("intro", &["intro", "t_intro"]),
        ("description", &["description", "t_description"]),
        ("thumbnail", &["thumbnail", "t_thumbnail"]),
        ("time_range", &["time_range", "t_time_range"]),
    ] {
        value[field] = text(pick(raw, aliases))?;
    }
    Ok(value)
}
fn option_tree(value: &Value, found: &mut Vec<Value>) {
    if let Some(rows) = value.as_array() {
        for row in rows {
            option_tree(row, found);
        }
    } else if let Some(row) = value.as_object() {
        found.push(value.clone());
        for field in ["children", "items", "options", "list", "rows", "data"] {
            if let Some(value) = row.get(field) {
                option_tree(value, found);
            }
        }
    }
}
impl Newbee {
    pub(super) fn wa_choices(&mut self, form: &Value) -> Result<()> {
        let options = self.read(&Read::GameVersions)?;
        let id = form["game_version_id"].as_u64().unwrap();
        if !options["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| wire_uint(row["id"].clone(), 0).is_ok_and(|n| n == id))
        {
            return Err(invalid(
                "$.game_version_id",
                "game identity is unavailable in current options",
            ));
        }
        let raw = self.creator("/creator/wow/wa/category", json!({"game_version":id}))?;
        let mut rows = Vec::new();
        option_tree(&raw, &mut rows);
        let ids = rows
            .iter()
            .filter_map(|row| wire_id(pick(row, &["id", "t_id", "category_id", "value"])).ok())
            .collect::<BTreeSet<_>>();
        if ids.is_empty() {
            return Err(remote("NewBeeBox returned no selectable WA categories"));
        }
        if form["category_id_list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| !ids.contains(&item.as_u64().unwrap()))
        {
            return Err(invalid(
                "$.category_id_list",
                "category is unavailable for the selected game",
            ));
        }
        Ok(())
    }
    pub(super) fn attachment_choices(&mut self, attachments: &Value) -> Result<()> {
        let attachments = attachments.as_array().unwrap();
        if attachments.is_empty() {
            return Ok(());
        }
        let raw = self.creator("/creator/wow/wa/attachment_install_path_list", json!({}))?;
        let mut rows = Vec::new();
        option_tree(&raw, &mut rows);
        let options = rows
            .iter()
            .filter(|row| {
                row.get("value").is_some()
                    && (row.get("extract_base_dir").is_some() || row.get("install_path").is_some())
            })
            .collect::<Vec<_>>();
        if options.is_empty() {
            return Err(remote(
                "NewBeeBox returned no selectable attachment destinations",
            ));
        }
        for attachment in attachments {
            if !options.iter().any(|row| {
                wire_uint(row["value"].clone(), u64::MAX)
                    .is_ok_and(|value| attachment["install_type"].as_u64() == Some(value))
                    && {
                        let directory = pick(row, &["extract_base_dir", "install_path"]);
                        directory == ""
                            || directory.is_null()
                            || directory == attachment["install_path"]
                    }
            }) {
                return Err(invalid(
                    "$.attachments",
                    "attachment install type/path is unavailable in current options",
                ));
            }
        }
        Ok(())
    }
    pub fn edit_wa(&mut self, request: &WaEdit) -> Result<Value> {
        let doc = &request.doc;
        let id = doc["id"].as_u64().unwrap();
        let identity = media::request_identity(doc, &request.cwd, &self.control)?;
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "wa",
            id,
            "edit",
            &identity,
            &self.control,
        )?;
        let before = form(&self.raw(Resource::Wa, id)?)?;
        verify_pending_form(&mutation, &before)?;
        let mut desired = before.clone();
        for (field, value) in doc.as_object().unwrap() {
            if desired.get(field).is_some() && field != "id" {
                desired[field] = value.clone();
            }
        }
        if let Some(value) = doc.get("attachments") {
            desired["attachments"] = attachment_values(value.clone(), true)?;
        }
        if let Some(file) = doc.get("thumbnail_file") {
            desired["thumbnail"] = self.image(
                Resource::Wa,
                &request.cwd.join(file.as_str().unwrap()),
                !mutation.pending(),
            )?["url"]
                .clone();
        }
        desired["images"] = self.resolve_images(
            Resource::Wa,
            &desired["images"],
            doc.get("image_files"),
            &request.cwd,
            !mutation.pending(),
        )?;
        if let Some(value) = doc.get("public") {
            desired["share_state"] = json!(if value == true { 1 } else { 2 });
        }
        let public = desired["share_state"] == 1;
        commercial(&mut desired, public);
        let recovered = mutation.pending();
        let mut writes = 0;
        if desired != before {
            if recovered {
                return Err(mutation.pending_error(
                    "the original WA edit is not present in readback; no update was sent again",
                ));
            }
            self.wa_choices(&desired)?;
            self.business_options(doc)?;
            self.attachment_choices(&desired["attachments"])?;
            mutation.prepare(&self.control)?;
            remember_form(&mut mutation, &desired)?;
            self.post(
                Origin::NewbeeCreator,
                "/creator/wow/wa/update",
                &desired,
                true,
            )
            .map_err(|error| mutation.failure(error))?;
            writes = 1;
            mutation.acknowledged().map_err(|_| {
                mutation.pending_error("WA edit was accepted; finish saving its receipt")
            })?;
            let readback = self
                .raw(Resource::Wa, id)
                .and_then(|raw| form(&raw))
                .map_err(|error| mutation.readback_error("WA edit readback is pending", error))?;
            if readback != desired {
                return Err(
                    mutation.pending_error("WA readback differs from the complete submitted form")
                );
            }
            mutation.verified()?;
        } else if recovered {
            mutation.verified()?;
        }
        let operation = mutation.id().map(str::to_owned);
        drop(mutation);
        let relationships = self.apply_relationships(Resource::Wa, id, doc)?;
        Ok(output::public(
            json!({"id":id,"operation_id":operation,"recovered":recovered,"author_writes":writes,"readback":desired,"relationships":relationships}),
            &[],
        ))
    }
    pub fn update_wa(&mut self, request: &WaUpdate) -> Result<Value> {
        let doc = &request.doc;
        let id = doc["id"].as_u64().unwrap();
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "wa",
            id,
            "update",
            doc,
            &self.control,
        )?;
        if mutation
            .evidence()
            .is_some_and(|value| value["verification"].is_string())
        {
            return Ok(mutation.evidence().unwrap().clone());
        }
        let pending = mutation.pending();
        let version = if pending {
            mutation
                .evidence()
                .and_then(|value| value["version"].as_str())
                .map(str::to_owned)
                .ok_or_else(|| {
                    mutation.pending_error("restore the original WA version intent before recovery")
                })?
        } else if let Some(version) = doc.get("version") {
            version.as_str().unwrap().to_owned()
        } else {
            let value = self.creator("/creator/wow/wa/get_next_version", json!({"id":id}))?;
            if value.is_string() {
                value.as_str().unwrap().to_owned()
            } else {
                pick(&object(value)?, &["version", "next_version", "t_version"])
                    .as_str()
                    .unwrap_or("")
                    .to_owned()
            }
        };
        if pending {
            let latest = self
                .creator("/creator/wow/wa_log/latest_str_info", json!({"wa_id":id}))
                .map_err(|error| {
                    mutation
                        .readback_error("WA version recovery needs its original readback", error)
                })?;
            let result = wa_version_result(id, &version, doc, &latest, mutation.id(), true)
                .map_err(|error| {
                    mutation.readback_error(
                        "the original WA version is not verified; no string was sent again",
                        error,
                    )
                })?;
            mutation.verified_with(result.clone())?;
            return Ok(result);
        }
        let current = self.raw(Resource::Wa, id)?;
        if !version_greater(
            &version,
            &pick(&current, &["t_version", "version"])
                .as_str()
                .unwrap_or(""),
        ) {
            return Err(invalid(
                "$.version",
                "WA version must be greater than its current numeric version",
            ));
        }
        let titles = doc.get("wa_str_titles").cloned().unwrap_or(names(pick(
            &current,
            &["t_wa_str_titles", "wa_str_titles"],
        ))?);
        let link = doc.get("link_to_channel").cloned().unwrap_or(boolean(pick(
            &current,
            &["t_link_to_channel", "link_to_channel"],
        ))?);
        let body = json!({"id":id,"version":version,"wa_str":doc["wa_str"],"wa_log":doc["wa_log"],"wa_str_titles":titles,"link_to_channel":link});
        mutation.prepare(&self.control)?;
        mutation.remember(json!({"version":version}))?;
        self.post(
            Origin::NewbeeCreator,
            "/creator/wow/wa/update_wa_str",
            &body,
            true,
        )
        .map_err(|error| mutation.failure(error))?;
        mutation.acknowledged().map_err(|_| {
            mutation.pending_error("WA update was accepted; finish saving its receipt")
        })?;
        let latest = self
            .creator("/creator/wow/wa_log/latest_str_info", json!({"wa_id":id}))
            .map_err(|error| mutation.readback_error("WA version readback is pending", error))?;
        let result = wa_version_result(id, &version, doc, &latest, mutation.id(), false).map_err(
            |error| {
                mutation.readback_error(
                    "WA version readback does not match the original update",
                    error,
                )
            },
        )?;
        mutation.verified_with(result.clone())?;
        Ok(result)
    }
}
fn wa_version_result(
    id: u64,
    version: &str,
    doc: &Value,
    latest: &Value,
    operation: Option<&str>,
    recovered: bool,
) -> Result<Value> {
    let latest = object(latest.clone())?;
    let observed = pick(&latest, &["version", "t_version"]);
    if observed.as_str() != Some(version) {
        return Err(remote(
            "WA readback version does not match the original version",
        ));
    }
    for field in ["wa_id", "module_id"] {
        if let Some(value) = latest.get(field) {
            if wire_id(value.clone())? != id {
                return Err(remote(
                    "WA version readback belongs to a different resource",
                ));
            }
        }
    }
    let content = pick(&latest, &["wa_str", "t_wa_str", "str"]);
    let verification = if content.is_null() {
        "not_available"
    } else {
        if content != doc["wa_str"] {
            return Err(remote("WA readback string differs from the original input"));
        }
        "readback"
    };
    Ok(
        json!({"id":id,"version":version,"operation_id":operation,"recovered":recovered,"verification":"version","wa_content_verification":verification,"review_status":"not_checked"}),
    )
}
fn version_greater(candidate: &str, current: &str) -> bool {
    fn parts(text: &str) -> Vec<String> {
        text.trim()
            .split('.')
            .filter(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .map(|part| {
                let part = part.trim_start_matches('0');
                if part.is_empty() {
                    "0".into()
                } else {
                    part.into()
                }
            })
            .collect()
    }
    let left = parts(candidate);
    let right = parts(current);
    if left.is_empty() || right.is_empty() {
        return false;
    }
    for (left, right) in left.iter().zip(&right) {
        let order = left.len().cmp(&right.len()).then_with(|| left.cmp(right));
        if !order.is_eq() {
            return order.is_gt();
        }
    }
    left.len() > right.len()
}

#[cfg(test)]
mod tests;
