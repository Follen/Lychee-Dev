//! Authenticated bundled Lua 5.1 syntax compiler. Only `-p` is used: no
//! bytecode publication, source execution, PATH discovery or installed fallback.
use crate::{git, process};
use lycheedev_core::{Result, ToolkitError, storage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub version: String,
    pub platform: String,
    pub executable: String,
    pub sha256: String,
    pub bytes: u64,
    pub license: String,
    #[serde(rename = "licenseSHA256")]
    pub license_sha256: String,
}
pub struct Runtime {
    root: PathBuf,
    manifest: Manifest,
}
impl Runtime {
    pub fn manifest(&self)->&Manifest { &self.manifest }
    pub fn verified(root: &Path, manifest: Manifest) -> Result<Self> {
        if manifest.schema != "lycheedev.luac-runtime.v1"
            || manifest.version != "5.1.5"
            || manifest.platform != "windows-amd64"
            || !git::source_path(&manifest.executable)
            || !git::source_path(&manifest.license)
            || manifest.bytes == 0
            || manifest.bytes > 16 << 20
            || !lycheedev_core::selection::valid_digest(&manifest.sha256, 64)
            || !lycheedev_core::selection::valid_digest(&manifest.license_sha256, 64)
        {
            return Err(ToolkitError::new(
                "luac.unavailable",
                "Invalid bundled luac identity",
                3,
            ));
        }
        let r = Self {
            root: fs::canonicalize(root)?,
            manifest,
        };
        r.verify()?;
        Ok(r)
    }
    pub fn verify(&self) -> Result<()> {
        self.lock_verified().map(|_| ())
    }
    pub fn verify_with_control(
        &self,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<()> {
        control.check()?;
        let _held = self.lock_verified()?;
        control.check()
    }
    fn lock_verified(&self) -> Result<Vec<fs::File>> {
        let mut held = vec![];
        for (name, expected, limit) in [
            (&self.manifest.executable, &self.manifest.sha256, 16 << 20),
            (
                &self.manifest.license,
                &self.manifest.license_sha256,
                1 << 20,
            ),
        ] {
            let path = storage::safe_relative(&self.root, name)?;
            storage::guard_path(&self.root, &path)?;
            let mut data = vec![];
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(1);
            }
            let mut file = options.open(path)?;
            (&mut file).take(limit + 1).read_to_end(&mut data)?;
            if data.len() as u64 > limit
                || storage::sha256(&data) != *expected
                || (name == &self.manifest.executable && data.len() as u64 != self.manifest.bytes)
            {
                return Err(ToolkitError::new(
                    "luac.unavailable",
                    "Bundled luac content changed",
                    3,
                ));
            }
            held.push(file);
        }
        Ok(held)
    }
    pub fn check(&self, staging: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<Value> {
        self.check_controlled(staging, files, None, None)
    }
    pub fn check_with_cancel(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Value> {
        self.check_controlled(staging, files, Some(cancel), None)
    }
    pub fn check_with_control(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Value> {
        control.check()?;
        self.check_controlled(
            staging,
            files,
            Some(control.token()),
            Some(control.deadline()),
        )
    }
    fn check_controlled(
        &self,
        staging: &Path,
        files: &BTreeMap<String, Vec<u8>>,
        cancel: Option<&std::sync::atomic::AtomicBool>,
        caller_deadline: Option<Instant>,
    ) -> Result<Value> {
        // Keep Windows deny-write/delete handles alive through every child
        // launch and reap; verification cannot authorize a substituted exe.
        let _held_runtime = self.lock_verified()?;
        if files.len() > 2048 {
            return Err(ToolkitError::new(
                "luac.input_budget",
                "Lua compile file budget",
                3,
            ));
        }
        let mut total = 0;
        for (name, data) in files {
            total += data.len();
            if !git::source_path(name)
                || data.len() > 32 << 20
                || total > 32 << 20
                || data.contains(&0)
                || data.starts_with(b"\x1bLua")
                || std::str::from_utf8(data).is_err()
            {
                return Err(ToolkitError::new(
                    "luac.input_budget",
                    "Invalid Lua source admission",
                    3,
                ));
            }
        }
        fs::create_dir_all(staging)?;
        let root = staging.join(format!(
            "luac-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let frozen = Frozen(root);
        let deadline = caller_deadline
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(90))
            .min(Instant::now() + Duration::from_secs(90));
        let mut diagnostics = vec![];
        let mut checked = 0;
        for (name, data) in files {
            let mut cmd = Command::new(self.root.join(&self.manifest.executable));
            cmd.current_dir(&frozen.0).args(["-p", "-"]);
            for key in ["LUA_PATH", "LUA_CPATH", "LUA_INIT", "LUA_INIT_5_1"] {
                cmd.env_remove(key);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ToolkitError::new(
                    "luac.input_budget",
                    "Lua compilation deadline exceeded",
                    3,
                ));
            }
            let out = process::run_controlled(
                cmd,
                32768,
                remaining.min(Duration::from_secs(10)),
                Some(data.clone()),
                cancel,
            )
            .map_err(|mut e| {
                e.code = e.code.replace("luals.", "luac.");
                e
            })?;
            checked += 1;
            if out.status != 0 {
                let raw = String::from_utf8_lossy(&out.stderr).into_owned();
                let marker = "stdin:";
                let Some(after) = raw.find(marker).map(|at| &raw[at + marker.len()..]) else {
                    return Err(ToolkitError::new(
                        "luac.failed",
                        format!("Compiler diagnostic cannot be mapped: {raw}"),
                        5,
                    ));
                };
                let Some((line, message)) = after.split_once(':') else {
                    return Err(ToolkitError::new(
                        "luac.failed",
                        "Compiler omitted error line",
                        5,
                    ));
                };
                let line: usize = line.parse().map_err(|_| {
                    ToolkitError::new("luac.failed", "Invalid compiler error line", 5)
                })?;
                if crate::xml_inline::source_map::lua_line_span_bytes(data,line).is_err() {
                    return Err(ToolkitError::new(
                        "luac.failed",
                        "Compiler line exceeds frozen source",
                        5,
                    ));
                }
                diagnostics.push(json!({"severity":"error","code":"lua_syntax","file":name,"line":line,"column":0,"columnKnown":false,"positionAccuracy":"line","columnMeaning":"unknown-sentinel","message":message.trim(),"rawError":raw,"engine":"luac-5.1.5","contentHash":storage::sha256(data)}));
            }
            if !out.stdout.is_empty() {
                return Err(ToolkitError::new(
                    "luac.failed",
                    "Unexpected syntax compiler output",
                    5,
                ));
            }
        }
        if frozen.0.join("luac.out").exists() {
            return Err(ToolkitError::new(
                "luac.failed",
                "Syntax compiler emitted unexpected bytecode",
                5,
            ));
        }
        Ok(
            json!({"state":"checked","version":"5.1.5","compilerSHA256":self.manifest.sha256,"checkedLua":checked,"passed":diagnostics.is_empty(),"diagnostics":diagnostics,"execution":"syntax-only -p","complete":true}),
        )
    }
}
struct Frozen(PathBuf);
impl Drop for Frozen {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
