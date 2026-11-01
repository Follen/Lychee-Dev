//! Native NCC project association, cloning and explicit content synchronization.
//! The remote read completes in an owned staging directory before local edits.
use super::*;
use crate::{input::invalid, local_backup as local};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Seek, SeekFrom},
};
enum Action {
    Init(NonZeroU64),
    Sync { confirm: bool },
}
pub struct PluginWorkspace {
    directory: PathBuf,
    action: Action,
}
impl PluginWorkspace {
    pub fn parse(
        initialize: bool,
        bytes: &[u8],
        cwd: &Path,
        control: &OperationControl,
    ) -> Result<Self> {
        let raw = input::document(bytes)?;
        if raw.values().any(Value::is_null) {
            return Err(invalid(
                "$",
                "omit optional workspace fields; null is invalid",
            ));
        }
        let (directory, action) = if initialize {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Doc {
                schema: String,
                id: NonZeroU64,
                dir: String,
            }
            let doc: Doc = serde_json::from_value(json!(raw))
                .map_err(|_| invalid("$", "plugin init requires schema, id and an explicit dir"))?;
            if doc.schema != "lycheedev.publish.v1.newbee.official.plugin.init" {
                return Err(invalid("$.schema", "select official plugin init schema"));
            }
            (doc.dir, Action::Init(doc.id))
        } else {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Doc {
                schema: String,
                dir: String,
                #[serde(default)]
                confirm: bool,
            }
            let doc: Doc = serde_json::from_value(json!(raw))
                .map_err(|_| invalid("$", "plugin sync input differs from its schema"))?;
            if doc.schema != "lycheedev.publish.v1.newbee.official.plugin.sync" {
                return Err(invalid("$.schema", "select official plugin sync schema"));
            }
            (
                doc.dir,
                Action::Sync {
                    confirm: doc.confirm,
                },
            )
        };
        if directory.trim().is_empty() {
            return Err(invalid(
                "$.dir",
                "select an explicit plugin project directory",
            ));
        }
        let request = Self {
            directory: std::path::absolute(cwd.join(directory))?,
            action,
        };
        request.local(control)?;
        Ok(request)
    }
    pub fn schema(initialize: bool) -> Value {
        let mut schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"schema":{"const":format!("lycheedev.publish.v1.newbee.official.plugin.{}",if initialize{"init"}else{"sync"})},"dir":{"type":"string","minLength":1}},"required":["schema","dir"],"additionalProperties":false});
        if initialize {
            schema["properties"]["id"] = json!({"type":"integer","minimum":1,"description":"Explicit official plugin ID. An existing addon/collection is associated without replacing its code. A new/empty directory clones the latest uploaded ZIP and original description. Existing ncc.json is never replaced."});
            schema["required"].as_array_mut().unwrap().push(json!("id"));
        } else {
            schema["properties"]["confirm"] = json!({"type":"boolean","default":false,"description":"Required true for execution: replace packaged content with the latest online version and remove obsolete selected files. Hidden/ignored entries, ncc.json and collection top-level loose files remain. --plan performs local inspection only; no remote preview or output writes. Bundle projects are not supported by sync."});
        }
        schema
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let local = self.local(control)?;
        Ok(
            json!({"id":local.id,"directory":self.directory,"action":if matches!(self.action,Action::Init(_)){"init"}else{"sync"},"local_layout":local.layout,"clones_latest":local.clone,"selected_local_files":local.files.len(),"confirmation_required":matches!(self.action,Action::Sync{..}),"credential_reads":false,"local_writes":0,"author_writes":0,"remote_validation":"not_run"}),
        )
    }
    fn local(&self, control: &OperationControl) -> Result<Local> {
        control.check()?;
        match self.action {
            Action::Init(id) => {
                if self.directory.join("ncc.json").try_exists()? {
                    return Err(invalid(
                        "$.dir",
                        "project is already initialized; edit its metadata explicitly or use plugin sync",
                    ));
                }
                if !self.directory.try_exists()?
                    || self.directory.is_dir() && fs::read_dir(&self.directory)?.next().is_none()
                {
                    project_files::empty_directory(&self.directory)?;
                    return Ok(Local {
                        id,
                        clone: true,
                        layout: "empty",
                        files: BTreeMap::new(),
                        profile: Value::Null,
                        rules: Vec::new(),
                    });
                }
                if !self.directory.is_dir() {
                    return Err(invalid("$.dir", "plugin project must be a directory"));
                }
                storage::guard_path_with_check(&self.directory, &self.directory, || {
                    control.check()
                })?;
                let layout = project_layout(&self.directory, control)?;
                Ok(Local {
                    id,
                    clone: false,
                    layout,
                    files: BTreeMap::new(),
                    profile: Value::Null,
                    rules: Vec::new(),
                })
            }
            Action::Sync { .. } => {
                let manifest = project::manifest(&self.directory, control)?;
                if !manifest.bundle.is_empty() {
                    return Err(invalid(
                        "$.dir",
                        "bundle projects span sibling directories; synchronize their modules explicitly instead",
                    ));
                }
                let id = manifest.id.ok_or_else(|| {
                    invalid(
                        "$.dir",
                        "sync requires an existing ncc.json plugin association",
                    )
                })?;
                let profile = project::local_json(&self.directory.join("ncc.json"), control)?;
                let rules = rule_identity(&project::ignore_patterns(
                    &self.directory,
                    &manifest.ignore,
                    control,
                )?);
                let package = project::Package::select(&self.directory, control)?;
                let result = Local {
                    id,
                    clone: false,
                    layout: package.layout(),
                    files: package.snapshot(),
                    profile,
                    rules,
                };
                package.finish(Ok(result), control)
            }
        }
    }
}
struct Local {
    id: NonZeroU64,
    clone: bool,
    layout: &'static str,
    files: BTreeMap<String, (u64, String)>,
    profile: Value,
    rules: Vec<String>,
}
fn rule_identity(rules: &[regex::Regex]) -> Vec<String> {
    rules
        .iter()
        .map(|v| v.as_str().to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn project_layout(directory: &Path, control: &OperationControl) -> Result<&'static str> {
    if project::has_toc(directory, control)? {
        return Ok("single");
    }
    for path in project::children(directory, control)? {
        let name = local::filename(&path)?;
        if project::omitted(&name, &name, &[]) {
            continue;
        }
        storage::guard_path_with_check(directory, &path, || control.check())?;
        if path.is_dir() && project::has_toc(&path, control)? {
            return Ok("collection");
        }
    }
    Err(invalid(
        "$.dir",
        "directory and its immediate addon directories contain no TOC",
    ))
}
fn latest_versions(detail: &Value, initializing: bool) -> Result<Vec<String>> {
    if initializing && detail.get("latest_versions").is_none_or(Value::is_null) {
        return Ok(Vec::new());
    }
    let values = detail["latest_versions"]
        .as_array()
        .ok_or_else(|| data("official plugin detail omitted latest client versions"))?;
    let values = values
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| data("official latest client versions are malformed"))
        })
        .collect::<Result<Vec<_>>>()?;
    if values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        return Err(data("official latest client versions contain duplicates"));
    }
    Ok(values)
}
struct Stage {
    parent: PathBuf,
    root: PathBuf,
    archive: Option<lycheedev_windows::filesystem::owned_cleanup::OwnedTemporary>,
}
impl Stage {
    fn create(target: &Path) -> Result<Self> {
        let mut parent = target
            .parent()
            .ok_or_else(|| invalid("$.dir", "workspace requires a parent"))?;
        while !parent.try_exists()? {
            parent = parent
                .parent()
                .ok_or_else(|| invalid("$.dir", "workspace requires an existing ancestor"))?;
        }
        let root = parent.join(format!(
            ".lycheedev-creator-{}",
            lycheedev_windows::random::token().map_err(|_| ToolkitError::new(
                "publish.workspace_id",
                "cannot create workspace staging identity",
                3
            ))?
        ));
        storage::guard_path(parent, &root)?;
        fs::create_dir(&root)?;
        match lycheedev_windows::filesystem::owned_cleanup::OwnedTemporary::create(
            &root.join("original.zip"),
        ) {
            Ok(archive) => Ok(Self {
                parent: parent.to_owned(),
                root,
                archive: Some(archive),
            }),
            Err(error) => {
                let _ = fs::remove_dir(&root);
                Err(error.into())
            }
        }
    }
    fn finish(self, mut result: Result<Value>, control: &OperationControl) -> Result<Value> {
        let keep = result.as_ref().err().is_some_and(|error| {
            error.details["local_changes"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        });
        if let Some(archive) = self.archive {
            let file = if keep {
                archive.into_file()
            } else {
                let (file, discard) = archive.discard();
                if discard.is_err() {
                    match &mut result {
                        Ok(value) => value["cleanup_pending"] = json!(true),
                        Err(error) => error.details["cleanup_pending"] = json!(true),
                    }
                }
                file
            };
            let report = result.as_ref().ok().cloned();
            result = storage::finish_file(file, result, control);
            if let (Err(error), Some(report)) = (&mut result, report) {
                error.details["verified_report"] = report;
            }
        }
        if keep {
            if let Err(error) = &mut result {
                error.details["recovery_directory"] = json!(self.root);
            }
            return result;
        }
        let cleanup = storage::guard_path(&self.parent, &self.root)
            .and_then(|_| fs::remove_dir_all(&self.root).map_err(ToolkitError::from));
        if cleanup.is_err() {
            match &mut result {
                Ok(value) => {
                    value["cleanup_pending"] = json!(true);
                    value["work_directory"] = json!(self.root);
                }
                Err(error) => {
                    error.details["cleanup_pending"] = json!(true);
                    error.details["work_directory"] = json!(self.root);
                }
            }
        }
        result
    }
}
impl Official {
    pub fn plugin_workspace(&mut self, request: &PluginWorkspace) -> Result<Value> {
        if matches!(request.action, Action::Sync { confirm: false }) {
            return Err(ToolkitError::arguments(
                "plugin sync replaces packaged content; inspect --plan and explicitly set confirm: true",
            ));
        }
        let local = request.local(&self.control)?;
        let detail = self.call("/mod/publish_detail", &json!({"id":local.id}), false)?;
        plugin_version::check_plugin(&detail, local.id)?;
        let versions = latest_versions(&detail, matches!(request.action, Action::Init(_)))?;
        if matches!(request.action, Action::Init(_)) && !local.clone {
            return self.associate_plugin(request, &local, &detail, &versions);
        }
        let file = &detail["latest_file"];
        let url=file["download_url"].as_str().filter(|v|!v.is_empty()).ok_or_else(||ToolkitError::new("publish.ncc_no_file","plugin has no uploaded version to clone or synchronize; associate an existing addon directory first",3))?;
        let url = crate::transport::object_url(url, true)?;
        let declared = file
            .get("file_size")
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| data("official plugin file size is malformed"))
            })
            .transpose()?;
        let version = file["version_name"]
            .as_str()
            .ok_or_else(|| data("official plugin file omitted its version label"))?
            .to_owned();
        let mut stage = Stage::create(&request.directory)?;
        let result = (|| {
            let archive = stage.archive.as_mut().unwrap().file_mut();
            let length = self.http.download_file(url, archive, project::MAX_ZIP)?;
            if declared.is_some_and(|n| n != length) {
                return Err(data(
                    "downloaded plugin archive length differs from its original metadata",
                ));
            }
            archive.seek(SeekFrom::Start(0))?;
            let payload = stage.root.join("payload");
            fs::create_dir(&payload)?;
            let incoming = project::extract(archive, &payload, &self.control)?;
            project_layout(&payload, &self.control)
                .map_err(|_| data("downloaded plugin archive has no addon TOC layout"))?;
            let mut changes = Vec::new();
            let result = if local.clone {
                self.clone_plugin(
                    request,
                    &local,
                    &detail,
                    &versions,
                    &payload,
                    &incoming,
                    &mut changes,
                )
            } else {
                self.sync_plugin(
                    request,
                    &local,
                    &versions,
                    &payload,
                    &incoming,
                    &mut changes,
                )
            };
            result
                .map(|mut value| {
                    value["version"] = json!(version);
                    value["archive_bytes"] = json!(length);
                    value["verification"] = json!("download_and_local_publication");
                    value["author_writes"] = json!(0);
                    value
                })
                .map_err(|mut error| {
                    error.details["id"] = json!(local.id);
                    error.details["version"] = json!(version);
                    error.details["local_changes"] = json!(changes);
                    error
                })
        })();
        stage
            .finish(result, &self.control)
            .map(|value| output::public(value, &[]))
    }
    fn associate_plugin(
        &self,
        request: &PluginWorkspace,
        local: &Local,
        detail: &Value,
        versions: &[String],
    ) -> Result<Value> {
        request.local(&self.control)?;
        let value = initial_manifest(local.id, &request.directory, detail, versions);
        let bytes = serde_json::to_vec_pretty(&value)?;
        project::save_reader(
            &request.directory.join("ncc.json"),
            &mut Cursor::new(&bytes),
            bytes.len() as u64,
            32 * 1024 * 1024,
            false,
            &self.control,
        )?;
        Ok(
            json!({"id":local.id,"initialized":true,"cloned":false,"directory":request.directory,"layout":local.layout,"manifest_sha256":storage::sha256(&bytes),"game_versions":versions,"game_versions_coverage":if detail.get("latest_versions").is_none_or(Value::is_null){"not_available"}else{"metadata"},"verification":"metadata_read_and_local_publication","author_writes":0}),
        )
    }
    fn clone_plugin(
        &self,
        request: &PluginWorkspace,
        local: &Local,
        detail: &Value,
        versions: &[String],
        payload: &Path,
        incoming: &project::Inventory,
        changes: &mut Vec<Value>,
    ) -> Result<Value> {
        project_files::empty_directory(&request.directory)?;
        fs::create_dir_all(&request.directory)?;
        let description = match detail.get("description") {
            Some(Value::String(v)) if !v.is_empty() => Some(v.as_str()),
            None | Some(Value::Null) | Some(Value::String(_)) => None,
            _ => return Err(data("official plugin description is malformed")),
        };
        let description_name = if detail["content_format"] == 1 {
            "description.html"
        } else {
            "description.md"
        };
        let mut skipped = Vec::new();
        let mut copied = 0;
        for (name, (size, _)) in &incoming.files {
            if name.eq_ignore_ascii_case("ncc.json")
                || description.is_some() && name.eq_ignore_ascii_case(description_name)
            {
                skipped.push(name.clone());
                continue;
            }
            copy_staged(
                payload,
                &request.directory,
                name,
                *size,
                false,
                &self.control,
                changes,
            )?;
            copied += 1;
        }
        if let Some(description) = description {
            publish_bytes(
                &request.directory.join(description_name),
                description.as_bytes(),
                false,
                &self.control,
                changes,
            )?;
        }
        let profile = initial_manifest(local.id, &request.directory, detail, versions);
        let bytes = serde_json::to_vec_pretty(&profile)?;
        publish_bytes(
            &request.directory.join("ncc.json"),
            &bytes,
            false,
            &self.control,
            changes,
        )?;
        Ok(
            json!({"id":local.id,"initialized":true,"cloned":true,"directory":request.directory,"file_count":copied,"description_file":description.map(|_|description_name),"skipped_project_files":skipped,"game_versions":versions,"manifest_sha256":storage::sha256(&bytes)}),
        )
    }
    fn sync_plugin(
        &self,
        request: &PluginWorkspace,
        local: &Local,
        versions: &[String],
        payload: &Path,
        incoming: &project::Inventory,
        changes: &mut Vec<Value>,
    ) -> Result<Value> {
        let manifest = project::manifest(&request.directory, &self.control)?;
        let patterns =
            project::ignore_patterns(&request.directory, &manifest.ignore, &self.control)?;
        if rule_identity(&patterns) != local.rules {
            return Err(ToolkitError::new(
                "publish.project_changed",
                "project ignore selection changed during download; inspect the new selection before synchronizing",
                6,
            ));
        }
        let roots = incoming
            .files
            .keys()
            .filter_map(|v| v.split_once('/').map(|v| v.0.to_owned()))
            .collect::<BTreeSet<_>>();
        if local.layout == "single" && roots.len() != 1 {
            return Err(invalid(
                "$.dir",
                "single-addon project requires exactly one online module; clone the collection into a new directory",
            ));
        }
        let mut selected = BTreeMap::new();
        for (name, identity) in &incoming.files {
            let Some((_, rest)) = name.split_once('/') else {
                continue;
            };
            let relative = if local.layout == "single" { rest } else { name };
            if ignored_path(relative, &patterns) {
                continue;
            }
            selected.insert(relative.to_owned(), (name.clone(), identity.clone()));
        }
        if selected.is_empty() {
            return Err(data(
                "online plugin version contains no selected content after project ignore rules",
            ));
        }
        // Re-read the manifest before touching files. The original metadata
        // snapshot is kept until all content publications have completed.
        if project::local_json(&request.directory.join("ncc.json"), &self.control)? != local.profile
        {
            return Err(ToolkitError::new(
                "publish.project_changed",
                "project metadata changed during download; inspect the original selection before synchronizing",
                6,
            ));
        }
        let mut removed = 0;
        let mut replaced = 0;
        let mut unchanged = 0;
        for (name, (size, sha256)) in &local.files {
            if selected.contains_key(name) {
                continue;
            }
            let path = request.directory.join(name);
            storage::guard_path_with_check(&request.directory, &path, || self.control.check())?;
            let cleanup = storage::authenticate_cleanup_file(&path, *size, sha256, &self.control)?;
            let (file, discard) = cleanup.discard();
            let discarded = discard.map_err(ToolkitError::from);
            if discarded.is_ok() {
                changes.push(json!({"file":name,"operation":"deletion_submitted"}));
            }
            storage::finish_file(file, discarded, &self.control)?;
            removed += 1;
        }
        for (relative, (source, (size, sha256))) in &selected {
            if local.files.get(relative) == Some(&(*size, sha256.clone())) {
                let (file, hash) = storage::locked_sha256(
                    &request.directory.join(relative),
                    u64::MAX,
                    &self.control,
                )?;
                let matches = hash == *sha256 && file.metadata()?.len() == *size;
                storage::finish_file(file, Ok(()), &self.control)?;
                if matches {
                    unchanged += 1;
                    continue;
                }
            }
            let destination = request.directory.join(relative);
            storage::guard_path_with_check(&request.directory, &destination, || {
                self.control.check()
            })?;
            fs::create_dir_all(destination.parent().unwrap())?;
            let (mut input, hash) =
                storage::locked_sha256(&payload.join(source), u64::MAX, &self.control)?;
            let result = (|| {
                if hash != *sha256 {
                    return Err(data(
                        "staged plugin content differs from its original archive",
                    ));
                }
                input.seek(SeekFrom::Start(0))?;
                let result = project::save_reader(
                    &destination,
                    &mut input,
                    *size,
                    u64::MAX,
                    true,
                    &self.control,
                );
                record_publication(result, relative, changes)
            })();
            storage::finish_file(input, result, &self.control)?;
            replaced += 1;
        }
        let empty_directories_removed =
            remove_empty_dirs(&request.directory, &patterns, &self.control, changes)?;
        if project::local_json(&request.directory.join("ncc.json"), &self.control)? != local.profile
        {
            return Err(ToolkitError::new(
                "publish.project_changed",
                "content was synchronized but project metadata changed; retain the local change report and original snapshot",
                6,
            ));
        }
        let mut profile = local.profile.clone();
        profile["game_versions"] = json!(versions);
        let bytes = serde_json::to_vec_pretty(&profile)?;
        publish_bytes(
            &request.directory.join("ncc.json"),
            &bytes,
            true,
            &self.control,
            changes,
        )?;
        Ok(
            json!({"id":local.id,"synced":true,"directory":request.directory,"layout":local.layout,"replaced_count":replaced,"removed_count":removed,"unchanged_count":unchanged,"empty_directories_removed":empty_directories_removed,"online_content_files":selected.len(),"game_versions":versions,"manifest_sha256":storage::sha256(&bytes)}),
        )
    }
}
fn initial_manifest(
    id: NonZeroU64,
    directory: &Path,
    detail: &Value,
    versions: &[String],
) -> Value {
    json!({"game":"wow","type":"mod","id":id,"name":detail.get("name").or_else(||detail.get("t_name")),"version":"","game_versions":versions,"changelog_file":if directory.join("CHANGELOG.md").is_file(){"CHANGELOG.md"}else{""},"ignore":["*.psd","node_modules/**"]})
}
fn ignored_path(path: &str, patterns: &[regex::Regex]) -> bool {
    let mut relative = String::new();
    for name in path.split('/') {
        if !relative.is_empty() {
            relative.push('/');
        }
        relative.push_str(name);
        if project::omitted(name, &relative, patterns) {
            return true;
        }
    }
    false
}
fn remove_empty_dirs(
    root: &Path,
    patterns: &[regex::Regex],
    control: &OperationControl,
    changes: &mut Vec<Value>,
) -> Result<usize> {
    let mut pending = vec![(root.to_owned(), false)];
    let mut removed = 0;
    while let Some((path, visited)) = pending.pop() {
        control.check()?;
        if visited {
            if path != root && fs::read_dir(&path)?.next().transpose()?.is_none() {
                storage::guard_path_with_check(root, &path, || control.check())?;
                fs::remove_dir(&path)?;
                changes.push(json!({"file":path.strip_prefix(root).unwrap(),"operation":"empty_directory_removed"}));
                removed += 1;
            }
            continue;
        }
        pending.push((path.clone(), true));
        for child in project::children(&path, control)? {
            let relative = child
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .ok_or_else(|| invalid("$.dir", "project paths must be UTF-8"))?
                .replace('\\', "/");
            if ignored_path(&relative, patterns) {
                continue;
            }
            storage::guard_path_with_check(root, &child, || control.check())?;
            if child.is_dir() {
                pending.push((child, false));
            }
        }
    }
    Ok(removed)
}
fn copy_staged(
    root: &Path,
    destination: &Path,
    name: &str,
    size: u64,
    overwrite: bool,
    control: &OperationControl,
    changes: &mut Vec<Value>,
) -> Result<()> {
    let source = root.join(name);
    let target = destination.join(name);
    storage::guard_path_with_check(destination, &target, || control.check())?;
    fs::create_dir_all(target.parent().unwrap())?;
    let (mut file, _) = storage::locked_sha256(&source, u64::MAX, control)?;
    file.seek(SeekFrom::Start(0))?;
    let result = project::save_reader(&target, &mut file, size, u64::MAX, overwrite, control);
    storage::finish_file(file, record_publication(result, name, changes), control)
}
fn publish_bytes(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
    control: &OperationControl,
    changes: &mut Vec<Value>,
) -> Result<()> {
    let name = path.file_name().unwrap().to_string_lossy();
    record_publication(
        project::save_reader(
            path,
            &mut Cursor::new(bytes),
            bytes.len() as u64,
            32 * 1024 * 1024,
            overwrite,
            control,
        ),
        &name,
        changes,
    )
}
fn record_publication(result: Result<()>, name: &str, changes: &mut Vec<Value>) -> Result<()> {
    match result {
        Ok(()) => {
            changes.push(json!({"file":name,"operation":"published"}));
            Ok(())
        }
        Err(error) => {
            if matches!(
                error.details["output_publication"].as_str(),
                Some("published" | "unknown")
            ) {
                changes.push(json!({"file":name,"operation":error.details["output_publication"]}));
            }
            Err(error)
        }
    }
}
#[cfg(test)]
mod tests;
