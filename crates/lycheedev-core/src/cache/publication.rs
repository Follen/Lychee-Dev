//! Derived Source cache publication under its original invocation. No pruning,
//! retention changes, or replay of an uncertain atomic publication occurs here.
use super::*;
use crate::{control::OperationControl, metadata::historical_write::finish_historical};
use sha2::{Digest, Sha256};
use std::io::Read;

/// Source manifests reference ordinary unretained objects, with the same quota,
/// digest and object lock namespace as `publish(..., None)`.
pub fn publish_derived_controlled(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    control: &OperationControl,
) -> Result<Value> {
    workspace::open_controlled(home, control)?;
    if bytes.len() > maximum {
        return Err(ToolkitError::new(
            "vault.cache_limit",
            "cache object exceeds caller budget",
            4,
        ));
    }
    let mut hash = Sha256::new();
    for chunk in bytes.chunks(65536) {
        control.check()?;
        hash.update(chunk);
    }
    let digest = hex::encode(hash.finalize());
    let blob = json!({"sha256":digest,"bytes":bytes.len()});
    let quota = ExclusiveLock::resource_controlled(&home.join("locks"), "cache:quota", control)?;
    let mut publication = "not_attempted";
    let result = (|| {
        let object = ExclusiveLock::resource_controlled(
            &home.join("locks"),
            &format!("cache:obj:{digest}"),
            control,
        )?;
        let result = (|| {
            let path = home
                .join("cache/objects")
                .join(&digest[..2])
                .join(&digest[2..]);
            control.check()?;
            if path.exists() {
                storage::guard_path_with_check(home, &path, || control.check())?;
                let mut file = fs::File::open(&path)?;
                let checked = (|| {
                    let mut offset = 0usize;
                    let mut buffer = [0; 65536];
                    loop {
                        control.check()?;
                        let n = file.read(&mut buffer)?;
                        if n == 0 {
                            break;
                        }
                        if bytes.get(offset..offset.saturating_add(n)) != Some(&buffer[..n]) {
                            return Err(ToolkitError::new(
                                "vault.cache_integrity",
                                "existing object content differs",
                                4,
                            ));
                        }
                        offset += n;
                    }
                    if offset != bytes.len() {
                        return Err(ToolkitError::new(
                            "vault.cache_integrity",
                            "existing object content differs",
                            4,
                        ));
                    }
                    Ok(())
                })();
                storage::finish_file(file, checked, control)?;
                publication = "existing";
            } else {
                let limit = config_limit(home, control)?;
                let total = object_bytes(home, control)?;
                if total
                    .checked_add(bytes.len() as u64)
                    .is_none_or(|n| n > limit)
                {
                    return Err(ToolkitError::new(
                        "vault.cache_budget_exceeded",
                        "explicit prune required",
                        3,
                    ));
                }
                control.check()?;
                publication = "unconfirmed";
                storage::atomic_write_controlled(&path, bytes, control)?;
                publication = "committed";
            }
            Ok(blob.clone())
        })();
        finish_historical(result, object.close())
    })();
    let result = finish_historical(result, quota.close());
    crate::selection::selection_finish(result, control.check()).map_err(|mut error| {
        if publication != "not_attempted" {
            error.details = json!({"schema":"lycheedev.cache-publication.v1","blob":blob,"state":publication,"cause":error.clone()});
        }
        error
    })
}

fn config_limit(home: &Path, control: &OperationControl) -> Result<u64> {
    // Preserve the existing config API's schema, ranges, JSON decoding and
    // absent-file defaults; only I/O and close gain the invocation's scope.
    workspace::open_controlled(home, control)?;
    let path = home.join("config.json");
    control.check()?;
    let value: Value = if path.exists() {
        let mut file = fs::File::open(path)?;
        let result = (|| {
            let mut raw = Vec::new();
            let mut buffer = [0; 65536];
            loop {
                control.check()?;
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buffer[..n]);
            }
            control.check()?;
            Ok(serde_json::from_slice(&raw)?)
        })();
        storage::finish_file(file, result, control)?
    } else {
        workspace::default_config()
    };
    if value["schema"] != "lycheedev.config.v1" {
        return Err(ToolkitError::new(
            "vault.config_schema_newer",
            "unsupported configuration schema",
            4,
        ));
    }
    if value["cache"]["maxBytes"]
        .as_u64()
        .is_none_or(|n| n > 1u64 << 45)
        || value["download"]["workers"]
            .as_u64()
            .is_none_or(|n| n > 256)
    {
        return Err(ToolkitError::new(
            "vault.config_format",
            "budget out of range",
            4,
        ));
    }
    control.check()?;
    Ok(value["cache"]["maxBytes"]
        .as_u64()
        .expect("validated quota"))
}
fn object_bytes(home: &Path, control: &OperationControl) -> Result<u64> {
    let root = home.join("cache/objects");
    control.check()?;
    if !root.exists() {
        return Ok(0);
    }
    storage::guard_path_with_check(home, &root, || control.check())?;
    let mut total = 0u64;
    let mut count = 0usize;
    for bucket in fs::read_dir(&root)? {
        control.check()?;
        let bucket = bucket?;
        let prefix = bucket.file_name().to_string_lossy().into_owned();
        if prefix.len() != 2 || !bucket.file_type()?.is_dir() {
            continue;
        }
        storage::guard_path_with_check(home, &bucket.path(), || control.check())?;
        for file in fs::read_dir(bucket.path())? {
            control.check()?;
            let file = file?;
            let digest = format!("{prefix}{}", file.file_name().to_string_lossy());
            if !valid_digest(&digest, 64) || !file.file_type()?.is_file() {
                continue;
            }
            storage::guard_path_with_check(home, &file.path(), || control.check())?;
            let info = file.metadata()?;
            // Existing object enumeration also authenticates this metadata.
            info.modified()?;
            total = total
                .checked_add(info.len())
                .ok_or_else(|| ToolkitError::invalid("cache size overflow"))?;
            count += 1;
            if count > 100_000 {
                return Err(ToolkitError::new(
                    "vault.cache_scan_limit",
                    "cache scan exceeded 100000 objects",
                    4,
                ));
            }
        }
    }
    control.check()?;
    Ok(total)
}

#[cfg(test)]
mod tests;
