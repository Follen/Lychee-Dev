use super::*;
use crate::{input, local_backup as local};
use lycheedev_core::storage;
use std::{fs, path::PathBuf};

pub(super) const SECTIONS: [&str; 6] = [
    "addons",
    "modifiedAddons",
    "unknownAddons",
    "fonts",
    "materials",
    "wtf",
];
pub struct BackupLocal {
    action: String,
    doc: Value,
    root: PathBuf,
    output: Option<PathBuf>,
}
struct Copy {
    section: &'static str,
    name: String,
    source: PathBuf,
    relative: String,
    meta: Value,
    files: Vec<PathBuf>,
}

impl BackupLocal {
    pub fn parse(action: &str, bytes: &[u8], cwd: &Path) -> Result<Self> {
        if !["scan", "generate", "create"].contains(&action) {
            return Err(ToolkitError::arguments("unknown ModUs local backup action"));
        }
        let doc = document(bytes)?;
        input_shape(Provider::Modus, "backup", action)?.admit(&doc)?;
        let mut doc = Value::Object(doc);
        if action != "scan" {
            let name = doc["backup_name"].as_str().unwrap().trim().to_owned();
            if name.is_empty() {
                return Err(invalid(
                    "$.backup_name",
                    "backup name must contain 1..80 characters",
                ));
            }
            doc["backup_name"] = json!(name);
            let mut count = 0;
            for key in ["addon_names", "font_names", "material_names"] {
                if doc.get(key).is_none() {
                    doc[key] = json!([]);
                }
                let names =
                    serde_json::from_value::<Vec<String>>(doc[key].clone()).map_err(|_| {
                        invalid(&format!("$.{key}"), "select an array of individual names")
                    })?;
                local::distinct_names(&names)?;
                for name in &names {
                    component(name)?;
                    if key == "font_names"
                        && ![".ttf", ".otf"]
                            .iter()
                            .any(|s| name.to_lowercase().ends_with(s))
                    {
                        return Err(invalid(
                            "$.font_names",
                            "select .ttf or .otf font filenames",
                        ));
                    }
                    if key == "material_names" && name.eq_ignore_ascii_case("AddOns") {
                        return Err(invalid(
                            "$.material_names",
                            "select Interface/AddOns through addon_names",
                        ));
                    }
                }
                count += names.len();
            }
            if doc.get("roles").is_none() {
                doc["roles"] = json!([]);
            }
            let mut seen = BTreeSet::new();
            for role in doc["roles"].as_array().unwrap() {
                if role.as_object().is_none_or(|r| r.len() != 3) {
                    return Err(invalid(
                        "$.roles",
                        "each role requires exactly account, server and role names",
                    ));
                }
                let mut parts = Vec::new();
                for key in ["account", "server", "role"] {
                    let name = role[key].as_str().ok_or_else(|| {
                        invalid("$.roles", "account, server and role must be names")
                    })?;
                    component(name)?;
                    if name.eq_ignore_ascii_case("SavedVariables") {
                        return Err(invalid(
                            "$.roles",
                            "SavedVariables is shared configuration, not an account/server/role",
                        ));
                    }
                    parts.push(name.to_lowercase());
                }
                if !seen.insert(parts) {
                    return Err(invalid(
                        "$.roles",
                        "role selections must be unique ignoring case",
                    ));
                }
                count += 1;
            }
            if count == 0 {
                return Err(invalid(
                    "$",
                    "select at least one addon, font, material or role; use backup scan",
                ));
            }
        }
        let root = std::path::absolute(cwd.join(doc["game_root"].as_str().unwrap()))?;
        let output = doc["output_dir"]
            .as_str()
            .map(|s| local::resolve_output(&cwd.join(s)))
            .transpose()?;
        Ok(Self {
            action: action.into(),
            doc,
            root,
            output,
        })
    }
    pub fn schema(action: &str) -> Value {
        let mut s = input_shape(Provider::Modus, "backup", action)
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        if action != "scan" {
            for key in ["addon_names", "font_names", "material_names"] {
                s["properties"][key]["items"] = json!({"type":"string","minLength":1,"description":"One ordinary Windows name; no path separators, commas, reserved devices or trailing dots/spaces."});
                s["properties"][key]["uniqueItems"] = json!(true);
            }
            s["properties"]["roles"]["items"] = json!({"type":"object","required":["account","server","role"],"additionalProperties":false,"properties":{"account":{"type":"string","minLength":1},"server":{"type":"string","minLength":1},"role":{"type":"string","minLength":1}},"description":"An exact role from scan. Copies only selected role trees and their account SavedVariables; includes Config.wtf when present. No full-account default."});
            s["anyOf"] = json!([{"required":["addon_names"],"properties":{"addon_names":{"minItems":1}}},{"required":["font_names"],"properties":{"font_names":{"minItems":1}}},{"required":["material_names"],"properties":{"material_names":{"minItems":1}}},{"required":["roles"],"properties":{"roles":{"minItems":1}}}]);
        }
        s
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let root = self.root(control)?;
        if self.action == "scan" {
            return self.scan(&root, control);
        }
        let (output, selection) = self.destination(&root)?;
        if let Some(manifest) = self.existing(&output, &selection, control)? {
            return Ok(
                json!({"platform":"modus","action":self.action,"planned":true,"reused":true,"manifest_file":output.join("manifest.v2.json"),"server_type":manifest["source"]["gameId"],"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false}),
            );
        }
        let copies = self.copies(&root, control)?;
        Ok(
            json!({"platform":"modus","action":self.action,"planned":true,"output_dir":output,"server_type":self.doc.get("server_type"),"selected_files":copies.iter().map(|p|p.files.len()).sum::<usize>(),"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false,"next":"Generation creates one native config-backup.v2 directory. An omitted build is resolved from the main client's current selection at execution (0 when unset). Repeating the input reuses the original manifest; create then uploads that same backup."}),
        )
    }
    pub fn execute(&self, home: &Path, control: &OperationControl) -> Result<Value> {
        let root = self.root(control)?;
        if self.action == "scan" {
            return self.scan(&root, control);
        }
        let result = self.generate(&root, control)?;
        if self.action == "create" {
            let doc = json!({"schema":"lycheedev.publish.v1.modus.backup.upload","manifest_file":result["manifest_file"],"server_type":result["server_type"]});
            super::upload::BackupUpload::parse(&serde_json::to_vec(&doc)?, Path::new(""))?
                .execute(home, control)
        } else {
            Ok(result)
        }
    }
    fn root(&self, control: &OperationControl) -> Result<PathBuf> {
        storage::guard_path_with_check(&self.root, &self.root.join("Interface/AddOns"), || {
            control.check()
        })?;
        if !self.root.join("Interface/AddOns").is_dir() {
            return Err(invalid(
                "$.game_root",
                "select the game branch containing Interface/AddOns",
            ));
        }
        Ok(fs::canonicalize(&self.root)?)
    }
    fn destination(&self, root: &Path) -> Result<(PathBuf, Value)> {
        let output = self.output.as_ref().unwrap().clone();
        if output.starts_with(root) {
            return Err(invalid(
                "$.output_dir",
                "backup output must be outside game_root",
            ));
        }
        let mut selection = self.doc.clone();
        let object = selection.as_object_mut().unwrap();
        object.remove("schema");
        object.remove("output_dir");
        object.remove("game_root");
        object.insert("gameRoot".into(), json!(root));
        Ok((output, selection))
    }
    fn existing(
        &self,
        output: &Path,
        selection: &Value,
        control: &OperationControl,
    ) -> Result<Option<Value>> {
        if !output.exists() {
            return Ok(None);
        }
        let path = output.join("manifest.v2.json");
        storage::guard_path(output, &path)?;
        if !path.is_file() {
            return Err(invalid(
                "$.output_dir",
                "output already exists without a completed manifest; retain it and choose an empty output directory",
            ));
        }
        let (file, _, text) = input::locked_text(&path, control)?;
        let manifest = storage::finish_file(
            file,
            input::value(text.trim_start_matches('\u{feff}').as_bytes()),
            control,
        )?;
        if manifest["schemaVersion"] != "config-backup.v2"
            || manifest["source"]["localSelection"] != *selection
            || manifest["source"]["gameId"].as_u64().is_none_or(|n| n > 4)
        {
            return Err(invalid(
                "$.output_dir",
                "existing backup belongs to a different selection; use its original input or a new output directory",
            ));
        }
        super::manifest::Snapshot::open(&path, None, control)?;
        Ok(Some(manifest))
    }
    fn copies(&self, root: &Path, control: &OperationControl) -> Result<Vec<Copy>> {
        let mut plans = Vec::new();
        for (field, section, prefix) in [
            ("addon_names", "unknownAddons", "Interface/AddOns"),
            ("font_names", "fonts", "Fonts"),
            ("material_names", "materials", "Interface"),
        ] {
            for value in self.doc[field].as_array().unwrap() {
                let name = value.as_str().unwrap();
                let relative = format!("{prefix}/{name}");
                let source = local::path(root, &relative.split('/').collect::<Vec<_>>(), control)?;
                if section == "fonts" && !source.is_file()
                    || section == "unknownAddons" && !source.is_dir()
                {
                    return Err(invalid(
                        &format!("$.{field}"),
                        "selected component has the wrong file/directory type",
                    ));
                }
                plans.push(Copy {
                    section,
                    name: name.into(),
                    source,
                    relative,
                    meta: json!({}),
                    files: vec![],
                });
            }
        }
        let mut accounts = BTreeSet::new();
        for role in self.doc["roles"].as_array().unwrap() {
            let account = role["account"].as_str().unwrap();
            let server = role["server"].as_str().unwrap();
            let name = role["role"].as_str().unwrap();
            accounts.insert(account);
            let relative = format!("WTF/Account/{account}/{server}/{name}");
            let source = local::path(root, &relative.split('/').collect::<Vec<_>>(), control)?;
            if !source.is_dir() {
                return Err(invalid(
                    "$.roles",
                    "selected character directory is missing",
                ));
            }
            plans.push(Copy {
                section: "wtf",
                name: name.into(),
                source,
                relative,
                meta: json!({"type":"role","account":account,"server":server}),
                files: vec![],
            });
        }
        for account in &accounts {
            let relative = format!("WTF/Account/{account}/SavedVariables");
            let source = local::path(root, &relative.split('/').collect::<Vec<_>>(), control)?;
            if source.is_dir() {
                plans.push(Copy {
                    section: "wtf",
                    name: format!("{account} SavedVariables"),
                    source,
                    relative,
                    meta: json!({"type":"account-shared","account":account}),
                    files: vec![],
                });
            }
        }
        if !accounts.is_empty() {
            let source = local::path(root, &["WTF", "Config.wtf"], control)?;
            if source.is_file() {
                plans.push(Copy {
                    section: "wtf",
                    name: "Config.wtf".into(),
                    source,
                    relative: "WTF/Config.wtf".into(),
                    meta: json!({"type":"config"}),
                    files: vec![],
                });
            }
        }
        for p in &mut plans {
            p.files = files(root, &p.source, control)?;
            if p.section == "wtf" {
                p.files.retain(|p| wtf_file(p));
            }
        }
        Ok(plans)
    }
    fn generate(&self, root: &Path, control: &OperationControl) -> Result<Value> {
        let (output, selection) = self.destination(root)?;
        if let Some(manifest) = self.existing(&output, &selection, control)? {
            return Ok(generated(&output, &manifest, true));
        }
        let copies = self.copies(root, control)?;
        let build = if let Some(n) = self.doc["server_type"].as_u64() {
            n
        } else {
            auth::builds(control)?["current"].as_u64().unwrap_or(0)
        };
        fs::create_dir_all(&output)?;
        let mut items: BTreeMap<&str, Vec<Value>> =
            SECTIONS.into_iter().map(|s| (s, vec![])).collect();
        for p in copies {
            control.check()?;
            let target = output.join(&p.relative);
            if p.source.is_dir() {
                fs::create_dir_all(&target)?;
            }
            let mut size = 0;
            for source in p.files {
                let dest = if p.source.is_dir() {
                    target.join(source.strip_prefix(&p.source).unwrap())
                } else {
                    target.clone()
                };
                storage::guard_path(&output, &dest)?;
                fs::create_dir_all(dest.parent().unwrap())?;
                local::copy(root, &source, &dest, control)?;
                size += dest.metadata()?.len();
            }
            let mut meta = p.meta;
            if p.section == "fonts" {
                let mut file = local::open_source(&target)?;
                let value = local::md5(&mut file, control);
                meta["md5"] = json!(storage::finish_file(file, value, control)?);
            }
            items.get_mut(p.section).unwrap().push(json!({"id":format!("{}:{}",p.section,p.relative),"section":p.section,"name":p.name,"targetRelativePath":p.relative,"sizeBytes":size,"meta":meta}));
        }
        let wtf = items.remove("wtf").unwrap();
        let mut grouped = wtf
            .iter()
            .filter(|r| r["meta"]["type"] == "config")
            .cloned()
            .collect::<Vec<_>>();
        let accounts = self.doc["roles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["account"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        for account in &accounts {
            let leaves = wtf
                .iter()
                .filter(|r| r["meta"]["account"] == *account)
                .collect::<Vec<_>>();
            let servers = leaves
                .iter()
                .filter_map(|r| r["meta"]["server"].as_str())
                .collect::<BTreeSet<_>>();
            let children = servers.into_iter().map(|server| {
                let roles = leaves.iter().filter(|r|r["meta"]["server"]==server).map(|r|(*r).clone()).collect::<Vec<_>>();
                json!({"id":format!("wtf:server:{account}:{server}"),"section":"wtf","name":server,"targetRelativePath":format!("WTF/Account/{account}/{server}"),"sizeBytes":roles.iter().map(|r|r["sizeBytes"].as_u64().unwrap()).sum::<u64>(),"children":roles,"meta":{"type":"server"}})
            }).collect::<Vec<_>>();
            grouped.push(json!({"id":format!("wtf:account:{account}"),"section":"wtf","name":account,"targetRelativePath":format!("WTF/Account/{account}"),"sizeBytes":leaves.iter().map(|r|r["sizeBytes"].as_u64().unwrap()).sum::<u64>(),"children":children,"meta":{"type":"account"}}));
        }
        items.insert("wtf", grouped);
        let sections = SECTIONS.iter().map(|key| { let rows=&items[key]; json!({"key":key,"title":key,"count":rows.len(),"sizeBytes":rows.iter().map(|r|r["sizeBytes"].as_u64().unwrap()).sum::<u64>(),"items":rows}) }).collect::<Vec<_>>();
        let mut totals = serde_json::Map::new();
        for section in &sections {
            if section["key"] != "wtf" {
                totals.insert(
                    section["key"].as_str().unwrap().into(),
                    section["count"].clone(),
                );
            }
        }
        totals.insert("wtfAccounts".into(), json!(accounts.len()));
        totals.insert(
            "files".into(),
            json!(
                sections
                    .iter()
                    .map(|s| s["count"].as_u64().unwrap())
                    .sum::<u64>()
            ),
        );
        totals.insert(
            "sizeBytes".into(),
            json!(
                sections
                    .iter()
                    .map(|s| s["sizeBytes"].as_u64().unwrap())
                    .sum::<u64>()
            ),
        );
        let id = lycheedev_windows::random::token()
            .map_err(|_| malformed("cannot create local backup identity"))?;
        let now = storage::utc_now();
        let manifest = json!({"schemaVersion":"config-backup.v2","backupId":id,"backupName":self.doc["backup_name"],"createdAt":now,"completedAt":now,"storageTargets":["local"],"source":{"origin":"modus","gameId":build,"localSelection":selection},"sections":sections,"totals":totals});
        storage::atomic_json(&output.join("manifest.v2.json"), &manifest)?;
        Ok(generated(&output, &manifest, false))
    }
    fn scan(&self, root: &Path, control: &OperationControl) -> Result<Value> {
        // ModUs accepts OTF and any explicit Interface material, and lists
        // role folders without another platform's marker-file rules.
        let names = |path: &Path| -> Result<Vec<String>> {
            local::entries(root, path, control)?
                .into_iter()
                .filter(|p| p.is_dir())
                .map(|p| local::filename(&p))
                .collect()
        };
        let addons = names(&root.join("Interface/AddOns"))?;
        let fonts = local::entries(root, &root.join("Fonts"), control)?
            .into_iter()
            .filter(|p| {
                p.is_file()
                    && p.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                        s.eq_ignore_ascii_case("ttf") || s.eq_ignore_ascii_case("otf")
                    })
            })
            .map(|p| local::filename(&p))
            .collect::<Result<Vec<_>>>()?;
        let materials = local::entries(root, &root.join("Interface"), control)?
            .into_iter()
            .filter(|p| {
                !p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("AddOns")
            })
            .map(|p| local::filename(&p))
            .collect::<Result<Vec<_>>>()?;
        let mut roles = Vec::new();
        for account in names(&root.join("WTF/Account"))? {
            if account.eq_ignore_ascii_case("SavedVariables") {
                continue;
            }
            for server in names(&root.join("WTF/Account").join(&account))? {
                if server.eq_ignore_ascii_case("SavedVariables") {
                    continue;
                }
                for role in names(&root.join("WTF/Account").join(&account).join(&server))? {
                    if !role.eq_ignore_ascii_case("SavedVariables") {
                        roles.push(json!({"account":account,"server":server,"role":role}));
                    }
                }
            }
        }
        Ok(
            json!({"platform":"modus","game_root":root,"server_type":self.doc.get("server_type"),"addon_names":addons,"font_names":fonts,"material_names":materials,"roles":roles,"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false}),
        )
    }
}
fn generated(output: &Path, manifest: &Value, reused: bool) -> Value {
    json!({"platform":"modus","local_backup_id":manifest["backupId"],"manifest_file":output.join("manifest.v2.json"),"output_dir":output,"server_type":manifest["source"]["gameId"],"totals":manifest["totals"],"generated":true,"reused":reused})
}
pub(super) fn component(name: &str) -> Result<&str> {
    local::name(name)?;
    if name.contains(',') {
        Err(invalid(
            "$.selection",
            "ModUs component names cannot contain commas",
        ))
    } else {
        Ok(name)
    }
}
pub(super) fn files(
    root: &Path,
    source: &Path,
    control: &OperationControl,
) -> Result<Vec<PathBuf>> {
    let mut todo = vec![source.to_owned()];
    let mut out = Vec::new();
    while let Some(path) = todo.pop() {
        control.check()?;
        storage::guard_path_with_check(root, &path, || control.check())?;
        if path.is_dir() {
            todo.extend(local::entries(root, &path, control)?.into_iter().rev());
        } else if path.is_file() {
            out.push(path);
        } else {
            return Err(invalid(
                "$.manifest_file",
                "selected backup source is missing or is not an ordinary file",
            ));
        }
    }
    Ok(out)
}
pub(super) fn wtf_file(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        ["lua", "wtf", "txt"]
            .iter()
            .any(|x| s.eq_ignore_ascii_case(x))
    }) && !path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .eq_ignore_ascii_case("BigFootSync.lua")
}

#[cfg(test)]
mod tests;
