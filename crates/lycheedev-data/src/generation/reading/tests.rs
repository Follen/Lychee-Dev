use super::*;
use crate::generation::application;
use arrow_array::{Array, Float32Array, Int64Array, StringArray, UInt32Array, UInt64Array};
use arrow_ipc::reader::FileReader;
use lycheedev_core::{Outcome, storage, workspace::data_disk::DataCachePaths};
use std::{fs, path::PathBuf};
#[allow(dead_code)]
#[path = "../../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};

fn sample() -> Fixture {
    Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[
            ("ID", "uint"),
            ("Exact", "signed64"),
            ("Text", "text"),
            ("Float", "float"),
        ],
        &[vec![
            Cell::U(7),
            Cell::I64(9_007_199_254_740_993),
            Cell::Text("中文🙂".into()),
            Cell::F(-0.0),
        ]],
    )])
}
fn request() -> crate::request::CachePrepareRequest {
    crate::request::CachePrepareRequest { table: "Sample".into(), timeout: std::time::Duration::from_secs(300) }
}
fn manifest_path(fx: &Fixture, outcome: &Outcome) -> PathBuf {
    DataCachePaths::new(&fx.home).generations.join(format!(
        "{}.json",
        outcome.data["generation"].as_str().unwrap()
    ))
}
fn object_path(fx: &Fixture, outcome: &Outcome) -> PathBuf {
    DataCachePaths::new(&fx.home)
        .object(outcome.data["object"]["sha256"].as_str().unwrap())
        .unwrap()
}
fn importer_calls() -> usize {
    application::IMPORTED_BATCH_REQUESTS.with(|count| count.get())
}
// Deliberately create a structurally authenticated but semantically invalid
// private fixture. Preserve the exact canonical manifest representation.
fn replace_object(fx: &Fixture, outcome: &Outcome, bytes: &[u8]) {
    let sha = storage::sha256(bytes);
    let path = DataCachePaths::new(&fx.home).object(&sha).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    let manifest = manifest_path(fx, outcome);
    let old = fs::read_to_string(&manifest).unwrap();
    let from = format!(
        "\"object\":{{\"sha256\":\"{}\",\"bytes\":{}}}",
        outcome.data["object"]["sha256"].as_str().unwrap(),
        outcome.data["object"]["bytes"]
    );
    let to = format!(
        "\"object\":{{\"sha256\":\"{sha}\",\"bytes\":{}}}",
        bytes.len()
    );
    assert_eq!(old.matches(&from).count(), 1);
    fs::write(manifest, old.replacen(&from, &to, 1)).unwrap();
}

