//! Explicit source checkout reclamation. Immutable pins, facts, mirrors and evidence are outside ownership.
use crate::{catalog, git};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError,
    selection::{SourcePin, valid_digest},
    storage::{self, ExclusiveLock},
    workspace,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};
const LIMIT: u64 = 4 << 30;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub repository: String,
    pub commit: String,
    pub tree: String,
    pub files: usize,
    pub bytes: u64,
    pub state: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneResult {
    pub budget_bytes: u64,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub removed: Vec<String>,
    pub skipped: Vec<String>,
    pub complete: bool,
}
fn fault(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new("codebase.source_integrity", message, 4)
}
fn guard(home: &Path, path: &Path) -> Result<()> {
    if path.starts_with(home) {
        storage::guard_path(home, path)
    } else {
        storage::guard_path(&home.canonicalize()?, path)
    }
}
fn pin(repository: &str, commit: &str) -> Result<SourcePin> {
    let repo = catalog::lookup(repository)?;
    let product = repo
        .tracks
        .keys()
        .next()
        .ok_or_else(|| fault("repository has no canonical track"))?;
    Ok(SourcePin {
        repository: repository.into(),
        product: product.clone(),
        requested_ref: String::new(),
        exact_commit: commit.into(),
        parser_revision: git::PARSER.into(),
    })
}
fn owned_path(home: &Path, repository: &str, commit: &str) -> PathBuf {
    home.join("source/v1/worktrees")
        .join(repository)
        .join(commit)
}
fn manifest_path(home: &Path, repository: &str, commit: &str) -> PathBuf {
    home.join("source/v1/manifests")
        .join(repository)
        .join(format!("{commit}.json"))
}
fn key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("//?/")
        .to_lowercase()
}
fn small(home: &Path, path: &Path) -> Result<String> {
    guard(home, path)?;
    let file = fs::File::open(path)?;
    let info = file.metadata()?;
    if !info.is_file() || info.len() > 4096 {
        return Err(fault("invalid Git binding file"));
    }
    let mut raw = Vec::new();
    file.take(4097).read_to_end(&mut raw)?;
    if raw.len() > 4096 {
        return Err(fault("oversized Git binding file"));
    }
    Ok(String::from_utf8(raw)
        .map_err(|_| fault("Git binding file encoding"))?
        .trim()
        .into())
}
fn resolve(home: &Path, base: &Path, value: &str) -> Result<PathBuf> {
    let raw = Path::new(value);
    let path = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        base.join(raw)
    };
    guard(home, &path)?;
    let actual = path.canonicalize()?;
    guard(home, &actual)?;
    Ok(actual)
}
fn binding(home: &Path, source: &SourcePin, checkout: &Path) -> Result<()> {
    guard(home, checkout)?;
    let line = small(home, &checkout.join(".git"))?;
    let admin = resolve(
        home,
        checkout,
        line.strip_prefix("gitdir: ")
            .ok_or_else(|| fault("checkout Git binding differs"))?,
    )?;
    let mirror = git::mirror(home, &source.repository).canonicalize()?;
    let root = mirror.join("worktrees");
    guard(home, &admin)?;
    let relative = admin
        .strip_prefix(&root)
        .map_err(|_| fault("checkout selects a foreign Git admin directory"))?;
    if relative.components().count() != 1 || admin.join("config.worktree").exists() {
        return Err(fault("checkout has foreign Git settings"));
    }
    let common = resolve(home, &admin, &small(home, &admin.join("commondir"))?)?;
    if key(&common) != key(&mirror) {
        return Err(fault("checkout selects a foreign common repository"));
    }
    let back = resolve(home, &admin, &small(home, &admin.join("gitdir"))?)?;
    if key(&back) != key(&checkout.join(".git").canonicalize()?)
        || small(home, &admin.join("HEAD"))? != source.exact_commit
    {
        return Err(fault("checkout detached identity differs"));
    }
    Ok(())
}
fn repository_inert(
    home: &Path,
    repo: &str,
    control: &lycheedev_core::control::OperationControl,
) -> Result<()> {
    let mirror = git::mirror(home, repo);
    guard(home, &mirror)?;
    let raw = git::bytes_with_control(
        Some(&mirror),
        &["config", "--local", "--list", "--null"],
        64 << 10,
        control,
    )?;
    for entry in raw.split(|b| *b == 0).filter(|entry| !entry.is_empty()) {
        let name = std::str::from_utf8(entry)
            .map_err(|_| fault("repository settings encoding"))?
            .split_once('\n')
            .ok_or_else(|| fault("repository setting structure"))?
            .0
            .to_lowercase();
        if [
            "core.repositoryformatversion",
            "core.filemode",
            "core.bare",
            "core.logallrefupdates",
            "core.ignorecase",
            "core.precomposeunicode",
            "core.symlinks",
            "extensions.objectformat",
        ]
        .contains(&name.as_str())
            || name.starts_with("remote.") && (name.ends_with(".url") || name.ends_with(".fetch"))
        {
            continue;
        }
        return Err(fault(format!(
            "codebase.worktree_untrusted_git_config: {name}"
        )));
    }
    Ok(())
}
fn registered(
    home: &Path,
    source: &SourcePin,
    checkout: &Path,
    control: &lycheedev_core::control::OperationControl,
) -> Result<bool> {
    let raw = git::bytes_with_control(
        Some(&git::mirror(home, &source.repository)),
        &["worktree", "list", "--porcelain", "-z"],
        1 << 20,
        control,
    )?;
    let text = std::str::from_utf8(&raw).map_err(|_| fault("Git worktree registry encoding"))?;
    for block in text.split("\0\0") {
        let fields: Vec<_> = block.split('\0').collect();
        if fields
            .first()
            .and_then(|field| field.strip_prefix("worktree "))
            .is_some_and(|path| key(Path::new(path)) == key(checkout))
        {
            return Ok(
                fields.contains(&format!("HEAD {}", source.exact_commit).as_str())
                    && fields.contains(&"detached"),
            );
        }
    }
    Ok(false)
}
fn scan(
    home: &Path,
    root: &Path,
    control: &lycheedev_core::control::OperationControl,
) -> Result<(u64, BTreeSet<String>)> {
    if !root.exists() {
        return Ok((0, BTreeSet::new()));
    }
    guard(home, root)?;
    let mut directories = vec![root.to_path_buf()];
    let mut bytes = 0u64;
    let mut files = BTreeSet::new();
    let mut count = 0;
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)? {
            control.check()?;
            let entry = entry?;
            let path = entry.path();
            guard(home, &path)?;
            count += 1;
            if count > 200000 {
                return Err(ToolkitError::new(
                    "codebase.source_budget",
                    "source checkout scan exceeds 200000 entries",
                    3,
                ));
            }
            let info = entry.metadata()?;
            if info.is_dir() {
                directories.push(path);
            } else if info.is_file() {
                bytes = bytes
                    .checked_add(info.len())
                    .ok_or_else(|| fault("checkout byte overflow"))?;
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            } else {
                return Err(fault("checkout contains a special file"));
            }
            if bytes > LIMIT {
                return Err(ToolkitError::new(
                    "codebase.source_budget",
                    "source checkout accounting exceeds 4 GiB",
                    3,
                ));
            }
        }
    }
    Ok((bytes, files))
}
pub fn verify(home: &Path, source: &SourcePin, checkout: &Path) -> Result<Manifest> {
    verify_with_control(home, source, checkout, &Default::default())
}
pub fn verify_with_control(
    home: &Path,
    source: &SourcePin,
    checkout: &Path,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Manifest> {
    control.check()?;
    git::validate_pin(source)?;
    repository_inert(home, &source.repository, control)?;
    binding(home, source, checkout)?;
    if !registered(home, source, checkout, control)? {
        return Err(fault("checkout has no exact detached registration"));
    }
    let tree = git::bytes_with_control(
        Some(&git::mirror(home, &source.repository)),
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{tree}}", source.exact_commit),
        ],
        128,
        control,
    )?;
    let tree = String::from_utf8(tree)
        .map_err(|_| fault("tree identity encoding"))?
        .trim()
        .to_owned();
    if !valid_digest(&tree, 40) {
        return Err(fault("tree identity differs"));
    }
    let files = git::regular_tree_with_control(home, source, control)?;
    let mut tracked = BTreeSet::new();
    let mut bytes = 0u64;
    for file in &files {
        let path = storage::safe_relative(checkout, &file.path)?;
        guard(home, &path)?;
        git::verify_file_with_control(&path, file, 128 << 20, control)?;
        guard(home, &path)?;
        if !tracked.insert(file.path.clone()) {
            return Err(fault("duplicate checkout path"));
        }
        bytes += file.bytes as u64;
    }
    let (_, actual) = scan(home, checkout, control)?;
    tracked.insert(".git".into());
    if actual != tracked {
        return Err(fault("checkout contains extra or missing files"));
    }
    Ok(Manifest {
        schema: "lycheedev.source-worktree.v1".into(),
        repository: source.repository.clone(),
        commit: source.exact_commit.clone(),
        tree,
        files: files.len(),
        bytes,
        state: "ready".into(),
    })
}
pub fn prune(home: &Path, budget: u64) -> Result<PruneResult> {
    prune_with_control(home, budget, &Default::default())
}
pub fn prune_with_control(
    home: &Path,
    budget: u64,
    control: &lycheedev_core::control::OperationControl,
) -> Result<PruneResult> {
    control.check()?;
    workspace::open(home)?;
    let _capacity = ExclusiveLock::resource(&home.join("locks"), "source:v1:worktree-capacity")?;
    // Broker retirement has a separate process identity protocol. Never infer it from age/PID absence.
    let _broker = ExclusiveLock::resource(&home.join("locks"), "source:v1:broker-owner")
        .map_err(|e| ToolkitError::new("codebase.source_session_in_use", e.message, 6))?;
    let root = home.join("source/v1/worktrees");
    let before = scan(home, &root, control)?.0;
    let mut result = PruneResult {
        budget_bytes: budget,
        before_bytes: before,
        after_bytes: before,
        removed: vec![],
        skipped: vec![],
        complete: false,
    };
    let manifests = home.join("source/v1/manifests");
    let mut candidates = Vec::new();
    let mut known = BTreeSet::new();
    if manifests.exists() {
        guard(home, &manifests)?;
        for repo in fs::read_dir(&manifests)? {
            control.check()?;
            let repo = repo?;
            guard(home, &repo.path())?;
            let name = repo.file_name().to_string_lossy().into_owned();
            catalog::lookup(&name)?;
            if !repo.file_type()?.is_dir() {
                return Err(fault("source manifest repository kind differs"));
            }
            for entry in fs::read_dir(repo.path())? {
                control.check()?;
                let entry = entry?;
                guard(home, &entry.path())?;
                let file_name = entry.file_name().to_string_lossy().into_owned();
                let commit = file_name
                    .strip_suffix(".json")
                    .ok_or_else(|| fault("unknown source manifest filename"))?;
                if !valid_digest(commit, 40)
                    || !entry.file_type()?.is_file()
                    || entry.metadata()?.len() > 65536
                {
                    return Err(fault("source manifest admission differs"));
                }
                if entry.path() != manifest_path(home, &name, commit) {
                    return Err(fault("source manifest path binding differs"));
                }
                let file = fs::File::open(entry.path())?;
                let mut raw = Vec::new();
                file.take(65537).read_to_end(&mut raw)?;
                if raw.len() > 65536 {
                    return Err(fault("source manifest exceeds budget"));
                }
                let manifest: Manifest = serde_json::from_slice(&raw)?;
                if manifest.schema != "lycheedev.source-worktree.v1"
                    || manifest.state != "ready"
                    || manifest.repository != name
                    || manifest.commit != commit
                    || !valid_digest(&manifest.tree, 40)
                    || manifest.files > 200000
                    || manifest.bytes > LIMIT
                {
                    return Err(fault("source manifest immutable identity differs"));
                }
                known.insert((name.clone(), commit.to_owned()));
                candidates.push((
                    entry
                        .metadata()?
                        .modified()
                        .unwrap_or(SystemTime::UNIX_EPOCH),
                    entry.path(),
                    manifest,
                ));
                if candidates.len() > 100000 {
                    return Err(ToolkitError::new(
                        "codebase.source_budget",
                        "source manifest scan exceeds 100000 entries",
                        3,
                    ));
                }
            }
        }
    }
    // Unknown roots and unowned orphans remain charged, never silently adopted or deleted.
    if root.exists() {
        for repo in fs::read_dir(&root)? {
            control.check()?;
            let repo = repo?;
            let name = repo.file_name().to_string_lossy().into_owned();
            guard(home, &repo.path())?;
            if !repo.file_type()?.is_dir() || catalog::lookup(&name).is_err() {
                result.skipped.push(format!("{name}:untrusted"));
                continue;
            }
            for entry in fs::read_dir(repo.path())? {
                let entry = entry?;
                let commit = entry.file_name().to_string_lossy().into_owned();
                guard(home, &entry.path())?;
                if !known.contains(&(name.clone(), commit.clone())) {
                    result.skipped.push(format!("{name}:{commit}:unowned"));
                }
            }
        }
    }
    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.2.repository.cmp(&b.2.repository))
            .then(a.2.commit.cmp(&b.2.commit))
    });
    for (_, path, manifest) in candidates {
        control.check()?;
        let source = pin(&manifest.repository, &manifest.commit)?;
        let checkout = owned_path(home, &manifest.repository, &manifest.commit);
        let label = format!("{}:{}", manifest.repository, manifest.commit);
        if checkout.exists() && result.after_bytes <= budget {
            continue;
        }
        let _lease = match ExclusiveLock::resource(
            &home.join("locks"),
            &format!("source:v1:worktree:{label}"),
        ) {
            Ok(lock) => lock,
            Err(error) if error.exit_code == 6 => {
                result.skipped.push(format!("{label}:in-use"));
                continue;
            }
            Err(error) => return Err(error),
        };
        let _repository = match ExclusiveLock::resource(
            &home.join("locks"),
            &format!("source:v1:repo:{}", manifest.repository),
        ) {
            Ok(lock) => lock,
            Err(error) if error.exit_code == 6 => {
                result.skipped.push(format!("{label}:lock"));
                continue;
            }
            Err(error) => return Err(error),
        };
        repository_inert(home, &manifest.repository, control)?;
        if !checkout.exists() {
            if registered(home, &source, &checkout, control)? {
                git::bytes_with_control(
                    Some(&git::mirror(home, &manifest.repository)),
                    &[
                        "worktree",
                        "remove",
                        "--force",
                        checkout
                            .to_str()
                            .ok_or_else(|| fault("checkout path encoding"))?,
                    ],
                    4096,
                    control,
                )?;
            }
            guard(home, &path)?;
            fs::remove_file(path)?;
            result.removed.push(format!("{label}:stale-manifest"));
            continue;
        }
        match verify_with_control(home, &source, &checkout, control) {
            Ok(actual) if actual == manifest => {}
            Ok(_) => {
                result.skipped.push(format!("{label}:modified"));
                continue;
            }
            Err(error) if error.exit_code == 3 || error.exit_code == 4 => {
                result.skipped.push(format!("{label}:modified"));
                continue;
            }
            Err(error) => return Err(error),
        }
        guard(home, &checkout)?;
        control.check()?;
        let expected = owned_path(home, &manifest.repository, &manifest.commit).canonicalize()?;
        if key(&expected) != key(&checkout.canonicalize()?)
            || !expected.starts_with(root.canonicalize()?)
        {
            return Err(fault("resolved deletion target escapes source ownership"));
        }
        match git::bytes_with_control(
            Some(&git::mirror(home, &manifest.repository)),
            &[
                "worktree",
                "remove",
                checkout
                    .to_str()
                    .ok_or_else(|| fault("checkout path encoding"))?,
            ],
            4096,
            control,
        ) {
            Ok(_) => {}
            Err(error) if error.exit_code == 7 || error.code == "command.deadline" => {
                return Err(error);
            }
            Err(_) => {
                result.skipped.push(format!("{label}:git-error"));
                continue;
            }
        }
        guard(home, &path)?;
        fs::remove_file(path)?;
        result.removed.push(label);
        result.after_bytes = scan(home, &root, control)?.0;
    }
    result.complete = result.after_bytes <= budget;
    Ok(result)
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    let target = inv
        .required("--target-bytes")?
        .parse::<u64>()
        .ok()
        .filter(|target| *target <= i64::MAX as u64)
        .ok_or_else(|| {
            ToolkitError::arguments(
                "source prune --target-bytes requires a nonnegative signed 64-bit integer",
            )
        })?;
    let data = prune_with_control(&inv.home, target, &inv.control)?;
    let complete = data.complete;
    let mut result = Outcome::new(serde_json::to_value(data)?);
    result.complete = complete;
    if !complete {
        result.warnings.push(json!("Source budget remains above target; inspect skipped owners and preserved unknown checkout bytes."));
    }
    Ok(result)
}
