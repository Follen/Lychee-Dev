use super::local::{SECTIONS, component, files, wtf_file};
use super::*;
use crate::{input, local_backup as shared};
use lycheedev_core::storage;
use std::path::PathBuf;

pub(super) struct Group {
    pub key: String,
    pub kind: u8,
    pub entries: Vec<(PathBuf, String)>,
    pub font: Option<PathBuf>,
    pub account: Option<Value>,
}
pub(super) struct Snapshot {
    pub path: PathBuf,
    pub root: PathBuf,
    pub manifest: Value,
    pub sections: BTreeMap<String, Vec<Value>>,
    pub groups: Vec<Group>,
    pub fonts: Vec<Value>,
    pub files: Value,
    pub build: u8,
}
impl Snapshot {
    pub fn open(path: &Path, selected: Option<u8>, control: &OperationControl) -> Result<Self> {
        let root = path
            .parent()
            .ok_or_else(|| bad("manifest_file must have a backup directory"))?;
        storage::guard_path(root, path)?;
        let (file, _, text) = input::locked_text(path, control)?;
        let manifest = storage::finish_file(
            file,
            input::value(text.trim_start_matches('\u{feff}').as_bytes()),
            control,
        )?;
        if manifest["schemaVersion"] != "config-backup.v2" {
            return Err(bad("manifest_file must use native config-backup.v2"));
        }
        let build = manifest["source"]["gameId"]
            .as_u64()
            .filter(|n| *n <= 4)
            .ok_or_else(|| bad("manifest source.gameId must be 0..4"))? as u8;
        if selected.is_some_and(|b| b != build) {
            return Err(invalid(
                "$.server_type",
                "manifest gameId differs from the selected server_type",
            ));
        }
        let id = manifest["backupId"]
            .as_str()
            .ok_or_else(|| bad("manifest backupId is required"))?;
        component(id)?;
        if manifest["backupName"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty() || s.chars().count() > 80)
        {
            return Err(bad("manifest backupName must contain 1..80 characters"));
        }
        let targets = manifest["storageTargets"]
            .as_array()
            .ok_or_else(|| bad("manifest storageTargets must be an array"))?;
        if targets
            .iter()
            .any(|v| v.as_str().is_none_or(|s| !["local", "cloud"].contains(&s)))
        {
            return Err(bad(
                "manifest storageTargets contains an unknown destination",
            ));
        }
        let rows = manifest["sections"]
            .as_array()
            .filter(|a| a.len() == 6)
            .ok_or_else(|| bad("manifest requires all six native sections"))?;
        let mut sections = BTreeMap::new();
        let mut ids = BTreeSet::new();
        for row in rows {
            let key = row["key"]
                .as_str()
                .filter(|k| SECTIONS.contains(k))
                .ok_or_else(|| bad("manifest has an unknown section"))?;
            let items = row["items"]
                .as_array()
                .ok_or_else(|| bad("manifest section items must be an array"))?;
            validate_items(items, key, 0, &mut ids)?;
            if sections.insert(key.to_string(), items.clone()).is_some() {
                return Err(bad("manifest repeats a section"));
            }
        }
        let mut groups = Vec::new();
        let mut fingerprints = serde_json::Map::new();
        let mut fonts = Vec::new();
        for key in SECTIONS {
            if key == "wtf" {
                continue;
            }
            let items = &sections[key];
            if key == "addons" {
                let mut projects = BTreeSet::new();
                for item in items {
                    let project = number(&item["meta"]["projectId"])?;
                    if !projects.insert(project) {
                        return Err(bad("known addons repeat a projectId"));
                    }
                    if item["meta"]
                        .get("isLocked")
                        .is_some_and(|v| !v.is_boolean())
                    {
                        return Err(bad("known addon isLocked must be boolean"));
                    }
                    if item["meta"]["isLocked"] == true {
                        number(&item["meta"]["fileId"])?;
                    }
                    if item["children"].as_array().is_some_and(|a| !a.is_empty()) {
                        return Err(bad(
                            "known addon references must be individual project items",
                        ));
                    }
                    component(item["name"].as_str().unwrap())?;
                }
                continue;
            }
            let mut entries = Vec::new();
            for item in leaves(items) {
                let name = component(item["name"].as_str().unwrap())?;
                if key == "materials" && name.eq_ignore_ascii_case("AddOns") {
                    return Err(bad("materials cannot select Interface/AddOns"));
                }
                let prefix = match key {
                    "fonts" => "Fonts",
                    "materials" => "Interface",
                    _ => "Interface/AddOns",
                };
                let path = require_path(root, item, &format!("{prefix}/{name}"), control)?;
                if key == "fonts" {
                    if !path.is_file() {
                        return Err(bad("font items must be individual files"));
                    }
                    let (mut file, sha) = storage::locked_sha256(&path, u64::MAX, control)?;
                    let size = file.metadata()?.len();
                    let md5 = shared::md5(&mut file, control);
                    let md5 = storage::finish_file(file, md5, control)?;
                    if item["meta"]
                        .get("md5")
                        .is_some_and(|v| v.as_str().is_none_or(|s| !s.eq_ignore_ascii_case(&md5)))
                    {
                        return Err(bad("font bytes differ from manifest meta.md5"));
                    }
                    fonts.push(json!({"name":name,"md5":md5,"size":size}));
                    fingerprints.insert(
                        shared::member_path(&path, root)?,
                        json!({"size":size,"sha256":sha}),
                    );
                    groups.push(Group {
                        key: format!("font:{md5}"),
                        kind: 3,
                        entries: vec![],
                        font: Some(path),
                        account: None,
                    });
                } else {
                    for file in files(root, &path, control)? {
                        let member = shared::member_path(&file, path.parent().unwrap())?;
                        entries.push((file, member));
                    }
                }
            }
            if key != "fonts" && !items.is_empty() {
                groups.push(Group {
                    key: key.into(),
                    kind: match key {
                        "modifiedAddons" => 8,
                        "unknownAddons" => 9,
                        _ => 4,
                    },
                    entries,
                    font: None,
                    account: None,
                });
            }
        }
        groups.extend(wtf(root, &sections["wtf"], control)?);
        for group in &groups {
            let mut members = BTreeSet::new();
            for (path, member) in &group.entries {
                if !members.insert(member.to_lowercase()) {
                    return Err(bad("backup archive repeats a Windows member path"));
                }
                let relative = shared::member_path(path, root)?;
                if !fingerprints.contains_key(&relative) {
                    let (file, sha) = storage::locked_sha256(path, u64::MAX, control)?;
                    let size = file.metadata()?.len();
                    storage::finish_file(file, Ok(()), control)?;
                    fingerprints.insert(relative, json!({"size":size,"sha256":sha}));
                }
            }
        }
        for (section, items) in &sections {
            if section != "addons" {
                check_sizes(items, &fingerprints)?;
            }
        }
        // Equal font content is one cloud object even when selected under two
        // filenames; both display entries remain in the backup's fonts list.
        let mut keys = BTreeSet::new();
        groups.retain(|g| keys.insert(g.key.clone()));
        Ok(Self {
            path: path.to_owned(),
            root: root.to_owned(),
            manifest,
            sections,
            groups,
            fonts,
            files: Value::Object(fingerprints),
            build,
        })
    }
    pub fn identity(&self) -> Value {
        json!({"manifest":canonical(self.manifest.clone()),"files":self.files})
    }
    pub fn cloud_manifest(&self) -> Value {
        let mut doc = canonical(self.manifest.clone());
        doc["storageTargets"] = json!(["local", "cloud"]);
        sanitize(&mut doc);
        doc
    }
    pub fn link(&self, id: u64, control: &OperationControl) -> Result<()> {
        let (file, _, text) = input::locked_text(&self.path, control)?;
        let current = storage::finish_file(
            file,
            input::value(text.trim_start_matches('\u{feff}').as_bytes()),
            control,
        )?;
        if canonical(current) != canonical(self.manifest.clone()) {
            return Err(bad(
                "local manifest changed after upload; retain the original cloud backup and restore its manifest before linking",
            ));
        }
        let mut linked = self.manifest.clone();
        linked["storageTargets"] = json!(["local", "cloud"]);
        linked["source"]["cloudBackupId"] = json!(id.to_string());
        storage::atomic_json(&self.path, &linked)
    }
}
pub(super) fn canonical(mut doc: Value) -> Value {
    if let Some(source) = doc["source"].as_object_mut() {
        source.remove("cloudBackupId");
    }
    doc["storageTargets"] = json!(["local"]);
    doc
}
fn check_sizes(items: &[Value], files: &serde_json::Map<String, Value>) -> Result<()> {
    for item in items {
        if let Some(declared) = item.get("sizeBytes") {
            let expected = declared
                .as_u64()
                .ok_or_else(|| bad("item sizeBytes must be a nonnegative integer"))?;
            if let Some(path) = item["targetRelativePath"].as_str() {
                let prefix = format!("{path}/").to_lowercase();
                let path = path.to_lowercase();
                let actual = files
                    .iter()
                    .filter(|(key, _)| {
                        key.to_lowercase() == path || key.to_lowercase().starts_with(&prefix)
                    })
                    .map(|(_, value)| value["size"].as_u64().unwrap())
                    .try_fold(0u64, |sum, n| {
                        sum.checked_add(n)
                            .ok_or_else(|| bad("backup size exceeds its numeric range"))
                    })?;
                if actual != expected {
                    return Err(bad(
                        "selected backup files differ from manifest sizeBytes; restore the original backup or generate a new backup directory",
                    ));
                }
            }
        }
        if let Some(children) = item["children"].as_array() {
            check_sizes(children, files)?;
        }
    }
    Ok(())
}
fn sanitize(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for k in ["sourcePath", "gamePath", "gameRoot", "backupPath"] {
                m.remove(k);
            }
            for v in m.values_mut() {
                sanitize(v);
            }
        }
        Value::Array(a) => {
            for v in a {
                sanitize(v);
            }
        }
        _ => {}
    }
}
fn validate_items(
    items: &[Value],
    key: &str,
    depth: usize,
    ids: &mut BTreeSet<String>,
) -> Result<()> {
    if depth > 20 {
        return Err(bad("manifest item hierarchy exceeds 20 levels"));
    }
    for item in items {
        if item["section"] != key
            || item["id"]
                .as_str()
                .is_none_or(|s| s.is_empty() || !ids.insert(s.into()))
            || item["name"].as_str().is_none()
            || item.get("meta").is_some_and(|v| !v.is_object())
        {
            return Err(bad(
                "manifest item needs a unique id, matching section, name and object metadata",
            ));
        }
        if let Some(c) = item.get("children") {
            validate_items(
                c.as_array()
                    .ok_or_else(|| bad("manifest children must be an array"))?,
                key,
                depth + 1,
                ids,
            )?;
        }
    }
    Ok(())
}
fn leaves(items: &[Value]) -> Vec<&Value> {
    let mut out = Vec::new();
    for item in items {
        if let Some(c) = item["children"].as_array().filter(|a| !a.is_empty()) {
            out.extend(leaves(c));
        } else {
            out.push(item);
        }
    }
    out
}
fn require_path(
    root: &Path,
    item: &Value,
    expected: &str,
    control: &OperationControl,
) -> Result<PathBuf> {
    if item["targetRelativePath"] != expected {
        return Err(bad(
            "item targetRelativePath differs from its category/account/server/role",
        ));
    }
    shared::path(root, &expected.split('/').collect::<Vec<_>>(), control)
}
fn number(v: &Value) -> Result<u64> {
    identity(v)
        .and_then(|s| s.parse().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| bad("manifest project/file ID must be a positive integer"))
}
fn bad(message: &str) -> ToolkitError {
    invalid("$.manifest_file", message)
}
fn wtf(root: &Path, items: &[Value], control: &OperationControl) -> Result<Vec<Group>> {
    let mut accounts = Vec::new();
    let mut extras = Vec::new();
    let mut seen = BTreeSet::new();
    for item in items {
        let kind = item["meta"]["type"].as_str().unwrap_or("");
        if ["config", "shared-saved-variables"].contains(&kind) {
            if !seen.insert(format!("extra:{kind}"))
                || item["children"].as_array().is_some_and(|a| !a.is_empty())
            {
                return Err(bad("duplicate or nested WTF common selection"));
            }
            let expected = if kind == "config" {
                "WTF/Config.wtf"
            } else {
                "WTF/Account/SavedVariables"
            };
            let path = require_path(root, item, expected, control)?;
            if kind == "config" && (item["name"] != "Config.wtf" || !path.is_file())
                || kind == "shared-saved-variables" && !path.is_dir()
            {
                return Err(bad("WTF common selection has the wrong type"));
            }
            for file in wtf_files(root, &path, control)? {
                let member = shared::member_path(&file, path.parent().unwrap())?;
                extras.push((file, member));
            }
            continue;
        }
        if kind != "account" {
            return Err(bad(
                "top-level WTF selections must be accounts or explicit common files",
            ));
        }
        let name = component(item["name"].as_str().unwrap())?;
        if name.eq_ignore_ascii_case("SavedVariables")
            || !seen.insert(format!("account:{}", name.to_lowercase()))
            || item["meta"].get("account").is_some_and(|v| v != name)
        {
            return Err(bad("invalid or duplicate WTF account"));
        }
        let path = require_path(root, item, &format!("WTF/Account/{name}"), control)?;
        if !path.is_dir() {
            return Err(bad("WTF account directory is missing"));
        }
        let servers = item["children"]
            .as_array()
            .ok_or_else(|| bad("WTF account requires its server hierarchy"))?;
        let mut server_names = BTreeSet::new();
        let mut selected = BTreeSet::new();
        let mut roles = Vec::new();
        for server in servers {
            let server_name = component(server["name"].as_str().unwrap())?;
            if server["meta"]["type"] != "server"
                || server_name.eq_ignore_ascii_case("SavedVariables")
                || !server_names.insert(server_name.to_lowercase())
                || server["meta"].get("account").is_some_and(|v| v != name)
                || server["meta"]
                    .get("server")
                    .is_some_and(|v| v != server_name)
            {
                return Err(bad("invalid WTF server metadata"));
            }
            require_path(
                root,
                server,
                &format!("WTF/Account/{name}/{server_name}"),
                control,
            )?;
            for role in server["children"]
                .as_array()
                .ok_or_else(|| bad("WTF server requires explicit role leaves"))?
            {
                let role_name = component(role["name"].as_str().unwrap())?;
                if role["meta"]["type"] != "role"
                    || role["children"].as_array().is_some_and(|a| !a.is_empty())
                    || role_name.eq_ignore_ascii_case("SavedVariables")
                    || !selected.insert((server_name.to_lowercase(), role_name.to_lowercase()))
                    || role["meta"].get("account").is_some_and(|v| v != name)
                    || role["meta"].get("server").is_some_and(|v| v != server_name)
                {
                    return Err(bad("invalid WTF role metadata"));
                }
                if !require_path(
                    root,
                    role,
                    &format!("WTF/Account/{name}/{server_name}/{role_name}"),
                    control,
                )?
                .is_dir()
                {
                    return Err(bad("WTF role directory is missing"));
                }
                roles.push(format!("{server_name}-{role_name}"));
            }
        }
        let mut entries = Vec::new();
        for file in wtf_files(root, &path, control)? {
            let rel = shared::member_path(&file, &path)?;
            let parts = rel.split('/').map(str::to_lowercase).collect::<Vec<_>>();
            let common = parts.len() == 1 || parts[0] == "savedvariables";
            let server_common = parts.len() >= 2
                && server_names.contains(&parts[0])
                && (parts.len() == 2 || parts[1] == "savedvariables");
            let role = parts.len() >= 3 && selected.contains(&(parts[0].clone(), parts[1].clone()));
            if !common && !server_common && !role {
                return Err(bad(
                    "WTF backup contains an unselected character/server file",
                ));
            }
            entries.push((file, format!("{name}/{rel}")));
        }
        accounts.push(Group {
            key: format!("wtf:{name}"),
            kind: 7,
            entries,
            font: None,
            account: Some(json!({"name":name,"roles":roles})),
        });
    }
    if accounts.is_empty() && !extras.is_empty() {
        return Err(bad(
            "WTF common files need an explicit account to receive them",
        ));
    }
    for account in &mut accounts {
        account.entries.extend(extras.iter().cloned());
    }
    Ok(accounts)
}
fn wtf_files(root: &Path, path: &Path, control: &OperationControl) -> Result<Vec<PathBuf>> {
    let files = files(root, path, control)?;
    if files.iter().any(|p| !wtf_file(p)) {
        return Err(bad(
            "WTF admits .lua/.wtf/.txt files and excludes BigFootSync.lua",
        ));
    }
    Ok(files)
}
