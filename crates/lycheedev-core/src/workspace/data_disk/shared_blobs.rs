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
    io::Read,
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
    let result = inspect_body_at(&root, &path, reference, checks, full_hash);
    join(result, close_directory(root), checks)
}

fn inspect_body_at(
    root: &Directory,
    path: &Path,
    reference: &BlobRef,
    checks: &Checks<'_>,
    full_hash: bool,
) -> Result<bool> {
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
    let opened = root.open_shared_child_controlled(shard_name, &mut Controlled(checks));
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
        let Some(file) = directory.open_optional_file_controlled(name, &mut Controlled(checks))?
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
}

fn inspect_inventory_body(
    home: &Path,
    reference: &BlobRef,
    checks: &Checks<'_>,
    root: &mut Option<Directory>,
) -> Result<bool> {
    let path = evidence::blob_path(home, &reference.sha256)?;
    if root.is_none() {
        *root = Some(Directory::open_shared_controlled(
            &home.join("blobs"),
            &mut Controlled(checks),
        )?);
        #[cfg(test)]
        inventory_tests::directory_opened(root.as_ref().unwrap().path());
    }
    inspect_body_at(root.as_ref().unwrap(), &path, reference, checks, false)
}

pub(super) fn inventory_recovering(
    home: &Path,
    paths: &DataCachePaths,
    checks: &Checks<'_>,
    recovery: Option<&BlobRef>,
    held: &[Directory],
) -> Result<(u64, u64)> {
    with_inventory_directory(&paths.shared_blobs(), held, checks, |directory| {
        // One invocation-local shared root; open it only at the first body.
        let mut root = None;
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
                if let Some(digest) = name.strip_suffix(".repair.intent") {
                    let repair = read_record(directory, name, INTENT_BYTES, checks)?
                        .ok_or_else(integrity)?;
                    let original =
                        read_record(directory, &format!("{digest}.intent"), INTENT_BYTES, checks)?
                            .ok_or_else(integrity)?;
                    let reference = decode(&format!("{digest}.intent"), &original)?;
                    let done = read_record(
                        directory,
                        &format!("{digest}.complete"),
                        COMPLETE_BYTES,
                        checks,
                    )?
                    .ok_or_else(integrity)?;
                    if repair != original || done != completion(&original) {
                        return Err(integrity());
                    }
                    // The original completed charge still covers the canonical B.
                    // This one extra B covers a possible surviving shard stage.
                    pending_bytes = sum([pending_bytes, reference.bytes])?;
                    // Deliberately fails the older strict `<digest>.intent` decoder.
                    // Only this exact new suffix has the narrow repair meaning.
                    continue;
                }
                if let Some(digest) = name.strip_suffix(".complete") {
                    // A complete marker alone must never erase a retained intent.
                    if read_record(directory, &format!("{digest}.intent"), INTENT_BYTES, checks)?
                        .is_none()
                    {
                        return Err(integrity());
                    }
                }
                if !name.ends_with(".intent") {
                    continue;
                }
                let raw =
                    read_record(directory, name, INTENT_BYTES, checks)?.ok_or_else(integrity)?;
                let reference = decode(name, &raw)?;
                match read_record(
                    directory,
                    &format!("{}.complete", reference.sha256),
                    COMPLETE_BYTES,
                    checks,
                )? {
                    Some(done) => {
                        if done != completion(&raw) {
                            return Err(integrity());
                        }
                        if !inspect_inventory_body(home, &reference, checks, &mut root)?
                            && !recovery.is_some_and(|target| same_blob(target, &reference))
                        {
                            return Err(integrity());
                        }
                        complete_bytes = sum([complete_bytes, reference.bytes])?;
                    }
                    None => {
                        inspect_inventory_body(home, &reference, checks, &mut root)?;
                        pending_bytes = sum([pending_bytes, reference.bytes, reference.bytes])?;
                    }
                }
            }
            pending_bytes = sum([
                pending_bytes,
                durable::shard_reservations(directory, checks)?,
            ])?;
            Ok((complete_bytes, pending_bytes))
        })();
        let cleanup = root.map(close_directory).unwrap_or_default();
        join(result, cleanup, checks)
    })
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
        let mut stage =
            StagedFile::create_rebuildable_cache(parent, &digest, &mut Controlled(&self.control))?
                .ok_or_else(|| {
                    ToolkitError::new(
                        "vault.data_stage_slots_exhausted",
                        "All bounded bookkeeping slots are occupied",
                        6,
                    )
                })?;
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
    publish_shared_blob_inner(home, bytes, maximum, control, supplemental, None)
}

