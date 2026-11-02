use super::plugin::{
    admit_metadata, boolean, embedded_relationships, relationship_schemas, remember_form,
    review_schema, text, verify_pending_form,
};
use super::*;
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::Mutation,
    provider::Provider,
};
use std::collections::{BTreeMap, BTreeSet};

pub struct ConfigEdit {
    doc: Value,
    cwd: PathBuf,
}
pub struct ConfigUpdate {
    doc: Value,
}
impl ConfigEdit {
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
impl ConfigUpdate {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        Ok(Self {
            doc: admit("update", bytes, cwd)?,
        })
    }
    pub fn schema() -> Value {
        schema("update")
    }
}
fn admit(action: &str, bytes: &[u8], cwd: &Path) -> Result<Value> {
    let doc = document(bytes)?;
    input_shape(Provider::Newbee, "config", action)?.admit(&doc)?;
    let doc = Value::Object(doc);
    let id = doc["id"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(|| invalid("$.id", "configuration identity must be positive"))?;
    admit_metadata(&doc, cwd)?;
    embedded_relationships(Resource::Config, id, &doc)?;
    if action == "update" {
        if let Some(cloud) = doc.get("cloud_id") {
            if !cloud.as_u64().is_some_and(|n| n > 0) {
                return Err(invalid("$.cloud_id", "cloud identity must be positive"));
            }
            for field in [
                "linked_mods",
                "ignored_unknown_mods",
                "ignored_materials",
                "ignored_fronts",
                "roleid",
            ] {
                if doc.get(field).is_none() {
                    return Err(invalid(
                        &format!("$.{field}"),
                        "an explicit selection is required when cloud_id changes",
                    ));
                }
            }
        }
        if let Some(mods) = doc.get("linked_mods") {
            linked_mods(mods.clone(), true)?;
        }
    }
    Ok(doc)
}
fn schema(action: &str) -> Value {
    let mut value = input_shape(Provider::Newbee, "config", action)
        .unwrap()
        .schema();
    value.as_object_mut().unwrap().remove("x-validation-scope");
    let props = &mut value["properties"];
    props["id"]["minimum"] = json!(1);
    if props.get("cloud_id").is_some() {
        props["cloud_id"]["minimum"] = json!(1);
    }
    for field in [
        "picture_urls",
        "picture_files",
        "ignored_unknown_mods",
        "ignored_materials",
        "ignored_fronts",
    ] {
        if props.get(field).is_some() {
            props[field]["items"] = json!({"type":"string","minLength":1});
        }
    }
    if props.get("linked_mods").is_some() {
        props["linked_mods"]["items"] = linked_schema();
    }
    relationship_schemas(props);
    value["allOf"] = if action == "edit" {
        review_schema()
    } else {
        json!([{"if":{"required":["cloud_id"]},"then":{"required":["linked_mods","ignored_unknown_mods","ignored_materials","ignored_fronts","roleid"]}}])
    };
    plugin::metadata_schema(&mut value);
    value
}
pub(super) fn linked_schema() -> Value {
    json!({"type":"object","properties":{"mod_id":{"type":"integer","minimum":1},"mod_name":{"type":["string","null"]},"mod_file_id":{"type":["integer","null"]},"mod_version":{"type":["string","null"]},"display_name":{"type":["string","null"]},"update_type":{"type":"integer"},"updateType":{"type":"integer"}},"required":["mod_id"],"additionalProperties":false})
}
pub(super) fn linked_mods(value: Value, strict: bool) -> Result<Value> {
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    for item in optional_array(value)? {
        let fields = [
            "mod_id",
            "mod_name",
            "mod_file_id",
            "mod_version",
            "display_name",
            "update_type",
            "updateType",
        ];
        if !item.is_object()
            || (strict
                && item
                    .as_object()
                    .unwrap()
                    .keys()
                    .any(|field| !fields.contains(&field.as_str())))
        {
            return Err(invalid(
                "$.linked_mods",
                "linked plugin contains unknown fields or is not an object",
            ));
        }
        let id = wire_id(item["mod_id"].clone())
            .map_err(|_| invalid("$.linked_mods", "linked plugin identity must be positive"))?;
        if strict && !item["mod_id"].is_u64() {
            return Err(invalid(
                "$.linked_mods",
                "linked plugin identity must be an integer",
            ));
        }
        if !ids.insert(id) {
            return Err(invalid("$.linked_mods", "linked plugin IDs must be unique"));
        }
        for field in ["mod_name", "mod_version", "display_name"] {
            if item
                .get(field)
                .is_some_and(|value| !value.is_null() && !value.is_string())
            {
                return Err(invalid(
                    "$.linked_mods",
                    "linked plugin text must be a string or null",
                ));
            }
        }
        if item
            .get("mod_file_id")
            .is_some_and(|value| !value.is_null() && !value.is_i64() && !value.is_u64())
        {
            return Err(invalid(
                "$.linked_mods",
                "linked plugin file ID must be an integer or null",
            ));
        }
        let update = pick(&item, &["update_type", "updateType"]);
        let update = if update.is_null() { json!(1) } else { update };
        if !update.is_i64() && !update.is_u64() {
            return Err(invalid("$.linked_mods", "updateType must be an integer"));
        }
        result.push(json!({"mod_id":id,"mod_name":item.get("mod_name"),"mod_file_id":item.get("mod_file_id"),"mod_version":item.get("mod_version").filter(|v|v!=&&json!("")),"display_name":item.get("display_name").filter(|v|v!=&&json!("")),"updateType":update}));
    }
    Ok(json!(result))
}
pub(super) fn names(value: Value) -> Result<Value> {
    Ok(json!(
        optional_array(value)?
            .into_iter()
            .map(|item| {
                let value = if item.is_object() {
                    pick(&item, &["name", "display_name", "label", "value"])
                } else {
                    item
                };
                value
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| remote("selected name must be a nonempty string"))
            })
            .collect::<Result<Vec<_>>>()?
    ))
}
fn role(value: Value) -> Result<Value> {
    match value {
        Value::Null => Ok(json!("")),
        Value::String(_) => Ok(value),
        value if value.as_u64() == Some(0) => Ok(json!("")),
        Value::Number(number) if number.as_u64().is_some() => Ok(json!(number.to_string())),
        _ => Err(remote("configuration role identity is invalid")),
    }
}
pub(super) fn form(raw: &Value) -> Result<Value> {
    let mut value = json!({"tid":wire_id(pick(raw,&["t_id","id"]))?,"cloud_id":wire_uint(pick(raw,&["t_cloudblackid","cloud_id"]),0)?,
        "content_format":wire_uint(pick(raw,&["t_content_format","content_format"]),0)?,"content_origin":wire_uint(pick(raw,&["t_content_origin","content_origin"]),0)?,
        "sharing":wire_uint(pick(raw,&["t_sharing","sharing"]),0)?,"subscribe_plan_level":wire_uint(pick(raw,&["t_subscribe_plan_level","subscribe_plan_level"]),0)?,
        "price":wire_uint(pick(raw,&["t_price","price"]),0)?,"pic_url":urls(pick(raw,&["pic_url","picture_urls","piclist"]))?,
        "link_to_channel":boolean(pick(raw,&["t_link_to_channel","link_to_channel"]))?,"linked_mods":linked_mods(pick(raw,&["t_linked_mods","linked_mods"]),false)?,
        "roleid":role(pick(raw,&["t_roleid","roleid","role_id"]))?});
    for (field, aliases) in [
        ("title", &["t_title", "title"][..]),
        ("content", &["t_content", "content"]),
        ("intro", &["t_intro", "intro"]),
        ("time_range", &["t_time_range", "time_range"]),
    ] {
        value[field] = text(pick(raw, aliases))?;
    }
    for (field, aliases) in [
        (
            "ignored_unknown_mods",
            &["t_ignored_unknown_mods", "ignored_unknown_mods"][..],
        ),
        (
            "ignored_materials",
            &["t_ignored_materials", "ignored_materials"],
        ),
        ("ignored_fronts", &["t_ignored_fronts", "ignored_fronts"]),
    ] {
        value[field] = names(pick(raw, aliases))?;
    }
    if raw.get("t_selected_eml").is_some() || raw.get("selected_eml").is_some() {
        value["selected_eml"] = json!(optional_array(pick(
            raw,
            &["t_selected_eml", "selected_eml"]
        ))?);
    }
    Ok(value)
}
pub(super) fn commercial(value: &mut Value, public: bool) {
    if !public {
        value["link_to_channel"] = json!(false);
    }
    if value.get("price") == Some(&json!(0)) {
        value["time_range"] = json!("");
    }
}
impl Newbee {
    fn backup_rows(&mut self) -> Result<Vec<Value>> {
        fn flatten(value: Value, rows: &mut Vec<Value>) -> Result<()> {
            match value {
                Value::Array(items) => {
                    for item in items {
                        flatten(item, rows)?;
                    }
                }
                Value::Object(_) => rows.push(value),
                _ => return Err(remote("configuration backup list has invalid rows")),
            }
            Ok(())
        }
        let value = self.creator("/creator/wow/share/list", json!({}))?;
        let mut rows = Vec::new();
        flatten(value, &mut rows)?;
        Ok(rows)
    }
    pub(super) fn config_backups(&mut self) -> Result<Value> {
        let items = self
            .backup_rows()?
            .iter()
            .map(backup)
            .collect::<Result<Vec<_>>>()?;
        Ok(json!({"total":items.len(),"items":items}))
    }
    pub(super) fn config_backup(&mut self, id: u64) -> Result<Value> {
        let row = self
            .backup_rows()?
            .into_iter()
            .find(|row| {
                wire_uint(pick(row, &["t_id", "cloud_id"]), 0).is_ok_and(|value| value == id)
            })
            .ok_or_else(|| {
                ToolkitError::new(
                    "publish.backup_not_found",
                    "selected NewBeeBox configuration backup was not found",
                    5,
                )
            })?;
        backup(&row)
    }
    pub(super) fn config_selection(&mut self, value: &mut Value) -> Result<()> {
        let cloud_id = wire_id(value["cloud_id"].clone())?;
        let backup = self.config_backup(cloud_id)?;
        let requested = linked_mods(value["linked_mods"].clone(), false)?;
        let available = backup["linked_mods"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| (item["mod_id"].as_u64().unwrap(), item.clone()))
            .collect::<BTreeMap<_, _>>();
        value["linked_mods"] = json!(
            requested
                .as_array()
                .unwrap()
                .iter()
                .map(|item| available
                    .get(&item["mod_id"].as_u64().unwrap())
                    .cloned()
                    .ok_or_else(|| invalid(
                        "$.linked_mods",
                        "linked plugin is absent from the selected cloud backup"
                    )))
                .collect::<Result<Vec<_>>>()?
        );
        for (field, source) in [
            ("ignored_unknown_mods", "unknown_plugins"),
            ("ignored_materials", "materials"),
            ("ignored_fronts", "fonts"),
        ] {
            let choices = backup[source].as_array().unwrap();
            if value[field]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| !choices.contains(item))
            {
                return Err(invalid(
                    &format!("$.{field}"),
                    "selected item is absent from the cloud backup",
                ));
            }
        }
        if value["roleid"] != ""
            && !backup["roles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["role_id"] == value["roleid"])
        {
            return Err(invalid(
                "$.roleid",
                "role is absent from the selected cloud backup",
            ));
        }
        Ok(())
    }
    pub fn edit_config(&mut self, request: &ConfigEdit) -> Result<Value> {
        self.change_config(&request.doc, &request.cwd, true)
    }
    pub fn update_config(&mut self, request: &ConfigUpdate) -> Result<Value> {
        self.change_config(&request.doc, Path::new(""), false)
    }
    fn change_config(&mut self, doc: &Value, cwd: &Path, metadata: bool) -> Result<Value> {
        let id = doc["id"].as_u64().unwrap();
        let action = if metadata { "edit" } else { "update" };
        let identity = media::request_identity(doc, cwd, &self.control)?;
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "config",
            id,
            action,
            &identity,
            &self.control,
        )?;
        let before = form(&self.raw(Resource::Config, id)?)?;
        verify_pending_form(&mutation, &before)?;
        let mut desired = before.clone();
        for (field, value) in doc.as_object().unwrap() {
            let wire = match field.as_str() {
                "picture_urls" => "pic_url",
                "public" => "sharing",
                other => other,
            };
            if desired.get(wire).is_some() && field != "id" {
                desired[wire] = if field == "public" {
                    json!(if value == true { 1 } else { 0 })
                } else {
                    value.clone()
                };
            }
        }
        if metadata {
            desired["pic_url"] = self.resolve_images(
                Resource::Config,
                &desired["pic_url"],
                doc.get("picture_files"),
                cwd,
                !mutation.pending(),
            )?;
        }
        let public = desired["sharing"] != 0;
        commercial(&mut desired, public);
        let recovered = mutation.pending();
        let mut writes = 0;
        if desired != before {
            if mutation.pending() {
                return Err(mutation.pending_error("the original configuration write is not present in readback; no update was sent again"));
            }
            self.config_selection(&mut desired)?;
            self.business_options(doc)?;
            mutation.prepare(&self.control)?;
            remember_form(&mut mutation, &desired)?;
            self.post(
                Origin::NewbeeCreator,
                "/creator/wow/share_config/update",
                &desired,
                true,
            )
            .map_err(|error| mutation.failure(error))?;
            writes = 1;
            mutation.acknowledged().map_err(|_| {
                mutation
                    .pending_error("configuration update was accepted; finish saving its receipt")
            })?;
            let readback = self
                .raw(Resource::Config, id)
                .and_then(|raw| form(&raw))
                .map_err(|error| {
                    mutation.readback_error("configuration update readback is pending", error)
                })?;
            if readback != desired {
                return Err(mutation.pending_error(
                    "configuration readback differs from the complete submitted form",
                ));
            }
            mutation.verified()?;
        } else if recovered {
            mutation.verified()?;
        }
        let operation = mutation.id().map(str::to_owned);
        drop(mutation);
        let relationships = self.apply_relationships(Resource::Config, id, doc)?;
        Ok(output::public(
            json!({"id":id,"operation_id":operation,"recovered":recovered,"author_writes":writes,"readback":desired,"relationships":relationships}),
            &["content"],
        ))
    }
}
pub(super) fn backup(row: &Value) -> Result<Value> {
    let mut linked = Vec::new();
    for item in optional_array(row.get("t_Known_plug").cloned().unwrap_or(Value::Null))? {
        linked.push(json!({"mod_id":wire_id(item["id"].clone())?,"mod_name":item.get("name"),"mod_file_id":item.get("mod_file_id"),"mod_version":item.get("mod_version"),"display_name":item.get("display_name"),"updateType":wire_uint(item.get("updateType").cloned().unwrap_or(Value::Null),1)?}));
    }
    let mut roles = Vec::new();
    for account in optional_array(row["wtflist"].clone())? {
        for server in optional_array(account["server"].clone())? {
            for item in optional_array(server["roleList"].clone())? {
                roles.push(json!({"account":account.get("account"),"server":server.get("serverName"),"name":item.get("name"),"role_id":role(item["role_id"].clone())?}));
            }
        }
    }
    Ok(
        json!({"cloud_id":wire_id(pick(row,&["t_id","cloud_id"]))?,"name":row.get("t_name"),"game_version_id":row.get("t_Versionid"),"linked_mods":linked,"unknown_plugins":names(row["t_unKnown_list"].clone())?,"materials":names(row["t_material_list"].clone())?,"fonts":names(row["t_font_list"].clone())?,"roles":roles}),
    )
}

#[cfg(test)]
mod tests;
