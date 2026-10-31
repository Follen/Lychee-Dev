use crate::catalog;
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    selection::{SourcePin, valid_digest},
    storage::ExclusiveLock,
};
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub const PARSER: &str = "source-v1";
pub const MAX_SOURCE_BYTES: usize = 32 << 20;
pub fn source_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.starts_with('/')
        && value != "."
        && value != ".."
        && !value.contains(['\\', ':', '\0', '\r', '\n'])
        && value
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
pub fn mirror(home: &Path, key: &str) -> PathBuf {
    home.join("mirrors").join(format!("{key}.git"))
}
/// Read-only repository-format probe. This does not fetch, publish references,
/// authenticate every object, or claim a requested source pin is available.
pub fn mirror_health_with_control(
    home: &Path,
    key: &str,
    product: &str,
    control: &OperationControl,
) -> Result<serde_json::Value> {
    control.check()?;
    let repository = catalog::lookup(key)?;
    if !repository.tracks.contains_key(product) {
        return Err(ToolkitError::new(
            "codebase.unknown_product",
            "Unknown source product",
            2,
        ));
    }
    let path = mirror(home, key);
    lycheedev_core::storage::guard_path(home, &path)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(
                serde_json::json!({"repository":key,"product":product,"exists":false,"scope":"repository-format","objectsVerified":false}),
            );
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Source mirror must be a regular contained directory",
            4,
        ));
    }
    let raw = bytes_with_control(
        Some(&path),
        &["rev-parse", "--is-bare-repository", "--show-object-format"],
        1024,
        control,
    )?;
    if raw != b"true\nsha1\n" && raw != b"true\r\nsha1\r\n" {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Source mirror must use a bare SHA-1 Git repository",
            4,
        ));
    }
    Ok(
        serde_json::json!({"repository":key,"product":product,"exists":true,"bare":true,"objectFormat":"sha1","scope":"repository-format","objectsVerified":false}),
    )
}

/// Git for Windows argv accepts ordinary DOS/UNC spellings, not Rust's
/// canonical verbatim prefix. File identities and containment stay canonical;
/// only the external process argument spelling is converted.
fn ordinary_argument(value: &str) -> std::borrow::Cow<'_, str> {
    if let Some(path) = value.strip_prefix("\\\\?\\UNC\\") {
        std::borrow::Cow::Owned(format!("\\\\{path}"))
    } else if let Some(path) = value.strip_prefix("\\\\?\\") {
        std::borrow::Cow::Borrowed(path)
    } else {
        std::borrow::Cow::Borrowed(value)
    }
}

