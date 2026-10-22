//! Only cache/objects and explicitly selected staging are reclaimable. Evidence blobs are separate.
use crate::{
    Outcome, Result, ToolkitError,
    selection::valid_digest,
    storage::{self, ExclusiveLock},
    workspace,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
mod source_facts;
pub use source_facts::{
    SourceCachePublicationPhase, SourceFactsCache, SourceFactsReader, SourceFactsWriter,
    SourceOriginalCache,
};
mod pruning;
mod publication;
pub use publication::publish_derived_controlled;
mod verification;
pub(crate) use verification::{verify, walk_files};
#[derive(Clone)]
struct Object {
    digest: String,
    path: PathBuf,
    bytes: u64,
    modified: std::time::SystemTime,
}
fn objects(home: &Path) -> Result<(Vec<Object>, usize)> {
    objects_checked(home, || Ok(()))
}
fn objects_checked(
    home: &Path,
    mut check: impl FnMut() -> Result<()>,
) -> Result<(Vec<Object>, usize)> {
    check()?;
    let root = home.join("cache/objects");
    let mut result = vec![];
    let mut foreign = 0;
    if !root.exists() {
        return Ok((result, foreign));
    }
    storage::guard_path_with_check(home, &root, &mut check)?;
    for bucket in fs::read_dir(&root)? {
        check()?;
        let bucket = bucket?;
        let prefix = bucket.file_name().to_string_lossy().into_owned();
        if prefix.len() != 2 || !bucket.file_type()?.is_dir() {
            foreign += 1;
            continue;
        }
        storage::guard_path_with_check(home, &bucket.path(), &mut check)?;
        for file in fs::read_dir(bucket.path())? {
            check()?;
            let file = file?;
            let digest = format!("{prefix}{}", file.file_name().to_string_lossy());
            if !valid_digest(&digest, 64) || !file.file_type()?.is_file() {
                foreign += 1;
                continue;
            }
            storage::guard_path_with_check(home, &file.path(), &mut check)?;
            let info = file.metadata()?;
            result.push(Object {
                digest,
                path: file.path(),
                bytes: info.len(),
                modified: info.modified()?,
            });
            if result.len() > 100_000 {
                return Err(ToolkitError::new(
                    "vault.cache_scan_limit",
                    "cache scan exceeded 100000 objects",
                    4,
                ));
            }
        }
    }
    check()?;
    result.sort_by_key(|o| (o.modified, o.digest.clone()));
    check()?;
    Ok((result, foreign))
}
fn retain(home: &Path, digest: &str) -> Result<Option<Value>> {
    retain_checked(home, digest, || Ok(()))
}
const MAX_RETAIN_BYTES: usize = 65536;
#[cfg(test)]
std::thread_local! {
    static RETAIN_READ_HOOK: std::cell::RefCell<Option<Box<dyn FnMut(usize)>>> =
        std::cell::RefCell::new(None);
}
fn retain_checked(
    home: &Path,
    digest: &str,
    mut check: impl FnMut() -> Result<()>,
) -> Result<Option<Value>> {
    check()?;
    let path = home.join("cache/retain").join(format!("{digest}.json"));
    if !path.exists() {
        return Ok(None);
    }
    storage::guard_path_with_check(home, &path, &mut check)?;
    let mut file = fs::File::open(&path)?;
    let result = (|| {
        use std::io::Read;
        let invalid = || ToolkitError::new("vault.cache_protection", "invalid retention marker", 4);
        let stat = file.metadata()?;
        if !stat.is_file() || stat.len() > MAX_RETAIN_BYTES as u64 {
            return Err(invalid());
        }
        // Read at most limit + 1 through the same admitted handle. A marker
        // growing after stat cannot turn into an unbounded allocation or an
        // apparently unprotected object.
        let mut raw = Vec::with_capacity(MAX_RETAIN_BYTES + 1);
        let mut chunk = [0; 8192];
        while raw.len() <= MAX_RETAIN_BYTES {
            #[cfg(test)]
            RETAIN_READ_HOOK.with(|hook| {
                if let Some(hook) = hook.borrow_mut().as_mut() {
                    hook(raw.len());
                }
            });
            check()?;
            let allowed = chunk.len().min(MAX_RETAIN_BYTES + 1 - raw.len());
            let n = file.read(&mut chunk[..allowed])?;
            raw.extend_from_slice(&chunk[..n]);
            check()?;
            if n == 0 {
                break;
            }
        }
        if raw.len() > MAX_RETAIN_BYTES {
            return Err(invalid());
        }
        let value: Value = serde_json::from_slice(&raw).map_err(|_| invalid())?;
        check()?;
        if value["sha256"] != digest
            || value["owners"]
                .as_array()
                .is_none_or(|owners| owners.len() > 1024)
        {
            return Err(invalid());
        }
        for owner in value["owners"].as_array().unwrap() {
            check()?;
            validate_owner(owner)?;
        }
        check()?;
        Ok(Some(value))
    })();
    let result = crate::selection::selection_finish(
        result,
        storage::close_file(file).map_err(ToolkitError::io),
    );
    crate::selection::selection_finish(result, check())
}
fn validate_owner(owner: &Value) -> Result<()> {
    if !["pinned", "evidence", "operation"].contains(&owner["kind"].as_str().unwrap_or(""))
        || owner["owner"]
            .as_str()
            .is_none_or(|name| name.is_empty() || name.len() > 512)
        || owner["retainedAt"]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .is_none()
    {
        return Err(ToolkitError::new(
            "vault.cache_protection",
            "invalid cache protective owner",
            4,
        ));
    }
    Ok(())
}
fn markers(home: &Path) -> Result<(Vec<Value>, Vec<String>, usize)> {
    markers_checked(home, || Ok(()))
}
fn markers_checked(
    home: &Path,
    mut check: impl FnMut() -> Result<()>,
) -> Result<(Vec<Value>, Vec<String>, usize)> {
    check()?;
    let root = home.join("cache/retain");
    let mut values = vec![];
    let mut missing = vec![];
    let mut foreign = 0;
    if !root.exists() {
        return Ok((values, missing, foreign));
    }
    storage::guard_path_with_check(home, &root, &mut check)?;
    for entry in fs::read_dir(&root)? {
        check()?;
        let entry = entry?;
        if values.len() + foreign > 100000 {
            return Err(ToolkitError::new(
                "vault.cache_scan_limit",
                "retention scan limit exceeded",
                4,
            ));
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(digest) = name
            .strip_suffix(".json")
            .filter(|digest| valid_digest(digest, 64))
        else {
            foreign += 1;
            continue;
        };
        let value = retain_checked(home, digest, &mut check)?
            .ok_or_else(|| ToolkitError::invalid("retention marker disappeared"))?;
        if !home
            .join("cache/objects")
            .join(&digest[..2])
            .join(&digest[2..])
            .exists()
        {
            missing.push(digest.to_owned());
        }
        values.push(value);
    }
    check()?;
    Ok((values, missing, foreign))
}
fn stages(home: &Path) -> Result<Vec<(PathBuf, u64, String)>> {
    stages_checked(home, || Ok(()))
}
fn stages_checked(
    home: &Path,
    mut check: impl FnMut() -> Result<()>,
) -> Result<Vec<(PathBuf, u64, String)>> {
    check()?;
    let root = home.join("cache/tmp");
    let mut values = vec![];
    if !root.exists() {
        return Ok(values);
    }
    storage::guard_path_with_check(home, &root, &mut check)?;
    for entry in fs::read_dir(root)? {
        check()?;
        let entry = entry?;
        if values.len() >= 100000 {
            return Err(ToolkitError::new(
                "vault.cache_scan_limit",
                "staging scan exceeds bound",
                4,
            ));
        }
        storage::guard_path_with_check(home, &entry.path(), &mut check)?;
        if entry.file_type()?.is_file() {
            values.push((
                entry.path(),
                entry.metadata()?.len(),
                entry.file_name().to_string_lossy().into_owned(),
            ));
        }
    }
    check()?;
    Ok(values)
}
pub fn publish(home: &Path, bytes: &[u8], max_bytes: usize, owner: Option<Value>) -> Result<Value> {
    workspace::open(home)?;
    if let Some(owner) = &owner {
        validate_owner(owner)?;
    }
    if bytes.len() > max_bytes {
        return Err(ToolkitError::new(
            "vault.cache_limit",
            "cache object exceeds caller budget",
            4,
        ));
    }
    let digest = storage::sha256(bytes);
    let _quota = ExclusiveLock::resource(&home.join("locks"), "cache:quota")?;
    let _lock = ExclusiveLock::resource(&home.join("locks"), &format!("cache:obj:{digest}"))?;
    let path = home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    if path.exists() {
        storage::guard_path(home, &path)?;
        if fs::read(&path)? != bytes {
            return Err(ToolkitError::new(
                "vault.cache_integrity",
                "existing object content differs",
                4,
            ));
        }
    } else {
        let limit = workspace::config(home)?["cache"]["maxBytes"]
            .as_u64()
            .unwrap();
        let (objs, _) = objects(home)?;
        if objs.iter().map(|o| o.bytes).sum::<u64>() + bytes.len() as u64 > limit {
            return Err(ToolkitError::new(
                "vault.cache_budget_exceeded",
                "explicit prune required",
                3,
            ));
        }
        storage::atomic_write(&path, bytes)?;
    }
    if let Some(owner) = owner {
        let mut value =
            retain(home, &digest)?.unwrap_or_else(|| json!({"sha256":digest,"owners":[]}));
        if !value["owners"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == owner["kind"] && v["owner"] == owner["owner"])
        {
            value["owners"].as_array_mut().unwrap().push(owner);
        }
        storage::atomic_json(
            &home.join("cache/retain").join(format!("{digest}.json")),
            &value,
        )?;
    }
    Ok(json!({"sha256":digest,"bytes":bytes.len()}))
}
pub fn read_object(
    home: &Path,
    blob: &Value,
    maximum: usize,
    control: &crate::control::OperationControl,
) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    control.check()?;
    let digest = blob["sha256"]
        .as_str()
        .filter(|digest| valid_digest(digest, 64))
        .ok_or_else(|| ToolkitError::invalid("cache digest missing"))?;
    let length = blob["bytes"]
        .as_u64()
        .filter(|bytes| *bytes <= maximum as u64)
        .ok_or_else(|| {
            ToolkitError::new("vault.cache_limit", "cache object exceeds read budget", 4)
        })?;
    let path = home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    storage::guard_path_with_check(home, &path, || control.check())?;
    let (mut file, actual) = storage::locked_sha256(&path, length, control)?;
    let result = (|| {
        if actual != digest || file.metadata()?.len() != length {
            return Err(ToolkitError::new(
                "vault.cache_integrity",
                "cache object digest/length differs",
                4,
            ));
        }
        file.seek(SeekFrom::Start(0))?;
        let mut raw = Vec::with_capacity(length as usize);
        let mut buffer = [0; 65536];
        loop {
            control.check()?;
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            if raw.len() + n > length as usize {
                return Err(ToolkitError::invalid("cache object grew"));
            }
            raw.extend_from_slice(&buffer[..n]);
        }
        if raw.len() != length as usize {
            return Err(ToolkitError::invalid("cache length changed"));
        }
        control.check()?;
        Ok(raw)
    })();
    storage::finish_file(file, result, control)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheRequest {
    Status,
    DataStatus,
    Verify,
    Prune {
        target_bytes: u64,
        dry_run: bool,
        max_objects: usize,
        include_uncommitted: bool,
    },
}
pub fn dispatch(
    context: &crate::request::RequestContext,
    request: CacheRequest,
) -> Result<Outcome> {
    if request == CacheRequest::DataStatus {
        return crate::evidence::data_storage_report(&context.home, &context.control).map(Outcome::new);
    }
    if let CacheRequest::Prune { target_bytes, dry_run, max_objects, include_uncommitted } = &request {
        return pruning::prune(context, *target_bytes, *dry_run, *max_objects, *include_uncommitted);
    }
    if request == CacheRequest::Verify {
        workspace::open_controlled(&context.home, &context.control)?;
        let report = verify(&context.home, &context.control)?;
        let value = serde_json::to_value(&report)?;
        if !report.ok {
            return Err(ToolkitError::new("vault.cache_integrity", "vault.cache_integrity", 4)
                .with_details(json!({"result":value,"warnings":["Managed cache integrity failed; inspect missing and corrupt lists and decide on an explicit prune. Nothing was repaired or deleted."]})));
        }
        return Ok(Outcome::new(value));
    }
    workspace::open(&context.home)?;
    let (objs, foreign) = objects(&context.home)?;
    let (staging, staging_bytes) = {
        let entries = stages(&context.home)?;
        let bytes = entries.iter().map(|(_, bytes, _)| *bytes).sum::<u64>();
        (entries, bytes)
    };
    let (protective, missing, marker_foreign) = markers(&context.home)?;
    let bytes = objs.iter().map(|o| o.bytes).sum::<u64>();
    match request {
        CacheRequest::Status => {
            let limit = workspace::config(&context.home)?["cache"]["maxBytes"]
                .as_u64()
                .unwrap();
            let mut protected = vec![];
            let mut kinds = serde_json::Map::new();
            for marker in &protective {
                for owner in marker["owners"].as_array().unwrap() {
                    let kind = owner["kind"].as_str().unwrap_or("invalid");
                    let value = kinds.entry(kind).or_insert(json!(0));
                    *value = json!(value.as_u64().unwrap() + 1);
                }
                protected.push(marker);
            }
            Ok(Outcome::new(
                json!({"objectBytes":bytes,"objectCount":objs.len(),"uncommittedBytes":staging_bytes,"uncommittedCount":staging.len(),"protectedCount":protected.len(),"protectedByKind":kinds,"protected":protected,"orphanMarkers":missing.len(),"foreignCount":foreign+marker_foreign,"limitBytes":limit,"withinLimit":bytes<=limit}),
            ))
        }
        _ => unreachable!("cache request routed by CLI parser"),
    }
}
