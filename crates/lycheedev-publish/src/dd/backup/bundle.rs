//! DD's five native backup components. Selection is local; wire metadata is
//! derived from it, and the same original archives are validated before upload.
use super::*;
use crate::{contract, input, local_backup as local, provider::Provider};
use lycheedev_core::storage;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub(super) const GROUPS: [&str; 5] = ["known_addon", "unknown_addon", "material", "font", "wtf"];
const SCHEMA: &str = "lycheedev.publish.dd.backup-bundle.v1";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Account {
    account: String,
    include_common: bool,
    roles: Vec<local::Role>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Selection {
    addon_names: Vec<String>,
    unknown_addons: Vec<String>,
    materials: Vec<String>,
    fonts: Vec<String>,
    include_wtf_root: bool,
    wtf_accounts: Vec<Account>,
}
impl Selection {
    fn groups(&self) -> [&[String]; 4] {
        [
            &self.addon_names,
            &self.unknown_addons,
            &self.materials,
            &self.fonts,
        ]
    }
    fn manifest(&self, game: u64) -> Result<Value> {
        if game == 0 {
            return Err(invalid("$.game_type", "game_type must be positive"));
        }
        for names in self.groups() {
            local::distinct_names(names)?;
        }
        if self.addon_names.iter().any(|name| {
            self.unknown_addons
                .iter()
                .any(|other| name.eq_ignore_ascii_case(other))
        }) {
            return Err(invalid(
                "$.unknown_addons",
                "addon folders cannot be in both upload groups",
            ));
        }
        if self
            .materials
            .iter()
            .any(|name| name.eq_ignore_ascii_case("AddOns"))
        {
            return Err(invalid(
                "$.materials",
                "select addon folders through addon_names or unknown_addons",
            ));
        }
        if self
            .fonts
            .iter()
            .any(|name| !name.to_ascii_lowercase().ends_with(".ttf"))
        {
            return Err(invalid("$.fonts", "DD fonts must be selected .ttf files"));
        }
        let mut accounts = Vec::new();
        let mut names = BTreeSet::new();
        for account in &self.wtf_accounts {
            local::name(&account.account)?;
            if !names.insert(account.account.to_lowercase())
                || (!account.include_common && account.roles.is_empty())
            {
                return Err(invalid(
                    "$.wtf_accounts",
                    "select unique accounts with common content or explicit roles",
                ));
            }
            let mut servers = std::collections::BTreeMap::<String, Vec<String>>::new();
            let mut roles = BTreeSet::new();
            for role in &account.roles {
                local::name(&role.server)?;
                local::name(&role.name)?;
                if role.server.eq_ignore_ascii_case("SavedVariables")
                    || role.name.eq_ignore_ascii_case("SavedVariables")
                    || !roles.insert((role.server.to_lowercase(), role.name.to_lowercase()))
                    || servers
                        .keys()
                        .any(|s| s.eq_ignore_ascii_case(&role.server) && s != &role.server)
                {
                    return Err(invalid(
                        "$.wtf_accounts.roles",
                        "select unique exact servers and roles; SavedVariables is common content",
                    ));
                }
                servers
                    .entry(role.server.clone())
                    .or_default()
                    .push(role.name.clone());
            }
            accounts.push(json!({"name":account.account,"servers":servers.into_iter().map(|(name,items)|json!({"name":name,"items":items})).collect::<Vec<_>>(),"wa_list":[]}));
        }
        if self.groups().iter().all(|v| v.is_empty())
            && accounts.is_empty()
            && !self.include_wtf_root
        {
            return Err(invalid("$", "select at least one backup component"));
        }
        let mut manifest = json!({"game_type":game});
        for group in GROUPS {
            manifest[group] =
                json!({"md5":"","size":0,if group=="wtf"{"accounts"}else{"items"}:[]});
        }
        manifest["known_addon"]["items"] = json!(
            self.addon_names
                .iter()
                .map(|name| json!({"name":name,"modified":true,"dirs":[name]}))
                .collect::<Vec<_>>()
        );
        manifest["unknown_addon"]["items"] = json!(self.unknown_addons);
        manifest["material"]["items"] = json!(
            self.materials
                .iter()
                .map(|name| json!({"name":name,"desc":""}))
                .collect::<Vec<_>>()
        );
        manifest["font"]["items"] = json!(self.fonts);
        manifest["wtf"]["accounts"] = json!(accounts);
        Ok(manifest)
    }
    fn member(&self, group: &str, parts: &[String], directory: bool) -> bool {
        match group {
            "known_addon" => self.addon_names.contains(&parts[0]) && (directory || parts.len() > 1),
            "unknown_addon" => {
                self.unknown_addons.contains(&parts[0]) && (directory || parts.len() > 1)
            }
            "material" => self.materials.contains(&parts[0]),
            "font" => {
                parts[0] == "Fonts"
                    && (directory && parts.len() == 1
                        || !directory && parts.len() == 2 && self.fonts.contains(&parts[1]))
            }
            "wtf" => {
                if parts[0] != "WTF" {
                    return false;
                }
                if parts.len() == 1 {
                    return directory;
                }
                if parts.len() == 2 {
                    return if directory {
                        parts[1] == "Account" && !self.wtf_accounts.is_empty()
                    } else {
                        self.include_wtf_root && parts[1] != "Account"
                    };
                }
                if parts[1] != "Account" {
                    return false;
                }
                let Some(account) = self.wtf_accounts.iter().find(|a| a.account == parts[2]) else {
                    return false;
                };
                if parts.len() == 3 {
                    return directory;
                }
                if parts[3].eq_ignore_ascii_case("SavedVariables") {
                    return account.include_common && (directory || parts.len() > 4);
                }
                let roles = account
                    .roles
                    .iter()
                    .filter(|r| r.server == parts[3])
                    .collect::<Vec<_>>();
                if parts.len() == 4 {
                    return if directory {
                        !roles.is_empty()
                    } else {
                        account.include_common && roles.is_empty()
                    };
                }
                if roles.is_empty() {
                    return false;
                }
                if parts[4].eq_ignore_ascii_case("SavedVariables") {
                    return account.include_common && (directory || parts.len() > 5);
                }
                let role = roles.iter().any(|r| r.name == parts[4]);
                if parts.len() == 5 {
                    if directory {
                        role
                    } else {
                        account.include_common && !role
                    }
                } else {
                    role
                }
            }
            _ => false,
        }
    }
}

pub struct BackupGenerate {
    root: PathBuf,
    output: PathBuf,
    game: u64,
    selection: Selection,
}
impl BackupGenerate {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        let mut v = input::document(bytes)?;
        contract::input_shape(Provider::Dd, "backup", "generate")?.admit(&v)?;
        let root = std::path::absolute(cwd.join(v.remove("game_dir").unwrap().as_str().unwrap()))?;
        storage::guard_path(&root, &root)?;
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(invalid("$.game_dir", "game directory does not exist"));
        }
        let output =
            local::resolve_output(&cwd.join(v.remove("output_dir").unwrap().as_str().unwrap()))?;
        if output.starts_with(&root) {
            return Err(invalid(
                "$.output_dir",
                "backup output must be outside the game directory",
            ));
        }
        let game = v
            .remove("game_type")
            .unwrap()
            .as_u64()
            .ok_or_else(|| invalid("$.game_type", "game type must be positive"))?;
        v.remove("schema");
        let selection: Selection = serde_json::from_value(json!(v)).map_err(|_| {
            invalid(
                "$.selection",
                "selection must match the documented nested types",
            )
        })?;
        selection.manifest(game)?;
        Ok(Self {
            root,
            output,
            game,
            selection,
        })
    }
    pub fn schema() -> Value {
        let mut v = contract::input_shape(Provider::Dd, "backup", "generate")
            .unwrap()
            .schema();
        v.as_object_mut().unwrap().remove("x-validation-scope");
        for field in ["addon_names", "unknown_addons", "materials", "fonts"] {
            v["properties"][field]["items"] = json!({"type":"string","minLength":1});
            v["properties"][field]["uniqueItems"] = json!(true);
        }
        v["properties"]["wtf_accounts"]["items"] = json!({"type":"object","properties":{"account":{"type":"string","minLength":1},"include_common":{"type":"boolean"},"roles":{"type":"array","items":{"type":"object","properties":{"server":{"type":"string","minLength":1},"name":{"type":"string","minLength":1}},"required":["server","name"],"additionalProperties":false}}},"required":["account","include_common","roles"],"additionalProperties":false});
        v["anyOf"]=json!(["addon_names","unknown_addons","materials","fonts","wtf_accounts"].into_iter().map(|f|json!({"required":[f],"properties":{f:{"minItems":1}}})).chain(std::iter::once(json!({"required":["include_wtf_root"],"properties":{"include_wtf_root":{"const":true}}}))).collect::<Vec<_>>());
        v["description"] = json!(
            "Select only explicit local components. addon_names and unknown_addons cannot overlap; directories do not imply a platform addon ID. WTF common files and root files require their respective explicit flags. Output must be outside the game directory. Repeating generate with the same input reuses its validated original bundle."
        );
        v
    }
    pub fn plan(&self) -> Value {
        json!({"platform":"dd","action":"generate","game_type":self.game,"output_dir":self.output,"selection":self.selection,"remote_write":false,"credential_reads":false,"local_writes":false})
    }
    pub fn generate(&self, control: &OperationControl) -> Result<Value> {
        let manifest = self.selection.manifest(self.game)?;
        let bundle_path = self.output.join("backup-bundle.json");
        if bundle_path.exists() {
            let bundle = Validated::open(&bundle_path, control)?;
            let result = if bundle.spec.source_root == self.root
                && bundle.spec.manifest["game_type"] == self.game
                && serde_json::to_value(&bundle.spec.selection)?
                    == serde_json::to_value(&self.selection)?
            {
                Ok(
                    json!({"bundle_file":bundle_path,"game_type":self.game,"archives":bundle.spec.artifacts,"reused":true}),
                )
            } else {
                Err(invalid(
                    "$.output_dir",
                    "existing backup belongs to a different selection",
                ))
            };
            return bundle.finish(result, control);
        }
        if self.output.exists()
            && (!self.output.is_dir() || fs::read_dir(&self.output)?.next().transpose()?.is_some())
        {
            return Err(invalid(
                "$.output_dir",
                "backup output must be empty or contain the original validated bundle",
            ));
        }
        let bases = [
            self.root.join("Interface/AddOns"),
            self.root.join("Interface/AddOns"),
            self.root.join("Interface"),
            self.root.join("Fonts"),
        ];
        let mut groups = Vec::new();
        for ((group, names), base) in GROUPS[..4].iter().zip(self.selection.groups()).zip(bases) {
            let mut paths = Vec::new();
            for name in names {
                let p = local::path(&base, &[name], control)?;
                if !p.exists()
                    || (*group == "font" && !p.is_file())
                    || (matches!(*group, "known_addon" | "unknown_addon") && !p.is_dir())
                {
                    return Err(invalid(
                        "$.selection",
                        "selected backup component is absent or has the wrong kind",
                    ));
                }
                paths.push(p);
            }
            let base = if *group == "font" {
                self.root.clone()
            } else {
                base
            };
            groups.push((*group, paths, base));
        }
        let mut wtf = if self.selection.include_wtf_root {
            local::entries(&self.root, &self.root.join("WTF"), control)?
                .into_iter()
                .filter(|p| p.is_file())
                .collect()
        } else {
            Vec::new()
        };
        for a in &self.selection.wtf_accounts {
            let account = local::path(&self.root, &["WTF", "Account", &a.account], control)?;
            if !account.is_dir() {
                return Err(invalid("$.wtf_accounts", "selected account is absent"));
            }
            let before = wtf.len();
            if a.include_common {
                wtf.extend(local::common_sources(&self.root, &account, control)?);
            }
            let mut servers = BTreeSet::new();
            for role in &a.roles {
                let p = local::path(&account, &[&role.server, &role.name], control)?;
                if !p.is_dir() {
                    return Err(invalid("$.wtf_accounts.roles", "selected role is absent"));
                }
                wtf.push(p);
                if a.include_common && servers.insert(&role.server) {
                    wtf.extend(local::common_sources(
                        &self.root,
                        &account.join(&role.server),
                        control,
                    )?);
                }
            }
            if before == wtf.len() {
                return Err(invalid(
                    "$.wtf_accounts",
                    "selected account has no common files or roles",
                ));
            }
        }
        groups.push(("wtf", wtf, self.root.clone()));
        if groups.iter().all(|(_, p, _)| p.is_empty()) {
            return Err(invalid(
                "$.selection",
                "selection contains no backup content",
            ));
        }
        fs::create_dir_all(&self.output)?;
        storage::guard_path(&self.output, &self.output)?;
        let mut spec = Spec {
            schema: SCHEMA.into(),
            source_root: self.root.clone(),
            manifest,
            selection: self.selection.clone(),
            artifacts: Vec::new(),
        };
        for (group, sources, base) in groups {
            if sources.is_empty() {
                continue;
            }
            let file = self.output.join(format!("{group}.zip"));
            let artifact =
                local::archive(&self.root, &sources, Some(&base), &file, false, control)?;
            let (mut locked, _) = storage::locked_sha256(&file, u64::MAX, control)?;
            let digest = md5(&mut locked, control);
            let md5 = storage::finish_file(locked, digest, control)?;
            spec.manifest[group]["md5"] = json!(md5);
            spec.manifest[group]["size"] = json!(artifact.size);
            spec.artifacts.push(Artifact {
                group: group.into(),
                file: artifact.file,
                size: artifact.size,
                md5,
                sha256: artifact.sha256,
                files: artifact.files,
            });
        }
        storage::atomic_json(&bundle_path, &spec)?;
        let validated = Validated::open(&bundle_path, control)?;
        validated.finish(Ok(json!({"bundle_file":bundle_path,"game_type":self.game,"archives":spec.artifacts,"reused":false})),control)
    }
}

