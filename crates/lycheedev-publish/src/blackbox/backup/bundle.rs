use super::*;
use crate::{input, local_backup as local};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};
const SCHEMA: &str = "lycheedev.publish.blackbox.backup-bundle.v1";
const GROUPS: &[(&str, &str, &str)] = &[
    ("UnknownAddons", "unknownModules", "unknown.zip"),
    ("Materials", "materials", "materials.zip"),
    ("Fonts", "fonts", "fonts.zip"),
];
const SKIP: &[&str] = &["heyboxplayerinfo.lua", "heyboxplayerinfo.lua.bak"];
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Character {
    realm: String,
    player: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Account {
    name: String,
    characters: Vec<Character>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Selection {
    #[serde(default)]
    known_addons: Vec<Value>,
    unknown_addons: Vec<String>,
    materials: Vec<String>,
    fonts: Vec<String>,
    wtf_accounts: Vec<Account>,
    #[serde(default)]
    strategies: Vec<Value>,
}
impl Selection {
    fn validate(&self) -> Result<()> {
        for values in [&self.unknown_addons, &self.materials, &self.fonts] {
            local::distinct_names(values)?;
        }
        if self
            .materials
            .iter()
            .any(|s| s.eq_ignore_ascii_case("AddOns"))
        {
            return Err(invalid(
                "$.materials",
                "Interface/AddOns belongs in unknown_addons",
            ));
        }
        if self
            .unknown_addons
            .iter()
            .any(|s| s.eq_ignore_ascii_case("heyboxplayerinfo"))
        {
            return Err(invalid(
                "$.unknown_addons",
                "Heybox player telemetry is excluded from author backups",
            ));
        }
        if self
            .fonts
            .iter()
            .any(|s| !s.to_ascii_lowercase().ends_with(".ttf"))
        {
            return Err(invalid("$.fonts", "select .ttf filenames"));
        }
        let mut known = BTreeSet::new();
        for row in &self.known_addons {
            if !row.is_object()
                || identifier(&row["ID"])
                    .is_none_or(|s| s.is_empty() || s == "0" || !known.insert(s))
                || identifier(&row["VersionID"]).is_none_or(|s| s.is_empty() || s == "0")
                || row["Name"].as_str().is_none_or(|s| s.is_empty())
                || !row["Strategy"]
                    .as_u64()
                    .is_some_and(|n| (1..=5).contains(&n))
            {
                return Err(invalid(
                    "$.known_addons",
                    "use distinct official briefs with ID, Name, Strategy (1..5), VersionID; known addons are references, not local archives",
                ));
            }
        }
        if self.strategies.iter().any(|s| !s.is_object()) {
            return Err(invalid(
                "$.strategies",
                "expected original official strategy objects",
            ));
        }
        let mut accounts = BTreeSet::new();
        for a in &self.wtf_accounts {
            local::name(&a.name)?;
            if a.name.eq_ignore_ascii_case("SavedVariables")
                || !accounts.insert(a.name.to_lowercase())
                || a.characters.is_empty()
            {
                return Err(invalid(
                    "$.wtf_accounts",
                    "select unique accounts and explicit characters",
                ));
            }
            let mut seen = BTreeSet::new();
            for c in &a.characters {
                local::name(&c.realm)?;
                local::name(&c.player)?;
                if c.realm.eq_ignore_ascii_case("SavedVariables")
                    || c.player.eq_ignore_ascii_case("SavedVariables")
                    || !seen.insert((c.realm.to_lowercase(), c.player.to_lowercase()))
                {
                    return Err(invalid(
                        "$.wtf_accounts.characters",
                        "select distinct realm/player pairs",
                    ));
                }
            }
        }
        if known.is_empty()
            && self.unknown_addons.is_empty()
            && self.materials.is_empty()
            && self.fonts.is_empty()
            && accounts.is_empty()
        {
            return Err(invalid(
                "$.selection",
                "select at least one addon, account, material or font",
            ));
        }
        Ok(())
    }
    fn configuration(&self, branch: &str, time: i64) -> Value {
        let mut value = json!({"HeyboxID":0,"Name":"","SaveTime":time,"IsClassic":branch!="wowretail","Strappid":branch,"KnownAddons":self.known_addons,"WTFList":[]});
        if !self.strategies.is_empty() {
            value["Strategies"] = json!(self.strategies);
        }
        value
    }
}
pub struct BackupGenerate {
    root: PathBuf,
    output: PathBuf,
    branch: String,
    selection: Selection,
}
impl BackupGenerate {
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        let mut doc = document(bytes)?;
        input_shape(Provider::Blackbox, "backup", "generate")?.admit(&doc)?;
        let root =
            std::path::absolute(cwd.join(doc.remove("game_dir").unwrap().as_str().unwrap()))?;
        storage::guard_path(&root, &root)?;
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(invalid(
                "$.game_dir",
                "expected an existing game branch directory",
            ));
        }
        let output =
            local::resolve_output(&cwd.join(doc.remove("output_dir").unwrap().as_str().unwrap()))?;
        if output.starts_with(&root) {
            return Err(invalid("$.output_dir", "output must be outside game_dir"));
        }
        let branch = forms::branch(&doc.remove("strappid").unwrap(), "$.strappid")?;
        doc.remove("schema");
        let selection: Selection = serde_json::from_value(Value::Object(doc)).map_err(|_| {
            invalid(
                "$.selection",
                "selection fields do not match the declared nested schema",
            )
        })?;
        selection.validate()?;
        Ok(Self {
            root,
            output,
            branch,
            selection,
        })
    }
    pub fn schema() -> Value {
        let mut s = input_shape(Provider::Blackbox, "backup", "generate")
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        for field in ["unknown_addons", "materials", "fonts"] {
            s["properties"][field]["items"] = json!({"type":"string","minLength":1});
            s["properties"][field]["uniqueItems"] = json!(true);
        }
        s["properties"]["strappid"]["pattern"] = json!("^wow[a-z0-9_]+$");
        s["properties"]["known_addons"]["items"] = json!({"type":"object","required":["ID","Name","Strategy","VersionID"],"properties":{"ID":{"type":["string","integer"]},"Name":{"type":"string","minLength":1},"Strategy":{"type":"integer","minimum":1,"maximum":5},"VersionID":{"type":["string","integer"]}},"additionalProperties":true});
        s["properties"]["strategies"]["items"] = json!({"type":"object","description":"Original official strategy object; preserved without guessing fields."});
        s["properties"]["wtf_accounts"]["items"] = json!({"type":"object","required":["name","characters"],"additionalProperties":false,"properties":{"name":{"type":"string","minLength":1},"characters":{"type":"array","minItems":1,"uniqueItems":true,"items":{"type":"object","required":["realm","player"],"additionalProperties":false,"properties":{"realm":{"type":"string","minLength":1},"player":{"type":"string","minLength":1}}}}}});
        s["anyOf"] = json!(
            [
                "known_addons",
                "unknown_addons",
                "materials",
                "fonts",
                "wtf_accounts"
            ]
            .map(|f| json!({"required":[f],"properties":{f:{"minItems":1}}}))
        );
        s["description"] = json!(
            "Explicit arrays select unknown addon directories, Interface material directories, TTF files and account characters. Known addon briefs are remote references. Selected accounts include their common files/SavedVariables plus selected characters only. Heybox telemetry files are excluded. Output is outside game_dir; a matching existing bundle is reused without rebuilding or rereading changed game content."
        );
        s
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let jobs = self.jobs(control)?;
        Ok(
            json!({"platform":"blackbox","planned":true,"remote_write":false,"credential_reads":false,"local_write":false,"output_dir":self.output,"strappid":self.branch,"archive_count":jobs.len(),"source_bytes":jobs.iter().map(|j|j.files.iter().map(|r|r["Size"].as_u64().unwrap()).sum::<u64>()).sum::<u64>(),"known_addon_count":self.selection.known_addons.len()}),
        )
    }
    pub fn generate(&self, control: &OperationControl) -> Result<Value> {
        let path = self.output.join("backup-bundle.json");
        if path.exists() {
            let bundle = Validated::open(&path, control)?;
            let same = bundle.spec.source_root == self.root
                && bundle.spec.selection == self.selection
                && bundle.spec.configuration["Strappid"] == self.branch;
            let result = if same {
                Ok(
                    json!({"bundle_file":path,"archive_count":bundle.spec.archives.len(),"source_bytes":bundle.spec.source_bytes,"reused":true}),
                )
            } else {
                Err(invalid(
                    "$.output_dir",
                    "existing bundle belongs to another source or selection",
                ))
            };
            return bundle.finish(result, control);
        }
        if self.output.exists()
            && (!self.output.is_dir() || fs::read_dir(&self.output)?.next().transpose()?.is_some())
        {
            return Err(invalid(
                "$.output_dir",
                "output must be empty or contain the original matching bundle",
            ));
        }
        let jobs = self.jobs(control)?;
        fs::create_dir_all(&self.output)?;
        storage::guard_path(&self.output, &self.output)?;
        let mut spec = Spec {
            schema: SCHEMA.into(),
            source_root: self.root.clone(),
            selection: self.selection.clone(),
            configuration: self.selection.configuration(&self.branch, now()),
            archives: Vec::new(),
            source_bytes: 0,
        };
        for job in jobs {
            let path = self.output.join(&job.filename);
            let archive = local::archive(
                &self.root,
                &job.sources,
                Some(&job.base),
                &path,
                false,
                control,
            )?;
            let (mut file, sha256) = storage::locked_sha256(&path, 5_000_000_000, control)?;
            let md5 = cos::md5(&mut file, control);
            let md5 = storage::finish_file(file, md5, control)?;
            let mut descriptor =
                json!({"Files":job.files,"Size":archive.size,"MD5":md5,"Host":"","Path":""});
            if let Some(a) = &job.account {
                descriptor["Name"] = json!(a.name);
                descriptor["RealmCharacters"] = json!(
                    a.characters
                        .iter()
                        .map(|c| json!({"Realm":c.realm,"Player":c.player}))
                        .collect::<Vec<_>>()
                );
                descriptor["Characters"] = json!(
                    a.characters
                        .iter()
                        .map(|c| format!("{} - {}", c.realm, c.player))
                        .collect::<Vec<_>>()
                );
                spec.configuration["WTFList"]
                    .as_array_mut()
                    .unwrap()
                    .push(descriptor);
            } else {
                spec.configuration[&job.field] = descriptor;
            }
            spec.source_bytes += job
                .files
                .iter()
                .map(|r| r["Size"].as_u64().unwrap())
                .sum::<u64>();
            spec.archives.push(Artifact {
                field: job.field,
                account_name: job.account.map(|a| a.name),
                file_type: job.file_type,
                file: job.filename,
                bytes: archive.size,
                md5,
                sha256,
            });
        }
        storage::atomic_json(&path, &spec)?;
        let bundle = Validated::open(&path, control)?;
        let result = json!({"bundle_file":path,"archive_count":bundle.spec.archives.len(),"source_bytes":bundle.spec.source_bytes,"reused":false});
        bundle.finish(Ok(result), control)
    }
    fn jobs(&self, control: &OperationControl) -> Result<Vec<Job>> {
        let mut jobs = Vec::new();
        for ((field, file_type, filename), (names, base)) in GROUPS.iter().zip([
            (
                &self.selection.unknown_addons,
                self.root.join("Interface/AddOns"),
            ),
            (&self.selection.materials, self.root.join("Interface")),
            (&self.selection.fonts, self.root.join("Fonts")),
        ]) {
            if names.is_empty() {
                continue;
            }
            let mut sources = Vec::new();
            let mut files = Vec::new();
            for name in names {
                let source = local::path(
                    &self.root,
                    &base
                        .strip_prefix(&self.root)
                        .unwrap()
                        .components()
                        .map(|s| s.as_os_str().to_str().unwrap())
                        .chain(std::iter::once(name.as_str()))
                        .collect::<Vec<_>>(),
                    control,
                )?;
                if (*field == "Fonts" && !source.is_file())
                    || (*field != "Fonts" && !source.is_dir())
                {
                    return Err(invalid(
                        "$.selection",
                        "selected source is missing or has the wrong kind",
                    ));
                }
                let selected = source_files(&self.root, &source, control)?;
                let size = size(&selected)?;
                sources.extend(selected);
                files.push(json!({"Name":name,"Size":size}));
            }
            if sources.is_empty() {
                return Err(invalid(
                    "$.selection",
                    "a selected backup category contains no files",
                ));
            }
            jobs.push(Job {
                field: (*field).into(),
                file_type: (*file_type).into(),
                filename: (*filename).into(),
                base,
                sources,
                files,
                account: None,
            });
        }
        for account in &self.selection.wtf_accounts {
            let source = local::path(&self.root, &["WTF", "Account", &account.name], control)?;
            let available = characters(&self.root, &source, control)?;
            if account.characters.iter().any(|c| !available.contains(c)) {
                return Err(invalid(
                    "$.wtf_accounts.characters",
                    "selected character is absent from backup scan",
                ));
            }
            let mut sources = Vec::new();
            for common in local::common_sources(&self.root, &source, control)? {
                sources.extend(source_files(&self.root, &common, control)?);
            }
            for c in &account.characters {
                sources.extend(source_files(
                    &self.root,
                    &local::path(&source, &[&c.realm, &c.player], control)?,
                    control,
                )?);
            }
            if sources.is_empty() {
                return Err(invalid("$.wtf_accounts", "selected account has no files"));
            }
            let files = vec![json!({"Name":account.name,"Size":size(&sources)?})];
            jobs.push(Job {
                field: "WTFList".into(),
                file_type: "wtf".into(),
                filename: format!("account_{}.zip", account.name),
                base: self.root.join("WTF/Account"),
                sources,
                files,
                account: Some(account.clone()),
            });
        }
        Ok(jobs)
    }
}
struct Job {
    field: String,
    file_type: String,
    filename: String,
    base: PathBuf,
    sources: Vec<PathBuf>,
    files: Vec<Value>,
    account: Option<Account>,
}
fn size(paths: &[PathBuf]) -> Result<u64> {
    paths.iter().try_fold(0u64, |n, p| {
        n.checked_add(p.metadata()?.len())
            .ok_or_else(|| invalid("$.selection", "backup size exceeds supported range"))
    })
}
fn source_files(root: &Path, source: &Path, control: &OperationControl) -> Result<Vec<PathBuf>> {
    let mut pending = vec![source.to_owned()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        storage::guard_path_with_check(root, &path, || control.check())?;
        if path.is_dir() {
            pending.extend(local::entries(root, &path, control)?.into_iter().rev());
        } else if path.is_file() {
            if !SKIP.contains(&local::filename(&path)?.to_ascii_lowercase().as_str()) {
                files.push(path);
            }
        } else {
            return Err(invalid(
                "$.selection",
                "backup source is not an ordinary file/directory",
            ));
        }
    }
    Ok(files)
}
fn characters(root: &Path, account: &Path, control: &OperationControl) -> Result<Vec<Character>> {
    if !account.is_dir() {
        return Err(invalid(
            "$.wtf_accounts",
            "selected account directory is absent",
        ));
    }
    let mut rows = Vec::new();
    for realm in local::entries(root, account, control)?
        .into_iter()
        .filter(|p| {
            p.is_dir()
                && !p
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("SavedVariables")
        })
    {
        for player in local::entries(root, &realm, control)?
            .into_iter()
            .filter(|p| p.is_dir())
        {
            if local::entries(root, &player, control)?.iter().any(|p| {
                p.is_file()
                    && [
                        "addons.txt",
                        "cache.md5",
                        "chat-cache.txt",
                        "config-cache.wtf",
                        "layout-local.txt",
                    ]
                    .contains(
                        &p.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .to_ascii_lowercase()
                            .as_str(),
                    )
            }) {
                rows.push(Character {
                    realm: local::filename(&realm)?,
                    player: local::filename(&player)?,
                });
            }
        }
    }
    Ok(rows)
}
pub fn scan(root: &Path, branch: &str, control: &OperationControl) -> Result<Value> {
    forms::branch(&json!(branch), "$.strappid")?;
    storage::guard_path(root, root)?;
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(invalid("$.game_dir", "expected a game branch directory"));
    }
    let mut output = json!({"platform":"blackbox","game_dir":root,"strappid":branch,"addon_classification":"folder_candidates_without_remote_module_identification"});
    for (key, base) in [
        ("unknown_addons", "Interface/AddOns"),
        ("materials", "Interface"),
        ("fonts", "Fonts"),
    ] {
        let mut names = Vec::new();
        for path in local::entries(&root, &root.join(base), control)? {
            let name = local::filename(&path)?;
            if key == "fonts" {
                if path.is_file() && name.to_ascii_lowercase().ends_with(".ttf") {
                    names.push(name);
                }
            } else if path.is_dir()
                && !(key == "materials" && name.eq_ignore_ascii_case("AddOns"))
                && !(key == "unknown_addons" && name.eq_ignore_ascii_case("heyboxplayerinfo"))
            {
                names.push(name);
            }
        }
        output[key] = json!(names);
    }
    let mut accounts = Vec::new();
    for account in local::entries(&root, &root.join("WTF/Account"), control)?
        .into_iter()
        .filter(|p| {
            p.is_dir()
                && !p
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("SavedVariables")
        })
    {
        let selected = characters(&root, &account, control)?;
        if !selected.is_empty() {
            accounts.push(Account {
                name: local::filename(&account)?,
                characters: selected,
            });
        }
    }
    output["wtf_accounts"] = json!(accounts);
    Ok(output)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Artifact {
    pub field: String,
    pub account_name: Option<String>,
    pub file_type: String,
    pub file: String,
    pub bytes: u64,
    pub md5: String,
    pub sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Spec {
    schema: String,
    source_root: PathBuf,
    selection: Selection,
    pub configuration: Value,
    pub archives: Vec<Artifact>,
    pub source_bytes: u64,
}
pub(super) struct Validated {
    source: File,
    pub sha256: String,
    pub spec: Spec,
    pub files: Vec<File>,
}
impl Validated {
    pub fn open(path: &Path, control: &OperationControl) -> Result<Self> {
        let root = path
            .parent()
            .ok_or_else(|| invalid("$.bundle_file", "bundle has no parent"))?;
        storage::guard_path(root, path)?;
        let (mut source, sha256) = storage::locked_sha256(path, 32 << 20, control)?;
        let mut files = Vec::new();
        let result = (|| {
            source.seek(SeekFrom::Start(0))?;
            let mut bytes = Vec::new();
            source.read_to_end(&mut bytes)?;
            let spec: Spec = serde_json::from_value(input::value(&bytes)?)
                .map_err(|_| invalid("$.bundle_file", "invalid Heybox bundle fields"))?;
            if spec.schema != SCHEMA {
                return Err(invalid(
                    "$.bundle_file",
                    "expected the original native Heybox backup bundle",
                ));
            }
            spec.selection.validate()?;
            let branch = forms::branch(
                &spec.configuration["Strappid"],
                "$.bundle_file.configuration.Strappid",
            )?;
            let time = spec.configuration["SaveTime"]
                .as_i64()
                .filter(|n| *n > 0)
                .ok_or_else(|| invalid("$.bundle_file", "invalid original save time"))?;
            let mut expected = spec.selection.configuration(&branch, time);
            let mut seen = BTreeSet::new();
            let mut total = 0;
            for artifact in &spec.archives {
                let descriptor = descriptor(&spec.configuration, artifact)?;
                if !seen.insert((artifact.field.clone(), artifact.account_name.clone()))
                    || descriptor["Size"] != artifact.bytes
                    || descriptor["MD5"] != artifact.md5
                    || descriptor["Host"] != ""
                    || descriptor["Path"] != ""
                {
                    return Err(invalid(
                        "$.bundle_file",
                        "archive descriptor changed or repeats a component",
                    ));
                }
                let expected_name = if artifact.field == "WTFList" {
                    format!(
                        "account_{}.zip",
                        artifact.account_name.as_deref().ok_or_else(|| invalid(
                            "$.bundle_file",
                            "WTF archive omitted its account"
                        ))?
                    )
                } else {
                    GROUPS
                        .iter()
                        .find(|(f, _, _)| *f == artifact.field)
                        .ok_or_else(|| invalid("$.bundle_file", "unknown backup archive"))?
                        .2
                        .to_owned()
                };
                if artifact.file != expected_name {
                    return Err(invalid(
                        "$.bundle_file",
                        "archive filename does not match its component",
                    ));
                }
                let (mut file, hash) = storage::locked_sha256(
                    &local::path(root, &[&artifact.file], control)?,
                    5_000_000_000,
                    control,
                )?;
                let check = (|| {
                    if hash != artifact.sha256
                        || file.metadata()?.len() != artifact.bytes
                        || cos::md5(&mut file, control)? != artifact.md5
                    {
                        return Err(invalid(
                            "$.bundle_file",
                            "archive bytes differ from the original bundle",
                        ));
                    }
                    validate_zip(&mut file, artifact, descriptor, &spec.selection, control)
                })();
                let size = match check {
                    Ok(n) => n,
                    Err(e) => return storage::finish_file(file, Err(e), control),
                };
                files.push(file);
                total += size;
                if artifact.field == "WTFList" {
                    expected["WTFList"]
                        .as_array_mut()
                        .unwrap()
                        .push(descriptor.clone());
                } else {
                    expected[&artifact.field] = descriptor.clone();
                }
            }
            let mut required = BTreeSet::new();
            for (field, names) in [
                ("UnknownAddons", &spec.selection.unknown_addons),
                ("Materials", &spec.selection.materials),
                ("Fonts", &spec.selection.fonts),
            ] {
                if !names.is_empty() {
                    required.insert((field.to_owned(), None));
                }
            }
            required.extend(
                spec.selection
                    .wtf_accounts
                    .iter()
                    .map(|a| ("WTFList".into(), Some(a.name.clone()))),
            );
            if required != seen || expected != spec.configuration || total != spec.source_bytes {
                return Err(invalid(
                    "$.bundle_file",
                    "configuration, archives or source size differ from the explicit selection",
                ));
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
            Err(error) => {
                let mut r = storage::finish_file(source, Err(error), control);
                for file in files {
                    r = storage::finish_file(file, r, control);
                }
                r
            }
        }
    }
    pub fn finish<T>(self, result: Result<T>, control: &OperationControl) -> Result<T> {
        let mut r = storage::finish_file(self.source, result, control);
        for file in self.files {
            r = storage::finish_file(file, r, control);
        }
        r
    }
}
pub(super) fn descriptor<'a>(configuration: &'a Value, artifact: &Artifact) -> Result<&'a Value> {
    let kind = if artifact.field == "WTFList" {
        "wtf"
    } else {
        GROUPS
            .iter()
            .find(|(f, _, _)| *f == artifact.field)
            .map(|(_, t, _)| *t)
            .ok_or_else(|| invalid("$.bundle_file", "unknown archive field"))?
    };
    if artifact.file_type != kind {
        return Err(invalid(
            "$.bundle_file",
            "archive file_type differs from its native group",
        ));
    }
    if artifact.field == "WTFList" {
        configuration["WTFList"]
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|r| r["Name"].as_str() == artifact.account_name.as_deref())
            })
            .ok_or_else(|| invalid("$.bundle_file", "missing account descriptor"))
    } else {
        configuration
            .get(&artifact.field)
            .filter(|v| v.is_object())
            .ok_or_else(|| invalid("$.bundle_file", "missing archive descriptor"))
    }
}
pub(super) fn set_location(
    configuration: &mut Value,
    artifact: &Artifact,
    host: &str,
    path: &str,
) -> Result<()> {
    let d = if artifact.field == "WTFList" {
        configuration["WTFList"]
            .as_array_mut()
            .and_then(|a| {
                a.iter_mut()
                    .find(|r| r["Name"].as_str() == artifact.account_name.as_deref())
            })
            .ok_or_else(|| invalid("$.bundle_file", "missing account descriptor"))?
    } else {
        &mut configuration[&artifact.field]
    };
    d["Host"] = json!(host);
    d["Path"] = json!(path);
    Ok(())
}
fn validate_zip(
    file: &mut File,
    artifact: &Artifact,
    descriptor: &Value,
    selection: &Selection,
    control: &OperationControl,
) -> Result<u64> {
    let mut raw = file.try_clone()?;
    let result = (|| {
        file.seek(SeekFrom::Start(0))?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|_| invalid("$.bundle_file", "archive is not a valid ZIP"))?;
        let mut paths = crate::zip_contract::Paths::default();
        let mut sizes = BTreeMap::<String, u64>::new();
        let mut roles = BTreeSet::new();
        let account = if artifact.field == "WTFList" {
            Some(
                selection
                    .wtf_accounts
                    .iter()
                    .find(|a| Some(a.name.as_str()) == artifact.account_name.as_deref())
                    .ok_or_else(|| {
                        invalid("$.bundle_file", "archive names an unselected account")
                    })?,
            )
        } else {
            None
        };
        let selected: &[String] = match artifact.field.as_str() {
            "UnknownAddons" => &selection.unknown_addons,
            "Materials" => &selection.materials,
            "Fonts" => &selection.fonts,
            "WTFList" => &[],
            _ => unreachable!(),
        };
        for index in 0..zip.len() {
            control.check()?;
            let member = zip
                .by_index_raw(index)
                .map_err(|_| invalid("$.bundle_file", "invalid ZIP member"))?;
            let directory = member.is_dir();
            let parts = paths.admit(
                &mut raw,
                member.name_raw(),
                member.header_start(),
                member.central_header_start(),
                directory,
                member.encrypted(),
            )?;
            if SKIP.contains(&parts.last().unwrap().to_ascii_lowercase().as_str()) {
                return Err(invalid(
                    "$.bundle_file",
                    "archive contains excluded Heybox player telemetry",
                ));
            }
            if let Some(a) = account {
                if parts[0] != a.name {
                    return Err(invalid("$.bundle_file", "archive contains another account"));
                }
                if parts.len() > 2 && !parts[1].eq_ignore_ascii_case("SavedVariables") {
                    if parts.len() < 4
                        || !a
                            .characters
                            .iter()
                            .any(|c| c.realm == parts[1] && c.player == parts[2])
                    {
                        return Err(invalid(
                            "$.bundle_file",
                            "archive contains an unselected realm/character",
                        ));
                    }
                    if !directory {
                        roles.insert((parts[1].clone(), parts[2].clone()));
                    }
                }
            } else if !selected.contains(&parts[0])
                || (artifact.field == "Fonts" && (directory || parts.len() != 1))
                || (!directory && artifact.field != "Fonts" && parts.len() < 2)
            {
                return Err(invalid(
                    "$.bundle_file",
                    "archive contains an unselected component",
                ));
            }
            drop(member);
            let mut member = zip
                .by_index(index)
                .map_err(|_| invalid("$.bundle_file", "invalid compressed ZIP member"))?;
            let mut buffer = [0u8; 65536];
            let mut n = 0;
            loop {
                control.check()?;
                let count = member
                    .read(&mut buffer)
                    .map_err(|_| invalid("$.bundle_file", "ZIP checksum or payload is invalid"))?;
                if count == 0 {
                    break;
                }
                n += count as u64;
            }
            if n != member.size() {
                return Err(invalid("$.bundle_file", "ZIP member size mismatch"));
            }
            *sizes.entry(parts[0].clone()).or_default() += n;
        }
        let rows = descriptor["Files"]
            .as_array()
            .ok_or_else(|| invalid("$.bundle_file", "archive omitted its file sizes"))?;
        let mut expected = BTreeMap::new();
        for row in rows {
            let name = forms::text(&row["Name"], "$.bundle_file", true)?;
            local::name(name)?;
            let n = row["Size"]
                .as_u64()
                .ok_or_else(|| invalid("$.bundle_file", "invalid source size"))?;
            if expected.insert(name.to_owned(), n).is_some() {
                return Err(invalid("$.bundle_file", "duplicate component size"));
            }
        }
        if let Some(a) = account {
            let pairs = json!(
                a.characters
                    .iter()
                    .map(|c| json!({"Realm":c.realm,"Player":c.player}))
                    .collect::<Vec<_>>()
            );
            let names = json!(
                a.characters
                    .iter()
                    .map(|c| format!("{} - {}", c.realm, c.player))
                    .collect::<Vec<_>>()
            );
            if descriptor["Name"] != a.name
                || descriptor["RealmCharacters"] != pairs
                || descriptor["Characters"] != names
                || expected.len() != 1
                || !expected.contains_key(&a.name)
                || a.characters
                    .iter()
                    .any(|c| !roles.contains(&(c.realm.clone(), c.player.clone())))
            {
                return Err(invalid(
                    "$.bundle_file",
                    "account archive differs from selected characters",
                ));
            }
        } else if expected.keys().cloned().collect::<BTreeSet<_>>()
            != selected.iter().cloned().collect()
        {
            return Err(invalid(
                "$.bundle_file",
                "archive omitted a selected component",
            ));
        }
        if sizes != expected {
            return Err(invalid(
                "$.bundle_file",
                "archive layout or source byte sizes mismatch",
            ));
        }
        Ok(sizes.values().sum())
    })();
    storage::finish_file(raw, result, control)
}

#[cfg(test)]
mod tests;
