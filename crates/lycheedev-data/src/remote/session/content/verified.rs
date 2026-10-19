//! One held verified body for a scoped asset consumer. No decoded-sized Vec.
//! The Core staged publisher dependency is pending its independent freeze.
use super::*;
use crate::phase_profile::Phase;
use crate::remote::cache::Store;
use md5::{Digest, Md5};

pub(in crate::remote::session) struct Verified {
    pub blob: evidence::VerifiedBlob,
    pub encoding_key: String,
    pub(in crate::remote::session) descriptor: OwnedRetainedReservation,
}
impl Verified {
    fn new(blob: evidence::VerifiedBlob, key: &str, budget: &Arc<Budget>) -> Result<Self> {
        let reference = blob.reference().clone();
        let reservation = std::mem::size_of::<Self>()
            .checked_add(key.len())
            .ok_or_else(limit)
            .and_then(|bytes| budget.reserve_retained(bytes))
            .and_then(|reservation| reservation.into_owned(budget));
        match reservation {
            Ok(reservation) => Ok(Self {
                blob,
                encoding_key: key.to_owned(),
                descriptor: reservation,
            }),
            Err(error) => blob.close_after(Err(completed_body(error, &reference))),
        }
    }
}

fn integrity() -> ToolkitError {
    fault("vault.blob_integrity", 4, "Decoded cache identity differs")
}

/// An actual cached body is SHA256- and CKey-checked through the same held
/// immutable handle. Native pre-open absence alone is a cache miss.
fn cached(
    store: &Store<'_>,
    key: Option<&str>,
    record: &EncodingRecord,
    keys: &mut keys::Context,
    stats: &mut Stats,
    budget: &Arc<Budget>,
) -> Result<alias::Lookup<evidence::VerifiedBlob>> {
    let reference = {
        let _span = budget.phase(Phase::DecodedCacheLookup);
        alias::reference(store, key, record, keys)?
    };
    let Some(reference) = reference else {
        return Ok(alias::Lookup::Unmapped);
    };
    let _span = budget.phase(Phase::DecodedCacheVerify);
    let size = usize::try_from(reference.bytes).map_err(|_| limit())?;
    // Go readVerifiedBlob admission survives a later missing/corrupt body. It
    // is logical usage, independent of this mode's bounded actual allocation.
    budget.charge_split(Cost::retained(size), Cost::default())?;
    let _scratch = budget.reserve_retained(65536)?;
    let safety = budget.safety_only();
    let mut ckey = Md5::new();
    let owned_budget = budget.clone();
    let found = evidence::open_verified_blob_bounded_optional_with_check(
        store.home,
        &serde_json::to_value(&reference)?,
        reference.bytes.max(1),
        store.control,
        |block| {
            store.check()?;
            // Core SHA256 and the CKey observer both perform actual work.
            safety.charge(Cost::work(2))?;
            ckey.update(block);
            Ok(())
        },
        move || owned_budget.check(),
    )?;
    let Some(blob) = found else {
        return Ok(alias::Lookup::Missing(reference));
    };
    stats.blob_reads += 1;
    stats.blob_read_bytes += reference.bytes;
    let result = store.check().and_then(|_| {
        if casc::hex(&ckey.finalize()) != record.content_key {
            Err(integrity())
        } else {
            Ok(())
        }
    });
    match result {
        Ok(()) => Ok(alias::Lookup::Hit(blob)),
        Err(error) => blob.close_after(Err(error)),
    }
}

fn before_publication(error: &ToolkitError) -> bool {
    match error.details.get("publication") {
        None => true, // No staged publisher was entered (e.g. source open).
        Some(value) => {
            value.get("state").and_then(|v| v.as_str()) == Some("not_published")
                && value.get("reused") != Some(&serde_json::Value::Bool(true))
                && value.get("prior").is_none_or(serde_json::Value::is_null)
        }
    }
}
fn retryable_availability(error: &ToolkitError) -> bool {
    if !availability_failure(error) || !before_publication(error) {
        return false;
    }
    error.details.is_null()
        || error
            .details
            .as_object()
            .is_some_and(|details| details.len() == 1 && details.contains_key("publication"))
}

/// The already returned immutable body exists even if alias save or its close
/// fails. Preserve this fact and forbid outer key recovery from repeating it.
pub(in crate::remote::session) fn completed_body(
    mut error: ToolkitError,
    reference: &evidence::BlobRef,
) -> ToolkitError {
    let mut details = match error.details {
        serde_json::Value::Object(details) => details,
        serde_json::Value::Null => serde_json::Map::new(),
        previous => serde_json::Map::from_iter([("originalDetails".into(), previous)]),
    };
    // Keep any existing publication event, especially unknown, unchanged.
    // A cold decode may have coalesced an existing winner, so do not invent
    // reused=false. The immutable body is known; its reuse history is not.
    if !details.contains_key("publication") {
        details.insert(
            "publication".into(),
            serde_json::json!({
                "state":"published", "reference":reference,
            }),
        );
    } else {
        details.insert("completedBlob".into(), serde_json::json!(reference));
    }
    error.details = serde_json::Value::Object(details);
    error
}