/// Publish two provenance bodies under one short quota owner. Each publication
/// retains its own attribution, peak admission and checked-close contract.
/// A later failure preserves the first body's actual publication receipts.
pub fn publish_shared_blob_pair_with_check(
    home: &Path,
    bytes: [&[u8]; 2],
    maximum: usize,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
) -> Result<[BlobRef; 2]> {
    control.check()?;
    supplemental()?;
    super::super::open_controlled_with_check(home, control, supplemental)?;
    let first = reference_for_bytes(bytes[0], maximum, control, supplemental)?;
    let mut owner = DerivedReservation::acquire_inner(
        None,
        home,
        super::super::DEFAULT_DATA_CACHE_MAX_BYTES,
        AdditionalPeak::default(),
        control,
        Some(supplemental),
        true,
    )?;
    let result = (|| {
        let first =
            publish_prehashed_with_owner(&mut owner, home, bytes[0], maximum, &first, None)?;
        // The second peak must include the first body's completed bytes and
        // attribution. No quota or directory owner is released between them.
        owner.report.existing = owner.verify_usage()?;
        control.check()?;
        supplemental()?;
        let second = reference_for_bytes(bytes[1], maximum, control, supplemental)?;
        let second =
            publish_prehashed_with_owner(&mut owner, home, bytes[1], maximum, &second, None)?;
        Ok([first, second])
    })();
    owner.finish(result)
}

/// Rebuild only the body named by an already authenticated decoded alias. The
/// original source must have passed its framing, CKey, exact length and EOF.
/// Core separately checks final SHA/length and the immutable attribution pair.
pub fn rebuild_shared_blob_with_check(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    expected: &BlobRef,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
) -> Result<BlobRef> {
    publish_shared_blob_inner(home, bytes, maximum, control, supplemental, Some(expected))
}
fn publish_shared_blob_inner(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
    recovery: Option<&BlobRef>,
) -> Result<BlobRef> {
    control.check()?;
    supplemental()?;
    super::super::open_controlled_with_check(home, control, supplemental)?;
    let reference = reference_for_bytes(bytes, maximum, control, supplemental)?;
    if recovery.is_some_and(|expected| !same_blob(expected, &reference)) {
        return Err(integrity());
    }
    let mut owner = DerivedReservation::acquire_inner_recovering(
        None,
        home,
        super::super::DEFAULT_DATA_CACHE_MAX_BYTES,
        AdditionalPeak::default(),
        control,
        Some(supplemental),
        true,
        recovery,
    )?;
    let result =
        publish_prehashed_with_owner(&mut owner, home, bytes, maximum, &reference, recovery);
    owner.finish(result)
}

fn reference_for_bytes(
    bytes: &[u8],
    maximum: usize,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
) -> Result<BlobRef> {
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
    intent(&reference)?;
    Ok(reference)
}

