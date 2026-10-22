//! One explicitly authenticated missing decoded body, never a general cache
//! inventory exception. Original attribution/aliases/evidence are not changed.
use super::*;

pub(super) fn same_blob(left: &BlobRef, right: &BlobRef) -> bool {
    left.sha256 == right.sha256 && left.bytes == right.bytes
}

pub(in crate::workspace::data_disk) fn validate_recovery(
    home: &Path,
    paths: &DataCachePaths,
    reference: &BlobRef,
    checks: &Checks<'_>,
) -> Result<()> {
    let raw = intent(reference)?;
    let directory = Directory::open_controlled(&paths.shared_blobs(), &mut Controlled(checks))?;
    let result = (|| {
        let original = read_record(
            &directory,
            &format!("{}.intent", reference.sha256),
            INTENT_BYTES,
            checks,
        )?;
        let complete = read_record(
            &directory,
            &format!("{}.complete", reference.sha256),
            COMPLETE_BYTES,
            checks,
        )?;
        // Historical decoded aliases may predate Data attribution. Both
        // records absent is an ordinary cold enrollment, never a partial pair.
        let managed_pending =
            complete.is_none() && durable::has_any_shard(&directory, reference, checks)?;
        if !(original.is_none() && complete.is_none())
            && (original.as_deref() != Some(raw.as_slice())
                || !managed_pending && complete.as_deref() != Some(completion(&raw).as_slice()))
        {
            return Err(integrity());
        }
        if let Some(repair) = read_record(
            &directory,
            &format!("{}.repair.intent", reference.sha256),
            INTENT_BYTES,
            checks,
        )? {
            if original.is_none() || complete.is_none() || repair != raw {
                return Err(integrity());
            }
            if !durable::has_any_shard(&directory, reference, checks)? {
                return Err(pending(
                    reference,
                    &directory
                        .path()
                        .join(format!("{}.repair.intent", reference.sha256)),
                ));
            }
        }
        // Inventory keeps native type/length checks. The original publisher
        // fully authenticates any concurrent winner under its canonical lease.
        // No existing body gains overwrite permission from this target.
        inspect_body(home, reference, checks, false)?;
        checks.check()
    })();
    join(result, close_directory(directory), checks)
}

/// The caller has admitted original B + record48 + possible shard-stage B.
/// Keep this new CREATE_NEW handle until body settlement; no reopened handle
/// ever acquires withdrawal rights. Closing precedes the owner's full rescan.
pub(super) fn run_repair<T>(
    owner: &DerivedReservation<'_>,
    reference: &BlobRef,
    raw: &[u8; INTENT_BYTES],
    run: impl FnOnce() -> (Result<T>, bool),
) -> Result<T> {
    let path = owner.paths.shared_blobs();
    let directory = owner
        .directories
        .iter()
        .find(|directory| directory.path() == path)
        .expect("held attribution directory");
    owner.prepare_publication_receipt()?;
    owner
        .publication_receipts
        .borrow_mut()
        .try_reserve(3)
        .map_err(|_| ToolkitError::invalid("cannot reserve repair settlement receipts"))?;
    let mut marker = QuotaIntent::create_bounded(
        directory,
        &reference.sha256,
        &mut Controlled(&owner.control),
    )?;
    let temporary = marker.name().to_owned();
    let mut clean = true; // No body publisher has been entered yet.
    let result = (|| {
        if marker.publish_repair_record(raw, &mut Controlled(&owner.control))?
            != StagedPublication::Published
        {
            return Err(integrity());
        }
        owner.prepare_publication_receipt()?;
        let (result, settled) = run();
        clean = settled;
        result
    })();
    let state = marker.publication();
    let marker_path = path.join(format!("{}.repair.intent", reference.sha256));
    owner.record_publication(
        state,
        "shared-blob-repair",
        &marker_path,
        &storage::sha256(raw),
        raw.len() as u64,
    );
    let withdraw = clean && state != StagedPublication::Unknown;
    let cleanup: Vec<_> = if withdraw {
        marker.withdraw()
    } else {
        marker.close()
    }
    .into_iter()
    .map(ToolkitError::io)
    .collect();
    let cleanup_failed = !cleanup.is_empty();
    // Preserve settlement even when this function succeeds and a later owner
    // inventory/directory/lease close fails. Published history is not erased.
    owner
        .publication_receipts
        .borrow_mut()
        .push(serde_json::json!({
            "kind":"shared-blob-repair-settlement", "path":marker_path,
            "sha256":reference.sha256, "bytes":reference.bytes,
            "markerPublication":format!("{state:?}"),
            "withdrawalRequested":withdraw, "withdrawalComplete":withdraw && !cleanup_failed,
            "markerCloseComplete":!cleanup_failed,
        }));
    publication::publication_result(join(result, cleanup, &owner.control), state, marker_path.clone(), temporary, cleanup_failed)
        .map_err(|mut error| {
            error.details = serde_json::json!({"schema":"lycheedev.data-shared-blob-repair.v1", "cause":error.clone(),
                "blob":reference, "repairPath":marker_path, "repairWithdrawalRequested":withdraw,
                "repairWithdrawalComplete":withdraw && !cleanup_failed,
                "originalAttributionUnchanged":true});
            error
        })
}

pub(super) fn repair_memory(
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
    let result = run_repair(owner, reference, raw, || {
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
            "shared-blob-rebuild",
            &evidence::blob_path(home, &reference.sha256).expect("validated digest"),
            &reference.sha256,
            reference.bytes,
        );
        let result = attempt.result.and_then(|value| {
            let published: BlobRef = serde_json::from_value(value)?;
            if same_blob(reference, &published) {
                Ok(published)
            } else {
                Err(integrity())
            }
        });
        let clean = result.is_ok() || attempt.settlement.may_release_unpublished_intent();
        (result, clean)
    });
    let cleanup = slot.finish(owner, result.is_ok());
    join(result, cleanup.err(), &owner.control)
}

#[cfg(test)]
mod tests;