/// Explicit byte transport, never a shell, no inherited Git routing/config/filter/replacements.
/// Both pipes are drained concurrently with caps. Timeout/output overflow kills and reaps.
pub fn bytes_with_control(
    directory: Option<&Path>,
    args: &[&str],
    cap: usize,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Vec<u8>> {
    control.check()?;
    bytes_controlled(
        directory,
        args,
        cap,
        None,
        control.remaining()?,
        control,
    )
}
fn bytes_controlled(
    directory: Option<&Path>,
    args: &[&str],
    cap: usize,
    input: Option<Vec<u8>>,
    timeout: Duration,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    let cmd = command(directory, args);
    let out = crate::process::run_with_limits(cmd, cap, 32768, timeout, input, control).map_err(
        |mut error| {
            error.code = match error.code.as_str() {
                "source.process_unavailable" => "codebase.source_not_ready",
                "source.process_input_budget" => "codebase.source_budget",
                "process.cancelled" => "command.cancelled",
                _ => "codebase.git_failed",
            }
            .into();
            error
        },
    )?;
    if out.status != 0 {
        return Err(ToolkitError::new(
            "codebase.git_failed",
            String::from_utf8_lossy(&out.stderr).trim(),
            5,
        ));
    }
    Ok(out.stdout)
}
/// Fixed Git-only launch surface for the demand reader. The request stream
/// contains authenticated object IDs, never shell text or revision expressions.
pub(crate) fn batch_command(directory: &Path) -> Command {
    command(Some(directory), &["cat-file", "--batch"])
}
fn command(directory: Option<&Path>, args: &[&str]) -> Command {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut cmd = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.args([
        "-c",
        &format!("core.hooksPath={null}"),
        "-c",
        "protocol.allow=never",
        "-c",
        "protocol.https.allow=always",
        "-c",
        "maintenance.auto=false",
        "-c",
        "gc.auto=0",
    ]);
    if let Some(dir) = directory {
        cmd.arg(format!(
            "--git-dir={}",
            ordinary_argument(&dir.to_string_lossy())
        ));
    }
    for arg in args {
        cmd.arg(&*ordinary_argument(arg));
    }
    for (key, _) in std::env::vars_os() {
        let upper = key.to_string_lossy().to_ascii_uppercase();
        if upper.starts_with("GIT_") || upper == "GCM_INTERACTIVE" {
            cmd.env_remove(key);
        }
    }
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    cmd
}
mod originals;
pub(crate) use originals::{Originals, read_object_frame};
/// Read only objects already authenticated by the pinned immutable tree. One
/// batch is bounded to 64 MiB plus framing; every returned object is rehashed.
pub fn visit_blobs_with_control(
    home: &Path,
    pin: &SourcePin,
    files: &[TreeFile],
    control: &lycheedev_core::control::OperationControl,
    visit: impl FnMut(&TreeFile, Vec<u8>) -> Result<()>,
) -> Result<usize> {
    control.check()?;
    visit_blobs_controlled(home, pin, files, control.deadline(), control, visit)
}
/// Typed boundaries for consumers with pure work in flight. A batch end is
/// visited before its tail is checked and before another Git process starts,
/// exactly where the last synchronous document callback previously completed.
pub(crate) enum BlobBatchVisit<'a> {
    BatchStart { allocation_bytes: usize },
    Blob(&'a TreeFile, Vec<u8>),
    BatchEnd,
}
pub(crate) fn visit_blob_batches_with_control(
    home: &Path,
    pin: &SourcePin,
    files: &[TreeFile],
    control: &OperationControl,
    visit: impl FnMut(BlobBatchVisit<'_>) -> Result<()>,
) -> Result<usize> {
    control.check()?;
    visit_blob_batches_controlled(home, pin, files, control.deadline(), control, visit)
}
fn visit_blobs_controlled(
    home: &Path,
    pin: &SourcePin,
    files: &[TreeFile],
    deadline: std::time::Instant,
    control: &OperationControl,
    mut visit: impl FnMut(&TreeFile, Vec<u8>) -> Result<()>,
) -> Result<usize> {
    visit_blob_batches_controlled(home, pin, files, deadline, control, |event| match event {
        BlobBatchVisit::Blob(file, data) => visit(file, data),
        BlobBatchVisit::BatchStart { .. } | BlobBatchVisit::BatchEnd => Ok(()),
    })
}
fn visit_blob_batches_controlled(
    home: &Path,
    pin: &SourcePin,
    files: &[TreeFile],
    deadline: std::time::Instant,
    control: &lycheedev_core::control::OperationControl,
    mut visit: impl FnMut(BlobBatchVisit<'_>) -> Result<()>,
) -> Result<usize> {
    validate_pin(pin)?;
    let dir = mirror(home, &pin.repository);
    let admitted: Vec<_> = files
        .iter()
        .filter(|f| !f.oversized && (!f.asset || f.bytes <= 64 << 20))
        .collect();
    let mut first = 0;
    let mut batches = 0;
    while first < admitted.len() {
        control.check()?;
        if std::time::Instant::now() >= deadline {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "Source execution deadline exceeded",
                3,
            ));
        }
        let mut last = first;
        let mut size = 0usize;
        while last < admitted.len() {
            let n = admitted[last].bytes;
            if last > first && (size + n > 64 << 20 || last - first >= 4096) {
                break;
            }
            size += n;
            last += 1;
        }
        let input = admitted[first..last]
            .iter()
            .map(|f| format!("{}\n", f.object))
            .collect::<String>()
            .into_bytes();
        let data = bytes_controlled(
            Some(&dir),
            &["cat-file", "--batch"],
            size + (last - first) * 128,
            Some(input),
            deadline.saturating_duration_since(std::time::Instant::now()),
            control,
        )?;
        visit(BlobBatchVisit::BatchStart {
            allocation_bytes: data.capacity(),
        })?;
        visit_batch_data(&data, &admitted[first..last], control, &mut visit)?;
        first = last;
        batches += 1;
    }
    Ok(batches)
}
fn visit_batch_data(
    data: &[u8],
    files: &[&TreeFile],
    control: &OperationControl,
    visit: &mut impl FnMut(BlobBatchVisit<'_>) -> Result<()>,
) -> Result<()> {
    let mut at = 0;
    for f in files {
        control.check()?;
        let end = data[at..]
            .iter()
            .position(|b| *b == b'\n')
            .map(|n| at + n)
            .ok_or_else(|| {
                ToolkitError::new(
                    "codebase.source_integrity",
                    "Missing batch object header",
                    4,
                )
            })?;
        let expected = format!("{} blob {}", f.object, f.bytes);
        if data[at..end] != *expected.as_bytes() {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Batch object identity mismatch",
                4,
            ));
        }
        at = end + 1;
        let end = at
            .checked_add(f.bytes)
            .filter(|n| *n < data.len())
            .ok_or_else(|| {
                ToolkitError::new("codebase.source_integrity", "Truncated batch object", 4)
            })?;
        if data[end] != b'\n' {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Invalid batch terminator",
                4,
            ));
        }
        let blob = &data[at..end];
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", f.bytes));
        hash.update(blob);
        if format!("{:x}", hash.finalize()) != f.object {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Batch blob hash mismatch",
                4,
            ));
        }
        visit(BlobBatchVisit::Blob(f, blob.to_vec()))?;
        at = end + 1;
    }
    visit(BlobBatchVisit::BatchEnd)?;
    if at != data.len() {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Unexpected batch tail",
            4,
        ));
    }
    Ok(())
}
pub fn validate_pin(pin: &SourcePin) -> Result<()> {
    let repo = catalog::lookup(&pin.repository)?;
    // Frozen regional pins retain their original identity and authenticated
    // objects. They are not active tracks and cannot select or sync a branch.
    let retired_forever = pin.repository == "wow-ui-source"
        && matches!(pin.product.as_str(), "forever-asia-beta" | "forever-cn-beta");
    if (!repo.tracks.contains_key(&pin.product) && !retired_forever)
        || !valid_digest(&pin.exact_commit, 40)
        || pin.parser_revision != PARSER
    {
        return Err(ToolkitError::new(
            "codebase.invalid_source_pin",
            "Source pin identity is invalid",
            4,
        ));
    }
    Ok(())
}
pub fn resolve_source_with_control(
    home: &Path,
    key: &str,
    product: &str,
    reference: &str,
    control: &OperationControl,
) -> Result<SourcePin> {
    control.check()?;
    let bytes = |directory: Option<&Path>, args: &[&str], cap: usize| {
        bytes_with_control(directory, args, cap, control)
    };
    let repo = catalog::lookup(key)?;
    let branch = repo.tracks.get(product).ok_or_else(|| {
        ToolkitError::new("codebase.unknown_product", "Unknown source product", 2)
    })?;
    let reference = if reference.is_empty() {
        format!("refs/heads/{branch}")
    } else {
        reference.into()
    };
    let mut invalid_ref = !valid_digest(&reference, 40)
        && !(reference.starts_with("refs/heads/") || reference.starts_with("refs/tags/"));
    if !valid_digest(&reference, 40)
        && !invalid_ref
        && let Err(error) = bytes(None, &["check-ref-format", &reference], 1024)
    {
        control.check()?;
        if error.exit_code == 7 {
            return Err(error);
        }
        invalid_ref = true;
    }
    if invalid_ref {
        return Err(ToolkitError::new(
            "codebase.invalid_source_ref",
            "Full branch/tag ref or exact commit required",
            2,
        ));
    }
    fs::create_dir_all(home.join("mirrors"))?;
    let _lock = ExclusiveLock::acquire(&home.join("locks").join(format!("source-{key}.lock")))?;
    let destination = mirror(home, key);
    let stage = home
        .join("mirrors")
        .join(format!(".source-{key}-{}", std::process::id()));
    let created = !destination.exists();
    let dir = if created { &stage } else { &destination };
    if created {
        if stage.exists() {
            return Err(ToolkitError::new(
                "codebase.source_not_ready",
                "Source staging path already exists",
                3,
            ));
        }
        bytes(
            None,
            &[
                "init",
                "--bare",
                "--object-format=sha1",
                &stage.to_string_lossy(),
            ],
            4096,
        )?;
    } else if !fs::symlink_metadata(dir)?.is_dir() {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Invalid mirror directory",
            4,
        ));
    }
    let result = (|| {
        bytes(
            Some(dir),
            &[
                "-c",
                "fetch.fsckObjects=true",
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-recurse-submodules",
                "--depth=1",
                &repo.url,
                &reference,
            ],
            4096,
        )?;
        let commit = String::from_utf8_lossy(&bytes(
            Some(dir),
            &["rev-parse", "--verify", "FETCH_HEAD^{commit}"],
            128,
        )?)
        .trim()
        .to_owned();
        if !valid_digest(&commit, 40) || (valid_digest(&reference, 40) && commit != reference) {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Resolved source commit mismatch",
                4,
            ));
        }
        bytes(
            Some(dir),
            &["update-ref", &format!("refs/pins/{commit}"), &commit],
            1024,
        )?;
        if created {
            fs::rename(dir, &destination)?;
        }
        Ok(SourcePin {
            repository: key.into(),
            product: product.into(),
            requested_ref: reference,
            exact_commit: commit,
            parser_revision: PARSER.into(),
        })
    })();
    if created && stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
