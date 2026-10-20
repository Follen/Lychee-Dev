//! Ordinary static SQL consumes one authenticated fixed generation at a time.
//! Quota is held only during manifest/open; the original immutable reader stays
//! under the resolver's original admission until its consuming checked close.
use super::{
    StaticIdentity, StaticSource, application,
    import::{self, ImportedSchema},
    reading,
};
use crate::{
    phase_profile::Phase,
    remote::provider::FileScope,
    resource::{Budget, Cost, OwnedRetainedReservation},
    table,
};
use lycheedev_core::{
    Result, ToolkitError,
    workspace::data_disk::{DerivedObject, DerivedReadCheck, OwnedDerivedObjectReader},
};
use std::{cell::RefCell, sync::Arc};

pub(crate) enum Reuse {
    Absent,
    UnsupportedSchema,
    MissingBody {
        generation: String,
        object: DerivedObject,
    },
    Hit(Arc<Generation>),
}
/// Only private query-borrowed cursors can consume this owner. A seek/read holds
/// the cell briefly and never spans a row callback, including correlated scans.
pub(crate) struct Generation {
    pub(crate) key: String,
    pub(crate) object: DerivedObject,
    reader: RefCell<Option<OwnedDerivedObjectReader>>,
    directory: reading::ipc::Directory,
    schema: application::PhysicalSchema,
    _visible_schema: Arc<ImportedSchema>,
    budget: Arc<Budget>,
    _allocation: OwnedRetainedReservation,
}
impl Generation {
    pub(crate) fn batch_count(&self) -> usize {
        self.directory.blocks.len()
    }
    pub(crate) fn locate_batch(&self, id: u32) -> Option<usize> {
        let index = self
            .directory
            .blocks
            .partition_point(|entry| entry.last < id);
        self.directory
            .blocks
            .get(index)
            .filter(|entry| entry.first <= id)
            .map(|_| index)
    }
    pub(crate) fn read_batch(&self, index: usize) -> Result<reading::ipc::OwnedBatch> {
        let _phase = self.budget.phase(Phase::GenerationBatch);
        self.budget.check()?;
        let entry = self
            .directory
            .blocks
            .get(index)
            .ok_or_else(reading::invalid)?;
        let mut owner = self.reader.try_borrow_mut().map_err(|_| reentered())?;
        let reader = owner.as_mut().ok_or_else(reentered)?;
        let batch = reading::ipc::read_owned_batch(
            reader,
            &self.schema.schema,
            &entry.block,
            &self.budget,
        )?;
        if batch.records.num_rows() != entry.rows {
            return Err(reading::invalid());
        }
        #[cfg(test)]
        observations::BATCH_READS.with(|count| count.set(count.get() + 1));
        Ok(batch)
    }
    /// All cursors are query borrows, so consuming Resolver closes only after
    /// they have ended. Multiple aliases share this one close, not a cloned File.
    pub(crate) fn close(&self) -> Result<()> {
        let reader = self
            .reader
            .try_borrow_mut()
            .map_err(|_| reentered())?
            .take();
        match reader {
            Some(reader) => {
                let result = reader.close_after(Ok(()));
                #[cfg(test)]
                let result = observations::after_close(result);
                result
            }
            None => Ok(()),
        }
    }
}
fn reentered() -> ToolkitError {
    crate::fault(
        "records.internal",
        8,
        "Typed generation reader is closed or reentered",
    )
}

