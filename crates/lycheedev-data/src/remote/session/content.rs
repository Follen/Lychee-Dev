//! Complete materialized content for scoped metadata/DB2 consumers. This lower
//! boundary does not yet implement persistent decoded-alias reuse or large
//! VerifiedBlob output. It never labels partial encrypted bytes complete.
use super::*;
use crate::{
    blte::StreamingReader, decoded_cache::Stats, metadata_format::EncodingRecord,
    resource::RetainedReservation,
};
use std::io::Read;

pub(super) struct Materialized<'a> {
    pub content: blte::Content,
    pub blob: evidence::BlobRef,
    pub encoding_key: String,
    _allocation: RetainedReservation<'a>,
}
fn retryable(error: &ToolkitError) -> bool {
    // A simultaneous resource/stop/cleanup cause must remain terminal even if
    // an underlying availability code happens to remain the primary code.
    availability_failure(error) && error.details.is_null()
}
pub(super) fn materialize<'a>(
    files: &Files<'_>,
    index: &Encoding<'_>,
    record: &EncodingRecord,
    maximum: usize,
    metadata: bool,
    limits: casc::Limits,
    keys: &mut keys::Context,
    stats: &mut Stats,
    budget: &'a Budget,
) -> Result<Materialized<'a>> {
    let decoded = usize::try_from(record.decoded_bytes).map_err(|_| limit())?;
    if decoded > maximum {
        return Err(limit());
    }
    files.store.check()?;
    if metadata {
        budget.charge(Cost {
            metadata_bytes: decoded as u64,
            ..Cost::default()
        })?;
    }
    let expected = casc::key(&record.content_key)?;
    let mut unavailable = None;
    for key in &record.encoding_keys {
        files.store.check()?;
        let physical =
            index.find_encoding_with_check(key, limits, budget.token(), || files.store.check())?;
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
            let mut decode =
                |keys: &keys::Context| -> Result<(blte::Content, RetainedReservation<'a>)> {
                    files.store.check()?;
                    source.rewind()?;
                    let allocation = budget.reserve_retained(decoded)?;
                    let scratch = budget.reserve_retained(65536)?;
                    let mut raw = Vec::with_capacity(decoded);
                    // The existing safe decoder's allocation/work estimates are
                    // safety-only here; the Go logical attempt work is above.
                    let safety = budget.safety_only();
                    let mut reader = StreamingReader::new(
                        &mut source,
                        encoded,
                        decoded,
                        expected,
                        keys.keys(),
                        &safety,
                    )?;
                    let mut buffer = [0; 65536];
                    loop {
                        files.store.check()?;
                        let n = reader.read(&mut buffer).map_err(blte::stream_io_error)?;
                        files.store.check()?;
                        if n == 0 {
                            break;
                        }
                        if n > decoded.saturating_sub(raw.len()) {
                            return Err(limit());
                        }
                        raw.extend_from_slice(&buffer[..n]);
                    }
                    // StreamingReader observes framing/chunk digests, exact EOF,
                    // output length and the complete supplied CKey before zero.
                    drop(reader);
                    drop(scratch);
                    if raw.len() != decoded {
                        return Err(invalid());
                    }
                    files.store.check()?;
                    Ok((
                        blte::Content {
                            bytes: raw,
                            missing: Vec::new(),
                        },
                        allocation,
                    ))
                };
            stats.content_decode_calls += 1;
            let (content, allocation) = match decode(keys) {
                Ok(value) => value,
                Err(error)
                    if error.code == "container.key_unavailable"
                        && error.details["missingKey"] == true
                        && keys.source.kind == "public"
                        && keys.source.state != "loaded" =>
                {
                    keys.ensure_public()?;
                    stats.content_decode_calls += 1;
                    decode(keys)?
                }
                Err(error) => return Err(error),
            };
            stats.extracted_bytes += content.bytes.len() as u64;
            let blob = files.store.publish_blob(&content.bytes, maximum.max(1))?;
            files.store.check()?;
            Ok(Materialized {
                content,
                blob,
                encoding_key: key.clone(),
                _allocation: allocation,
            })
        })();
        match attempt {
            Ok(value) => return Ok(value),
            Err(error) if retryable(&error) => {
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
