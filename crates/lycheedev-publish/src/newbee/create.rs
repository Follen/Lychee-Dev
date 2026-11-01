use super::*;
use super::{
    config::{commercial, linked_mods, linked_schema},
    plugin::{
        admit_metadata, embedded_relationships, form_hash, relationship_schemas, review_schema,
        verify_pending_form,
    },
    wa_write::{attachment_schema, attachment_values},
};
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::Mutation,
    provider::Provider,
};

pub struct AuthorCreate {
    resource: Resource,
    doc: Value,
    cwd: PathBuf,
}
impl AuthorCreate {
    pub fn parse(resource: Resource, bytes: &[u8], cwd: &Path) -> Result<Self> {
        let doc = document(bytes)?;
        input_shape(Provider::Newbee, resource.key(), "create")?.admit(&doc)?;
        let doc = Value::Object(doc);
        admit_metadata(&doc, cwd)?;
        embedded_relationships(resource, 1, &doc)?;
        for field in ["cloud_id", "game_version_id"] {
            if let Some(value) = doc.get(field) {
                if !value.as_u64().is_some_and(|n| n > 0) {
                    return Err(invalid(&format!("$.{field}"), "identity must be positive"));
                }
            }
        }
        let alternatives: &[(&str, &str)] = match resource {
            Resource::Plugin => &[("logo", "logo_file"), ("screenshots", "screenshot_files")],
            Resource::Config => &[("picture_urls", "picture_files")],
            Resource::Wa => &[("thumbnail", "thumbnail_file")],
        };
        for (existing, local) in alternatives {
            let nonempty = |value: Option<&Value>| {
                value.is_some_and(|value| {
                    value.as_str().is_some_and(|v| !v.is_empty())
                        || value.as_array().is_some_and(|v| !v.is_empty())
                })
            };
            if !nonempty(doc.get(*existing)) && !nonempty(doc.get(*local)) {
                return Err(invalid(
                    &format!("$.{existing}"),
                    "provide an existing media value or at least one local media file",
                ));
            }
        }
        if matches!(resource, Resource::Config) {
            linked_mods(doc["linked_mods"].clone(), true)?;
        }
        if let Some(value) = doc.get("attachments") {
            attachment_values(value.clone(), true)?;
        }
        if doc["string_mode"] == "collection"
            && !doc["wa_str_titles"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        {
            return Err(invalid(
                "$.wa_str_titles",
                "collection mode requires explicit string titles",
            ));
        }
        Ok(Self {
            resource,
            doc,
            cwd: cwd.to_owned(),
        })
    }
    pub fn schema(resource: Resource) -> Value {
        let mut schema = input_shape(Provider::Newbee, resource.key(), "create")
            .unwrap()
            .schema();
        schema.as_object_mut().unwrap().remove("x-validation-scope");
        let props = &mut schema["properties"];
        for field in ["cloud_id", "game_version_id"] {
            if props.get(field).is_some() {
                props[field]["minimum"] = json!(1);
            }
        }
        for field in ["mod_categories", "category_id_list"] {
            if props.get(field).is_some() {
                props[field]["items"] = json!({"type":"integer","minimum":1});
                props[field]["uniqueItems"] = json!(true);
            }
        }
        for field in [
            "screenshots",
            "screenshot_files",
            "picture_urls",
            "picture_files",
            "images",
            "image_files",
            "ignored_unknown_mods",
            "ignored_materials",
            "ignored_fronts",
            "wa_str_titles",
        ] {
            if props.get(field).is_some() {
                props[field]["items"] = json!({"type":"string","minLength":1});
            }
        }
        if props.get("linked_mods").is_some() {
            props["linked_mods"]["items"] = linked_schema();
        }
        if props.get("attachments").is_some() {
            props["attachments"]["items"] = attachment_schema();
        }
        relationship_schemas(props);
        schema["allOf"] = review_schema();
        for (existing, local, array) in match resource {
            Resource::Plugin => vec![
                ("logo", "logo_file", false),
                ("screenshots", "screenshot_files", true),
            ],
            Resource::Config => vec![("picture_urls", "picture_files", true)],
            Resource::Wa => vec![("thumbnail", "thumbnail_file", false)],
        } {
            let mut existing_shape = json!({"required":[existing]});
            existing_shape["properties"] = json!({});
            existing_shape["properties"][existing] = if array {
                json!({"minItems":1})
            } else {
                json!({"minLength":1})
            };
            let mut local_shape = json!({"required":[local]});
            local_shape["properties"] = json!({});
            local_shape["properties"][local] = if local.ends_with("_files") {
                json!({"minItems":1})
            } else {
                json!({"minLength":1})
            };
            schema["allOf"]
                .as_array_mut()
                .unwrap()
                .push(json!({"anyOf":[existing_shape,local_shape]}));
        }
        if matches!(resource, Resource::Wa) {
            schema["allOf"].as_array_mut().unwrap().push(json!({"if":{"properties":{"string_mode":{"const":"collection"}},"required":["string_mode"]},"then":{"required":["wa_str_titles"],"properties":{"wa_str_titles":{"minItems":1}}}}));
        }
        plugin::metadata_schema(&mut schema);
        schema
    }
}
impl Newbee {
    /// Recovery selects the already created remote ID explicitly. A title
    /// match never authorizes adopting another project or sending create again.
    pub fn create(
        &mut self,
        request: &AuthorCreate,
        recover_id: Option<NonZeroU64>,
    ) -> Result<Value> {
        let resource = request.resource;
        let doc = &request.doc;
        let identity = media::request_identity(doc, &request.cwd, &self.control)?;
        let identity = json!({"author_subject":self.subject()?,"request":identity});
        let identity_hash = plugin::form_hash(&identity)?;
        let scope = u64::from_str_radix(&identity_hash[..16], 16)
            .unwrap()
            .max(1);
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            &format!("{}-create", resource.key()),
            scope,
            "create",
            &identity,
            &self.control,
        )?;
        if let Some(evidence) = mutation
            .evidence()
            .filter(|value| value["verification"] == "metadata_readback")
        {
            let result = evidence.clone();
            let id = result["id"].as_u64().unwrap();
            if recover_id.is_some_and(|value| value.get() != id) {
                return Err(invalid(
                    "$.id",
                    "recovery ID differs from the original creation receipt",
                ));
            }
            drop(mutation);
            let relationships = self.apply_relationships(resource, id, doc)?;
            let mut result = result;
            result["reused"] = json!(true);
            result["relationships"] = json!(relationships);
            return Ok(result);
        }
        let recovered = mutation.pending();
        if !recovered && recover_id.is_some() {
            return Err(ToolkitError::arguments(
                "--id recovers an original pending creation; omit it for a new creation",
            ));
        }
        let id = if recovered {
            let saved = mutation
                .evidence()
                .and_then(|value| value["created_id"].as_u64());
            if saved.is_some_and(|id| recover_id.is_some_and(|value| value.get() != id)) {
                return Err(invalid(
                    "$.id",
                    "recovery ID differs from the acknowledged creation",
                ));
            }
            saved.or(recover_id.map(NonZeroU64::get)).ok_or_else(||mutation.pending_error("creation outcome is pending; read the author list and recover the original input with its explicit --id"))?
        } else {
            let body = self.create_form(request)?;
            let endpoint = match resource {
                Resource::Plugin => "/creator/wow/mod/create",
                Resource::Config => "/creator/wow/share_config/release",
                Resource::Wa => "/creator/wow/wa/publish",
            };
            let expected = creation_compare(resource, body.clone());
            let expected_hash = form_hash(&expected)?;
            mutation.prepare(&self.control)?;
            mutation.remember(json!({"expected_form_sha256":expected_hash}))?;
            let value = self
                .post(Origin::NewbeeCreator, endpoint, &body, true)
                .map_err(|error| mutation.failure(error))?;
            mutation.acknowledged().map_err(|_| {
                mutation.pending_error("creation was accepted; finish saving the original receipt")
            })?;
            let value = if value.is_number() {
                value
            } else {
                pick(
                    &object(value).unwrap_or(Value::Null),
                    &["id", "t_id", "mod_id", "wa_id"],
                )
            };
            let id=wire_id(value).map_err(|_|mutation.pending_error("creation was accepted without a resource ID; select its explicit author-list ID before recovery"))?;
            mutation
                .remember(json!({"expected_form_sha256":expected_hash,"created_id":id}))
                .map_err(|_| {
                    mutation.pending_error(
                        "creation was accepted; finish saving its returned resource ID",
                    )
                })?;
            id
        };
        let raw = self.raw(resource, id).map_err(|error| {
            mutation.readback_error("creation needs the original resource readback", error)
        })?;
        let actual = match resource {
            Resource::Plugin => plugin::form(&raw)?,
            Resource::Config => config::form(&raw)?,
            Resource::Wa => wa_write::form(&raw)?,
        };
        verify_pending_form(&mutation, &creation_compare(resource, actual))?;
        let mut content_verification = "not_applicable";
        if matches!(resource, Resource::Wa) {
            let latest = object(
                self.creator("/creator/wow/wa_log/latest_str_info", json!({"wa_id":id}))
                    .map_err(|error| {
                        mutation
                            .readback_error("created WA needs its initial content readback", error)
                    })?,
            )?;
            for field in ["wa_id", "module_id"] {
                if let Some(value) = latest.get(field) {
                    if wire_id(value.clone())? != id {
                        return Err(mutation.pending_error(
                            "created WA content readback belongs to another resource",
                        ));
                    }
                }
            }
            let content = pick(&latest, &["wa_str", "t_wa_str", "str"]);
            content_verification = if content.is_null() {
                "not_available"
            } else {
                if content != doc["wa_str"] {
                    return Err(mutation
                        .pending_error("created WA content differs from the original input"));
                }
                "readback"
            };
        }
        let actual_public = if matches!(resource, Resource::Plugin) {
            false
        } else {
            doc["public"] == true
        };
        let result = json!({"id":id,"resource":resource.key(),"operation_id":mutation.id(),"verification":"metadata_readback","wa_content_verification":content_verification,"recovered":recovered,"public":actual_public,"review_intent":doc["public"]==true,"public_after_first_version":matches!(resource,Resource::Plugin)&&doc["public"]==true,"review_status":"not_checked"});
        mutation.verified_with(result.clone())?;
        drop(mutation);
        let relationships = self.apply_relationships(resource, id, doc)?;
        let mut result = result;
        result["relationships"] = json!(relationships);
        Ok(output::public(result, &[]))
    }
    fn create_form(&mut self, request: &AuthorCreate) -> Result<Value> {
        let doc = &request.doc;
        let resource = request.resource;
        let mut body = match resource {
            Resource::Plugin => {
                self.creator("/creator/wow/mod/permission_check", json!({}))?;
                self.categories(&doc["mod_categories"])?;
                let logo = if let Some(file) = doc.get("logo_file") {
                    self.image(resource, &request.cwd.join(file.as_str().unwrap()), true)?["url"]
                        .clone()
                } else {
                    doc["logo"].clone()
                };
                let screenshots = self.resolve_images(
                    resource,
                    doc.get("screenshots").unwrap_or(&Value::Null),
                    doc.get("screenshot_files"),
                    &request.cwd,
                    true,
                )?;
                json!({"mod_categories":doc["mod_categories"],"content_origin":doc["content_origin"],"content_format":doc["content_format"],"name":doc["name"],"description":doc["description"],"intro":doc["intro"],"logo":logo,"screenshots":screenshots,"share_state":0,"subscribe_plan_level":doc.get("subscribe_plan_level").cloned().unwrap_or(json!(0)),"link_to_channel":false})
            }
            Resource::Config => {
                let pictures = self.resolve_images(
                    resource,
                    doc.get("picture_urls").unwrap_or(&Value::Null),
                    doc.get("picture_files"),
                    &request.cwd,
                    true,
                )?;
                let mut body = json!({"cloud_id":doc["cloud_id"],"title":doc["title"],"content":doc["content"],"content_format":doc["content_format"],"intro":doc.get("intro").cloned().unwrap_or(json!("")),"pic_url":pictures,"content_origin":doc["content_origin"],"sharing":if doc["public"]==true{1}else{0},"link_to_channel":doc.get("link_to_channel").cloned().unwrap_or(json!(false)),"subscribe_plan_level":doc.get("subscribe_plan_level").cloned().unwrap_or(json!(0)),"price":doc.get("price").cloned().unwrap_or(json!(0)),"time_range":doc.get("time_range").cloned().unwrap_or(json!("")),"linked_mods":doc["linked_mods"],"ignored_unknown_mods":doc["ignored_unknown_mods"],"ignored_materials":doc["ignored_materials"],"ignored_fronts":doc["ignored_fronts"],"roleid":doc["roleid"]});
                self.config_selection(&mut body)?;
                body
            }
            Resource::Wa => {
                let thumbnail = if let Some(file) = doc.get("thumbnail_file") {
                    self.image(resource, &request.cwd.join(file.as_str().unwrap()), true)?["url"]
                        .clone()
                } else {
                    doc["thumbnail"].clone()
                };
                let images = self.resolve_images(
                    resource,
                    doc.get("images").unwrap_or(&Value::Null),
                    doc.get("image_files"),
                    &request.cwd,
                    true,
                )?;
                let body = json!({"game_version_id":doc["game_version_id"],"name":doc["name"],"intro":doc.get("intro").cloned().unwrap_or(json!("")),"description":doc["description"],"content_format":doc["content_format"],"thumbnail":thumbnail,"images":images,"category_id_list":doc["category_id_list"],"content_origin":doc["content_origin"],"subscribe_plan_level":doc.get("subscribe_plan_level").cloned().unwrap_or(json!(0)),"price":doc.get("price").cloned().unwrap_or(json!(0)),"time_range":doc.get("time_range").cloned().unwrap_or(json!("")),"share_state":if doc["public"]==true{1}else{2},"link_to_channel":doc.get("link_to_channel").cloned().unwrap_or(json!(false)),"attachments":attachment_values(doc.get("attachments").cloned().unwrap_or(Value::Null),true)?,"wa_str":doc["wa_str"],"wa_str_titles":doc.get("wa_str_titles").cloned().unwrap_or(json!([])),"wa_log":doc["wa_log"],"string_mode":doc["string_mode"]});
                self.wa_choices(&body)?;
                self.attachment_choices(&body["attachments"])?;
                body
            }
        };
        commercial(
            &mut body,
            doc["public"] == true && !matches!(resource, Resource::Plugin),
        );
        self.business_options(&body)?;
        Ok(body)
    }
}
fn creation_compare(resource: Resource, mut value: Value) -> Value {
    let map = value.as_object_mut().unwrap();
    map.remove("id");
    map.remove("tid");
    if matches!(resource, Resource::Config) {
        map.remove("selected_eml");
    }
    if matches!(resource, Resource::Wa) {
        for key in ["wa_log", "wa_str", "wa_str_titles", "string_mode"] {
            map.remove(key);
        }
    }
    value
}

#[cfg(test)]
mod tests;
