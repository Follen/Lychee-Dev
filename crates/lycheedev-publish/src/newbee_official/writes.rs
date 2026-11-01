use super::*;
use crate::{input::invalid, mutation::Mutation, provider::Provider};
pub struct ReviewReply {
    id: NonZeroU64,
    content: String,
}
pub struct ChangelogEdit {
    file_id: NonZeroU64,
    changelog: String,
}
fn parsed<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let doc = input::document(bytes)?;
    serde_json::from_value(json!(doc)).map_err(|_| {
        invalid(
            "$",
            "official author input differs from its declared schema",
        )
    })
}
impl ReviewReply {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Doc {
            schema: String,
            review_id: NonZeroU64,
            content: String,
        }
        let doc: Doc = parsed(bytes)?;
        if doc.schema != "lycheedev.publish.v1.newbee.official.review.reply" {
            return Err(invalid(
                "$.schema",
                "select the official review reply schema",
            ));
        }
        if doc.content.trim().is_empty() {
            return Err(invalid("$.content", "review reply must not be blank"));
        }
        Ok(Self {
            id: doc.review_id,
            content: doc.content,
        })
    }
    pub fn schema() -> Value {
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"schema":{"const":"lycheedev.publish.v1.newbee.official.review.reply"},"review_id":{"type":"integer","minimum":1,"description":"Global review ID from the official received-review list."},"content":{"type":"string","minLength":1,"description":"Reply text. API acceptance enters platform review; it does not prove public visibility."}},"required":["schema","review_id","content"],"additionalProperties":false})
    }
    pub fn plan(&self) -> Value {
        json!({"provider":"newbee_official","review_id":self.id,"content_summary":{"length":self.content.chars().count(),"sha256":storage::sha256(self.content.as_bytes())},"credential_reads":false,"author_writes":0,"remote_validation":"not_run"})
    }
}
impl ChangelogEdit {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Doc {
            schema: String,
            file_id: NonZeroU64,
            changelog: String,
        }
        let doc: Doc = parsed(bytes)?;
        if doc.schema != "lycheedev.publish.v1.newbee.official.plugin-changelog.edit" {
            return Err(invalid(
                "$.schema",
                "select the official plugin changelog edit schema",
            ));
        }
        Ok(Self {
            file_id: doc.file_id,
            changelog: doc.changelog,
        })
    }
    pub fn schema() -> Value {
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"schema":{"const":"lycheedev.publish.v1.newbee.official.plugin-changelog.edit"},"file_id":{"type":"integer","minimum":1},"changelog":{"type":"string","description":"Complete new changelog; an explicit empty string clears it."}},"required":["schema","file_id","changelog"],"additionalProperties":false})
    }
    pub fn plan(&self) -> Value {
        json!({"provider":"newbee_official","file_id":self.file_id,"changelog_summary":{"length":self.changelog.chars().count(),"sha256":storage::sha256(self.changelog.as_bytes())},"credential_reads":false,"author_writes":0,"remote_validation":"not_run"})
    }
}
impl Official {
    pub fn reply_review(&mut self, request: &ReviewReply) -> Result<Value> {
        let author = self.author()?;
        let identity = json!({"author":author,"review_id":request.id,"content_sha256":storage::sha256(request.content.as_bytes())});
        let hash = storage::sha256(&storage::identity_json(&identity)?);
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "official-review-reply",
            u64::from_str_radix(&hash[..16], 16).unwrap().max(1),
            "reply",
            &identity,
            &self.control,
        )?;
        if let Some(value) = mutation
            .evidence()
            .filter(|value| value["api_accepted"] == true)
        {
            let mut value = value.clone();
            value["reused"] = json!(true);
            value["author_writes"] = json!(0);
            return Ok(value);
        }
        if mutation.pending() {
            return Err(mutation.pending_error("the original review reply outcome is uncertain; inspect the official received-review list without sending another reply"));
        }
        mutation.prepare(&self.control)?;
        let raw = self
            .call(
                "/content_review/reply",
                &json!({"review_id":request.id,"content":request.content}),
                true,
            )
            .map_err(|error| mutation.failure(error))?;
        let mut result = json!({"review_id":request.id,"operation_id":mutation.id(),"api_accepted":true,"verification":"api_acceptance","effect_verification":"not_available","publication_process":"platform_review","author_writes":1,"reused":false});
        if let Some(value) = raw.get("reply_id").or_else(|| raw.get("id")) {
            if let Ok(id) = positive(value) {
                result["reply_id"] = json!(id);
            }
        }
        mutation.verified_with(result.clone()).map_err(|_| {
            mutation.pending_error(
                "review reply was accepted; finish saving its original acknowledgement",
            )
        })?;
        Ok(result)
    }
    pub fn edit_changelog(&mut self, request: &ChangelogEdit) -> Result<Value> {
        let author = self.author()?;
        let identity =
            json!({"author":author,"file_id":request.file_id,"changelog":request.changelog});
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "official-plugin-file",
            request.file_id.get(),
            "changelog-edit",
            &identity,
            &self.control,
        )?;
        let recovered = mutation.pending();
        let read = self.changelog(request.file_id).map_err(|error| {
            if recovered {
                mutation.readback_error("original official changelog readback is pending", error)
            } else {
                error
            }
        })?;
        if read == request.changelog {
            if recovered {
                mutation.verified()?;
            }
            return Ok(
                json!({"file_id":request.file_id,"operation_id":mutation.id(),"verification":"requested_file_changelog","recovered":recovered,"changed":false,"author_writes":0}),
            );
        }
        if recovered {
            return Err(mutation.pending_error("the original official changelog has not appeared in readback; the edit was not repeated"));
        }
        mutation.prepare(&self.control)?;
        self.call(
            "/mod_file/edit_changelog",
            &json!({"file_id":request.file_id,"changelog":request.changelog}),
            true,
        )
        .map_err(|error| mutation.failure(error))?;
        mutation.acknowledged().map_err(|_| {
            mutation.pending_error(
                "official changelog edit was accepted; finish saving its original acknowledgement",
            )
        })?;
        let read = self.changelog(request.file_id).map_err(|error| {
            mutation.readback_error("official changelog readback is pending", error)
        })?;
        if read != request.changelog {
            return Err(mutation.pending_error(
                "official changelog readback differs from the original requested text",
            ));
        }
        mutation.verified()?;
        Ok(
            json!({"file_id":request.file_id,"operation_id":mutation.id(),"verification":"requested_file_changelog","recovered":false,"changed":true,"author_writes":1}),
        )
    }
    fn changelog(&mut self, id: NonZeroU64) -> Result<String> {
        let raw = self.call("/mod_file/get_changelog", &json!({"file_id":id}), false)?;
        if raw
            .get("file_id")
            .is_some_and(|value| !positive(value).is_ok_and(|value| value == id.get()))
        {
            return Err(data("official changelog readback identifies another file"));
        }
        if let Some(value) = raw.as_str() {
            return Ok(value.to_owned());
        }
        match raw.get("changelog") {
            Some(Value::String(value)) => Ok(value.clone()),
            Some(Value::Null) => Ok(String::new()),
            _ => Err(data("official changelog readback omitted its text")),
        }
    }
}
#[cfg(test)]
mod tests;
