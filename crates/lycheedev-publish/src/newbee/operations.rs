use super::*;
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::Mutation,
    provider::Provider,
};
use std::collections::BTreeSet;

pub struct AuthorDelete {
    resource: Resource,
    id: NonZeroU64,
}
pub struct WaChangelogEdit {
    id: NonZeroU64,
    parent: Option<NonZeroU64>,
    content: String,
}
pub struct WaShareCode {
    id: NonZeroU64,
}
impl AuthorDelete {
    pub fn parse(resource: Resource, bytes: &[u8]) -> Result<Self> {
        let value = document(bytes)?;
        input_shape(Provider::Newbee, resource.key(), "delete")?.admit(&value)?;
        Ok(Self {
            resource,
            id: positive(&value["id"], "$.id")?,
        })
    }
    pub fn schema(resource: Resource) -> Value {
        let mut value = input_shape(Provider::Newbee, resource.key(), "delete")
            .unwrap()
            .schema();
        value.as_object_mut().unwrap().remove("x-validation-scope");
        value["properties"]["id"]["minimum"] = json!(1);
        plugin::metadata_schema(&mut value);
        value
    }
}
impl WaChangelogEdit {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value = document(bytes)?;
        input_shape(Provider::Newbee, "wa-changelog", "edit")?.admit(&value)?;
        Ok(Self {
            id: positive(&value["id"], "$.id")?,
            parent: value
                .get("wa_id")
                .map(|value| positive(value, "$.wa_id"))
                .transpose()?,
            content: value["wa_log"].as_str().unwrap_or("").to_owned(),
        })
    }
    pub fn schema() -> Value {
        let mut value = input_shape(Provider::Newbee, "wa-changelog", "edit")
            .unwrap()
            .schema();
        value.as_object_mut().unwrap().remove("x-validation-scope");
        for field in ["id", "wa_id"] {
            value["properties"][field]["minimum"] = json!(1);
        }
        plugin::metadata_schema(&mut value);
        value
    }
}
impl WaShareCode {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value = document(bytes)?;
        input_shape(Provider::Newbee, "wa-share-code", "set")?.admit(&value)?;
        Ok(Self {
            id: positive(&value["module_id"], "$.module_id")?,
        })
    }
    pub fn schema() -> Value {
        let mut value = input_shape(Provider::Newbee, "wa-share-code", "set")
            .unwrap()
            .schema();
        value.as_object_mut().unwrap().remove("x-validation-scope");
        value["properties"]["module_id"]["minimum"] = json!(1);
        plugin::metadata_schema(&mut value);
        value
    }
}
fn positive(value: &Value, path: &str) -> Result<NonZeroU64> {
    value
        .as_u64()
        .and_then(NonZeroU64::new)
        .ok_or_else(|| invalid(path, "identity must be a positive integer"))
}
impl Newbee {
    /// Only complete author pagination establishes deletion absence or backup
    /// reference coverage. Missing totals, repeated IDs and moving cursors fail.
    pub(super) fn author_rows(&mut self, resource: Resource) -> Result<Vec<Value>> {
        let mut all = Vec::new();
        let mut ids = BTreeSet::new();
        let mut total = None;
        let mut offset = 0;
        for page in 1..=10000 {
            let (path, body) = match resource {
                Resource::Plugin => (
                    "/creator/wow/mod/publish_list",
                    json!({"keyword":"","game_version_id":0,"sort_by":"t_last_update","sort_order":"DESC","pagenum":page,"pagesize":100}),
                ),
                Resource::Config => (
                    "/creator/wow/share_config/publish_list",
                    json!({"keyword":"","game_version_id":0,"sort":3,"offset":offset,"pagesize":100}),
                ),
                Resource::Wa => (
                    "/creator/wow/wa/mtg_uc_publish_list",
                    json!({"keyword":"","game_version_id":0,"sort":3,"offset":offset,"pagesize":100}),
                ),
            };
            let value = object(self.creator(path, body)?)?;
            let rows = array(pick(&value, &["list", "items"]))?;
            let declared = pick(&value, &["total", "count"]);
            if declared.is_null() {
                return Err(remote(
                    "complete author pagination requires an explicit total",
                ));
            }
            let declared = wire_uint(declared, 0)?;
            if total.is_some_and(|total| total != declared) {
                return Err(remote("author pagination total changed"));
            }
            total = Some(declared);
            for row in &rows {
                if !ids.insert(wire_id(pick(row, &["t_id", "id"]))?) {
                    return Err(remote("author pagination repeats a resource identity"));
                }
            }
            let count = rows.len();
            all.extend(rows);
            if all.len() as u64 == declared {
                return Ok(all);
            }
            if count == 0 || all.len() as u64 > declared {
                return Err(remote("author pagination has a coverage gap"));
            }
            let next = value
                .get("next_offset")
                .filter(|v| !v.is_null())
                .map(|value| wire_uint(value.clone(), 0))
                .transpose()?
                .unwrap_or(offset + count as u64);
            if !matches!(resource, Resource::Plugin) && next <= offset {
                return Err(remote("author pagination cursor did not advance"));
            }
            offset = next;
        }
        Err(remote("author pagination exceeded its page bound"))
    }
    pub fn delete(&mut self, request: &AuthorDelete) -> Result<Value> {
        let id = request.id.get();
        let resource = request.resource;
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            resource.key(),
            id,
            "delete",
            &json!({"id":id,"confirm":"DELETE"}),
            &self.control,
        )?;
        let recovered = mutation.pending();
        let rows = self.author_rows(resource).map_err(|error| {
            if recovered {
                mutation.readback_error(
                    "original deletion still needs complete author coverage",
                    error,
                )
            } else {
                error
            }
        })?;
        let present = rows
            .iter()
            .any(|row| wire_uint(pick(row, &["t_id", "id"]), 0).is_ok_and(|value| value == id));
        if !present {
            if recovered {
                mutation.verified()?;
            }
            return Ok(
                json!({"id":id,"deleted":true,"already_absent":!recovered,"recovered":recovered,"author_writes":0,"verification":"complete_author_list","operation_id":mutation.id()}),
            );
        }
        if recovered {
            return Err(mutation.pending_error(
                "the original deletion target remains present; no delete was repeated",
            ));
        }
        self.raw(resource, id)?;
        let path = match resource {
            Resource::Plugin => "/creator/wow/mod/remove",
            Resource::Config => "/creator/wow/share_config/delete",
            Resource::Wa => "/creator/wow/wa/delete",
        };
        mutation.prepare(&self.control)?;
        self.post(Origin::NewbeeCreator, path, &json!({"id":id}), true)
            .map_err(|error| mutation.failure(error))?;
        mutation.acknowledged().map_err(|_| {
            mutation.pending_error("deletion was accepted; finish saving its receipt")
        })?;
        let rows = self.author_rows(resource).map_err(|error| {
            mutation.readback_error("deletion absence needs complete author coverage", error)
        })?;
        if rows
            .iter()
            .any(|row| wire_uint(pick(row, &["t_id", "id"]), 0).is_ok_and(|value| value == id))
        {
            return Err(
                mutation.pending_error("deleted resource remains in the complete author list")
            );
        }
        mutation.verified()?;
        Ok(
            json!({"id":id,"deleted":true,"recovered":false,"author_writes":1,"verification":"complete_author_list","operation_id":mutation.id()}),
        )
    }
    fn wa_log(&mut self, parent: u64, id: u64) -> Result<Value> {
        let mut seen = BTreeSet::new();
        for page in 1..=10000 {
            let value = object(self.creator(
                "/creator/wow/wa_log/list",
                json!({"wa_id":parent,"pagenum":page,"pagesize":100}),
            )?)?;
            let rows = array(pick(&value, &["list", "items"]))?;
            let count = rows.len();
            for row in rows {
                let returned = wire_id(pick(&row, &["wa_log_id", "id", "t_id"]))?;
                if !seen.insert(returned) {
                    return Err(remote("WA history repeats a log identity"));
                }
                if row.get("wa_id").is_some_and(|value| {
                    !wire_uint(value.clone(), 0).is_ok_and(|value| value == parent)
                }) {
                    return Err(remote("WA history belongs to another parent"));
                }
                if returned == id {
                    return Ok(row);
                }
            }
            if count < 100 {
                break;
            }
        }
        Err(remote(
            "selected WA log is absent from the requested parent history",
        ))
    }
    pub fn edit_wa_changelog(&mut self, request: &WaChangelogEdit) -> Result<Value> {
        let id = request.id.get();
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "wa-log",
            id,
            "edit",
            &json!({"id":id,"content":request.content}),
            &self.control,
        )?;
        let saved_parent = mutation
            .evidence()
            .and_then(|value| value["wa_id"].as_u64());
        if saved_parent
            .is_some_and(|parent| request.parent.is_some_and(|value| value.get() != parent))
        {
            return Err(invalid(
                "$.wa_id",
                "parent differs from the original version-log context",
            ));
        }
        let parent = saved_parent.or(request.parent.map(NonZeroU64::get));
        let acknowledged = mutation
            .evidence()
            .is_some_and(|value| value["verification"] == "api_acceptance");
        let recovered = mutation.pending() || acknowledged;
        if let Some(parent) = parent {
            let before = self.wa_log(parent, id).map_err(|error| {
                if recovered {
                    mutation.readback_error("original WA log needs its parent history", error)
                } else {
                    error
                }
            })?;
            if changelog_matches(&before, &request.content)? {
                if recovered {
                    mutation.verified_with(json!({"wa_id":parent,"verification":"readback"}))?;
                }
                return Ok(
                    json!({"id":id,"wa_id":parent,"author_writes":0,"recovered":recovered,"verification":"readback","operation_id":mutation.id()}),
                );
            }
            if recovered {
                return Err(mutation.pending_error(
                    "the original WA log text is not visible; no edit was repeated",
                ));
            }
        } else if recovered {
            if acknowledged {
                return Ok(
                    json!({"id":id,"verification":"api_acceptance","readback":"wa_id_required","reused":true,"author_writes":0,"operation_id":mutation.id()}),
                );
            }
            return Err(mutation.pending_error(
                "original WA log outcome is uncertain; supply wa_id for readback recovery",
            ));
        }
        mutation.prepare(&self.control)?;
        mutation.remember(json!({"wa_id":parent}))?;
        self.post(
            Origin::NewbeeCreator,
            "/creator/wow/wa_log/edit",
            &json!({"wa_log_id":id,"content":request.content}),
            true,
        )
        .map_err(|error| mutation.failure(error))?;
        mutation.acknowledged().map_err(|_| {
            mutation.pending_error("WA log edit was accepted; finish saving its receipt")
        })?;
        let verification = if let Some(parent) = parent {
            let row = self
                .wa_log(parent, id)
                .map_err(|error| mutation.readback_error("WA log readback is pending", error))?;
            if !changelog_matches(&row, &request.content)? {
                return Err(mutation
                    .pending_error("WA log readback differs from the submitted complete text"));
            }
            "readback"
        } else {
            "api_acceptance"
        };
        mutation.verified_with(json!({"wa_id":parent,"verification":verification}))?;
        Ok(
            json!({"id":id,"wa_id":parent,"verification":verification,"readback":if parent.is_some(){"verified"}else{"wa_id_required"},"author_writes":1,"operation_id":mutation.id()}),
        )
    }
    pub fn set_wa_share_code(&mut self, request: &WaShareCode) -> Result<Value> {
        let id = request.id.get();
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "wa",
            id,
            "share-code",
            &json!({"gameId":1,"moduleId":id,"moduleType":3}),
            &self.control,
        )?;
        if let Some(value) = mutation
            .evidence()
            .filter(|value| value["readback"] == "resource_identity")
        {
            return Ok(value.clone());
        }
        let mut result = if let Some(value) = mutation.evidence() {
            value.clone()
        } else {
            if mutation.pending() {
                return Err(mutation.pending_error("share-code outcome is uncertain; inspect the original WA in Creator before another change"));
            }
            self.raw(Resource::Wa, id)?;
            mutation.prepare(&self.control)?;
            let value = self
                .post(
                    Origin::NewbeeNext,
                    "/bannerserver/ShareCode/Set",
                    &json!({"gameId":1,"moduleId":id,"moduleType":3}),
                    true,
                )
                .map_err(|error| mutation.failure(error))?;
            mutation.acknowledged().map_err(|_| {
                mutation.pending_error("share-code request was accepted; finish saving its receipt")
            })?;
            let code = if value.is_string() {
                value
            } else {
                pick(&value, &["shareCode", "share_code", "code"])
            };
            let result = output::public(
                json!({"id":id,"operation_id":mutation.id(),"share_code":code,"verification":"api_acceptance","readback":"pending","share_code_verification":"not_available"}),
                &[],
            );
            mutation.remember(result.clone()).map_err(|_| {
                mutation
                    .pending_error("share-code was accepted; finish saving its returned reference")
            })?;
            result
        };
        self.raw(Resource::Wa, id).map_err(|error| {
            mutation.readback_error(
                "accepted share-code request needs its resource identity readback",
                error,
            )
        })?;
        result["readback"] = json!("resource_identity");
        mutation.verified_with(result.clone())?;
        Ok(result)
    }
}
fn changelog_matches(row: &Value, text: &str) -> Result<bool> {
    let value = ["content", "t_content", "wa_log", "changelog"]
        .iter()
        .find_map(|key| row.get(*key))
        .ok_or_else(|| remote("WA log readback has no complete text field"))?;
    if value.is_null() {
        Ok(text.is_empty())
    } else {
        value
            .as_str()
            .map(|value| value == text)
            .ok_or_else(|| remote("WA log text is invalid"))
    }
}

#[cfg(test)]
mod tests;
