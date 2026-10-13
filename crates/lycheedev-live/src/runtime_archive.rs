//! Explicit archive-input locator research. A runtime archive is never a disk PE.
//! No type in this module issues RootRule or permits game input.
use crate::locator::{self, History, Layout, Method, Parameter, Pe};
use lycheedev_core::storage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::windows::fs::OpenOptionsExt,
    path::{Component, Path},
};
type Result<T> = std::result::Result<T, String>;
pub const ENGINE: &str = "lycheedev.runtime-archive-locator.v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Provenance {
    pub kind: String,
    pub product: String,
    pub build: String,
    pub module_sha256: String,
    pub manifest_sha256: String,
    pub manifest_file: String,
    pub module_name: String,
    pub domain_sha256: String,
    pub capture_id: String,
    pub pid: u32,
    pub created: u64,
    pub module_base: u64,
    pub atomic: bool,
    pub section: String,
    pub rva: u64,
    pub bytes: u64,
    pub fragment_hashes: Vec<String>,
    pub fragments: Vec<Fragment>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fragment {
    pub file: String,
    pub sha256: String,
    pub rva: u64,
    pub offset: u64,
    pub bytes: u64,
}
pub struct Archive {
    provenance: Provenance,
    bytes: Vec<u8>,
    _locks: Vec<File>,
}
#[derive(Debug, Clone)]
pub struct ArchivedMethod {
    method: Method,
    digest: String,
    source: Provenance,
}
#[derive(Debug, Serialize)]
pub struct Report {
    schema: String,
    input: Provenance,
    source_provenance: Provenance,
    history: History,
    source_replay_required: bool,
    disk_finalization: bool,
    execution_admission: bool,
    current_instance_revalidation: String,
}
impl Report {
    pub fn history(&self) -> &History {
        &self.history
    }
    pub fn input(&self) -> &Provenance {
        &self.input
    }
}
fn text(v: &Value, key: &str) -> Result<String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .map(str::to_owned)
        .ok_or_else(|| format!("runtime.input_field:{key}"))
}
fn number(v: &Value) -> Result<u64> {
    if let Some(n) = v.as_u64() {
        return Ok(n);
    }
    let s = v.as_str().ok_or("runtime.input_number")?;
    if let Some(hex) = s.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        s.parse()
    }
    .map_err(|_| "runtime.input_number".into())
}
fn locked(path: &Path, limit: u64) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > limit {
        return Err("runtime.file_budget".into());
    }
    Ok(file)
}
impl Archive {
    /// Hash and hold deny-write/delete handles; selected domain must exactly cover PE section.
    pub fn open(manifest: &Path, original: &Pe, section: &str) -> Result<Self> {
        let root = manifest.parent().ok_or("runtime.input_parent")?;
        storage::guard_path(root, manifest).map_err(|e| e.to_string())?;
        let parent = root.canonicalize().map_err(|e| e.to_string())?;
        let manifest = parent.join(manifest.file_name().ok_or("runtime.input_name")?);
        let mut manifest_file = locked(&manifest, 8 << 20)?;
        let mut raw = Vec::new();
        manifest_file
            .read_to_end(&mut raw)
            .map_err(|e| e.to_string())?;
        let document: Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
        let source = &document["source"];
        if document["schema"] != "wowdump.analysis-input.v2"
            || source["kind"] != "runtime"
            || source["atomic"] != false
            || document["moduleSha256"] != original.sha256
            || document["identity"]["executableSha256"] != original.sha256
            || source["processInstance"]["executableSha256"] != original.sha256
        {
            return Err("runtime.input_identity".into());
        }
        let capture = &document["metadata"]["capture"];
        for observed in [&capture["startedProcess"], &capture["endedProcess"]["Ok"]] {
            if !observed.is_null()
                && (observed["instance"] != source["processInstance"]["instance"]
                    || observed["executableSha256"] != original.sha256
                    || observed["pid"] != source["processInstance"]["pid"])
            {
                return Err("runtime.capture_instance_mismatch".into());
            }
        }
        let instance = text(&source["processInstance"], "instance")?;
        let (pid, created) = instance.split_once(':').ok_or("runtime.process_identity")?;
        let pid: u32 = pid.parse().map_err(|_| "runtime.process_identity")?;
        let created: u64 = created.parse().map_err(|_| "runtime.process_identity")?;
        if pid == 0 || created == 0 || source["processInstance"]["pid"].as_u64() != Some(pid as u64)
        {
            return Err("runtime.process_identity".into());
        }
        let module_base = number(&source["moduleBase"])?;
        if module_base == 0 || module_base.checked_add(original.image_size).is_none() {
            return Err("runtime.module_base".into());
        }
        let sections = document["sections"]
            .as_array()
            .filter(|s| s.len() <= 96)
            .ok_or("runtime.sections_budget")?;
        let matching = sections
            .iter()
            .filter(|s| s["name"] == section)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err("runtime.domain_ambiguous".into());
        }
        let domain = matching[0];
        let pe_section = original
            .sections
            .iter()
            .find(|s| s.name == section && s.flags & 0x20000000 != 0)
            .ok_or("runtime.domain_not_executable")?;
        if number(&domain["rva"])? != pe_section.rva
            || number(&domain["size"])? != pe_section.virtual_size
            || pe_section.virtual_size == 0
            || pe_section.virtual_size > 128 << 20
            || domain["executable"] != true
        {
            return Err("TARGET_COVERAGE_INCOMPLETE".into());
        }
        let fragments = domain["fragments"]
            .as_array()
            .filter(|s| !s.is_empty() && s.len() <= 4096)
            .ok_or("runtime.fragments_budget")?;
        let mut sorted = fragments
            .iter()
            .map(|f| Ok((number(&f["rva"])?, f)))
            .collect::<Result<Vec<_>>>()?;
        sorted.sort_by_key(|(rva, _)| *rva);
        let mut cursor = pe_section.rva;
        let mut bytes = Vec::with_capacity(pe_section.virtual_size as usize);
        let mut locks = vec![manifest_file];
        let mut hashes = Vec::new();
        let mut inputs = Vec::new();
        for (rva, fragment) in sorted {
            let size = number(&fragment["size"])?;
            let offset = number(&fragment["offset"])?;
            if rva != cursor
                || size == 0
                || size > 128 << 20
                || rva
                    .checked_add(size)
                    .is_none_or(|end| end > pe_section.rva + pe_section.virtual_size)
            {
                return Err("TARGET_COVERAGE_INCOMPLETE".into());
            }
            let name = text(fragment, "file")?;
            let relative = Path::new(&name);
            if relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
                || name.contains(':')
            {
                return Err("runtime.fragment_path".into());
            }
            let path = parent.join(relative);
            storage::guard_path(&parent, &path).map_err(|e| e.to_string())?;
            let mut file = locked(&path, 256 << 20)?;
            if offset
                .checked_add(size)
                .is_none_or(|end| end > file.metadata().map(|m| m.len()).unwrap_or(0))
            {
                return Err("runtime.fragment_bounds".into());
            }
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            let start = bytes.len();
            bytes.resize(start + size as usize, 0);
            file.read_exact(&mut bytes[start..])
                .map_err(|e| e.to_string())?;
            let digest = storage::sha256(&bytes[start..]);
            if fragment["sha256"] != digest {
                return Err("runtime.fragment_hash".into());
            }
            inputs.push(Fragment {
                file: path.to_string_lossy().into_owned(),
                sha256: digest.clone(),
                rva,
                offset,
                bytes: size,
            });
            hashes.push(digest);
            locks.push(file);
            cursor += size;
        }
        if bytes.len() as u64 != pe_section.virtual_size {
            return Err("TARGET_COVERAGE_INCOMPLETE".into());
        }
        let provenance = Provenance {
            kind: "runtime-archive".into(),
            product: text(&document["identity"], "product")?,
            build: text(&document["identity"], "buildKey")?,
            module_sha256: original.sha256.clone(),
            manifest_sha256: storage::sha256(&raw),
            manifest_file: manifest.to_string_lossy().into_owned(),
            module_name: text(&document, "moduleName")?,
            domain_sha256: storage::sha256(&bytes),
            capture_id: text(source, "captureId")?,
            pid,
            created,
            module_base,
            atomic: false,
            section: section.into(),
            rva: pe_section.rva,
            bytes: bytes.len() as u64,
            fragment_hashes: hashes,
            fragments: inputs,
        };
        Ok(Self {
            provenance,
            bytes,
            _locks: locks,
        })
    }
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    /// Read-only current-instance corroboration. Two complete code passes must match
    /// the archive. This remains non-atomic and does not issue a reader/input capability.
    pub fn validate_current(
        &self,
        process: &lycheedev_windows::Process,
        module: &lycheedev_windows::process::ModulePlan,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<CurrentValidation> {
        let start = std::time::Instant::now();
        let identity = process.identity();
        let pe = Pe::parse(module.disk())?;
        if identity.pid != self.provenance.pid
            || identity.created != self.provenance.created
            || module.hash() != self.provenance.module_sha256
            || module.base() != self.provenance.module_base
            || pe.image_size != module.size()
        {
            return Err("runtime.current_instance_mismatch".into());
        }
        let section = pe
            .sections
            .iter()
            .find(|s| {
                s.name == self.provenance.section
                    && s.rva == self.provenance.rva
                    && s.virtual_size == self.provenance.bytes
                    && s.flags & 0x20000000 != 0
            })
            .ok_or("runtime.current_domain_mismatch")?;
        let mut hashes = Vec::new();
        let mut reads = 0u64;
        let mut buffer = vec![0; 65536];
        for _ in 0..2 {
            process.verify()?;
            let mut hasher = Sha256::new();
            let mut offset = 0u64;
            while offset < section.virtual_size {
                control.check().map_err(|e| e.to_string())?;
                let size = (section.virtual_size - offset).min(buffer.len() as u64) as usize;
                process.read_module(
                    module,
                    module.base() + section.rva + offset,
                    &mut buffer[..size],
                )?;
                hasher.update(&buffer[..size]);
                offset += size as u64;
                reads += 1;
            }
            let digest = hex::encode(hasher.finalize());
            if digest != self.provenance.domain_sha256 {
                return Err("runtime.current_code_changed".into());
            }
            process.verify()?;
            hashes.push(digest);
        }
        Ok(CurrentValidation {schema:"lycheedev.runtime-current-validation.v1".into(),pid:identity.pid,created:identity.created,module_sha256:module.hash().into(),module_base:module.base(),domain_rva:self.provenance.rva,domain_bytes:self.provenance.bytes,domain_hashes:hashes,logical_reads:reads,elapsed_millis:start.elapsed().as_millis()as u64,atomic:false,execution_admission:false,scope:"two exact complete code-domain passes; no heap, ABI, reader or physical-input admission".into()})
    }
    fn scan(&self, pe: &Pe, method: &Method, source: &Provenance) -> Result<Report> {
        if pe.sha256 != self.provenance.module_sha256 || method.section != self.provenance.section {
            return Err("runtime.input_identity".into());
        }
        let digest =
            storage::sha256(&serde_json::to_vec(&self.provenance).map_err(|e| e.to_string())?);
        let mut history =
            locator::scan_domain(pe, method, self.provenance.rva, &self.bytes, &digest)?;
        history.engine = ENGINE.into();
        Ok(Report {
            schema: "lycheedev.runtime-archive-location.v1".into(),
            input: self.provenance.clone(),
            source_provenance: source.clone(),
            history,
            source_replay_required: false,
            disk_finalization: false,
            execution_admission: false,
            current_instance_revalidation:
                "not_performed: immutable archive search only; atomic=false is preserved".into(),
        })
    }
    pub fn finalize_source(
        &self,
        pe: &Pe,
        method: Method,
        independent_evidence: &[u8],
    ) -> Result<(ArchivedMethod, Report)> {
        if method.source.sha256 != self.provenance.module_sha256
            || method.source.build
                != format!("{}@{}", self.provenance.product, self.provenance.build)
            || method.source.evidence_kind != "independent-confirmation"
            || storage::sha256(independent_evidence) != method.source.evidence_sha256
        {
            return Err("SOURCE_PROOF_INCOMPLETE".into());
        }
        let report = self.scan(pe, &method, &self.provenance)?;
        if report.history.candidates.len() != 1 {
            return Err(if report.history.candidates.is_empty() {
                "SOURCE_METHOD_NO_MATCH"
            } else {
                "SOURCE_METHOD_AMBIGUOUS"
            }
            .into());
        }
        if report.history.result != Some(method.source.value) {
            return Err("SOURCE_LOCATION_CHANGED".into());
        }
        let archived = ArchivedMethod {
            digest: report.history.method_digest.clone(),
            method,
            source: self.provenance.clone(),
        };
        Ok((archived, report))
    }
    /// Target archive is rescanned completely; source archive files are never reread.
    pub fn locate(&self, pe: &Pe, method: &ArchivedMethod) -> Result<Report> {
        let report = self.scan(pe, &method.method, &method.source)?;
        if report.history.method_digest != method.digest {
            return Err("runtime.method_changed".into());
        }
        if report.history.candidates.len() != 1 {
            return Err(if report.history.candidates.is_empty() {
                "TARGET_METHOD_NO_MATCH"
            } else {
                "TARGET_METHOD_AMBIGUOUS"
            }
            .into());
        }
        Ok(report)
    }
}
#[derive(Debug, Serialize)]
pub struct CurrentValidation {
    schema: String,
    pid: u32,
    created: u64,
    module_sha256: String,
    module_base: u64,
    domain_rva: u64,
    domain_bytes: u64,
    domain_hashes: Vec<String>,
    logical_reads: u64,
    elapsed_millis: u64,
    atomic: bool,
    execution_admission: bool,
    scope: String,
}
pub fn bind(
    parameter: &Parameter,
    method: &ArchivedMethod,
    report: &Report,
    layout: &Layout,
) -> Result<u64> {
    if parameter.name.is_empty()
        || parameter.reader_pointer != "/root"
        || parameter.expected_type != method.method.output
        || parameter.method_id != method.method.id
        || report.history.method_digest != method.digest
        || report.history.candidates.len() != 1
    {
        return Err("reader.binding_missing".into());
    }
    layout.admit(&report.input.module_sha256)?;
    report
        .history
        .result
        .ok_or_else(|| "reader.binding_missing".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(second: bool, pid: u32) -> (Vec<u8>, Method, Vec<u8>, std::path::PathBuf, Value) {
        let (bytes, mut method, evidence) = crate::locator::tests::fixture(second);
        method.source.build = "fixture@1".into();
        let root = std::env::temp_dir().join(format!(
            "lycheedev-runtime-archive-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let domain = &bytes[0x200..0x300];
        std::fs::write(root.join("text.bin"), domain).unwrap();
        let manifest = serde_json::json!({"schema":"wowdump.analysis-input.v2","moduleName":"fixture.exe","moduleSha256":method.source.sha256,
            "identity":{"product":"fixture","buildKey":"1","executableSha256":method.source.sha256},
            "source":{"kind":"runtime","atomic":false,"captureId":format!("capture{pid}"),"moduleBase":"0x10000000","processInstance":{"pid":pid,"instance":format!("{pid}:9876"),"executableSha256":method.source.sha256}},
            "sections":[{"name":".text","rva":0x1000,"size":256,"executable":true,"fragments":[{"file":"text.bin","offset":0,"rva":0x1000,"size":256,"sha256":storage::sha256(domain)}]}]});
        storage::atomic_json(&root.join("manifest.json"), &manifest).unwrap();
        (bytes, method, evidence, root, manifest)
    }
    #[test]
    fn source_is_independent_and_target_replays_without_source_files_or_old_pid_filter() {
        let (bytes, method, evidence, root, _) = fixture(false, 100);
        let pe = Pe::parse(&bytes).unwrap();
        let source = Archive::open(&root.join("manifest.json"), &pe, ".text").unwrap();
        let (method, report) = source.finalize_source(&pe, method, &evidence).unwrap();
        assert_eq!(report.history.result, Some(0x2000));
        assert!(report.history.complete);
        assert!(!report.input.atomic);
        assert!(
            OpenOptions::new()
                .write(true)
                .open(root.join("text.bin"))
                .is_err(),
            "archive holds deny-write handle"
        );
        drop(source);
        std::fs::remove_file(root.join("manifest.json")).unwrap();
        std::fs::remove_file(root.join("text.bin")).unwrap();
        let (target_bytes, _, _, target, _) = fixture(false, 200);
        let target_pe = Pe::parse(&target_bytes).unwrap();
        let target = Archive::open(&target.join("manifest.json"), &target_pe, ".text").unwrap();
        let report = target.locate(&target_pe, &method).unwrap();
        assert_eq!((report.source_provenance.pid, report.input.pid), (100, 200));
        assert!(
            !report.source_replay_required
                && !report.execution_admission
                && !report.disk_finalization
        );
        assert_eq!(report.history.result, Some(0x2000));
    }
    #[test]
    fn ambiguity_and_wrong_independent_value_cannot_be_filtered_into_success() {
        for second in [false, true] {
            let (bytes, mut method, evidence, root, _) = fixture(second, 100);
            let pe = Pe::parse(&bytes).unwrap();
            let archive = Archive::open(&root.join("manifest.json"), &pe, ".text").unwrap();
            if !second {
                method.source.value = 0x2008;
            }
            let error = archive.finalize_source(&pe, method, &evidence).unwrap_err();
            assert_eq!(
                error,
                if second {
                    "SOURCE_METHOD_AMBIGUOUS"
                } else {
                    "SOURCE_LOCATION_CHANGED"
                }
            );
        }
    }
    #[test]
    fn changed_hash_gap_overlap_mixed_capture_and_path_escape_fail_closed() {
        for kind in ["hash", "gap", "overlap", "identity", "capture", "escape"] {
            let (bytes, _, _, root, mut manifest) = fixture(false, 100);
            let pe = Pe::parse(&bytes).unwrap();
            match kind {
                "hash" => {
                    std::fs::write(root.join("text.bin"), vec![0; 256]).unwrap();
                }
                "gap" => manifest["sections"][0]["fragments"][0]["size"] = serde_json::json!(128),
                "overlap" => {
                    let f = manifest["sections"][0]["fragments"][0].clone();
                    manifest["sections"][0]["fragments"]
                        .as_array_mut()
                        .unwrap()
                        .push(f);
                }
                "identity" => manifest["moduleSha256"] = serde_json::json!("f".repeat(64)),
                "capture" => {
                    manifest["metadata"]["capture"]["endedProcess"]["Ok"] = serde_json::json!({"pid":200,"instance":"200:9876","executableSha256":pe.sha256})
                }
                "escape" => {
                    manifest["sections"][0]["fragments"][0]["file"] =
                        serde_json::json!("../text.bin")
                }
                _ => unreachable!(),
            }
            storage::atomic_json(&root.join("manifest.json"), &manifest).unwrap();
            assert!(
                Archive::open(&root.join("manifest.json"), &pe, ".text").is_err(),
                "{kind}"
            );
        }
    }
}
