//! Explicit SN deletion. An uncertain original POST is recovered only by a
//! complete author-list read; it never permits another delete submission.
use super::{
    read::{self, Attached, Reference, Resource},
    task::Client,
};
use crate::{contract, input, mutation::Mutation, provider::Provider};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{num::NonZeroU64, path::Path};

pub struct Delete {
    resource: Resource,
    reference: Reference,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    resource: Resource,
    reference: String,
    game_type: NonZeroU64,
    before: Value,
    deleted: bool,
}
fn data(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_delete", message, 5)
}
trait Api: read::Api {
    fn delete(&mut self, path: &str, body: &Value) -> Result<Value>;
}
impl Api for Attached<'_> {
    fn delete(&mut self, path: &str, body: &Value) -> Result<Value> {
        self.client.post(path, &[], body, true, self.control)
    }
}
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) if n.as_u64().is_some() => Some(n.to_string()),
        _ => None,
    }
}
fn game(value: &Value) -> Option<NonZeroU64> {
    let selected = value
        .get("game_type")
        .filter(|v| {
            !v.is_null()
                && v.as_str() != Some("")
                && v.as_u64() != Some(0)
                && v.as_bool() != Some(false)
        })
        .or_else(|| value.get("game_types")?.as_array()?.first())?;
    selected
        .as_u64()
        .or_else(|| selected.as_str()?.parse().ok())
        .and_then(NonZeroU64::new)
}
fn present(rows: &[Value], reference: &str) -> Result<bool> {
    let mut found = false;
    for row in rows {
        let ids = ["sn", "share_sn", "id"]
            .iter()
            .filter_map(|k| row.get(*k))
            .filter(|v| !v.is_null() && v.as_str() != Some(""))
            .map(|v| text(v).ok_or_else(|| data("DD author row has an invalid resource identity")))
            .collect::<Result<Vec<_>>>()?;
        let primary = ids
            .first()
            .ok_or_else(|| data("DD author row omitted its resource identity"))?;
        if primary == reference {
            found = true;
        } else if ids.iter().any(|id| id == reference) {
            return Err(data(
                "DD author row contradicts the selected resource identity",
            ));
        }
    }
    Ok(found)
}
impl Delete {
    pub fn parse(resource: Resource, bytes: &[u8]) -> Result<Self> {
        let value = input::document(bytes)?;
        contract::input_shape(Provider::Dd, resource.name(), "delete")?.admit(&value)?;
        Ok(Self {
            resource,
            reference: Reference::parse(value["sn"].as_str().unwrap())
                .map_err(|error| input::invalid("$.sn", &error.message))?,
        })
    }
    pub fn schema(resource: Resource) -> Value {
        let mut value = contract::input_shape(Provider::Dd, resource.name(), "delete")
            .unwrap()
            .schema();
        value.as_object_mut().unwrap().remove("x-validation-scope");
        value["properties"]["sn"]["maxLength"] = json!(1024);
        value["properties"]["sn"]["description"] = json!(
            "Exact opaque DD resource SN, at most 1024 UTF-8 bytes without control characters; no guessed reference or numeric conversion."
        );
        value
    }
    pub fn plan(&self) -> Value {
        json!({"platform":"dd","resource":self.resource,"action":"delete","reference":self.reference.as_str(),"confirm_delete":true,
            "remote_write":false,"remote_read":false,"credential_reads":false,
            "execution_requires":["the selected retained DDT author","fresh ownership and game type","complete author-list absence readback"],"uncertain_submission_replay":false})
    }
    pub fn execute(
        &self,
        home: &Path,
        client: &Client,
        control: &OperationControl,
    ) -> Result<Value> {
        control.check()?;
        let identity = client.author_identity(control)?;
        let author = identity
            .pointer("/author/author_id")
            .and_then(Value::as_str)
            .ok_or_else(|| data("the retained DD task omitted its original author identity"))?;
        self.execute_with(home, author, &mut Attached { client, control }, control)
    }
    fn execute_with(
        &self,
        home: &Path,
        author: &str,
        api: &mut impl Api,
        control: &OperationControl,
    ) -> Result<Value> {
        control.check()?;
        if author.trim().is_empty() || author.len() > 1024 || author.chars().any(char::is_control) {
            return Err(data("invalid original DD author identity"));
        }
        let reference = self.reference.as_str();
        let identity =
            json!({"author":author,"resource":self.resource,"sn":reference,"confirm_delete":true});
        let target = super::forms::target(author, self.resource, reference)?;
        let mut mutation = Mutation::open(
            home,
            Provider::Dd,
            self.resource.name(),
            target,
            "delete",
            &identity,
            control,
        )?;
        let original = mutation
            .evidence()
            .map(|value| {
                serde_json::from_value::<Context>(value.clone())
                    .map_err(|_| ToolkitError::invalid("invalid original DD deletion context"))
            })
            .transpose()?;
        if let Some(context) = &original {
            if context.resource != self.resource
                || context.reference != reference
                || !context.before.is_object()
            {
                return Err(ToolkitError::invalid(
                    "DD deletion context differs from its original target",
                ));
            }
        }
        if let Some(context) = original
            .as_ref()
            .filter(|context| mutation.pending() || context.deleted)
        {
            let rows = read::all_author(api, self.resource, context.game_type, control).map_err(
                |error| mutation.readback_error("original DD deletion readback is pending", error),
            )?;
            if !present(&rows, reference).map_err(|error| {
                mutation.readback_error("original DD deletion readback is ambiguous", error)
            })? {
                let value = self.completed(&mutation, context, true);
                if mutation.pending() {
                    let context = Context {
                        resource: context.resource,
                        reference: context.reference.clone(),
                        game_type: context.game_type,
                        before: context.before.clone(),
                        deleted: true,
                    };
                    // The original context survives publication before the POST;
                    // completing its state adds no new author mutation.
                    mutation.verified_with(serde_json::to_value(context)?)?;
                }
                return Ok(value);
            }
            if mutation.pending() {
                return Err(mutation.pending_error("the original DD target is still present; deletion is unverified and will not be submitted again"));
            }
        } else if mutation.pending() {
            return Err(mutation.pending_error(
                "retain the original DD deletion while its recovery context is incomplete",
            ));
        }
        let before = read::detail(api, self.resource, &self.reference)?;
        if before.get("is_owner") == Some(&Value::Bool(false)) {
            return Err(data(
                "the selected DD resource is not owned by the retained author",
            ));
        }
        let game_type = match game(&before) {
            Some(value) => value,
            None if self.resource == Resource::Config => {
                let backup = before.get("backup_sn").and_then(text).ok_or_else(|| {
                    data("DD configuration omitted its game type and backup reference")
                })?;
                let raw = api.get("/backup/detail", &[("sn", backup.clone())])?;
                let value = raw
                    .get("result")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| data("DD backup dependency omitted its detail"))?;
                if value.get("sn").and_then(text).as_deref() != Some(backup.as_str()) {
                    return Err(data(
                        "DD backup dependency differs from the selected configuration",
                    ));
                }
                game(value).ok_or_else(|| data("DD backup dependency omitted a valid game type"))?
            }
            None => return Err(data("DD target omitted a valid game type before deletion")),
        };
        let rows = read::all_author(api, self.resource, game_type, control)?;
        if before.get("is_owner") != Some(&Value::Bool(true)) && !present(&rows, reference)? {
            return Err(data(
                "DD target ownership is unverified in its complete author list",
            ));
        }
        let context = Context {
            resource: self.resource,
            reference: reference.into(),
            game_type,
            before: read::public_resource(self.resource, before)?,
            deleted: false,
        };
        mutation.prepare_with_evidence(serde_json::to_value(&context)?, control)?;
        let path = match self.resource {
            Resource::Plugin => "/addon/delete",
            Resource::Config => "/share/delete",
            Resource::Wa => "/wa/delete",
        };
        api.delete(path, &json!({"sn":reference}))
            .map_err(|error| mutation.failure(error))?;
        mutation.acknowledged().map_err(|error| {
            mutation.readback_error(
                "DD deletion was accepted; finish saving its original acknowledgement",
                error,
            )
        })?;
        let rows = read::all_author(api, self.resource, game_type, control).map_err(|error| {
            mutation.readback_error(
                "DD deletion was accepted; complete author-list readback is pending",
                error,
            )
        })?;
        if present(&rows, reference)
            .map_err(|error| mutation.readback_error("DD deletion readback is ambiguous", error))?
        {
            return Err(mutation.pending_error(
                "DD deletion was accepted but the target remains in its complete author list",
            ));
        }
        let value = self.completed(&mutation, &context, false);
        let context = Context {
            deleted: true,
            ..context
        };
        mutation
            .verified_with(serde_json::to_value(context)?)
            .map_err(|error| {
                mutation.readback_error(
                    "DD target is absent; finish saving its original deletion receipt",
                    error,
                )
            })?;
        Ok(value)
    }
    fn completed(&self, mutation: &Mutation, context: &Context, recovered: bool) -> Value {
        json!({"resource":self.resource,"reference":self.reference.as_str(),"deleted":true,"before":context.before,
            "game_type":context.game_type,"operation_id":mutation.id(),"verification":"complete_author_list_absence","recovered":recovered,
            "readback":{"present":false,"complete_author_list":true},"submitted_this_call":!recovered,"replay_performed":false})
    }
}

#[cfg(test)]
mod tests;
