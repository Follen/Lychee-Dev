//! Rust owns channel selection, pinned npm preparation, prefix replacement and payload recovery.
//! The launcher only starts the verified helper after the old mapped native process exits.
use crate::{
    deploy, release, slots,
    update::{self, UpdateTarget},
};
use lycheedev_core::{
    Outcome, Result, ToolkitError,
    control::{OperationControl, TrackedCancellation},
    process,
    request::RequestContext,
    storage::{self, ExclusiveLock},
};
use serde::de::DeserializeOwned;
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
fn read_controlled<T: DeserializeOwned>(
    path: &Path,
    max: usize,
    control: &OperationControl,
) -> Result<T> {
    Ok(serde_json::from_slice(&slots::read_bounded_controlled(
        path, max, control,
    )?)?)
}
fn exists_controlled(path: &Path, control: &OperationControl) -> Result<bool> {
    control.check()?;
    let result = match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    };
    control.check()?;
    result
}
fn finish_prefix(result: Result<Outcome>, lock: ExclusiveLock) -> Result<Outcome> {
    let close = lock.close();
    #[cfg(test)]
    let close = tests::observe_prefix_close(close);
    deploy::finish_close(result, close)
}
fn finish_update_file(
    file: fs::File,
    result: Result<Outcome>,
    control: &OperationControl,
) -> Result<Outcome> {
    // File teardown must not discard an already completed prefix/target result.
    deploy::finish_close(result, storage::finish_file(file, Ok(()), control))
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
    binding_controlled(root, version, &OperationControl::default())
}
fn binding_controlled(root: &Path, version: &str, control: &OperationControl) -> Result<()> {
    let manifest: Value = read_controlled(&root.join("release.json"), 1 << 20, control)?;
    if manifest["runtime"] != "rust" || !valid_channel_binding(&manifest["npmChannel"], version) {
        return Err(fault(
            "update.runtime_incompatible",
            "self-update requires the version-bound Rust release channel",
            3,
        ));
    }
    Ok(())
}
fn npm_entry(node: &Path, search_paths: &[PathBuf], control: &OperationControl) -> Result<PathBuf> {
    let mut directories = vec![
        node.parent()
            .ok_or_else(|| fault("update.npm_missing", "Node parent missing", 3))?
            .to_path_buf(),
    ];
    directories.extend_from_slice(search_paths);
    for directory in directories {
        control.check()?;
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
    decode_native_output(output)
}
fn decode_native_output(output: process::Output) -> Result<Value> {
    let envelope: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        fault(
            "update.invalid_native_result",
            "staged Rust result is not a legal envelope",
            5,
        )
    })?;
    if output.status != 0 || envelope["ok"] != true {
        let mut details = envelope["error"]["details"].clone();
        if !envelope["result"].is_null() {
            if let Some(fields) = details.as_object_mut() {
                fields
                    .entry("result")
                    .or_insert_with(|| envelope["result"].clone());
            } else if details.is_null() {
                details = json!({"result":envelope["result"]});
            } else {
                details = json!({"result":envelope["result"], "cause":details});
            }
        }
        return Err(fault(
            envelope["error"]["code"]
                .as_str()
                .unwrap_or("update.native_failed"),
            envelope["error"]["message"]
                .as_str()
                .unwrap_or("staged Rust command failed"),
            if output.status == 0 { 5 } else { output.status },
        )
        .with_details(details));
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
fn open_tarball(path: &Path, integrity: &str, control: &OperationControl) -> Result<fs::File> {
    use sha2::{Digest, Sha512};
    use std::io::Read;
    control.check()?;
    storage::guard_path_with_check(
        path.parent()
            .ok_or_else(|| fault("update.invalid_path", "tarball parent missing", 4))?,
        path,
        || control.check(),
    )?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(path)?;
    let verified = (|| {
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > 512 << 20 {
            return Err(fault(
                "update.package_size",
                "pinned package exceeds 512 MiB or is not regular",
                3,
            ));
        }
        let mut digest = Sha512::new();
        let mut count = 0u64;
        let mut block = [0u8; 65536];
        loop {
            control.check()?;
            let next = file.read(&mut block)?;
            control.check()?;
            if next == 0 {
                break;
            }
            count += next as u64;
            if count > metadata.len() {
                return Err(fault(
                    "update.package_integrity",
                    "pinned tarball grew during read",
                    4,
                ));
            }
            digest.update(&block[..next]);
            #[cfg(test)]
            tests::observe_tarball_block(control, count);
        }
        if count != metadata.len()
            || storage::sha512_integrity_digest(&digest.finalize().into()) != integrity
        {
            return Err(fault(
                "update.package_integrity",
                "pinned npm tarball SRI differs",
                4,
            ));
        }
        control.check()
    })();
    match verified {
        Ok(()) => Ok(file),
        Err(error) => storage::finish_file(file, Err(error), control),
    }
}
#[cfg(test)]
fn save(path: &Path, record: &Handoff) -> Result<()> {
    save_controlled(path, record, &OperationControl::default())
}
fn save_controlled(path: &Path, record: &Handoff, control: &OperationControl) -> Result<()> {
    storage::atomic_write_controlled(path, &serde_json::to_vec_pretty(record)?, control)
}
fn canonical_home(ctx: &RequestContext) -> Result<PathBuf> {
    ctx.control.check()?;
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
        ctx.control.check()?;
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
    storage::guard_path_with_check(&existing, &existing, || ctx.control.check())?;
    let mut actual = ordinary(existing.canonicalize()?);
    for name in tail.into_iter().rev() {
        ctx.control.check()?;
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
#[cfg(test)]
fn archive_completed(directory: &Path, journal: &Path, record: &Handoff) -> Result<()> {
    archive_completed_controlled(directory, journal, record, &OperationControl::default())
}
fn archive_completed_controlled(
    directory: &Path,
    journal: &Path,
    record: &Handoff,
    control: &OperationControl,
) -> Result<()> {
    let raw = slots::read_bounded_controlled(journal, 1 << 20, control)?;
    let history = directory.join("history").join(storage::sha256(&raw));
    storage::guard_path_with_check(directory, &history, || control.check())?;
    control.check()?;
    fs::create_dir_all(&history)?;
    for (source, name) in [
        (&record.tarball, "archive.tgz"),
        (&directory.join("staging"), "staging"),
    ] {
        control.check()?;
        if exists_controlled(source, control)? {
            storage::guard_path_with_check(directory, source, || control.check())?;
            let destination = history.join(name);
            if exists_controlled(&destination, control)? {
                return Err(fault(
                    "update.pending_conflict",
                    "completed update history destination already exists",
                    6,
                ));
            }
            if source == &record.tarball {
                let held = open_tarball(source, &record.integrity, control)?;
                storage::finish_file(held, Ok(()), control)?;
            }
            control.check()?;
            fs::rename(source, destination)?;
        }
    }
    let destination = history.join("transaction.json");
    if exists_controlled(&destination, control)? {
        if slots::read_bounded_controlled(&destination, 1 << 20, control)? != raw {
            return Err(fault(
                "update.invalid_state",
                "completed journal history differs",
                4,
            ));
        }
        control.check()?;
        fs::remove_file(journal)?;
    } else {
        control.check()?;
        fs::rename(journal, destination)?;
    }
    control.check()
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
    ctx.control.check()?;
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
    let package: Value = read_controlled(&installed.join("package.json"), 1 << 20, &ctx.control)?;
    let current = package["version"]
        .as_str()
        .ok_or_else(|| fault("update.invalid_package", "installed version missing", 4))?;
    binding_controlled(&installed, current, &ctx.control)?;
    release::inspect_with_control(&installed, current, &ctx.control)?;
    let directory = prefix.join(".lycheedev-update");
    storage::guard_path_with_check(&prefix, &directory, || ctx.control.check())?;
    ctx.control.check()?;
    fs::create_dir_all(&directory)?;
    let lock = ExclusiveLock::resource_controlled(&directory, "npm-prefix-update", &ctx.control)?;
    let result = (|| {
        let journal = directory.join("transaction.json");
        if exists_controlled(&journal, &ctx.control)? {
            let record: Handoff = read_controlled(&journal, 1 << 20, &ctx.control)?;
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
                archive_completed_controlled(&directory, &journal, &record, &ctx.control)?;
            }
        }
        let npm = npm_entry(node, npm_search_paths, &ctx.control)?;
        let mut record = if exists_controlled(&journal, &ctx.control)? {
            let record: Handoff = read_controlled(&journal, 1 << 20, &ctx.control)?;
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
                    directory.to_str().ok_or_else(|| {
                        fault("update.invalid_path", "prefix path is not UTF-8", 4)
                    })?,
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
            save_controlled(&journal, &record, &ctx.control)?;
            record
        };
        storage::guard_path_with_check(&directory, &record.tarball, || ctx.control.check())?;
        let tarball = open_tarball(&record.tarball, &record.integrity, &ctx.control)?;
        let result = (|| {
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
            let staged_package: Value =
                read_controlled(&record.staged.join("package.json"), 1 << 20, &ctx.control)?;
            if staged_package["name"] != "lycheedev" || staged_package["version"] != record.version
            {
                return Err(fault(
                    "update.version_mismatch",
                    "staged package identity differs",
                    4,
                ));
            }
            if let Err(error) = binding_controlled(&record.staged, &record.version, &ctx.control) {
                if error.code == "update.runtime_incompatible" {
                    record.phase = "rejected".into();
                    record.failure = Some(error.clone());
                    save_controlled(&journal, &record, &ctx.control)?;
                }
                return Err(error);
            }
            let release =
                release::inspect_with_control(&record.staged, &record.version, &ctx.control)?;
            let helper = record.staged.join("native/windows-amd64/lycheedev.exe");
            record.helper_sha256 = release.binaries["windows-amd64"].sha256.clone();
            let targets = directory.join("targets.json");
            storage::atomic_write_controlled(
                &targets,
                &serde_json::to_vec_pretty(&record.targets)?,
                &ctx.control,
            )?;
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
            save_controlled(&journal, &record, &ctx.control)?;
            let handoff = directory.join(format!("handoff-{nonce}.json"));
            let mut intent: Handoff = serde_json::from_value(serde_json::to_value(&record)?)?;
            intent.phase = "handoff".into();
            save_controlled(&handoff, &intent, &ctx.control)?;
            Err(fault("update.handoff_ready","prepared verified Rust helper; the launcher must exit the old native process before prefix replacement",6).with_details(json!({"result":{"complete":false,"handoff":handoff,"helper":helper,"version":record.version}})))
        })();
        finish_update_file(tarball, result, &ctx.control)
    })();
    finish_prefix(result, lock)
}
pub fn apply_handoff(path: &Path, nonce: &str, control: &OperationControl) -> Result<Outcome> {
    control.check()?;
    let resolved = ordinary(path.canonicalize()?);
    let path = resolved.as_path();
    let mut record: Handoff = read_controlled(path, 1 << 20, control)?;
    let directory = record.prefix.join(".lycheedev-update");
    storage::guard_path_with_check(&record.prefix, &directory, || control.check())?;
    storage::guard_path_with_check(&directory, path, || control.check())?;
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
    if own != helper {
        return Err(fault(
            "update.invalid_handoff",
            "this process is not the exact verified staged helper",
            4,
        ));
    }
    binding_controlled(&record.staged, &record.version, control)?;
    let staged_release = release::inspect_with_control(&record.staged, &record.version, control)?;
    let expected_binary = &staged_release.binaries["windows-amd64"];
    let (held_helper, digest) = storage::locked_sha256(&helper, expected_binary.bytes, control)?;
    let result = (|| {
        if digest != record.helper_sha256 || held_helper.metadata()?.len() != expected_binary.bytes
        {
            return Err(fault(
                "update.invalid_handoff",
                "this process is not the exact verified staged helper",
                4,
            ));
        }
        let lock = ExclusiveLock::resource_controlled(&directory, "npm-prefix-update", control)?;
        let result = (|| {
            let journal = directory.join("transaction.json");
            let current: Handoff = read_controlled(&journal, 1 << 20, control)?;
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
                binding_controlled(&record.installed, &record.version, control)?;
                release::inspect_with_control(&record.installed, &record.version, control)?;
                return record.result.ok_or_else(|| {
                    fault(
                        "update.invalid_state",
                        "completed journal has no durable result",
                        4,
                    )
                });
            }
            control.check()?;
            storage::guard_path_with_check(&directory, &record.tarball, || control.check())?;
            let tarball = open_tarball(&record.tarball, &record.integrity, control)?;
            let result = (|| {
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
                    save_controlled(&journal, &record, control)?;
                }
                binding_controlled(&record.installed, &record.version, control)?;
                release::inspect_with_control(&record.installed, &record.version, control)?;
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
                result.data["cli"] =
                    json!({"version":record.version,"state":"current","prefix":record.prefix});
                result.warnings.push(json!("Exact update artifacts remain for recovery; the mapped helper staging tree is reclaimed only after its process exits."));
                record.phase = "complete".into();
                record.result = Some(result.clone());
                if let Err(mut error) = save_controlled(&journal, &record, control) {
                    let cause = error.clone();
                    error.details = json!({
                        "schema":"lycheedev.delivery-cleanup.v1", "operationCompleted":true,
                        "result":result, "journal":journal, "journalState":"unconfirmed", "cause":cause,
                    });
                    return Err(error);
                }
                Ok(result)
            })();
            finish_update_file(tarball, result, control)
        })();
        finish_prefix(result, lock)
    })();
    finish_update_file(held_helper, result, control)
}
pub fn helper_control(path: &Path, cancel: TrackedCancellation) -> Result<OperationControl> {
    let record: Handoff = read_controlled(path, 1 << 20, &OperationControl::default())?;
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
    let record: Handoff = read_controlled(path, 1 << 20, &OperationControl::default())?;
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
    type PrefixClose = Box<dyn FnMut(Result<()>) -> Result<()>>;
    type TarballBlock = Box<dyn FnMut(&OperationControl, u64)>;
    thread_local! {
        static PREFIX_CLOSE: std::cell::RefCell<Option<PrefixClose>> = std::cell::RefCell::new(None);
        static TARBALL_BLOCK: std::cell::RefCell<Option<TarballBlock>> = std::cell::RefCell::new(None);
    }
    pub(super) fn observe_prefix_close(result: Result<()>) -> Result<()> {
        PREFIX_CLOSE.with_borrow_mut(|observer| match observer {
            Some(observer) => observer(result),
            None => result,
        })
    }
    pub(super) fn observe_tarball_block(control: &OperationControl, count: u64) {
        TARBALL_BLOCK.with_borrow_mut(|observer| {
            if let Some(observer) = observer {
                observer(control, count);
            }
        });
    }
    struct Observation;
    impl Drop for Observation {
        fn drop(&mut self) {
            PREFIX_CLOSE.with_borrow_mut(|observer| *observer = None);
            TARBALL_BLOCK.with_borrow_mut(|observer| *observer = None);
        }
    }
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
    fn native_result_keeps_public_recovery_assessment_and_cleanup_details() {
        // These are the public JSON envelope fields emitted by the staged CLI.
        // The parser below is the same one native() consumes after process::run.
        let primary = ToolkitError::new("delivery.installation_conflict", "selected target changed", 4)
            .with_details(json!({"path":"D:/owned/lycheedev", "assessment":{"state":"modified","detail":"owned payload differs"}}));
        let close = ToolkitError::new("external.io", "original receipt close failed", 5)
            .with_details(json!({"owner":"original receipt"}));
        let cleanup = json!({"schema":storage::FILE_CLEANUP_SCHEMA,"primary":primary,"cleanup":close,"control":null});
        for details in [
            json!({"path":"D:/owned/lycheedev", "assessment":{"state":"modified","detail":"owned payload differs"}}),
            json!({"path":"D:/owned/lycheedev", "recovery":"D:/owned/.lycheedev-updates/original", "recoveries":["D:/owned/.lycheedev-updates/original"]}),
            cleanup,
            json!({"schema":"lycheedev.delivery-cleanup.v1","operationCompleted":true,"result":{"complete":true,"targets":[{"component":"skill","state":"current"}]},"cleanup":{"code":"external.io","message":"close failed","exit_code":5,"details":null}}),
        ] {
            let public_result = details.get("result").cloned().unwrap_or(Value::Null);
            let wire = json!({"schema":"lycheedev.result.v1","ok":false,"operationId":"", "context":{},"result":public_result,"captures":[],"warnings":[],
                "error":{"code":"delivery.installation_conflict","message":"selected target changed","stage":"dispatch","retryable":false,"resumeOperationId":"","details":details}});
            let error = decode_native_output(process::Output {
                status: 4,
                stdout: serde_json::to_vec(&wire).unwrap(),
                stderr: vec![],
            })
            .unwrap_err();
            assert_eq!(error.code, "delivery.installation_conflict");
            assert_eq!(error.exit_code, 4);
            assert_eq!(
                error.details, details,
                "a staged plan failure retains its original public recovery facts"
            );
        }
    }
    #[test]
    fn native_result_success_control_retains_real_plan_result() {
        let wire = json!({"schema":"lycheedev.result.v1","ok":true,"operationId":"", "context":{},"result":{"plan":true,"complete":false,"targets":[{"component":"skill","state":"absent"}]},"captures":[],"warnings":[],"error":null});
        let actual = decode_native_output(process::Output {
            status: 0,
            stdout: serde_json::to_vec(&wire).unwrap(),
            stderr: vec![],
        })
        .unwrap();
        assert_eq!(actual, wire);
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

    // A complete filesystem-only prefix reaches the real history handoff, then
    // stops at a deliberately absent npm entry. No Node/helper can be launched.
    fn completed_prefix_fixture() -> (Temp, RequestContext, PathBuf) {
        let temp = fixture();
        let prefix = ordinary(temp.0.canonicalize().unwrap());
        let installed = prefix.join("node_modules/lycheedev");
        fs::create_dir_all(&installed).unwrap();
        fs::write(prefix.join("lycheedev.cmd"), b"owned launcher fixture").unwrap();
        storage::atomic_json(
            &installed.join("package.json"),
            &json!({"name":"lycheedev", "version":lycheedev_core::VERSION}),
        )
        .unwrap();
        let files = [
            ("skill/SKILL.md", b"owned skill\n".as_slice()),
            ("addon/Lychee Dev.toc", b"## Interface: 120105\n".as_slice()),
        ];
        let mut resources = Vec::new();
        for (name, bytes) in files {
            let path = installed.join("payload").join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
            resources
                .push(json!({"path":name,"bytes":bytes.len(),"sha256":storage::sha256(bytes)}));
        }
        let native = b"owned native inventory fixture; never executed";
        let binary = installed.join("native/windows-amd64/lycheedev.exe");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, native).unwrap();
        storage::atomic_json(&installed.join("release.json"), &json!({
            "schema":"lycheedev.release.v1","version":lycheedev_core::VERSION,"commit":"a".repeat(40),
            "runtime":"rust","npmChannel":{
                "schema":"lycheedev.npm-channel.v1","version":lycheedev_core::VERSION,
                "distTag":release_channel(lycheedev_core::VERSION),"preserveLatest":"3.2.7"
            },
            "binaries":{"windows-amd64":{"path":"native/windows-amd64/lycheedev.exe","bytes":native.len(),"sha256":storage::sha256(native)}},
            "resources":resources,
        })).unwrap();
        // Establish that the selected prefix passes the same channel and full
        // inventory gates used by prepare, rather than failing an earlier gate.
        binding(&installed, lycheedev_core::VERSION).unwrap();
        release::inspect_with_control(
            &installed,
            lycheedev_core::VERSION,
            &OperationControl::default(),
        )
        .unwrap();
        let mut prior = record(&prefix, lycheedev_core::VERSION);
        prior.result = Some(Outcome::new(json!({"complete":true,"targets":[]})));
        fs::create_dir_all(prior.tarball.parent().unwrap()).unwrap();
        fs::write(&prior.tarball, b"archive").unwrap();
        fs::create_dir_all(prefix.join(".lycheedev-update/staging")).unwrap();
        fs::write(
            prefix.join(".lycheedev-update/staging/owned.txt"),
            b"prior completed staging",
        )
        .unwrap();
        save(&prefix.join(".lycheedev-update/transaction.json"), &prior).unwrap();
        let ctx = RequestContext {
            home: prior.home,
            cwd: prefix,
            control: OperationControl::default(),
        };
        (temp, ctx, installed)
    }
    fn prefix_snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn visit(
            root: &Path,
            directory: &Path,
            result: &mut std::collections::BTreeMap<PathBuf, Option<Vec<u8>>>,
        ) {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    result.insert(path.strip_prefix(root).unwrap().to_owned(), None);
                    visit(root, &path, result);
                } else {
                    result.insert(
                        path.strip_prefix(root).unwrap().to_owned(),
                        Some(fs::read(path).unwrap()),
                    );
                }
            }
        }
        let mut result = std::collections::BTreeMap::new();
        visit(root, root, &mut result);
        result
    }
    #[test]
    fn prepare_complete_prefix_reaches_history_without_starting_a_tool() {
        let (_owned, ctx, installed) = completed_prefix_fixture();
        let error = prepare(
            &ctx,
            "json",
            &installed,
            &ctx.cwd.join("no-node-will-run.exe"),
            &"1".repeat(64),
            &[],
            vec![],
        )
        .unwrap_err();
        assert_eq!(error.code, "update.npm_missing");
        let directory = ctx.cwd.join(".lycheedev-update");
        assert!(!directory.join("transaction.json").exists());
        let history: Vec<_> = fs::read_dir(directory.join("history"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(history.len(), 1);
        assert_eq!(
            fs::read(history[0].join("archive.tgz")).unwrap(),
            b"archive"
        );
        assert_eq!(
            fs::read(history[0].join("staging/owned.txt")).unwrap(),
            b"prior completed staging"
        );
        assert!(history[0].join("transaction.json").is_file());
        assert!(!ctx.home.exists());
    }
    #[test]
    fn prepare_stopped_control_never_archives_completed_history_or_creates_a_lock() {
        for deadline in [false, true] {
            let (_owned, mut ctx, installed) = completed_prefix_fixture();
            if deadline {
                ctx.control =
                    OperationControl::uncancelable(Instant::now() - Duration::from_millis(1));
            } else {
                ctx.control.tracked_cancel_handle().unwrap().cancel();
            }
            let expected = ctx.control.check().unwrap_err();
            let before = prefix_snapshot(&ctx.cwd);
            let error = prepare(
                &ctx,
                "json",
                &installed,
                &ctx.cwd.join("no-node-will-run.exe"),
                &"1".repeat(64),
                &[],
                vec![],
            )
            .unwrap_err();
            assert_eq!(
                prefix_snapshot(&ctx.cwd),
                before,
                "stopped prepare mutated its prefix"
            );
            assert_eq!(error.code, expected.code);
            assert_eq!(error.exit_code, expected.exit_code);
            assert!(!ctx.home.exists());
        }
    }
    #[test]
    fn streamed_tarball_uses_fixed_sri_and_stops_under_original_control() {
        let f = fixture();
        let path = f.0.join("archive.tgz");
        let bytes = vec![b'a'; 131089];
        fs::write(&path, &bytes).unwrap();
        // Independent SHA-512 SRI for131089 ASCII 'a' bytes, crossing two
        // complete read blocks. This is not generated by either Rust helper.
        let sri = "sha512-C0CdgWIYHUhc7ZJIO/agK34GlTLIKmDpXOQ15ZhopuFqEnqjRqAE6DXyTXczIs4VO01tE2Aa5xbD8J/fagL6mA==";
        let control = OperationControl::default();
        let held = open_tarball(&path, sri, &control).unwrap();
        #[cfg(windows)]
        assert!(fs::write(&path, b"must remain pinned").is_err());
        storage::finish_file(held, Ok(()), &control).unwrap();
        let original = control.clone();
        let cancel = control.tracked_cancel_handle().unwrap();
        let blocks = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let observed = blocks.clone();
        TARBALL_BLOCK.with_borrow_mut(|observer| {
            *observer = Some(Box::new(move |actual, count| {
                assert!(actual.same_scope(&original));
                observed.borrow_mut().push(count);
                cancel.cancel();
            }))
        });
        let observation = Observation;
        let error = open_tarball(&path, sri, &control).unwrap_err();
        drop(observation);
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(*blocks.borrow(), [65536]);
        fs::write(&path, &bytes).unwrap();
    }
    #[test]
    fn prepare_prefix_lock_checked_close_preserves_primary_error_and_releases_owner() {
        let (_owned, ctx, installed) = completed_prefix_fixture();
        PREFIX_CLOSE.with_borrow_mut(|observer| {
            *observer = Some(Box::new(|result| {
                result?;
                Err(fault(
                    "fixture.prefix_close",
                    "injected after real prefix close",
                    5,
                )
                .with_details(json!({"originalCloseConsumed":true})))
            }))
        });
        let observation = Observation;
        let error = prepare(
            &ctx,
            "json",
            &installed,
            &ctx.cwd.join("no-node-will-run.exe"),
            &"1".repeat(64),
            &[],
            vec![],
        )
        .unwrap_err();
        drop(observation);
        assert_eq!(error.code, "update.npm_missing");
        assert_eq!(error.details["schema"], "lycheedev.delivery-cleanup.v1");
        assert_eq!(error.details["primary"]["code"], "update.npm_missing");
        assert_eq!(error.details["cleanup"]["code"], "fixture.prefix_close");
        assert_eq!(
            error.details["cleanup"]["details"]["originalCloseConsumed"],
            true
        );
        ExclusiveLock::resource(&ctx.cwd.join(".lycheedev-update"), "npm-prefix-update")
            .unwrap()
            .close()
            .unwrap();
    }
    #[cfg(windows)]
    fn applying_prefix_fixture() -> (Temp, RequestContext, PathBuf, PathBuf) {
        let (owned, ctx, installed) = completed_prefix_fixture();
        let mut record: Handoff = read_controlled(
            &ctx.cwd.join(".lycheedev-update/transaction.json"),
            1 << 20,
            &ctx.control,
        )
        .unwrap();
        record.phase = "prefix".into();
        record.result = None;
        record.deadline_unix_millis = millis().unwrap() + 60000;
        let skill_parent = ctx.cwd.join("skills");
        fs::create_dir_all(&skill_parent).unwrap();
        let target = fs::canonicalize(&skill_parent).unwrap().join("lycheedev");
        record.targets = vec![UpdateTarget {
            component: "skill".into(),
            path: target.clone(),
        }];
        for relative in [
            "package.json",
            "release.json",
            "payload/skill/SKILL.md",
            "payload/addon/Lychee Dev.toc",
        ] {
            let destination = record.staged.join(relative);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(installed.join(relative), destination).unwrap();
        }
        let helper = record.staged.join("native/windows-amd64/lycheedev.exe");
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::copy(std::env::current_exe().unwrap(), &helper).unwrap();
        let bytes = fs::metadata(&helper).unwrap().len();
        let (held, digest) = storage::locked_sha256(&helper, bytes, &ctx.control).unwrap();
        storage::finish_file(held, Ok(()), &ctx.control).unwrap();
        record.helper_sha256 = digest.clone();
        let mut manifest: Value =
            read_controlled(&record.staged.join("release.json"), 1 << 20, &ctx.control).unwrap();
        manifest["binaries"]["windows-amd64"]["bytes"] = json!(bytes);
        manifest["binaries"]["windows-amd64"]["sha256"] = json!(digest);
        storage::atomic_json(&record.staged.join("release.json"), &manifest).unwrap();
        save(&ctx.cwd.join(".lycheedev-update/transaction.json"), &record).unwrap();
        record.phase = "handoff".into();
        save(
            &ctx.cwd
                .join(".lycheedev-update")
                .join(format!("handoff-{}.json", record.nonce)),
            &record,
        )
        .unwrap();
        (owned, ctx, helper, target)
    }
    #[test]
    #[cfg(windows)]
    #[ignore = "child fixture only; invoked by handoff_final_save parent regressions"]
    fn handoff_save_fixture() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = PathBuf::from(
            std::env::var_os("LYCHEEDEV_NPM_SAVE_FIXTURE")
                .expect("explicit owned npm handoff fixture root"),
        );
        let journal = root.join(".lycheedev-update/transaction.json");
        let denied = if std::env::var("LYCHEEDEV_NPM_DENY_SAVE").as_deref() == Ok("1") {
            Some(
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(1)
                    .open(&journal)
                    .unwrap(),
            )
        } else {
            None
        };
        let handoff = root
            .join(".lycheedev-update")
            .join(format!("handoff-{}.json", "1".repeat(64)));
        let control = helper_control(&handoff, TrackedCancellation::new()).unwrap();
        let report = match apply_handoff(&handoff, &"1".repeat(64), &control) {
            Ok(result) => json!({"ok":true,"result":result}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        drop(denied);
        storage::atomic_json(&root.join("apply-report.json"), &report).unwrap();
    }
    #[cfg(windows)]
    fn run_handoff_save_fixture(deny_save: bool) -> (Temp, RequestContext, PathBuf, Value) {
        let (owned, ctx, helper, target) = applying_prefix_fixture();
        let mut command = Command::new(helper);
        command
            .args([
                "--exact",
                "npm_update::tests::handoff_save_fixture",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("LYCHEEDEV_NPM_SAVE_FIXTURE", &ctx.cwd)
            .env("LYCHEEDEV_NPM_DENY_SAVE", if deny_save { "1" } else { "0" });
        let output = process::run(command, None, 1 << 20, 1 << 20, &ctx.control).unwrap();
        assert_eq!(
            output.status,
            0,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value =
            read_controlled(&ctx.cwd.join("apply-report.json"), 1 << 20, &ctx.control).unwrap();
        assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"owned skill\n");
        (owned, ctx, target, report)
    }
    #[test]
    #[cfg(windows)]
    fn handoff_final_save_control_proves_real_managed_update_and_completed_record() {
        let (_owned, ctx, target, report) = run_handoff_save_fixture(false);
        assert_eq!(report["ok"], true, "{report}");
        assert_eq!(
            report["result"]["data"]["targets"][0]["path"],
            json!(target)
        );
        assert_eq!(report["result"]["data"]["targets"][0]["state"], "current");
        let current: Handoff = read_controlled(
            &ctx.cwd.join(".lycheedev-update/transaction.json"),
            1 << 20,
            &ctx.control,
        )
        .unwrap();
        assert_eq!(current.phase, "complete");
        assert!(current.result.unwrap().data["complete"].as_bool().unwrap());
    }
    #[test]
    #[cfg(windows)]
    fn handoff_final_save_failure_retains_committed_managed_result() {
        let (_owned, ctx, target, report) = run_handoff_save_fixture(true);
        assert_eq!(report["ok"], false, "{report}");
        assert_eq!(report["error"]["exit_code"], 5);
        assert_eq!(report["error"]["details"]["operationCompleted"], true);
        assert_eq!(report["error"]["details"]["journalState"], "unconfirmed");
        assert!(report["error"]["details"]["cause"]["details"].is_object());
        let current: Handoff = read_controlled(
            &ctx.cwd.join(".lycheedev-update/transaction.json"),
            1 << 20,
            &ctx.control,
        )
        .unwrap();
        assert_eq!(
            current.phase, "prefix",
            "the failed save must not claim durable completion"
        );
        let retained = &report["error"]["details"]["result"];
        assert_eq!(
            retained["data"]["complete"], true,
            "committed update facts must survive journal failure: {report}"
        );
        assert_eq!(retained["data"]["targets"][0]["path"], json!(target));
        assert_eq!(retained["data"]["targets"][0]["state"], "current");
        assert_eq!(retained["data"]["cli"]["state"], "current");
    }
}
