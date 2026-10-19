//! Monotonic attribution of explicitly published Data bodies in shared blobs/.
//!
//! This covers remote Store memory publication and explicitly opted-in decoded
//! streams, not historical references, capture metadata or every Data writer. The same quota
//! lease serializes it with derived writers. Never call this while holding that
//! lease, a metadata transaction, or from a producer already holding quota.
//!
//! A durable intent precedes the existing evidence publisher. An unfinished
//! intent reserves two bodies (possible winner + failed shard stage) until a
//! proven-clean failure withdraws it or audited crash recovery is implemented. Only after evidence's
//! checked close may a second immutable record reduce the charge to one body.
//! Neither record grants deletion rights. All shared bodies remain protected.
use super::*;
use crate::evidence::{self, BlobRef};
use lycheedev_windows::filesystem::destination::{QuotaIntent, StagedFile, StagedPublication};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    io::{Read, Write},
};

const INTENT_BYTES: usize = 48;
const COMPLETE_BYTES: usize = 32;
const RECORD_PEAK: u64 = (INTENT_BYTES + COMPLETE_BYTES) as u64;
const MAGIC: &[u8; 8] = b"LDSB0001";

fn integrity() -> ToolkitError {
    ToolkitError::new(
        "vault.data_shared_blob_integrity",
        "Data shared-blob attribution is inconsistent; files retained",
        4,
    )
}
fn pending(reference: &BlobRef, path: &Path) -> ToolkitError {
    ToolkitError::new("vault.data_shared_blob_recovery_required", "Unfinished Data shared-blob publication retains its peak; automatic retry is disabled", 3)
        .with_details(serde_json::json!({"schema":"lycheedev.data-shared-blob-pending.v1", "blob":reference,"intentPath":path,"retainedPeakBytes":reference.bytes.saturating_mul(2),"automaticRecovery":false}))
}
fn intent(reference: &BlobRef) -> Result<[u8; INTENT_BYTES]> {
    if !valid_digest(&reference.sha256, 64)
        || reference.sha256.bytes().any(|b| b.is_ascii_uppercase())
        || reference.bytes > evidence::MAX_CAPTURE_BYTES
    {
        return Err(integrity());
    }
    let mut result = [0; INTENT_BYTES];
    result[..8].copy_from_slice(MAGIC);
    result[8..16].copy_from_slice(&reference.bytes.to_le_bytes());
    hex::decode_to_slice(&reference.sha256, &mut result[16..]).map_err(|_| integrity())?;
    Ok(result)
}
fn decode(name: &str, bytes: &[u8]) -> Result<BlobRef> {
    if bytes.len() != INTENT_BYTES || &bytes[..8] != MAGIC {
        return Err(integrity());
    }
    let reference = BlobRef {
        sha256: hex::encode(&bytes[16..]),
        bytes: u64::from_le_bytes(bytes[8..16].try_into().map_err(|_| integrity())?),
    };
    if name != format!("{}.intent", reference.sha256) || intent(&reference)?.as_slice() != bytes {
        return Err(integrity());
    }
    Ok(reference)
}
fn completion(raw: &[u8]) -> [u8; COMPLETE_BYTES] {
    Sha256::digest(raw).into()
}

fn read_record(
    directory: &Directory,
    name: &str,
    expected: usize,
    checks: &Checks<'_>,
) -> Result<Option<Vec<u8>>> {
    let Some(mut file) =
        directory.open_optional_file_controlled(OsStr::new(name), &mut Controlled(checks))?
    else {
        return Ok(None);
    };
    let result = (|| {
        checks.check()?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != expected as u64 {
            return Err(integrity());
        }
        let mut raw = vec![0; expected];
        file.read_exact(&mut raw)?;
        checks.check()?;
        if file.read(&mut [0])? != 0 {
            return Err(integrity());
        }
        checks.check()?;
        Ok(Some(raw))
    })();
    finish_file(file, result, checks)
}

