//! Pinned real LuaLS: authenticated runtime bytes, isolated frozen input,
//! Lua 5.1 configuration, bounded check process and public LSP navigation.
use crate::{git, lsp_frame, process};
use lycheedev_core::{Result, ToolkitError, storage};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
// Keep the enclosing broker budget at 2 GiB. The LuaLS child cannot consume the
// final 256 MiB needed by control threads, raw frames and response processing.
// This partition is not a proof of all owner-side allocations; full workload
// and lifecycle acceptance remain necessary, including source initialization.
const LUALS_COMMIT_BUDGET: usize = (2 << 30) - (256 << 20);
pub fn pinned() -> Value {
    serde_json::from_str(include_str!("../../../release/tools/luals.json"))
        .expect("pinned runtime metadata")
}
pub struct Runtime {
    root: PathBuf,
    inventory: BTreeMap<String, String>,
}
pub struct CheckArtifacts {
    pub assessment: Value,
    pub raw_report: Vec<u8>,
    pub configuration: Vec<u8>,
}
pub fn configuration(library: &Path) -> Value {
    json!({"runtime.version":"Lua 5.1","runtime.path":["?.lua","?/init.lua"],"runtime.plugin":"","runtime.pluginArgs":[],"workspace.library":[library],"workspace.checkThirdParty":"Disable","workspace.userThirdParty":[],"workspace.dofileRoots":[],"workspace.ignoreDir":[".git","node_modules"],"workspace.useGitIgnore":false,"workspace.ignoreSubmodules":false,"files.associations":{},"files.exclude":{"**/.git":true,"**/node_modules":true},"workspace.maxPreload":20000,"workspace.preloadFileSize":1024,"addonManager.enable":false,"telemetry.enable":false,"diagnostics.libraryFiles":"Disable","diagnostics.neededFileStatus":{"codestyle-check":"None"}})
}
impl Runtime {
    /// Legacy public LuaLS identity authenticates the pinned distribution ZIP.
    pub fn identity(&self) -> Value {
        pinned()
    }
    /// Separate actual executable identity from the public archive identity.
    /// Constructors authenticated this inventory; launch retains read locks.
    pub fn executable_identity(&self) -> Value {
        json!({"path":self.root.join("bin/lua-language-server.exe"),"sha256":self.inventory.get("bin/lua-language-server.exe")})
    }
    /// Caller must authenticate the sealed release and fixed runtime.json before
    /// supplying its stripped tool/luals inventory. This constructor verifies
    /// every member and uses only this selected release root.
    pub fn from_verified_release(root: &Path, inventory: BTreeMap<String, String>) -> Result<Self> {
        let runtime = Self::prepare_inspected_release(root, inventory)?;
        runtime.verify()?;
        Ok(runtime)
    }
    /// Construct a non-executing provider from a release inventory the CLI has
    /// already inspected. No worker/check can start until lock_verified hashes
    /// every runtime member and holds read locks through its actual lifetime.
    /// This avoids repeating a full runtime scan before the broker endpoint can
    /// be admitted; it does not skip authenticated launch verification.
    pub fn prepare_inspected_release(
        root: &Path,
        inventory: BTreeMap<String, String>,
    ) -> Result<Self> {
        if inventory.len() > 20000
            || inventory.iter().any(|(name, digest)| {
                !git::source_path(name) || !lycheedev_core::selection::valid_digest(digest, 64)
            })
        {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Invalid sealed runtime inventory",
                3,
            ));
        }
        let runtime = Self {
            root: ordinary_path(fs::canonicalize(root)?),
            inventory,
        };
        Ok(runtime)
    }
    /// Exact pinned ZIP digest authenticates the complete runtime inventory.
    /// Extraction never executes archive files and rejects nonlocal/symlink entries.
    pub fn unpack_pinned(archive: &Path, destination: &Path) -> Result<Self> {
        let identity = pinned();
        let expected_bytes = identity["bytes"].as_u64().unwrap();
        let mut input = fs::File::open(archive)?;
        let metadata = input.metadata()?;
        if !metadata.is_file() || metadata.len() != expected_bytes {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Pinned archive byte identity differs",
                3,
            ));
        }
        let mut raw = Vec::new();
        (&mut input)
            .take(expected_bytes + 1)
            .read_to_end(&mut raw)?;
        if raw.len() as u64 != identity["bytes"].as_u64().unwrap()
            || storage::sha256(&raw) != identity["sha256"].as_str().unwrap()
        {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Pinned runtime archive identity differs",
                3,
            ));
        }
        if destination.exists() {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Runtime extraction target already exists",
                3,
            ));
        }
        fs::create_dir(destination)?;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw))
            .map_err(|e| ToolkitError::new("luals.unavailable", e.to_string(), 3))?;
        let mut inventory = BTreeMap::new();
        let mut total = 0usize;
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| ToolkitError::new("luals.unavailable", e.to_string(), 3))?;
            let name = entry.name().replace('\\', "/");
            if name.ends_with('/') {
                let path = storage::safe_relative(destination, &name)?;
                fs::create_dir_all(path)?;
                continue;
            }
            if !git::source_path(&name) || entry.unix_mode().is_some_and(|m| m & 0xf000 == 0xa000) {
                return Err(ToolkitError::new(
                    "luals.unavailable",
                    "Unsafe runtime archive member",
                    3,
                ));
            }
            let mut bytes = Vec::new();
            entry
                .by_ref()
                .take((32 << 20) + 1)
                .read_to_end(&mut bytes)?;
            total += bytes.len();
            if bytes.len() > 32 << 20 || total > 128 << 20 {
                return Err(ToolkitError::new(
                    "luals.input_budget",
                    "Runtime archive byte budget",
                    3,
                ));
            }
            let path = destination.join(&name);
            if let Some(p) = path.parent() {
                fs::create_dir_all(p)?;
            }
            fs::write(path, &bytes)?;
            if inventory.insert(name, storage::sha256(&bytes)).is_some() {
                return Err(ToolkitError::new(
                    "luals.unavailable",
                    "Duplicate runtime archive member",
                    3,
                ));
            }
        }
        let runtime = Self {
            root: ordinary_path(fs::canonicalize(destination)?),
            inventory,
        };
        runtime.verify()?;
        Ok(runtime)
    }
    pub fn verify(&self) -> Result<()> {
        self.verify_with_control(&Default::default())
    }
    pub fn verify_with_control(
        &self,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<()> {
        self.lock_verified(control).map(|_| ())
    }
    fn lock_verified(
        &self,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Vec<fs::File>> {
        let mut held = Vec::new();
        let mut total = 0u64;
        for (name, expected) in &self.inventory {
            control.check()?;
            let path = self.root.join(name);
            storage::guard_path(&self.root, &path)?;
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(1);
            }
            let mut file = options.open(path)?;
            let metadata = file.metadata()?;
            total = total.checked_add(metadata.len()).ok_or_else(|| {
                ToolkitError::new("luals.input_budget", "Runtime inventory size overflow", 3)
            })?;
            if !metadata.is_file() || metadata.len() > 32 << 20 || total > 128 << 20 {
                return Err(ToolkitError::new(
                    "luals.input_budget",
                    "Runtime inventory byte budget",
                    3,
                ));
            }
            let mut bytes = Vec::new();
            (&mut file).take((32 << 20) + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 != metadata.len() || storage::sha256(&bytes) != *expected {
                return Err(ToolkitError::new(
                    "luals.unavailable",
                    "Pinned runtime file changed",
                    3,
                ));
            }
            held.push(file);
        }
        for required in [
            "bin/lua-language-server.exe",
            "bin/main.lua",
            "main.lua",
            "LICENSE",
        ] {
            if !self.inventory.contains_key(required) {
                return Err(ToolkitError::new(
                    "luals.unavailable",
                    format!("Missing runtime member {required}"),
                    3,
                ));
            }
        }
        Ok(held)
    }
    fn freeze(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        definitions: &[u8],
    ) -> Result<Frozen> {
        if files.len() > 2048 || definitions.len() > 8 << 20 {
            return Err(ToolkitError::new(
                "luals.input_budget",
                "LuaLS input admission",
                3,
            ));
        }
        let mut total = definitions.len();
        for (name, bytes) in files {
            if !git::source_path(name)
                || !name.ends_with(".lua")
                || bytes.len() > 1 << 20
                || std::str::from_utf8(bytes).is_err()
            {
                return Err(ToolkitError::new(
                    "luals.input_budget",
                    "Invalid frozen Lua document",
                    3,
                ));
            }
            total += bytes.len();
        }
        if total > 32 << 20 {
            return Err(ToolkitError::new(
                "luals.input_budget",
                "LuaLS total input byte budget",
                3,
            ));
        }
        fs::create_dir_all(staging)?;
        let root = staging.join(format!(
            "luals-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let root = ordinary_path(fs::canonicalize(root)?);
        let workspace = root.join("workspace");
        let library = root.join("library");
        fs::create_dir(&workspace)?;
        fs::create_dir(&library)?;
        for (name, bytes) in files {
            let p = workspace.join(name);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(p, bytes)?;
        }
        fs::write(library.join("api.d.lua"), definitions)?;
        fs::write(
            root.join("config.json"),
            serde_json::to_vec(&configuration(&library))?,
        )?;
        Ok(Frozen {
            root,
            workspace,
            library,
        })
    }
    pub fn check(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        definitions: &[u8],
    ) -> Result<Value> {
        self.check_with_control(staging, files, definitions, &Default::default())
    }
    pub fn check_with_control(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        definitions: &[u8],
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Value> {
        Ok(self
            .check_artifacts_with_control(staging, files, definitions, control)?
            .assessment)
    }
    pub fn check_artifacts_with_control(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        definitions: &[u8],
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<CheckArtifacts> {
        control.check()?;
        let _held_runtime = self.lock_verified(control)?;
        let frozen = self.freeze(staging, files, definitions)?;
        let report = frozen.root.join("report.json");
        let mut cmd = Command::new(self.root.join("bin/lua-language-server.exe"));
        cmd.current_dir(&frozen.root).args([
            format!("--check={}", frozen.workspace.display()),
            "--num_threads=1".into(),
            format!("--configpath={}", frozen.root.join("config.json").display()),
            format!("--logpath={}", frozen.root.join("log").display()),
            format!("--metapath={}", frozen.root.join("meta").display()),
            format!("--check_out_path={}", report.display()),
            "--check_format=json".into(),
            "--checklevel=Warning".into(),
            "--quiet".into(),
        ]);
        let output = process::run_with_limits(
            cmd,
            32768,
            32768,
            control.remaining()?.min(Duration::from_secs(90)),
            None,
            Some(control.token()),
        )?;
        control.check()?;
        if ![0, 1].contains(&output.status) {
            return Err(ToolkitError::new(
                "luals.check_failed",
                format!("LuaLS check exited {}", output.status),
                5,
            ));
        }
        let mut report_file = fs::File::open(report).map_err(|_| {
            ToolkitError::new(
                "luals.check_failed",
                "LuaLS did not publish a complete check report",
                5,
            )
        })?;
        let mut raw = Vec::new();
        (&mut report_file)
            .take((8 << 20) + 1)
            .read_to_end(&mut raw)?;
        if raw.len() > 8 << 20 {
            return Err(ToolkitError::new(
                "luals.input_budget",
                "LuaLS report budget",
                3,
            ));
        }
        let decoded: Value = serde_json::from_slice(&raw)?;
        let mut diagnostics = Vec::new();
        if decoded != json!([]) {
            let map = decoded.as_object().ok_or_else(|| {
                ToolkitError::new("luals.check_failed", "Invalid diagnostic report", 5)
            })?;
            for (uri, items) in map {
                let path = frozen.path_from_uri(uri)?;
                if !files.contains_key(&path) {
                    return Err(ToolkitError::new(
                        "luals.check_failed",
                        "Diagnostic outside frozen closure",
                        5,
                    ));
                }
                let items = items.as_array().ok_or_else(|| {
                    ToolkitError::new("luals.check_failed", "Invalid diagnostic array", 5)
                })?;
                for d in items {
                    if ![1, 2].contains(&d["severity"].as_u64().unwrap_or(0))
                        || d["code"].as_str().unwrap_or("").is_empty()
                    {
                        return Err(ToolkitError::new(
                            "luals.check_failed",
                            "Invalid diagnostic record",
                            5,
                        ));
                    }
                    let mut d = d.clone();
                    d["path"] = json!(path);
                    diagnostics.push(d);
                    if diagnostics.len() > 10000 {
                        return Err(ToolkitError::new(
                            "luals.input_budget",
                            "LuaLS diagnostic count",
                            3,
                        ));
                    }
                }
            }
        }
        if output.status != 0 && diagnostics.is_empty() {
            return Err(ToolkitError::new(
                "luals.check_failed",
                "Nonzero LuaLS exit without diagnostics",
                5,
            ));
        }
        diagnostics.sort_by_key(|d| {
            format!(
                "{}:{:08}:{:08}:{}",
                d["path"].as_str().unwrap_or(""),
                d["range"]["start"]["line"].as_u64().unwrap_or(0),
                d["range"]["start"]["character"].as_u64().unwrap_or(0),
                d["code"].as_str().unwrap_or("")
            )
        });
        let mut configuration = Vec::new();
        fs::File::open(frozen.root.join("config.json"))?
            .take((64 << 10) + 1)
            .read_to_end(&mut configuration)?;
        if configuration.len() > 64 << 10 {
            return Err(ToolkitError::new(
                "luals.input_budget",
                "LuaLS configuration byte budget",
                3,
            ));
        }
        Ok(CheckArtifacts {
            assessment: json!({"runtime":pinned(),"state":"checked","files":files.len(),"passed":diagnostics.is_empty(),"diagnostics":diagnostics,"positionEncoding":"utf-16; zero-based lines and characters"}),
            raw_report: raw,
            configuration,
        })
    }
    pub fn analyze(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        definitions: &[u8],
        queries: &[Value],
    ) -> Result<Value> {
        self.analyze_with_control(staging, files, definitions, queries, &Default::default())
    }
    pub fn analyze_with_control(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        definitions: &[u8],
        queries: &[Value],
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Value> {
        let mut session = self.open_session_with_control(staging, files, definitions, control)?;
        let result = session.analyze_with_control(queries, control);
        session.close();
        result
    }
    /// Own one initialized real LuaLS worker for repeated batches of unchanged
    /// frozen documents. Each batch retains its caller's deadline and token.
    pub fn open_session_with_control(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        definitions: &[u8],
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<WorkspaceSession> {
        control.check()?;
        let held_runtime = self.lock_verified(control)?;
        let frozen = self.freeze(staging, files, definitions)?;
        let held_inputs = frozen.lock_inputs(files, definitions, control)?;
        let (watches, mutation) = self.watch_inputs(&frozen, control)?;
        std::str::from_utf8(definitions)
            .map_err(|_| ToolkitError::new("luals.input_budget", "Definitions are not UTF-8", 3))?;
        let (rpc, caps) = self.initialize_workspace(&frozen, control, mutation.clone())?;
        Ok(WorkspaceSession {
            rpc: Some(rpc),
            frozen: Some(frozen),
            held_runtime,
            held_inputs,
            files: files
                .iter()
                .map(|(name, bytes)| (name.clone(), bytes.len()))
                .collect(),
            capabilities: caps,
            watches,
            mutation,
            coverage: json!({"luaFiles":files.len(),"luaBytes":files.values().map(Vec::len).sum::<usize>(),"excludedFiles":0}),
            unavailable: Default::default(),
            opened: Default::default(),
            lease: None,
            batches: 0,
            life_deadline: Instant::now() + Duration::from_secs(600),
        })
    }
    fn watch_inputs(
        &self,
        frozen: &Frozen,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<(Vec<lycheedev_windows::ChangeWatch>, Arc<AtomicBool>)> {
        let mutation = Arc::new(AtomicBool::new(false));
        let mut watches = Vec::new();
        for path in [&self.root, &frozen.workspace, &frozen.library] {
            control.check()?;
            watches.push(
                lycheedev_windows::ChangeWatch::start(path, true, mutation.clone())
                    .map_err(|e| ToolkitError::new("luals.unavailable", e, 3))?,
            );
        }
        Ok((watches, mutation))
    }
    fn initialize_workspace(
        &self,
        frozen: &Frozen,
        control: &lycheedev_core::control::OperationControl,
        mutation: Arc<AtomicBool>,
    ) -> Result<(Rpc, lsp_frame::Capabilities)> {
        let mut cmd = Command::new(self.root.join("bin/lua-language-server.exe"));
        cmd.current_dir(&frozen.root).args([
            "--quiet".into(),
            format!("--configpath={}", frozen.root.join("config.json").display()),
            format!("--logpath={}", frozen.root.join("log").display()),
            format!("--metapath={}", frozen.root.join("meta").display()),
        ]);
        let mut rpc = Rpc::start(cmd, control.clone())?;
        rpc.input_changed = Some(mutation);
        let init=rpc.request("initialize",json!({"processId":std::process::id(),"rootUri":file_uri(&frozen.workspace)?,"rootPath":frozen.workspace,"workspaceFolders":[{"uri":file_uri(&frozen.workspace)?,"name":"frozen"}],"capabilities":{"general":{"positionEncodings":["utf-16"]},"workspace":{"configuration":false,"workspaceFolders":true},"textDocument":{"definition":{"linkSupport":false},"references":{},"hover":{"contentFormat":["markdown","plaintext"]}}}}))?;
        let caps = lsp_frame::capabilities(init.result())?;
        drop(init);
        rpc.notify("initialized", json!({}))?;
        Ok((rpc, caps))
    }
    /// Authenticate and stream every admitted Lua document of a fixed source
    /// tree into a private disk workspace, retaining the selected worktree lease.
    /// Files above LuaLS's one-MiB preload limit are counted as exclusions, never
    /// silently converted to complete absence. Nothing executes repository Lua.
    pub fn open_source_session_with_control(
        &self,
        home: &Path,
        pin: &lycheedev_core::selection::SourcePin,
        index: &crate::index::Index,
        definitions: &[u8],
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<WorkspaceSession> {
        let prepared = self.prepare_source_workspace(home, pin, index, definitions, control)?;
        self.open_prepared_source_session(prepared, control)
    }
    /// Freeze and authenticate the complete semantic input before spawning
    /// LuaLS. The broker can drop its already-consumed Index/environment when
    /// this owned preparation leaves their scope; no borrowed facts escape it.
    pub(crate) fn prepare_source_workspace(
        &self,
        home: &Path,
        pin: &lycheedev_core::selection::SourcePin,
        index: &crate::index::Index,
        definitions: &[u8],
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<PreparedSourceWorkspace> {
        control.check()?;
        git::validate_pin(pin)?;
        if index.summary["repository"] != pin.repository
            || index.summary["product"] != pin.product
            || index.summary["commit"] != pin.exact_commit
        {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Semantic index pin identity differs",
                4,
            ));
        }
        let lease = storage::ExclusiveLock::resource(
            &home.join("locks"),
            &format!("worktree:{}:{}", pin.repository, pin.exact_commit),
        )?;
        let held_runtime = self.lock_verified(control)?;
        let files = git::tree_with_control(home, pin, control)?
            .into_iter()
            .filter(|f| f.path.to_ascii_lowercase().ends_with(".lua"))
            .collect::<Vec<_>>();
        let total = files
            .iter()
            .try_fold(0usize, |sum, f| sum.checked_add(f.bytes))
            .ok_or_else(|| ToolkitError::new("luals.input_budget", "Workspace size overflow", 3))?;
        if files.len() > 20_000 || total > 1 << 30 {
            return Err(ToolkitError::new(
                "luals.input_budget",
                "Complete Lua workspace exceeds20k files/1GiB",
                3,
            ));
        }
        let mut canonical = std::collections::BTreeSet::new();
        for f in &files {
            control.check()?;
            if !canonical.insert(f.path.to_lowercase()) {
                return Err(ToolkitError::new(
                    "codebase.source_integrity",
                    "Windows source path collision",
                    4,
                ));
            }
        }
        let unavailable = files
            .iter()
            .filter(|f| f.bytes > 1 << 20 || f.oversized)
            .map(|f| f.path.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let admitted = files
            .iter()
            .filter(|f| !unavailable.contains(&f.path))
            .cloned()
            .collect::<Vec<_>>();
        let documents = index
            .documents
            .iter()
            .map(|d| (d.path.as_str(), d))
            .collect::<BTreeMap<_, _>>();
        let frozen = self.freeze(
            &home.join("tmp/source-semantic-workspaces"),
            &BTreeMap::new(),
            definitions,
        )?;
        let mut held_inputs = frozen.lock_inputs(&BTreeMap::new(), definitions, control)?;
        git::visit_blobs_with_control(home, pin, &admitted, control, |file, raw| {
            control.check()?;
            let expected = documents.get(file.path.as_str()).ok_or_else(|| {
                ToolkitError::new(
                    "codebase.source_integrity",
                    "Semantic Lua document is absent from Index",
                    4,
                )
            })?;
            if expected.bytes != raw.len() || expected.sha256 != storage::sha256(&raw) {
                return Err(ToolkitError::new(
                    "codebase.source_integrity",
                    "Semantic raw blob differs from Index",
                    4,
                ));
            }
            let path = storage::safe_relative(&frozen.workspace, &file.path)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            storage::guard_path(&frozen.root, &path)?;
            fs::write(&path, &raw)?;
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(1);
            }
            let mut handle = options.open(&path)?;
            if !handle.metadata()?.is_file() || handle.metadata()?.len() != expected.bytes as u64 {
                return Err(ToolkitError::new(
                    "codebase.source_integrity",
                    "Frozen source file identity changed",
                    4,
                ));
            }
            let mut actual = Vec::new();
            (&mut handle).take((1 << 20) + 1).read_to_end(&mut actual)?;
            if actual != raw {
                return Err(ToolkitError::new(
                    "codebase.source_integrity",
                    "Frozen source bytes changed",
                    4,
                ));
            }
            held_inputs.push(handle);
            Ok(())
        })?;
        let expected_members = admitted
            .iter()
            .map(|f| (f.path.clone(), f.bytes))
            .collect::<BTreeMap<_, _>>();
        frozen.verify_members(&expected_members, control)?;
        let (watches, mutation) = self.watch_inputs(&frozen, control)?;
        let session = WorkspaceSession {
            rpc: None,
            frozen: Some(frozen),
            held_runtime,
            held_inputs,
            files: expected_members,
            capabilities: lsp_frame::Capabilities {
                definition: false,
                references: false,
                hover: false,
            },
            watches,
            mutation,
            coverage: json!({"luaFiles":files.len(),"luaBytes":total,"excludedFiles":unavailable.len(),"reasons":if unavailable.is_empty(){vec![]}else{vec!["Lua files larger than1MiB were excluded from LuaLS preload"]},"sourcePin":pin,"indexIdentity":crate::index::fingerprint(pin)}),
            unavailable,
            opened: Default::default(),
            lease: Some(lease),
            batches: 0,
            life_deadline: Instant::now() + Duration::from_secs(600),
        };
        Ok(PreparedSourceWorkspace {
            session,
            definitions_bytes: definitions.len(),
            definitions_sha256: storage::sha256(definitions),
        })
    }
    pub(crate) fn open_prepared_source_session(
        &self,
        prepared: PreparedSourceWorkspace,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<WorkspaceSession> {
        control.check()?;
        let mut session = prepared.session;
        if session.changed() {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Sealed session inputs changed",
                3,
            ));
        }
        let frozen = session.frozen.as_ref().unwrap();
        let (rpc, caps) = self.initialize_workspace(frozen, control, session.mutation.clone())?;
        if prepared.definitions_bytes != 0 {
            let mut raw = Vec::new();
            raw.try_reserve_exact(prepared.definitions_bytes)
                .map_err(|_| {
                    ToolkitError::new("luals.input_budget", "Definitions allocation budget", 3)
                })?;
            fs::File::open(frozen.library.join("api.d.lua"))?
                .take((8 << 20) + 1)
                .read_to_end(&mut raw)?;
            if raw.len() != prepared.definitions_bytes
                || storage::sha256(&raw) != prepared.definitions_sha256
            {
                return Err(ToolkitError::new(
                    "luals.unavailable",
                    "Locked definitions changed",
                    3,
                ));
            }
            std::str::from_utf8(&raw).map_err(|_| {
                ToolkitError::new("luals.input_budget", "Definitions are not UTF-8", 3)
            })?;
        }
        session.rpc = Some(rpc);
        session.capabilities = caps;
        session.life_deadline = Instant::now() + Duration::from_secs(600);
        Ok(session)
    }
}
pub(crate) struct PreparedSourceWorkspace {
    // WorkspaceSession::drop closes locked handles BEFORE deleting the frozen
    // directory, including cancellation or initialization failure in phase2.
    session: WorkspaceSession,
    definitions_bytes: usize,
    definitions_sha256: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Analysis {
    pub runtime: Value,
    pub state: String,
    pub position_encoding: String,
    pub results: Vec<QueryResult>,
    pub coverage: Value,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Location {
    pub path: String,
    pub range: lsp_frame::Range,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct QueryResult {
    pub query: Value,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<Location>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
pub(crate) struct ResultAdmission {
    bytes: usize,
    queries: usize,
    rows: usize,
}
impl ResultAdmission {
    pub(crate) fn new() -> Self {
        Self {
            bytes: 0,
            queries: 0,
            rows: 0,
        }
    }
    fn query(&mut self, query: &Value) -> Result<()> {
        let bytes = lsp_frame::value_bytes(query, lsp_frame::FRAME_BYTES)?;
        self.queries = self
            .queries
            .checked_add(bytes)
            .filter(|n| *n <= lsp_frame::FRAME_BYTES)
            .ok_or_else(|| {
                ToolkitError::new(
                    "luals.input_budget",
                    "Accumulated query descriptor allocation budget",
                    3,
                )
            })?;
        Ok(())
    }
    fn result(&mut self, result: &QueryResult) -> Result<()> {
        let bytes = lsp_frame::serialized_bytes(result, lsp_frame::FRAME_BYTES)?;
        self.bytes = self
            .bytes
            .checked_add(bytes + usize::from(self.rows != 0))
            .filter(|n| *n <= lsp_frame::FRAME_BYTES)
            .ok_or_else(|| {
                ToolkitError::new(
                    "luals.input_budget",
                    "Accumulated semantic result byte budget",
                    3,
                )
            })?;
        self.rows += 1;
        Ok(())
    }
}
impl QueryResult {
    fn new(query: &Value, state: &str) -> Self {
        Self {
            query: query.clone(),
            state: state.into(),
            locations: None,
            hover: None,
            truncated: None,
            reason: None,
        }
    }
    fn truncate(&mut self) {
        self.truncated = Some(true);
        self.state = "incomplete".into();
        self.reason = Some("semantic result exceeded output budget".into());
    }
}
/// An initialized, immutable workspace worker. It is serialized by ownership;
/// cancellation, protocol error and admission failure retire it permanently.
pub struct WorkspaceSession {
    rpc: Option<Rpc>,
    frozen: Option<Frozen>,
    held_runtime: Vec<fs::File>,
    held_inputs: Vec<fs::File>,
    files: BTreeMap<String, usize>,
    capabilities: lsp_frame::Capabilities,
    watches: Vec<lycheedev_windows::ChangeWatch>,
    mutation: Arc<AtomicBool>,
    coverage: Value,
    unavailable: std::collections::BTreeSet<String>,
    opened: std::collections::BTreeSet<String>,
    lease: Option<storage::ExclusiveLock>,
    batches: usize,
    life_deadline: Instant,
}
impl WorkspaceSession {
    pub fn pid(&self) -> Option<u32> {
        self.rpc.as_ref().map(|r| r.child.id())
    }
    pub fn batches(&self) -> usize {
        self.batches
    }
    /// Diagnostic identity of the actual locked configuration, independent of
    /// LuaLS archive identity and of process memory counters. No source body is
    /// returned. The session retains the corresponding read lock until close.
    pub fn configuration_identity(&self) -> Result<Value> {
        let frozen = self
            .frozen
            .as_ref()
            .ok_or_else(|| ToolkitError::new("luals.unavailable", "Session retired", 3))?;
        let expected = serde_json::to_vec(&configuration(&frozen.library))?;
        let mut raw = Vec::new();
        fs::File::open(frozen.root.join("config.json"))?
            .take((64 << 10) + 1)
            .read_to_end(&mut raw)?;
        if raw.len() > 64 << 10 || raw != expected {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Locked session configuration changed",
                3,
            ));
        }
        Ok(
            json!({"sha256":storage::sha256(&raw),"bytes":raw.len(),"configuration":configuration(&frozen.library),"coverage":self.coverage}),
        )
    }
    pub fn usable(&mut self) -> bool {
        if Instant::now() >= self.life_deadline || self.batches >= 256 || self.changed() {
            self.close();
        }
        self.rpc.is_some()
    }
    pub fn close(&mut self) {
        // Rpc drop terminates its job, reaps the child and joins all pipe threads
        // before releasing authenticated runtime/input handles.
        self.rpc.take();
        self.opened.clear();
        self.watches.clear();
        self.held_inputs.clear();
        self.held_runtime.clear();
        self.frozen.take();
        self.lease.take();
    }
    pub fn analyze_with_control(
        &mut self,
        queries: &[Value],
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Value> {
        let result = self.analyze_typed(queries, control, &mut ResultAdmission::new())?;
        Ok(serde_json::to_value(result)?)
    }
    pub(crate) fn analyze_typed(
        &mut self,
        queries: &[Value],
        control: &lycheedev_core::control::OperationControl,
        admission: &mut ResultAdmission,
    ) -> Result<Analysis> {
        let result = self.analyze_inner(queries, control, admission);
        if result.is_err() {
            self.close();
        }
        result
    }
    fn analyze_inner(
        &mut self,
        queries: &[Value],
        control: &lycheedev_core::control::OperationControl,
        admission: &mut ResultAdmission,
    ) -> Result<Analysis> {
        control.check()?;
        if self.changed() {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Sealed session inputs changed",
                3,
            ));
        }
        if !self.usable() {
            return Err(ToolkitError::new("luals.unavailable", "Session retired", 3));
        }
        if queries.len() > 128 {
            return Err(ToolkitError::new(
                "luals.input_budget",
                "LuaLS query count",
                3,
            ));
        }
        for query in queries {
            let position = &query["position"];
            if !["definition", "references", "hover"]
                .contains(&query["kind"].as_str().unwrap_or(""))
                || !self.query_path(query["path"].as_str().unwrap_or(""))
                || !position["line"].as_u64().is_some_and(|n| n <= 1_000_000)
                || !position["character"]
                    .as_u64()
                    .is_some_and(|n| n <= 1_000_000)
            {
                return Err(ToolkitError::new(
                    "luals.input_budget",
                    "Invalid LuaLS query kind/path/UTF-16 position",
                    3,
                ));
            }
        }
        self.frozen
            .as_ref()
            .unwrap()
            .verify_members(&self.files, control)?;
        if self.changed() {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Sealed session inputs changed",
                3,
            ));
        }
        let rpc = self.rpc.as_mut().unwrap();
        rpc.control = control.clone();
        rpc.deadline = control
            .deadline()
            .min(self.life_deadline)
            .min(Instant::now() + Duration::from_secs(90));
        self.batches += 1;
        let mut results = Vec::new();
        results.try_reserve_exact(queries.len()).map_err(|_| {
            ToolkitError::new("luals.input_budget", "Semantic result allocation budget", 3)
        })?;
        // Match the Go batch contract: a definition request on the first
        // admitted worktree Lua file waits for that scope before external API
        // queries can answer from a fallback library scope. This is additional
        // protocol work, not a query descriptor or a user result row.
        if let Some(probe) = self.files.keys().next().cloned() {
            self.open_batch_document(&probe, control)?;
            let uri = file_uri(&self.frozen.as_ref().unwrap().workspace.join(&probe))?;
            let barrier = self.rpc.as_mut().unwrap().request(
                "textDocument/definition",
                json!({"textDocument":{"uri":uri},"position":{"line":0,"character":0}}),
            )?;
            drop(barrier);
        }
        for query in queries {
            control.check()?;
            admission.query(query)?;
            let kind = query["kind"].as_str().unwrap_or("");
            let path = query["path"].as_str().unwrap_or("");
            if !["definition", "references", "hover"].contains(&kind) || !self.query_path(path) {
                return Err(ToolkitError::new(
                    "luals.input_budget",
                    "Invalid LuaLS query kind/path",
                    3,
                ));
            }
            if self.unavailable.contains(path) {
                let mut result = QueryResult::new(query, "incomplete");
                result.locations = Some(Vec::new());
                result.reason = Some("query file unavailable or exceeds file budget".into());
                admission.result(&result)?;
                results.push(result);
                continue;
            }
            let supported = match kind {
                "definition" => self.capabilities.definition,
                "references" => self.capabilities.references,
                _ => self.capabilities.hover,
            };
            if !supported {
                let mut result = QueryResult::new(query, "unsupported");
                result.reason = Some("server did not advertise capability".into());
                admission.result(&result)?;
                results.push(result);
                continue;
            }
            self.open_batch_document(path, control)?;
            let document = if path == "@environment/api.d.lua" {
                self.frozen.as_ref().unwrap().library.join("api.d.lua")
            } else {
                self.frozen.as_ref().unwrap().workspace.join(path)
            };
            let mut params =
                json!({"textDocument":{"uri":file_uri(&document)?},"position":query["position"]});
            if kind == "references" {
                params["context"] = json!({"includeDeclaration":true});
            }
            let raw = self
                .rpc
                .as_mut()
                .unwrap()
                .request(&format!("textDocument/{kind}"), params)?;
            let mut result = QueryResult::new(query, "complete");
            if kind == "hover" {
                let (hover, cut) = lsp_frame::hover(raw.result())?;
                result.hover = Some(hover);
                if cut {
                    result.truncate();
                }
            } else {
                let parsed = lsp_frame::locations(raw.result())?;
                let mut locations = Vec::new();
                locations
                    .try_reserve_exact(parsed.items.len())
                    .map_err(|_| {
                        ToolkitError::new(
                            "luals.input_budget",
                            "Semantic locations allocation budget",
                            3,
                        )
                    })?;
                for item in parsed.items {
                    locations.push(Location {
                        path: self.frozen.as_ref().unwrap().path_from_uri(&item.uri)?,
                        range: item.range,
                    });
                }
                result.locations = Some(locations);
                if parsed.truncated {
                    result.truncate();
                }
            }
            drop(raw);
            admission.result(&result)?;
            results.push(result);
        }
        // Successful batches release every document they opened, including the
        // preload probe. Any error/cancel retires the whole Rpc through the
        // analyze_typed owner instead of waiting for cleanup notifications.
        for path in std::mem::take(&mut self.opened) {
            let frozen = self.frozen.as_ref().unwrap();
            let document = if path == "@environment/api.d.lua" {
                frozen.library.join("api.d.lua")
            } else {
                frozen.workspace.join(path)
            };
            self.rpc.as_mut().unwrap().notify(
                "textDocument/didClose",
                json!({"textDocument":{"uri":file_uri(&document)?}}),
            )?;
        }
        self.frozen
            .as_ref()
            .unwrap()
            .verify_members(&self.files, control)?;
        if self.changed() {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Sealed session inputs changed",
                3,
            ));
        }
        let complete =
            self.coverage["excludedFiles"] == 0 && results.iter().all(|r| r.state == "complete");
        Ok(Analysis {
            runtime: pinned(),
            state: if complete { "complete" } else { "partial" }.into(),
            position_encoding: "utf-16".into(),
            results,
            coverage: self.coverage.clone(),
        })
    }
    fn changed(&self) -> bool {
        self.mutation.load(Ordering::Acquire) || self.watches.iter().any(|w| w.changed())
    }
    fn open_batch_document(
        &mut self,
        path: &str,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<()> {
        control.check()?;
        if self.opened.contains(path) {
            return Ok(());
        }
        let frozen = self.frozen.as_ref().unwrap();
        let (document, maximum, expected) = if path == "@environment/api.d.lua" {
            (frozen.library.join("api.d.lua"), 8 << 20, None)
        } else {
            (frozen.workspace.join(path), 1 << 20, Some(self.files[path]))
        };
        let mut file = fs::File::open(&document)?;
        let metadata = file.metadata()?;
        let size = usize::try_from(metadata.len())
            .ok()
            .filter(|size| *size <= maximum)
            .filter(|size| expected.is_none_or(|expected| *size == expected))
            .filter(|_| metadata.is_file())
            .ok_or_else(|| {
                ToolkitError::new("luals.unavailable", "Frozen query input changed", 3)
            })?;
        let mut raw = Vec::new();
        raw.try_reserve_exact(size).map_err(|_| {
            ToolkitError::new("luals.input_budget", "Query document allocation budget", 3)
        })?;
        (&mut file).take(maximum as u64 + 1).read_to_end(&mut raw)?;
        if raw.len() != size {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Frozen query input changed",
                3,
            ));
        }
        self.rpc.as_mut().unwrap().notify("textDocument/didOpen", json!({"textDocument":{"uri":file_uri(&document)?,"languageId":"lua","version":1,"text":String::from_utf8_lossy(&raw)}}))?;
        self.opened.insert(path.to_owned());
        Ok(())
    }
    fn query_path(&self, path: &str) -> bool {
        path == "@environment/api.d.lua"
            || self.files.contains_key(path)
            || self.unavailable.contains(path)
    }
}
impl Drop for WorkspaceSession {
    fn drop(&mut self) {
        self.close();
    }
}

struct Frozen {
    root: PathBuf,
    workspace: PathBuf,
    library: PathBuf,
}
impl Drop for Frozen {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn ordinary_path(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    PathBuf::from(raw.strip_prefix("\\\\?\\").unwrap_or(&raw))
}
fn file_uri(path: &Path) -> Result<String> {
    let raw = path.to_string_lossy();
    let path = Path::new(raw.strip_prefix("\\\\?\\").unwrap_or(&raw));
    url::Url::from_file_path(path)
        .map(String::from)
        .map_err(|_| ToolkitError::new("luals.check_failed", "Invalid file URI", 5))
}
impl Frozen {
    fn lock_inputs(
        &self,
        files: &BTreeMap<String, Vec<u8>>,
        definitions: &[u8],
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Vec<fs::File>> {
        let config = serde_json::to_vec(&configuration(&self.library))?;
        let mut held = Vec::new();
        for (path, expected) in files
            .iter()
            .map(|(name, bytes)| (self.workspace.join(name), bytes.as_slice()))
            .chain([
                (self.library.join("api.d.lua"), definitions),
                (self.root.join("config.json"), config.as_slice()),
            ])
        {
            control.check()?;
            storage::guard_path(&self.root, &path)?;
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(1);
            }
            let mut file = options.open(path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() != expected.len() as u64 {
                return Err(ToolkitError::new(
                    "luals.unavailable",
                    "Frozen input identity changed",
                    3,
                ));
            }
            let mut actual = Vec::new();
            (&mut file)
                .take(expected.len() as u64 + 1)
                .read_to_end(&mut actual)?;
            if actual != expected {
                return Err(ToolkitError::new(
                    "luals.unavailable",
                    "Frozen input bytes changed",
                    3,
                ));
            }
            held.push(file);
        }
        Ok(held)
    }
    fn verify_members(
        &self,
        files: &BTreeMap<String, usize>,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<()> {
        for (root, expected) in [
            (
                &self.workspace,
                files
                    .keys()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>(),
            ),
            (
                &self.library,
                std::collections::BTreeSet::from(["api.d.lua".to_string()]),
            ),
        ] {
            let mut pending = vec![root.clone()];
            let mut seen = std::collections::BTreeSet::new();
            let mut entries = 0usize;
            while let Some(dir) = pending.pop() {
                control.check()?;
                storage::guard_path(&self.root, &dir)?;
                for entry in fs::read_dir(dir)? {
                    control.check()?;
                    let entry = entry?;
                    entries += 1;
                    if entries > 200_000 {
                        return Err(ToolkitError::new(
                            "luals.input_budget",
                            "Frozen input member budget",
                            3,
                        ));
                    }
                    let path = entry.path();
                    storage::guard_path(&self.root, &path)?;
                    let metadata = entry.metadata()?;
                    if metadata.is_dir() {
                        pending.push(path);
                    } else if metadata.is_file() {
                        let relative = path
                            .strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/");
                        if !expected.contains(&relative) {
                            return Err(ToolkitError::new(
                                "luals.unavailable",
                                "Unexpected frozen input member",
                                3,
                            ));
                        }
                        seen.insert(relative);
                    } else {
                        return Err(ToolkitError::new(
                            "luals.unavailable",
                            "Invalid frozen input member",
                            3,
                        ));
                    }
                }
            }
            if seen != expected {
                return Err(ToolkitError::new(
                    "luals.unavailable",
                    "Frozen input member missing",
                    3,
                ));
            }
        }
        Ok(())
    }
    fn path_from_uri(&self, uri: &str) -> Result<String> {
        let url = url::Url::parse(uri)
            .map_err(|_| ToolkitError::new("luals.check_failed", "Invalid file URI", 5))?;
        let path = url
            .to_file_path()
            .map_err(|_| ToolkitError::new("luals.check_failed", "Invalid local URI", 5))?;
        let path = ordinary_path(fs::canonicalize(path)?);
        for (root, prefix) in [(&self.workspace, ""), (&self.library, "@environment/")] {
            if let Ok(relative) = path.strip_prefix(root) {
                return Ok(format!(
                    "{prefix}{}",
                    relative.to_string_lossy().replace('\\', "/")
                ));
            }
        }
        Err(ToolkitError::new(
            "luals.check_failed",
            "Location outside frozen workspace",
            5,
        ))
    }
}
struct Rpc {
    child: std::process::Child,
    job: lycheedev_windows::JobGuard,
    writer: Option<mpsc::SyncSender<Vec<u8>>>,
    written: mpsc::Receiver<Result<()>>,
    rx: Option<mpsc::Receiver<Result<lsp_frame::Frame>>>,
    threads: Vec<std::thread::JoinHandle<()>>,
    next: u64,
    deadline: Instant,
    control: lycheedev_core::control::OperationControl,
    input_changed: Option<Arc<AtomicBool>>,
}
impl Rpc {
    fn start(cmd: Command, control: lycheedev_core::control::OperationControl) -> Result<Self> {
        Self::start_with_threads(
            cmd,
            control,
            [
                std::thread::Builder::new().name("source-lsp-writer".into()),
                std::thread::Builder::new().name("source-lsp-reader".into()),
                std::thread::Builder::new().name("source-lsp-stderr".into()),
            ],
            |_| Ok(()),
        )
    }
    fn start_with_threads(
        mut cmd: Command,
        control: lycheedev_core::control::OperationControl,
        builders: [std::thread::Builder; 3],
        observe: impl FnOnce(&std::process::Child) -> Result<()>,
    ) -> Result<Self> {
        control.check()?;
        let mut threads = Vec::new();
        threads.try_reserve_exact(3).map_err(|_| {
            ToolkitError::new(
                "luals.input_budget",
                "LSP thread handle allocation budget",
                3,
            )
        })?;
        let debugpath = cmd.get_current_dir().unwrap().join("lsp-stderr.txt");
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut child, job) = lycheedev_windows::guarded_spawn::spawn_suspended_guarded(
            cmd,
            LUALS_COMMIT_BUDGET,
            control.deadline(),
            || control.token().load(Ordering::Acquire),
        )
        .map_err(|failure| {
            let mut error = if matches!(failure.cause.as_str(), "startup.cancelled" | "startup.deadline") {
                control.check().err().unwrap_or_else(|| {
                    let (code, exit) = if failure.cause == "startup.cancelled" {
                        ("command.cancelled", 7)
                    } else {
                        ("command.deadline", 5)
                    };
                    ToolkitError::new(code, &failure.cause, exit)
                })
            } else if failure.stage == "spawn" {
                ToolkitError::new("external.io", &failure.cause, 5)
            } else {
                ToolkitError::new("luals.unavailable", &failure.cause, 3)
            };
            error.details = json!({"resource":"lualsStartup","startupStage":failure.stage,"startupCause":failure.cause,"cleanup":failure.cleanup});
            error
        })?;
        let mut stdin = child.stdin.take().unwrap();
        let out = child.stdout.take().unwrap();
        let mut err = child.stderr.take().unwrap();
        // Backpressure retains raw frames rather than up to 256 expanded JSON
        // trees. One queued frame can coexist with the reader's next frame and
        // the consumer's current frame; each remains under the same 8 MiB cap.
        let (tx, rx) = mpsc::sync_channel(1);
        let (writer, queued) = mpsc::sync_channel::<Vec<u8>>(1);
        let (ack, written) = mpsc::sync_channel(1);
        // Establish cleanup ownership before admitting any pipe thread. Every
        // Builder failure drops this owner, reaps the child and drains only the
        // threads already admitted, including partial reader/writer startup.
        let mut rpc = Self {
            child,
            job,
            writer: Some(writer),
            written,
            rx: Some(rx),
            threads,
            next: 0,
            deadline: control
                .deadline()
                .min(Instant::now() + Duration::from_secs(90)),
            control,
            input_changed: None,
        };
        observe(&rpc.child)?;
        let [writer_builder, reader_builder, stderr_builder] = builders;
        let writer_thread = writer_builder
            .spawn(move || {
                while let Ok(raw) = queued.recv() {
                    let result = (|| -> Result<()> {
                        write!(stdin, "Content-Length: {}\r\n\r\n", raw.len())?;
                        stdin.write_all(&raw)?;
                        stdin.flush()?;
                        Ok(())
                    })();
                    let failed = result.is_err();
                    if ack.try_send(result).is_err() || failed {
                        break;
                    }
                }
            })
            .map_err(|error| thread_admission("lspWriterThread", error))?;
        rpc.threads.push(writer_thread);
        let reader_thread = reader_builder
            .spawn(move || {
                let mut reader = std::io::BufReader::new(out);
                let mut frames = 0;
                let mut bytes = 0usize;
                loop {
                    let value = (|| {
                        let mut size = None;
                        let mut header = 0;
                        loop {
                            let mut line = String::new();
                            let n = (&mut reader).take(8193).read_line(&mut line)?;
                            if n == 0 {
                                return Err(ToolkitError::new(
                                    "luals.check_failed",
                                    "LSP stream ended",
                                    5,
                                ));
                            }
                            header += n;
                            if header > 8192 {
                                return Err(ToolkitError::new(
                                    "luals.input_budget",
                                    "LSP header budget",
                                    3,
                                ));
                            }
                            if line == "\r\n" {
                                break;
                            }
                            if let Some(value) = line.strip_prefix("Content-Length:") {
                                if size.is_some() {
                                    return Err(ToolkitError::new(
                                        "luals.check_failed",
                                        "Duplicate LSP content length",
                                        5,
                                    ));
                                }
                                size = Some(value.trim().parse::<usize>().map_err(|_| {
                                    ToolkitError::new("luals.check_failed", "Invalid LSP length", 5)
                                })?);
                            }
                        }
                        let size = size.ok_or_else(|| {
                            ToolkitError::new("luals.check_failed", "Missing LSP length", 5)
                        })?;
                        bytes = bytes.saturating_add(size);
                        if size > 8 << 20 || frames >= 10000 || bytes > 64 << 20 {
                            return Err(ToolkitError::new(
                                "luals.input_budget",
                                "LSP frame budget",
                                3,
                            ));
                        }
                        let mut raw = Vec::new();
                        raw.try_reserve_exact(size).map_err(|_| {
                            ToolkitError::new(
                                "luals.input_budget",
                                "LSP frame allocation budget",
                                3,
                            )
                        })?;
                        raw.resize(size, 0);
                        reader.read_exact(&mut raw)?;
                        let frame = lsp_frame::Frame::parse(raw)?;
                        if frame.ignored() {
                            Ok(None)
                        } else {
                            Ok(Some(frame))
                        }
                    })();
                    frames += 1;
                    let value = match value {
                        Ok(None) => continue,
                        Ok(Some(frame)) => Ok(frame),
                        Err(error) => Err(error),
                    };
                    let bad = value.is_err();
                    // A full queue waits; it never discards a legitimate response.
                    // Rpc::drop disconnects this receiver before joining us.
                    if tx.send(value).is_err() || bad {
                        break;
                    }
                }
            })
            .map_err(|error| thread_admission("lspReaderThread", error))?;
        rpc.threads.push(reader_thread);
        let stderr_thread = stderr_builder
            .spawn(move || {
                let mut buffer = [0u8; 4096];
                let mut captured = Vec::new();
                while let Ok(n) = err.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    let room = 32768usize.saturating_sub(captured.len());
                    captured.extend_from_slice(&buffer[..n.min(room)]);
                }
                let _ = fs::write(debugpath, captured);
            })
            .map_err(|error| thread_admission("lspStderrThread", error))?;
        rpc.threads.push(stderr_thread);
        Ok(rpc)
    }
    fn check_control(&self) -> Result<()> {
        if self
            .input_changed
            .as_ref()
            .is_some_and(|v| v.load(Ordering::Acquire))
        {
            return Err(ToolkitError::new(
                "luals.unavailable",
                "Sealed session inputs changed",
                3,
            ));
        }
        self.control.check()
    }
    fn send(&mut self, value: Value) -> Result<()> {
        self.check_control()?;
        let raw = lsp_frame::serialize(&value, lsp_frame::FRAME_BYTES)?;
        self.send_raw(raw)
    }
    fn send_raw(&mut self, raw: Vec<u8>) -> Result<()> {
        self.check_control()?;
        self.writer.as_ref().unwrap().try_send(raw).map_err(|_| {
            ToolkitError::new(
                "luals.check_failed",
                "LSP writer stopped or its bounded queue is occupied",
                5,
            )
        })?;
        loop {
            self.check_control()?;
            if Instant::now() >= self.deadline {
                return Err(ToolkitError::new(
                    "luals.check_failed",
                    "LSP writer deadline exceeded",
                    5,
                ));
            }
            match self.written.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    return Err(ToolkitError::new(
                        "luals.check_failed",
                        "LSP writer stopped",
                        5,
                    ));
                }
            }
        }
    }
    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}))
    }
    fn request(&mut self, method: &str, params: Value) -> Result<lsp_frame::Frame> {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        loop {
            self.check_control()?;
            if Instant::now() >= self.deadline {
                return Err(ToolkitError::new(
                    "luals.check_failed",
                    "LSP execution deadline",
                    5,
                ));
            }
            let frame = match self
                .rx
                .as_ref()
                .unwrap()
                .recv_timeout(Duration::from_millis(20))
            {
                Ok(value) => value?,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => {
                    return Err(ToolkitError::new(
                        "luals.check_failed",
                        "LSP stream stopped",
                        5,
                    ));
                }
            };
            if let Some(method) = frame
                .field("method")
                .filter(|raw| lsp_frame::is_string(raw))
            {
                if let Some(server_id) = frame.field("id") {
                    let result = if lsp_frame::string_is(method, "workspace/configuration") {
                        b"[]".as_slice()
                    } else if [
                        "window/workDoneProgress/create",
                        "client/registerCapability",
                        "client/unregisterCapability",
                    ]
                    .iter()
                    .any(|known| lsp_frame::string_is(method, known))
                    {
                        b"null".as_slice()
                    } else {
                        return Err(ToolkitError::new(
                            "luals.check_failed",
                            "Unexpected server request",
                            5,
                        ));
                    };
                    // Echo the already validated ID without building its DOM.
                    // Ordinary/null IDs retain exactly the same JSON meaning.
                    let prefix = b"{\"jsonrpc\":\"2.0\",\"id\":";
                    let middle = b",\"result\":";
                    let length = prefix
                        .len()
                        .checked_add(server_id.len())
                        .and_then(|n| n.checked_add(middle.len() + result.len() + 1))
                        .filter(|n| *n <= lsp_frame::FRAME_BYTES)
                        .ok_or_else(|| {
                            ToolkitError::new("luals.input_budget", "LSP input frame budget", 3)
                        })?;
                    let mut reply = Vec::new();
                    reply.try_reserve_exact(length).map_err(|_| {
                        ToolkitError::new("luals.input_budget", "LSP reply allocation budget", 3)
                    })?;
                    reply.extend_from_slice(prefix);
                    reply.extend_from_slice(server_id);
                    reply.extend_from_slice(middle);
                    reply.extend_from_slice(result);
                    reply.push(b'}');
                    self.send_raw(reply)?;
                }
                continue;
            }
            if frame.id_matches(id)? {
                if let Some(error) = frame.field("error").filter(|raw| *raw != b"null") {
                    return Err(ToolkitError::new(
                        "luals.check_failed",
                        lsp_frame::error_message(error)?,
                        5,
                    ));
                }
                return Ok(frame);
            }
        }
    }
}
fn thread_admission(resource: &str, cause: std::io::Error) -> ToolkitError {
    let mut error = ToolkitError::new(
        "luals.input_budget",
        "Unable to admit the bounded LSP pipe thread",
        3,
    );
    error.details = json!({"resource":resource,"osCode":cause.raw_os_error()});
    error
}
impl Drop for Rpc {
    fn drop(&mut self) {
        // Release a reader blocked on bounded send before joining it. Killing
        // the child then releases partial-frame reads and blocked pipe writes.
        self.rx.take();
        let _ = self.job.terminate();
        let _ = self.child.wait();
        self.writer.take();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
fn validate_range(range: &Value) -> Result<()> {
    let position = |name: &str| -> Result<(u64, u64)> {
        let p = &range[name];
        let line = p["line"].as_u64().filter(|n| *n <= i32::MAX as u64);
        let column = p["character"].as_u64().filter(|n| *n <= i32::MAX as u64);
        line.zip(column)
            .ok_or_else(|| ToolkitError::new("luals.check_failed", "Invalid LSP location range", 5))
    };
    if position("end")? < position("start")? {
        return Err(ToolkitError::new(
            "luals.check_failed",
            "Invalid LSP location range",
            5,
        ));
    }
    Ok(())
}
#[cfg(test)]
fn decode_hover(raw: &Value) -> Result<(String, bool)> {
    if raw.is_null() {
        return Ok((String::new(), false));
    }
    let contents = raw
        .get("contents")
        .ok_or_else(|| ToolkitError::new("luals.check_failed", "Hover lacks contents", 5))?;
    struct Text {
        value: String,
        truncated: bool,
        parts: usize,
    }
    fn visit(value: &Value, output: &mut Text, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(ToolkitError::new(
                "luals.input_budget",
                "Hover nesting budget",
                3,
            ));
        }
        if value.is_null() {
            return Ok(());
        }
        let plain = value.as_str().or_else(|| {
            value
                .get("value")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        });
        if let Some(plain) = plain {
            if !plain.is_empty() {
                if output.parts > 0 {
                    append(output, "\n");
                }
                output.parts += 1;
                append(output, plain);
            }
        } else if let Some(list) = value.as_array() {
            for item in list {
                visit(item, output, depth + 1)?;
            }
        } else {
            return Err(ToolkitError::new(
                "luals.check_failed",
                "Unsupported hover contents",
                5,
            ));
        }
        Ok(())
    }
    fn append(output: &mut Text, text: &str) {
        if output.truncated {
            return;
        }
        let room = 16384usize.saturating_sub(output.value.len());
        if text.len() <= room {
            output.value.push_str(text);
            return;
        }
        let mut end = room;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        output.value.push_str(&text[..end]);
        output.truncated = true;
    }
    let mut text = Text {
        value: String::new(),
        truncated: false,
        parts: 0,
    };
    visit(contents, &mut text, 0)?;
    Ok((text.value, text.truncated))
}

#[cfg(test)]
mod rpc_lifecycle_tests {
    use super::*;
    use lycheedev_core::control::OperationControl;
    const PEER: &str = r#"
const fs=require('fs'), mode=process.argv[1], ready=process.argv[2];
const frame=v=>{const b=Buffer.from(typeof v==='string'?v:JSON.stringify(v));return Buffer.concat([Buffer.from('Content-Length: '+b.length+'\r\n\r\n'),b]);};
const mark=()=>fs.writeFileSync(ready,mode);
setInterval(()=>{},1000);
if(mode==='full'){
 const raw=frame({id:77,result:'x'.repeat(1<<20)});
 process.stdout.write(raw,()=>process.stdout.write(raw,mark));
}else if(mode==='partial'){
 process.stdout.write('Content-Length: 1048576\r\n\r\n{"id":1',mark);
}else if(mode==='pending'){
 process.stdin.once('data',mark);process.stdin.resume();
}else{
 let input=Buffer.alloc(0), waiting;
 process.stdin.on('data',chunk=>{
  input=Buffer.concat([input,chunk]);
  while(true){const end=input.indexOf('\r\n\r\n');if(end<0)return;
   const size=Number(/Content-Length: (\d+)/i.exec(input.subarray(0,end).toString())[1]);
   if(input.length<end+4+size)return;
   const msg=JSON.parse(input.subarray(end+4,end+4+size));input=input.subarray(end+4+size);
   if(msg.method==='fixture/query'){
    waiting=msg.id;
    for(let i=0;i<300;i++)process.stdout.write(frame({method:'$/progress',params:{items:Array(32).fill({a:[1,true,null]})}}));
    process.stdout.write(frame(String.raw`{"\u0069d":null,"method":"ignored-duplicate","method":"window/workDoneProgress/create"}`));
   }else if(msg.id===null&&Object.hasOwn(msg,'result')){
    process.stdout.write(frame({id:waiting,result:{value:'answer',items:[1,2]}}));mark();
   }
  }
 });
}
"#;

