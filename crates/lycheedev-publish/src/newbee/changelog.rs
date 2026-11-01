use super::*;
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::Mutation,
    provider::Provider,
};
use std::time::{Duration, Instant};

pub struct PluginChangelogEdit {
    file_id: NonZeroU64,
    changelog: String,
}
impl PluginChangelogEdit {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value = document(bytes)?;
        input_shape(Provider::Newbee, "plugin-changelog", "edit")?.admit(&value)?;
        let file_id = value["file_id"]
            .as_u64()
            .and_then(NonZeroU64::new)
            .ok_or_else(|| invalid("$.file_id", "plugin file ID must be positive"))?;
        let changelog = value["changelog"].as_str().unwrap_or("").to_owned();
        Ok(Self { file_id, changelog })
    }
    pub fn schema() -> Value {
        let mut schema = input_shape(Provider::Newbee, "plugin-changelog", "edit")
            .expect("plugin changelog contract")
            .schema();
        schema.as_object_mut().unwrap().remove("x-validation-scope");
        schema["properties"]["file_id"]["minimum"] = json!(1);
        plugin::metadata_schema(&mut schema);
        schema
    }
    fn matches(&self, value: &Value) -> Result<bool> {
        let value = object(value.clone())?;
        if wire_id(pick(&value, &["file_id"]))? != self.file_id.get() {
            return Err(remote(
                "plugin changelog response belongs to a different file",
            ));
        }
        let actual = value
            .get("changelog")
            .ok_or_else(|| remote("plugin changelog response is missing its content field"))?;
        match actual {
            Value::Null => Ok(self.changelog.is_empty()),
            Value::String(text) => Ok(text == &self.changelog),
            _ => Err(remote(
                "plugin changelog response content has an invalid type",
            )),
        }
    }
}
impl Newbee {
    pub fn edit_plugin_changelog(&mut self, edit: &PluginChangelogEdit) -> Result<Value> {
        let body = json!({"file_id":edit.file_id,"changelog":edit.changelog});
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "plugin-changelog",
            edit.file_id.get(),
            "edit",
            &body,
            &self.control,
        )?;
        let path = "/creator/wow/mod_file/get_changelog";
        let before = self
            .creator(path, json!({"file_id":edit.file_id}))
            .map_err(|error| {
                if mutation.pending() {
                    mutation
                        .readback_error("the original changelog edit still needs readback", error)
                } else {
                    error
                }
            })?;
        let matches = edit.matches(&before).map_err(|error| {
            if mutation.pending() {
                mutation.readback_error(
                    "the original changelog edit still needs a valid readback",
                    error,
                )
            } else {
                error
            }
        })?;
        if matches {
            let recovered = mutation.pending();
            if recovered {
                mutation.verified()?;
            }
            return Ok(output::public(
                json!({"operation_id":if recovered{mutation.id()}else{None},"file_id":edit.file_id,"changed":false,"recovered":recovered,"author_writes":0,"verification":"readback","readback":before}),
                &[],
            ));
        }
        if mutation.pending() {
            return Err(mutation
                .pending_error("the original changelog edit is not verified; no edit was resent"));
        }
        mutation.prepare(&self.control)?;
        if let Err(error) = self.post(
            Origin::NewbeeCreator,
            "/creator/wow/mod_file/edit_changelog",
            &body,
            true,
        ) {
            return Err(mutation.failure(error));
        }
        mutation.acknowledged().map_err(|_| {
            mutation.pending_error("changelog edit was acknowledged; finish saving its receipt")
        })?;
        // This endpoint is observed to lag a successful edit. Repeat these
        // bounded reads only; the edit request is never retried.
        for attempt in 0..4 {
            let readback = self
                .creator(path, json!({"file_id":edit.file_id}))
                .map_err(|error| {
                    mutation.readback_error(
                        "changelog edit was acknowledged; readback is pending",
                        error,
                    )
                })?;
            if edit.matches(&readback).map_err(|error| {
                mutation.readback_error(
                    "changelog edit was acknowledged; its readback is invalid",
                    error,
                )
            })? {
                mutation.verified().map_err(|_| {
                    mutation
                        .pending_error("changelog readback is verified; finish saving its receipt")
                })?;
                return Ok(output::public(
                    json!({"operation_id":mutation.id(),"file_id":edit.file_id,"changed":true,"recovered":false,"author_writes":1,"verification":"readback","read_attempts":attempt+1,"readback":readback}),
                    &[],
                ));
            }
            if attempt < 3 {
                let until = Instant::now() + Duration::from_millis(250 * (1 << attempt));
                while Instant::now() < until {
                    self.control.check().map_err(|error| {
                        mutation.readback_error(
                            "changelog edit was acknowledged; readback was interrupted",
                            error,
                        )
                    })?;
                    std::thread::sleep(
                        Duration::from_millis(20)
                            .min(until.saturating_duration_since(Instant::now())),
                    );
                }
            }
        }
        Err(mutation
            .pending_error("changelog edit is acknowledged but its exact text is not yet verified"))
    }
}