pub(crate) fn open_existing(
    scope: &dyn FileScope,
    source: StaticSource<'_>,
    table: &table::Table<'_>,
    maximum: usize,
    budget: &Arc<Budget>,
) -> Result<Reuse> {
    let _phase = budget.phase(Phase::GenerationOpen);
    budget.check()?;
    #[cfg(test)]
    if observations::ORIGINAL_WDC.with(|flag| flag.get()) {
        return Ok(Reuse::Absent);
    }
    if !application::source_schema_may_fit(table.columns(), budget)? {
        return Ok(Reuse::UnsupportedSchema);
    }
    let identity = StaticIdentity::from_source(source)?;
    let visible_schema = import::schema_for_table(table, budget)?;
    let schema = application::PhysicalSchema::new(visible_schema.schema(), budget)?;
    let Some(schema_sha) =
        application::physical_schema_digest_if_supported(&schema.schema, budget)?
    else {
        return Ok(Reuse::UnsupportedSchema);
    };
    let key = identity.generation_digest(
        &schema_sha,
        application::REPRESENTATION,
        application::DERIVATION_VERSION,
        budget,
    )?;
    let maximum_object = (maximum as u64)
        .checked_mul(4)
        .and_then(|n| n.checked_add(16 << 20))
        .ok_or_else(reading::invalid)?;
    let check_budget = budget.clone();
    let check: DerivedReadCheck = Arc::new(move || check_budget.check());
    let mut receipt_allocation = None;
    let lease = {
        let _phase = budget.phase(Phase::GenerationQuota);
        scope.reserve_derived_read_owned(&check)?
    };
    let receipt = (|| {
        let _phase = budget.phase(Phase::GenerationManifest);
        let Some(bytes) = lease.read_generation_with_admission(&key, |length| {
            #[cfg(test)]
            observations::MANIFEST_ADMISSIONS.with(|n| n.set(n.get() + 1));
            let scratch = length
                .checked_mul(4)
                .and_then(|n| n.checked_add(128 << 10))
                .ok_or_else(reading::invalid)?;
            receipt_allocation = Some(budget.reserve_retained(scratch)?);
            Ok(())
        })?
        else {
            return Ok(None);
        };
        budget.charge(Cost::work(bytes.len()))?;
        let receipt: reading::Receipt<'_> =
            serde_json::from_slice(&bytes).map_err(|_| reading::invalid())?;
        if receipt.available_rows != table.logical_count()
            || receipt.batch_count > receipt.available_rows
            || (receipt.available_rows != 0 && receipt.batch_count == 0)
            || receipt.object.bytes == 0
            || receipt.object.bytes > maximum_object
            || !lycheedev_core::selection::valid_digest(receipt.object.sha256, 64)
            || receipt
                .object
                .sha256
                .bytes()
                .any(|b| b.is_ascii_uppercase())
        {
            return Err(reading::invalid());
        }
        budget.safety_only().charge(Cost::retained(512))?;
        let object = DerivedObject {
            sha256: receipt.object.sha256.to_owned(),
            bytes: receipt.object.bytes,
        };
        let expected = application::manifest_source(
            source,
            &identity,
            &key,
            &schema_sha,
            &object,
            receipt.available_rows,
            receipt.batch_count,
            schema.schema.fields().len() - 1,
        );
        reading::canonical_matches(&expected, &bytes, budget)?;
        Ok(Some((object, receipt.batch_count)))
    })();
    let (object, batches) = match receipt {
        Ok(Some(receipt)) => receipt,
        Ok(None) => return lease.finish(Ok(Reuse::Absent)),
        Err(error) => return lease.finish(Err(error)),
    };
    drop(receipt_allocation);
    // The original SHA/length/EOF pass is admitted before Core performs it.
    let admitted = usize::try_from(object.bytes)
        .map_err(|_| reading::invalid())
        .and_then(|bytes| reading::admit_bulk_work(budget, bytes));
    if let Err(error) = admitted {
        return lease.finish(Err(error));
    }
    let opened = {
        let _phase = budget.phase(Phase::GenerationObject);
        lease.open_pinned_object(&object.sha256, object.bytes)?
    };
    let Some(mut reader) = opened else {
        // SQL may use its already authenticated WDC source without persistence.
        // A valid manifest with only a missing body is explicitly degraded, not
        // a hit, a repair, or permission to hide other authentication failures.
        return Ok(Reuse::MissingBody {
            generation: key,
            object,
        });
    };
    let prepared = (|| {
        let allocation = budget.reserve_retained(2048)?.into_owned(budget)?;
        let directory = {
            let _phase = budget.phase(Phase::GenerationDirectory);
            reading::ipc::directory(&mut reader, &schema.schema, table, batches, budget)?
        };
        budget.check()?;
        Ok((allocation, directory))
    })();
    let (allocation, directory) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return reader.close_after(Err(error)),
    };
    Ok(Reuse::Hit(Arc::new(Generation {
        key,
        object,
        reader: RefCell::new(Some(reader)),
        directory,
        schema,
        _visible_schema: visible_schema,
        budget: budget.clone(),
        _allocation: allocation,
    })))
}

#[cfg(test)]
pub(crate) mod observations {
    use super::*;
    use std::cell::Cell;
    thread_local! {
        pub(crate) static MANIFEST_ADMISSIONS: Cell<usize> = const { Cell::new(0) };
        pub(crate) static ORIGINAL_WDC: Cell<bool> = const { Cell::new(false) };
        pub(crate) static BATCH_READS: Cell<usize> = const { Cell::new(0) };
        pub(crate) static CLOSES: Cell<usize> = const { Cell::new(0) };
        pub(crate) static FAIL_CLOSES: Cell<usize> = const { Cell::new(0) };
    }
    pub(super) fn after_close(result: Result<()>) -> Result<()> {
        CLOSES.with(|count| count.set(count.get() + 1));
        let fail = FAIL_CLOSES.with(|count| {
            let pending = count.get();
            count.set(pending.saturating_sub(1));
            pending != 0
        });
        let error = fail.then(|| {
            crate::fault(
                "storage.close_failed",
                4,
                "Injected after real typed reader close",
            )
        });
        crate::remote::admission::finish(result, error, Ok(()), Ok(()))
    }
}