    fn start(mode: &str) -> (Rpc, PathBuf, lycheedev_windows::Process) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../.tmp/rust-lsp-lifecycle-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            mode,
        ));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let ready = root.join("ready");
        let mut cmd =
            Command::new(std::env::var_os("LYCHEEDEV_TEST_NODE").unwrap_or_else(|| "node".into()));
        cmd.args(["-e", PEER, mode]).arg(&ready).current_dir(&root);
        let control = OperationControl::new(
            Instant::now() + Duration::from_secs(8),
            Arc::new(AtomicBool::new(false)),
        );
        let rpc = Rpc::start(cmd, control).unwrap();
        let process = lycheedev_windows::Process::open(rpc.child.id()).unwrap();
        (rpc, ready, process)
    }
    fn ready(path: &Path) {
        // Fixture-only barrier: the child marks after its pipe write callback
        // (or receipt of the pending request). No product polling is added.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "fixture did not reach its declared pipe boundary"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn close(rpc: Rpc, process: lycheedev_windows::Process) {
        let start = Instant::now();
        drop(rpc);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "pipe/queue thread did not join promptly"
        );
        assert_eq!(
            process
                .wait_exit(
                    Instant::now() + Duration::from_secs(1),
                    &AtomicBool::new(false)
                )
                .unwrap(),
            7
        );
    }
    #[test]
    fn full_raw_queue_close_reaps_blocked_sender_and_child() {
        let (rpc, path, process) = start("full");
        ready(&path);
        close(rpc, process);
    }
    #[test]
    fn partial_frame_close_reaps_blocked_reader_and_child() {
        let (rpc, path, process) = start("partial");
        ready(&path);
        close(rpc, process);
    }
    #[test]
    fn pending_response_cancellation_reaps_all_owned_io() {
        let (mut rpc, path, process) = start("pending");
        let cancel = rpc.control.cancel_handle();
        let trigger = std::thread::spawn(move || {
            ready(&path);
            cancel.store(true, Ordering::Release);
        });
        let error = rpc.request("fixture/query", json!({})).unwrap_err();
        trigger.join().unwrap();
        assert_eq!(error.code, "command.cancelled");
        close(rpc, process);
    }
    #[test]
    fn notification_burst_preserves_null_id_server_request_and_response() {
        let (mut rpc, path, process) = start("legitimate");
        let reply = rpc.request("fixture/query", json!({})).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(reply.result()).unwrap(),
            json!({"value":"answer","items":[1,2]})
        );
        drop(reply);
        ready(&path);
        close(rpc, process);
    }
}

