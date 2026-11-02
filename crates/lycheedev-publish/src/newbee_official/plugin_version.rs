use super::*;
use crate::{input::invalid, mutation::Mutation, provider::Provider};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Seek, SeekFrom},
};
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Versions {
    Explicit(Vec<String>),
    Automatic(String),
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Log {
    Inline(String),
    File(LogFile),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogFile {
    file: String,
}
pub struct PluginVersion {
    id: NonZeroU64,
    version: String,
    versions: Versions,
    log: Log,
    file: Option<PathBuf>,
    directory: PathBuf,
    cwd: PathBuf,
}
impl PluginVersion {
    pub fn parse(bytes: &[u8], cwd: &Path, control: &OperationControl) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Doc {
            schema: String,
            id: Option<NonZeroU64>,
            version: Option<String>,
            game_versions: Option<Versions>,
            changelog: Option<Log>,
            file: Option<String>,
            dir: Option<String>,
        }
        let raw = input::document(bytes)?;
        if raw.values().any(Value::is_null) {
            return Err(invalid("$", "omit optional upload fields; null is invalid"));
        }
        let doc: Doc = serde_json::from_value(json!(raw))
            .map_err(|_| invalid("$", "official plugin version input differs from its schema"))?;
        if doc.schema != "lycheedev.publish.v1.newbee.official.plugin-version.upload" {
            return Err(invalid(
                "$.schema",
                "select official plugin-version upload schema",
            ));
        }
        if doc.dir.as_ref().is_some_and(|v| v.trim().is_empty())
            || doc.file.as_ref().is_some_and(|v| v.trim().is_empty())
        {
            return Err(invalid("$", "directory/archive paths must not be blank"));
        }
        let directory = std::path::absolute(cwd.join(doc.dir.as_deref().unwrap_or(".")))?;
        let manifest = if doc.dir.is_some()
            || doc.id.is_none()
            || doc.version.is_none()
            || doc.game_versions.is_none()
            || doc.changelog.is_none()
        {
            project::manifest(&directory, control)?
        } else {
            project::Manifest::default()
        };
        let id = doc
            .id
            .or(manifest.id)
            .ok_or_else(|| invalid("$.id", "provide plugin id or set ncc.json id"))?;
        let version = doc.version.unwrap_or(manifest.version);
        if version.trim().is_empty() {
            return Err(invalid(
                "$.version",
                "provide a version label or set ncc.json version",
            ));
        }
        let versions = doc
            .game_versions
            .unwrap_or(Versions::Explicit(manifest.game_versions));
        match &versions {
            Versions::Automatic(value) if value == "auto" => (),
            Versions::Explicit(values)
                if !values.is_empty()
                    && values.iter().all(|v| !v.trim().is_empty())
                    && values.iter().collect::<BTreeSet<_>>().len() == values.len() =>
            {
                ()
            }
            _ => {
                return Err(invalid(
                    "$.game_versions",
                    "provide unique client version strings or auto, or configure ncc.json game_versions",
                ));
            }
        }
        let log = doc.changelog.unwrap_or_else(|| {
            if manifest.changelog_file.is_empty() {
                Log::Inline(String::new())
            } else {
                Log::File(LogFile {
                    file: directory
                        .join(manifest.changelog_file)
                        .to_string_lossy()
                        .into_owned(),
                })
            }
        });
        let file = doc.file.map(|file| cwd.join(file));
        if file.as_ref().is_some_and(|path| !path.is_file()) {
            return Err(invalid(
                "$.file",
                "select an existing ZIP file, or omit file to package the addon directory",
            ));
        }
        Ok(Self {
            id,
            version,
            versions,
            log,
            file,
            directory,
            cwd: cwd.to_owned(),
        })
    }
    pub fn schema() -> Value {
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"schema":{"const":"lycheedev.publish.v1.newbee.official.plugin-version.upload"},"id":{"type":"integer","minimum":1,"description":"Plugin ID; default ncc.json id."},"version":{"type":"string","minLength":1,"description":"Unique version label; default ncc.json version."},"game_versions":{"oneOf":[{"const":"auto"},{"type":"array","items":{"type":"string","minLength":1},"minItems":1,"uniqueItems":true}],"description":"Current client version strings, not game-group IDs. auto detects Interface values from the original ZIP members. Default ncc.json game_versions."},"changelog":{"oneOf":[{"type":"string"},{"type":"object","properties":{"file":{"type":"string","minLength":1}},"required":["file"],"additionalProperties":false}],"description":"Inline UTF-8 log or file, default ncc.json changelog_file then empty text. Files resolve from invocation directory; the manifest's own changelog_file resolves from its project directory."},"file":{"type":"string","minLength":1,"description":"Existing addon ZIP up to 2048 MiB. Omit to package dir, reusing an existing matching ZIP without rebuilding."},"dir":{"type":"string","minLength":1,"default":".","description":"Addon project for ncc.json defaults and optional packaging; no managed game deployment."}},"required":["schema"],"additionalProperties":false})
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let (file, hash, log) = self.log(control)?;
        let result = (|| {
            let mut value = if let Some(path) = &self.file {
                let (mut file, sha) = storage::locked_sha256(path, project::MAX_ZIP, control)?;
                let result = (|| {
                    let inv = project::inventory(&mut file, control)?;
                    Ok(
                        json!({"file":path,"sha256":sha,"zip_bytes":file.metadata()?.len(),"file_count":inv.files.len(),"interfaces":inv.interfaces,"archive_validation":"original_bytes"}),
                    )
                })();
                storage::finish_file(file, result, control)?
            } else {
                PluginPack::for_upload(&self.directory, &self.cwd, &self.version, control)?
                    .plan(control)?
            };
            value["id"] = json!(self.id);
            value["version"] = json!(self.version);
            value["game_versions"] = json!(self.versions);
            value["changelog_summary"] = json!({"length":log.chars().count(),"sha256":hash});
            value["credential_reads"] = json!(false);
            value["author_writes"] = json!(0);
            value["remote_validation"] = json!("not_run");
            Ok(value)
        })();
        if let Some(file) = file {
            storage::finish_file(file, result, control)
        } else {
            result
        }
    }
    fn log(&self, control: &OperationControl) -> Result<(Option<File>, String, String)> {
        match &self.log {
            Log::Inline(value) => Ok((None, storage::sha256(value.as_bytes()), value.clone())),
            Log::File(value) => {
                let (file, hash, text) = input::locked_text(&self.cwd.join(&value.file), control)?;
                Ok((Some(file), hash, text))
            }
        }
    }
}
impl Official {
    pub fn upload_plugin_version(
        &mut self,
        request: &PluginVersion,
        recover_file: Option<NonZeroU64>,
    ) -> Result<Value> {
        let (log_file, log_sha256, log) = request.log(&self.control)?;
        let result = (|| {
            let package = if request.file.is_none() {
                let pack = PluginPack::for_upload(
                    &request.directory,
                    &request.cwd,
                    &request.version,
                    &self.control,
                )?;
                let report = pack.run(&self.control)?;
                Some((pack.output().to_owned(), report))
            } else {
                None
            };
            let path = request
                .file
                .as_ref()
                .or_else(|| package.as_ref().map(|p| &p.0))
                .unwrap();
            let (mut file, sha256) = storage::locked_sha256(path, project::MAX_ZIP, &self.control)?;
            let result = self
                .upload_plugin_locked(
                    request,
                    path,
                    &mut file,
                    &sha256,
                    &log_sha256,
                    &log,
                    recover_file,
                )
                .map(|mut value| {
                    if let Some((_, report)) = package {
                        value["package"] = report;
                    }
                    value
                });
            storage::finish_file(file, result, &self.control)
        })();
        if let Some(file) = log_file {
            storage::finish_file(file, result, &self.control)
        } else {
            result
        }
    }
    fn upload_plugin_locked(
        &mut self,
        request: &PluginVersion,
        path: &Path,
        file: &mut File,
        sha256: &str,
        log_sha256: &str,
        log: &str,
        recover_file: Option<NonZeroU64>,
    ) -> Result<Value> {
        let inventory = project::inventory(file, &self.control)?;
        let size = file.metadata()?.len();
        let author = self.author()?;
        let identity = json!({"author":author,"plugin_id":request.id,"version":request.version,"game_versions":request.versions,"zip_sha256":sha256,"size":size,"changelog_sha256":log_sha256});
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            "official-plugin-version",
            request.id.get(),
            "upload",
            &identity,
            &self.control,
        )?;
        let recovered = mutation.pending();
        if let Some(value) = mutation
            .evidence()
            .filter(|v| v["verification"] == "version_metadata")
        {
            if recover_file.is_some_and(|id| value["file_id"] != json!(id)) {
                return Err(invalid(
                    "$.file_id",
                    "recovery file ID differs from the verified original upload",
                ));
            }
            let mut value = value.clone();
            if recovered {
                mutation.verified()?;
            }
            value["reused"] = json!(true);
            value["verification_source"] = json!("original_receipt");
            value["upload_writes"] = json!(0);
            return Ok(value);
        }
        if !recovered && recover_file.is_some() {
            return Err(ToolkitError::arguments(
                "--file-id recovers an original pending upload; omit it for a new version",
            ));
        }
        let (games, file_id) = if recovered {
            let evidence = mutation.evidence().ok_or_else(|| {
                mutation.pending_error("original plugin upload metadata is missing")
            })?;
            let games =
                serde_json::from_value::<Vec<String>>(evidence["selected_game_versions"].clone())
                    .map_err(|_| {
                    mutation.pending_error("original client version selection is missing")
                })?;
            let saved = evidence["file_id"].as_u64().and_then(NonZeroU64::new);
            if saved.is_some_and(|saved| recover_file.is_some_and(|id| id != saved)) {
                return Err(invalid(
                    "$.file_id",
                    "recovery file ID differs from the acknowledged original upload",
                ));
            }
            let file_id = saved.or(recover_file);
            if evidence["api_accepted"] != true && file_id.is_none() {
                return Err(mutation.pending_error("original plugin upload outcome is uncertain; inspect official plugin versions and recover its explicit --file-id with the original input"));
            }
            (games, file_id)
        } else {
            let detail = self.call("/mod/publish_detail", &json!({"id":request.id}), false)?;
            check_plugin(&detail, request.id)?;
            let rows = self.official_versions(request.id)?;
            if rows
                .iter()
                .any(|row| version_name(row).is_ok_and(|name| name == request.version))
            {
                return Err(invalid(
                    "$.version",
                    "version label already exists; choose a different label",
                ));
            }
            let games = self.select_versions(&request.versions, &inventory.interfaces)?;
            let before = rows.iter().map(file_id_value).collect::<Result<Vec<_>>>()?;
            let mut evidence = json!({"selected_game_versions":games,"before_file_ids":before,"api_accepted":false});
            let headers = self.headers()?;
            let filename = crate::local_backup::filename(path)?;
            file.seek(SeekFrom::Start(0))?;
            let stream = file.try_clone()?;
            mutation.prepare(&self.control)?;
            mutation.remember(evidence.clone())?;
            let raw = self
                .http
                .multipart_file(
                    Origin::NewbeeCreator,
                    &format!("{API}/mod_file/upload_mod_file"),
                    headers,
                    &[
                        ("mod_id", request.id.to_string()),
                        ("version", request.version.clone()),
                        ("game_version_list", serde_json::to_string(&games)?),
                        ("changelog", log.to_owned()),
                        ("size", size.to_string()),
                    ],
                    &filename,
                    "application/zip",
                    stream,
                )
                .and_then(|value| envelope(value, true))
                .map_err(|error| mutation.failure(error))?;
            let id=raw.get("file_id").or_else(||raw.get("id")).map(positive).transpose().map_err(|_|mutation.pending_error("upload was accepted with an invalid file ID; recover the original version explicitly"))?.and_then(NonZeroU64::new);
            evidence["api_accepted"] = json!(true);
            if let Some(id) = id {
                evidence["file_id"] = json!(id);
            }
            mutation
                .remember(evidence)
                .and_then(|_| mutation.acknowledged())
                .map_err(|_| {
                    mutation.pending_error(
                        "plugin upload was accepted; finish saving its original acknowledgement",
                    )
                })?;
            (games, id)
        };
        let rows = self.official_versions(request.id).map_err(|error| {
            mutation.readback_error(
                "original official plugin version readback is pending",
                error,
            )
        })?;
        let before = mutation
            .evidence()
            .and_then(|e| e["before_file_ids"].as_array())
            .ok_or_else(|| {
                mutation.pending_error("original pre-upload version inventory is missing")
            })?;
        let mut matches = Vec::new();
        for row in &rows {
            let id = file_id_value(row).map_err(|error| {
                mutation.readback_error("official plugin file identity is pending", error)
            })?;
            if file_id.is_some_and(|selected| id != selected.get()) {
                continue;
            }
            if before.iter().any(|prior| prior.as_u64() == Some(id)) {
                continue;
            }
            if version_name(row).is_ok_and(|name| name == request.version)
                && file_size(row).is_ok_and(|bytes| bytes == size)
                && row_versions(row)
                    .is_ok_and(|value| value == games.iter().cloned().collect::<BTreeSet<_>>())
            {
                matches.push((id, row));
            }
        }
        if matches.len() != 1 {
            return Err(mutation.pending_error("original plugin file ID/version/size/client versions have not matched a unique new row; upload was not repeated"));
        }
        let (id, row) = matches[0];
        let log_verification = match row.get("changelog") {
            Some(Value::String(value)) if storage::sha256(value.as_bytes()) == log_sha256 => {
                "readback"
            }
            Some(Value::String(_)) => {
                return Err(mutation.pending_error(
                    "original uploaded file changelog differs from the original text",
                ));
            }
            None | Some(Value::Null) => "not_available",
            _ => {
                return Err(mutation.pending_error("original file changelog readback is malformed"));
            }
        };
        let result = json!({"id":request.id,"file_id":id,"version":request.version,"game_versions":games,"sha256":sha256,"zip_bytes":size,"operation_id":mutation.id(),"verification":"version_metadata","verification_source":"version_list_readback","remote_byte_verification":"not_available","changelog_verification":log_verification,"review_status":"not_checked","recovered":recovered,"reused":false,"upload_writes":if recovered{0}else{1}});
        mutation.verified_with(result.clone()).map_err(|_| {
            mutation
                .pending_error("official plugin version matched; finish saving its original result")
        })?;
        Ok(result)
    }
    fn select_versions(
        &self,
        selection: &Versions,
        interfaces: &BTreeSet<u64>,
    ) -> Result<Vec<String>> {
        let metadata = self.http.get(
            Origin::NewbeeMetadata,
            "/modconfig.json",
            &[],
            HeaderMap::new(),
        )?;
        let groups = metadata["game_version"]
            .as_array()
            .ok_or_else(|| data("current client version groups are missing"))?;
        let mut available = BTreeSet::new();
        for row in groups {
            for value in row["version"]
                .as_array()
                .ok_or_else(|| data("current client version group is incomplete"))?
            {
                available.insert(
                    value
                        .as_str()
                        .ok_or_else(|| data("current client version is not text"))?
                        .to_owned(),
                );
            }
        }
        let selected = match selection {
            Versions::Explicit(values) => {
                if values.iter().any(|v| !available.contains(v)) {
                    return Err(invalid(
                        "$.game_versions",
                        "client version is absent from current platform metadata; inspect plugin game-versions",
                    ));
                }
                values.iter().cloned().collect::<BTreeSet<_>>()
            }
            Versions::Automatic(_) => interfaces
                .iter()
                .map(|v| format!("{}.{}.{}", v / 10000, (v / 100) % 100, v % 100))
                .filter(|v| available.contains(v))
                .collect(),
        };
        if selected.is_empty() {
            return Err(invalid(
                "$.game_versions",
                "no selected TOC Interface maps to a currently supported client version; choose explicit client versions",
            ));
        }
        Ok(selected.into_iter().collect())
    }
    fn official_versions(&mut self, id: NonZeroU64) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        let mut total = None;
        let mut ids = BTreeSet::new();
        for page in 1u64..=10000 {
            let raw = self.call(
                "/mod_file/mod_file_list",
                &json!({"mod_id":id,"pagenum":page,"pagesize":2000}),
                false,
            )?;
            let count = raw["total"].as_u64().ok_or_else(|| {
                data("complete plugin version enumeration requires an explicit total")
            })?;
            if total.is_some_and(|v| v != count)
                || raw.get("pagenum").is_some_and(|v| v.as_u64() != Some(page))
            {
                return Err(data("official plugin pagination changed its total or page"));
            }
            total = Some(count);
            let rows = raw["list"]
                .as_array()
                .ok_or_else(|| data("official plugin version page omitted list"))?;
            for row in rows {
                if !ids.insert(file_id_value(row)?) {
                    return Err(data("official plugin version pagination repeats a file ID"));
                }
                version_name(row)?;
            }
            let length = rows.len();
            result.extend(rows.iter().cloned());
            if result.len() as u64 == count {
                return Ok(result);
            }
            if length == 0 || result.len() as u64 > count {
                return Err(data(
                    "official plugin version pagination has a coverage gap",
                ));
            }
        }
        Err(data(
            "official plugin version enumeration exceeded the page bound",
        ))
    }
}
pub(super) fn check_plugin(raw: &Value, id: NonZeroU64) -> Result<()> {
    if !raw.is_object() {
        return Err(data("official plugin detail is not an object"));
    }
    for key in ["id", "mod_id", "t_id"] {
        if raw
            .get(key)
            .is_some_and(|v| !positive(v).is_ok_and(|v| v == id.get()))
        {
            return Err(data("official plugin detail identifies another plugin"));
        }
    }
    Ok(())
}
fn file_id_value(row: &Value) -> Result<u64> {
    positive(
        row.get("file_id")
            .or_else(|| row.get("t_id"))
            .unwrap_or(&Value::Null),
    )
}
fn version_name(row: &Value) -> Result<&str> {
    row.get("version_name")
        .or_else(|| row.get("t_display_name"))
        .or_else(|| row.get("t_file_name"))
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| data("official plugin version omitted its label"))
}
fn file_size(row: &Value) -> Result<u64> {
    row.get("file_size")
        .or_else(|| row.get("t_file_length"))
        .and_then(Value::as_u64)
        .ok_or_else(|| data("official plugin file omitted its byte length"))
}
fn row_versions(row: &Value) -> Result<BTreeSet<String>> {
    let values = row
        .get("game_versions")
        .or_else(|| row.get("versions"))
        .and_then(Value::as_array)
        .ok_or_else(|| data("official plugin file omitted its client versions"))?;
    let values = values
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| data("official plugin file client version is invalid"))
        })
        .collect::<Result<Vec<_>>>()?;
    let set = values.iter().cloned().collect::<BTreeSet<_>>();
    if set.len() != values.len() {
        return Err(data("official plugin file repeats client versions"));
    }
    Ok(set)
}
#[cfg(test)]
mod tests;
