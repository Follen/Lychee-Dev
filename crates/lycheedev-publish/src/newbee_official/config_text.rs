use super::*;
use crate::{input::invalid, mutation::Mutation, provider::Provider};

pub struct ConfigTextUpdate {
    id: NonZeroU64,
    file: Option<PathBuf>,
    title: Option<String>,
    intro: Option<String>,
}
impl ConfigTextUpdate {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Doc {
            schema: String,
            id: NonZeroU64,
            file: Option<String>,
            title: Option<String>,
            intro: Option<String>,
        }
        let raw = input::document(bytes)?;
        for key in ["file", "title", "intro"] {
            if raw.get(key).is_some_and(Value::is_null) {
                return Err(invalid(
                    &format!("$.{key}"),
                    "omit a field to preserve it; null is not text",
                ));
            }
        }
        let doc: Doc = serde_json::from_value(json!(raw)).map_err(|_| {
            invalid(
                "$",
                "official configuration text input differs from its declared schema",
            )
        })?;
        if doc.schema != "lycheedev.publish.v1.newbee.official.config.update" {
            return Err(invalid(
                "$.schema",
                "select the official configuration text update schema",
            ));
        }
        if doc.file.is_none() && doc.title.is_none() && doc.intro.is_none() {
            return Err(invalid("$", "select at least one of file, title or intro"));
        }
        if doc
            .title
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(invalid("$.title", "configuration title must not be blank"));
        }
        let file = doc.file.map(|value| cwd.join(value));
        if file.as_ref().is_some_and(|path| !path.is_file()) {
            return Err(invalid("$.file", "select an existing UTF-8 Markdown file"));
        }
        Ok(Self {
            id: doc.id,
            file,
            title: doc.title,
            intro: doc.intro,
        })
    }
    pub fn schema() -> Value {
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"schema":{"const":"lycheedev.publish.v1.newbee.official.config.update"},"id":{"type":"integer","minimum":1},"file":{"type":"string","minLength":1,"description":"Nonempty UTF-8 Markdown file, at most 32 MiB. HTML bodies use the Creator interface. Relative paths resolve from the invocation directory."},"title":{"type":"string","minLength":1},"intro":{"type":"string","description":"Explicit summary text; empty clears it. Omitted with a new body lets the server derive a summary; otherwise omission preserves it."}},"required":["schema","id"],"anyOf":[{"required":["file"]},{"required":["title"]},{"required":["intro"]}],"additionalProperties":false})
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let mut result = json!({"provider":"newbee_official","id":self.id,"title_supplied":self.title.is_some(),"intro_supplied":self.intro.is_some(),"server_derived_intro":self.file.is_some()&&self.intro.is_none(),"credential_reads":false,"author_writes":0,"remote_validation":"not_run"});
        for (key, value) in [
            ("title_summary", &self.title),
            ("intro_summary", &self.intro),
        ] {
            if let Some(value) = value {
                result[key] = json!({"length":value.chars().count(),"sha256":storage::sha256(value.as_bytes())});
            }
        }
        if let Some(path) = &self.file {
            let (file, sha256, text) = input::locked_text(path, control)?;
            result["content_summary"] = json!({"length":text.chars().count(),"sha256":sha256});
            return storage::finish_file(file, Ok(result), control);
        }
        control.check()?;
        Ok(result)
    }
}
#[derive(Clone)]
struct TextState {
    title: String,
    intro: Option<String>,
    format: u64,
    content_sha256: String,
}
impl TextState {
    fn projection(&self, intro: bool) -> Value {
        let mut result = json!({"title":self.title,"content_format":self.format,"content_sha256":self.content_sha256});
        if intro {
            result["intro"] = json!(self.intro);
        }
        result
    }
    fn hash(&self, intro: bool) -> Result<String> {
        Ok(storage::sha256(&storage::identity_json(
            &self.projection(intro),
        )?))
    }
}
impl Official {
    pub fn update_config_text(&mut self, request: &ConfigTextUpdate) -> Result<Value> {
        if let Some(path) = &request.file {
            let (file, sha256, text) = input::locked_text(path, &self.control)?;
            let control = self.control.clone();
            let result = self.config_text_locked(request, Some((&sha256, &text)));
            storage::finish_file(file, result, &control)
        } else {
            self.config_text_locked(request, None)
        }
    }
    fn config_text_locked(
        &mut self,
        request: &ConfigTextUpdate,
        file: Option<(&str, &str)>,
    ) -> Result<Value> {
        let author = self.author()?;
        let identity = json!({"author":author,"id":request.id,"file_sha256":file.map(|value|value.0),"title":request.title,"intro":request.intro});
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "official-config-text",
            request.id.get(),
            "update",
            &identity,
            &self.control,
        )?;
        let recovered = mutation.pending();
        let mut current = self.config_text_state(request.id).map_err(|error| {
            if recovered {
                mutation.readback_error(
                    "original official configuration text readback is pending",
                    error,
                )
            } else {
                error
            }
        })?;
        if let Some(value) = mutation
            .evidence()
            .filter(|value| value["verification"] == "requested_text_fields")
        {
            let mut result = value.clone();
            let matches = result["observed_fields_sha256"] == current.hash(true)?;
            if recovered || matches {
                if recovered {
                    mutation.verified()?;
                }
                result["reused"] = json!(true);
                result["recovered"] = json!(recovered);
                result["current_state_matches"] = json!(matches);
                result["author_writes"] = json!(0);
                return Ok(result);
            }
        }
        let evidence = if recovered {
            mutation.evidence().cloned().ok_or_else(|| {
                mutation.pending_error("original configuration text field hashes are missing")
            })?
        } else {
            if file.is_some() && current.format != 2 {
                return Err(ToolkitError::new(
                    "publish.ncc_config_format",
                    "official body updates support Markdown; edit existing HTML through the Creator interface",
                    3,
                ));
            }
            let derived = file.is_some() && request.intro.is_none();
            let verify_intro = request.intro.is_some() || (!derived && current.intro.is_some());
            let mut expected = current.clone();
            if let Some(value) = &request.title {
                expected.title = value.clone();
            }
            if let Some(value) = &request.intro {
                expected.intro = Some(value.clone());
            }
            if let Some((sha256, _)) = file {
                expected.content_sha256 = sha256.to_owned();
            }
            let before = current.hash(verify_intro)?;
            let expected = expected.hash(verify_intro)?;
            if !derived && before == expected {
                return Ok(
                    json!({"id":request.id,"verification":"requested_text_fields","changed":false,"author_writes":0,"intro_verification":if verify_intro{"readback"}else{"not_available"}}),
                );
            }
            let mut evidence = json!({"expected_fields_sha256":expected,"before_fields_sha256":before,"verify_intro":verify_intro,"server_derived_intro":derived,"api_accepted":false});
            let mut body = json!({"id":request.id});
            if let Some(value) = &request.title {
                body["title"] = json!(value);
            }
            if let Some(value) = &request.intro {
                body["intro"] = json!(value);
            }
            if let Some((_, text)) = file {
                body["content"] = json!(text);
            }
            mutation.prepare(&self.control)?;
            mutation.remember(evidence.clone())?;
            self.call("/share_config/push_text", &body, true)
                .map_err(|error| mutation.failure(error))?;
            evidence["api_accepted"] = json!(true);
            mutation.remember(evidence.clone()).and_then(|_|mutation.acknowledged()).map_err(|_|mutation.pending_error("official configuration text was accepted; finish saving its acknowledgement"))?;
            current = self.config_text_state(request.id).map_err(|error| {
                mutation.readback_error("official configuration text readback is pending", error)
            })?;
            evidence
        };
        let verify_intro = evidence["verify_intro"].as_bool().ok_or_else(|| {
            mutation.pending_error("original configuration text field selection is missing")
        })?;
        let derived = evidence["server_derived_intro"].as_bool().ok_or_else(|| {
            mutation.pending_error("original configuration summary policy is missing")
        })?;
        let expected = evidence["expected_fields_sha256"].as_str().ok_or_else(|| {
            mutation.pending_error("original configuration text hashes are missing")
        })?;
        if current.hash(verify_intro)? != expected {
            return Err(mutation.pending_error("official configuration text differs from the original requested and preserved fields; the update was not repeated"));
        }
        if derived
            && evidence["api_accepted"] != true
            && evidence["before_fields_sha256"] == evidence["expected_fields_sha256"]
        {
            return Err(mutation.pending_error("only a derived summary may have changed and API acceptance is unknown; inspect the original configuration without another submission"));
        }
        let result = json!({"id":request.id,"operation_id":mutation.id(),"verification":"requested_text_fields","content_sha256":current.content_sha256,"intro_verification":if verify_intro{"readback"}else{"not_available"},"server_derived_intro":derived,"api_acceptance":if evidence["api_accepted"]==true{"acknowledged"}else{"not_observed"},"observed_fields_sha256":current.hash(true)?,"changed":true,"recovered":recovered,"reused":false,"author_writes":if recovered{0}else{1}});
        // Only hashes and observed metadata coverage are retained, never the
        // original configuration body or a fabricated summary expectation.
        mutation.verified_with(result.clone()).map_err(|_| {
            mutation.pending_error(
                "configuration text readback matched; finish saving its original result",
            )
        })?;
        Ok(result)
    }
    fn config_text_state(&mut self, id: NonZeroU64) -> Result<TextState> {
        let detail = self.call("/share_config/publish_detail", &json!({"id":id}), false)?;
        let text = self.call("/share_config/pull_text", &json!({"id":id}), false)?;
        for raw in [&detail, &text] {
            for key in ["id", "config_id"] {
                if raw
                    .get(key)
                    .is_some_and(|value| !positive(value).is_ok_and(|value| value == id.get()))
                {
                    return Err(data(
                        "official configuration text identifies another resource",
                    ));
                }
            }
        }
        let title = detail["title"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| data("official configuration detail omitted its title"))?
            .to_owned();
        let intro = match detail.get("intro") {
            Some(Value::String(value)) => Some(value.clone()),
            Some(Value::Null) => Some(String::new()),
            None => None,
            _ => return Err(data("official configuration summary is malformed")),
        };
        let format = positive(&text["content_format"])?;
        if !matches!(format, 1 | 2)
            || detail
                .get("content_format")
                .is_some_and(|value| !positive(value).is_ok_and(|value| value == format))
        {
            return Err(data(
                "official configuration body format is missing or inconsistent",
            ));
        }
        let body = match text.get("content") {
            Some(Value::String(value)) => value.as_str(),
            Some(Value::Null) => "",
            _ => return Err(data("official configuration body is missing")),
        };
        Ok(TextState {
            title,
            intro,
            format,
            content_sha256: storage::sha256(body.as_bytes()),
        })
    }
}
#[cfg(test)]
mod tests;
