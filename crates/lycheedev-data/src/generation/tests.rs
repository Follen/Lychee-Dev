//! Declared regression coverage for the typed producer and real local prepare
//! command. Synthetic WDC/CASC inputs do not prove actual-client performance.
use super::*;
use crate::{
    resource::{Budget, Cost},
    schema, table,
};
use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Int64Array, StringArray, UInt32Array, UInt64Array,
};
use arrow_ipc::reader::FileReader;
use arrow_schema::{DataType, Field, Schema};
use lycheedev_core::{Invocation, Result, storage, workspace::data_disk::DataCachePaths};
use std::{
    collections::BTreeSet,
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};

fn budget() -> Arc<Budget> {
    Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    ))
}
fn definition(spec: &fixture::Table) -> schema::Definition {
    schema::Document::parse(spec.dbd.as_bytes())
        .unwrap()
        .select(fixture::BUILD, &format!("{:08X}", spec.layout))
        .unwrap()
}
fn open<'a>(
    spec: &'a fixture::Table,
    definition: &schema::Definition,
    budget: &Arc<Budget>,
) -> table::Table<'a> {
    table::open_budgeted(
        &spec.raw,
        definition,
        table::TableLimits::default(),
        &BTreeSet::new(),
        &[],
        budget.clone(),
    )
    .unwrap()
}
fn limits(rows: usize, bytes: usize) -> ImportLimits {
    ImportLimits {
        rows,
        value_bytes: bytes,
        text_bytes: 1 << 20,
    }
}
fn command(fixture: &Fixture, name: &str) -> Invocation {
    let mut inv = fixture.invocation("data cache prepare", &[]);
    inv.positional = Some(name.to_owned());
    inv
}

#[test]
fn typed_batches_preserve_exact_i64_float32_strings_and_sorted_logical_ids() {
    let spec = fixture::table(
        "Typed",
        811,
        &[
            ("ID", "uint"),
            ("Integer", "signed64"),
            ("Float", "float"),
            ("Text", "text"),
        ],
        &[
            vec![
                Cell::U(9),
                Cell::I64(i64::MIN),
                Cell::F(-0.0),
                Cell::Text("中文🙂".into()),
            ],
            vec![
                Cell::U(2),
                Cell::I64(9_007_199_254_740_993),
                Cell::F(0.1),
                Cell::Text("".into()),
            ],
        ],
    );
    let budget = budget();
    let view = open(&spec, &definition(&spec), &budget);
    let mut importer = TableImporter::new(&view, budget, limits(3, 4096)).unwrap();
    let batch = importer.next_batch().unwrap().unwrap();
    assert_eq!(batch.record_ids().values().as_ref(), &[2, 9]);
    assert_eq!(batch.locate(9), Some(1));
    assert_eq!(batch.locate(3), None);
    let integers = batch
        .records()
        .column(1)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(
        integers.values().as_ref(),
        &[9_007_199_254_740_993, i64::MIN]
    );
    let floats = batch
        .records()
        .column(2)
        .as_any()
        .downcast_ref::<Float32Array>()
        .unwrap();
    assert_eq!(floats.value(0).to_bits(), 0.1f32.to_bits());
    assert_eq!(floats.value(1).to_bits(), (-0.0f32).to_bits());
    let strings = batch
        .records()
        .column(3)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(strings.value(0), "");
    assert_eq!(strings.value(1), "中文🙂");
    assert!(importer.next_batch().unwrap().is_none());
}

#[test]
fn typed_batches_preserve_u64_max_without_float_coercion() {
    let spec = fixture::table(
        "Unsigned",
        812,
        &[("ID", "uint"), ("Value", "signed64")],
        &[vec![Cell::U(1), Cell::I64(-1)]],
    );
    let mut definition = definition(&spec);
    definition.fields[1].signed = false;
    let budget = budget();
    let view = open(&spec, &definition, &budget);
    let mut importer = TableImporter::new(&view, budget, limits(1, 64)).unwrap();
    let batch = importer.next_batch().unwrap().unwrap();
    assert_eq!(
        batch
            .records()
            .column(1)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .value(0),
        u64::MAX
    );
}

