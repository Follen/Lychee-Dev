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
    pub(crate) source: Option<&'a durable::SourceIdentity>,
}
impl StreamQuota<'_> {
    pub(crate) fn reserve_spool(&self, expected: u64) -> Result<DerivedReservation<'_>> {
        DerivedReservation::acquire_inner_recovering(
            None,
            self.home,
            crate::workspace::DEFAULT_DATA_CACHE_MAX_BYTES,
            AdditionalPeak {
                staging: expected,
                generations: if self.source.is_some() { 128 } else { 0 },
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
    fn publish_legacy(
        &self,
        reference: &BlobRef,
        run: impl FnOnce(
            &DerivedReservation<'_>,
            &mut StreamPublicationSettlement,
            bool,
            Option<&lycheedev_windows::filesystem::destination::ManagedStageName>,
        ) -> Result<()>,
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
                        let result = run(&owner, &mut settlement, false, None);
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
                let result = run(&owner, &mut settlement, true, None);
                record_body(&owner, self.home, reference, &settlement)?;
                if result.is_ok() && complete.is_none() {
                    recovery::complete_closed_publication(&mut owner, reference, &raw)?;
                }
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
                let result = run(&owner, &mut settlement, false, None);
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
    pub(crate) fn declare_spool(
        &self,
        owner: &DerivedReservation<'_>,
        expected: u64,
    ) -> Result<Option<durable::StageSlot>> {
        self.source
            .map(|source| durable::StageSlot::create_spool(owner, source, expected))
            .transpose()
    }
    pub(crate) fn close_spool_slot(&self, slot: durable::StageSlot, clean: bool) -> Result<()> {
        let owner = DerivedReservation::acquire_inner_recovering(
            None,
            self.home,
            crate::workspace::DEFAULT_DATA_CACHE_MAX_BYTES,
            AdditionalPeak::default(),
            self.control,
            Some(self.supplemental),
            true,
            self.recovery,
        );
        match owner {
            Ok(owner) => {
                let result = slot.finish(&owner, clean);
                owner.finish(result)
            }
            Err(error) => {
                crate::metadata::historical_write::finish_historical(Err(error), slot.release())
            }
        }
    }
    pub(crate) fn publish(
        &self,
        reference: &BlobRef,
        run: impl FnOnce(
            &DerivedReservation<'_>,
            &mut StreamPublicationSettlement,
            bool,
            Option<&lycheedev_windows::filesystem::destination::ManagedStageName>,
        ) -> Result<()>,
    ) -> Result<()> {
        if self.source.is_none() {
            return self.publish_legacy(reference, run);
        }
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
        let mut slot = None;
        let mut settlement = StreamPublicationSettlement::new();
        let result = (|| {
            let directory = owner
                .directories
                .iter()
                .find(|d| d.path() == path)
                .expect("held attribution directory");
            let existing = read_record(directory, &intent_name, INTENT_BYTES, &owner.control)?;
            let complete = read_record(directory, &complete_name, COMPLETE_BYTES, &owner.control)?;
            if existing
                .as_ref()
                .is_some_and(|bytes| bytes.as_slice() != raw)
                || complete
                    .as_ref()
                    .is_some_and(|bytes| bytes.as_slice() != completion(&raw))
                || complete.is_some() && existing.is_none()
            {
                return Err(integrity());
            }
            let present = inspect_body(self.home, reference, &owner.control, false)?;
            let managed_recovery = self
                .source
                .map(|source| durable::has_shard(&owner, source, reference))
                .transpose()?
                .unwrap_or(false);
            if existing.is_some() && !present && self.recovery.is_none() && !managed_recovery {
                return Err(pending(reference, &path.join(&intent_name)));
            }
            let needs_declaration = !present || existing.is_none();
            let managed_bytes = if self.source.is_some() && needs_declaration && !managed_recovery {
                128
            } else {
                0
            };
            let record_peak = if existing.is_none() {
                RECORD_PEAK
            } else if complete.is_none() {
                COMPLETE_BYTES as u64
            } else {
                0
            };
            // Existing pending records already retain two bodies. A completed
            // missing-body repair requires one extra possible shard-stage body.
            let body_peak = if existing.is_none() {
                // A declaration-only crash already reserves one shard body.
                if managed_recovery {
                    reference.bytes
                } else {
                    sum([reference.bytes, reference.bytes])?
                }
            } else if !present && complete.is_some() && !managed_recovery {
                reference.bytes
            } else {
                0
            };
            owner.admit_shared(record_peak + managed_bytes, body_peak)?;
            owner.prepare_publication_receipt()?;
            if needs_declaration {
                if let Some(source) = self.source {
                    slot = Some(durable::StageSlot::create_shard(&owner, source, reference)?);
                }
            }
            if existing.is_none() {
                owner.publish_shared_record(&intent_name, &raw)?;
            }
            let result = run(
                &owner,
                &mut settlement,
                present,
                slot.as_ref().map(|slot| &slot.name),
            );
            let result = join(
                result,
                record_body(&owner, self.home, reference, &settlement).err(),
                &owner.control,
            );
            result?;
            // Completion is deferred until the source parent and its slot have
            // also completed their outer checked close.
            Ok(())
        })();
        let cleanup = slot.map(|slot| slot.finish(&owner, false)).transpose();
        let result = join(result, cleanup.err(), &owner.control);
        if result.is_ok() {
            owner.recovery = None;
        }
        owner.finish(result)
    }
    pub(crate) fn complete_identified(&self, reference: &BlobRef) -> Result<()> {
        let Some(source) = self.source else {
            return Ok(());
        };
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
        let result = (|| {
            let raw = intent(reference)?;
            let guard = ExclusiveLock::resource_wait_checked(
                &self.home.join("locks"),
                &format!("blob:{}", reference.sha256),
                Some(self.control.deadline()),
                || owner.control.check(),
            )?;
            let inspected = (|| {
                let directory = owner
                    .directories
                    .iter()
                    .find(|d| d.path() == owner.paths.shared_blobs())
                    .expect("held records");
                let original = read_record(
                    directory,
                    &format!("{}.intent", reference.sha256),
                    INTENT_BYTES,
                    &owner.control,
                )?;
                if original.as_deref() != Some(raw.as_slice())
                    || !inspect_body(self.home, reference, &owner.control, true)?
                {
                    return Err(integrity());
                }
                let complete = read_record(
                    directory,
                    &format!("{}.complete", reference.sha256),
                    COMPLETE_BYTES,
                    &owner.control,
                )?;
                if complete
                    .as_ref()
                    .is_some_and(|done| done.as_slice() != completion(&raw))
                {
                    return Err(integrity());
                }
                Ok((
                    complete.is_some(),
                    durable::has_shard(&owner, source, reference)?,
                ))
            })();
            // The already-open verified body pins its identity. Quota excludes
            // all managed-stage writers after the canonical lock closes.
            let (complete, managed) = join(inspected, guard.close().err(), &owner.control)?;
            if !complete {
                if !managed {
                    return Ok(());
                } // protected legacy pending
                owner.admit_shared(COMPLETE_BYTES as u64, 0)?;
                owner.publish_shared_record(
                    &format!("{}.complete", reference.sha256),
                    &completion(&raw),
                )?;
            }
            durable::finish_shards(&owner, source, reference)
        })();
        if result.is_ok() {
            owner.recovery = None;
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
        None,
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
    source: Option<&durable::SourceIdentity>,
) -> Result<VerifiedBlob> {
    let supplemental = Arc::new(supplemental);
    let check = || supplemental();
    let quota = StreamQuota {
        home,
        control,
        supplemental: &check,
        recovery,
        source,
    };
    if let Some(source) = source {
        crate::workspace::open_controlled_with_check(home, control, &check)?;
        if expected_bytes > maximum || expected_bytes > evidence::MAX_CAPTURE_BYTES {
            return Err(integrity());
        }
        // Read-only source validation can reclaim a declared old prefix even
        // when no new spool fits. This never invokes the producer twice.
        durable::recover_spool_once(
            home,
            source,
            reader,
            expected_bytes,
            recovery,
            control,
            &check,
            accounting,
        )?;
    }
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

/// Production streaming requires immutable source/policy identity for durable
/// predeclared spooling and authenticated crash recovery.
#[allow(clippy::too_many_arguments)]
pub fn publish_shared_blob_stream_identified_with_check(
    home: &Path,
    reader: &mut impl Read,
    expected_bytes: u64,
    maximum: u64,
    control: &OperationControl,
    supplemental: impl Fn() -> Result<()> + Send + Sync + 'static,
    accounting: &impl CaptureAccounting,
    source: &durable::SourceIdentity,
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
        Some(source),
    )
}
#[allow(clippy::too_many_arguments)]
pub fn rebuild_shared_blob_stream_identified_with_check(
    home: &Path,
    reader: &mut impl Read,
    expected: &BlobRef,
    maximum: u64,
    control: &OperationControl,
    supplemental: impl Fn() -> Result<()> + Send + Sync + 'static,
    accounting: &impl CaptureAccounting,
    source: &durable::SourceIdentity,
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
        Some(source),
    )
}
