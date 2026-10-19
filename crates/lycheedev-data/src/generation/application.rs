//! Explicit one-table preparation through the real admitted provider. This
//! command does not alter existing JSONL callbacks or prefetch other tables.
use super::{ImportLimits, StaticIdentity, TableImporter};
use crate::{
    capture, db2, fault,
    remote::provider::{FileScope, OpenedScope},
    resource::{Budget, Cost, OwnedRetainedReservation, RetainedReservation},
    table,
};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_ipc::writer::{DictionaryTracker, FileWriter, IpcDataGenerator, IpcWriteOptions};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError, selection,
    workspace::data_disk::{AdditionalPeak, DerivedObject, DerivedReservation, ReservationReport},
};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

const MANIFEST_MAXIMUM: usize = 1 << 20;
#[cfg(test)]
thread_local! {
    pub(super) static IMPORTED_BATCH_REQUESTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) const REPRESENTATION: &str = "arrow-ipc-file-v5/arrow-58.3.0/uncompressed";
pub(super) const DERIVATION_VERSION: &str = "lycheedev.table-arrow-import.v1";

// The exact Schema lent to FileWriter owns its construction reservation until
// every physical batch and writer has been dropped.
struct PhysicalSchema {
    schema: SchemaRef,
    scratch_bound: usize,
    _allocation: OwnedRetainedReservation,
}
impl PhysicalSchema {
    fn new(visible: &SchemaRef, budget: &Arc<Budget>) -> Result<Self> {
        let scratch_bound = visible
            .fields()
            .iter()
            .try_fold(4096usize, |total, field| {
                total
                    .checked_add(field.name().len())
                    .and_then(|n| n.checked_add(512))
                    .ok_or_else(limit)
            })?;
        let allocation =
            budget.reserve_retained(scratch_bound.checked_mul(4).ok_or_else(limit)?)?;
        let mut fields = visible.fields().iter().cloned().collect::<Vec<_>>();
        // The physical-only ID locator has no DBD identifier. Its ordinal and
        // role metadata are included in the schema hash, never a SQL field.
        fields.push(Arc::new(
            Field::new("", DataType::UInt32, false).with_metadata(HashMap::from([(
                "lycheedev.role".to_owned(),
                "logical-record-id".to_owned(),
            )])),
        ));
        Ok(Self {
            schema: Arc::new(Schema::new(fields)),
            scratch_bound,
            _allocation: allocation.into_owned(budget)?,
        })
    }
}

pub(super) fn reserve_schema_encoding<'a>(
    schema: &Schema,
    budget: &'a Budget,
) -> Result<RetainedReservation<'a>> {
    budget.check()?;
    let input_bytes =
        schema
            .metadata()
            .iter()
            .try_fold(schema.fields().size(), |size, (key, value)| {
                size.checked_add(key.len())
                    .and_then(|n| n.checked_add(value.len()))
                    .and_then(|n| n.checked_add(128))
                    .ok_or_else(limit)
            })?;
    if input_bytes > MANIFEST_MAXIMUM {
        return Err(limit());
    }
    // FlatBuffer growth/old-new overlap, returned message bytes and metadata
    // ordering scratch are admitted before entering the synchronous encoder.
    budget.reserve_retained(
        input_bytes
            .checked_mul(16)
            .and_then(|n| n.checked_add(65536))
            .ok_or_else(limit)?,
    )
}

/// Hash Arrow's canonical native IPC schema message: the same encoder and
/// options FileWriter uses for this exact physical Schema. Arrow sorts metadata
/// keys while preserving field order, nested types, lengths and nullability.
pub(super) fn physical_schema_digest(schema: &Schema, budget: &Budget) -> Result<String> {
    let _encoding = reserve_schema_encoding(schema, budget)?;
    let encoded = IpcDataGenerator::default().schema_to_bytes_with_dictionary_tracker(
        schema,
        &mut DictionaryTracker::new(true),
        &IpcWriteOptions::default(),
    );
    if encoded.ipc_message.len() > MANIFEST_MAXIMUM || !encoded.arrow_data.is_empty() {
        return Err(limit());
    }
    budget.charge(Cost::work(encoded.ipc_message.len()))?;
    let digest = lycheedev_core::storage::sha256(&encoded.ipc_message);
    budget.check()?;
    Ok(digest)
}

