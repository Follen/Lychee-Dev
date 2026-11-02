//! Offline addon packages. Selected bytes and NCC project metadata are local
//! inputs; no desktop session, vendor executor or managed game deployment.
use super::*;
use crate::{input::invalid, local_backup as local, zip_contract};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
};

pub(super) const MAX_ZIP: u64 = 2048 * 1024 * 1024;
#[derive(Default, Deserialize)]
pub(super) struct Manifest {
    pub(super) id: Option<NonZeroU64>,
    #[serde(default)]
    pub(super) version: String,
    #[serde(default)]
    pub(super) game_versions: Vec<String>,
    #[serde(default)]
    pub(super) changelog_file: String,
    #[serde(default)]
    pub(super) ignore: Vec<String>,
    #[serde(default)]
    pub(super) bundle: Vec<String>,
}
pub struct PluginPack {
    directory: PathBuf,
    output: PathBuf,
    version: String,
}
impl PluginPack {
    pub(super) fn for_upload(
        directory: &Path,
        cwd: &Path,
        version: &str,
        control: &OperationControl,
    ) -> Result<Self> {
        let doc = json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.pack","dir":directory,"version":version});
        Self::parse(&serde_json::to_vec(&doc)?, cwd, control)
    }
    pub(super) fn output(&self) -> &Path {
        &self.output
    }
    pub fn parse(bytes: &[u8], cwd: &Path, control: &OperationControl) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Doc {
            schema: String,
            dir: Option<String>,
            version: Option<String>,
            out: Option<String>,
        }
        let raw = input::document(bytes)?;
        if raw.values().any(Value::is_null) {
            return Err(invalid(
                "$",
                "omit optional package fields; null is invalid",
            ));
        }
        let doc: Doc = serde_json::from_value(json!(raw))
            .map_err(|_| invalid("$", "plugin package input differs from its schema"))?;
        if doc.schema != "lycheedev.publish.v1.newbee.official.plugin.pack" {
            return Err(invalid("$.schema", "select official plugin pack schema"));
        }
        if doc.dir.as_ref().is_some_and(|v| v.trim().is_empty())
            || doc.out.as_ref().is_some_and(|v| v.trim().is_empty())
        {
            return Err(invalid(
                "$",
                "package directory/output paths must not be blank",
            ));
        }
        let directory = std::path::absolute(cwd.join(doc.dir.unwrap_or_else(|| ".".into())))?;
        let manifest = manifest(&directory, control)?;
        let version = doc.version.unwrap_or(manifest.version);
        if version.trim().is_empty() {
            return Err(invalid(
                "$.version",
                "provide a version or set ncc.json version",
            ));
        }
        let base = local::filename(&directory)?;
        local::name(&base)?;
        let safe = version
            .chars()
            .map(|c| {
                if c < ' ' || "\\/:*?\"<>|".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .collect::<String>();
        let output = cwd.join(doc.out.unwrap_or_else(|| format!("{base}-{safe}.zip")));
        if !output.parent().is_some_and(Path::is_dir) {
            return Err(invalid(
                "$.out",
                "package output requires an existing parent directory",
            ));
        }
        Ok(Self {
            directory,
            output,
            version,
        })
    }
    pub fn schema() -> Value {
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"schema":{"const":"lycheedev.publish.v1.newbee.official.plugin.pack"},"dir":{"type":"string","minLength":1,"default":"."},"version":{"type":"string","minLength":1,"description":"Version label, defaulting to ncc.json version; used in the default ZIP filename."},"out":{"type":"string","minLength":1,"description":"ZIP file path with an existing parent. Default: invocation directory / <folder>-<version>.zip. An existing package is reused only after exact member/content validation; different bytes are never overwritten."}},"required":["schema"],"additionalProperties":false})
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let package = Package::select(&self.directory, control)?;
        let result = package.summary(&self.version, &self.output, true)?;
        package.finish(Ok(result), control)
    }
    pub fn run(&self, control: &OperationControl) -> Result<Value> {
        let mut package = Package::select(&self.directory, control)?;
        let result = package.write(&self.output, &self.version, control);
        package.finish(result, control)
    }
}
pub(super) fn manifest(directory: &Path, control: &OperationControl) -> Result<Manifest> {
    let path = directory.join("ncc.json");
    if !path.try_exists()? {
        return Ok(Manifest::default());
    }
    let raw = local_json(&path, control)?;
    if raw.get("game").is_some_and(|v| v != "wow") || raw.get("type").is_some_and(|v| v != "mod") {
        return Err(invalid(
            "$.dir",
            "ncc.json identifies another game or resource type",
        ));
    }
    serde_json::from_value(raw)
        .map_err(|_| invalid("$.dir", "ncc.json package fields are malformed"))
}
pub(super) fn local_json(path: &Path, control: &OperationControl) -> Result<Value> {
    let text = read_local_text(path, control)?;
    input::value(text.strip_prefix('\u{feff}').unwrap_or(&text).as_bytes())
}
fn read_local_text(path: &Path, control: &OperationControl) -> Result<String> {
    storage::guard_path_with_check(
        path.parent()
            .ok_or_else(|| invalid("$.file", "file requires a parent"))?,
        path,
        || control.check(),
    )?;
    let (mut file, _) = storage::locked_sha256(path, 32 * 1024 * 1024, control)?;
    let result = (|| {
        file.seek(SeekFrom::Start(0))?;
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|_| invalid("$.file", "project metadata must be UTF-8"))?;
        Ok(text)
    })();
    storage::finish_file(file, result, control)
}
pub(super) fn ignore_patterns(
    directory: &Path,
    configured: &[String],
    control: &OperationControl,
) -> Result<Vec<regex::Regex>> {
    let mut patterns = configured.to_vec();
    for name in [".nccignore", ".gitignore"] {
        let path = directory.join(name);
        if path.try_exists()? {
            let text = read_local_text(&path, control)?;
            patterns.extend(
                text.trim_start_matches('\u{feff}')
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with(['#', '!']))
                    .map(|line| {
                        line.trim_start_matches('/')
                            .trim_end_matches('/')
                            .to_owned()
                    }),
            );
            break;
        }
    }
    patterns
        .iter()
        .map(|value| {
            let mut pattern = if value.contains('/') {
                "^".to_owned()
            } else {
                "(^|/)".to_owned()
            };
            let mut chars = value.chars().peekable();
            while let Some(ch) = chars.next() {
                match ch {
                    '*' if chars.peek() == Some(&'*') => {
                        chars.next();
                        pattern.push_str(".*");
                    }
                    '*' => pattern.push_str("[^/]*"),
                    '?' => pattern.push_str("[^/]"),
                    other => pattern.push_str(&regex::escape(&other.to_string())),
                }
            }
            pattern.push_str("(/|$)");
            regex::Regex::new(&pattern)
                .map_err(|_| invalid("$.ignore", "NCC ignore pattern is invalid"))
        })
        .collect()
}
pub(super) fn omitted(name: &str, relative: &str, patterns: &[regex::Regex]) -> bool {
    name.starts_with('.')
        || ["ncc.json", "Thumbs.db", "addon_version.txt"]
            .iter()
            .any(|v| name.eq_ignore_ascii_case(v))
        || name.to_ascii_lowercase().ends_with(".zip")
        || patterns.iter().any(|p| p.is_match(relative))
}
pub(super) fn children(path: &Path, control: &OperationControl) -> Result<Vec<PathBuf>> {
    control.check()?;
    let mut paths = fs::read_dir(path)?
        .map(|entry| entry.map(|e| e.path()).map_err(ToolkitError::from))
        .collect::<Result<Vec<_>>>()?;
    paths.sort_by_key(|p| p.file_name().unwrap().to_string_lossy().to_lowercase());
    Ok(paths)
}
pub(super) fn has_toc(path: &Path, control: &OperationControl) -> Result<bool> {
    Ok(children(path, control)?.iter().any(|p| {
        p.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toc"))
            && p.is_file()
    }))
}
pub(super) struct Source {
    member: String,
    file: File,
    sha256: String,
    size: u64,
}
pub(super) struct Package {
    root_name: String,
    layout: &'static str,
    addon_count: usize,
    skipped: Vec<String>,
    sources: Vec<Source>,
}
impl Package {
    pub(super) fn snapshot(&self) -> BTreeMap<String, (u64, String)> {
        self.sources
            .iter()
            .map(|s| {
                let name = if self.layout == "single" {
                    s.member.split_once('/').unwrap().1
                } else {
                    &s.member
                };
                (name.to_owned(), (s.size, s.sha256.clone()))
            })
            .collect()
    }
    pub(super) fn layout(&self) -> &'static str {
        self.layout
    }
    pub(super) fn select(directory: &Path, control: &OperationControl) -> Result<Self> {
        storage::guard_path_with_check(directory, directory, || control.check())?;
        if !directory.is_dir() {
            return Err(invalid(
                "$.dir",
                "select an existing addon or collection directory",
            ));
        }
        let manifest = manifest(directory, control)?;
        let root_name = local::filename(directory)?;
        local::name(&root_name)?;
        let mut modules = vec![directory.to_owned()];
        let mut names = BTreeSet::from([root_name.to_lowercase()]);
        for name in &manifest.bundle {
            let name = name.trim();
            if name.is_empty() || name.eq_ignore_ascii_case(&root_name) {
                continue;
            }
            local::name(name)?;
            if !names.insert(name.to_lowercase()) {
                continue;
            }
            let path = directory
                .parent()
                .ok_or_else(|| invalid("$.bundle", "bundle requires a parent"))?
                .join(name);
            storage::guard_path_with_check(directory.parent().unwrap(), &path, || control.check())?;
            if !path.is_dir() || !has_toc(&path, control)? {
                return Err(invalid(
                    "$.bundle",
                    "each sibling module must exist and contain a TOC",
                ));
            }
            modules.push(path);
        }
        let single = has_toc(directory, control)?;
        if modules.len() > 1 && !single {
            return Err(invalid("$.bundle", "bundle main module must contain a TOC"));
        }
        let mut addon_count = modules.len();
        let layout = if modules.len() > 1 {
            "bundle"
        } else if single {
            "single"
        } else {
            addon_count = 0;
            for path in children(directory, control)? {
                let name = local::filename(&path)?;
                if omitted(&name, &name, &[]) {
                    continue;
                }
                storage::guard_path_with_check(directory, &path, || control.check())?;
                if path.is_dir() && has_toc(&path, control)? {
                    addon_count += 1;
                }
            }
            if addon_count == 0 {
                return Err(invalid(
                    "$.dir",
                    "directory and its immediate child directories contain no TOC",
                ));
            }
            "collection"
        };
        let mut package = Self {
            root_name,
            layout,
            addon_count,
            skipped: Vec::new(),
            sources: Vec::new(),
        };
        let mut seen = BTreeSet::new();
        for module in modules {
            let patterns = ignore_patterns(&module, &manifest.ignore, control)?;
            let module_name = local::filename(&module)?;
            let mut pending = vec![module.clone()];
            while let Some(parent) = pending.pop() {
                for path in children(&parent, control)?.into_iter().rev() {
                    let relative = path
                        .strip_prefix(&module)
                        .map_err(|_| invalid("$.dir", "package source escaped its module"))?;
                    let rel = relative
                        .to_str()
                        .ok_or_else(|| invalid("$.dir", "package member names must be UTF-8"))?
                        .replace('\\', "/");
                    let name = local::filename(&path)?;
                    if omitted(&name, &rel, &patterns) {
                        continue;
                    }
                    storage::guard_path_with_check(&module, &path, || control.check())?;
                    local::name(&name)?;
                    if path.is_dir() {
                        pending.push(path);
                        continue;
                    }
                    if !path.is_file() {
                        return Err(invalid(
                            "$.dir",
                            "package selection contains a non-regular file",
                        ));
                    }
                    if layout == "collection" && parent == module {
                        package.skipped.push(name);
                        continue;
                    }
                    let member = if layout == "collection" {
                        rel
                    } else {
                        format!("{module_name}/{rel}")
                    };
                    if !seen.insert(member.to_lowercase()) {
                        return Err(invalid("$.dir", "package repeats a Windows member path"));
                    }
                    let (file, sha256) = storage::locked_sha256(&path, u64::MAX, control)?;
                    let size = file.metadata()?.len();
                    package.sources.push(Source {
                        member,
                        file,
                        sha256,
                        size,
                    });
                }
            }
        }
        package.sources.sort_by(|a, b| a.member.cmp(&b.member));
        package.skipped.sort();
        if package.sources.is_empty() {
            return Err(invalid("$.dir", "ignore rules selected no package files"));
        }
        if !package
            .sources
            .iter()
            .any(|s| s.member.to_ascii_lowercase().ends_with(".toc"))
        {
            return Err(invalid(
                "$.dir",
                "package must retain at least one selected TOC",
            ));
        }
        if layout == "bundle" {
            for module in names {
                if !package.sources.iter().any(|s| {
                    s.member
                        .split('/')
                        .next()
                        .is_some_and(|name| name.to_lowercase() == module)
                        && s.member.split('/').count() == 2
                        && s.member.to_ascii_lowercase().ends_with(".toc")
                }) {
                    return Err(invalid(
                        "$.bundle",
                        "every bundle module must retain its own selected top-level TOC",
                    ));
                }
            }
        }
        Ok(package)
    }
    fn identity(&self) -> Value {
        json!(
            self.sources
                .iter()
                .map(|s| json!({"member":s.member,"sha256":s.sha256,"size":s.size}))
                .collect::<Vec<_>>()
        )
    }
    fn summary(&self, version: &str, output: &Path, plan: bool) -> Result<Value> {
        Ok(
            json!({"version":version,"file":output,"root_name":self.root_name,"layout":self.layout,"addon_count":self.addon_count,"file_count":self.sources.len(),"source_bytes":self.sources.iter().try_fold(0u64,|sum,s|sum.checked_add(s.size)).ok_or_else(||invalid("$.dir","source byte count overflow"))?,"skipped_top_files":self.skipped,"input_sha256":storage::sha256(&storage::identity_json(&self.identity())?),"archive_validation":if plan{"not_built"}else{"original_bytes"},"credential_reads":false,"author_writes":0,"plan":plan}),
        )
    }
    pub(super) fn write(
        &mut self,
        output: &Path,
        version: &str,
        control: &OperationControl,
    ) -> Result<Value> {
        let mut summary = self.summary(version, output, false)?;
        if output.try_exists()? {
            let (mut file, sha256) = storage::locked_sha256(output, MAX_ZIP, control)?;
            let result = (|| {
                let inventory = inventory(&mut file, control)?;
                let expected = self
                    .sources
                    .iter()
                    .map(|s| (s.member.clone(), (s.size, s.sha256.clone())))
                    .collect::<BTreeMap<_, _>>();
                if inventory.files != expected {
                    return Err(invalid(
                        "$.out",
                        "existing ZIP differs from selected original files; select another output path",
                    ));
                }
                summary["sha256"] = json!(sha256);
                summary["zip_bytes"] = json!(file.metadata()?.len());
                summary["reused"] = json!(true);
                Ok(summary)
            })();
            return storage::finish_file(file, result, control);
        }
        let temporary = output
            .parent()
            .ok_or_else(|| invalid("$.out", "output needs a parent"))?
            .join(format!(
                ".lycheedev-package-{}.tmp",
                lycheedev_windows::random::token().map_err(|_| ToolkitError::new(
                    "publish.package_id",
                    "cannot create package staging identity",
                    3
                ))?
            ));
        let mut owned =
            lycheedev_windows::filesystem::owned_cleanup::OwnedTemporary::create(&temporary)?;
        let result: Result<Value> = (|| {
            {
                let writer = Limited {
                    file: owned.file_mut(),
                    position: 0,
                };
                let mut archive = zip::ZipWriter::new(writer);
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated);
                for source in &mut self.sources {
                    control.check()?;
                    archive
                        .start_file(&source.member, options)
                        .map_err(zip_error)?;
                    source.file.seek(SeekFrom::Start(0))?;
                    let mut buffer = [0u8; 65536];
                    loop {
                        control.check()?;
                        let n = source.file.read(&mut buffer)?;
                        if n == 0 {
                            break;
                        }
                        archive.write_all(&buffer[..n])?;
                    }
                }
                archive.finish().map_err(zip_error)?;
            }
            owned.file_mut().sync_all()?;
            let length = owned.file_mut().metadata()?.len();
            owned.file_mut().seek(SeekFrom::Start(0))?;
            save_reader(output, owned.file_mut(), length, MAX_ZIP, false, control)?;
            let (file, sha256) = storage::locked_sha256(output, MAX_ZIP, control)?;
            storage::finish_file(file, Ok(()), control)?;
            summary["sha256"] = json!(sha256);
            summary["zip_bytes"] = json!(length);
            summary["reused"] = json!(false);
            Ok(summary)
        })();
        let (file, discard) = owned.discard();
        let result = match (result, discard) {
            (Ok(mut value), Err(_)) => {
                value["cleanup_pending"] = json!(true);
                value["temporary"] = json!(temporary);
                Ok(value)
            }
            (Err(mut error), Err(_)) => {
                error.details["cleanup_pending"] = json!(true);
                error.details["temporary"] = json!(temporary);
                Err(error)
            }
            (value, Ok(())) => value,
        };
        storage::finish_file(file, result, control)
    }
    pub(super) fn finish<T>(self, mut result: Result<T>, control: &OperationControl) -> Result<T> {
        for source in self.sources {
            result = storage::finish_file(source.file, result, control);
        }
        result
    }
}
struct Limited<'a> {
    file: &'a mut File,
    position: u64,
}
impl Write for Limited<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .position
            .checked_add(bytes.len() as u64)
            .is_none_or(|n| n > MAX_ZIP)
        {
            return Err(std::io::Error::other("package exceeds 2048 MiB"));
        }
        let n = self.file.write(bytes)?;
        self.position += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
