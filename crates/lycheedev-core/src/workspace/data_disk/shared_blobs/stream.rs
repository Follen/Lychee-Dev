//! Data's exact-length stream reuses the original private spool as its durable
//! logical-byte reservation. No quota lock is held while pulling its producer.
use super::*;
use crate::evidence::{CaptureAccounting, StreamPublicationSettlement, VerifiedBlob};
use std::sync::Arc;

pub(crate) struct StreamQuota<'a> {
    home: &'a Path,
    control: &'a OperationControl,
    supplemental: &'a dyn Fn() -> Result<()>,
    recovery: Option<&'a BlobRef>,
}
impl StreamQuota<'_> {
    pub(crate) fn reserve_spool(&self, expected: u64) -> Result<DerivedReservation<'_>> {
        DerivedReservation::acquire_inner_recovering(
            None,
            self.home,
            crate::workspace::DEFAULT_DATA_CACHE_MAX_BYTES,
            AdditionalPeak {
                staging: expected,
                ..Default::default()
            },
            self.control,
            Some(self.supplemental),
            false,
            self.recovery,
        )
    }

    /// Only called after producer EOF. The callback may perform local spool I/O
    /// and CAS operations, never another producer/metadata transaction. Its
    /// checked spool/shard/lock cleanup is part of the original typed settlement.
    pub(crate) fn publish(
        &self,
        reference: &BlobRef,
        run: impl FnOnce(&mut StreamPublicationSettlement, bool) -> Result<()>,
    ) -> Result<()> {
        let raw = intent(reference)?;
        if self
            .recovery
            .is_some_and(|expected| !same_blob(expected, reference))
        {
            return Err(integrity());
        }
        let mut owner = DerivedReservation::acquire_inner_recovering(
            None,
            self.home,
            crate::workspace::DEFAULT_DATA_CACHE_MAX_BYTES,
            AdditionalPeak::default(),
            self.control,
            Some(self.supplemental),
            true,
            self.recovery,
        )?;
        let intent_name = format!("{}.intent", reference.sha256);
        let complete_name = format!("{}.complete", reference.sha256);
        let path = owner.paths.shared_blobs();
        let result = (|| {
            let directory = owner
                .directories
                .iter()
                .find(|d| d.path() == path)
                .expect("held shared attribution directory");
            if let Some(existing) =
                read_record(directory, &intent_name, INTENT_BYTES, &owner.control)?
            {
                if existing != raw {
                    return Err(integrity());
                }
                let complete =
                    read_record(directory, &complete_name, COMPLETE_BYTES, &owner.control)?;
                if complete
                    .as_ref()
                    .is_some_and(|done| done.as_slice() != completion(&raw))
                {
                    return Err(integrity());
                }
                let present = inspect_body(self.home, reference, &owner.control, false)?;
                if self.recovery.is_some() && !present {
                    // Completed B remains charged and the already-sized source
                    // spool is in existing staging. Only shard B + record48 are new.
                    owner.admit_shared(INTENT_BYTES as u64, reference.bytes)?;
                    return repair::run_repair(&owner, reference, &raw, || {
                        let mut settlement = StreamPublicationSettlement::new();
                        let result = run(&mut settlement, false);
                        // Record even a failed publication's Published/Unknown fact.
                        let result = join(
                            result,
                            record_body(&owner, self.home, reference, &settlement).err(),
                            &owner.control,
                        );
                        let clean = result.is_ok() || settlement.may_release_unpublished_intent();
                        (result, clean)
                    });
                }
                // Existing pending reservations may be reused only when their
                // body already exists; never replay an unresolved publication.
                if !present {
                    return Err(if complete.is_some() {
                        integrity()
                    } else {
                        pending(reference, &path.join(&intent_name))
                    });
                }
                owner.admit_shared(0, 0)?;
                owner.prepare_publication_receipt()?;
                let mut settlement = StreamPublicationSettlement::new();
                let result = run(&mut settlement, true);
                record_body(&owner, self.home, reference, &settlement)?;
                return result;
            }
            // The pre-sized spool is already counted in existing staging. The
            // established pending-intent contract conservatively reserves a
            // possible shared winner AND a failed shard stage, independently.
            // Consequently cold peak may be 3*N + 80, not an asserted 2*N.
            owner.admit_shared(RECORD_PEAK, sum([reference.bytes, reference.bytes])?)?;
            owner.prepare_publication_receipt()?;
            owner
                .publication_receipts
                .borrow_mut()
                .try_reserve(3)
                .map_err(|_| ToolkitError::invalid("cannot reserve stream settlement receipts"))?;
            let directory = owner
                .directories
                .iter()
                .find(|d| d.path() == path)
                .expect("held shared attribution directory");
            let mut intent_owner = QuotaIntent::create(directory, &mut Controlled(&owner.control))?;
            let temporary = intent_owner.name().to_owned();
            let mut settlement = StreamPublicationSettlement::new();
            let mut entered_body = false;
            let result = (|| {
                if intent_owner.publish_record(&raw, &mut Controlled(&owner.control))?
                    != StagedPublication::Published
                {
                    return Err(integrity());
                }
                entered_body = true;
                let result = run(&mut settlement, false);
                record_body(&owner, self.home, reference, &settlement)?;
                result
            })();
            let state = intent_owner.publication();
            owner.record_publication(
                state,
                "shared-blob-intent",
                &path.join(&intent_name),
                &storage::sha256(&raw),
                raw.len() as u64,
            );
            let withdraw = result.is_err()
                && state != StagedPublication::Unknown
                && (!entered_body || settlement.may_release_unpublished_intent());
            let cleanup: Vec<_> = if withdraw {
                intent_owner.withdraw()
            } else {
                intent_owner.close()
            }
            .into_iter()
            .map(ToolkitError::io)
            .collect();
            let cleanup_failed = !cleanup.is_empty();
            let result = publication::publication_result(
                join(result, cleanup, &owner.control),
                state,
                path.join(&intent_name),
                temporary,
                cleanup_failed,
            );
            result.map_err(|mut error| {
                error.details = serde_json::json!({"schema":"lycheedev.data-stream-settlement.v1",
                    "cause":error.clone(), "intentWithdrawalRequested":withdraw,
                    "intentWithdrawalComplete":withdraw && !cleanup_failed,
                    "bodySettlement":settlement.summary()});
                error
            })?;
            // Both source spool and final stage were checked closed inside run.
            // The original intent handle has also closed before its record is read.
            owner.publish_shared_record(&complete_name, &completion(&raw))
        })();
        if result.is_ok() {
            owner.recovery = None; // Initial spool admission alone permits absence.
        }
        owner.finish(result)
    }
}
fn record_body(
    owner: &DerivedReservation<'_>,
    home: &Path,
    reference: &BlobRef,
    settlement: &StreamPublicationSettlement,
) -> Result<()> {
    owner.record_publication(
        if settlement.reused {
            StagedPublication::Published
        } else {
            settlement.publication
        },
        if settlement.reused {
            "shared-stream-reused"
        } else {
            "shared-stream"
        },
        &evidence::blob_path(home, &reference.sha256)?,
        &reference.sha256,
        reference.bytes,
    );
    Ok(())
}