#[test]
fn typed_fixed_arrays_preserve_element_order_and_element_types() {
    let spec = fixture::table(
        "Arrays",
        813,
        &[
            ("ID", "uint"),
            ("Unsigned", "array3"),
            ("Signed", "array2signed"),
            ("Float", "array2float"),
        ],
        &[vec![
            Cell::U(1),
            Cell::Array(vec![Cell::U(7), Cell::U(0), Cell::U(u32::MAX)]),
            Cell::Array(vec![Cell::I(-3), Cell::I(5)]),
            Cell::Array(vec![Cell::F(0.1), Cell::F(-0.0)]),
        ]],
    );
    let budget = budget();
    let view = open(&spec, &definition(&spec), &budget);
    let mut importer = TableImporter::new(&view, budget, limits(1, 256)).unwrap();
    let batch = importer.next_batch().unwrap().unwrap();
    let unsigned = batch
        .records()
        .column(1)
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .unwrap()
        .value(0);
    assert_eq!(
        unsigned
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .values()
            .as_ref(),
        &[7, 0, u32::MAX as u64]
    );
    let signed = batch
        .records()
        .column(2)
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .unwrap()
        .value(0);
    assert_eq!(
        signed
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .values()
            .as_ref(),
        &[-3, 5]
    );
    let floats = batch
        .records()
        .column(3)
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .unwrap()
        .value(0);
    assert_eq!(
        floats
            .as_any()
            .downcast_ref::<Float32Array>()
            .unwrap()
            .value(1)
            .to_bits(),
        (-0.0f32).to_bits()
    );
}

#[test]
fn pending_row_moves_once_across_a_byte_bounded_batch() {
    let rows = (1..=3)
        .map(|id| vec![Cell::U(id), Cell::Text("x".repeat(350))])
        .collect::<Vec<_>>();
    let spec = fixture::table("Bounds", 814, &[("ID", "uint"), ("Name", "text")], &rows);
    let budget = budget();
    let view = open(&spec, &definition(&spec), &budget);
    let oracle = open(&spec, &definition(&spec), &budget);
    let mut importer = TableImporter::new(&view, budget, limits(32, 500)).unwrap();
    for id in 1..=3 {
        let batch = importer.next_batch().unwrap().unwrap();
        assert_eq!(batch.records().num_rows(), 1);
        assert_eq!(batch.record_ids().value(0), id);
        assert_eq!(batch.ordinal(), id as usize - 1);
        oracle.leased_row_values(id, 1 << 20).unwrap();
    }
    assert!(importer.next_batch().unwrap().is_none());
    assert_eq!(
        view.coverage()["decodeWork"],
        oracle.coverage()["decodeWork"],
        "pending rows are not decoded twice"
    );
}

#[test]
fn retained_batch_and_schema_lease_outlive_the_importer() {
    let spec = fixture::table(
        "Lease",
        815,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(7)]],
    );
    let budget = budget();
    let view = open(&spec, &definition(&spec), &budget);
    let before = budget.refundable_retained_bytes();
    let mut importer = TableImporter::new(&view, budget.clone(), limits(1, 256)).unwrap();
    let batch = importer.next_batch().unwrap().unwrap();
    drop(importer);
    assert!(budget.refundable_retained_bytes() > before);
    assert_eq!(batch.records().num_rows(), 1);
    drop(batch);
    assert_eq!(budget.refundable_retained_bytes(), before);
}

#[test]
fn cancelled_importer_stops_and_does_not_resume_or_replay() {
    let spec = fixture::table(
        "Cancel",
        816,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(7)], vec![Cell::U(2), Cell::U(8)]],
    );
    let cancelled = Arc::new(AtomicBool::new(false));
    let budget = Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(cancelled.clone()),
    ));
    let view = open(&spec, &definition(&spec), &budget);
    let mut importer = TableImporter::new(&view, budget, limits(1, 256)).unwrap();
    assert_eq!(
        importer
            .next_batch()
            .unwrap()
            .unwrap()
            .record_ids()
            .value(0),
        1
    );
    cancelled.store(true, Ordering::Release);
    assert_eq!(
        importer.next_batch().err().unwrap().code,
        "operation.cancelled"
    );
    assert_eq!(
        importer.next_batch().err().unwrap().code,
        "records.generation_stopped"
    );
}