#[test]
fn second_public_prepare_reuses_exact_typed_object_without_import_or_encoding() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let before = importer_calls();
    let object = object_path(&fx, &first);
    let bytes = fs::read(&object).unwrap();
    let manifest = fs::read(manifest_path(&fx, &first)).unwrap();
    let second =
        application::dispatch_after_encode(&inv, &request(), &mut || panic!("hit encoded a table")).unwrap();
    assert_eq!(importer_calls(), before);
    assert_eq!(second.data["generationReused"], true);
    assert_eq!(second.data["publicationThisCommand"], false);
    for key in first.data.as_object().unwrap().keys() {
        assert!(
            second.data.get(key).is_some(),
            "hit omitted public field {key}"
        );
    }
    for key in first.data["diskReservation"].as_object().unwrap().keys() {
        assert!(
            second.data["diskReservation"].get(key).is_some(),
            "hit omitted disk field {key}"
        );
    }
    assert_eq!(second.data["diskReservation"]["readOnly"], true);
    assert_eq!(
        second.data["diskReservation"]["sharedBlobAdditionalPeak"],
        0
    );
    assert!(
        second.data["diskReservation"]["additionalPeak"]
            .as_object()
            .unwrap()
            .values()
            .all(|bytes| bytes == 0)
    );
    assert_eq!(second.data["generation"], first.data["generation"]);
    assert_eq!(second.data["object"], first.data["object"]);
    assert_eq!(fs::read(&object).unwrap(), bytes);
    assert_eq!(fs::read(manifest_path(&fx, &first)).unwrap(), manifest);
    let mut reader = FileReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let batch = reader.next().unwrap().unwrap();
    assert_eq!(
        batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .value(0),
        7
    );
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        9_007_199_254_740_993
    );
    assert_eq!(
        batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "中文🙂"
    );
    assert_eq!(
        batch
            .column(3)
            .as_any()
            .downcast_ref::<Float32Array>()
            .unwrap()
            .value(0)
            .to_bits(),
        (-0.0f32).to_bits()
    );
    assert_eq!(
        batch
            .column(4)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap()
            .value(0),
        7
    );
    assert!(reader.next().is_none());
    let paths = DataCachePaths::new(&fx.home);
    assert_eq!(fs::read_dir(paths.generations).unwrap().count(), 1);
    assert_eq!(fs::read_dir(object.parent().unwrap()).unwrap().count(), 1);
    assert_eq!(fs::read_dir(paths.staging).unwrap().count(), 0);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn repeated_partial_generation_keeps_exact_missing_key_coverage() {
    let spec = fixture::encrypted_sections(201);
    let (_, encoded) = fixture::encrypted_frame(&spec.raw);
    let fx = Fixture::with_encoded_table(spec, encoded);
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let before = importer_calls();
    let second = crate::prepare_cache(&inv, &request()).unwrap();
    assert_eq!(importer_calls(), before);
    assert!(!first.complete && !second.complete);
    assert_eq!(second.data["generationReused"], true);
    assert_eq!(second.data["missing"], first.data["missing"]);
    assert_eq!(
        second.data["unavailablePartitions"],
        first.data["unavailablePartitions"]
    );
    assert_eq!(second.data["availableRows"], 1);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn existing_hit_remains_readable_after_quota_is_lowered_without_write_permission() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let original = fs::read(manifest_path(&fx, &first)).unwrap();
    fs::write(fx.home.join("config.json"), br#"{"schema":"lycheedev.config.v1","cache":{"maxBytes":21474836480},"dataCache":{"maxBytes":1}}"#).unwrap();
    let second = crate::prepare_cache(&inv, &request()).unwrap();
    assert_eq!(second.data["generationReused"], true);
    assert_eq!(second.data["diskRead"]["overBudget"], true);
    assert_eq!(fs::read(manifest_path(&fx, &first)).unwrap(), original);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn invalid_manifest_coverage_is_not_a_silent_miss_or_rewrite() {
    let spec = fixture::encrypted_sections(201);
    let (_, encoded) = fixture::encrypted_frame(&spec.raw);
    let fx = Fixture::with_encoded_table(spec, encoded);
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let path = manifest_path(&fx, &first);
    let bytes = fs::read_to_string(&path).unwrap();
    assert!(bytes.contains("\"complete\":false"));
    let bad = bytes.replacen("\"complete\":false", "\"complete\":true", 1);
    fs::write(&path, &bad).unwrap();
    let before = importer_calls();
    assert_eq!(
        crate::prepare_cache(&inv, &request()).unwrap_err().code,
        "records.generation_invalid"
    );
    assert_eq!(importer_calls(), before);
    assert_eq!(fs::read_to_string(path).unwrap(), bad);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn corrupt_object_hash_is_hard_error_and_preserves_receipt() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let path = object_path(&fx, &first);
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] ^= 1;
    fs::write(&path, &bytes).unwrap();
    let before = importer_calls();
    assert!(crate::prepare_cache(&inv, &request()).is_err());
    assert_eq!(importer_calls(), before);
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert!(manifest_path(&fx, &first).exists());
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn authenticated_invalid_footer_length_is_rejected_before_arrow_allocation() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let mut bytes = fs::read(object_path(&fx, &first)).unwrap();
    let at = bytes.len() - 10;
    bytes[at..at + 4].copy_from_slice(&i32::MAX.to_le_bytes());
    replace_object(&fx, &first, &bytes);
    let before = importer_calls();
    assert_eq!(
        crate::prepare_cache(&inv, &request()).unwrap_err().code,
        "records.generation_invalid"
    );
    assert_eq!(importer_calls(), before);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn authenticated_wrong_private_record_id_is_rejected_without_reimport() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let old = fs::read(object_path(&fx, &first)).unwrap();
    let mut reader = FileReader::try_new(std::io::Cursor::new(old), None).unwrap();
    let batch = reader.next().unwrap().unwrap();
    let mut columns = batch.columns().to_vec();
    columns[4] = std::sync::Arc::new(UInt32Array::from(vec![8]));
    let bad = arrow_array::RecordBatch::try_new(batch.schema(), columns).unwrap();
    let mut bytes = Vec::new();
    let mut writer =
        arrow_ipc::writer::FileWriter::try_new(&mut bytes, bad.schema().as_ref()).unwrap();
    writer.write(&bad).unwrap();
    writer.finish().unwrap();
    drop(writer);
    replace_object(&fx, &first, &bytes);
    let before = importer_calls();
    assert_eq!(
        crate::prepare_cache(&inv, &request()).unwrap_err().code,
        "records.generation_invalid"
    );
    assert_eq!(importer_calls(), before);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn cancellation_after_authenticated_hit_keeps_existing_object_and_generation() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let manifest = fs::read(manifest_path(&fx, &first)).unwrap();
    let object = fs::read(object_path(&fx, &first)).unwrap();
    let cancel = inv.control.tracked_cancel_handle().unwrap();
    let mut reached = false;
    let error = application::dispatch_with_read_hook(&inv, &request(),
        &mut || panic!("hit encoded"),
        &mut |result| result,
        &mut || {
            reached = true;
            cancel.cancel();
            Ok(())
        },
    )
    .unwrap_err();
    assert!(reached);
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.exit_code, 7);
    assert_eq!(fs::read(manifest_path(&fx, &first)).unwrap(), manifest);
    assert_eq!(fs::read(object_path(&fx, &first)).unwrap(), object);
    assert_eq!(
        fs::read_dir(DataCachePaths::new(&fx.home).staging)
            .unwrap()
            .count(),
        0
    );
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn hit_still_reauthenticates_original_source_before_returning_success() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let version = fx.client.join("version.txt");
    let original = fs::read(&version).unwrap();
    let mut reached = false;
    let error = application::dispatch_with_read_hook(&inv, &request(),
        &mut || panic!("hit encoded"),
        &mut |result| result,
        &mut || {
            reached = true;
            fs::write(&version, b"12.1.0.69934")?;
            Ok(())
        },
    )
    .unwrap_err();
    fs::write(&version, original).unwrap();
    assert!(reached);
    assert_eq!(error.code, "records.pinned_build_changed");
    assert!(manifest_path(&fx, &first).exists());
    assert!(crate::prepare_cache(&inv, &request()).unwrap().complete);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn checked_close_failure_retains_existing_reuse_fact_without_new_publication() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let error =
        application::dispatch_after_hooks(&inv, &request(), &mut || panic!("hit encoded"), &mut |result| {
            assert_eq!(result.as_ref().unwrap().data["generationReused"], true);
            Err(crate::fault(
                "vault.close_failed",
                5,
                "fixture terminal failure",
            ))
        })
        .unwrap_err();
    assert_eq!(error.code, "vault.close_failed");
    assert_eq!(
        error.details["publishedGeneration"]["generationReused"],
        true
    );
    assert_eq!(
        error.details["publishedGeneration"]["publicationThisCommand"],
        false
    );
    assert_eq!(
        error.details["publishedGeneration"]["generation"],
        first.data["generation"]
    );
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn cached_fixed_lists_and_u64_max_span_multiple_real_ipc_batches() {
    let rows: Vec<_> = (1..=1025)
        .rev()
        .map(|id| {
            vec![
                Cell::U(id),
                Cell::I64(-1),
                Cell::Array(vec![Cell::U(id), Cell::U(0), Cell::U(u32::MAX)]),
                Cell::Array(vec![Cell::F(0.1), Cell::F(-0.0)]),
            ]
        })
        .collect();
    let mut spec = fixture::table(
        "Sample",
        201,
        &[
            ("ID", "uint"),
            ("Exact", "signed64"),
            ("List", "array3"),
            ("Float", "array2float"),
        ],
        &rows,
    );
    assert!(spec.dbd.contains("Exact<64>"));
    spec.dbd = spec.dbd.replace("Exact<64>", "Exact<u64>");
    let fx = Fixture::new(&[spec]);
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    assert_eq!(first.data["batchCount"], 2);
    let before = importer_calls();
    let second = crate::prepare_cache(&inv, &request()).unwrap();
    assert_eq!(importer_calls(), before);
    assert_eq!(second.data["generationReused"], true);
    let reader = FileReader::try_new(
        std::io::Cursor::new(fs::read(object_path(&fx, &first)).unwrap()),
        None,
    )
    .unwrap();
    let mut expected_id = 1;
    for batch in reader {
        let batch = batch.unwrap();
        let exact = batch
            .column(1)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let lists = batch
            .column(2)
            .as_any()
            .downcast_ref::<arrow_array::FixedSizeListArray>()
            .unwrap();
        let floats = batch
            .column(3)
            .as_any()
            .downcast_ref::<arrow_array::FixedSizeListArray>()
            .unwrap();
        let ids = batch
            .column(4)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();
        for row in 0..batch.num_rows() {
            assert_eq!(ids.value(row), expected_id);
            assert_eq!(exact.value(row), u64::MAX);
            let list = lists.value(row);
            let list = list.as_any().downcast_ref::<UInt64Array>().unwrap();
            assert_eq!(
                list.values().as_ref(),
                &[expected_id as u64, 0, u32::MAX as u64]
            );
            let values = floats.value(row);
            let values = values.as_any().downcast_ref::<Float32Array>().unwrap();
            assert_eq!(values.value(0).to_bits(), 0.1f32.to_bits());
            assert_eq!(values.value(1).to_bits(), (-0.0f32).to_bits());
            expected_id += 1;
        }
    }
    assert_eq!(expected_id, 1026);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn matching_object_with_nonfinite_float_is_not_adopted_as_valid_generation() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let old = fs::read(object_path(&fx, &first)).unwrap();
    let mut reader = FileReader::try_new(std::io::Cursor::new(old), None).unwrap();
    let batch = reader.next().unwrap().unwrap();
    let mut columns = batch.columns().to_vec();
    columns[3] = std::sync::Arc::new(Float32Array::from(vec![f32::NAN]));
    let bad = arrow_array::RecordBatch::try_new(batch.schema(), columns).unwrap();
    let mut bytes = Vec::new();
    let mut writer =
        arrow_ipc::writer::FileWriter::try_new(&mut bytes, bad.schema().as_ref()).unwrap();
    writer.write(&bad).unwrap();
    writer.finish().unwrap();
    drop(writer);
    replace_object(&fx, &first, &bytes);
    assert_eq!(
        crate::prepare_cache(&inv, &request()).unwrap_err().code,
        "records.generation_invalid"
    );
    fs::remove_dir_all(fx.root).unwrap();
}

// Rebuild malformed messages/footers with the official builders. Their object
// SHA and receipt are updated, so these cases reach IPC semantic validation.
fn malformed_ipc(original: &[u8], case: usize) -> Vec<u8> {
    use arrow_ipc::{
        Block, FooterBuilder, MetadataVersion,
        convert::IpcSchemaEncoder,
        writer::{DictionaryTracker, EncodedData, IpcWriteOptions, write_message},
    };
    use flatbuffers::FlatBufferBuilder;
    let reader = FileReader::try_new(std::io::Cursor::new(original), None).unwrap();
    let schema = reader.schema();
    let footer_len =
        arrow_ipc::reader::read_footer_length(original[original.len() - 10..].try_into().unwrap())
            .unwrap();
    let footer_start = original.len() - 10 - footer_len;
    let footer = arrow_ipc::root_as_footer(&original[footer_start..original.len() - 10]).unwrap();
    let original_blocks = footer.recordBatches().unwrap();
    assert_eq!(original_blocks.len(), 1);
    let original_block = original_blocks.get(0);
    let offset = original_block.offset() as usize;
    let meta_len = original_block.metaDataLength() as usize;
    let body_len = original_block.bodyLength() as usize;
    let message = arrow_ipc::root_as_message(&original[offset + 8..offset + meta_len]).unwrap();
    let batch = message.header_as_record_batch().unwrap();
    let nodes: Vec<_> = batch.nodes().unwrap().iter().copied().collect();
    let mut buffers: Vec<_> = batch.buffers().unwrap().iter().copied().collect();
    let mut body = original[offset + meta_len..offset + meta_len + body_len].to_vec();
    match case {
        3 => buffers[1].set_offset(-1),
        4 => buffers[1].set_offset(body_len as i64 + 64),
        6 => buffers.push(arrow_ipc::Buffer::new(0, 0)),
        7 => {
            let start = buffers[5].offset() as usize; // UTF8 offsets, one row.
            let impossible = i32::try_from(buffers[6].length() + 1).unwrap();
            body[start + 4..start + 8].copy_from_slice(&impossible.to_le_bytes());
        }
        8 => body[buffers[6].offset() as usize] = 255, // Invalid UTF8 byte.
        _ => {}
    }
    let mut mb = FlatBufferBuilder::new();
    let nodes = mb.create_vector(&nodes);
    let buffers = mb.create_vector(&buffers);
    let compression = if case == 5 {
        Some(arrow_ipc::BodyCompression::create(
            &mut mb,
            &Default::default(),
        ))
    } else {
        None
    };
    let batch = arrow_ipc::RecordBatch::create(
        &mut mb,
        &arrow_ipc::RecordBatchArgs {
            length: 1,
            nodes: Some(nodes),
            buffers: Some(buffers),
            compression,
            variadicBufferCounts: None,
        },
    );
    let message = arrow_ipc::Message::create(
        &mut mb,
        &arrow_ipc::MessageArgs {
            version: MetadataVersion::V5,
            header_type: arrow_ipc::MessageHeader::RecordBatch,
            header: Some(batch.as_union_value()),
            bodyLength: body_len as i64,
            custom_metadata: None,
        },
    );
    mb.finish(message, None);
    let mut bytes = original[..offset].to_vec();
    let (new_meta, new_body) = write_message(
        &mut bytes,
        EncodedData {
            ipc_message: mb.finished_data().to_vec(),
            arrow_data: body,
        },
        &IpcWriteOptions::default(),
    )
    .unwrap();
    let mut block = Block::new(offset as i64, new_meta as i32, new_body as i64);
    if case == 1 {
        block.set_offset(-1);
    }
    if case == 2 {
        block.set_offset(i64::MAX);
    }
    bytes.extend_from_slice(&[255, 255, 255, 255, 0, 0, 0, 0]);
    let mut fbb = FlatBufferBuilder::new();
    let dictionary_blocks = if case == 9 { vec![block] } else { Vec::new() };
    let dictionaries = fbb.create_vector(&dictionary_blocks);
    let blocks = fbb.create_vector(&[block]);
    let altered_schema;
    let schema = if case == 0 {
        let mut fields = schema.fields().to_vec();
        fields[1] = std::sync::Arc::new(arrow_schema::Field::new(
            "Exact",
            arrow_schema::DataType::UInt64,
            true,
        ));
        altered_schema = arrow_schema::Schema::new(fields);
        &altered_schema
    } else {
        schema.as_ref()
    };
    let encoded_schema = IpcSchemaEncoder::new()
        .with_dictionary_tracker(&mut DictionaryTracker::new(true))
        .schema_to_fb_offset(&mut fbb, schema);
    let mut fb = FooterBuilder::new(&mut fbb);
    fb.add_version(MetadataVersion::V5);
    fb.add_schema(encoded_schema);
    fb.add_dictionaries(dictionaries);
    fb.add_recordBatches(blocks);
    let footer = fb.finish();
    fbb.finish(footer, None);
    bytes.extend_from_slice(fbb.finished_data());
    bytes.extend_from_slice(&(fbb.finished_data().len() as i32).to_le_bytes());
    bytes.extend_from_slice(b"ARROW1");
    bytes
}

#[test]
fn authenticated_malformed_ipc_cases_are_typed_errors_without_panic_or_fallback() {
    let fx = sample();
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    let original = fs::read(object_path(&fx, &first)).unwrap();
    let path = manifest_path(&fx, &first);
    let original_manifest = fs::read(&path).unwrap();
    let before = importer_calls();
    for case in 0..=9 {
        fs::write(&path, &original_manifest).unwrap();
        let bad = malformed_ipc(&original, case);
        replace_object(&fx, &first, &bad);
        let bad_manifest = fs::read(&path).unwrap();
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::prepare_cache(&inv, &request())))
                .unwrap_or_else(|_| panic!("IPC case {case} panicked"));
        assert_eq!(
            result.unwrap_err().code,
            "records.generation_invalid",
            "IPC case {case}"
        );
        assert_eq!(
            importer_calls(),
            before,
            "IPC case {case} re-imported source"
        );
        assert_eq!(fs::read(&path).unwrap(), bad_manifest);
        assert_eq!(fs::read(object_path(&fx, &first)).unwrap(), original);
    }
    fs::write(&path, original_manifest).unwrap();
    assert_eq!(
        crate::prepare_cache(&inv, &request()).unwrap().data["generationReused"],
        true
    );
    assert_eq!(importer_calls(), before);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn empty_static_generation_reuses_zero_batch_ipc_without_claiming_rows() {
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "uint")],
        &[],
    )]);
    let inv = fx.context();
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    assert_eq!(first.data["availableRows"], 0);
    assert_eq!(first.data["batchCount"], 0);
    let before = importer_calls();
    let second = crate::prepare_cache(&inv, &request()).unwrap();
    assert!(second.complete);
    assert_eq!(second.data["generationReused"], true);
    assert_eq!(importer_calls(), before);
    let mut reader = FileReader::try_new(
        std::io::Cursor::new(fs::read(object_path(&fx, &first)).unwrap()),
        None,
    )
    .unwrap();
    assert!(reader.next().is_none());
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn bulk_pass_work_is_pre_admitted_in_bounded_chunks_without_resetting_budget() {
    use std::time::{Duration, Instant};
    let budget = Budget::new(
        Cost {
            decode_work: 3,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(5),
        None,
    );
    admit_bulk_work(&budget, 0).unwrap();
    assert_eq!(budget.snapshot().unwrap().decode_work, 0);
    admit_bulk_work(&budget, 65537).unwrap();
    assert_eq!(budget.snapshot().unwrap().decode_work, 2);
    assert_eq!(
        admit_bulk_work(&budget, 65537).unwrap_err().code,
        "records.query_resource_budget"
    );
    assert_eq!(budget.snapshot().unwrap().decode_work, 2);
    admit_bulk_work(&budget, 65536).unwrap();
    assert_eq!(budget.snapshot().unwrap().decode_work, 3);
    assert_eq!(
        admit_bulk_work(&budget, 1).unwrap_err().code,
        "records.query_resource_budget"
    );
    assert_eq!(budget.snapshot().unwrap().decode_work, 3);
}

// Reuse the authenticated fixture's real WDC3 layout, expanding only its one
// plain unsigned array. Construct raw rows directly so this capacity oracle
// does not first allocate eight million generic fixture Cells.
fn wide_unsigned_fixture(rows: u32, width: usize) -> Fixture {
    let mut spec = fixture::table("Sample", 201, &[("ID", "uint"), ("List", "array3")], &[]);
    assert_eq!(spec.raw.len(), 112 + 2 * 28 + 1);
    assert!(spec.dbd.contains("List<u32>[3]"));
    spec.dbd = spec
        .dbd
        .replace("List<u32>[3]", &format!("List<u32>[{width}]"));
    let metadata = 112 + 2 * 28;
    let stride = 4 + width * 4;
    spec.raw.truncate(metadata);
    spec.raw.reserve(rows as usize * stride + 1);
    for (at, value) in [
        (4, rows),
        (12, u32::try_from(stride).unwrap()),
        (28, 1),
        (32, rows),
        (84, rows),
    ] {
        spec.raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let array_width_at = 112 + 2 * 4 + 24 + 2;
    spec.raw[array_width_at..array_width_at + 2]
        .copy_from_slice(&u16::try_from(width * 32).unwrap().to_le_bytes());
    for id in 1..=rows {
        spec.raw.extend_from_slice(&id.to_le_bytes());
        for element in 0..width {
            spec.raw
                .extend_from_slice(&(id + element as u32).to_le_bytes());
        }
    }
    spec.raw.push(0); // Existing one-byte empty string block.
    assert_eq!(spec.raw.len(), metadata + rows as usize * stride + 1);
    Fixture::new(&[spec])
}

#[test]
fn cold_import_and_large_typed_rehit_fit_the_same_original_work_budget() {
    const ROWS: u32 = 20_000;
    const WIDTH: usize = 400;
    let fx = wide_unsigned_fixture(ROWS, WIDTH);
    let inv = fx.context();
    let before_cold = importer_calls();
    // First invocation starts with no decoded/derived table cache. Do not warm
    // the source or relax the public query's original 100M work allowance.
    let first = crate::prepare_cache(&inv, &request()).unwrap();
    assert!(importer_calls() > before_cold);
    assert_eq!(first.data["availableRows"], ROWS);
    assert_eq!(first.data["complete"], true);
    let original_work_limit = Cost::defaults().decode_work;
    assert_eq!(original_work_limit, 100_000_000);
    let object_bytes = first.data["object"]["bytes"].as_u64().unwrap();
    assert!(object_bytes.checked_mul(2).unwrap() > original_work_limit);
    let cold_work = first.data["resources"]["decodeWork"].as_u64().unwrap();
    assert!(cold_work >= u64::from(ROWS) * (WIDTH as u64 * 8 + 12));
    // Admission must still include every decoded WDC element and every Arrow
    // output byte. Fixing client metadata's unused-capacity scans must not
    // exempt either real row pass or increase the original command allowance.
    let wdc_row_work = u64::from(ROWS) * (WIDTH as u64 + 2);
    let arrow_row_work = u64::from(ROWS) * (WIDTH as u64 * 8 + 12);
    assert_eq!(wdc_row_work + arrow_row_work, 72_280_000);
    assert!(cold_work >= wdc_row_work + arrow_row_work);
    assert!(cold_work <= original_work_limit);
    let object = object_path(&fx, &first);
    let manifest = manifest_path(&fx, &first);
    let original_object = fs::read(&object).unwrap();
    let original_manifest = fs::read(&manifest).unwrap();
    let before_hit = importer_calls();
    let second =
        application::dispatch_after_encode(&inv, &request(), &mut || panic!("hit wrote typed IPC")).unwrap();
    assert_eq!(importer_calls(), before_hit);
    assert_eq!(second.data["generationReused"], true);
    assert_eq!(second.data["publicationThisCommand"], false);
    assert_eq!(second.data["generation"], first.data["generation"]);
    assert_eq!(second.data["object"], first.data["object"]);
    assert_eq!(second.data["availableRows"], ROWS);
    assert_eq!(second.data["complete"], true);
    assert!(second.data["resources"]["decodeWork"].as_u64().unwrap() <= original_work_limit);
    assert_eq!(fs::read(&object).unwrap(), original_object);
    assert_eq!(fs::read(&manifest).unwrap(), original_manifest);
    drop(original_object);
    // Independent mature reader checks every logical value and locator, not
    // merely the object size or the production reader's own acceptance.
    let reader = FileReader::try_new(fs::File::open(&object).unwrap(), None).unwrap();
    let mut expected_id = 1u32;
    for batch in reader {
        let batch = batch.unwrap();
        let visible_ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let list = batch
            .column(1)
            .as_any()
            .downcast_ref::<arrow_array::FixedSizeListArray>()
            .unwrap();
        let values = list
            .values()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let ids = batch
            .column(2)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();
        assert_eq!(list.value_length(), WIDTH as i32);
        for row in 0..batch.num_rows() {
            assert_eq!(visible_ids.value(row), u64::from(expected_id));
            assert_eq!(ids.value(row), expected_id);
            for element in 0..WIDTH {
                assert_eq!(
                    values.value(row * WIDTH + element),
                    u64::from(expected_id) + element as u64
                );
            }
            expected_id += 1;
        }
    }
    assert_eq!(expected_id, ROWS + 1);
    fs::remove_dir_all(fx.root).unwrap();
}
