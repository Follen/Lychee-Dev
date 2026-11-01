use super::*;
use crate::{input::invalid, mutation::Mutation, provider::Provider};
use std::fs::File;

pub struct WaFileUpdate {
    id: NonZeroU64,
    file: PathBuf,
    log: String,
    version: Option<String>,
}
fn version(text: &str) -> Option<[u64; 3]> {
    let parts = text.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }
    let mut result = [0; 3];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        result[index] = part.parse().ok()?;
    }
    Some(result)
}
impl WaFileUpdate {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Doc {
            schema: String,
            id: NonZeroU64,
            file: String,
            wa_log: String,
            version: Option<String>,
        }
        let raw = input::document(bytes)?;
        if raw.get("version").is_some_and(Value::is_null) {
            return Err(invalid(
                "$.version",
                "omit the version for automatic selection; null is not a version",
            ));
        }
        let doc: Doc = serde_json::from_value(json!(raw)).map_err(|_| {
            invalid(
                "$",
                "official WA update input differs from its declared schema",
            )
        })?;
        if doc.schema != "lycheedev.publish.v1.newbee.official.wa.update" {
            return Err(invalid("$.schema", "select the official WA update schema"));
        }
        if doc
            .version
            .as_deref()
            .is_some_and(|value| version(value).is_none())
        {
            return Err(invalid(
                "$.version",
                "version must contain three unsigned decimal components, such as 1.2.3",
            ));
        }
        let file = cwd.join(doc.file);
        if !file.is_file() {
            return Err(invalid("$.file", "select an existing UTF-8 string file"));
        }
        Ok(Self {
            id: doc.id,
            file,
            log: doc.wa_log,
            version: doc.version,
        })
    }
    pub fn schema() -> Value {
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"schema":{"const":"lycheedev.publish.v1.newbee.official.wa.update"},"id":{"type":"integer","minimum":1},"file":{"type":"string","minLength":1,"description":"Existing nonempty UTF-8 opaque string file, at most 32 MiB. Relative paths resolve from the invocation directory."},"wa_log":{"type":"string","description":"Required update log; an explicit empty string is retained."},"version":{"type":"string","pattern":"^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$","description":"A version greater than the current one. Omit to increment a current three-part version's patch; legacy version forms require an explicit target. The selected version is recorded before submission."}},"required":["schema","id","file","wa_log"],"additionalProperties":false})
    }
    fn locked(&self, control: &OperationControl) -> Result<(File, String, String)> {
        input::locked_text(&self.file, control)
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let (file, sha256, text) = self.locked(control)?;
        storage::finish_file(
            file,
            Ok(
                json!({"provider":"newbee_official","id":self.id,"content_summary":{"length":text.chars().count(),"sha256":sha256},"wa_log_summary":{"length":self.log.chars().count(),"sha256":storage::sha256(self.log.as_bytes())},"version":self.version,"automatic_version":self.version.is_none(),"credential_reads":false,"author_writes":0,"remote_validation":"not_run"}),
            ),
            control,
        )
    }
}
struct Current {
    version: String,
    sha256: String,
    log_sha256: Option<String>,
}
fn current(raw: Value, id: NonZeroU64) -> Result<Current> {
    for key in ["id", "wa_id"] {
        if raw
            .get(key)
            .is_some_and(|value| !positive(value).is_ok_and(|value| value == id.get()))
        {
            return Err(data("official string readback identifies another resource"));
        }
    }
    let version = match &raw["version"] {
        Value::String(value) if !value.is_empty() => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => return Err(data("official string readback omitted its version")),
    };
    let text = raw["wa_str"]
        .as_str()
        .ok_or_else(|| data("official string readback omitted its opaque content"))?;
    let log_sha256 = match raw.get("wa_log") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(storage::sha256(value.as_bytes())),
        _ => return Err(data("official string readback has an invalid update log")),
    };
    Ok(Current {
        version,
        sha256: storage::sha256(text.as_bytes()),
        log_sha256,
    })
}
impl Official {
    pub fn update_wa_file(&mut self, request: &WaFileUpdate) -> Result<Value> {
        let (file, sha256, text) = request.locked(&self.control)?;
        let control = self.control.clone();
        let result = self.update_wa_locked(request, &sha256, &text);
        storage::finish_file(file, result, &control)
    }
    fn update_wa_locked(
        &mut self,
        request: &WaFileUpdate,
        sha256: &str,
        text: &str,
    ) -> Result<Value> {
        let author = self.author()?;
        let log_sha256 = storage::sha256(request.log.as_bytes());
        let identity = json!({"author":author,"id":request.id,"file_sha256":sha256,"wa_log_sha256":log_sha256,"version":request.version});
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "official-wa-version",
            request.id.get(),
            "update",
            &identity,
            &self.control,
        )?;
        let recovered = mutation.pending();
        let old = self.wa_current(request.id).map_err(|error| {
            if recovered {
                mutation.readback_error("original official string readback is pending", error)
            } else {
                error
            }
        })?;
        if !recovered {
            if let Some(value) = mutation.evidence().filter(|value| {
                value["verification"] == "version_and_content"
                    && value["version"] == old.version
                    && old.sha256 == sha256
                    && old
                        .log_sha256
                        .as_ref()
                        .is_none_or(|value| value == &log_sha256)
            }) {
                let mut result = value.clone();
                result["original_log_verification"] = result["log_verification"].clone();
                result["log_verification"] = json!(if old.log_sha256.is_some() {
                    "readback"
                } else {
                    "not_available"
                });
                result["reused"] = json!(true);
                result["author_writes"] = json!(0);
                return Ok(result);
            }
        }
        let selected = if recovered {
            mutation.evidence().and_then(|value|value["selected_version"].as_str().or_else(||if value["verification"]=="version_and_content"{value["version"].as_str()}else{None})).map(str::to_owned).ok_or_else(||mutation.pending_error("the original official string version selection is missing; retain its original receipt"))?
        } else {
            let detail = self.call("/wa/publish_detail", &json!({"id":request.id}), false)?;
            for key in ["id", "wa_id"] {
                if detail.get(key).is_some_and(|value| {
                    !positive(value).is_ok_and(|value| value == request.id.get())
                }) {
                    return Err(data("official string detail identifies another resource"));
                }
            }
            match detail["is_multi_string"].as_bool() {
                Some(false) => (),
                Some(true) => {
                    return Err(ToolkitError::new(
                        "publish.ncc_wa_mode",
                        "official string updates support single strings; update collections through the Creator interface",
                        3,
                    ));
                }
                None => {
                    return Err(data(
                        "official string detail did not declare its string mode",
                    ));
                }
            }
            let selected = if let Some(value) = &request.version {
                value.clone()
            } else {
                let mut parts=version(&old.version).ok_or_else(||invalid("$.version","current legacy version has no automatic patch mapping; provide an explicit three-part target version"))?;
                parts[2] = parts[2].checked_add(1).ok_or_else(|| {
                    invalid(
                        "$.version",
                        "patch component is full; provide an explicit target version",
                    )
                })?;
                format!("{}.{}.{}", parts[0], parts[1], parts[2])
            };
            if version(&old.version).is_some_and(|old| version(&selected).unwrap() <= old) {
                return Err(invalid(
                    "$.version",
                    "target version must be greater than the current version",
                ));
            }
            mutation.prepare(&self.control)?;
            mutation.remember(json!({"selected_version":selected,"content_sha256":sha256,"wa_log_sha256":log_sha256}))?;
            self.call(
                "/wa/push_str",
                &json!({"id":request.id,"wa_str":text,"wa_log":request.log,"version":selected}),
                true,
            )
            .map_err(|error| mutation.failure(error))?;
            mutation.acknowledged().map_err(|_|mutation.pending_error("official string update was accepted; finish saving its original acknowledgement"))?;
            selected
        };
        let read = if recovered {
            old
        } else {
            self.wa_current(request.id).map_err(|error| {
                mutation.readback_error("official string update readback is pending", error)
            })?
        };
        if read.version != selected
            || read.sha256 != sha256
            || read
                .log_sha256
                .as_ref()
                .is_some_and(|value| value != &log_sha256)
        {
            return Err(mutation.pending_error("the original official string version/content/log has not appeared in readback; the update was not repeated"));
        }
        let result = json!({"id":request.id,"operation_id":mutation.id(),"version":selected,"content_sha256":sha256,"verification":"version_and_content","log_verification":if read.log_sha256.is_some(){"readback"}else{"not_available"},"recovered":recovered,"reused":false,"author_writes":if recovered{0}else{1}});
        mutation.verified_with(result.clone()).map_err(|_| {
            mutation.pending_error(
                "official string readback matched; finish saving its original result",
            )
        })?;
        Ok(result)
    }
    fn wa_current(&mut self, id: NonZeroU64) -> Result<Current> {
        current(self.call("/wa/pull_str", &json!({"id":id}), false)?, id)
    }
}
#[cfg(test)]
mod tests;