#[cfg(test)]
mod response_tests {
    use super::*;
    #[test]
    fn hover_decodes_marked_arrays_and_bounds_utf8_without_json_dump() {
        assert_eq!(
            decode_hover(&json!({"contents":["first",{"language":"lua","value":"second"},""]}))
                .unwrap(),
            ("first\nsecond".into(), false)
        );
        let (text, cut) =
            decode_hover(&json!({"contents":format!("{}中尾", "a".repeat(16383))})).unwrap();
        assert_eq!(text.len(), 16383);
        assert!(cut);
        assert!(decode_hover(&json!({"contents":{"unexpected":"field"}})).is_err());
        assert!(decode_hover(&json!({})).is_err());
        assert_eq!(decode_hover(&Value::Null).unwrap(), (String::new(), false));
    }
    #[test]
    fn navigation_ranges_are_nonnegative_and_ordered() {
        for range in [
            json!({"start":{"line":2,"character":3},"end":{"line":1,"character":9}}),
            json!({"start":{"line":2,"character":3},"end":{"line":2,"character":2}}),
            json!({"start":{"line":-1,"character":0},"end":{"line":2,"character":0}}),
            Value::Null,
        ] {
            assert_eq!(
                validate_range(&range).unwrap_err().code,
                "luals.check_failed"
            );
        }
        validate_range(&json!({"start":{"line":2,"character":3},"end":{"line":3,"character":0}}))
            .unwrap();
    }
}

