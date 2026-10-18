//! Complete sequential content with one live chunk. EOF is a verified terminal
//! state, including exact framing, output length and the external whole CKey.
use super::*;
use crate::phase_profile::Phase;
use crate::resource::RetainedReservation;
use std::io;
mod zlib;
use zlib::ZlibChunk;

pub(crate) fn stream_io_error(error: io::Error) -> lycheedev_core::ToolkitError {
    if let Some(inner) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<lycheedev_core::ToolkitError>())
    {
        inner.clone()
    } else if error.kind() == io::ErrorKind::UnexpectedEof {
        fault("container.invalid_format", 4, "Truncated encoded content")
    } else {
        lycheedev_core::ToolkitError::io(error)
    }
}

pub(crate) struct Reader<'a> {
    source: &'a mut dyn Read,
    decoder: Decoder<'a>,
    parts: Vec<Part>,
    next: usize,
    expected_bytes: usize,
    expected_key: [u8; 16],
    hash: Md5,
    emitted: usize,
    current: Vec<u8>,
    zlib: Option<ZlibChunk<'a>>,
    offset: usize,
    scratch: Option<RetainedReservation<'a>>,
    incremental: bool,
    failed: Option<lycheedev_core::ToolkitError>,
    finished: bool,
    framed: bool,
    allow_missing: bool,
    chunks_authenticated: bool,
    missing: Vec<Missing>,
}
impl<'a> Reader<'a> {
    pub(crate) fn new(
        source: &'a mut dyn Read,
        encoded_bytes: usize,
        decoded_bytes: usize,
        expected_key: [u8; 16],
        keys: &'a BTreeMap<u64, Vec<u8>>,
        budget: &'a Budget,
    ) -> Result<Self> {
        Self::new_with_check(
            source,
            encoded_bytes,
            decoded_bytes,
            expected_key,
            keys,
            budget,
            None,
        )
    }
    pub(crate) fn new_with_check(
        source: &'a mut dyn Read,
        encoded_bytes: usize,
        decoded_bytes: usize,
        expected_key: [u8; 16],
        keys: &'a BTreeMap<u64, Vec<u8>>,
        budget: &'a Budget,
        source_check: Option<&'a dyn Fn() -> Result<()>>,
    ) -> Result<Self> {
        let check = || {
            if let Some(check) = source_check {
                check()?;
            }
            budget.check()
        };
        check()?;
        let limits = Limits {
            encoded_bytes,
            decoded_bytes: decoded_bytes.max(1),
            ..Default::default()
        };
        let mut prefix = [0; 8];
        source.read_exact(&mut prefix).map_err(stream_io_error)?;
        if &prefix[..4] != b"BLTE" {
            return malformed();
        }
        let header = u32::from_be_bytes(prefix[4..].try_into().unwrap()) as usize;
        let mut parts = Vec::new();
        if header == 0 {
            let encoded = encoded_bytes
                .checked_sub(8)
                .ok_or_else(|| fault("container.invalid_format", 4, "Short BLTE extent"))?;
            if encoded == 0 || encoded > limits.chunk_bytes || decoded_bytes > limits.chunk_bytes {
                return exceeded();
            }
            budget.charge(Cost::metadata(std::mem::size_of::<Part>()))?;
            parts.push(Part {
                encoded,
                decoded: decoded_bytes,
                digest: [0; 16],
            });
        } else {
            let mut count = [0; 4];
            source.read_exact(&mut count).map_err(stream_io_error)?;
            let count_value =
                ((count[1] as usize) << 16) | ((count[2] as usize) << 8) | count[3] as usize;
            if count[0] != 15
                || count_value == 0
                || count_value > limits.chunks
                || header != 12 + 24 * count_value
                || header > encoded_bytes
            {
                return malformed();
            }
            budget.charge(Cost::metadata(count_value * std::mem::size_of::<Part>()))?;
            parts.reserve_exact(count_value);
            let (mut encoded_total, mut decoded_total) = (header, 0usize);
            for _ in 0..count_value {
                check()?;
                let mut record = [0; 24];
                source.read_exact(&mut record).map_err(stream_io_error)?;
                let part = Part {
                    encoded: u32::from_be_bytes(record[..4].try_into().unwrap()) as usize,
                    decoded: u32::from_be_bytes(record[4..8].try_into().unwrap()) as usize,
                    digest: record[8..].try_into().unwrap(),
                };
                if part.encoded == 0 {
                    return malformed();
                }
                if part.encoded > limits.chunk_bytes || part.decoded > limits.chunk_bytes {
                    return exceeded();
                }
                encoded_total = encoded_total.checked_add(part.encoded).ok_or_else(|| {
                    fault("container.resource_limit", 3, "Encoded extent overflow")
                })?;
                decoded_total = decoded_total.checked_add(part.decoded).ok_or_else(|| {
                    fault("container.resource_limit", 3, "Decoded extent overflow")
                })?;
                if encoded_total > encoded_bytes || decoded_total > decoded_bytes {
                    return exceeded();
                }
                parts.push(part);
            }
            if encoded_total != encoded_bytes {
                return malformed();
            }
            if decoded_total != decoded_bytes {
                return integrity();
            }
        }
        let chunks = parts.len();
        let chunks_authenticated = parts.iter().all(|part| part.digest != [0; 16]);
        Ok(Self {
            source,
            decoder: Decoder {
                limits,
                keys,
                chunks,
                allow_missing: false,
                budget: Some(budget),
                logical_layout: None,
                source_check,
            },
            parts,
            next: 0,
            expected_bytes: decoded_bytes,
            expected_key,
            hash: Md5::new(),
            emitted: 0,
            current: Vec::new(),
            zlib: None,
            offset: 0,
            scratch: None,
            incremental: false,
            failed: None,
            finished: false,
            framed: header != 0,
            allow_missing: false,
            chunks_authenticated,
            missing: Vec::new(),
        })
    }