/// Internal CAP publication borrows its one existing Data quota owner. The
/// caller derived `reference` from these exact immutable bytes and retains the
/// evidence gate through metadata commit; this helper never reacquires quota.
pub(super) fn publish_prehashed_with_owner(
    owner: &mut DerivedReservation<'_>,
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    reference: &BlobRef,
    recovery: Option<&BlobRef>,
) -> Result<BlobRef> {
    if bytes.len() > maximum || bytes.len() as u64 != reference.bytes {
        return Err(integrity());
    }
    let raw = intent(reference)?;
    let source = durable::SourceIdentity::memory(reference);
    let recovered = durable::recover_memory(owner, &source, reference, bytes)?;
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
            let present = inspect_body(home, &reference, &owner.control, true)?;
            if recovered && !present {
                owner.admit_shared(
                    if complete.is_none() {
                        COMPLETE_BYTES as u64
                    } else {
                        0
                    },
                    0,
                )?;
                return publish_recovered_memory(owner, home, bytes, maximum, reference, &raw);
            }
            if recovery.is_some() && !present {
                owner.admit_shared(INTENT_BYTES as u64 + 128, reference.bytes)?;
                return repair_memory(&owner, home, bytes, maximum, &reference, &raw);
            }
            owner.admit_shared(0, 0)?;
            // A clean canonical shard plus fully verified winner closes the
            // crash window. Unknown stages keep the conservative pending peak.
            if present && complete.is_none() {
                recovery::complete_closed_publication(owner, &reference, &raw)?;
            }
            return if present {
                Ok(reference.clone())
            } else if complete.is_none() {
                Err(pending(&reference, &directory_path.join(&intent_name)))
            } else {
                Err(integrity())
            };
        }
        // A declaration-only interrupted attempt already charges its fixed
        // record and one possible shard body. Reusing it adds only the future
        // canonical body and attribution records, not that retained peak twice.
        owner.admit_shared(
            RECORD_PEAK + if recovered { 0 } else { 128 },
            if recovered {
                reference.bytes
            } else {
                sum([reference.bytes, reference.bytes])?
            },
        )?;
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
    let result = result.and_then(|value| {
        durable::finish_memory_recovery(owner, &source, reference, bytes)?;
        if recovered {
            durable::finish_repair(owner, reference)?;
        }
        Ok(value)
    });
    if result.is_ok() {
        owner.recovery = None; // Final success requires the completed body to exist.
    }
    result.map_err(|mut error| {
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
    let mut intent = QuotaIntent::create_bounded(
        directory,
        &reference.sha256,
        &mut Controlled(&owner.control),
    )?;
    let temporary = intent.name().to_owned();
    let mut slot = None;
    let mut withdraw = true;
    let result = (|| {
        slot = Some(durable::StageSlot::create_shard(
            owner,
            &durable::SourceIdentity::memory(reference),
            reference,
        )?);
        let state = intent.publish_record(raw, &mut Controlled(&owner.control))?;
        if state != StagedPublication::Published {
            return Err(integrity());
        }
        owner.prepare_publication_receipt()?;
        let attempt = evidence::publish_blob_attempt_predeclared_with_check(
            home,
            bytes,
            reference,
            maximum,
            owner.control.control,
            || owner.control.extra(),
            &slot.as_ref().expect("durable shard slot").name,
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
        });
    // Evidence AND intent checked closes completed before reducing the charge.
    let result = result.and_then(|value| {
        owner.publish_shared_record(complete_name, &completion(raw))?;
        Ok(value)
    });
    let clean = result.is_ok() || requested_withdraw && !cleanup_failed;
    let cleanup = slot.map(|slot| slot.finish(owner, clean)).transpose();
    join(result, cleanup.err(), &owner.control)
}

mod recovery;
mod repair;
pub(super) use repair::validate_recovery;
use repair::{repair_memory, same_blob};

#[cfg(test)]
mod tests;

pub(super) mod stream;

pub(super) mod retention;

pub(super) mod durable;

fn publish_recovered_memory(
    owner: &DerivedReservation<'_>,
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    reference: &BlobRef,
    raw: &[u8; INTENT_BYTES],
) -> Result<BlobRef> {
    let slot = durable::StageSlot::create_shard(
        owner,
        &durable::SourceIdentity::memory(reference),
        reference,
    )?;
    let attempt = evidence::publish_blob_attempt_predeclared_with_check(
        home,
        bytes,
        reference,
        maximum,
        owner.control.control,
        || owner.control.extra(),
        &slot.name,
    );
    let state = if attempt.settlement.reused {
        StagedPublication::Published
    } else {
        attempt.settlement.publication
    };
    owner.record_publication(
        state,
        "shared-blob-recovered",
        &evidence::blob_path(home, &reference.sha256)?,
        &reference.sha256,
        reference.bytes,
    );
    let result = attempt.result.and_then(|value| {
        let actual: BlobRef = serde_json::from_value(value)?;
        if !same_blob(&actual, reference) {
            return Err(integrity());
        }
        owner.publish_shared_record(&format!("{}.complete", reference.sha256), &completion(raw))?;
        Ok(actual)
    });
    let cleanup = slot.finish(owner, result.is_ok());
    join(result, cleanup.err(), &owner.control)
}
/// Enroll an already fully verified, reader-pinned body without another copy.
/// The CAP owner holds quota and its evidence gate until its final DB close.
pub(super) fn enroll_verified_with_owner(
    owner: &mut DerivedReservation<'_>,
    blob: &evidence::VerifiedBlob,
) -> Result<()> {
    let reference = blob.reference();
    let source = durable::SourceIdentity::memory(reference);
    let raw = intent(reference)?;
    let name = format!("{}.intent", reference.sha256);
    let done = format!("{}.complete", reference.sha256);
    let directory = owner
        .directories
        .iter()
        .find(|d| d.path() == owner.paths.shared_blobs())
        .expect("held attribution directory");
    if let Some(existing) = read_record(directory, &name, INTENT_BYTES, &owner.control)? {
        if existing != raw {
            return Err(integrity());
        }
        let completed = read_record(directory, &done, COMPLETE_BYTES, &owner.control)?;
        if let Some(completed) = completed {
            if completed != completion(&raw) {
                return Err(integrity());
            }
            // Existing declarations still reserve their own possible payload.
            // Enrollment never deletes a candidate without its full source.
            return Ok(());
        }
        if durable::has_shard(owner, &source, reference)? {
            // Only absent payloads may be settled using the verified body's
            // retained reader pin; finish_shards refuses any surviving stage.
            // Keep declarations until the completion marker is durable.
            durable::require_shards_absent(owner, &source, reference)?;
            owner.admit_shared(COMPLETE_BYTES as u64, 0)?;
            owner.publish_shared_record(&done, &completion(&raw))?;
            return durable::finish_shards(owner, &source, reference);
        }
        return Err(pending(reference, &directory.path().join(name)));
    }
    let declared = durable::has_shard(owner, &source, reference)?;
    if declared {
        // A verified reader is sufficient for enrollment, not for deleting an
        // interrupted payload. Reuse only an exactly absent payload slot.
        durable::require_shards_absent(owner, &source, reference)?;
    }
    owner.admit_shared(
        RECORD_PEAK + if declared { 0 } else { 128 },
        if declared {
            reference.bytes
        } else {
            sum([reference.bytes, reference.bytes])?
        },
    )?;
    let slot = durable::StageSlot::create_shard(owner, &source, reference)?;
    let result = owner
        .publish_shared_record(&name, &raw)
        .and_then(|()| owner.publish_shared_record(&done, &completion(&raw)));
    let cleanup = slot.finish(owner, result.is_ok());
    join(result, cleanup.err(), &owner.control)
}