/// Publish a Data stream with a known exact decoded length, not a request ceiling.
/// The producer must validate its content identity before real EOF. The original
/// control, independent Budget, 64KiB loops and checked-close errors stay active.
/// Unknown-length streams must use a separately designed growth admission path;
/// callers must never substitute `maximum` as a guessed expected length.
///
/// This adds only the two decoded-stream consumers to partial Data attribution.
/// Historical unregistered hits, CAP, SQLite/WAL/SHM and external outputs remain
/// outside full coverage. All shared bodies stay protected; no eviction occurs.
pub fn publish_shared_blob_stream_with_check(
    home: &Path,
    reader: &mut impl Read,
    expected_bytes: u64,
    maximum: u64,
    control: &OperationControl,
    supplemental: impl Fn() -> Result<()> + Send + Sync + 'static,
    accounting: &impl CaptureAccounting,
) -> Result<VerifiedBlob> {
    publish_shared_blob_stream_inner(
        home,
        reader,
        expected_bytes,
        maximum,
        control,
        supplemental,
        accounting,
        None,
    )
}

/// Same producer/CKey/EOF contract as ordinary streaming, with one authenticated
/// missing alias target. Rebuilt SHA/length must match before any shard write.
#[allow(clippy::too_many_arguments)]
pub fn rebuild_shared_blob_stream_with_check(
    home: &Path,
    reader: &mut impl Read,
    expected: &BlobRef,
    maximum: u64,
    control: &OperationControl,
    supplemental: impl Fn() -> Result<()> + Send + Sync + 'static,
    accounting: &impl CaptureAccounting,
) -> Result<VerifiedBlob> {
    publish_shared_blob_stream_inner(
        home,
        reader,
        expected.bytes,
        maximum,
        control,
        supplemental,
        accounting,
        Some(expected),
    )
}
#[allow(clippy::too_many_arguments)]
fn publish_shared_blob_stream_inner(
    home: &Path,
    reader: &mut impl Read,
    expected_bytes: u64,
    maximum: u64,
    control: &OperationControl,
    supplemental: impl Fn() -> Result<()> + Send + Sync + 'static,
    accounting: &impl CaptureAccounting,
    recovery: Option<&BlobRef>,
) -> Result<VerifiedBlob> {
    let supplemental = Arc::new(supplemental);
    let check = || supplemental();
    let quota = StreamQuota {
        home,
        control,
        supplemental: &check,
        recovery,
    };
    let stream_check = supplemental.clone();
    evidence::publish_data_blob_stream_with_check(
        home,
        reader,
        expected_bytes,
        maximum,
        control,
        move || stream_check(),
        accounting,
        &quota,
    )
}

#[cfg(test)]
mod tests;