// Size-only inventory of an already attributed immutable body. Full SHA/EOF is
// rechecked by the original publication or the read-only reuse path. No all-body
// rehash is hidden inside each quota admission.
fn inspect_body(
    home: &Path,
    reference: &BlobRef,
    checks: &Checks<'_>,
    full_hash: bool,
) -> Result<bool> {
    let path = evidence::blob_path(home, &reference.sha256)?;
    let root = Directory::open_shared_controlled(&home.join("blobs"), &mut Controlled(checks))?;
    let result = (|| {
        let shard_name = path
            .parent()
            .expect("blob shard")
            .file_name()
            .expect("shard name");
        let Some(shard_handle) =
            root.open_optional_file_controlled(shard_name, &mut Controlled(checks))?
        else {
            return Ok(false);
        };
        let opened = Directory::open_shared_controlled(
            path.parent().expect("blob shard"),
            &mut Controlled(checks),
        );
        let closed = storage::close_file(shard_handle)
            .err()
            .map(ToolkitError::io);
        let directory = match opened {
            Ok(directory) => match join(Ok(()), closed, checks) {
                Ok(()) => directory,
                Err(error) => return join(Err(error), close_directory(directory), checks),
            },
            Err(error) => return join(Err(error), closed, checks),
        };
        let result = (|| {
            let name = path.file_name().expect("blob name");
            if full_hash {
                return publication::verify_existing(
                    &directory,
                    name,
                    reference.bytes,
                    &reference.sha256,
                    checks,
                );
            }
            let Some(file) =
                directory.open_optional_file_controlled(name, &mut Controlled(checks))?
            else {
                return Ok(false);
            };
            let result = file
                .metadata()
                .map_err(ToolkitError::io)
                .and_then(|metadata| {
                    if metadata.is_file() && metadata.len() == reference.bytes {
                        Ok(true)
                    } else {
                        Err(integrity())
                    }
                });
            finish_file(file, result, checks)
        })();
        join(result, close_directory(directory), checks)
    })();
    join(result, close_directory(root), checks)
}

pub(super) fn inventory(
    home: &Path,
    paths: &DataCachePaths,
    checks: &Checks<'_>,
) -> Result<(u64, u64)> {
    let directory = Directory::open_controlled(&paths.shared_blobs(), &mut Controlled(checks))?;
    let result = (|| {
        let mut complete_bytes = 0;
        let mut pending_bytes = 0;
        for (index, entry) in fs::read_dir(directory.path())?.enumerate() {
            checks.check()?;
            if index >= MAX_ENTRIES {
                return Err(integrity());
            }
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if let Some(digest) = name.strip_suffix(".complete") {
                // A complete marker alone must never erase a retained intent.
                if read_record(
                    &directory,
                    &format!("{digest}.intent"),
                    INTENT_BYTES,
                    checks,
                )?
                .is_none()
                {
                    return Err(integrity());
                }
            }
            if !name.ends_with(".intent") {
                continue;
            }
            let raw = read_record(&directory, name, INTENT_BYTES, checks)?.ok_or_else(integrity)?;
            let reference = decode(name, &raw)?;
            match read_record(
                &directory,
                &format!("{}.complete", reference.sha256),
                COMPLETE_BYTES,
                checks,
            )? {
                Some(done) => {
                    if done != completion(&raw) {
                        return Err(integrity());
                    }
                    if !inspect_body(home, &reference, checks, false)? {
                        return Err(integrity());
                    }
                    complete_bytes = sum([complete_bytes, reference.bytes])?;
                }
                None => {
                    inspect_body(home, &reference, checks, false)?;
                    pending_bytes = sum([pending_bytes, reference.bytes, reference.bytes])?;
                }
            }
        }
        Ok((complete_bytes, pending_bytes))
    })();
    join(result, close_directory(directory), checks)
}