#[derive(Debug, Clone)]
pub struct TreeFile {
    pub path: String,
    pub object: String,
    pub bytes: usize,
    pub asset: bool,
    pub oversized: bool,
}
pub fn is_asset(ext: &str) -> bool {
    [
        "blp", "tga", "png", "jpg", "jpeg", "dds", "ttf", "otf", "mp3", "ogg", "wav", "m2", "wmo",
    ]
    .contains(&ext)
}
pub fn tree_with_control(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Vec<TreeFile>> {
    tree_listing(home, pin, false, control)
}
/// Complete regular-file checkout admission, including files outside source
/// query extensions. Symlinks/submodules fail admission instead of disappearing.
pub fn regular_tree_with_control(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Vec<TreeFile>> {
    tree_listing(home, pin, true, control)
}
fn tree_listing(
    home: &Path,
    pin: &SourcePin,
    all_regular: bool,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Vec<TreeFile>> {
    validate_pin(pin)?;
    let dir = mirror(home, &pin.repository);
    control.check()?;
    let raw = bytes_controlled(
        Some(&dir),
        &[
            "ls-tree",
            "-r",
            "-l",
            "-z",
            "--full-tree",
            &pin.exact_commit,
        ],
        32 << 20,
        None,
        control.remaining()?,
        control,
    )?;
    let mut files = Vec::new();
    let (mut documents, mut assets, mut total) = (0, 0, 0usize);
    for entry in raw.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        control.check()?;
        let Some(split) = entry.iter().position(|b| *b == b'\t') else {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Invalid Git tree record",
                4,
            ));
        };
        let header = std::str::from_utf8(&entry[..split])
            .map_err(|_| ToolkitError::invalid("Invalid Git tree encoding"))?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 4 {
            return Err(ToolkitError::invalid("Invalid Git tree fields"));
        }
        if fields[0] != "100644" && fields[0] != "100755" {
            if all_regular {
                return Err(ToolkitError::new(
                    "codebase.worktree_invalid",
                    "Pinned checkout contains a nonregular Git mode",
                    4,
                ));
            }
            continue;
        }
        let path = std::str::from_utf8(&entry[split + 1..])
            .map_err(|_| ToolkitError::invalid("Invalid source path encoding"))?;
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let doc = ["lua", "xml", "toc"].contains(&ext.as_str());
        let asset = is_asset(&ext);
        if !all_regular && !doc && !asset {
            continue;
        }
        let size: usize = fields[3]
            .parse()
            .map_err(|_| ToolkitError::invalid("Invalid Git blob size"))?;
        if fields[1] != "blob" || !valid_digest(fields[2], 40) || !source_path(path) {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Invalid source tree object",
                4,
            ));
        }
        if asset {
            assets += 1;
        } else {
            documents += 1;
        }
        let oversized = doc && size > MAX_SOURCE_BYTES;
        if all_regular && size > 128 << 20 {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "Checkout file byte budget exceeded",
                3,
            ));
        }
        if all_regular || !oversized && (!asset || size <= 64 << 20) {
            total = total.checked_add(size).ok_or_else(|| {
                ToolkitError::new("codebase.source_budget", "Source tree overflow", 3)
            })?;
        }
        if all_regular && (files.len() >= 200000 || total > 4usize << 30)
            || !all_regular && (documents > 100000 || assets > 200000 || total > 1 << 30)
        {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "Source tree budget exceeded",
                3,
            ));
        }
        files.push(TreeFile {
            path: path.into(),
            object: fields[2].into(),
            bytes: size,
            asset,
            oversized,
        });
    }
    Ok(files)
}
/// Verify one admitted checkout file against the Git framed blob hash. The
/// caller must guard its managed checkout boundary; this helper rejects links,
/// nonregular files, length changes and differing content.
pub fn verify_file_with_control(
    path: &Path,
    file: &TreeFile,
    max: usize,
    control: &lycheedev_core::control::OperationControl,
) -> Result<()> {
    verify_file_controlled(path, file, max, control)
}
fn verify_file_controlled(
    path: &Path,
    file: &TreeFile,
    max: usize,
    control: &lycheedev_core::control::OperationControl,
) -> Result<()> {
    if file.bytes > max || max > 128 << 20 || !valid_digest(&file.object, 40) {
        return Err(ToolkitError::new(
            "codebase.source_budget",
            "Checkout verification byte budget exceeded",
            3,
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != file.bytes as u64
    {
        return Err(ToolkitError::new(
            "codebase.worktree_modified",
            "Checkout file mode or byte count differs",
            4,
        ));
    }
    let mut input = fs::File::open(path)?;
    let result = (|| {
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", file.bytes).as_bytes());
        let deadline = std::time::Instant::now() + Duration::from_secs(180);
        let mut count = 0usize;
        let mut buffer = [0u8; 65536];
        loop {
            control.check()?;
            if std::time::Instant::now() >= deadline {
                return Err(ToolkitError::new(
                    "codebase.source_budget",
                    "Checkout verification deadline exceeded",
                    3,
                ));
            }
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            count = count
                .checked_add(n)
                .ok_or_else(|| ToolkitError::invalid("Checkout byte count overflow"))?;
            if count > file.bytes {
                return Err(ToolkitError::new(
                    "codebase.worktree_modified",
                    "Checkout file grew during verification",
                    4,
                ));
            }
            hash.update(&buffer[..n]);
        }
        if count != file.bytes
            || input.metadata()?.len() != file.bytes as u64
            || format!("{:x}", hash.finalize()) != file.object
        {
            return Err(ToolkitError::new(
                "codebase.worktree_modified",
                "Checkout file content differs from pinned Git blob",
                4,
            ));
        }
        Ok(())
    })();
    lycheedev_core::storage::finish_file(input, result, control)
}
pub fn blob_with_control(
    home: &Path,
    pin: &SourcePin,
    path: &str,
    max: usize,
    control: &lycheedev_core::control::OperationControl,
) -> Result<(Vec<u8>, String)> {
    control.check()?;
    blob_controlled(home, pin, path, max, MAX_SOURCE_BYTES, control)
}
/// Asset originals have the existing 64 MiB archive bound, independently of
/// the 32 MiB document-analysis bound. Both routes authenticate the same pinned
/// commit/tree chain and regular-file leaf before exposing any bytes.
pub(crate) fn asset_blob_with_control(
    home: &Path,
    pin: &SourcePin,
    path: &str,
    max: usize,
    control: &OperationControl,
) -> Result<(Vec<u8>, String)> {
    control.check()?;
    blob_controlled(
        home,
        pin,
        path,
        max,
        crate::assets::MAX_BYTES,
        control,
    )
}
fn blob_controlled(
    home: &Path,
    pin: &SourcePin,
    path: &str,
    max: usize,
    maximum: usize,
    control: &lycheedev_core::control::OperationControl,
) -> Result<(Vec<u8>, String)> {
    validate_pin(pin)?;
    if !source_path(path) {
        return Err(ToolkitError::new(
            "codebase.invalid_span",
            "Invalid source path",
            2,
        ));
    }
    let dir = mirror(home, &pin.repository);
    // One process returns the commit, every parent tree and the leaf. Verify
    // framed Git SHA-1 AND each tree edge, retaining regular-file admission.
    // Resolving commit:path as a blob alone would accept a symlink's bytes.
    let components: Vec<_> = path.split('/').collect();
    if components.len() > 64 || max > maximum {
        return Err(ToolkitError::new(
            "codebase.source_budget",
            "Source path/decode budget exceeded",
            3,
        ));
    }
    let mut requests = format!("{}\n{}^{{tree}}\n", pin.exact_commit, pin.exact_commit);
    for end in 1..components.len() {
        requests.push_str(&format!(
            "{}:{}\n",
            pin.exact_commit,
            components[..end].join("/")
        ));
    }
    requests.push_str(&format!("{}:{path}\n", pin.exact_commit));
    let batch = bytes_controlled(
        Some(&dir),
        &["cat-file", "--batch"],
        max + (16 << 20) + (1 << 20) + 8192,
        Some(requests.into_bytes()),
        control.remaining()?,
        control,
    )?;
    let mut at = 0;
    let commit = take_batch_object(&batch, &mut at, &pin.exact_commit, "commit", 1 << 20)?;
    let commit_text =
        std::str::from_utf8(&commit).map_err(|_| ToolkitError::invalid("Invalid commit text"))?;
    let mut expected = commit_text
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("tree "))
        .filter(|hash| valid_digest(hash, 40))
        .ok_or_else(|| ToolkitError::invalid("Commit omitted tree identity"))?
        .to_owned();
    let mut tree_bytes = 0usize;
    for (i, component) in components.iter().enumerate() {
        control.check()?;
        let tree = take_batch_object(&batch, &mut at, &expected, "tree", 4 << 20)?;
        tree_bytes += tree.len();
        if tree_bytes > 16 << 20 {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "Parent tree byte budget exceeded",
                3,
            ));
        }
        let mut cursor = 0;
        let mut found = None;
        while cursor < tree.len() {
            let end = tree[cursor..]
                .iter()
                .position(|b| *b == 0)
                .map(|n| cursor + n)
                .ok_or_else(|| ToolkitError::invalid("Truncated tree entry"))?;
            let hash_end = end
                .checked_add(21)
                .filter(|n| *n <= tree.len())
                .ok_or_else(|| ToolkitError::invalid("Truncated tree object identity"))?;
            let entry = &tree[cursor..end];
            let split = entry
                .iter()
                .position(|b| *b == b' ')
                .ok_or_else(|| ToolkitError::invalid("Invalid tree mode"))?;
            if &entry[split + 1..] == component.as_bytes() {
                if found.is_some() {
                    return Err(ToolkitError::invalid("Duplicate tree path"));
                }
                let mode = &entry[..split];
                let admitted = if i + 1 == components.len() {
                    mode == b"100644" || mode == b"100755"
                } else {
                    mode == b"40000"
                };
                if !admitted {
                    return Err(ToolkitError::new(
                        "codebase.source_integrity",
                        "Pinned path is not a regular source file",
                        4,
                    ));
                }
                found = Some(
                    tree[end + 1..hash_end]
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>(),
                );
            }
            cursor = hash_end;
        }
        expected = found.ok_or_else(|| {
            ToolkitError::new("codebase.source_not_ready", "Pinned file is absent", 3)
        })?;
    }
    let data = take_batch_object(&batch, &mut at, &expected, "blob", max)?;
    if at != batch.len() {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Git blob content integrity failed",
            4,
        ));
    }
    Ok((data, expected))
}
fn take_batch_object(
    raw: &[u8],
    at: &mut usize,
    object: &str,
    kind: &str,
    max: usize,
) -> Result<Vec<u8>> {
    let header_end = raw[*at..]
        .iter()
        .position(|b| *b == b'\n')
        .map(|n| *at + n)
        .ok_or_else(|| {
            ToolkitError::new("codebase.source_integrity", "Missing Git batch header", 4)
        })?;
    let header = std::str::from_utf8(&raw[*at..header_end]).map_err(|_| {
        ToolkitError::new("codebase.source_integrity", "Invalid Git batch header", 4)
    })?;
    let fields: Vec<_> = header.split(' ').collect();
    if fields.len() != 3 || fields[0] != object || fields[1] != kind {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Pinned object type or identity differs",
            4,
        ));
    }
    let size: usize = fields[2]
        .parse()
        .map_err(|_| ToolkitError::new("codebase.source_integrity", "Invalid Git batch size", 4))?;
    if size > max {
        return Err(ToolkitError::new(
            "codebase.source_budget",
            "Git object budget exceeded",
            3,
        ));
    }
    let start = header_end + 1;
    let end = start
        .checked_add(size)
        .filter(|end| *end < raw.len())
        .ok_or_else(|| {
            ToolkitError::new("codebase.source_integrity", "Truncated Git batch bytes", 4)
        })?;
    if raw[end] != b'\n' {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Invalid Git batch separator",
            4,
        ));
    }
    let mut hash = Sha1::new();
    hash.update(format!("{kind} {size}\0"));
    hash.update(&raw[start..end]);
    if format!("{:x}", hash.finalize()) != object {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Pinned Git object hash differs",
            4,
        ));
    }
    *at = end + 1;
    Ok(raw[start..end].to_vec())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_forever_beta_pins_remain_readable_without_active_track_admission() {
        let home = Path::new("unused-retired-forever-admission");
        let control = OperationControl::default();
        for product in ["forever-asia-beta", "forever-cn-beta"] {
            let pin = SourcePin { repository:"wow-ui-source".into(), product:product.into(),
                requested_ref:"refs/heads/forever".into(), exact_commit:"a".repeat(40), parser_revision:PARSER.into() };
            validate_pin(&pin).unwrap();
            assert_eq!(resolve_source_with_control(home,"wow-ui-source",product,&pin.exact_commit,&control).unwrap_err().code,"codebase.unknown_product");
            assert_eq!(mirror_health_with_control(home,"wow-ui-source",product,&control).unwrap_err().code,"codebase.unknown_product");
            assert_eq!(blob_with_control(home,&pin,"../Fixture.lua",MAX_SOURCE_BYTES,&control).unwrap_err().code,"codebase.invalid_span");
            for invalid in [
                SourcePin { product:"forever-unknown-beta".into(), ..pin.clone() },
                SourcePin { repository:"elvui".into(), ..pin.clone() },
                SourcePin { exact_commit:"a".repeat(39), ..pin.clone() },
                SourcePin { parser_revision:"retired-parser".into(), ..pin.clone() },
            ] { assert_eq!(validate_pin(&invalid).unwrap_err().code,"codebase.invalid_source_pin"); }
        }
    }

    #[test]
    fn asset_recovery_budget_keeps_document_requests_paths_and_control_bounded() {
        let home = Path::new("unused-asset-budget-home");
        let pin = SourcePin {
            repository: "elvui".into(),
            product: "main".into(),
            requested_ref: "fixture".into(),
            exact_commit: "a".repeat(40),
            parser_revision: PARSER.into(),
        };
        let control = OperationControl::default();
        // These are request-limit checks before any Git access, independently
        // of the size of the object that might exist at the pinned path.
        for maximum in [MAX_SOURCE_BYTES + 1, crate::assets::MAX_BYTES] {
            assert_eq!(
                blob_with_control(home, &pin, "Image.png", maximum, &control)
                    .unwrap_err()
                    .code,
                "codebase.source_budget"
            );
        }
        assert_eq!(
            asset_blob_with_control(
                home,
                &pin,
                "Image.png",
                crate::assets::MAX_BYTES + 1,
                &control,
            )
            .unwrap_err()
            .code,
            "codebase.source_budget"
        );
        for path in ["../Image.png", "Assets\\Image.png", "Assets/Image.png\n"] {
            assert_eq!(
                asset_blob_with_control(home, &pin, path, crate::assets::MAX_BYTES, &control)
                    .unwrap_err()
                    .code,
                "codebase.invalid_span"
            );
        }
        let deep = format!("{}Image.png", "dir/".repeat(64));
        assert_eq!(
            asset_blob_with_control(home, &pin, &deep, crate::assets::MAX_BYTES, &control)
                .unwrap_err()
                .code,
            "codebase.source_budget"
        );
        for (control, code) in [
            (
                OperationControl::uncancelable(std::time::Instant::now()),
                "command.deadline",
            ),
            (
                crate::native::stopped_control(false, std::time::Instant::now() + Duration::from_secs(60)),
                "command.cancelled",
            ),
        ] {
            assert_eq!(
                asset_blob_with_control(home, &pin, "../Image.png", usize::MAX, &control)
                    .unwrap_err()
                    .code,
                code
            );
        }
    }

    #[test]
    fn asset_recovery_frame_authenticates_above_source_limit_and_rejects_oversize() {
        // The production decoder sees actual bytes above 32 MiB. The same
        // frame remains forbidden to document callers, while the existing
        // asset bound admits it with the full Git SHA-1 and framing checks.
        let size = MAX_SOURCE_BYTES + 1;
        let raw = vec![0xa5; size];
        let mut hash = Sha1::new();
        hash.update(format!("blob {size}\0"));
        hash.update(&raw);
        let object = format!("{:x}", hash.finalize());
        let mut frame = format!("{object} blob {size}\n").into_bytes();
        let start = frame.len();
        frame.extend_from_slice(&raw);
        frame.push(b'\n');
        drop(raw);
        let mut at = 0;
        assert_eq!(
            take_batch_object(&frame, &mut at, &object, "blob", MAX_SOURCE_BYTES)
                .unwrap_err()
                .code,
            "codebase.source_budget"
        );
        assert_eq!(at, 0);
        let actual =
            take_batch_object(&frame, &mut at, &object, "blob", crate::assets::MAX_BYTES).unwrap();
        assert_eq!(actual, frame[start..start + size]);
        assert_eq!(at, frame.len());
        drop(actual);
        frame[start + size - 1] ^= 1;
        assert_eq!(
            take_batch_object(&frame, &mut 0, &object, "blob", crate::assets::MAX_BYTES)
                .unwrap_err()
                .code,
            "codebase.source_integrity"
        );
        drop(frame);
        let oversized = format!("{object} blob {}\n", crate::assets::MAX_BYTES + 1);
        // A declared oversized frame is refused before its body is needed.
        assert_eq!(
            take_batch_object(
                oversized.as_bytes(),
                &mut 0,
                &object,
                "blob",
                crate::assets::MAX_BYTES,
            )
            .unwrap_err()
            .code,
            "codebase.source_budget"
        );
    }

    #[test]
    fn actual_batch_transport_matches_immutable_reader_and_rejects_identity_change() {
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/rust-source-batch-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        assert_eq!(
            mirror_health_with_control(&home, "wow-ui-source", "retail", &Default::default())
                .unwrap()["exists"],
            false
        );
        fs::create_dir_all(home.join("mirrors")).unwrap();
        let dir = mirror(&home, "wow-ui-source");
        bytes_with_control(
            None,
            &[
                "init",
                "--bare",
                "--object-format=sha1",
                &dir.to_string_lossy(),
            ],
            4096, &Default::default(),
        )
        .unwrap();
        let health =
            mirror_health_with_control(&home, "wow-ui-source", "retail", &Default::default())
                .unwrap();
        assert_eq!(health["bare"], true);
        assert_eq!(health["objectFormat"], "sha1");
        assert_eq!(health["objectsVerified"], false);
        let input = b"function Exact()\r\n return 2\r\nend\r\n";
        let object = String::from_utf8(
            bytes_controlled(
                Some(&dir),
                &["hash-object", "-w", "--stdin"],
                128,
                Some(input.to_vec()), Duration::from_secs(180), &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
        let object = object.trim();
        let tree = String::from_utf8(
            bytes_controlled(
                Some(&dir),
                &["mktree"],
                128,
                Some(format!("100644 blob {object}\tFixture.lua\n").into_bytes()), Duration::from_secs(180), &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
        let tree = tree.trim();
        let commit = String::from_utf8(
            bytes_with_control(
                Some(&dir),
                &[
                    "-c",
                    "user.name=Rust fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit-tree",
                    tree,
                    "-m",
                    "isolated immutable source fixture",
                ],
                128, &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
        let pin = SourcePin {
            repository: "wow-ui-source".into(),
            product: "retail".into(),
            requested_ref: "fixture".into(),
            exact_commit: commit.trim().into(),
            parser_revision: PARSER.into(),
        };
        // The old product remains part of its frozen pin; the same actual
        // commit/tree/blob authentication reads its original object unchanged.
        for product in ["forever-asia-beta", "forever-cn-beta"] {
            let frozen = SourcePin { product:product.into(), ..pin.clone() };
            let files = tree_with_control(&home,&frozen,&Default::default()).unwrap();
            assert_eq!(files.len(),1);
            assert_eq!(files[0].path,"Fixture.lua");
            assert_eq!(files[0].object,object);
            let (raw,hash) = blob_with_control(&home,&frozen,"Fixture.lua",MAX_SOURCE_BYTES,&Default::default()).unwrap();
            assert_eq!(raw,input);
            assert_eq!(hash,object);
            let mut tampered = files;
            tampered[0].bytes += 1;
            assert_eq!(visit_blobs_with_control(&home,&frozen,&tampered,&Default::default(),|_,_|Ok(())).unwrap_err().code,"codebase.source_integrity");
        }
        let canonical_home = fs::canonicalize(&home).unwrap();
        let canonical_files = tree_with_control(&canonical_home, &pin, &Default::default()).unwrap();
        let files = tree_with_control(&home, &pin, &Default::default()).unwrap();
        assert_eq!(canonical_files[0].object, files[0].object);
        let mut canonical_bytes = vec![];
        visit_blobs_with_control(&canonical_home, &pin, &canonical_files, &Default::default(), |_, bytes| {
            canonical_bytes = bytes;
            Ok(())
        })
        .unwrap();
        assert_eq!(canonical_bytes, input);
        let mut observed = vec![];
        visit_blobs_with_control(&home, &pin, &files, &Default::default(), |f, data| {
            observed.push((f.path.clone(), data));
            Ok(())
        })
        .unwrap();
        assert_eq!(observed, vec![("Fixture.lua".into(), input.to_vec())]);
        // A real transport boundary occurs at 4,096 objects, even when many
        // admitted paths happen to address the same immutable content.
        let mut repeated = vec![files[0].clone(); 4097];
        let mut documents = 0usize;
        let mut boundaries = 0usize;
        let batches = visit_blob_batches_with_control(
            &home,
            &pin,
            &repeated,
            &OperationControl::default(),
            |event| {
                match event {
                    BlobBatchVisit::Blob(_, raw) => {
                        assert_eq!(raw, input);
                        documents += 1;
                    }
                    BlobBatchVisit::BatchStart { allocation_bytes } => {
                        assert!(allocation_bytes > 0)
                    }
                    BlobBatchVisit::BatchEnd => boundaries += 1,
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!((batches, documents, boundaries), (2, 4097, 2));
        // A first-batch publication failure must be returned before attempting
        // the missing Git object in the following batch.
        repeated[4096].object = "0".repeat(40);
        let mut documents = 0usize;
        let error = visit_blob_batches_with_control(
            &home,
            &pin,
            &repeated,
            &OperationControl::default(),
            |event| {
                match event {
                    BlobBatchVisit::Blob(_, _) => documents += 1,
                    BlobBatchVisit::BatchStart { .. } => {}
                    BlobBatchVisit::BatchEnd => {
                        return Err(ToolkitError::new(
                            "test.first_publication",
                            "first batch commit",
                            5,
                        ));
                    }
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "test.first_publication");
        assert_eq!(documents, 4096);

        assert_eq!(
            blob_with_control(&home, &pin, "Fixture.lua", MAX_SOURCE_BYTES, &Default::default())
                .unwrap()
                .0,
            input
        );
        let status_control = OperationControl::default();
        let before_status =
            crate::query_store::status_with_control(&home, &pin, &status_control).unwrap();
        assert_eq!(before_status["ready"], false);
        let first_index = crate::index::build_with_control(&home, &pin, &Default::default()).unwrap();
        let status = crate::query_store::status_with_control(&home, &pin, &status_control).unwrap();
        assert_eq!(status["ready"], true);
        assert_eq!(status["snapshotFiles"], 1);
        assert_eq!(status["astFiles"], 1);
        assert_eq!(status["storage"], "file-cache");
        assert_eq!(first_index.summary.io["gitBlobProcesses"], 1);
        assert_eq!(first_index.summary.io["parsedDocuments"], 1);
        let first_query = first_index
            .search(&pin, "PIN-fixture", "Exact", "precise", "lua", 1, "")
            .unwrap();
        assert_eq!(first_query["results"][0]["matchedBy"], "exact_symbol");
        assert_eq!(first_query["results"][0]["sourceSpan"]["startByte"], 0);
        assert_eq!(
            first_query["results"][0]["sourceSpan"]["endByte"],
            input.len()
        );
        assert_eq!(first_query["analysis"]["semantic"], "not_requested");
        assert!(serde_json::to_vec(&first_query).unwrap().len() < 4096);
        let hot = crate::index::build_with_control(&home, &pin, &Default::default()).unwrap();
        assert_eq!(hot.summary.io["cachedFacts"], 1);
        assert_eq!(hot.symbols[0].id, first_index.symbols[0].id);
        let hot_query = hot
            .search(&pin, "PIN-fixture", "Exact", "precise", "lua", 1, "")
            .unwrap();
        assert_eq!(hot_query["results"], first_query["results"]);
        assert_eq!(hot_query["coverage"]["io"]["sourceDocumentsRead"], 1);
        let cache = fs::read_dir(home.join("cache/source-facts-rust"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        // Intentionally obsolete schema/version literals below are negative-case
        // inputs: they must stay cache misses/rebuilds as the current formats move.
        lycheedev_core::storage::atomic_json(&cache,&serde_json::json!({"schema":"lycheedev.source-facts.rust.v2","facts":{"legacyLayout":true}})).unwrap();
        let mut metadata = lycheedev_core::metadata::Metadata::open(&home, true).unwrap();
        let mut old_index: serde_json::Value = metadata
            .list("source/rust-index/", 1)
            .unwrap()
            .pop()
            .unwrap();
        // A stale index schema literal must be rebuilt, never accepted as ready.
        old_index["schema"] = serde_json::json!("lycheedev.source-index.rust.v3");
        let key = format!(
            "source/rust-index/{}",
            old_index["identity"].as_str().unwrap()
        );
        let generation = metadata.generation(&key).unwrap();
        metadata.put(&key, &old_index, generation).unwrap();
        let rebuilt = crate::index::build_with_control(&home, &pin, &Default::default()).unwrap();
        assert_eq!(rebuilt.summary.io["parsedDocuments"], 1);
        assert_eq!(rebuilt.symbols[0].id, first_index.symbols[0].id);
        let mut tampered = files.clone();
        tampered[0].bytes += 1;
        assert_eq!(
            visit_blobs_with_control(&home, &pin, &tampered, &Default::default(), |_, _| Ok(()))
                .unwrap_err()
                .code,
            "codebase.source_integrity"
        );
        drop(metadata);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn path_admission() {
        for p in [
            "",
            "../a.lua",
            "a/../b.lua",
            "/a.lua",
            "a\\b.lua",
            "a//b.lua",
            "a\0.lua",
            "C:a.lua",
        ] {
            assert!(!source_path(p));
        }
        assert!(source_path("Interface/FrameXML/UIParent.lua"));
    }
}

#[cfg(test)]
mod batch_boundary_tests {
    use super::*;

    fn fixture() -> (TreeFile, Vec<u8>) {
        let raw = b"Run()\n";
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", raw.len()));
        hash.update(raw);
        let object = format!("{:x}", hash.finalize());
        let mut batch = format!("{object} blob {}\n", raw.len()).into_bytes();
        batch.extend_from_slice(raw);
        batch.push(b'\n');
        (
            TreeFile {
                path: "One.lua".into(),
                object,
                bytes: raw.len(),
                asset: false,
                oversized: false,
            },
            batch,
        )
    }
    #[test]
    fn authenticated_document_precedes_its_final_batch_boundary() {
        let (file, batch) = fixture();
        let mut order = Vec::new();
        visit_batch_data(&batch, &[&file], &Default::default(), &mut |event| {
            order.push(match event {
                BlobBatchVisit::Blob(f, raw) => {
                    assert_eq!(f.path, "One.lua");
                    assert_eq!(raw, b"Run()\n");
                    "document"
                }
                BlobBatchVisit::BatchStart { .. } => {
                    panic!("transport start is outside frame decoder")
                }
                BlobBatchVisit::BatchEnd => "batch_end",
            });
            Ok(())
        })
        .unwrap();
        assert_eq!(order, ["document", "batch_end"]);
    }
    #[test]
    fn original_document_commit_error_precedes_a_bad_batch_tail() {
        let (file, mut batch) = fixture();
        batch.extend_from_slice(b"unexpected");
        let error = visit_batch_data(&batch, &[&file], &Default::default(), &mut |event| match event {
            BlobBatchVisit::Blob(_, _) | BlobBatchVisit::BatchStart { .. } => Ok(()),
            BlobBatchVisit::BatchEnd => Err(ToolkitError::new(
                "test.commit",
                "original commit failure",
                5,
            )),
        })
        .unwrap_err();
        assert_eq!(error.code, "test.commit");
        let error = visit_batch_data(&batch, &[&file], &Default::default(), &mut |_| Ok(())).unwrap_err();
        assert_eq!(error.code, "codebase.source_integrity");
        assert_eq!(error.message, "Unexpected batch tail");
    }
    #[test]
    fn unauthenticated_document_never_reaches_either_event() {
        let (file, mut batch) = fixture();
        let raw_start = batch.iter().position(|b| *b == b'\n').unwrap() + 1;
        batch[raw_start] = b'X';
        let mut events = 0;
        let error = visit_batch_data(&batch, &[&file], &Default::default(), &mut |_| {
            events += 1;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.code, "codebase.source_integrity");
        assert_eq!(events, 0);
    }
}
