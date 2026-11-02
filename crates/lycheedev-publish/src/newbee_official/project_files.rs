//! Read-only official exports into explicitly selected local files/projects.
use super::*;
use crate::{input::invalid, local_backup as local};
use std::{collections::BTreeMap, fs, io::Cursor};
enum Action {
    Pull {
        output: Option<PathBuf>,
        overwrite: bool,
    },
    Init {
        directory: PathBuf,
    },
}
pub struct TextProject {
    resource: Resource,
    id: NonZeroU64,
    cwd: PathBuf,
    action: Action,
}
impl TextProject {
    pub fn parse(resource: Resource, initialize: bool, bytes: &[u8], cwd: &Path) -> Result<Self> {
        if resource == Resource::Plugin {
            return Err(ToolkitError::arguments(
                "text project exports select WA or configuration",
            ));
        }
        let raw = input::document(bytes)?;
        if raw.values().any(Value::is_null) {
            return Err(invalid("$", "omit optional export fields; null is invalid"));
        }
        let schema = format!(
            "lycheedev.publish.v1.newbee.official.{}.{}",
            resource.key(),
            if initialize { "init" } else { "pull" }
        );
        let (id, action) = if initialize {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Doc {
                schema: String,
                id: NonZeroU64,
                dir: String,
            }
            let doc: Doc = serde_json::from_value(json!(raw)).map_err(|_| {
                invalid(
                    "$",
                    "project initialization requires schema, id and an empty/new dir",
                )
            })?;
            if doc.schema != schema {
                return Err(invalid(
                    "$.schema",
                    "select the initialization schema for this resource",
                ));
            }
            if doc.dir.trim().is_empty() {
                return Err(invalid(
                    "$.dir",
                    "select an explicit empty or new project directory",
                ));
            }
            (
                doc.id,
                Action::Init {
                    directory: std::path::absolute(cwd.join(doc.dir))?,
                },
            )
        } else {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Doc {
                schema: String,
                id: NonZeroU64,
                out: Option<String>,
                #[serde(default)]
                overwrite: bool,
            }
            let doc: Doc = serde_json::from_value(json!(raw))
                .map_err(|_| invalid("$", "text pull input differs from its schema"))?;
            if doc.schema != schema {
                return Err(invalid(
                    "$.schema",
                    "select the pull schema for this resource",
                ));
            }
            if doc.out.as_ref().is_some_and(|v| v.trim().is_empty()) {
                return Err(invalid("$.out", "output path must not be blank"));
            }
            (
                doc.id,
                Action::Pull {
                    output: doc.out.map(|value| cwd.join(value)),
                    overwrite: doc.overwrite,
                },
            )
        };
        let request = Self {
            resource,
            id,
            cwd: cwd.to_owned(),
            action,
        };
        request.admit()?;
        Ok(request)
    }
    pub fn schema(resource: Resource, initialize: bool) -> Value {
        let mut value = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"schema":{"const":format!("lycheedev.publish.v1.newbee.official.{}.{}",resource.key(),if initialize{"init"}else{"pull"})},"id":{"type":"integer","minimum":1}},"required":["schema","id"],"additionalProperties":false});
        if initialize {
            value["properties"]["dir"] = json!({"type":"string","minLength":1,"description":"Explicit empty or new directory. Existing project files are never replaced. Initialization writes the original text and a metadata snapshot; it submits no author changes."});
            value["required"].as_array_mut().unwrap().push(json!("dir"));
        } else {
            value["properties"]["out"] = json!({"type":"string","minLength":1,"description":"File with an existing parent. Default WA: wa_<ID>_v<server version>.txt; configuration: uipack_<ID>.html or .md from its original format."});
            value["properties"]["overwrite"] = json!({"type":"boolean","default":false,"description":"Explicitly replace an existing regular output file; never a directory or redirected entry."});
        }
        value
    }
    pub fn plan(&self) -> Result<Value> {
        self.admit()?;
        Ok(
            json!({"resource":self.resource,"id":self.id,"action":match self.action{Action::Pull{..}=>"pull",Action::Init{..}=>"init"},"credential_reads":false,"author_writes":0,"remote_validation":"not_run","local_writes":0,"output_selection":match &self.action{Action::Pull{output:Some(path),..}=>json!(path),Action::Pull{output:None,..}=>json!("after_server_version_or_format"),Action::Init{directory}=>json!(directory)}}),
        )
    }
    fn admit(&self) -> Result<()> {
        match &self.action {
            Action::Init { directory } => empty_directory(directory),
            Action::Pull {
                output: Some(path),
                overwrite,
            } => output_path(path, *overwrite),
            Action::Pull { output: None, .. } => Ok(()),
        }
    }
}
pub(super) fn empty_directory(path: &Path) -> Result<()> {
    let mut root = path;
    while !root.try_exists()? {
        root = root
            .parent()
            .ok_or_else(|| invalid("$.dir", "project directory requires an existing ancestor"))?;
    }
    storage::guard_path(root, path)?;
    if path.try_exists()? && (!path.is_dir() || fs::read_dir(path)?.next().is_some()) {
        return Err(invalid(
            "$.dir",
            "initialization requires an empty or new directory; use pull for an existing project",
        ));
    }
    Ok(())
}
fn output_path(path: &Path, overwrite: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("$.out", "output requires a parent"))?;
    if !parent.is_dir() {
        return Err(invalid("$.out", "output parent must exist"));
    }
    local::name(&local::filename(path)?)?;
    storage::guard_path(parent, path)?;
    if path.try_exists()? && (!overwrite || !path.is_file()) {
        return Err(invalid(
            "$.out",
            "output exists; select a different file or explicitly set overwrite for a regular file",
        ));
    }
    Ok(())
}
struct TextData {
    content: String,
    version: Option<String>,
    format: Option<u64>,
    multi: Option<bool>,
}
impl TextData {
    fn filename(&self, resource: Resource, id: NonZeroU64) -> String {
        match resource {
            Resource::Wa => {
                let version = self
                    .version
                    .as_ref()
                    .unwrap()
                    .chars()
                    .map(|c| {
                        if c < ' ' || "\\/:*?\"<>|".contains(c) {
                            '_'
                        } else {
                            c
                        }
                    })
                    .collect::<String>();
                format!("wa_{id}_v{version}.txt")
            }
            Resource::Config => format!(
                "uipack_{id}.{}",
                if self.format == Some(1) { "html" } else { "md" }
            ),
            Resource::Plugin => unreachable!(),
        }
    }
}
fn identity(raw: &Value, resource: Resource, id: NonZeroU64) -> Result<()> {
    if !raw.is_object() {
        return Err(data("official text export response is not an object"));
    }
    for key in [
        "id",
        if resource == Resource::Wa {
            "wa_id"
        } else {
            "config_id"
        },
    ] {
        if raw
            .get(key)
            .is_some_and(|value| !positive(value).is_ok_and(|value| value == id.get()))
        {
            return Err(data("official text export identifies another resource"));
        }
    }
    Ok(())
}
fn text_data(raw: &Value, resource: Resource, id: NonZeroU64) -> Result<TextData> {
    identity(raw, resource, id)?;
    if resource == Resource::Wa {
        let content = raw["wa_str"]
            .as_str()
            .ok_or_else(|| data("official string pull omitted its opaque content"))?
            .to_owned();
        let version = match &raw["version"] {
            Value::String(value) if !value.is_empty() => value.clone(),
            Value::Number(value) => value.to_string(),
            _ => return Err(data("official string pull omitted its version")),
        };
        Ok(TextData {
            content,
            version: Some(version),
            format: None,
            multi: raw.get("is_multi_string").and_then(Value::as_bool),
        })
    } else {
        let content = match &raw["content"] {
            Value::String(value) => value.clone(),
            Value::Null if raw.get("content").is_some() => String::new(),
            _ => return Err(data("official configuration pull omitted its text")),
        };
        let format = positive(&raw["content_format"])?;
        if !matches!(format, 1 | 2) {
            return Err(data(
                "official configuration format must declare HTML or Markdown",
            ));
        }
        Ok(TextData {
            content,
            version: None,
            format: Some(format),
            multi: None,
        })
    }
}
fn file_report(path: &Path, bytes: &[u8]) -> Value {
    json!({"file":path,"sha256":storage::sha256(bytes),"size":bytes.len()})
}
impl Official {
    pub fn export_text_project(&mut self, request: &TextProject) -> Result<Value> {
        self.control.check()?;
        request.admit()?;
        let mut detail = None;
        if matches!(request.action, Action::Init { .. }) {
            let raw = self.call(
                &format!("/{}/publish_detail", request.resource.route()),
                &json!({"id":request.id}),
                false,
            )?;
            identity(&raw, request.resource, request.id)?;
            detail = Some(raw);
        }
        let path = if request.resource == Resource::Wa {
            "/wa/pull_str"
        } else {
            "/share_config/pull_text"
        };
        let raw = self.call(path, &json!({"id":request.id}), false)?;
        let text = text_data(&raw, request.resource, request.id)?;
        let result = match &request.action {
            Action::Pull { output, overwrite } => {
                let output = output.clone().unwrap_or_else(|| {
                    request
                        .cwd
                        .join(text.filename(request.resource, request.id))
                });
                output_path(&output, *overwrite)?;
                let mut report = file_report(&output, text.content.as_bytes());
                project::save_reader(
                    &output,
                    &mut Cursor::new(text.content.as_bytes()),
                    text.content.len() as u64,
                    32 * 1024 * 1024,
                    *overwrite,
                    &self.control,
                )
                .map_err(|mut error| {
                    error.details["export"] = report.clone();
                    error
                })?;
                report["saved"] = json!(true);
                report
            }
            Action::Init { directory } => {
                let detail = detail.as_ref().unwrap();
                if let Some(format) = text.format {
                    if detail
                        .get("content_format")
                        .is_some_and(|value| !positive(value).is_ok_and(|value| value == format))
                    {
                        return Err(data(
                            "official configuration detail and body formats differ",
                        ));
                    }
                }
                let filename = text.filename(request.resource, request.id);
                let mut files =
                    BTreeMap::from([(filename.clone(), text.content.as_bytes().to_vec())]);
                let snapshot = if request.resource == Resource::Wa {
                    let description = match detail.get("description") {
                        Some(Value::String(value)) => value.as_str(),
                        None | Some(Value::Null) => "",
                        _ => return Err(data("official string description is malformed")),
                    };
                    files.insert("description.md".into(), description.as_bytes().to_vec());
                    json!({"game":"wow","type":"wa","id":request.id,"name":detail.get("name"),"intro":detail.get("intro"),"version":text.version,"game_versions":detail.get("game_versions"),"visibility":detail.get("visibility_text"),"price":detail.get("price"),"is_multi_string":detail.get("is_multi_string"),"string_file":filename,"description_file":"description.md"})
                } else {
                    let backup = match detail.get("cloud_backup") {
                        None | Some(Value::Null) => Value::Null,
                        Some(value) => {
                            json!({"backup_id":value.get("backup_id"),"name":value.get("name"),"backup_time":value.get("backup_time")})
                        }
                    };
                    let role = match detail.get("shared_role") {
                        None | Some(Value::Null) => Value::Null,
                        Some(value) => {
                            json!({"role_id":value.get("role_id"),"account":value.get("account"),"server":value.get("server"),"role":value.get("role")})
                        }
                    };
                    json!({"game":"wow","type":"share_config","id":request.id,"title":detail.get("title"),"intro":detail.get("intro"),"content_format":if text.format==Some(1){"html"}else{"markdown"},"visibility":detail.get("visibility_text"),"cloud_backup":backup,"shared_role":role,"content_file":filename})
                };
                let mut bytes = serde_json::to_vec_pretty(&snapshot)?;
                bytes.push(b'\n');
                // Write project.json last. A partial local export never becomes
                // a completed initialized project or truncates existing files.
                empty_directory(directory)?;
                fs::create_dir_all(directory)?;
                let mut written = Vec::new();
                for (name, bytes) in files {
                    let path = directory.join(name);
                    project::save_reader(
                        &path,
                        &mut Cursor::new(&bytes),
                        bytes.len() as u64,
                        32 * 1024 * 1024,
                        false,
                        &self.control,
                    )
                    .map_err(|mut error| {
                        error.details["initialization"] = json!("partial");
                        error.details["files_written"] = json!(written);
                        error.details["current_file"] = json!(path);
                        error
                    })?;
                    written.push(file_report(&path, &bytes));
                }
                let path = directory.join("project.json");
                project::save_reader(
                    &path,
                    &mut Cursor::new(&bytes),
                    bytes.len() as u64,
                    32 * 1024 * 1024,
                    false,
                    &self.control,
                )
                .map_err(|mut error| {
                    error.details["initialization"] = json!("partial");
                    error.details["files_written"] = json!(written);
                    error
                })?;
                written.push(file_report(&path, &bytes));
                json!({"initialized":true,"directory":directory,"files":written})
            }
        };
        let mut result = result;
        result["id"] = json!(request.id);
        result["resource"] = json!(request.resource);
        result["verification"] = json!("read_response_and_local_publication");
        result["author_writes"] = json!(0);
        if let Some(version) = text.version {
            result["version"] = json!(version);
        }
        if let Some(format) = text.format {
            result["content_format"] = json!(format);
        }
        if let Some(multi) = text.multi {
            result["is_multi_string"] = json!(multi);
        }
        Ok(output::public(result, &[]))
    }
}
#[cfg(test)]
mod tests;