    /// Only private staged publishers opt in. A partial chunk may reach their
    /// unpublished spool, never a public blob. On failure they stop immediately
    /// and retain the first observed error, then perform their checked cleanup.
    /// The public eager Decode/unpack and other Reader consumers stay unchanged.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_incremental_with_check(
        source: &'a mut dyn Read,
        encoded_bytes: usize,
        decoded_bytes: usize,
        expected_key: [u8; 16],
        keys: &'a BTreeMap<u64, Vec<u8>>,
        budget: &'a Budget,
        source_check: Option<&'a dyn Fn() -> Result<()>>,
    ) -> Result<Self> {
        let mut reader = Self::new_with_check(
            source,
            encoded_bytes,
            decoded_bytes,
            expected_key,
            keys,
            budget,
            source_check,
        )?;
        reader.incremental = true;
        Ok(reader)
    }

    /// Lazy public-key resolution precedes the table-only missing fallback.
    pub(crate) fn with_policy(mut self, policy: DecodePolicy) -> Self {
        self.allow_missing = policy == DecodePolicy::TableAllowMissing;
        self
    }
    pub(crate) fn take_missing(&mut self) -> Vec<Missing> {
        std::mem::take(&mut self.missing)
    }

    fn advance(&mut self) -> Result<()> {
        self.current = Vec::new();
        self.zlib = None;
        self.scratch = None;
        self.offset = 0;
        let budget = self.decoder.budget.expect("stream owns a budget");
        if self.next == self.parts.len() {
            let mut trailing = [0; 1];
            if self.source.read(&mut trailing).map_err(stream_io_error)? != 0 {
                return malformed();
            }
            if self.emitted != self.expected_bytes
                || (self.missing.is_empty()
                    && self.hash.clone().finalize().as_slice() != self.expected_key)
            {
                return Err(fault(
                    "records.content_integrity",
                    4,
                    "Decoded CASC content key or length differs",
                ));
            }
            self.decoder.check()?;
            self.finished = true;
            return Ok(());
        }
        let part = &self.parts[self.next];
        // Keep the original conservative admission and its error order, even
        // when Z no longer allocates a decoded-sized Vec. This guard survives
        // consumption and drops before the next chunk. Lower actual buffer
        // capacity does not imply a lower admitted/snapshot memory peak.
        let reserved = (part.encoded + part.decoded)
            .checked_mul(2)
            .and_then(|n| n.checked_add(65536))
            .ok_or_else(|| fault("container.resource_limit", 3, "Chunk allocation overflow"))?;
        let scratch = budget.reserve_retained(reserved)?;
        let mut encoded = vec![0; part.encoded];
        {
            let _span = budget.phase(Phase::EncodedChunkRead);
            for block in encoded.chunks_mut(65536) {
                self.decoder.check()?;
                self.source.read_exact(block).map_err(stream_io_error)?;
            }
        }
        if part.digest != [0; 16] {
            let _span = budget.phase(Phase::EncodedChunkMd5);
            let mut hash = Md5::new();
            for block in encoded.chunks(65536) {
                self.decoder.check()?;
                budget.charge(Cost::work(1))?;
                hash.update(block);
            }
            if hash.finalize().as_slice() != part.digest {
                return integrity();
            }
        }
        if self.incremental && encoded[0] == b'Z' {
            self.decoder.check()?;
            self.zlib = Some(ZlibChunk::new(encoded, part.decoded, budget)?);
        } else {
            // Nested decoding stays strict: Go323 marks the entire outer chunk
            // unavailable when any nested key is missing, including its metadata.
            let content =
                match self
                    .decoder
                    .unpack(&encoded, 1, part.decoded, self.next, Some(part.decoded))
                {
                    Ok(content) => content,
                    Err(error)
                        if self.allow_missing
                            && error.code == "container.key_unavailable"
                            && error.details["missingKey"] == true
                            && error.details.as_object().is_some_and(|details| {
                                details
                                    .keys()
                                    .all(|key| matches!(key.as_str(), "missingKey" | "keyId"))
                            }) =>
                    {
                        self.decoder.check()?;
                        if !self.framed {
                            return Err(fault(
                                "container.unsupported_encoding",
                                3,
                                "Headerless BLTE cannot expose available ranges",
                            ));
                        }
                        // Missing bytes remove the whole-CKey fallback proof.
                        if !self.chunks_authenticated {
                            return integrity();
                        }
                        budget.charge(Cost::metadata(size_of::<Missing>() * 3 + 64))?;
                        self.missing.push(Missing {
                            offset: self.emitted,
                            bytes: part.decoded,
                            key_id: error.details["keyId"].as_str().unwrap_or("").to_owned(),
                            chunk: self.next,
                        });
                        Content {
                            bytes: vec![0; part.decoded],
                            missing: Vec::new(),
                        }
                    }
                    Err(error) => return Err(error),
                };
            if content.bytes.len() != part.decoded {
                return integrity();
            }
            self.current = content.bytes;
        }
        self.scratch = Some(scratch);
        self.next += 1;
        Ok(())
    }
    fn read_content(&mut self, out: &mut [u8]) -> Result<usize> {
        self.decoder.check()?;
        if out.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some(zlib) = &mut self.zlib {
                if !zlib.finished() {
                    let n = zlib.read(out, &self.decoder)?;
                    if n != 0 {
                        let budget = self.decoder.budget.expect("stream owns a budget");
                        budget.charge(Cost::work(1))?;
                        let _span = budget.phase(Phase::WholeContentKeyHash);
                        self.hash.update(&out[..n]);
                        self.emitted += n;
                        return Ok(n);
                    }
                }
            } else if self.offset != self.current.len() {
                break;
            }
            if self.finished {
                return Ok(0);
            }
            self.advance()?;
        }
        let n = out.len().min(65536).min(self.current.len() - self.offset);
        let block = &self.current[self.offset..self.offset + n];
        self.decoder
            .budget
            .expect("stream owns a budget")
            .charge(Cost::work(1))?;
        {
            let _span = self
                .decoder
                .budget
                .expect("stream owns a budget")
                .phase(Phase::WholeContentKeyHash);
            self.hash.update(block);
        }
        out[..n].copy_from_slice(block);
        self.offset += n;
        self.emitted += n;
        Ok(n)
    }
}
impl Read for Reader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if let Some(error) = &self.failed {
            return Err(io::Error::other(error.clone()));
        }
        self.read_content(out).map_err(|error| {
            // Incremental aggregation can consume buffered bytes before an
            // error. No caller may retry and accidentally emit a shifted suffix.
            if self.incremental {
                self.failed = Some(error.clone());
            }
            io::Error::other(error)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lycheedev_core::control::OperationControl;
    use std::sync::atomic::Ordering;

    #[test]
    fn cancelling_after_first_stream_block_stops_before_more_decoded_output() {
        let bytes = vec![b'x'; 3 * 65536];
        let expected = Md5::digest(&bytes).into();
        let mut encoded = b"BLTE\0\0\0\0N".to_vec();
        encoded.extend_from_slice(&bytes);
        let encoded_length = encoded.len();
        let mut source = io::Cursor::new(encoded);
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let keys = BTreeMap::new();
        let before = budget.snapshot().unwrap().retained_bytes;
        let mut reader = Reader::new(
            &mut source,
            encoded_length,
            bytes.len(),
            expected,
            &keys,
            &budget,
        )
        .unwrap();
        let mut buffer = [0; 65536];
        assert_eq!(reader.read(&mut buffer).unwrap(), buffer.len());
        assert!(buffer.iter().all(|byte| *byte == b'x'));
        control.cancel_handle().store(true, Ordering::Release);
        let error = stream_io_error(reader.read(&mut buffer).unwrap_err());
        assert_eq!(error.exit_code, 7);
        assert_eq!(reader.emitted, 65536);
        drop(reader);
        // Only frame metadata remains admitted; the live chunk's allocation is
        // released with the cancelled reader rather than carried into a retry.
        assert_eq!(
            budget.snapshot().unwrap().retained_bytes,
            before + std::mem::size_of::<Part>() as u64
        );
    }
}

#[cfg(test)]
mod controlled_tests;

#[cfg(test)]
mod zlib_tests;

#[cfg(test)]
mod partial_tests;