pub(in crate::remote::session) fn read(
    files: &Files<'_>,
    index: &dyn metadata_format::EncodingLookup,
    record: &EncodingRecord,
    maximum: usize,
    limits: casc::Limits,
    keys: &mut keys::Context,
    stats: &mut Stats,
    budget: &Arc<Budget>,
) -> Result<Verified> {
    files.store.check()?;
    let decoded = usize::try_from(record.decoded_bytes).map_err(|_| limit())?;
    if decoded > maximum {
        return Err(limit());
    }
    let expected = casc::key(&record.content_key)?;
    let mut unavailable = None;
    for key in &record.encoding_keys {
        files.store.check()?;
        let physical = index.physical(keys.keys(), key, limits, &mut || files.store.check())?;
        let encoded = usize::try_from(physical.encoded_bytes).map_err(|_| limit())?;
        if encoded > maximum {
            return Err(limit());
        }
        budget.charge(Cost::work(
            encoded
                .checked_add(decoded)
                .ok_or_else(limit)?
                .div_ceil(65536),
        ))?;
        let attempt = (|| {
            let mut source =
                files.open(key, physical.encoded_bytes.try_into().map_err(|_| limit())?)?;
            let identity = {
                let _span = budget.phase(Phase::EncodedIdentityHash);
                alias::key(files, &mut source, &record.content_key, keys, stats, budget)?
            };
            let missing = match cached(
                &files.store,
                identity.as_deref(),
                record,
                keys,
                stats,
                budget,
            )? {
                alias::Lookup::Hit(blob) => {
                    stats.hits += 1;
                    stats.reused_bytes += blob.reference().bytes;
                    return Verified::new(blob, key, budget);
                }
                alias::Lookup::Missing(reference) => Some(reference),
                alias::Lookup::Unmapped => None,
            };
            // Only the live codec chunks and fixed Core I/O buffer are actual
            // retained allocations. The separate asset consumer owns Go's
            // logical retained charge before capture; do not charge it twice.
            let _io_buffer = budget.reserve_retained(65536)?;
            let safety = budget.safety_only();
            let mut decode = |keys: &keys::Context| {
                files.store.check()?;
                source.rewind()?;
                let check = || files.store.check();
                let mut reader = StreamingReader::new_incremental_with_check(
                    &mut source,
                    encoded,
                    decoded,
                    expected,
                    keys.keys(),
                    &safety,
                    Some(&check),
                )?;
                let owned_budget = budget.clone();
                let _span = budget.phase(Phase::DecodedStreamPublish);
                if let Some(expected) = missing.as_ref() {
                    let mut source = CheckedEof {
                        reader: &mut reader,
                        check: || {
                            alias::validate_rebuild(
                                &files.store,
                                identity.as_deref(),
                                record,
                                keys,
                                expected,
                            )
                        },
                    };
                    lycheedev_core::workspace::data_disk::rebuild_shared_blob_stream_with_check(
                        files.store.home,
                        &mut source,
                        expected,
                        maximum.max(1) as u64,
                        files.store.control,
                        move || owned_budget.check(),
                        &safety,
                    )
                } else {
                    lycheedev_core::workspace::data_disk::publish_shared_blob_stream_with_check(
                        files.store.home,
                        &mut reader,
                        record.decoded_bytes,
                        maximum.max(1) as u64,
                        files.store.control,
                        move || owned_budget.check(),
                        &safety,
                    )
                }
            };
            stats.content_decode_calls += 1;
            let blob = match decode(keys) {
                Ok(blob) => blob,
                Err(error)
                    if super::super::recoverable_missing_key(&error)
                        && keys.source.kind == "public"
                        && keys.source.state != "loaded" =>
                {
                    keys.ensure_public()?;
                    // The fixed Go key lookup completes within this one logical
                    // ExtractContent call. A Rust failed-prefix restart still
                    // charges every real codec/Core pass to the safety ledger.
                    decode(keys)?
                }
                Err(error) => return Err(error),
            };
            stats.extracted_bytes += record.decoded_bytes;
            let reference = blob.reference().clone();
            let result = (|| {
                let _span = budget.phase(Phase::DecodedCacheCommit);
                if alias::save_entry(&files.store, identity.as_deref(), record, &reference, keys)? {
                    if let alias::Lookup::Hit(winner) = cached(
                        &files.store,
                        identity.as_deref(),
                        record,
                        keys,
                        stats,
                        budget,
                    )? {
                        winner.close()?;
                    }
                }
                files.store.check()
            })();
            match result {
                Ok(()) => Verified::new(blob, key, budget),
                Err(error) => blob.close_after(Err(completed_body(error, &reference))),
            }
        })();
        match attempt {
            Ok(value) => return Ok(value),
            Err(error) if retryable_availability(&error) => {
                if unavailable.is_none() || error.code == "records.remote_http" {
                    unavailable = Some(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(unavailable.unwrap_or_else(|| {
        fault(
            "records.remote_object_missing",
            3,
            "No available encoding for content",
        )
    }))
}

// Only a nonempty read returning real EOF authorizes the alias recheck. The
// codec has already validated exact decoded size, CKey and encoded framing.
struct CheckedEof<R, F> {
    reader: R,
    check: F,
}
impl<R: Read, F: FnMut() -> Result<()>> Read for CheckedEof<R, F> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(buffer)?;
        if n == 0 && !buffer.is_empty() {
            (self.check)().map_err(std::io::Error::other)?;
        }
        Ok(n)
    }
}

#[cfg(test)]
mod disk_tests;
