//! Rust owns channel selection, pinned npm preparation, prefix replacement and payload recovery.
//! The launcher only starts the verified helper after the old mapped native process exits.
use crate::{
    release,
    update::{self, UpdateTarget},
};
use lycheedev_core::{
    Outcome, Result, ToolkitError, request::RequestContext,
    control::{OperationControl, TrackedCancellation},
    process,
    storage::{self, ExclusiveLock},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Handoff {
    schema: String,
    prefix: PathBuf,
    installed: PathBuf,
    staged: PathBuf,
    node: PathBuf,
    npm: PathBuf,
    tarball: PathBuf,
    integrity: String,
    version: String,
    targets: Vec<UpdateTarget>,
    home: PathBuf,
    phase: String,
    nonce: String,
    helper_sha256: String,
    format: String,
    deadline_unix_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<Outcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure: Option<ToolkitError>,
}
#[derive(Deserialize)]
struct Packed {
    name: String,
    version: String,
    filename: String,
    integrity: String,
}
fn fault(code: &str, message: impl Into<String>, exit: i32) -> ToolkitError {
    ToolkitError::new(code, message, exit)
}
fn ordinary(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix("\\\\?\\UNC\\") {
        PathBuf::from(format!("\\\\{unc}"))
    } else if let Some(local) = text.strip_prefix("\\\\?\\") {
        PathBuf::from(local)
    } else {
        path
    }
}
fn read(path: &Path, max: u64) -> Result<Value> {
    if !fs::metadata(path)?.is_file() || fs::metadata(path)?.len() > max {
        return Err(fault(
            "update.invalid_state",
            "update state file admission differs",
            4,
        ));
    }
    storage::read_json(path)
}
// Release channels are explicitly version-bound; historical fixtures stay on next.
pub(crate) fn release_channel(version: &str) -> &'static str {
    if version == "3.5.0" { "beta" } else { "next" }
}
/// Match the published channel manifest contract. preserveLatest records the
/// publisher's protected stable tag; runtime admission does not pin its value.
pub(crate) fn valid_channel_binding(channel: &Value, version: &str) -> bool {
    let Some(fields) = channel.as_object() else {
        return false;
    };
    let (Some(bound_version), Some(preserved)) = (
        fields.get("version").and_then(Value::as_str),
        fields.get("preserveLatest").and_then(Value::as_str),
    ) else {
        return false;
    };
    let stable = |value: &str| {
        let mut parts = value.split('.');
        (0..3).all(|_| {
            parts.next().is_some_and(|part| {
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
            })
        }) && parts.next().is_none()
    };
    fields.len() == 4
        && fields.get("schema").and_then(Value::as_str) == Some("lycheedev.npm-channel.v1")
        && fields.get("distTag").and_then(Value::as_str) == Some(release_channel(bound_version))
        && bound_version == version
        && stable(bound_version)
        && stable(preserved)
        && preserved != bound_version
}
pub fn binding(root: &Path, version: &str) -> Result<()> {
    let manifest = read(&root.join("release.json"), 1 << 20)?;
    if manifest["runtime"] != "rust"
        || !valid_channel_binding(&manifest["npmChannel"], version)
    {
        return Err(fault(
            "update.runtime_incompatible",
            "self-update requires the version-bound Rust release channel",
            3,
        ));
    }
    Ok(())
}
fn npm_entry(node: &Path, search_paths: &[PathBuf]) -> Result<PathBuf> {
    let mut directories = vec![
        node.parent()
            .ok_or_else(|| fault("update.npm_missing", "Node parent missing", 3))?
            .to_path_buf(),
    ];
    directories.extend_from_slice(search_paths);
    for directory in directories {
        let path = directory.join("node_modules/npm/bin/npm-cli.js");
        if path.is_file() {
            return Ok(ordinary(path.canonicalize()?));
        }
    }
    Err(fault(
        "update.npm_missing",
        "npm-cli.js was not found beside the launcher Node or a PATH entry",
        3,
    ))
}
fn call(node: &Path, npm: &Path, args: &[&str], control: &OperationControl) -> Result<Vec<u8>> {
    let mut command = Command::new(node);
    command.arg(npm).args(args);
    let output = process::run(command, None, 16 << 20, 4 << 20, control)?;
    if output.status != 0 {
        return Err(fault(
            "update.npm_failed",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .rev()
                .take(4000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>(),
            5,
        ));
    }
    Ok(output.stdout)
}
fn native(binary: &Path, args: &[String], control: &OperationControl) -> Result<Value> {
    let mut command = Command::new(binary);
    command.args(args);
    let output = process::run(command, None, 16 << 20, 4 << 20, control)?;
    let envelope: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        fault(
            "update.invalid_native_result",
            "staged Rust result is not a legal envelope",
            5,
        )
    })?;
    if output.status != 0 || envelope["ok"] != true {
        return Err(fault(
            envelope["error"]["code"]
                .as_str()
                .unwrap_or("update.native_failed"),
            envelope["error"]["message"]
                .as_str()
                .unwrap_or("staged Rust command failed"),
            if output.status == 0 { 5 } else { output.status },
        )
        .with_details(json!({"result":envelope["result"]})));
    }
    Ok(envelope)
}
fn version(value: &str) -> bool {
    let mut parts = value.splitn(2, '-');
    let Some(base) = parts.next() else {
        return false;
    };
    let numbers: Vec<_> = base.split('.').collect();
    numbers.len() == 3
        && numbers
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && parts.next().is_none_or(|suffix| {
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        })
}
fn verify_tarball(path: &Path, integrity: &str) -> Result<()> {
    if fs::metadata(path)?.len() > 512 << 20 {
        return Err(fault(
            "update.package_size",
            "pinned package exceeds 512 MiB",
            3,
        ));
    }
    if storage::sha512_integrity(&fs::read(path)?) != integrity {
        return Err(fault(
            "update.package_integrity",
            "pinned npm tarball SRI differs",
            4,
        ));
    }
    Ok(())
}
fn save(path: &Path, record: &Handoff) -> Result<()> {
    storage::atomic_json(path, record)
}
fn canonical_home(ctx: &RequestContext) -> Result<PathBuf> {
    let raw = if ctx.home.is_absolute() {
        ctx.home.clone()
    } else {
        ctx.cwd.join(&ctx.home)
    };
    let mut normalized = PathBuf::new();
    for component in raw.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err(fault(
                        "update.invalid_workspace",
                        "workspace escapes its absolute root",
                        2,
                    ));
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    let mut existing = normalized.clone();
    let mut tail = vec![];
    while !existing.exists() {
        tail.push(
            existing
                .file_name()
                .ok_or_else(|| {
                    fault(
                        "update.invalid_workspace",
                        "workspace has no existing ancestor",
                        2,
                    )
                })?
                .to_os_string(),
        );
        existing.pop();
    }
    storage::guard_path(&existing, &existing)?;
    let mut actual = ordinary(existing.canonicalize()?);
    for name in tail.into_iter().rev() {
        actual.push(name);
    }
    Ok(actual)
}
fn millis() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            fault(
                "update.clock_invalid",
                "system clock precedes Unix epoch",
                5,
            )
        })?
        .as_millis() as u64)
}
fn archive_completed(directory: &Path, journal: &Path, record: &Handoff) -> Result<()> {
    let raw = fs::read(journal)?;
    let history = directory.join("history").join(storage::sha256(&raw));
    storage::guard_path(directory, &history)?;
    fs::create_dir_all(&history)?;
    for (source, name) in [
        (&record.tarball, "archive.tgz"),
        (&directory.join("staging"), "staging"),
    ] {
        if source.exists() {
            storage::guard_path(directory, source)?;
            let destination = history.join(name);
            if destination.exists() {
                return Err(fault(
                    "update.pending_conflict",
                    "completed update history destination already exists",
                    6,
                ));
            }
            if source == &record.tarball {
                verify_tarball(source, &record.integrity)?;
            }
            fs::rename(source, destination)?;
        }
    }
    let destination = history.join("transaction.json");
    if destination.exists() {
        if fs::read(&destination)? != raw {
            return Err(fault(
                "update.invalid_state",
                "completed journal history differs",
                4,
            ));
        }
        fs::remove_file(journal)?;
    } else {
        fs::rename(journal, destination)?;
    }
    Ok(())
}
pub fn prepare(
    ctx: &RequestContext,
    presentation: &str,
    installed: &Path,
    node: &Path,
    nonce: &str,
    npm_search_paths: &[PathBuf],
    targets: Vec<UpdateTarget>,
) -> Result<Outcome> {
    if !lycheedev_core::selection::valid_digest(nonce, 64) {
        return Err(fault(
            "update.invalid_handoff",
            "launcher nonce shape differs",
            4,
        ));
    }
    let installed = ordinary(installed.canonicalize()?);
    let workspace = canonical_home(ctx)?;
    let prefix = installed
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            fault(
                "update.global_install_required",
                "global npm prefix missing",
                3,
            )
        })?
        .to_path_buf();
    if installed != prefix.join("node_modules/lycheedev") || !prefix.join("lycheedev.cmd").is_file()
    {
        return Err(fault(
            "update.global_install_required",
            "self-update requires an npm global prefix; archives use --release",
            3,
        ));
    }
    let package = read(&installed.join("package.json"), 1 << 20)?;
    let current = package["version"]
        .as_str()
        .ok_or_else(|| fault("update.invalid_package", "installed version missing", 4))?;
    binding(&installed, current)?;
    release::inspect(&installed, current)?;
    let directory = prefix.join(".lycheedev-update");
    storage::guard_path(&prefix, &directory)?;
    fs::create_dir_all(&directory)?;
    let _lock = ExclusiveLock::resource(&directory, "npm-prefix-update")?;
    let journal = directory.join("transaction.json");
    if journal.exists() {
        let record: Handoff = storage::read_json(&journal)?;
        if record.schema != "lycheedev.npm-update.rust.v1" || record.prefix != prefix {
            return Err(fault(
                "update.invalid_state",
                "existing npm transaction identity differs",
                4,
            ));
        }
        if record.phase == "complete" || record.phase == "rejected" {
            if record.phase == "rejected"
                && record
                    .failure
                    .as_ref()
                    .is_none_or(|failure| failure.code != "update.runtime_incompatible")
            {
                return Err(fault(
                    "update.invalid_state",
                    "terminal rejection has no exact incompatibility receipt",
                    4,
                ));
            }
            archive_completed(&directory, &journal, &record)?;
        }
    }
    let npm = npm_entry(node, npm_search_paths)?;
    let mut record = if journal.exists() {
        let record: Handoff = storage::read_json(&journal)?;
        if record.schema != "lycheedev.npm-update.rust.v1"
            || record.prefix != prefix
            || record.home != workspace
            || serde_json::to_value(&record.targets)? != serde_json::to_value(&targets)?
        {
            return Err(fault(
                "update.pending_conflict",
                "retry the original frozen targets and workspace before changing update selection",
                6,
            ));
        }
        record
    } else {
        let package = format!("lycheedev@{}", release_channel(env!("CARGO_PKG_VERSION")));
        let bytes = call(
            node,
            &npm,
            &[
                "pack",
                &package,
                "--ignore-scripts",
                "--json",
                "--pack-destination",
                directory
                    .to_str()
                    .ok_or_else(|| fault("update.invalid_path", "prefix path is not UTF-8", 4))?,
            ],
            &ctx.control,
        )?;
        let mut packages: Vec<Packed> = serde_json::from_slice(&bytes)?;
        if packages.len() != 1 {
            return Err(fault(
                "update.invalid_package",
                "npm pack returned multiple packages",
                4,
            ));
        }
        let packed = packages.remove(0);
        if packed.name != "lycheedev"
            || !version(&packed.version)
            || packed.filename != format!("lycheedev-{}.tgz", packed.version)
        {
            return Err(fault(
                "update.invalid_package",
                "pinned npm package identity differs",
                4,
            ));
        }
        let record = Handoff {
            schema: "lycheedev.npm-update.rust.v1".into(),
            prefix: prefix.clone(),
            installed: installed.clone(),
            staged: directory.join("staging/node_modules/lycheedev"),
            node: ordinary(node.canonicalize()?),
            npm,
            tarball: directory.join(packed.filename),
            integrity: packed.integrity,
            version: packed.version,
            targets,
            home: workspace,
            phase: "packed".into(),
            nonce: nonce.into(),
            helper_sha256: String::new(),
            format: presentation.into(),
            deadline_unix_millis: millis()?
                .saturating_add(ctx.control.remaining()?.as_millis() as u64),
            result: None,
            failure: None,
        };
        save(&journal, &record)?;
        record
    };
    storage::guard_path(&directory, &record.tarball)?;
    verify_tarball(&record.tarball, &record.integrity)?;
    if !["packed", "handoff", "prefix"].contains(&record.phase.as_str()) {
        return Err(fault(
            "update.invalid_state",
            "pending npm phase is unknown",
            4,
        ));
    }
    if record.phase == "packed" {
        call(
            &record.node,
            &record.npm,
            &[
                "install",
                "--prefix",
                directory.join("staging").to_str().unwrap(),
                "--ignore-scripts",
                "--no-audit",
                "--no-fund",
                "--package-lock=false",
                record.tarball.to_str().unwrap(),
            ],
            &ctx.control,
        )?;
    }
    let staged_package = read(&record.staged.join("package.json"), 1 << 20)?;
    if staged_package["name"] != "lycheedev" || staged_package["version"] != record.version {
        return Err(fault(
            "update.version_mismatch",
            "staged package identity differs",
            4,
        ));
    }
    if let Err(error) = binding(&record.staged, &record.version) {
        if error.code == "update.runtime_incompatible" {
            record.phase = "rejected".into();
            record.failure = Some(error.clone());
            save(&journal, &record)?;
        }
        return Err(error);
    }
    let release = release::inspect(&record.staged, &record.version)?;
    let helper = record.staged.join("native/windows-amd64/lycheedev.exe");
    record.helper_sha256 = release.binaries["windows-amd64"].sha256.clone();
    let targets = directory.join("targets.json");
    storage::atomic_json(&targets, &record.targets)?;
    if record.phase == "packed" {
        native(
            &helper,
            &[
                "update".into(),
                "--file".into(),
                targets.to_string_lossy().into(),
                "--release".into(),
                record.staged.to_string_lossy().into(),
                "--home".into(),
                record.home.to_string_lossy().into(),
                "--plan".into(),
                "--format=json".into(),
            ],
            &ctx.control,
        )?;
        record.phase = "handoff".into();
    }
    record.nonce = nonce.into();
    save(&journal, &record)?;
    let handoff = directory.join(format!("handoff-{nonce}.json"));
    let mut intent: Handoff = serde_json::from_value(serde_json::to_value(&record)?)?;
    intent.phase = "handoff".into();
    save(&handoff, &intent)?;
    Err(fault("update.handoff_ready","prepared verified Rust helper; the launcher must exit the old native process before prefix replacement",6).with_details(json!({"result":{"complete":false,"handoff":handoff,"helper":helper,"version":record.version}})))
}
pub fn apply_handoff(path: &Path, nonce: &str, control: &OperationControl) -> Result<Outcome> {
    let resolved = ordinary(path.canonicalize()?);
    let path = resolved.as_path();
    let mut record: Handoff = storage::read_json(path)?;
    let directory = record.prefix.join(".lycheedev-update");
    storage::guard_path(&record.prefix, &directory)?;
    storage::guard_path(&directory, path)?;
    if record.schema != "lycheedev.npm-update.rust.v1"
        || record.nonce != nonce
        || path != directory.join(format!("handoff-{nonce}.json"))
        || record.phase != "handoff"
        || record.installed != record.prefix.join("node_modules/lycheedev")
        || record.staged != directory.join("staging/node_modules/lycheedev")
    {
        return Err(fault(
            "update.invalid_handoff",
            "helper intent identity differs",
            4,
        ));
    }
    let own = ordinary(std::env::current_exe()?.canonicalize()?);
    let helper = ordinary(
        record
            .staged
            .join("native/windows-amd64/lycheedev.exe")
            .canonicalize()?,
    );
    if own != helper || storage::sha256(&fs::read(&helper)?) != record.helper_sha256 {
        return Err(fault(
            "update.invalid_handoff",
            "this process is not the exact verified staged helper",
            4,
        ));
    }
    binding(&record.staged, &record.version)?;
    release::inspect(&record.staged, &record.version)?;
    let _lock = ExclusiveLock::resource(&directory, "npm-prefix-update")?;
    let journal = directory.join("transaction.json");
    let current: Handoff = storage::read_json(&journal)?;
    let identity = |record: &Handoff| -> Result<Value> {
        let mut value = serde_json::to_value(record)?;
        value.as_object_mut().unwrap().remove("phase");
        value.as_object_mut().unwrap().remove("result");
        value.as_object_mut().unwrap().remove("failure");
        Ok(value)
    };
    if identity(&current)? != identity(&record)?
        || !["handoff", "prefix", "complete"].contains(&current.phase.as_str())
    {
        return Err(fault(
            "update.invalid_handoff",
            "active journal does not own this exact helper intent",
            4,
        ));
    }
    record = current;
    if record.phase == "complete" {
        binding(&record.installed, &record.version)?;
        release::inspect(&record.installed, &record.version)?;
        return record.result.ok_or_else(|| {
            fault(
                "update.invalid_state",
                "completed journal has no durable result",
                4,
            )
        });
    }
    control.check()?;
    storage::guard_path(&directory, &record.tarball)?;
    verify_tarball(&record.tarball, &record.integrity)?;
    if record.phase == "handoff" {
        call(
            &record.node,
            &record.npm,
            &[
                "install",
                "--global",
                "--prefix",
                record.prefix.to_str().unwrap(),
                "--ignore-scripts",
                "--no-audit",
                "--no-fund",
                record.tarball.to_str().unwrap(),
            ],
            control,
        )?;
        record.phase = "prefix".into();
        save(&journal, &record)?;
    }
    binding(&record.installed, &record.version)?;
    release::inspect(&record.installed, &record.version)?;
    let state = {
        let mut name = record.home.as_os_str().to_os_string();
        name.push(".delivery");
        PathBuf::from(name)
    };
    let mut result = update::managed_with_control(
        &record.installed,
        &record.version,
        &state,
        &record.targets,
        false,
        control,
    )?;
    result.data["cli"] = json!({"version":record.version,"state":"current","prefix":record.prefix});
    result.warnings.push(json!("Exact update artifacts remain for recovery; the mapped helper staging tree is reclaimed only after its process exits."));
    record.phase = "complete".into();
    record.result = Some(result.clone());
    save(&journal, &record)?;
    Ok(result)
}
pub fn helper_control(path: &Path, cancel: TrackedCancellation) -> Result<OperationControl> {
    let record: Handoff = storage::read_json(path)?;
    let remaining = record
        .deadline_unix_millis
        .saturating_sub(millis()?)
        .min(600000);
    Ok(OperationControl::tracked(
        Instant::now() + Duration::from_millis(remaining),
        cancel,
    ))
}
pub fn helper_format(path: &Path) -> Result<String> {
    let record: Handoff = storage::read_json(path)?;
    if !["text", "json", "jsonl"].contains(&record.format.as_str()) {
        return Err(fault(
            "update.invalid_handoff",
            "update output format differs",
            4,
        ));
    }
    Ok(record.format)
}
#[cfg(test)]
mod tests {
    use super::*;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Temp {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-npm-life-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Temp(root)
    }
    fn record(root: &Path, version: &str) -> Handoff {
        Handoff {
            schema: "lycheedev.npm-update.rust.v1".into(),
            prefix: root.into(),
            installed: root.join("node_modules/lycheedev"),
            staged: root.join(".lycheedev-update/staging/node_modules/lycheedev"),
            node: PathBuf::from("node.exe"),
            npm: PathBuf::from("npm-cli.js"),
            tarball: root.join(".lycheedev-update/archive.tgz"),
            integrity: storage::sha512_integrity(b"archive"),
            version: version.into(),
            targets: vec![],
            home: root.join("workspace"),
            phase: "complete".into(),
            nonce: "1".repeat(64),
            helper_sha256: "2".repeat(64),
            format: "json".into(),
            deadline_unix_millis: millis().unwrap() + 1000,
            result: None,
            failure: None,
        }
    }
    #[test]
    fn channel_binding_refuses_unbound_or_latest_version_before_mutation() {
        let temp = fixture();
        let channel = json!({"schema":"lycheedev.npm-channel.v1","version":"3.1.2","distTag":"next","preserveLatest":"2.5.1"});
        // Negative evidence: a historical Go runtime record is not a current
        // runtime and must be refused before any update mutation.
        storage::atomic_json(
            &temp.0.join("release.json"),
            &json!({"runtime":"go","npmChannel":channel}),
        )
        .unwrap();
        assert_eq!(
            binding(&temp.0, "3.1.2").unwrap_err().code,
            "update.runtime_incompatible"
        );
        storage::atomic_json(
            &temp.0.join("release.json"),
            &json!({"runtime":"rust","npmChannel":channel}),
        )
        .unwrap();
        assert!(binding(&temp.0, "3.1.2").is_ok());
        assert!(binding(&temp.0, "3.1.3").is_err());
        let mut latest = channel;
        latest["distTag"] = json!("latest");
        storage::atomic_json(
            &temp.0.join("release.json"),
            &json!({"runtime":"rust","npmChannel":latest}),
        )
        .unwrap();
        assert_eq!(binding(&temp.0, "3.1.2").unwrap_err().exit_code, 3);
        assert!(!temp.0.join(".lycheedev-update").exists());
    }
    #[test]
    fn independent_rust_350_requires_beta_and_rejects_other_channels() {
        let temp = fixture();
        for tag in ["beta", "next", "latest"] {
            storage::atomic_json(
                &temp.0.join("release.json"),
                &json!({"runtime":"rust","npmChannel":{
                    "schema":"lycheedev.npm-channel.v1","version":"3.5.0",
                    "distTag":tag,"preserveLatest":"2.5.1"
                }}),
            )
            .unwrap();
            assert_eq!(binding(&temp.0, "3.5.0").is_ok(), tag == "beta");
            assert!(binding(&temp.0, "3.2.2").is_err());
        }
        assert!(!temp.0.join(".lycheedev-update").exists());
    }
    #[cfg(windows)]
    #[test]
    fn completed_history_allows_two_new_lifecycles_and_resumes_denied_journal_rename() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = fixture();
        let directory = temp.0.join(".lycheedev-update");
        fs::create_dir_all(&directory).unwrap();
        let journal = directory.join("transaction.json");
        for version in ["3.1.2", "3.1.3"] {
            let record = record(&temp.0, version);
            fs::write(&record.tarball, b"archive").unwrap();
            fs::create_dir_all(directory.join("staging")).unwrap();
            fs::write(directory.join("staging/owned.txt"), version).unwrap();
            save(&journal, &record).unwrap();
            let digest = storage::sha256(&fs::read(&journal).unwrap());
            if version == "3.1.2" {
                let held = fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1)
                    .open(&journal)
                    .unwrap();
                assert_eq!(
                    archive_completed(&directory, &journal, &record)
                        .unwrap_err()
                        .exit_code,
                    5
                );
                assert!(journal.exists());
                drop(held);
            }
            archive_completed(&directory, &journal, &record).unwrap();
            assert!(!journal.exists());
            let history = directory.join("history").join(digest);
            assert_eq!(fs::read(history.join("archive.tgz")).unwrap(), b"archive");
            assert_eq!(
                fs::read_to_string(history.join("staging/owned.txt")).unwrap(),
                version
            );
            assert!(history.join("transaction.json").exists());
        }
        assert_eq!(fs::read_dir(directory.join("history")).unwrap().count(), 2);
    }
    #[test]
    fn workspace_and_helper_deadline_do_not_depend_on_new_cwd_or_restart_the_budget() {
        let temp = fixture();
        let ctx = RequestContext {
            home: "relative/../workspace".into(),
            cwd: temp.0.clone(),
            control: Default::default(),
        };
        assert_eq!(
            canonical_home(&ctx).unwrap(),
            ordinary(temp.0.canonicalize().unwrap()).join("workspace")
        );
        let mut record = record(&temp.0, "3.1.2");
        record.deadline_unix_millis = millis().unwrap().saturating_sub(1);
        let path = temp.0.join("handoff.json");
        save(&path, &record).unwrap();
        let control = helper_control(&path, Default::default()).unwrap();
        assert_eq!(control.check().unwrap_err().code, "command.deadline");
        assert_eq!(helper_format(&path).unwrap(), "json");
    }
}
