//! Derived decoded reuse never substitutes another selected encoded source.
//! Hash all current encoded bytes, then rehash cached blob and check whole CKey.
use crate::{
    blte, casc, fault, keys,
    metadata_format::{Encoding, EncodingRecord},
    resource::{Budget, Cost},
};
use lycheedev_core::{Result, evidence, metadata::DataCacheMetadata, storage};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::{path::Path, time::Instant};
mod streaming;
pub(crate) use streaming::{
    read_content_controlled, read_content_in_session, read_content_with_reference_controlled,
    read_content_with_reference_in_session, read_verified_content,
    read_verified_content_in_session,
};
#[derive(Default, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub hits: u64,
    pub reused_bytes: u64,
    pub content_hash_calls: u64,
    pub content_hash_bytes: u64,
    pub content_decode_calls: u64,
    pub extracted_bytes: u64,
    pub blob_reads: u64,
    pub blob_read_bytes: u64,
    pub fragment_loads: u64,
    pub block_cache_hits: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    content_key: String,
    blob: Value,
    keys: keys::KeySource,
}
fn integrity() -> lycheedev_core::ToolkitError {
    fault("vault.blob_integrity", 4, "Decoded cache identity differs")
}
fn cache_key(
    object: &casc::LocalObject,
    content_key: &str,
    source: &keys::KeySource,
    limits: casc::Limits,
    stats: &mut Stats,
    budget: Option<&Budget>,
) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(
        format!(
            "decoded-v1\0{content_key}\0{}\0{}\0",
            source.kind, source.sha256
        )
        .as_bytes(),
    );
    stats.content_hash_calls += 1;
    for chunk in object.encoded.chunks(65536) {
        if let Some(budget) = budget {
            budget.charge(Cost::work(1))?;
        }
        if Instant::now() >= limits.deadline {
            return Err(fault(
                "records.resource_limit",
                3,
                "Decoded cache hashing deadline exceeded",
            ));
        }
        hash.update(chunk);
        stats.content_hash_bytes += chunk.len() as u64;
    }
    Ok(format!("decoded-v1/{}", casc::hex(&hash.finalize())))
}
pub fn decode(
    home: &Path,
    object: &casc::LocalObject,
    record: &EncodingRecord,
    limits: casc::Limits,
    keys: &mut keys::Context,
    stats: &mut Stats,
) -> Result<blte::Content> {
    decode_budgeted(home, object, record, limits, keys, stats, None)
}
pub fn decode_budgeted(
    home: &Path,
    object: &casc::LocalObject,
    record: &EncodingRecord,
    limits: casc::Limits,
    keys: &mut keys::Context,
    stats: &mut Stats,
    budget: Option<&Budget>,
) -> Result<blte::Content> {
    let max = usize::try_from(record.decoded_bytes).map_err(|_| {
        fault(
            "records.metadata_limit",
            3,
            "Decoded content size exceeds platform bound",
        )
    })?;
    if max > 256 << 20 {
        return Err(fault(
            "records.metadata_limit",
            3,
            "Full decoded materialization byte budget exceeded",
        ));
    }
    let key = cache_key(
        object,
        &record.content_key,
        &keys.source,
        limits,
        stats,
        budget,
    )?;
    let control = lycheedev_core::control::OperationControl::uncancelable(limits.deadline);
    let check = || budget.map_or(Ok(()), Budget::check);
    let existing =
        match DataCacheMetadata::read_controlled_with_check::<Entry>(home, &key, &control, check) {
            Ok(entry) => Some(entry),
            Err(error) if crate::remote::cache::pure_expected(&error, "vault.record_missing") => {
                None
            }
            Err(error) => return Err(error),
        };
    let mut missing_body = None;
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
        let path = evidence::blob_path(home, entry.blob["sha256"].as_str().ok_or_else(integrity)?)?;
        storage::guard_path(home, &path)?;
        if path.try_exists()? {
            if let Some(budget) = budget {
                budget.charge(Cost {
                    retained_bytes: max as u64,
                    decode_work: max.div_ceil(65536) as u64 * 2,
                    ..Default::default()
                })?;
            }
            let raw = evidence::read_blob(home, &entry.blob, max)?;
            stats.blob_reads += 1;
            stats.blob_read_bytes += raw.len() as u64;
            if Md5::digest(&raw).as_slice() != casc::key(&record.content_key)? {
                return Err(integrity());
            }
            stats.hits += 1;
            stats.reused_bytes += raw.len() as u64;
            return Ok(blte::Content {
                bytes: raw,
                missing: vec![],
            });
        }
        missing_body = Some(
            serde_json::from_value::<evidence::BlobRef>(entry.blob.clone())
                .map_err(|_| integrity())?,
        );
    }
    let decode = |keys: &keys::Context| {
        let reservation = budget
            .map(|budget| {
                budget.reserve_retained(max.checked_mul(2).ok_or_else(|| {
                    fault(
                        "records.query_resource_budget",
                        3,
                        "Decode allocation size overflow",
                    )
                })?)
            })
            .transpose()?;
        if let Some(budget) = budget {
            budget.charge(Cost {
                decode_work: (max + object.encoded.len()).div_ceil(65536) as u64,
                ..Default::default()
            })?;
        }
        let result = object.decode(
            &record.content_key,
            blte::Limits {
                decoded_bytes: max,
                ..Default::default()
            },
            keys.keys(),
        );
        // A failed decode has dropped its output and scratch allocations before
        // returning. Retain actual successful ownership, release only the failed
        // temporary reservation before admitting a public-key retry.
        if result.is_ok()
            && let Some(reservation) = reservation
        {
            reservation.commit();
        }
        result
    };
    stats.content_decode_calls += 1;
    let content = match decode(keys) {
        Ok(content) => content,
        Err(error)
            if error.code == "container.key_unavailable"
                && keys.source.kind == "public"
                && keys.source.state != "loaded" =>
        {
            keys.ensure_public()?;
            stats.content_decode_calls += 1;
            decode(keys)?
        }
        Err(error) => return Err(error),
    };
    if content.bytes.len() != max {
        return Err(integrity());
    }
    if let Some(budget) = budget {
        budget.check()?;
    }
    stats.extracted_bytes += content.bytes.len() as u64;
    let blob = if let Some(expected) = missing_body.as_ref() {
        let current: Entry =
            DataCacheMetadata::read_controlled_with_check(home, &key, &control, check)?;
        let original = existing
            .as_ref()
            .expect("authenticated missing body has alias");
        if current.content_key != original.content_key
            || current.blob != original.blob
            || current.keys.kind != original.keys.kind
            || current.keys.sha256 != original.keys.sha256
        {
            return Err(integrity());
        }
        lycheedev_core::workspace::data_disk::rebuild_shared_blob_with_check(
            home,
            &content.bytes,
            max,
            expected,
            &control,
            &check,
        )?
    } else {
        lycheedev_core::workspace::data_disk::publish_shared_blob_with_check(
            home,
            &content.bytes,
            max,
            &control,
            &check,
        )?
    };
    let blob = serde_json::to_value(blob)?;
    let entry = Entry {
        content_key: record.content_key.clone(),
        blob,
        keys: keys.source.clone(),
    };
    // A missing original blob is restored under its original hash. A different
    // persisted association is an error, never a silent cache overwrite.
    match DataCacheMetadata::put_controlled_with_check(home, &key, &entry, None, &control, check) {
        Ok(()) => {}
        Err(error) if crate::remote::cache::pure_expected(&error, "vault.generation_conflict") => {
            let other: Entry =
                DataCacheMetadata::read_controlled_with_check(home, &key, &control, check)?;
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
    Ok(content)
}
pub fn read_content(
    home: &Path,
    root: &Path,
    index: &Encoding<'_>,
    record: &EncodingRecord,
    limits: casc::Limits,
    keys: &mut keys::Context,
    stats: &mut Stats,
) -> Result<(casc::LocalObject, blte::Content)> {
    read_content_budgeted(home, root, index, record, limits, keys, stats, None, None)
}
#[allow(clippy::too_many_arguments)]
pub fn read_content_budgeted(
    home: &Path,
    root: &Path,
    index: &Encoding<'_>,
    record: &EncodingRecord,
    limits: casc::Limits,
    keys: &mut keys::Context,
    stats: &mut Stats,
    budget: Option<&Budget>,
    selected_maximum: Option<usize>,
) -> Result<(casc::LocalObject, blte::Content)> {
    let mut unavailable = None;
    for encoding_key in &record.encoding_keys {
        // Keep cancellation owned by the budget/control instead of a raw token.
        let physical = index.find_encoding_with_check(encoding_key, limits, None, || {
            budget.map_or(Ok(()), |budget| budget.check())
        })?;
        let bytes = usize::try_from(physical.encoded_bytes).map_err(|_| {
            fault(
                "records.metadata_limit",
                3,
                "Encoded size exceeds platform bounds",
            )
        })?;
        if selected_maximum.is_some_and(|maximum| bytes > maximum) {
            return Err(fault(
                "records.metadata_limit",
                3,
                "Selected encoded content exceeds byte budget",
            ));
        }
        match casc::open_local_budgeted(root, encoding_key, bytes, limits, None, budget) {
            Ok(object) => {
                let content = decode_budgeted(home, &object, record, limits, keys, stats, budget)?;
                return Ok((object, content));
            }
            Err(error) if error.code == "records.local_object_unavailable" => {
                unavailable = Some(error)
            }
            Err(error) => return Err(error),
        }
    }
    Err(unavailable.unwrap_or_else(|| {
        fault(
            "records.local_object_unavailable",
            3,
            "No physical encoding alternative is available",
        )
    }))
}