#[cfg(all(test, windows))]
#[path = "rpc_budget_tests.rs"]
mod rpc_budget_tests;
#[cfg(all(test, windows))]
#[path = "rpc_document_lifecycle_tests.rs"]
mod rpc_document_lifecycle_tests;
#[cfg(all(test, windows))]
#[path = "rpc_native_quota_tests.rs"]
mod rpc_native_quota_tests;

#[cfg(all(test, windows))]
mod runtime_lock_tests {
    use super::*;
    #[test]
    fn verified_inventory_rejects_runtime_mutation_until_handles_are_released() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../.tmp/rust-luals-hold-{}", std::process::id()));
        fs::create_dir_all(root.join("bin")).unwrap();
        let mut inventory = BTreeMap::new();
        for name in [
            "bin/lua-language-server.exe",
            "bin/main.lua",
            "main.lua",
            "LICENSE",
        ] {
            let data = format!("authenticated {name}").into_bytes();
            fs::write(root.join(name), &data).unwrap();
            inventory.insert(name.into(), storage::sha256(&data));
        }
        let runtime = Runtime {
            root: root.clone(),
            inventory,
        };
        let held = runtime.lock_verified(&Default::default()).unwrap();
        for name in runtime.inventory.keys() {
            assert!(
                fs::OpenOptions::new()
                    .write(true)
                    .open(root.join(name))
                    .is_err(),
                "write {name}"
            );
            assert!(fs::remove_file(root.join(name)).is_err(), "delete {name}");
        }
        drop(held);
        fs::write(root.join("bin/main.lua"), b"modified").unwrap();
        assert_eq!(runtime.verify().unwrap_err().code, "luals.unavailable");
        fs::remove_dir_all(root).unwrap();
    }
}
