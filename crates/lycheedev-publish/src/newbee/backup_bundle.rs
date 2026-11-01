use super::*;
use crate::{
    contract::input_shape,
    input::{document, invalid},
    local_backup as local,
    provider::Provider,
};
use lycheedev_core::storage;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
};

pub(super) const BUNDLE_SCHEMA: &str = "lycheedev.publish.newbee.backup-bundle.v1";
pub(super) mod validation;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct KnownPlugin {
    name: String,
    id: NonZeroU64,
    #[serde(rename = "fileName")]
    file_name: Option<String>,
    #[serde(rename = "fileUrl")]
    file_url: Option<String>,
    mod_file_id: Option<NonZeroU64>,
    mod_version: Option<String>,
    display_name: Option<String>,
    #[serde(rename = "updateType")]
    update_type: NonZeroU64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountChoice {
    account: String,
    include_common: bool,
    roles: Vec<local::Role>,
}
pub struct BackupGenerate {
    root: PathBuf,
    output: PathBuf,
    version: NonZeroU64,
    known: Vec<KnownPlugin>,
    unknown: Vec<String>,
    materials: Vec<String>,
    fonts: Vec<String>,
    accounts: Vec<AccountChoice>,
}
fn input_array<T: serde::de::DeserializeOwned>(
    doc: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Vec<T>> {
    doc.get(field)
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|_| {
                invalid(
                    &format!("$.{field}"),
                    "selection does not match its nested type",
                )
            })
        })
        .unwrap_or(Ok(Vec::new()))
}
impl BackupGenerate {
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        control.check()?;
        Ok(
            json!({"provider":"newbee","validation":"local_generation_input","selection_scan":"on_execution","game_version_id":self.version,"output_dir":self.output,"known_plugins":self.known.len(),"unknown_plugins":self.unknown.len(),"materials":self.materials.len(),"fonts":self.fonts.len(),"wtf_accounts":self.accounts.len(),"author_writes":0,"credential_reads":false,"local_writes":0}),
        )
    }
    pub fn parse(bytes: &[u8], cwd: &Path) -> Result<Self> {
        let doc = document(bytes)?;
        input_shape(Provider::Newbee, "backup", "generate")?.admit(&doc)?;
        let selected_root = std::path::absolute(cwd.join(doc["game_dir"].as_str().unwrap()))?;
        if !selected_root.is_dir() {
            return Err(invalid("$.game_dir", "game directory does not exist"));
        }
        storage::guard_path(&selected_root, &selected_root)?;
        let root = fs::canonicalize(selected_root)?;
        let output = local::resolve_output(&cwd.join(doc["output_dir"].as_str().unwrap()))?;
        if output.starts_with(&root) {
            return Err(invalid(
                "$.output_dir",
                "backup output must be outside the game directory",
            ));
        }
        if output.exists()
            && (!output.is_dir() || fs::read_dir(&output)?.next().transpose()?.is_some())
        {
            return Err(invalid(
                "$.output_dir",
                "backup output must be an empty directory or a new path",
            ));
        }
        let known_values = doc
            .get("known_plugins")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let required = [
            "name",
            "id",
            "fileName",
            "fileUrl",
            "mod_file_id",
            "mod_version",
            "display_name",
            "updateType",
        ];
        for value in &known_values {
            if !value.is_object() || required.iter().any(|key| value.get(*key).is_none()) {
                return Err(invalid(
                    "$.known_plugins",
                    "known plugins require every desktop manifest field; IDs must come from platform evidence",
                ));
            }
        }
        let known: Vec<KnownPlugin> =
            serde_json::from_value(json!(known_values)).map_err(|_| {
                invalid(
                    "$.known_plugins",
                    "known plugin fields do not match the desktop manifest types",
                )
            })?;
        if known.iter().any(|item| item.name.is_empty())
            || known
                .iter()
                .map(|item| item.id)
                .collect::<BTreeSet<_>>()
                .len()
                != known.len()
        {
            return Err(invalid(
                "$.known_plugins",
                "known plugin names must be nonempty and IDs unique",
            ));
        }
        let unknown = input_array::<String>(&doc, "unknown_plugins")?;
        let materials = input_array::<String>(&doc, "materials")?;
        let fonts = input_array::<String>(&doc, "fonts")?;
        for items in [&unknown, &materials, &fonts] {
            local::distinct_names(items)?;
        }
        if materials
            .iter()
            .any(|item| item.eq_ignore_ascii_case("AddOns"))
        {
            return Err(invalid(
                "$.materials",
                "AddOns cannot be selected as a material folder",
            ));
        }
        let accounts = input_array::<AccountChoice>(&doc, "wtf_accounts")?;
        let mut seen = BTreeSet::new();
        for account in &accounts {
            local::name(&account.account)?;
            if !seen.insert(account.account.to_lowercase()) {
                return Err(invalid(
                    "$.wtf_accounts",
                    "account selection repeats a Windows name",
                ));
            }
            let mut roles = BTreeSet::new();
            for role in &account.roles {
                local::name(&role.server)?;
                local::name(&role.name)?;
                if !roles.insert((role.server.to_lowercase(), role.name.to_lowercase())) {
                    return Err(invalid(
                        "$.wtf_accounts",
                        "role selection repeats a Windows path",
                    ));
                }
            }
        }
        if known.is_empty()
            && unknown.is_empty()
            && materials.is_empty()
            && fonts.is_empty()
            && accounts.is_empty()
        {
            return Err(invalid("$", "select at least one backup component"));
        }
        Ok(Self {
            root,
            output,
            version: NonZeroU64::new(doc["game_version_id"].as_u64().unwrap()).unwrap(),
            known,
            unknown,
            materials,
            fonts,
            accounts,
        })
    }
    pub fn schema() -> Value {
        let mut schema = input_shape(Provider::Newbee, "backup", "generate")
            .unwrap()
            .schema();
        schema.as_object_mut().unwrap().remove("x-validation-scope");
        let props = &mut schema["properties"];
        for field in ["unknown_plugins", "materials", "fonts"] {
            props[field]["items"] = json!({"type":"string","minLength":1});
            props[field]["uniqueItems"] = json!(true);
        }
        props["known_plugins"]["items"] = json!({"type":"object","properties":{"name":{"type":"string","minLength":1},"id":{"type":"integer","minimum":1},"fileName":{"type":["string","null"]},"fileUrl":{"type":["string","null"]},"mod_file_id":{"type":["integer","null"],"minimum":1},"mod_version":{"type":["string","null"]},"display_name":{"type":["string","null"]},"updateType":{"type":"integer","minimum":1}},"required":["name","id","fileName","fileUrl","mod_file_id","mod_version","display_name","updateType"],"additionalProperties":false});
        props["wtf_accounts"]["items"] = json!({"type":"object","properties":{"account":{"type":"string","minLength":1},"include_common":{"type":"boolean"},"roles":{"type":"array","items":{"type":"object","properties":{"server":{"type":"string","minLength":1},"name":{"type":"string","minLength":1}},"required":["server","name"],"additionalProperties":false}}},"required":["account","include_common","roles"],"additionalProperties":false});
        schema["anyOf"] = json!(
            [
                "known_plugins",
                "unknown_plugins",
                "materials",
                "fonts",
                "wtf_accounts"
            ]
            .iter()
            .map(|field| {
                let mut choice = json!({"required":[field],"properties":{}});
                choice["properties"][*field] = json!({"minItems":1});
                choice
            })
            .collect::<Vec<_>>()
        );
        props_account_condition(&mut schema);
        schema["description"] = json!(
            "Generate only explicit local selections. Scan returns directory candidates, not official addon identities; known plugin manifest fields require existing platform evidence. Output must be outside the game root and empty. All source links/reparse points and unselected role trees are excluded or rejected; generated artifacts bind their actual hashes and file counts."
        );
        schema
    }
    pub fn generate(&self, control: &OperationControl) -> Result<Value> {
        let available = local::scan(&self.root, control)?;
        for (field, selected, choices) in [
            ("unknown_plugins", &self.unknown, &available.unknown_plugins),
            ("materials", &self.materials, &available.materials),
        ] {
            if selected.iter().any(|value| !choices.contains(value)) {
                return Err(invalid(
                    &format!("$.{field}"),
                    "selected component is absent from the local scan",
                ));
            }
        }
        if self
            .fonts
            .iter()
            .any(|name| !available.fonts.iter().any(|font| font.name == *name))
        {
            return Err(invalid(
                "$.fonts",
                "selected font is absent from the local scan",
            ));
        }
        let mut accounts = Vec::new();
        let mut sources = Vec::new();
        for account in &self.accounts {
            let current = available
                .accounts
                .iter()
                .find(|value| value.account == account.account)
                .ok_or_else(|| {
                    invalid(
                        "$.wtf_accounts",
                        "selected WTF account is absent from the local scan",
                    )
                })?;
            if account
                .roles
                .iter()
                .any(|role| !current.roles.contains(role))
            {
                return Err(invalid(
                    "$.wtf_accounts",
                    "selected WTF role is absent from the local scan",
                ));
            }
            let directory =
                local::path(&self.root, &["WTF", "Account", &account.account], control)?;
            let common = if account.include_common {
                local::common_sources(&self.root, &directory, control)?
            } else {
                Vec::new()
            };
            if common.is_empty() && account.roles.is_empty() {
                return Err(invalid(
                    "$.wtf_accounts",
                    "selected account has no common content or selected roles",
                ));
            }
            let mut servers: Vec<Value> = Vec::new();
            let mut roles = Vec::new();
            for role in &account.roles {
                let server = if let Some(index) = servers
                    .iter()
                    .position(|value| value["serverName"] == role.server)
                {
                    index
                } else {
                    servers.push(json!({"serverName":role.server,"roleList":[]}));
                    servers.len() - 1
                };
                let index = servers[server]["roleList"].as_array().unwrap().len();
                servers[server]["roleList"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"name":role.name,"zip":""}));
                roles.push((
                    local::path(
                        &self.root,
                        &["WTF", "Account", &account.account, &role.server, &role.name],
                        control,
                    )?,
                    server,
                    index,
                ));
            }
            accounts.push(json!({"id":accounts.len()+1,"account":account.account,"server":servers,"commonConfig":""}));
            sources.push((common, roles));
        }
        // Recheck the destination at the write boundary; files are always new.
        if self.output.exists() && fs::read_dir(&self.output)?.next().transpose()?.is_some() {
            return Err(invalid("$.output_dir", "backup output is no longer empty"));
        }
        fs::create_dir_all(&self.output)?;
        storage::guard_path_with_check(&self.output, &self.output, || control.check())?;
        let mut manifest = json!({"knowplug_list":self.known,"t_unKnown_list":[],"t_font_list":[],"t_material_list":[],"t_WTF_list":[],"versionid":self.version,"wtfconfig":null,"unknown_plug":null,"material":null,"font":null,"font_hash":null,"wtf":accounts});
        let mut artifacts = Vec::new();
        for (selected, base, file_type, list_key) in [
            (
                &self.unknown,
                vec!["Interface", "AddOns"],
                "unknown_plug",
                "t_unKnown_list",
            ),
            (
                &self.materials,
                vec!["Interface"],
                "material",
                "t_material_list",
            ),
        ] {
            if selected.is_empty() {
                continue;
            }
            let files = selected
                .iter()
                .map(|name| {
                    let mut parts = base.clone();
                    parts.push(name.as_str());
                    local::path(&self.root, &parts, control)
                })
                .collect::<Result<Vec<_>>>()?;
            let artifact = local::archive(
                &self.root,
                &files,
                None,
                &self.output.join(format!("{file_type}.zip")),
                false,
                control,
            )?;
            let mut artifact = serde_json::to_value(artifact)?;
            artifact["file_type"] = json!(file_type);
            artifact["target"] = json!([file_type]);
            artifacts.push(artifact);
            manifest[list_key] = json!(
                selected
                    .iter()
                    .map(|name| json!({"name":name}))
                    .collect::<Vec<_>>()
            );
        }
        for (account, (common, roles)) in sources.iter().enumerate() {
            if !common.is_empty() {
                let artifact = local::archive(
                    &self.root,
                    common,
                    None,
                    &self.output.join(format!("wtf-{account}-common.zip")),
                    true,
                    control,
                )?;
                let mut artifact = serde_json::to_value(artifact)?;
                artifact["file_type"] = json!("wtfserve");
                artifact["target"] = json!(["wtf", account, "commonConfig"]);
                artifacts.push(artifact);
            }
            for (index, (source, server, role)) in roles.iter().enumerate() {
                let artifact = local::archive(
                    &self.root,
                    std::slice::from_ref(source),
                    None,
                    &self.output.join(format!("wtf-{account}-role-{index}.zip")),
                    true,
                    control,
                )?;
                let mut artifact = serde_json::to_value(artifact)?;
                artifact["file_type"] = json!("wtfrole");
                artifact["target"] =
                    json!(["wtf", account, "server", server, "roleList", role, "zip"]);
                artifacts.push(artifact);
            }
        }
        let mut font_files = Vec::new();
        if !self.fonts.is_empty() {
            let directory = self.output.join("Fonts");
            fs::create_dir(&directory)?;
            for name in &self.fonts {
                let source = local::path(&self.root, &["Fonts", name], control)?;
                let target = directory.join(name);
                local::copy(&self.root, &source, &target, control)?;
                let (file, sha256) = storage::locked_sha256(&target, u64::MAX, control)?;
                let size = file.metadata()?.len();
                storage::finish_file(file, Ok(()), control)?;
                font_files.push(json!({"name":name,"sha256":sha256}));
                manifest["t_font_list"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"name":name,"size":size}));
            }
        }
        let bundle = json!({"schema":BUNDLE_SCHEMA,"manifest":manifest,"artifacts":artifacts,"font_directory":if self.fonts.is_empty(){None}else{Some("Fonts")},"font_files":font_files});
        let bundle_file = self.output.join("backup-bundle.json");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&bundle_file)?;
        let result = (|| {
            file.write_all(&serde_json::to_vec_pretty(&bundle)?)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            Ok(())
        })();
        storage::finish_file(file, result, control)?;
        Ok(output::public(
            json!({"bundle_file":bundle_file,"manifest":manifest,"artifacts":artifacts,"font_files":font_files,"remote_writes":0}),
            &[],
        ))
    }
}
pub fn scan_backup(root: &Path, control: &OperationControl) -> Result<Value> {
    let selected = std::path::absolute(root)?;
    let result = local::scan(&selected, control)?;
    Ok(
        json!({"game_dir":selected,"unknown_plugins":result.unknown_plugins.iter().map(|name|json!({"name":name})).collect::<Vec<_>>(),"materials":result.materials.iter().map(|name|json!({"name":name})).collect::<Vec<_>>(),"fonts":result.fonts,"wtf_accounts":result.accounts,"blocked":result.blocked,"coverage":if result.blocked.is_empty(){"complete"}else{"partial"},"automatic_addon_identification":"not_performed"}),
    )
}
fn props_account_condition(schema: &mut Value) {
    schema["properties"]["wtf_accounts"]["items"]["allOf"] = json!([{"if":{"properties":{"include_common":{"const":false}},"required":["include_common"]},"then":{"properties":{"roles":{"minItems":1}}}}]);
}

#[cfg(test)]
pub(super) mod tests;
