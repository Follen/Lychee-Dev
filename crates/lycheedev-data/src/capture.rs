//! JSON evidence serialization against the producer's existing ledger.
//! Go escaping is shared by catalog bodies and ordered SQL request identities.
use crate::{
    fault,
    resource::{Budget, Cost, RetainedReservation},
};
use lycheedev_core::{Result, ToolkitError, evidence::CaptureAccounting};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{self, Write};

/// Match encoding/json.Marshal's HTML and JavaScript string escaping without
/// allocating a replaced copy of the complete JSON document.
pub(crate) struct GoCaptureFormatter;
impl serde_json::ser::Formatter for GoCaptureFormatter {
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (at, ch) in fragment.char_indices() {
            let escaped: &[u8] = match ch {
                '<' => b"\\u003c",
                '>' => b"\\u003e",
                '&' => b"\\u0026",
                '\u{2028}' => b"\\u2028",
                '\u{2029}' => b"\\u2029",
                _ => continue,
            };
            writer.write_all(&fragment.as_bytes()[start..at])?;
            writer.write_all(escaped)?;
            start = at + ch.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

impl CaptureAccounting for Budget {
    fn blob_stream_phase(&self, phase: lycheedev_core::evidence::BlobStreamPhase) -> impl Drop + '_ {
        use crate::phase_profile::Phase;
        use lycheedev_core::evidence::BlobStreamPhase;
        self.phase(match phase {
            BlobStreamPhase::SourceSpool => Phase::PublisherSourceSpool,
            BlobStreamPhase::ShardCopy => Phase::PublisherShardCopy,
            BlobStreamPhase::FinalVerify => Phase::PublisherFinalVerify,
            BlobStreamPhase::StageCreate => Phase::PublisherStageCreate,
            BlobStreamPhase::PrivateSpoolReadReady => Phase::PublisherPrivateSpoolReadReady,
            BlobStreamPhase::FinalStageSeal => Phase::PublisherFinalStageSeal,
            BlobStreamPhase::DigestLock => Phase::PublisherDigestLock,
            BlobStreamPhase::ShardOpen => Phase::PublisherShardOpen,
            BlobStreamPhase::Rename => Phase::PublisherRename,
            BlobStreamPhase::WinnerOpen => Phase::PublisherWinnerOpen,
            BlobStreamPhase::CheckedClose => Phase::PublisherCheckedClose,
        })
    }
    fn check(&self) -> Result<()> {
        Budget::check(self)
    }
    fn admit_allocation(&self, bytes: usize) -> Result<()> {
        self.charge(Cost::metadata(bytes))
    }
    fn admit_work(&self, bytes: usize) -> Result<()> {
        self.charge(Cost::work(bytes))
    }
}

struct Buffer<'a> {
    bytes: Vec<u8>,
    allocation: Option<RetainedReservation<'a>>,
    maximum: usize,
    limit: fn() -> ToolkitError,
    budget: &'a Budget,
    error: Option<ToolkitError>,
}
impl Buffer<'_> {
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.budget.check()?;
        let end = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|end| *end <= self.maximum)
            .ok_or_else(self.limit)?;
        self.budget
            .charge(Cost::work(bytes.len().saturating_add(1)))?;
        if end > self.bytes.capacity() {
            let capacity = end
                .max(self.bytes.capacity().saturating_mul(2))
                .max(4096.min(self.maximum))
                .min(self.maximum);
            // A realloc may keep the old buffer alive while allocating its replacement.
            let allocation = self.budget.reserve_retained(capacity)?;
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| {
                    fault(
                        "records.query_resource_budget",
                        3,
                        "Evidence buffer allocation failed",
                    )
                })?;
            self.allocation = Some(allocation);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}