fn limit() -> ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Typed generation metadata exceeds its bound",
    )
}
fn ipc_error(error: arrow_schema::ArrowError) -> ToolkitError {
    fault(
        "records.generation_ipc",
        4,
        format!("Typed Arrow IPC encoding failed: {error}"),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest<'a> {
    schema: &'static str,
    identity_sha256: &'a str,
    source_kind: &'static str,
    data: &'a selection::DataPin,
    table: &'a crate::definitions::TableIdentity,
    definition_sha256: &'a str,
    typed_schema_sha256: &'a str,
    object: &'a DerivedObject,
    representation: &'static str,
    derivation_version: &'static str,
    available_rows: usize,
    batch_count: usize,
    complete: bool,
    missing: &'a [crate::blte::Missing],
    unavailable_partitions: &'a std::collections::BTreeSet<usize>,
    record_id_column: usize,
    record_id_order: &'static str,
    // Original verified resources remain references. No raw table body is
    // copied into a generation and no raw/IPC/Parquet triple is created.
    source_content: &'a serde_json::Value,
    partial_content: &'a serde_json::Value,
    definition_object: &'a serde_json::Value,
    definition_manifest: &'a serde_json::Value,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn manifest<'a>(
    prepared: &'a db2::Prepared,
    identity: &StaticIdentity<'_>,
    identity_sha256: &'a str,
    schema_sha256: &'a str,
    object: &'a DerivedObject,
    rows: usize,
    batches: usize,
    visible_columns: usize,
) -> Manifest<'a> {
    Manifest {
        schema: "lycheedev.data-static-generation.v1",
        identity_sha256,
        source_kind: "static",
        data: prepared.pin.data.as_ref().expect("validated data pin"),
        table: &prepared.bundle.identity,
        definition_sha256: &prepared.definition.sha256,
        typed_schema_sha256: schema_sha256,
        object,
        representation: REPRESENTATION,
        derivation_version: DERIVATION_VERSION,
        available_rows: rows,
        batch_count: batches,
        complete: identity.complete(),
        missing: prepared.file.bytes.missing(),
        unavailable_partitions: &prepared.availability.skipped,
        record_id_column: visible_columns,
        record_id_order: "ascending",
        source_content: &prepared.file.reading["content"],
        partial_content: &prepared.file.reading["partialContent"],
        definition_object: &prepared.bundle.definition,
        definition_manifest: &prepared.bundle.manifest,
    }
}

pub(crate) fn dispatch(inv: &Invocation) -> Result<Outcome> {
    dispatch_after_encode(inv, &mut || Ok(()))
}

pub(super) fn dispatch_after_encode(
    inv: &Invocation,
    after_encode: &mut dyn FnMut() -> Result<()>,
) -> Result<Outcome> {
    dispatch_after_hooks(inv, after_encode, &mut |result| result)
}

pub(super) fn dispatch_after_hooks(
    inv: &Invocation,
    after_encode: &mut dyn FnMut() -> Result<()>,
    after_finish: &mut dyn FnMut(Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    dispatch_with_read_hook(inv, after_encode, after_finish, &mut || Ok(()))
}

pub(super) fn dispatch_with_read_hook(
    inv: &Invocation,
    after_encode: &mut dyn FnMut() -> Result<()>,
    after_finish: &mut dyn FnMut(Result<Outcome>) -> Result<Outcome>,
    after_read: &mut dyn FnMut() -> Result<()>,
) -> Result<Outcome> {
    inv.control.check()?;
    let name = inv.argument()?;
    if !crate::definitions::valid_name(name) {
        return Err(ToolkitError::arguments(
            "data cache prepare requires one valid table name",
        ));
    }
    let timeout = inv.usize_flag("--timeout-seconds", 300, 1, 3600)?;
    let maximum = inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?;
    let budget = Arc::new(Budget::query_controlled(
        Instant::now() + Duration::from_secs(timeout as u64),
        inv.control.clone(),
    ));
    let (mut scope, pin) = OpenedScope::open(inv, &budget)?.admit_pinned(fault(
        "command.failed",
        5,
        "records.data_pin_required",
    ))?;
    let mut published_generation = None;
    let result = (|| {
        let data = pin
            .data
            .as_ref()
            .ok_or_else(|| fault("command.failed", 5, "records.data_pin_required"))?;
        let mut run = |reader: &mut dyn FileScope| {
            let prepared =
                db2::prepare_scoped(inv, name, &budget, Some(&mut *reader), Some(pin.clone()))?;
            reader.check_consumer(inv, &budget)?;
            let identity = StaticIdentity::from_prepared(&prepared)?;
            let view = table::open_budgeted(
                &prepared.file.bytes,
                &prepared.definition,
                prepared.limits,
                &prepared.availability.skipped,
                &prepared.availability.ranges,
                budget.clone(),
            )?;
            let mut importer = TableImporter::new(&view, budget.clone(), ImportLimits::default())?;
            let physical_schema = PhysicalSchema::new(importer.schema(), &budget)?;
            let schema_sha256 = physical_schema_digest(&physical_schema.schema, &budget)?;
            let identity_sha256 = identity.generation_digest(
                &schema_sha256,
                REPRESENTATION,
                DERIVATION_VERSION,
                &budget,
            )?;
            // This is a hard output bound, not an asserted compression ratio.
            // Exceeding it preserves the source and never publishes a generation.
            let ipc_maximum = (maximum as u64)
                .checked_mul(4)
                .and_then(|n| n.checked_add(16 << 20))
                .ok_or_else(limit)?;
            // Keep a bounded recovery receipt until every provider/workspace
            // owner has completed checked teardown, even if teardown fails.
            budget.safety_only().charge(Cost::retained(1024))?;
            // Source preparation completes before borrowing the quota owner:
            // source/decoded-cache producers must never recursively take it.
            let check = || budget.check();
            let read_owner = reader.reserve_derived_read(&check)?;
            let read_report = read_owner.report().clone();
            let hit = super::reading::read(
                &read_owner,
                &prepared,
                &identity,
                &identity_sha256,
                &schema_sha256,
                &physical_schema.schema,
                &view,
                ipc_maximum,
                &budget,
            );
            if let Ok(Some(hit)) = &hit {
                // Keep the existing published fact even if read-owner cleanup
                // fails; source verification has not yet authorized reuse.
                published_generation = Some(json!({
                    "generation":identity_sha256,"object":hit.object,
                    "generationPublished":true,"generationReused":false,
                    "publicationThisCommand":false,"reuseVerification":"pending",
                }));
            }
            let hit = read_owner.finish(hit)?;
            if let Some(hit) = hit {
                // Original mutable source observations are still held, and are
                // verified even though no WDC row was decoded or IPC re-written.
                after_read()?;
                reader.check_consumer(inv, &budget)?;
                reader.verify_source()?;
                let compatible_report = ReservationReport {
                    configured_max_bytes: read_report.configured_max_bytes,
                    existing: read_report.existing,
                    additional_peak: AdditionalPeak::default(),
                    shared_blob_additional_peak: 0,
                    private_metadata_additional_peak: 0,
                    admitted_peak_bytes: read_report.existing.total()?,
                    coverage: read_report.coverage.clone(),
                };
                let mut disk_reservation = json!(compatible_report);
                disk_reservation["readOnly"] = json!(true);
                disk_reservation["overBudget"] = json!(read_report.over_budget);
                let mut outcome = Outcome::new(json!({
                    "generation":identity_sha256,
                    "generationPublished":true,
                    "generationReused":true,
                    "publicationThisCommand":false,
                    "sourceKind":"static",
                    "snapshot":prepared.pin.id,
                    "table":prepared.bundle.identity.name,
                    "definitionSHA256":prepared.definition.sha256,
                    "typedSchemaSHA256":schema_sha256,
                    "object":hit.object,
                    "representation":REPRESENTATION,
                    "derivationVersion":DERIVATION_VERSION,
                    "availableRows":hit.rows,
                    "batchCount":hit.batches,
                    "complete":identity.complete(),
                    "missing":prepared.file.bytes.missing(),
                    "unavailablePartitions":prepared.availability.skipped,
                    "recordIdColumn":physical_schema.schema.fields().len() - 1,
                    "recordIdOrder":"ascending",
                    "resources":budget.logical_snapshot()?,
                    "memoryBudget":budget.memory_snapshot()?,
                    "diskReservation":disk_reservation,
                    "diskRead":read_report,
                }));
                outcome.complete = identity.complete();
                outcome.selection = json!({"snapshot":prepared.pin.id});
                outcome.coverage = json!({"sourceKind":"static","complete":identity.complete(),"availableRows":hit.rows,"missing":prepared.file.bytes.missing()});
                published_generation = Some(json!({
                    "generation":identity_sha256,"object":hit.object,
                    "generationPublished":true,"generationReused":true,
                    "publicationThisCommand":false,
                }));
                return Ok(outcome);
            }
            // A miss has released its read owner. The normal write path below
            // re-admits capacity and authenticates source before no-replace CAS.
            let check = || budget.check();
            let reservation = reader.reserve_derived_cache(
                AdditionalPeak {
                    objects: ipc_maximum,
                    staging: ipc_maximum,
                    generations: MANIFEST_MAXIMUM as u64,
                    ..Default::default()
                },
                &check,
            )?;
            let report = reservation.report().clone();
            let result = write_generation(
                inv,
                reader,
                &prepared,
                &identity,
                &identity_sha256,
                &schema_sha256,
                &budget,
                &mut importer,
                &physical_schema,
                &reservation,
                ipc_maximum,
                after_encode,
            )
            .map(|mut outcome| {
                published_generation = Some(json!({
                    "generation": outcome.data["generation"],
                    "object": outcome.data["object"],
                    "generationPublished": true,
                }));
                outcome.data["diskReservation"] = json!(report);
                outcome
            });
            reservation.finish(result)
        };
        if inv.flag_bool("--cdn") {
            scope.with_session(data, |reader| run(reader))
        } else {
            scope.with_local_session(data, |reader| run(reader))
        }
    })();
    let result = scope.finish(result);
    after_finish(result).map_err(|mut error| {
        if let Some(published) = published_generation {
            let primary = error.clone();
            error
                .message
                .push_str("\nData generation already published: ");
            error
                .message
                .push_str(published["generation"].as_str().expect("generation digest"));
            error.details = json!({
                "schema":"lycheedev.data-generation-committed-error.v1",
                "primary":primary,
                "publishedGeneration":published,
            });
        }
        error
    })
}

#[allow(clippy::too_many_arguments)]
fn write_generation(
    inv: &Invocation,
    reader: &dyn FileScope,
    prepared: &db2::Prepared,
    identity: &StaticIdentity<'_>,
    identity_sha256: &str,
    schema_sha256: &str,
    budget: &Arc<Budget>,
    importer: &mut TableImporter<'_, '_>,
    physical_schema: &PhysicalSchema,
    reservation: &DerivedReservation<'_>,
    ipc_maximum: u64,
    after_encode: &mut dyn FnMut() -> Result<()>,
) -> Result<Outcome> {
    budget.check()?;
    let visible_columns = importer.schema().fields().len();
    let schema = &physical_schema.schema;
    let schema_bytes = physical_schema.scratch_bound;
    // FileWriter repeats the native schema encoding at open and finish. Its
    // scratch reservation therefore remains live for the whole writer owner.
    let _schema_encoding = reserve_schema_encoding(schema, budget)?;
    let mut object_writer = reservation.begin_object(ipc_maximum)?;
    let mut rows = 0usize;
    let mut batches = 0usize;
    let written = (|| {
        let mut writer = FileWriter::try_new(&mut object_writer, schema).map_err(ipc_error)?;
        loop {
            #[cfg(test)]
            IMPORTED_BATCH_REQUESTS.with(|count| count.set(count.get() + 1));
            let Some(batch) = importer.next_batch()? else {
                break;
            };
            budget.check()?;
            if batch.ordinal() != batches {
                return Err(fault(
                    "records.generation_order",
                    4,
                    "Typed batch order changed",
                ));
            }
            let mut columns = batch.records().columns().to_vec();
            columns.push(Arc::new(batch.record_ids().clone()) as ArrayRef);
            let physical = RecordBatch::try_new(schema.clone(), columns).map_err(ipc_error)?;
            // FileWriter retains one bounded footer entry per batch. Body and
            // locator buffers stay under ImportedBatch's live refund owner.
            budget.safety_only().charge(Cost::metadata(256))?;
            // Arrow's IPC encoder owns a separate growable encoded body. Admit
            // that measured input-sized scratch before entering its synchronous
            // call, including geometric reallocation overlap and per-field pads.
            let scratch_bytes = physical
                .get_array_memory_size()
                .checked_mul(3)
                .and_then(|n| n.checked_add(schema_bytes))
                .ok_or_else(limit)?;
            let _ipc_scratch = budget.reserve_retained(scratch_bytes)?;
            writer.write(&physical).map_err(ipc_error)?;
            rows = rows.checked_add(physical.num_rows()).ok_or_else(limit)?;
            batches = batches.checked_add(1).ok_or_else(limit)?;
        }
        writer.finish().map_err(ipc_error)?;
        after_encode()?;
        budget.check()?;
        reader.check_consumer(inv, budget)?;
        reader.verify_source()?;
        Ok(())
    })();
    if let Err(error) = written {
        let error = object_writer.error().cloned().unwrap_or(error);
        return object_writer.abort(Err(error));
    }
    let object = object_writer.commit()?;
    let published = json!({"object":object,"generationPublication":"not-confirmed"});
    let result: Result<Outcome> = (|| {
        budget.check()?;
        let manifest = manifest(
            prepared,
            identity,
            identity_sha256,
            schema_sha256,
            &object,
            rows,
            batches,
            visible_columns,
        );
        let bytes = capture::json_bytes(&manifest, budget, MANIFEST_MAXIMUM, limit)?;
        let resources = budget.logical_snapshot()?;
        let memory_budget = budget.memory_snapshot()?;
        let mut outcome = Outcome::new(json!({
            "generation":identity_sha256,
            "generationPublished":true,
            "sourceKind":"static",
            "snapshot":prepared.pin.id,
            "table":prepared.bundle.identity.name,
            "definitionSHA256":prepared.definition.sha256,
            "typedSchemaSHA256":schema_sha256,
            "object":object,
            "representation":manifest.representation,
            "derivationVersion":DERIVATION_VERSION,
            "availableRows":rows,
            "batchCount":batches,
            "complete":identity.complete(),
            "missing":prepared.file.bytes.missing(),
            "unavailablePartitions":prepared.availability.skipped,
            "recordIdColumn":visible_columns,
            "recordIdOrder":"ascending",
            "resources":resources,
            "memoryBudget":memory_budget,
        }));
        outcome.complete = identity.complete();
        outcome.selection = json!({"snapshot":prepared.pin.id});
        outcome.coverage = json!({"sourceKind":"static","complete":identity.complete(),"availableRows":rows,"missing":prepared.file.bytes.missing()});
        reservation.publish_generation(identity_sha256, &bytes)?;
        Ok(outcome)
    })();
    result.map_err(|mut error| {
        error.details = json!({"schema":"lycheedev.data-generation-publication-error.v1","cause":error.details,"published":published});
        error
    })
}
