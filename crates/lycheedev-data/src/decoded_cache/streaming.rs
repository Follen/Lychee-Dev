//! Selected local assets retain authenticated archive and verified-blob owners.
//! Decode once into Core's bounded staged publisher, never a decoded-sized Vec.
use super::*;
use crate::remote::cache::Store;
use lycheedev_core::{ToolkitError, control::OperationControl};
use std::io::Read;

fn pure(error: &ToolkitError, code: &str) -> bool {
    error.code == code && error.details.is_null()
}
fn unpublished(error: &ToolkitError) -> bool {
    error.details.get("publication").is_none_or(|p| {
        p["state"] == "not_published"
            && p["reused"] != true
            && p.get("prior").is_none_or(Value::is_null)
    })
}
fn completed(mut error: ToolkitError, reference: &evidence::BlobRef) -> ToolkitError {
    let mut details = match error.details {
        Value::Object(map) => map,
        Value::Null => serde_json::Map::new(),
        value => serde_json::Map::from_iter([("originalDetails".into(), value)]),
    };
    if !details.contains_key("publication") {
        details.insert(
            "publication".into(),
            serde_json::json!({"state":"published","reference":reference}),
        );
    } else {
        details.insert("completedBlob".into(), serde_json::json!(reference));
    }
    error.details = Value::Object(details);
    error
}
fn after_blob(
    store: &Store<'_>,
    result: Result<evidence::VerifiedBlob>,
    cleanup: Result<()>,
) -> Result<evidence::VerifiedBlob> {
    match result {
        Ok(blob) => match store.finish(Ok(()), cleanup) {
            Ok(()) => Ok(blob),
            Err(error) => {
                let error = completed(error, blob.reference());
                blob.close_after(Err(error))
            }
        },
        Err(error) => store.finish(Err(error), cleanup),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read_verified_content(
    home: &Path,
    root: &Path,
    index: &Encoding<'_>,
    record: &EncodingRecord,
    limits: casc::Limits,
    maximum: usize,
    keys: &mut keys::Context,
    stats: &mut Stats,
    budget: &Budget,
    control: &OperationControl,
) -> Result<(String, evidence::VerifiedBlob)> {
    let store = Store {
        home,
        control,
        budget: Some(budget),
    };
    store.check()?;
    let expected_key = casc::key(&record.content_key)?;
    let decoded = usize::try_from(record.decoded_bytes).map_err(|_| {
        fault(
            "records.metadata_limit",
            3,
            "Decoded content size exceeds platform bound",
        )
    })?;
    // Hash buffer and Core's sequential source/copy/verify buffer are bounded;
    // the decoder admits its own chunk allocation independently.
    let _io_buffers = budget.reserve_retained(4 * 65536)?;
    let mut unavailable = None;
    for encoding_key in &record.encoding_keys {
        let physical =
            index.find_encoding_with_check(encoding_key, limits, None, || store.check())?;
        let encoded = usize::try_from(physical.encoded_bytes).map_err(|_| {
            fault(
                "records.metadata_limit",
                3,
                "Encoded content size exceeds platform bound",
            )
        })?;
        if encoded > maximum {
            return Err(fault(
                "records.metadata_limit",
                3,
                "Selected encoded content exceeds byte budget",
            ));
        }
        let mut object =
            match casc::open_selected(root, encoding_key, encoded, limits, budget, control) {
                Ok(object) => object,
                Err(error) if pure(&error, "records.local_object_unavailable") => {
                    unavailable = Some(error);
                    continue;
                }
                Err(error) => return Err(error),
            };
        let result = (|| {
            let mut identity = Sha256::new();
            identity.update(
                format!(
                    "decoded-v1\0{}\0{}\0{}\0",
                    record.content_key, keys.source.kind, keys.source.sha256
                )
                .as_bytes(),
            );
            stats.content_hash_calls += 1;
            let mut buffer = [0; 65536];
            loop {
                let n = object.read(&mut buffer).map_err(blte::stream_io_error)?;
                if n == 0 {
                    break;
                }
                stats.content_hash_bytes += n as u64;
                budget.charge(Cost::work(1))?;
                identity.update(&buffer[..n]);
            }
            let cache_key = format!("decoded-v1/{}", casc::hex(&identity.finalize()));
            let mut metadata =
                Metadata::open_controlled_with_check(home, true, control, || budget.check())?;
            let result = (|| {
                let existing =
                    match metadata
                        .read_controlled_with_check::<Entry>(&cache_key, control, || budget.check())
                    {
                        Ok(entry) => Some(entry),
                        Err(error) if pure(&error, "vault.record_missing") => None,
                        Err(error) => return Err(error),
                    };
                if let Some(entry) = &existing {
                    if entry.content_key != record.content_key
                        || entry.blob["bytes"].as_u64() != Some(record.decoded_bytes)
                        || entry.keys.kind != keys.source.kind
                        || entry.keys.sha256 != keys.source.sha256
                    {
                        return Err(integrity());
                    }
                    if entry.keys.kind == "public" && entry.keys.state == "loaded" {
                        keys.ensure_public()?;
                    }
                    let mut ckey = Md5::new();
                    let owned_budget = budget.safety_only();
                    let found = evidence::open_verified_blob_bounded_optional_with_check(
                        home,
                        &entry.blob,
                        maximum as u64,
                        control,
                        |block| {
                            store.check()?;
                            budget.charge(Cost::work(2))?;
                            ckey.update(block);
                            Ok(())
                        },
                        move || owned_budget.check(),
                    )?;
                    if let Some(blob) = found {
                        if ckey.finalize().as_slice() != expected_key {
                            return blob.close_after(Err(integrity()));
                        }
                        stats.blob_reads += 1;
                        stats.blob_read_bytes += record.decoded_bytes;
                        stats.hits += 1;
                        stats.reused_bytes += record.decoded_bytes;
                        return Ok(blob);
                    }
                }
                // One logical extraction. A pre-publication missing public key
                // can restart actual decode work, without a second logical call.
                stats.content_decode_calls += 1;
                let mut publish = |keys: &keys::Context| -> Result<evidence::VerifiedBlob> {
                    object.rewind()?;
                    let check = || store.check();
                    let mut reader = blte::StreamingReader::new_with_check(
                        &mut object,
                        encoded,
                        decoded,
                        expected_key,
                        keys.keys(),
                        budget,
                        Some(&check),
                    )?;
                    let owned_budget = budget.safety_only();
                    evidence::publish_blob_stream_bounded_with_check(
                        home,
                        &mut reader,
                        record.decoded_bytes,
                        maximum as u64,
                        control,
                        move || owned_budget.check(),
                        &budget.safety_only(),
                    )
                };
                let blob = match publish(keys) {
                    Ok(blob) => blob,
                    Err(error)
                        if error.code == "container.key_unavailable"
                            && error.details["missingKey"] == true
                            && unpublished(&error)
                            && keys.source.kind == "public"
                            && keys.source.state != "loaded" =>
                    {
                        store.check()?;
                        keys.ensure_public()?;
                        publish(keys)?
                    }
                    Err(error) => return Err(error),
                };
                stats.extracted_bytes += record.decoded_bytes;
                stats.blob_reads += 1;
                stats.blob_read_bytes += record.decoded_bytes;
                let publish_alias = (|| {
                    let entry = Entry {
                        content_key: record.content_key.clone(),
                        blob: serde_json::to_value(blob.reference())?,
                        keys: keys.source.clone(),
                    };
                    match metadata.put_controlled_with_check(
                        &cache_key,
                        &entry,
                        None,
                        control,
                        || budget.check(),
                    ) {
                        Ok(_) => {}
                        Err(error) if pure(&error, "vault.generation_conflict") => {
                            let other: Entry =
                                metadata.read_controlled_with_check(&cache_key, control, || {
                                    budget.check()
                                })?;
                            if other.content_key != entry.content_key
                                || other.blob != entry.blob
                                || other.keys.kind != entry.keys.kind
                                || other.keys.sha256 != entry.keys.sha256
                            {
                                return Err(integrity());
                            }
                        }
                        Err(error) => return Err(error),
                    }
                    store.check()
                })();
                match publish_alias {
                    Ok(()) => Ok(blob),
                    Err(error) => {
                        let error = completed(error, blob.reference());
                        blob.close_after(Err(error))
                    }
                }
            })();
            after_blob(&store, result, metadata.close())
        })();
        let result = after_blob(&store, result, object.close_after(Ok(())))?;
        return Ok((encoding_key.clone(), result));
    }
    Err(unavailable.unwrap_or_else(|| {
        fault(
            "records.local_object_unavailable",
            3,
            "No physical encoding alternative is available",
        )
    }))
}

/// Actual local DB2/root consumers need a bounded decoded owner, not the old
/// additional whole encoded allocation. The verified source keeps both controls
/// through this second, disk-only read. It never restarts decode or publication.
#[allow(clippy::too_many_arguments)]
pub(crate) fn read_content_controlled(
    home: &Path,
    root: &Path,
    index: &Encoding<'_>,
    record: &EncodingRecord,
    limits: casc::Limits,
    keys: &mut keys::Context,
    stats: &mut Stats,
    budget: &Budget,
    selected_maximum: Option<usize>,
    control: &OperationControl,
) -> Result<(String, blte::Content)> {
    control.check()?;
    budget.check()?;
    let decoded = usize::try_from(record.decoded_bytes).map_err(|_| {
        fault(
            "records.metadata_limit",
            3,
            "Decoded content size exceeds platform bound",
        )
    })?;
    if decoded > 256 << 20 {
        return Err(fault(
            "records.metadata_limit",
            3,
            "Full decoded materialization byte budget exceeded",
        ));
    }
    let maximum = selected_maximum.unwrap_or(limits.encoded_bytes);
    if decoded > maximum {
        return Err(fault(
            "records.metadata_limit",
            3,
            "Selected content exceeds byte budget",
        ));
    }
    let (key, mut blob) = read_verified_content(
        home, root, index, record, limits, maximum, keys, stats, budget, control,
    )?;
    let result = (|| {
        budget.charge_split(Cost::retained(decoded), Cost::default())?;
        let allocation = budget.reserve_retained(decoded)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(decoded).map_err(|_| {
            fault(
                "records.query_resource_budget",
                3,
                "Decoded allocation failed",
            )
        })?;
        bytes.resize(decoded, 0);
        let mut position = 0;
        while position < decoded {
            control.check()?;
            budget.safety_only().charge(Cost::work(1))?;
            let end = (position + 65536).min(decoded);
            let n = blob.read_checked(&mut bytes[position..end])?;
            if n == 0 {
                return Err(integrity());
            }
            position += n;
        }
        if blob.read_checked(&mut [0; 1])? != 0 {
            return Err(integrity());
        }
        Ok((bytes, allocation))
    })();
    let reference = blob.reference().clone();
    let (bytes, allocation) = blob
        .close_after(result)
        .map_err(|error| completed(error, &reference))?;
    // Existing local FileBytes owns the Vec for the query lifetime. Its legacy
    // ledger retains this successful output until the query budget is dropped.
    allocation.commit();
    Ok((
        key,
        blte::Content {
            bytes,
            missing: vec![],
        },
    ))
}