impl Seek for Limited<'_> {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.position = self.file.seek(position)?;
        Ok(self.position)
    }
}
pub(super) fn zip_error(_: zip::result::ZipError) -> ToolkitError {
    zip_contract::invalid("addon ZIP cannot be encoded or decoded")
}
pub(super) struct Inventory {
    pub(super) files: BTreeMap<String, (u64, String)>,
    pub(super) interfaces: BTreeSet<u64>,
}
pub(super) fn inventory(file: &mut File, control: &OperationControl) -> Result<Inventory> {
    inventory_to(file, None, control)
}
pub(super) fn extract(
    file: &mut File,
    root: &Path,
    control: &OperationControl,
) -> Result<Inventory> {
    inventory_to(file, Some(root), control)
}
fn inventory_to(
    file: &mut File,
    root: Option<&Path>,
    control: &OperationControl,
) -> Result<Inventory> {
    file.seek(SeekFrom::Start(0))?;
    let mut raw = file.try_clone()?;
    let result = (|| {
        let mut archive = zip::ZipArchive::new(file).map_err(zip_error)?;
        let mut paths = zip_contract::Paths::default();
        let mut files = BTreeMap::new();
        let mut interfaces = BTreeSet::new();
        let mut directories = Vec::new();
        for index in 0..archive.len() {
            control.check()?;
            let (name, local, central, directory, encrypted) = {
                let m = archive.by_index_raw(index).map_err(zip_error)?;
                (
                    m.name_raw().to_vec(),
                    m.header_start(),
                    m.central_header_start(),
                    m.is_dir(),
                    m.encrypted(),
                )
            };
            let parts = paths.admit(&mut raw, &name, local, central, directory, encrypted)?;
            let name = parts.join("/");
            if directory {
                if archive.by_index_raw(index).map_err(zip_error)?.size() != 0 {
                    return Err(zip_contract::invalid(
                        "addon ZIP directory contains unselected file content",
                    ));
                }
                directories.push(name);
                continue;
            }
            if parts.len() < 2 && root.is_none() {
                return Err(zip_contract::invalid(
                    "addon ZIP files require a top-level module directory",
                ));
            }
            let mut member = archive.by_index(index).map_err(zip_error)?;
            let mut output = if let Some(root) = root {
                let path = root.join(&name);
                storage::guard_path_with_check(root, &path, || control.check())?;
                fs::create_dir_all(path.parent().unwrap())?;
                Some(
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)?,
                )
            } else {
                None
            };
            let size = member.size();
            let mut length = 0u64;
            let mut digest = Sha256::new();
            let mut toc = Vec::new();
            let is_toc = name.to_ascii_lowercase().ends_with(".toc");
            let mut buffer = [0u8; 65536];
            let decoded: Result<()> = (|| {
                loop {
                    control.check()?;
                    let n = member.read(&mut buffer).map_err(|_| {
                        zip_contract::invalid("addon ZIP checksum or compressed member is invalid")
                    })?;
                    if n == 0 {
                        break;
                    }
                    length = length
                        .checked_add(n as u64)
                        .ok_or_else(|| zip_contract::invalid("ZIP member size overflow"))?;
                    digest.update(&buffer[..n]);
                    if let Some(output) = &mut output {
                        output.write_all(&buffer[..n])?;
                    }
                    if is_toc {
                        if toc.len() + n > 32 * 1024 * 1024 {
                            return Err(zip_contract::invalid(
                                "addon TOC exceeds the text metadata limit",
                            ));
                        }
                        toc.extend_from_slice(&buffer[..n]);
                    }
                }
                Ok(())
            })();
            if let Some(output) = output {
                storage::finish_file(output, decoded, control)?;
            } else {
                decoded?;
            }
            if length != size {
                return Err(zip_contract::invalid(
                    "addon ZIP member length differs from its header",
                ));
            }
            if is_toc {
                interfaces.extend(toc_interfaces(&toc));
            }
            files.insert(name, (size, hex::encode(digest.finalize())));
        }
        if files.is_empty() {
            return Err(zip_contract::invalid("addon ZIP contains no files"));
        }
        for dir in directories.into_iter().filter(|_| root.is_none()) {
            if !files
                .keys()
                .any(|file| file.starts_with(&format!("{dir}/")))
            {
                return Err(zip_contract::invalid(
                    "addon ZIP contains an unselected empty directory",
                ));
            }
        }
        Ok(Inventory { files, interfaces })
    })();
    storage::finish_file(raw, result, control)
}
fn toc_interfaces(bytes: &[u8]) -> BTreeSet<u64> {
    let mut result = BTreeSet::new();
    let pattern = regex::Regex::new(r"(?i)^##\s*Interface[^:]*:\s*(.+)$").unwrap();
    for line in String::from_utf8_lossy(bytes)
        .trim_start_matches('\u{feff}')
        .lines()
    {
        if let Some(c) = pattern.captures(line) {
            for value in c[1].split(',') {
                if let Ok(value) = value.trim().parse::<u64>() {
                    result.insert(value);
                }
            }
        }
    }
    result
}
struct Controlled<'a>(&'a OperationControl);
impl storage::FileControl for Controlled<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, _: storage::FileCost) -> Result<()> {
        self.0.check()
    }
    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
}
pub(super) fn save_reader(
    path: &Path,
    reader: &mut impl Read,
    length: u64,
    maximum: u64,
    overwrite: bool,
    control: &OperationControl,
) -> Result<()> {
    let mut scope = Controlled(control);
    let destination = storage::Destination::prepare_controlled(path, overwrite, &mut scope)?;
    let publication = destination.publish_reader_controlled(reader, length, maximum, &mut scope);
    let close = destination.close();
    let cleanup = publication.cleanup.len() + close.as_ref().err().map(Vec::len).unwrap_or(0);
    let state = match publication.publication {
        storage::StagedPublication::Published => "published",
        storage::StagedPublication::Unknown => "unknown",
        storage::StagedPublication::NotPublished => "not_published",
    };
    if let Err(mut error) = publication.result {
        error.details["output_publication"] = json!(state);
        error.details["cleanup_errors"] = json!(cleanup);
        return Err(error);
    }
    if cleanup != 0 {
        return Err(ToolkitError::new(
            "publish.output_cleanup",
            "output was published with pending file cleanup",
            6,
        )
        .with_details(json!({"file":path,"output_publication":state,"cleanup_errors":cleanup})));
    }
    Ok(())
}
#[cfg(test)]
mod tests;