impl Write for Buffer<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Err(error) = self.append(bytes) {
            self.error = Some(error);
            return Err(io::Error::other("Bounded evidence serialization stopped"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn json_bytes(
    value: &impl Serialize,
    budget: &Budget,
    maximum: usize,
    limit: fn() -> ToolkitError,
) -> Result<Vec<u8>> {
    json_bytes_with_formatter(
        value,
        budget,
        maximum,
        limit,
        serde_json::ser::CompactFormatter,
    )
}

pub(crate) fn json_bytes_with_formatter(
    value: &impl Serialize,
    budget: &Budget,
    maximum: usize,
    limit: fn() -> ToolkitError,
    formatter: impl serde_json::ser::Formatter,
) -> Result<Vec<u8>> {
    let mut writer = Buffer {
        bytes: Vec::new(),
        allocation: None,
        maximum,
        limit,
        budget,
        error: None,
    };
    if let Err(error) = value.serialize(&mut serde_json::Serializer::with_formatter(
        &mut writer,
        formatter,
    )) {
        return Err(writer.error.unwrap_or_else(|| error.into()));
    }
    budget.check()?;
    if let Some(allocation) = writer.allocation.take() {
        allocation.commit();
    }
    Ok(writer.bytes)
}

// Counting and hashing do not need a temporary JSON body. Row counting uses
// ordinary serde encoding; hash callers explicitly select their wire format.
struct Counter<'a> {
    bytes: usize,
    hash: Option<Sha256>,
    budget: &'a Budget,
    error: Option<ToolkitError>,
}
impl Write for Counter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let admitted = (|| {
            self.budget
                .charge(Cost::work(bytes.len().saturating_add(1)))?;
            self.bytes = self.bytes.checked_add(bytes.len()).ok_or_else(|| {
                fault(
                    "records.query_resource_budget",
                    3,
                    "Evidence byte count overflow",
                )
            })?;
            Ok::<_, ToolkitError>(())
        })();
        if let Err(error) = admitted {
            self.error = Some(error);
            return Err(io::Error::other("Evidence serialization stopped"));
        }
        if let Some(hash) = &mut self.hash {
            hash.update(bytes);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn json_size(value: &impl Serialize, budget: &Budget) -> Result<usize> {
    let mut writer = Counter {
        bytes: 0,
        hash: None,
        budget,
        error: None,
    };
    if let Err(error) = serde_json::to_writer(&mut writer, value) {
        return Err(writer.error.unwrap_or_else(|| error.into()));
    }
    budget.check()?;
    Ok(writer.bytes)
}
pub(crate) fn json_sha256(
    value: &impl Serialize,
    budget: &Budget,
    formatter: impl serde_json::ser::Formatter,
) -> Result<String> {
    let mut writer = Counter {
        bytes: 0,
        hash: Some(Sha256::new()),
        budget,
        error: None,
    };
    if let Err(error) = value.serialize(&mut serde_json::Serializer::with_formatter(
        &mut writer,
        formatter,
    )) {
        return Err(writer.error.unwrap_or_else(|| error.into()));
    }
    budget.charge(Cost::retained(64))?;
    Ok(format!(
        "{:x}",
        writer.hash.expect("hashing writer").finalize()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };
    fn budget(limits: Cost) -> Budget {
        Budget::new(limits, Instant::now() + Duration::from_secs(30), None)
    }
    fn limit() -> ToolkitError {
        fault("test.body_limit", 3, "original body limit")
    }
    #[test]
    fn ordinary_bytes_counts_and_locator_digest_keep_exact_existing_encoding() {
        let value =
            json!({"text":"<>&\u{2028}\u{2029}🙂\0", "signed":i64::MIN, "unsigned":u64::MAX});
        let expected = serde_json::to_vec(&value).unwrap();
        let ledger = budget(Cost::defaults());
        assert_eq!(
            json_bytes(&value, &ledger, expected.len(), limit).unwrap(),
            expected
        );
        let count = budget(Cost::defaults());
        assert_eq!(json_size(&value, &count).unwrap(), expected.len());
        assert_eq!(count.snapshot().unwrap().retained_bytes, 0);
        assert_eq!(
            json_sha256(&value, &count, serde_json::ser::CompactFormatter).unwrap(),
            lycheedev_core::storage::sha256(&expected)
        );
    }
    #[test]
    fn bounded_serialization_keeps_limit_budget_and_cancellation_errors_distinct() {
        let escaped = json!("\0".repeat(1000));
        let ledger = budget(Cost::defaults());
        let error = json_bytes(&escaped, &ledger, 6001, limit).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str(), error.exit_code),
            ("test.body_limit", "original body limit", 3)
        );
        assert_eq!(
            ledger.snapshot().unwrap().retained_bytes,
            0,
            "failed temporary body is released"
        );
        let tight = budget(Cost {
            retained_bytes: 4096,
            ..Cost::defaults()
        });
        assert_eq!(
            json_bytes(&escaped, &tight, 16000, limit).unwrap_err().code,
            "records.query_resource_budget"
        );
        assert_eq!(tight.snapshot().unwrap().retained_bytes, 0);
        let cancelled = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            Some(Arc::new(AtomicBool::new(true))),
        );
        assert_eq!(
            json_bytes(&escaped, &cancelled, 16000, limit)
                .unwrap_err()
                .exit_code,
            7
        );
        assert_eq!(json_size(&escaped, &cancelled).unwrap_err().exit_code, 7);
    }
}