impl DerivedReservation<'_> {
    fn admit_shared(&mut self, record_peak: u64, body_peak: u64) -> Result<()> {
        self.control.check()?;
        self.report.additional_peak.generations = record_peak;
        self.report.shared_blob_additional_peak = body_peak;
        self.report.admitted_peak_bytes =
            sum([self.report.existing.total()?, record_peak, body_peak])?;
        // Existing excess does not authorize deletion. A verified read-only
        // duplicate is allowed; any new attribution, record or body is growth.
        if (record_peak != 0 || body_peak != 0)
            && self.report.admitted_peak_bytes > self.report.configured_max_bytes
        {
            return Err(budget_error(&self.report));
        }
        Ok(())
    }
    fn publish_shared_record(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let path = self.paths.shared_blobs();
        let parent = self
            .directories
            .iter()
            .find(|d| d.path() == path)
            .expect("held attribution directory");
        let digest = storage::sha256(bytes);
        if publication::verify_existing(
            parent,
            OsStr::new(name),
            bytes.len() as u64,
            &digest,
            &self.control,
        )? {
            return Ok(());
        }
        self.prepare_publication_receipt()?;
        let mut stage = StagedFile::create(parent, &mut Controlled(&self.control))?;
        let result = (|| {
            self.control.check()?;
            stage.write_all(bytes)?; // Fixed 48/32-byte records, admitted together.
            self.control.check()?;
            stage.seal()?;
            self.control.check()?;
            let state = stage.publish_name(OsStr::new(name), &mut Controlled(&self.control))?;
            if state == StagedPublication::NotPublished
                && !publication::verify_existing(
                    parent,
                    OsStr::new(name),
                    bytes.len() as u64,
                    &digest,
                    &self.control,
                )?
            {
                return Err(integrity());
            }
            Ok(())
        })();
        let state = stage.publication();
        let temporary = stage.name().to_owned();
        self.record_publication(
            state,
            "shared-blob-attribution",
            &path.join(name),
            &digest,
            bytes.len() as u64,
        );
        let cleanup: Vec<_> = stage.close().into_iter().map(ToolkitError::io).collect();
        let cleanup_failed = !cleanup.is_empty();
        publication::publication_result(
            join(result, cleanup, &self.control),
            state,
            path.join(name),
            temporary,
            cleanup_failed,
        )
    }
}

/// Quota-enrolled Data in-memory publication into the existing evidence CAS.
/// The borrowed callback is the caller's original Budget, including in workers.
/// It must not recursively enter this quota owner. This function acquires no
/// query slot and holds no SQLite transaction; the existing blob lock is inner.
pub fn publish_shared_blob_with_check(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
) -> Result<BlobRef> {
    control.check()?;
    supplemental()?;
    super::super::open_controlled_with_check(home, control, supplemental)?;
    if bytes.len() > maximum || bytes.len() as u64 > evidence::MAX_CAPTURE_BYTES {
        return Err(ToolkitError::new(
            "vault.blob_limit",
            "blob exceeds budget",
            4,
        ));
    }
    let mut digest = Sha256::new();
    for chunk in bytes.chunks(65536) {
        control.check()?;
        supplemental()?;
        digest.update(chunk);
    }
    control.check()?;
    supplemental()?;
    let reference = BlobRef {
        sha256: hex::encode(digest.finalize()),
        bytes: bytes.len() as u64,
    };
    let raw = intent(&reference)?;
    let mut owner = DerivedReservation::acquire_inner(
        None,
        home,
        super::super::DEFAULT_DATA_CACHE_MAX_BYTES,
        AdditionalPeak::default(),
        control,
        Some(supplemental),
        true,
    )?;
    let intent_name = format!("{}.intent", reference.sha256);
    let complete_name = format!("{}.complete", reference.sha256);
    let directory_path = owner.paths.shared_blobs();
    let result = (|| {
        let directory = owner
            .directories
            .iter()
            .find(|d| d.path() == directory_path)
            .expect("held attribution directory");
        if let Some(existing) = read_record(directory, &intent_name, INTENT_BYTES, &owner.control)?
        {
            if existing != raw {
                return Err(integrity());
            }
            let complete = read_record(directory, &complete_name, COMPLETE_BYTES, &owner.control)?;
            if complete
                .as_ref()
                .is_some_and(|done| done.as_slice() != completion(&raw).as_slice())
            {
                return Err(integrity());
            }
            owner.admit_shared(0, 0)?;
            // Pending reuse never releases a possibly surviving shard stage.
            return if inspect_body(home, &reference, &owner.control, true)? {
                Ok(reference.clone())
            } else if complete.is_none() {
                Err(pending(&reference, &directory_path.join(&intent_name)))
            } else {
                Err(integrity())
            };
        }
        owner.admit_shared(RECORD_PEAK, sum([reference.bytes, reference.bytes])?)?;
        publish_new_attribution(
            &owner,
            home,
            bytes,
            maximum,
            &reference,
            &raw,
            &intent_name,
            &complete_name,
        )
    })();
    owner.finish(result).map_err(|mut error| {
        error.details = serde_json::json!({"schema":"lycheedev.data-shared-blob-owner-error.v1", "cause":error.clone(),"blob":reference,"intentPath":directory_path.join(intent_name),"completionPath":directory_path.join(complete_name),"totalDataCoverageComplete":false});
        error
    })
}

