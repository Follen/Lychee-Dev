use super::*;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Named {
    name: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PendingRole {
    name: String,
    zip: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PendingServer {
    #[serde(rename = "serverName")]
    name: String,
    #[serde(rename = "roleList")]
    roles: Vec<PendingRole>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PendingAccount {
    id: NonZeroU64,
    account: String,
    server: Vec<PendingServer>,
    #[serde(rename = "commonConfig")]
    common: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FontSelection {
    name: String,
    size: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    knowplug_list: Vec<KnownPlugin>,
    #[serde(rename = "t_unKnown_list")]
    unknown: Vec<Named>,
    #[serde(rename = "t_font_list")]
    fonts: Vec<FontSelection>,
    #[serde(rename = "t_material_list")]
    materials: Vec<Named>,
    #[serde(rename = "t_WTF_list")]
    unused_wtf: Vec<()>,
    versionid: NonZeroU64,
    wtfconfig: Option<String>,
    unknown_plug: Option<String>,
    material: Option<String>,
    font: Option<String>,
    font_hash: Option<String>,
    wtf: Vec<PendingAccount>,
}
#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub(in crate::newbee) enum Category {
    #[serde(rename = "unknown_plug")]
    Unknown,
    #[serde(rename = "material")]
    Material,
    #[serde(rename = "wtfserve")]
    Common,
    #[serde(rename = "wtfrole")]
    Role,
}
impl Category {
    pub(in crate::newbee) fn key(self) -> &'static str {
        match self {
            Self::Unknown => "unknown_plug",
            Self::Material => "material",
            Self::Common => "wtfserve",
            Self::Role => "wtfrole",
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Part {
    Key(String),
    Index(usize),
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactSpec {
    file: String,
    size: u64,
    sha256: String,
    files: u64,
    file_type: Category,
    target: Vec<Part>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FontSpec {
    name: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSpec {
    schema: String,
    manifest: Manifest,
    artifacts: Vec<ArtifactSpec>,
    font_directory: Option<String>,
    font_files: Vec<FontSpec>,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::newbee) enum Target {
    Unknown,
    Material,
    Common(usize),
    Role(usize, usize, usize),
}
impl Target {
    pub(in crate::newbee) fn apply(&self, manifest: &mut Value, value: Value) {
        match self {
            Self::Unknown => manifest["unknown_plug"] = value,
            Self::Material => manifest["material"] = value,
            Self::Common(account) => manifest["wtf"][*account]["commonConfig"] = value,
            Self::Role(account, server, role) => {
                manifest["wtf"][*account]["server"][*server]["roleList"][*role]["zip"] = value
            }
        }
    }
}
pub(in crate::newbee) struct LockedArtifact {
    pub(in crate::newbee) file: File,
    pub(in crate::newbee) sha256: String,
    pub(in crate::newbee) size: u64,
    pub(in crate::newbee) category: Category,
    pub(in crate::newbee) target: Target,
}
pub(in crate::newbee) struct LockedFont {
    pub(in crate::newbee) name: String,
    pub(in crate::newbee) file: File,
    pub(in crate::newbee) sha256: String,
}
pub(in crate::newbee) struct ValidatedBundle {
    source: File,
    pub(in crate::newbee) sha256: String,
    pub(in crate::newbee) manifest: Value,
    pub(in crate::newbee) version: NonZeroU64,
    pub(in crate::newbee) artifacts: Vec<LockedArtifact>,
    pub(in crate::newbee) fonts: Vec<LockedFont>,
}
fn bundle_error(message: &str) -> ToolkitError {
    invalid("$.bundle_file", message)
}
fn digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn target(spec: &ArtifactSpec, manifest: &Manifest) -> Result<Target> {
    let result = match spec.target.as_slice() {
        [Part::Key(key)] if key == "unknown_plug" && spec.file_type == Category::Unknown => {
            Target::Unknown
        }
        [Part::Key(key)] if key == "material" && spec.file_type == Category::Material => {
            Target::Material
        }
        [Part::Key(root), Part::Index(account), Part::Key(field)]
            if root == "wtf"
                && field == "commonConfig"
                && spec.file_type == Category::Common
                && manifest.wtf.get(*account).is_some() =>
        {
            Target::Common(*account)
        }
        [
            Part::Key(root),
            Part::Index(account),
            Part::Key(servers),
            Part::Index(server),
            Part::Key(roles),
            Part::Index(role),
            Part::Key(field),
        ] if root == "wtf"
            && servers == "server"
            && roles == "roleList"
            && field == "zip"
            && spec.file_type == Category::Role
            && manifest
                .wtf
                .get(*account)
                .and_then(|value| value.server.get(*server))
                .and_then(|value| value.roles.get(*role))
                .is_some() =>
        {
            Target::Role(*account, *server, *role)
        }
        _ => {
            return Err(bundle_error(
                "artifact category and manifest target do not match",
            ));
        }
    };
    Ok(result)
}
fn manifest_names(manifest: &Manifest) -> Result<()> {
    local::distinct_names(
        &manifest
            .unknown
            .iter()
            .map(|value| value.name.clone())
            .collect::<Vec<_>>(),
    )?;
    local::distinct_names(
        &manifest
            .materials
            .iter()
            .map(|value| value.name.clone())
            .collect::<Vec<_>>(),
    )?;
    local::distinct_names(
        &manifest
            .fonts
            .iter()
            .map(|value| value.name.clone())
            .collect::<Vec<_>>(),
    )?;
    if manifest
        .materials
        .iter()
        .any(|value| value.name.eq_ignore_ascii_case("AddOns"))
    {
        return Err(bundle_error("material selection cannot contain AddOns"));
    }
    if manifest
        .knowplug_list
        .iter()
        .any(|value| value.name.is_empty())
        || manifest
            .knowplug_list
            .iter()
            .map(|value| value.id)
            .collect::<BTreeSet<_>>()
            .len()
            != manifest.knowplug_list.len()
    {
        return Err(bundle_error(
            "known plugin identities are invalid or duplicated",
        ));
    }
    let mut accounts = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for account in &manifest.wtf {
        local::name(&account.account)?;
        if !accounts.insert(account.account.to_lowercase())
            || !ids.insert(account.id)
            || !account.common.is_empty()
        {
            return Err(bundle_error(
                "pending account identity or common archive is invalid",
            ));
        }
        let mut servers = BTreeSet::new();
        for server in &account.server {
            local::name(&server.name)?;
            if !servers.insert(server.name.to_lowercase()) {
                return Err(bundle_error("WTF server selection is duplicated"));
            }
            let mut roles = BTreeSet::new();
            for role in &server.roles {
                local::name(&role.name)?;
                if !roles.insert(role.name.to_lowercase()) || !role.zip.is_empty() {
                    return Err(bundle_error("pending role identity or archive is invalid"));
                }
            }
        }
    }
    Ok(())
}
impl ValidatedBundle {
    pub(in crate::newbee) fn open(path: &Path, control: &OperationControl) -> Result<Self> {
        let root = path
            .parent()
            .ok_or_else(|| bundle_error("bundle file has no parent directory"))?;
        storage::guard_path_with_check(root, path, || control.check())?;
        let (mut source, sha256) = storage::locked_sha256(path, 32 * 1024 * 1024, control)?;
        source.seek(SeekFrom::Start(0))?;
        let result = (|| {
            let mut bytes = Vec::new();
            source.read_to_end(&mut bytes)?;
            control.check()?;
            let raw = crate::input::value(&bytes)?;
            for key in [
                "schema",
                "manifest",
                "artifacts",
                "font_directory",
                "font_files",
            ] {
                if raw.get(key).is_none() {
                    return Err(bundle_error("bundle is missing a required field"));
                }
            }
            let fields = [
                "knowplug_list",
                "t_unKnown_list",
                "t_font_list",
                "t_material_list",
                "t_WTF_list",
                "versionid",
                "wtfconfig",
                "unknown_plug",
                "material",
                "font",
                "font_hash",
                "wtf",
            ];
            if !raw["manifest"].as_object().is_some_and(|map| {
                map.len() == fields.len() && fields.iter().all(|key| map.contains_key(*key))
            }) {
                return Err(bundle_error(
                    "manifest must contain exactly the desktop backup fields",
                ));
            }
            for plugin in raw["manifest"]["knowplug_list"]
                .as_array()
                .ok_or_else(|| bundle_error("known plugin selection must be an array"))?
            {
                if [
                    "name",
                    "id",
                    "fileName",
                    "fileUrl",
                    "mod_file_id",
                    "mod_version",
                    "display_name",
                    "updateType",
                ]
                .iter()
                .any(|key| plugin.get(*key).is_none())
                {
                    return Err(bundle_error("known plugin manifest fields are incomplete"));
                }
            }
            let spec: BundleSpec = serde_json::from_value(raw).map_err(|_| {
                bundle_error("bundle fields do not match the typed pending manifest")
            })?;
            if spec.schema != BUNDLE_SCHEMA
                || !spec.manifest.unused_wtf.is_empty()
                || spec.manifest.wtfconfig.is_some()
                || spec.manifest.unknown_plug.is_some()
                || spec.manifest.material.is_some()
                || spec.manifest.font.is_some()
                || spec.manifest.font_hash.is_some()
            {
                return Err(bundle_error(
                    "bundle must contain pending local artifacts, without existing cloud references",
                ));
            }
            manifest_names(&spec.manifest)?;
            let mut artifacts = Vec::new();
            let mut targets = BTreeSet::new();
            let mut names = BTreeSet::new();
            for artifact in spec.artifacts {
                control.check()?;
                local::name(&artifact.file)?;
                if !digest(&artifact.sha256) || !names.insert(artifact.file.to_lowercase()) {
                    return Err(bundle_error(
                        "artifact hash or filename is invalid or duplicated",
                    ));
                }
                let target = target(&artifact, &spec.manifest)?;
                if !targets.insert(target.clone()) {
                    return Err(bundle_error("artifact target is duplicated"));
                }
                let path = local::path(root, &[&artifact.file], control)?;
                let (mut file, hash) = storage::locked_sha256(&path, u64::MAX, control)?;
                let admission = (|| {
                    if hash != artifact.sha256 || file.metadata()?.len() != artifact.size {
                        return Err(bundle_error(
                            "backup archive bytes differ from their original manifest identity",
                        ));
                    }
                    validate_zip(&mut file, &artifact, &target, &spec.manifest, control)
                })();
                if let Err(error) = admission {
                    return storage::finish_file(file, Err(error), control);
                }
                artifacts.push(LockedArtifact {
                    file,
                    sha256: hash,
                    size: artifact.size,
                    category: artifact.file_type,
                    target,
                });
            }
            if !spec.manifest.unknown.is_empty() != targets.contains(&Target::Unknown)
                || !spec.manifest.materials.is_empty() != targets.contains(&Target::Material)
            {
                return Err(bundle_error("selected components have no matching archive"));
            }
            for (ai, account) in spec.manifest.wtf.iter().enumerate() {
                let mut selected = targets.contains(&Target::Common(ai));
                for (si, server) in account.server.iter().enumerate() {
                    for (ri, _) in server.roles.iter().enumerate() {
                        if !targets.contains(&Target::Role(ai, si, ri)) {
                            return Err(bundle_error("selected role has no archive"));
                        }
                        selected = true;
                    }
                }
                if !selected {
                    return Err(bundle_error(
                        "selected account has no common or role archives",
                    ));
                }
            }
            if spec.font_directory.is_some() != !spec.manifest.fonts.is_empty() {
                return Err(bundle_error(
                    "font directory and manifest selection do not match",
                ));
            }
            let mut fonts = Vec::new();
            if let Some(directory) = spec.font_directory {
                let directory = local::path(root, &[&directory], control)?;
                let selected = spec
                    .manifest
                    .fonts
                    .iter()
                    .map(|value| value.name.clone())
                    .collect::<BTreeSet<_>>();
                let mut listed = BTreeSet::new();
                for font in spec.font_files {
                    local::name(&font.name)?;
                    if !font.name.to_ascii_lowercase().ends_with(".ttf")
                        || !digest(&font.sha256)
                        || !listed.insert(font.name.clone())
                    {
                        return Err(bundle_error("font entry is invalid or duplicated"));
                    }
                    let expected = spec
                        .manifest
                        .fonts
                        .iter()
                        .find(|value| value.name == font.name)
                        .ok_or_else(|| {
                            bundle_error("font metadata is absent from the selection")
                        })?;
                    let path = local::path(&directory, &[&font.name], control)?;
                    let (file, hash) = storage::locked_sha256(&path, u64::MAX, control)?;
                    if hash != font.sha256 || file.metadata()?.len() != expected.size {
                        return storage::finish_file(
                            file,
                            Err(bundle_error("selected font bytes changed")),
                            control,
                        );
                    }
                    fonts.push(LockedFont {
                        name: font.name,
                        file,
                        sha256: hash,
                    });
                }
                let actual = local::entries(root, &directory, control)?
                    .iter()
                    .map(|path| local::filename(path))
                    .collect::<Result<BTreeSet<_>>>()?;
                if selected != listed || actual != listed {
                    return Err(bundle_error(
                        "font directory contains missing or unselected entries",
                    ));
                }
            } else if !spec.font_files.is_empty() {
                return Err(bundle_error("unselected font metadata is present"));
            }
            if artifacts.is_empty() && fonts.is_empty() && spec.manifest.knowplug_list.is_empty() {
                return Err(bundle_error("bundle contains no selected backup content"));
            }
            let version = spec.manifest.versionid;
            Ok((
                serde_json::to_value(spec.manifest)?,
                version,
                artifacts,
                fonts,
            ))
        })();
        match result {
            Ok((manifest, version, artifacts, fonts)) => Ok(Self {
                source,
                sha256,
                manifest,
                version,
                artifacts,
                fonts,
            }),
            Err(error) => storage::finish_file(source, Err(error), control),
        }
    }
    pub(in crate::newbee) fn finish<T>(
        self,
        result: Result<T>,
        control: &OperationControl,
    ) -> Result<T> {
        let mut result = storage::finish_file(self.source, result, control);
        for artifact in self.artifacts {
            result = storage::finish_file(artifact.file, result, control);
        }
        for font in self.fonts {
            result = storage::finish_file(font.file, result, control);
        }
        result
    }
}
fn validate_zip(
    file: &mut File,
    spec: &ArtifactSpec,
    target: &Target,
    manifest: &Manifest,
    control: &OperationControl,
) -> Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut raw = file.try_clone()?;
    let result = (|| {
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|_| bundle_error("backup archive is not a supported ZIP"))?;
        control.check()?;
        let roots = match target {
            Target::Unknown => Some(
                manifest
                    .unknown
                    .iter()
                    .map(|value| value.name.as_str())
                    .collect::<BTreeSet<_>>(),
            ),
            Target::Material => Some(
                manifest
                    .materials
                    .iter()
                    .map(|value| value.name.as_str())
                    .collect(),
            ),
            Target::Role(a, s, r) => Some(BTreeSet::from([manifest.wtf[*a].server[*s].roles[*r]
                .name
                .as_str()])),
            Target::Common(_) => None,
        };
        let mut paths = crate::zip_contract::Paths::default();
        let mut actual_roots = BTreeSet::new();
        let mut files = 0;
        for index in 0..archive.len() {
            control.check()?;
            let (name, local_header, central_header, directory, encrypted) = {
                let member = archive
                    .by_index_raw(index)
                    .map_err(|_| bundle_error("backup ZIP member metadata is invalid"))?;
                (
                    member.name_raw().to_vec(),
                    member.header_start(),
                    member.central_header_start(),
                    member.is_dir(),
                    member.encrypted(),
                )
            };
            // Clones share a cursor; decode seeks again after raw admission.
            let parts = paths
                .admit(
                    &mut raw,
                    &name,
                    local_header,
                    central_header,
                    directory,
                    encrypted,
                )
                .map_err(|error| bundle_error(&error.message))?;
            let top = &parts[0];
            let allowed = match target {
                Target::Common(_) => {
                    (!directory && parts.len() == 1)
                        || (top.eq_ignore_ascii_case("SavedVariables")
                            && (directory || parts.len() > 1))
                }
                _ => {
                    roots.as_ref().unwrap().contains(top.as_str())
                        && (!matches!(target, Target::Unknown | Target::Role(..))
                            || directory
                            || parts.len() > 1)
                }
            };
            if !allowed
                || matches!(target, Target::Common(_) | Target::Role(..))
                    && !directory
                    && parts.last().unwrap().to_ascii_lowercase().ends_with(".bak")
            {
                return Err(bundle_error(
                    "backup ZIP contains an unselected component, role or backup file",
                ));
            }
            actual_roots.insert(top.clone());
            let mut member = archive
                .by_index(index)
                .map_err(|_| bundle_error("backup ZIP member cannot be decoded"))?;
            let expected = member.size();
            let mut length = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                control.check()?;
                let count = member.read(&mut buffer).map_err(|_| {
                    bundle_error("backup ZIP member checksum or compressed content is invalid")
                })?;
                if count == 0 {
                    break;
                }
                length = length
                    .checked_add(count as u64)
                    .ok_or_else(|| bundle_error("backup ZIP member length overflow"))?;
            }
            if length != expected {
                return Err(bundle_error(
                    "backup ZIP member length differs from its header",
                ));
            }
            if !directory {
                files += 1;
            }
        }
        if roots.is_some_and(|roots| {
            actual_roots
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                != roots
        }) || files != spec.files
        {
            return Err(bundle_error(
                "backup ZIP roots or file count differ from the selected manifest",
            ));
        }
        Ok(())
    })();
    storage::finish_file(raw, result, control)
}

#[cfg(test)]
mod tests;
