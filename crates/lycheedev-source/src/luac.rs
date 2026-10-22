//! Authenticated bundled Lua 5.1 syntax compiler. Only `-p` is used: no
//! bytecode publication, source execution, PATH discovery or installed fallback.
use crate::{git, process};
use lycheedev_core::{Result, ToolkitError, storage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
mod retention;
pub use retention::{requires_scratch_retention, retain_scratch};
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
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn verified(root: &Path, manifest: Manifest) -> Result<Self> {
        Self::verified_with_ordered_control(root, manifest, &Default::default())
    }
    pub fn verified_with_ordered_control(
        root: &Path,
        manifest: Manifest,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Self> {
        control.ordered_child(control.deadline())?;
        control.check()?;
        let runtime = Self::admit(root, manifest)?;
        let files = runtime.lock_verified_ordered(control)?;
        finish_runtime(files, Ok(runtime), control)
    }
    fn admit(root: &Path, manifest: Manifest) -> Result<Self> {
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
        Ok(Self {
            root: fs::canonicalize(root)?,
            manifest,
        })
    }
    pub fn verify(&self) -> Result<()> { self.verify_with_control(&Default::default()) }
    pub fn verify_with_control(&self, control: &lycheedev_core::control::OperationControl) -> Result<()> {
        let held = self.lock_verified_ordered(control)?;
        finish_runtime(held, Ok(()), control)
    }
    pub fn check(&self, staging: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<Value> {
        self.check_with_control(staging, files, &Default::default())
    }
    pub fn check_with_control(&self, staging: &Path, files: &BTreeMap<String, Vec<u8>>, control: &lycheedev_core::control::OperationControl) -> Result<Value> {
        control.ordered_child(control.deadline())?;
        control.check()?;
        // Keep Windows deny-write/delete handles alive through every child
        // launch and reap; verification cannot authorize a substituted exe.
        let held_runtime = self.lock_verified_ordered(control)?;
        let mut frozen = None;
        let result = (|| {
            control.check()?;
            if files.len() > 2048 {
                return Err(ToolkitError::new(
                    "luac.input_budget",
                    "Lua compile file budget",
                    3,
                ));
            }
            let mut total = 0;
            for (name, data) in files {
                control.check()?;
                total += data.len();
                if !git::source_path(name)
                    || data.len() > 32 << 20
                    || total > 32 << 20
                    || data.contains(&0)
                    || data.starts_with(b"\x1bLua")
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
            frozen = Some(Frozen(root, true));
            let frozen = frozen.as_mut().expect("owned scratch was created");
            let deadline = control.deadline()
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
                control.check()?;
                let out = run_with_frozen(frozen, || process::run_with_operation_control(cmd, 32768, remaining.min(Duration::from_secs(10)), Some(data.clone()), control))
                .map_err(|mut e| {
                    e.code = e.code.replace("source.process_", "luac.");
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
                    if crate::xml_inline::source_map::lua_line_span_bytes(data, line).is_err() {
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
        })();
        let result = match frozen { Some(frozen) => frozen.finish(result), None => result };
        finish_runtime(held_runtime, result, control)
    }
    fn lock_verified_ordered(
        &self,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Vec<fs::File>> {
        let mut held = vec![];
        let result = (|| {
            for (name, expected, limit) in [
                (&self.manifest.executable, &self.manifest.sha256, 16 << 20),
                (
                    &self.manifest.license,
                    &self.manifest.license_sha256,
                    1 << 20,
                ),
            ] {
                control.check()?;
                let path = storage::safe_relative(&self.root, name)?;
                storage::guard_path(&self.root, &path)?;
                let (file, hash) = storage::locked_sha256(&path, limit, control)?;
                // Retain ownership before metadata/identity can fail so every
                // acquired handle is consumed exactly once on all ordinary exits.
                held.push(file);
                let length = held.last().unwrap().metadata()?.len();
                if hash != *expected
                    || (name == &self.manifest.executable && length != self.manifest.bytes)
                {
                    return Err(ToolkitError::new(
                        "luac.unavailable",
                        "Bundled luac content changed",
                        3,
                    ));
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(held),
            Err(error) => finish_runtime(held, Err(error), control),
        }
    }
}
fn finish_runtime<T>(
    files: Vec<fs::File>,
    mut result: Result<T>,
    control: &lycheedev_core::control::OperationControl,
) -> Result<T> {
    for file in files {
        result = storage::finish_file(file, result, control);
    }
    result
}
/// The compiler runner retains the owned scratch boundary. Only a
/// successful output satisfies the runner's terminal-completion contract.
/// Errors remain conservative even when their diagnostic metadata describes
/// completed cleanup; this caller does not independently verify that evidence.
fn run_with_frozen(
    frozen: &mut Frozen,
    run: impl FnOnce() -> Result<process::Output>,
) -> Result<process::Output> {
    // An unwind also leaves the owner disarmed for deletion.
    frozen.1 = false;
    let result = run();
    frozen.1 = result.is_ok();
    result
}
struct Frozen(PathBuf, bool);
impl Frozen {
    fn finish<T>(mut self, result: Result<T>) -> Result<T> {
        let allowed = self.1;
        self.1 = false;
        if !allowed {
            let cause = result.err().unwrap_or_else(|| {
                ToolkitError::new(
                    "luac.process_unresolved",
                    "compiler returned without terminal process proof",
                    5,
                )
            });
            let retained = retain_scratch(cause, &self.0);
            let record = storage::bounded_json(&retained, 1 << 20).and_then(|bytes| {
                storage::atomic_write(&self.0.join("retained-owner.json"), &bytes)
            });
            return match record {
                Ok(()) => Err(retained),
                Err(cleanup) => {
                    let mut error = retained.clone();
                    error.details = json!({"schema":storage::FILE_CLEANUP_SCHEMA,"primary":retained,"cleanup":cleanup,"control":null});
                    Err(error)
                }
            };
        }
        match (
            result,
            fs::remove_dir_all(&self.0).map_err(ToolkitError::io),
        ) {
            (result, Ok(())) => result,
            (Ok(_), Err(cleanup)) => Err(cleanup),
            (Err(primary), Err(cleanup)) => {
                let mut result = primary.clone();
                result.details = json!({"schema":storage::FILE_CLEANUP_SCHEMA,"primary":primary,"cleanup":cleanup,"control":null});
                Err(result)
            }
        }
    }
}
impl Drop for Frozen {
    fn drop(&mut self) {
        if self.1 {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
#[path = "luac/ordered_tests.rs"]
mod ordered_tests;