#[test]
fn real_prepare_publishes_authenticated_ipc_once_and_never_prefetches_other_tables() {
    let fx = Fixture::new(&[
        fixture::table(
            "Sample",
            201,
            &[("ID", "uint"), ("Value", "signed64")],
            &[vec![Cell::U(1), Cell::I64(9_007_199_254_740_993)]],
        ),
        fixture::table(
            "Unused",
            202,
            &[("ID", "uint"), ("Text", "text")],
            &[vec![Cell::U(1), Cell::BadString]],
        ),
    ]);
    let inv = command(&fx, "Sample");
    let first = crate::dispatch(&inv).unwrap();
    assert!(first.complete);
    assert_eq!(first.data["sourceKind"], "static");
    assert_eq!(first.data["availableRows"], 1);
    assert_eq!(
        first.data["diskReservation"]["configuredMaxBytes"],
        5_000_000_000u64
    );
    assert_eq!(
        first.data["diskReservation"]["coverage"]["totalDataCoverageComplete"],
        false
    );
    let paths = DataCachePaths::new(&fx.home);
    let object = paths
        .object(first.data["object"]["sha256"].as_str().unwrap())
        .unwrap();
    let bytes = fs::read(&object).unwrap();
    assert_eq!(
        storage::sha256(&bytes),
        first.data["object"]["sha256"].as_str().unwrap()
    );
    assert_eq!(
        bytes.len() as u64,
        first.data["object"]["bytes"].as_u64().unwrap()
    );
    let mut reader = FileReader::try_new(std::io::Cursor::new(&bytes), None).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(paths.generations.join(format!(
            "{}.json",
            first.data["generation"].as_str().unwrap()
        )))
        .unwrap(),
    )
    .unwrap();
    let independently_read_schema =
        application::physical_schema_digest(reader.schema().as_ref(), &budget()).unwrap();
    assert_eq!(manifest["typedSchemaSha256"], independently_read_schema);
    assert_eq!(first.data["typedSchemaSHA256"], independently_read_schema);
    assert_eq!(
        manifest["derivationVersion"],
        application::DERIVATION_VERSION
    );
    assert_eq!(manifest["representation"], application::REPRESENTATION);
    // Inspect the actual first IPC message stored by FileWriter, not just a
    // reconstructed schema. Arrow58 defaults to 64-byte header alignment.
    let encoded = arrow_ipc::writer::IpcDataGenerator::default()
        .schema_to_bytes_with_dictionary_tracker(
            reader.schema().as_ref(),
            &mut arrow_ipc::writer::DictionaryTracker::new(true),
            &arrow_ipc::writer::IpcWriteOptions::default(),
        );
    assert_eq!(&bytes[..6], b"ARROW1");
    assert_eq!(&bytes[64..68], &[255, 255, 255, 255]);
    let metadata_length = u32::from_le_bytes(bytes[68..72].try_into().unwrap()) as usize;
    assert!(encoded.ipc_message.len() <= metadata_length);
    assert_eq!(
        &bytes[72..72 + encoded.ipc_message.len()],
        encoded.ipc_message.as_slice()
    );
    assert!(
        bytes[72 + encoded.ipc_message.len()..72 + metadata_length]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(
        storage::sha256(&encoded.ipc_message),
        independently_read_schema
    );
    let batch = reader.next().unwrap().unwrap();
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        9_007_199_254_740_993
    );
    assert_eq!(batch.num_columns(), 3);
    assert_eq!(
        batch
            .column(2)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap()
            .value(0),
        1
    );
    assert_eq!(
        reader.schema().field(2).metadata()["lycheedev.role"],
        "logical-record-id"
    );
    assert!(reader.next().is_none());
    let second = crate::dispatch(&inv).unwrap();
    assert_eq!(first.data["generation"], second.data["generation"]);
    assert_eq!(first.data["object"], second.data["object"]);
    assert_eq!(fs::read_dir(&paths.generations).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
    drop(reader);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn prepare_preserves_partial_key_coverage_and_never_promotes_it_to_complete() {
    let spec = fixture::encrypted_sections(201);
    let (_, encoded) = fixture::encrypted_frame(&spec.raw);
    let fx = Fixture::with_encoded_table(spec, encoded);
    let result = crate::dispatch(&command(&fx, "Sample")).unwrap();
    assert!(!result.complete);
    assert_eq!(result.data["complete"], false);
    assert_eq!(result.data["availableRows"], 1);
    assert_eq!(result.data["missing"][0]["keyID"], "1234567890abcdef");
    assert_eq!(result.data["unavailablePartitions"], serde_json::json!([1]));
    let paths = DataCachePaths::new(&fx.home);
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(paths.generations.join(format!(
            "{}.json",
            result.data["generation"].as_str().unwrap()
        )))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["complete"], false);
    assert_eq!(manifest["sourceContent"]["sha256"], "");
    assert!(manifest["partialContent"]["sha256"].as_str().unwrap().len() == 64);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn bad_row_preparation_never_publishes_a_generation_and_cleans_private_staging() {
    let fx = Fixture::new(&[fixture::table(
        "Broken",
        203,
        &[("ID", "uint"), ("Text", "text")],
        &[vec![Cell::U(1), Cell::BadString]],
    )]);
    let error = crate::dispatch(&command(&fx, "Broken")).unwrap_err();
    assert_eq!(error.code, "table.invalid_format");
    let paths = DataCachePaths::new(&fx.home);
    assert_eq!(fs::read_dir(&paths.generations).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn low_derived_quota_preserves_existing_evidence_and_publishes_nothing() {
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(42)]],
    )]);
    let capture = lycheedev_core::evidence::commit_capture(
        &fx.home,
        b"already retained",
        "application/octet-stream",
        serde_json::json!({"kind":"test-fixture","locator":"fixture:quota","snapshot":fx.pin}),
        true,
        false,
    )
    .unwrap();
    let original = lycheedev_core::evidence::read_blob(&fx.home, &capture["blob"], 1024).unwrap();
    fs::write(fx.home.join("config.json"), br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":21474836480},"dataCache":{"maxBytes":1}}"#).unwrap();
    let error = crate::dispatch(&command(&fx, "Sample")).unwrap_err();
    assert_eq!(error.code, "vault.data_cache_budget_exceeded");
    assert_eq!(
        lycheedev_core::evidence::read_blob(&fx.home, &capture["blob"], 1024).unwrap(),
        original
    );
    assert_eq!(
        lycheedev_core::evidence::read_capture(&fx.home, capture["id"].as_str().unwrap()).unwrap(),
        capture
    );
    let paths = DataCachePaths::new(&fx.home);
    if paths.generations.exists() {
        assert_eq!(fs::read_dir(paths.generations).unwrap().count(), 0);
    }
    if paths.staging.exists() {
        assert_eq!(fs::read_dir(paths.staging).unwrap().count(), 0);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn cancellation_after_ipc_finish_aborts_before_object_or_generation_publication() {
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(42)]],
    )]);
    let inv = command(&fx, "Sample");
    let cancel = inv.control.cancel_handle();
    let mut reached = false;
    let error = application::dispatch_after_encode(&inv, &mut || {
        reached = true;
        cancel.store(true, Ordering::Release);
        Ok(())
    })
    .unwrap_err();
    assert!(
        reached,
        "the real Arrow encoder and provider ran before cancellation"
    );
    assert_eq!(error.code, "operation.cancelled");
    let paths = DataCachePaths::new(&fx.home);
    assert_eq!(fs::read_dir(&paths.generations).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
    // Quota/source owners have already completed checked teardown.
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn generation_identity_ignores_runtime_counters_but_binds_locale_schema_and_key_coverage() {
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(42)]],
    )]);
    let inv = command(&fx, "Sample");
    let budget = Arc::new(Budget::query_controlled(
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let mut scope = crate::remote::provider::OpenedScope::open(&inv, &budget)
        .unwrap()
        .admit()
        .unwrap();
    let result = (|| -> Result<()> {
        let pin = scope.resolve_pin()?;
        scope.with_local_session(pin.data.as_ref().unwrap(), |reader| {
            let mut prepared = crate::db2::prepare_scoped(
                &inv,
                "Sample",
                &budget,
                Some(&mut *reader),
                Some(pin.clone()),
            )?;
            let digest = StaticIdentity::from_prepared(&prepared)?.digest(&budget)?;
            let identity = StaticIdentity::from_prepared(&prepared)?;
            let schema_hash = "11".repeat(32);
            let generation = identity.generation_digest(
                &schema_hash,
                application::REPRESENTATION,
                application::DERIVATION_VERSION,
                &budget,
            )?;
            assert_ne!(
                generation,
                identity.generation_digest(
                    &"22".repeat(32),
                    application::REPRESENTATION,
                    application::DERIVATION_VERSION,
                    &budget
                )?
            );
            assert_ne!(
                generation,
                identity.generation_digest(
                    &schema_hash,
                    "different-representation",
                    application::DERIVATION_VERSION,
                    &budget
                )?
            );
            assert_ne!(
                generation,
                identity.generation_digest(
                    &schema_hash,
                    application::REPRESENTATION,
                    "different-derivation",
                    &budget
                )?
            );
            prepared.file.reading["decodedCache"] = serde_json::json!({"hits":999,"duration":8765});
            prepared.file.reading["encodingIO"] = serde_json::json!({"bytes":123});
            assert_eq!(
                digest,
                StaticIdentity::from_prepared(&prepared)?.digest(&budget)?
            );
            prepared.pin.spec.data.as_mut().unwrap().language = "enUS".into();
            assert_ne!(
                digest,
                StaticIdentity::from_prepared(&prepared)?.digest(&budget)?
            );
            prepared.pin.spec.data.as_mut().unwrap().language = "zhCN".into();
            prepared.definition.fields[1].signed = !prepared.definition.fields[1].signed;
            assert_ne!(
                digest,
                StaticIdentity::from_prepared(&prepared)?.digest(&budget)?
            );
            prepared.definition.fields[1].signed = !prepared.definition.fields[1].signed;
            prepared.file.reading["keySource"] =
                serde_json::json!({"kind":"fixture","sha256":"11".repeat(32)});
            assert_ne!(
                digest,
                StaticIdentity::from_prepared(&prepared)?.digest(&budget)?
            );
            reader.verify_source()
        })
    })();
    scope.finish(result).unwrap();
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn changed_original_source_after_ipc_finish_never_acquires_a_generation() {
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(42)]],
    )]);
    let inv = command(&fx, "Sample");
    let version = fx.client.join("version.txt");
    let original = fs::read(&version).unwrap();
    let mut reached = false;
    let result = application::dispatch_after_encode(&inv, &mut || {
        reached = true;
        fs::write(&version, b"12.1.0.69934")?;
        Ok(())
    });
    fs::write(&version, original).unwrap();
    assert!(reached);
    let error = result.unwrap_err();
    assert_eq!(error.code, "records.pinned_build_changed");
    let paths = DataCachePaths::new(&fx.home);
    assert_eq!(fs::read_dir(&paths.generations).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
    assert!(crate::dispatch(&inv).unwrap().complete);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn checked_scope_close_error_retains_the_published_generation_receipt() {
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[vec![Cell::U(1), Cell::U(42)]],
    )]);
    let inv = command(&fx, "Sample");
    let mut closes = 0;
    let error = application::dispatch_after_hooks(&inv, &mut || Ok(()), &mut |result| {
        assert!(result.is_ok());
        closes += 1;
        // The real quota, local provider and workspace owners have closed.
        // Inject only the checked terminal result, never replay publication.
        lycheedev_core::workspace::data_resources::acquire(
            &fx.home,
            lycheedev_core::workspace::data_resources::Policy::Measurement,
            &lycheedev_core::control::OperationControl::default(),
        )?
        .close()?;
        Err(
            crate::fault("vault.close_failed", 5, "fixture checked close failure")
                .with_details(serde_json::json!({"fixtureCause":"after-real-close"})),
        )
    })
    .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "vault.close_failed");
    assert_eq!(
        error.details["primary"]["details"]["fixtureCause"],
        "after-real-close"
    );
    let published = &error.details["publishedGeneration"];
    assert_eq!(published["generationPublished"], true);
    let paths = DataCachePaths::new(&fx.home);
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(paths.generations.join(format!(
            "{}.json",
            published["generation"].as_str().unwrap()
        )))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["object"], published["object"]);
    assert_eq!(fs::read_dir(&paths.generations).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn physical_schema_digest_is_canonical_and_binds_all_arrow_schema_semantics() {
    use std::collections::HashMap;
    let fields = vec![
        Field::new("Value", DataType::Int64, true),
        Field::new(
            "Array",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::UInt64, true)), 2),
            true,
        ),
        Field::new("", DataType::UInt32, false).with_metadata(HashMap::from([
            ("lycheedev.role".into(), "logical-record-id".into()),
            ("second-key".into(), "second-value".into()),
        ])),
    ];
    let schema = Schema::new_with_metadata(
        fields.clone(),
        HashMap::from([("a".into(), "first".into()), ("z".into(), "last".into())]),
    );
    let ledger = budget();
    let baseline = application::physical_schema_digest(&schema, &ledger).unwrap();
    let before = ledger.snapshot().unwrap().retained_bytes;
    let mut reordered_fields = fields.clone();
    reordered_fields[2] = Field::new("", DataType::UInt32, false).with_metadata(HashMap::from([
        ("second-key".into(), "second-value".into()),
        ("lycheedev.role".into(), "logical-record-id".into()),
    ]));
    let reordered_maps = Schema::new_with_metadata(
        reordered_fields,
        HashMap::from([("z".into(), "last".into()), ("a".into(), "first".into())]),
    );
    assert_eq!(
        baseline,
        application::physical_schema_digest(&reordered_maps, &ledger).unwrap()
    );
    assert_eq!(
        before,
        ledger.snapshot().unwrap().retained_bytes,
        "canonical schema scratch refunds after hashing"
    );
    let mut variants = Vec::new();
    let mut reordered = fields.clone();
    reordered.swap(0, 1);
    variants.push(reordered);
    let mut renamed = fields.clone();
    renamed[0] = Field::new("Renamed", DataType::Int64, true);
    variants.push(renamed);
    let mut unsigned = fields.clone();
    unsigned[0] = Field::new("Value", DataType::UInt64, true);
    variants.push(unsigned);
    let mut required = fields.clone();
    required[0] = Field::new("Value", DataType::Int64, false);
    variants.push(required);
    let mut wider = fields.clone();
    wider[1] = Field::new(
        "Array",
        DataType::FixedSizeList(Arc::new(Field::new("item", DataType::UInt64, true)), 3),
        true,
    );
    variants.push(wider);
    let mut child_required = fields.clone();
    child_required[1] = Field::new(
        "Array",
        DataType::FixedSizeList(Arc::new(Field::new("item", DataType::UInt64, false)), 2),
        true,
    );
    variants.push(child_required);
    let mut role = fields.clone();
    role[2]
        .metadata_mut()
        .insert("lycheedev.role".into(), "different-role".into());
    variants.push(role);
    let mut locator_type = fields;
    locator_type[2] = locator_type[2].clone().with_data_type(DataType::UInt64);
    variants.push(locator_type);
    for changed in variants {
        let changed = Schema::new_with_metadata(changed, schema.metadata.clone());
        assert_ne!(
            baseline,
            application::physical_schema_digest(&changed, &ledger).unwrap()
        );
    }
}