// Keep the quota-intent CREATE_NEW handle through byte publication. Ordinary
// NotPublished + clean-stage errors withdraw only this exact intent; crash or
// Unknown outcomes retain it. No path is reopened to acquire deletion rights.
#[allow(clippy::too_many_arguments)]
fn publish_new_attribution(
    owner: &DerivedReservation<'_>,
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    reference: &BlobRef,
    raw: &[u8; INTENT_BYTES],
    intent_name: &str,
    complete_name: &str,
) -> Result<BlobRef> {
    let directory_path = owner.paths.shared_blobs();
    let directory = owner
        .directories
        .iter()
        .find(|d| d.path() == directory_path)
        .expect("held attribution directory");
    owner.prepare_publication_receipt()?;
    owner
        .publication_receipts
        .borrow_mut()
        .try_reserve(2)
        .map_err(|_| {
            ToolkitError::new(
                "vault.data_cache_publication_limit",
                "Cannot reserve shared-blob settlement receipts",
                3,
            )
        })?;
    let mut intent = QuotaIntent::create(directory, &mut Controlled(&owner.control))?;
    let temporary = intent.name().to_owned();
    let mut withdraw = true;
    let result = (|| {
        let state = intent.publish_record(raw, &mut Controlled(&owner.control))?;
        if state != StagedPublication::Published {
            return Err(integrity());
        }
        owner.prepare_publication_receipt()?;
        let attempt = evidence::publish_blob_attempt_with_check(
            home,
            bytes,
            maximum,
            owner.control.control,
            || owner.control.extra(),
        );
        withdraw = attempt.settlement.may_release_unpublished_intent();
        let body_state = if attempt.settlement.reused {
            StagedPublication::Published
        } else {
            attempt.settlement.publication
        };
        owner.record_publication(
            body_state,
            if attempt.settlement.reused {
                "shared-blob-reused"
            } else {
                "shared-blob"
            },
            &evidence::blob_path(home, &reference.sha256)?,
            &reference.sha256,
            reference.bytes,
        );
        let published: BlobRef = serde_json::from_value(attempt.result?)?;
        if published.sha256 != reference.sha256 || published.bytes != reference.bytes {
            return Err(integrity());
        }
        Ok(reference.clone())
    })();
    let state = intent.publication();
    owner.record_publication(
        state,
        "shared-blob-intent",
        &directory_path.join(intent_name),
        &storage::sha256(raw),
        raw.len() as u64,
    );
    let requested_withdraw = result.is_err() && withdraw && state != StagedPublication::Unknown;
    let cleanup: Vec<_> = if requested_withdraw {
        intent.withdraw()
    } else {
        intent.close()
    }
    .into_iter()
    .map(ToolkitError::io)
    .collect();
    let cleanup_failed = !cleanup.is_empty();
    let result = join(result, cleanup, &owner.control);
    let result = publication::publication_result(result, state, directory_path.join(intent_name), temporary, cleanup_failed)
        .map_err(|mut error| {
            error.details = serde_json::json!({"schema":"lycheedev.data-shared-blob-settlement.v1","cause":error.clone(),
                "intentWithdrawalRequested":requested_withdraw,"intentWithdrawalComplete":requested_withdraw && !cleanup_failed,
                "intentPublication":format!("{state:?}")});
            error
        })?;
    // Evidence AND intent checked closes completed before reducing the charge.
    owner.publish_shared_record(complete_name, &completion(raw))?;
    Ok(result)
}

#[cfg(test)]
mod tests;

pub(super) mod stream;

pub(super) mod retention;