pub fn scan(root: &Path, control: &OperationControl) -> Result<Value> {
    let inventory = local::scan(root, control)?;
    let mut accounts = Vec::new();
    for account in local::entries(root, &root.join("WTF/Account"), control)?
        .into_iter()
        .filter(|p| p.is_dir())
    {
        let mut roles = Vec::new();
        for server in local::entries(root, &account, control)?
            .into_iter()
            .filter(|p| {
                p.is_dir()
                    && p.file_name().unwrap().to_string_lossy().to_lowercase() != "savedvariables"
            })
        {
            for role in local::entries(root, &server, control)?
                .into_iter()
                .filter(|p| {
                    p.is_dir()
                        && p.file_name().unwrap().to_string_lossy().to_lowercase()
                            != "savedvariables"
                })
            {
                roles.push(
                    json!({"server":local::filename(&server)?,"name":local::filename(&role)?}),
                );
            }
        }
        accounts.push(json!({"account":local::filename(&account)?,"roles":roles,"has_common":!local::common_sources(root,&account,control)?.is_empty()}));
    }
    let addons = inventory
        .unknown_plugins
        .iter()
        .map(|name| json!({"name":name}))
        .collect::<Vec<_>>();
    let materials = local::entries(root, &root.join("Interface"), control)?
        .into_iter()
        .filter(|p| {
            p.file_name().unwrap().to_string_lossy().to_lowercase() != "addons"
                && (p.is_dir()
                    || p.extension().is_some_and(|s| {
                        matches!(
                            s.to_string_lossy().to_lowercase().as_str(),
                            "blp" | "tga" | "dds" | "png" | "jpg" | "jpeg" | "bmp" | "tex"
                        )
                    }))
        })
        .map(|p| Ok(json!({"name":local::filename(&p)?})))
        .collect::<Result<Vec<_>>>()?;
    let root_files = local::entries(root, &root.join("WTF"), control)?
        .into_iter()
        .filter(|p| p.is_file())
        .map(|p| local::filename(&p))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"game_dir":root,"addon_names":addons,"unknown_addons":addons,"materials":materials,"fonts":inventory.fonts,"wtf_accounts":accounts,"wtf_root_files":root_files,"blocked":inventory.blocked,"platform_ids_inferred":false}),
    )
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Artifact {
    pub group: String,
    pub file: String,
    pub size: u64,
    pub md5: String,
    pub sha256: String,
    pub files: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Spec {
    schema: String,
    source_root: PathBuf,
    pub manifest: Value,
    selection: Selection,
    pub artifacts: Vec<Artifact>,
}
pub(super) struct Validated {
    source: File,
    pub sha256: String,
    pub spec: Spec,
    pub files: Vec<File>,
}
use crate::local_backup::md5;
impl Validated {
    pub fn open(path: &Path, control: &OperationControl) -> Result<Self> {
        let root = path
            .parent()
            .ok_or_else(|| invalid("$.bundle_file", "bundle has no parent"))?;
        storage::guard_path_with_check(root, path, || control.check())?;
        let (mut source, sha256) = storage::locked_sha256(path, 32 << 20, control)?;
        let mut files = Vec::new();
        let result = (|| {
            source.seek(SeekFrom::Start(0))?;
            let mut bytes = Vec::new();
            source.read_to_end(&mut bytes)?;
            let spec: Spec = serde_json::from_value(input::value(&bytes)?)
                .map_err(|_| invalid("$.bundle_file", "invalid DD backup bundle fields"))?;
            if spec.schema != SCHEMA {
                return Err(invalid(
                    "$.bundle_file",
                    "expected the native DD backup bundle",
                ));
            }
            let mut expected = spec
                .selection
                .manifest(spec.manifest["game_type"].as_u64().unwrap_or(0))?;
            let mut groups = BTreeSet::new();
            for artifact in &spec.artifacts {
                if !GROUPS.contains(&artifact.group.as_str())
                    || !groups.insert(artifact.group.clone())
                    || artifact.file != format!("{}.zip", artifact.group)
                {
                    return Err(invalid(
                        "$.bundle_file.artifacts",
                        "invalid or repeated backup component",
                    ));
                }
                let file_path = local::path(root, &[&artifact.file], control)?;
                let (mut file, digest) = storage::locked_sha256(&file_path, u64::MAX, control)?;
                let check = (|| {
                    if digest != artifact.sha256
                        || file.metadata()?.len() != artifact.size
                        || md5(&mut file, control)? != artifact.md5
                    {
                        return Err(invalid(
                            "$.bundle_file.artifacts",
                            "backup archive changed after generation",
                        ));
                    }
                    validate_zip(&mut file, artifact, &spec.selection, control)
                })();
                if let Err(e) = check {
                    return storage::finish_file(file, Err(e), control);
                }
                files.push(file);
                expected[&artifact.group]["md5"] = json!(artifact.md5);
                expected[&artifact.group]["size"] = json!(artifact.size);
            }
            if files.is_empty() || expected != spec.manifest {
                return Err(invalid(
                    "$.bundle_file.manifest",
                    "archive metadata differs from the original selection",
                ));
            }
            for group in GROUPS {
                let field = if group == "wtf" { "accounts" } else { "items" };
                if (!expected[group][field].as_array().unwrap().is_empty()
                    || group == "wtf" && spec.selection.include_wtf_root)
                    && !groups.contains(group)
                {
                    return Err(invalid(
                        "$.bundle_file.artifacts",
                        "selected component has no archive",
                    ));
                }
            }
            Ok(spec)
        })();
        match result {
            Ok(spec) => Ok(Self {
                source,
                sha256,
                spec,
                files,
            }),
            Err(e) => {
                let mut r = storage::finish_file(source, Err(e), control);
                for file in files {
                    r = storage::finish_file(file, r, control);
                }
                r
            }
        }
    }
    pub fn finish<T>(self, result: Result<T>, control: &OperationControl) -> Result<T> {
        let mut result = storage::finish_file(self.source, result, control);
        for file in self.files {
            result = storage::finish_file(file, result, control);
        }
        result
    }
}
fn validate_zip(
    file: &mut File,
    artifact: &Artifact,
    selection: &Selection,
    control: &OperationControl,
) -> Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut raw = file.try_clone()?;
    let result = (|| {
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|_| invalid("$.bundle_file.artifacts", "backup component is not a ZIP"))?;
        let mut paths = crate::zip_contract::Paths::default();
        let mut count = 0;
        let mut roots = BTreeSet::new();
        let mut roles = BTreeSet::new();
        for i in 0..zip.len() {
            control.check()?;
            let member = zip
                .by_index_raw(i)
                .map_err(|_| invalid("$.bundle_file.artifacts", "invalid ZIP member"))?;
            let directory = member.is_dir();
            let parts = paths.admit(
                &mut raw,
                member.name_raw(),
                member.header_start(),
                member.central_header_start(),
                directory,
                member.encrypted(),
            )?;
            if !selection.member(&artifact.group, &parts, directory) {
                return Err(invalid(
                    "$.bundle_file.artifacts",
                    "ZIP contains an unselected component or role",
                ));
            }
            roots.insert(if artifact.group == "font" && parts.len() == 2 {
                parts[1].clone()
            } else {
                parts[0].clone()
            });
            if artifact.group == "wtf" && !directory && parts.len() >= 6 {
                roles.insert((parts[2].clone(), parts[3].clone(), parts[4].clone()));
            }
            drop(member);
            let mut member = zip
                .by_index(i)
                .map_err(|_| invalid("$.bundle_file.artifacts", "invalid compressed member"))?;
            let mut bytes = [0u8; 65536];
            let mut size = 0;
            loop {
                control.check()?;
                let n = member.read(&mut bytes).map_err(|_| {
                    invalid("$.bundle_file.artifacts", "ZIP checksum or data is invalid")
                })?;
                if n == 0 {
                    break;
                }
                size += n as u64;
            }
            if size != member.size() {
                return Err(invalid(
                    "$.bundle_file.artifacts",
                    "ZIP member size differs",
                ));
            }
            count += u64::from(!directory);
        }
        if count != artifact.files {
            return Err(invalid("$.bundle_file.artifacts", "ZIP file count differs"));
        }
        if let Some(index) = GROUPS[..4].iter().position(|g| *g == artifact.group) {
            if selection.groups()[index].iter().any(|s| !roots.contains(s)) {
                return Err(invalid(
                    "$.bundle_file.artifacts",
                    "ZIP omitted a selected component",
                ));
            }
        } else if selection.wtf_accounts.iter().any(|a| {
            a.roles
                .iter()
                .any(|r| !roles.contains(&(a.account.clone(), r.server.clone(), r.name.clone())))
        }) {
            return Err(invalid(
                "$.bundle_file.artifacts",
                "ZIP omitted a selected WTF role",
            ));
        }
        Ok(())
    })();
    storage::finish_file(raw, result, control)
}

#[cfg(test)]
mod tests;
