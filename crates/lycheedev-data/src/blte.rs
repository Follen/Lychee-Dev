// SPDX-License-Identifier: MIT
// BLTE/Salsa20 contracts ported from Lychee Dev's MIT wowdata-derived reader.
use crate::{
    fault,
    resource::{Budget, Cost},
};
use lycheedev_core::Result;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::BTreeMap, io::Read};
mod span;
pub use span::EncodedSpan;
pub(crate) use span::OwnedDecodedSpan;
mod guarded;
mod streaming;
pub(crate) use streaming::{Reader as StreamingReader, stream_io_error};

pub trait ByteSource {
    fn size(&self) -> usize;
    fn read_span(&self, offset: usize, bytes: usize) -> Result<Cow<'_, [u8]>>;
    fn read_checked_span(
        &self,
        offset: usize,
        bytes: usize,
        expected: &[u8],
    ) -> Result<Cow<'_, [u8]>> {
        let raw = self.read_span(offset, bytes)?;
        if raw.len() != bytes
            || expected.len() != 16
            || expected.iter().all(|b| *b == 0)
            || Md5::digest(&raw).as_slice() != expected
        {
            return integrity();
        }
        Ok(raw)
    }
}
impl ByteSource for Content {
    fn size(&self) -> usize {
        self.bytes.len()
    }
    fn read_span(&self, offset: usize, bytes: usize) -> Result<Cow<'_, [u8]>> {
        self.checked_span(offset, bytes).map(Cow::Borrowed)
    }
}
/// Immutable physical section with bounded random reads.
pub trait EncodedRead {
    /// Observe the source's original scope without reading encoded bytes.
    fn check_encoded(&self) -> Result<()> {
        Ok(())
    }
    fn encoded_size(&self) -> usize;
    fn read_encoded(&self, offset: usize, bytes: usize) -> Result<EncodedSpan<'_>>;
}
enum RangeInput<'a> {
    Slice(&'a [u8]),
    Reader(&'a dyn EncodedRead),
}
impl RangeInput<'_> {
    fn check(&self) -> Result<()> {
        match self {
            Self::Slice(_) => Ok(()),
            Self::Reader(reader) => reader.check_encoded(),
        }
    }
    fn len(&self) -> usize {
        match self {
            Self::Slice(raw) => raw.len(),
            Self::Reader(reader) => reader.encoded_size(),
        }
    }
    fn read(&self, offset: usize, bytes: usize) -> Result<EncodedSpan<'_>> {
        self.check()?;
        match self {
            Self::Slice(raw) => raw
                .get(
                    offset..offset.checked_add(bytes).ok_or_else(|| {
                        fault("container.invalid_format", 4, "Encoded range overflow")
                    })?,
                )
                .map(EncodedSpan::borrowed)
                .ok_or_else(|| {
                    fault(
                        "container.invalid_format",
                        4,
                        "Encoded range outside extent",
                    )
                }),
            Self::Reader(reader) => {
                let raw = reader.read_encoded(offset, bytes)?;
                if raw.len() != bytes {
                    return malformed();
                }
                Ok(raw)
            }
        }
    }
}
struct Segment {
    encoded: usize,
    decoded: usize,
    part: Part,
}
/// General immutable framed BLTE range reader. Admission validates the whole
/// directory; chunk limits apply to visited chunks, not untouched segments.
/// The caller authenticates the framing EKey. Reading a range is not whole
/// decoded CKey authentication.
pub struct Ranges<'a> {
    raw: RangeInput<'a>,
    parts: Vec<Segment>,
    size: usize,
    limits: Limits,
    keys: &'a BTreeMap<u64, Vec<u8>>,
    budget: Option<&'a Budget>,
}
impl<'a> Ranges<'a> {
    pub fn open(raw: &'a [u8], limits: Limits, keys: &'a BTreeMap<u64, Vec<u8>>) -> Result<Self> {
        Self::from_input(RangeInput::Slice(raw), limits, keys, None)
    }
    pub fn open_reader(
        reader: &'a dyn EncodedRead,
        limits: Limits,
        keys: &'a BTreeMap<u64, Vec<u8>>,
    ) -> Result<Self> {
        Self::from_input(RangeInput::Reader(reader), limits, keys, None)
    }
    pub fn open_reader_budgeted(
        reader: &'a dyn EncodedRead,
        limits: Limits,
        keys: &'a BTreeMap<u64, Vec<u8>>,
        budget: &'a Budget,
    ) -> Result<Self> {
        Self::from_input(RangeInput::Reader(reader), limits, keys, Some(budget))
    }
    fn from_input(
        raw: RangeInput<'a>,
        limits: Limits,
        keys: &'a BTreeMap<u64, Vec<u8>>,
        budget: Option<&'a Budget>,
    ) -> Result<Self> {
        raw.check()?;
        if let Some(budget) = budget {
            budget.check()?;
        }
        if limits.encoded_bytes == 0
            || raw.len() > limits.encoded_bytes
            || limits.decoded_bytes == 0
            || limits.chunk_bytes == 0
            || limits.chunk_bytes > 64 << 20
            || limits.chunks == 0
            || limits.chunks > 65536
            || limits.depth == 0
            || limits.depth > 16
        {
            return exceeded();
        }
        if raw.len() < 8 {
            return malformed();
        }
        let short = raw.read(0, 8)?;
        if &short[..4] != b"BLTE" {
            return malformed();
        }
        let header = u32::from_be_bytes(short[4..8].try_into().unwrap()) as usize;
        if header == 0 {
            return Err(fault(
                "container.unsupported_encoding",
                3,
                "Headerless BLTE has no range directory",
            ));
        }
        if header < 12 || header > raw.len() {
            return malformed();
        }
        if header > 12 + 24 * limits.chunks {
            return exceeded();
        }
        // Go's logical header scratch is distinct from the real owned spans
        // supplied by an encoded reader and refunded with those allocations.
        let _header_scratch = budget
            .map(|b| b.reserve_logical_scratch(header as u64))
            .transpose()?;
        let prefix = raw.read(0, 12)?;
        if prefix[8] != 15 {
            return malformed();
        }
        if header == 0 {
            return Err(fault(
                "container.unsupported_encoding",
                3,
                "Headerless BLTE has no range directory",
            ));
        }
        let count =
            ((prefix[9] as usize) << 16) | ((prefix[10] as usize) << 8) | prefix[11] as usize;
        if count == 0 || count > limits.chunks || header != 12 + 24 * count || header > raw.len() {
            return malformed();
        }
        let (mut encoded, mut decoded) = (header, 0usize);
        if let Some(budget) = budget {
            budget.charge(Cost::metadata(count * std::mem::size_of::<Segment>()))?;
        }
        let directory = raw.read(0, header)?;
        let mut parts = Vec::with_capacity(count);
        for i in 0..count {
            let at = 12 + 24 * i;
            let p = Part {
                encoded: u32::from_be_bytes(directory[at..at + 4].try_into().unwrap()) as usize,
                decoded: u32::from_be_bytes(directory[at + 4..at + 8].try_into().unwrap()) as usize,
                digest: directory[at + 8..at + 24].try_into().unwrap(),
            };
            if p.encoded == 0 {
                return malformed();
            }
            let next = encoded
                .checked_add(p.encoded)
                .ok_or_else(|| fault("container.resource_limit", 3, "Encoded extent overflow"))?;
            let logical = decoded
                .checked_add(p.decoded)
                .ok_or_else(|| fault("container.resource_limit", 3, "Decoded extent overflow"))?;
            if next > raw.len() || logical > limits.decoded_bytes {
                return exceeded();
            }
            parts.push(Segment {
                encoded,
                decoded,
                part: p,
            });
            encoded = next;
            decoded = logical;
        }
        if encoded != raw.len() {
            return malformed();
        }
        drop(directory);
        drop(prefix);
        drop(short);
        Ok(Self {
            raw,
            parts,
            size: decoded,
            limits,
            keys,
            budget,
        })
    }
    fn check(&self) -> Result<()> {
        self.raw.check()?;
        if let Some(budget) = self.budget {
            budget.check()?;
        }
        Ok(())
    }
    fn extent(&self, offset: usize, bytes: usize) -> Result<usize> {
        let end = offset
            .checked_add(bytes)
            .filter(|end| *end <= self.size)
            .ok_or_else(|| {
                fault(
                    "container.invalid_format",
                    4,
                    "BLTE logical range outside extent",
                )
            })?;
        if bytes > self.limits.chunk_bytes {
            return exceeded();
        }
        Ok(end)
    }
    // Both lifetime policies share exactly one chunk decoder/read loop.
    fn decode_span(&self, offset: usize, bytes: usize, end: usize) -> Result<Vec<u8>> {
        if bytes == 0 {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(bytes);
        for (index, s) in self.parts.iter().enumerate() {
            let last = s.decoded + s.part.decoded;
            if offset >= last || end <= s.decoded {
                continue;
            }
            if s.part.encoded > self.limits.chunk_bytes || s.part.decoded > self.limits.chunk_bytes
            {
                return exceeded();
            }
            let raw = self.raw.read(s.encoded, s.part.encoded)?;
            if let Some(budget) = self.budget {
                budget.charge(Cost {
                    retained_bytes: s.part.decoded as u64 * 2,
                    decode_work: (s.part.encoded + s.part.decoded).div_ceil(65536) as u64,
                    ..Default::default()
                })?;
            }
            if Md5::digest(&raw).as_slice() != s.part.digest {
                return integrity();
            }
            let mut d = Decoder {
                limits: self.limits,
                keys: self.keys,
                chunks: 1,
                allow_missing: false,
                // The extent reservation covers the output/copy and digest
                // work. Decoder scratch, nested/cipher buffers and codec-loop
                // work still belong to this same ledger and cancellation tree.
                budget: self.budget,
            };
            let content = d.unpack(
                &raw,
                1,
                self.limits.chunk_bytes,
                index,
                Some(s.part.decoded),
            )?;
            if content.bytes.len() != s.part.decoded {
                return integrity();
            }
            result.extend_from_slice(
                &content.bytes
                    [offset.saturating_sub(s.decoded)..(end - s.decoded).min(s.part.decoded)],
            );
        }
        if result.len() != bytes {
            return malformed();
        }
        Ok(result)
    }

    fn checked_direct_span<'s, T>(
        &'s self,
        offset: usize,
        bytes: usize,
        expected: &[u8],
        consume: impl FnOnce(EncodedSpan<'s>) -> Result<T>,
    ) -> Result<Option<T>> {
        self.check()?;
        if expected.len() != 16 || expected.iter().all(|b| *b == 0) {
            return integrity();
        }
        let end = self.extent(offset, bytes)?;
        // An authenticated logical-page digest permits bounded direct reads
        // inside N segments, even when their entire decoded segment is huge.
        for s in &self.parts {
            if bytes > 0
                && offset >= s.decoded
                && end <= s.decoded + s.part.decoded
                && self.raw.read(s.encoded, 1)?[0] == b'N'
            {
                if s.part.encoded != s.part.decoded + 1 {
                    return integrity();
                }
                let _scratch = self
                    .budget
                    .map(|b| b.reserve_logical_scratch(bytes as u64))
                    .transpose()?;
                let raw = self.raw.read(s.encoded + 1 + offset - s.decoded, bytes)?;
                if expected.len() != 16
                    || expected.iter().all(|b| *b == 0)
                    || Md5::digest(&raw).as_slice() != expected
                {
                    return integrity();
                }
                return consume(raw).map(Some);
            }
        }
        Ok(None)
    }
}
impl ByteSource for Ranges<'_> {
    fn size(&self) -> usize {
        self.size
    }
    fn read_span(&self, offset: usize, bytes: usize) -> Result<Cow<'_, [u8]>> {
        self.check()?;
        let end = self.extent(offset, bytes)?;
        if let Some(budget) = self.budget {
            budget.charge(Cost::retained(bytes))?;
        }
        self.decode_span(offset, bytes, end).map(Cow::Owned)
    }
    fn read_checked_span(
        &self,
        offset: usize,
        bytes: usize,
        expected: &[u8],
    ) -> Result<Cow<'_, [u8]>> {
        if let Some(raw) = self.checked_direct_span(offset, bytes, expected, |raw| {
            raw.decoded_copy(self.budget, || self.check())
        })? {
            return Ok(raw);
        }
        let raw = self.read_span(offset, bytes)?;
        if expected.len() != 16
            || expected.iter().all(|b| *b == 0)
            || Md5::digest(&raw).as_slice() != expected
        {
            return integrity();
        }
        Ok(raw)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub encoded_bytes: usize,
    pub decoded_bytes: usize,
    pub chunk_bytes: usize,
    pub chunks: usize,
    pub depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            encoded_bytes: 256 << 20,
            decoded_bytes: 256 << 20,
            chunk_bytes: 64 << 20,
            chunks: 65536,
            depth: 16,
        }
    }
}
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Missing {
    pub offset: usize,
    pub bytes: usize,
    pub key_id: String,
    pub chunk: usize,
}
#[derive(Debug)]
pub struct Content {
    pub bytes: Vec<u8>,
    pub missing: Vec<Missing>,
}
impl Content {
    pub fn complete(&self) -> bool {
        self.missing.is_empty()
    }
    pub fn checked_span(&self, offset: usize, len: usize) -> Result<&[u8]> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| fault("container.resource_limit", 3, "Range overflow"))?;
        if end > self.bytes.len() {
            return Err(fault(
                "container.invalid_format",
                4,
                "Range outside content",
            ));
        }
        let mut intersects = false;
        for m in &self.missing {
            let missing_end = m
                .offset
                .checked_add(m.bytes)
                .filter(|n| *n <= self.bytes.len())
                .ok_or_else(|| {
                    fault("container.invalid_format", 4, "Malformed unavailable range")
                })?;
            intersects |= offset < missing_end && end > m.offset;
        }
        if intersects {
            return Err(fault(
                "container.key_unavailable",
                3,
                "Range intersects unavailable encrypted bytes",
            ));
        }
        Ok(&self.bytes[offset..end])
    }
}
fn malformed<T>() -> Result<T> {
    Err(fault(
        "container.invalid_format",
        4,
        "Malformed BLTE container",
    ))
}
fn exceeded<T>() -> Result<T> {
    Err(fault(
        "container.resource_limit",
        3,
        "BLTE resource limit exceeded",
    ))
}
fn integrity<T>() -> Result<T> {
    Err(fault(
        "container.integrity_mismatch",
        4,
        "BLTE checksum/extent mismatch",
    ))
}
#[derive(Clone)]
struct Part {
    encoded: usize,
    decoded: usize,
    digest: [u8; 16],
}
pub fn decode(raw: &[u8], limits: Limits, keys: &BTreeMap<u64, Vec<u8>>) -> Result<Vec<u8>> {
    let c = decode_available(raw, limits, keys, false)?;
    Ok(c.bytes)
}
pub fn decode_available(
    raw: &[u8],
    limits: Limits,
    keys: &BTreeMap<u64, Vec<u8>>,
    allow_missing: bool,
) -> Result<Content> {
    if limits.encoded_bytes == 0
        || limits.decoded_bytes == 0
        || limits.decoded_bytes > 256 << 20
        || limits.chunk_bytes == 0
        || limits.chunk_bytes > 64 << 20
        || limits.chunks == 0
        || limits.chunks > 65536
        || limits.depth == 0
        || limits.depth > 16
        || raw.len() > limits.encoded_bytes
    {
        return exceeded();
    }
    let mut d = Decoder {
        limits,
        keys,
        chunks: 0,
        allow_missing,
        budget: None,
    };
    d.expand(raw, 1, limits.decoded_bytes)
}
struct Decoder<'a> {
    limits: Limits,
    keys: &'a BTreeMap<u64, Vec<u8>>,
    chunks: usize,
    allow_missing: bool,
    budget: Option<&'a Budget>,
}
impl Decoder<'_> {
    fn check(&self) -> Result<()> {
        self.budget.map_or(Ok(()), Budget::check)
    }
    fn expand(&mut self, raw: &[u8], depth: usize, allowance: usize) -> Result<Content> {
        self.check()?;
        if depth > self.limits.depth {
            return exceeded();
        }
        if raw.len() < 8 || &raw[..4] != b"BLTE" {
            return malformed();
        }
        let header = u32::from_be_bytes(raw[4..8].try_into().unwrap()) as usize;
        let mut parts = Vec::new();
        let start = if header == 0 {
            parts.push(Part {
                encoded: raw.len() - 8,
                decoded: 0,
                digest: [0; 16],
            });
            8
        } else {
            if raw.len() < 12 || raw[8] != 15 {
                return malformed();
            }
            let count = ((raw[9] as usize) << 16) | ((raw[10] as usize) << 8) | raw[11] as usize;
            if count == 0 || header != 12 + count * 24 || header > raw.len() {
                return malformed();
            }
            if count > self.limits.chunks.saturating_sub(self.chunks) {
                return exceeded();
            }
            let mut total = 0usize;
            let mut extent = header;
            for i in 0..count {
                self.check()?;
                let at = 12 + i * 24;
                let p = Part {
                    encoded: u32::from_be_bytes(raw[at..at + 4].try_into().unwrap()) as usize,
                    decoded: u32::from_be_bytes(raw[at + 4..at + 8].try_into().unwrap()) as usize,
                    digest: raw[at + 8..at + 24].try_into().unwrap(),
                };
                if p.encoded == 0 {
                    return malformed();
                }
                total = total.checked_add(p.decoded).ok_or_else(|| {
                    fault("container.resource_limit", 3, "Decoded extent overflow")
                })?;
                extent = extent.checked_add(p.encoded).ok_or_else(|| {
                    fault("container.resource_limit", 3, "Encoded extent overflow")
                })?;
                if total > allowance
                    || p.encoded > self.limits.chunk_bytes
                    || p.decoded > self.limits.chunk_bytes
                {
                    return exceeded();
                }
                parts.push(p);
            }
            if extent != raw.len() {
                return malformed();
            }
            header
        };
        self.chunks += parts.len();
        if self.chunks > self.limits.chunks {
            return exceeded();
        }
        let declared = parts
            .iter()
            .try_fold(0usize, |n, p| n.checked_add(p.decoded))
            .ok_or_else(|| fault("container.resource_limit", 3, "Decoded extent overflow"))?;
        let mut content = Content {
            bytes: Vec::with_capacity(declared),
            missing: Vec::new(),
        };
        let mut offset = start;
        for (index, p) in parts.iter().enumerate() {
            self.check()?;
            let encoded = &raw[offset..offset + p.encoded];
            offset += p.encoded;
            if encoded.len() > self.limits.chunk_bytes {
                return exceeded();
            }
            if p.digest != [0; 16] && Md5::digest(encoded).as_slice() != p.digest {
                return integrity();
            }
            let remaining = if header == 0 {
                (allowance - content.bytes.len()).min(self.limits.chunk_bytes)
            } else {
                p.decoded
            };
            match self.unpack(
                encoded,
                depth,
                remaining,
                index,
                if header == 0 { None } else { Some(p.decoded) },
            ) {
                Ok(mut c) => {
                    if header != 0 && c.bytes.len() != p.decoded {
                        return integrity();
                    }
                    for m in &mut c.missing {
                        m.offset += content.bytes.len();
                    }
                    content.missing.extend(c.missing);
                    content.bytes.extend(c.bytes);
                }
                Err(e)
                    if self.allow_missing
                        && header != 0
                        && e.code == "container.key_unavailable"
                        && e.details.get("missingKey").and_then(|v| v.as_bool()) == Some(true) =>
                {
                    let key = e.details["keyId"].as_str().unwrap_or("").to_owned();
                    let begin = content.bytes.len();
                    content.bytes.resize(begin + p.decoded, 0);
                    content.missing.push(Missing {
                        offset: begin,
                        bytes: p.decoded,
                        key_id: key,
                        chunk: index,
                    });
                }
                Err(e) => return Err(e),
            }
        }
        Ok(content)
    }
    fn unpack(
        &mut self,
        raw: &[u8],
        depth: usize,
        allowance: usize,
        index: usize,
        declared: Option<usize>,
    ) -> Result<Content> {
        self.check()?;
        if depth > self.limits.depth {
            return exceeded();
        }
        if raw.is_empty() {
            return malformed();
        }
        let bytes = match raw[0] {
            b'N' => {
                if raw.len() - 1 > allowance {
                    return exceeded();
                }
                raw[1..].to_vec()
            }
            b'Z' => {
                let _state = self.budget.map(|budget| budget.reserve_retained(128 << 10)).transpose()?;
                let mut decoder = flate2::bufread::ZlibDecoder::new(&raw[1..]);
                // Exact framed chunk size is admitted before decompression;
                // reserve once instead of repeatedly growing/copying large Z chunks.
                let mut bytes = Vec::with_capacity(declared.unwrap_or(allowance.min(65536)));
                let mut block = [0; 65536];
                loop {
                    self.check()?;
                    let requested = block.len().min(allowance.saturating_sub(bytes.len()) + 1);
                    let n = decoder.read(&mut block[..requested])
                        .map_err(|_| fault("container.invalid_format", 4, "Invalid zlib payload"))?;
                    self.check()?;
                    if n == 0 { break; }
                    if n > allowance - bytes.len() { return exceeded(); }
                    if let Some(budget) = self.budget { budget.charge(Cost::work(1))?; }
                    bytes.extend_from_slice(&block[..n]);
                }
                if decoder.total_in() as usize != raw.len() - 1 {
                    return malformed();
                }
                bytes
            }
            b'F' => {
                let _nested = self.budget.map(|budget| budget.reserve_retained(allowance.saturating_mul(2))).transpose()?;
                return self.expand(&raw[1..], depth + 1, allowance);
            }
            b'E' => {
                let _cipher = self.budget.map(|budget| budget.reserve_retained(raw.len())).transpose()?;
                let decoded = self.unlock(&raw[1..], index)?;
                return self.unpack(&decoded, depth + 1, allowance, index, declared);
            }
            _ => {
                return Err(fault(
                    "container.unsupported_encoding",
                    3,
                    "Unsupported BLTE mode",
                ));
            }
        };
        Ok(Content {
            bytes,
            missing: Vec::new(),
        })
    }
    fn unlock(&self, raw: &[u8], index: usize) -> Result<Vec<u8>> {
        if raw.len() < 10 || raw[0] != 8 {
            return malformed();
        }
        let id = u64::from_le_bytes(raw[1..9].try_into().unwrap());
        let iv = raw[9] as usize;
        if ![4, 8].contains(&iv) || raw.len() < 12 + iv {
            return malformed();
        }
        if raw[10 + iv] != b'S' {
            return Err(fault(
                "container.unsupported_encoding",
                3,
                "Unsupported BLTE cipher",
            ));
        }
        let Some(key) = self.keys.get(&id) else {
            let mut e = fault(
                "container.key_unavailable",
                3,
                format!("BLTE key unavailable: {id:016x}"),
            );
            e.details = serde_json::json!({"missingKey":true,"keyId":format!("{id:016x}")});
            return Err(e);
        };
        if ![16, 32].contains(&key.len()) {
            return Err(fault(
                "container.key_unavailable",
                3,
                "Invalid BLTE key length",
            ));
        }
        let mut nonce = [0; 8];
        nonce[..iv].copy_from_slice(&raw[10..10 + iv]);
        for (n, b) in nonce[..4].iter_mut().enumerate() {
            *b ^= (index >> (n * 8)) as u8;
        }
        salsa(&raw[11 + iv..], key, nonce, self.budget)
    }
}
fn salsa(raw: &[u8], key: &[u8], nonce: [u8; 8], budget: Option<&Budget>) -> Result<Vec<u8>> {
    let mut state = [0u32; 16];
    let constant = if key.len() == 16 {
        b"expand 16-byte k"
    } else {
        b"expand 32-byte k"
    };
    for (i, p) in [0, 5, 10, 15].iter().enumerate() {
        state[*p] = u32::from_le_bytes(constant[i * 4..i * 4 + 4].try_into().unwrap());
    }
    for i in 0..4 {
        state[1 + i] = u32::from_le_bytes(key[i * 4..i * 4 + 4].try_into().unwrap());
        let o = (16 + i * 4) % key.len();
        state[11 + i] = u32::from_le_bytes(key[o..o + 4].try_into().unwrap());
    }
    state[6] = u32::from_le_bytes(nonce[..4].try_into().unwrap());
    state[7] = u32::from_le_bytes(nonce[4..].try_into().unwrap());
    let mut out = vec![0; raw.len()];
    for start in (0..raw.len()).step_by(64) {
        if start % 65536 == 0 && let Some(budget) = budget {
            budget.charge(Cost::work(1))?;
        }
        let mut x = state;
        for _ in 0..10 {
            for (a, b, c, d) in [
                (0, 4, 8, 12),
                (5, 9, 13, 1),
                (10, 14, 2, 6),
                (15, 3, 7, 11),
                (0, 1, 2, 3),
                (5, 6, 7, 4),
                (10, 11, 8, 9),
                (15, 12, 13, 14),
            ] {
                quarter(&mut x, a, b, c, d);
            }
        }
        for i in 0..(raw.len() - start).min(64) {
            out[start + i] =
                raw[start + i] ^ x[i / 4].wrapping_add(state[i / 4]).to_le_bytes()[i % 4];
        }
        state[8] = state[8].wrapping_add(1);
        if state[8] == 0 {
            state[9] = state[9].wrapping_add(1);
        }
    }
    Ok(out)
}
fn quarter(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[b] ^= x[a].wrapping_add(x[d]).rotate_left(7);
    x[c] ^= x[b].wrapping_add(x[a]).rotate_left(9);
    x[d] ^= x[c].wrapping_add(x[b]).rotate_left(13);
    x[a] ^= x[d].wrapping_add(x[c]).rotate_left(18);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn frame(payload: &[u8], decoded: usize) -> Vec<u8> {
        let mut v = b"BLTE".to_vec();
        v.extend(36u32.to_be_bytes());
        v.extend([15, 0, 0, 1]);
        v.extend((payload.len() as u32).to_be_bytes());
        v.extend((decoded as u32).to_be_bytes());
        v.extend(Md5::digest(payload));
        v.extend(payload);
        v
    }
    #[test]
    fn raw_zlib_nested_and_integrity() {
        let keys = BTreeMap::new();
        let l = Limits::default();
        assert_eq!(decode(&frame(b"Nhello", 5), l, &keys).unwrap(), b"hello");
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(b"hello").unwrap();
        let mut payload = vec![b'Z'];
        payload.extend(z.finish().unwrap());
        assert_eq!(decode(&frame(&payload, 5), l, &keys).unwrap(), b"hello");
        payload.push(0);
        assert!(decode(&frame(&payload, 5), l, &keys).is_err());
        let mut nested = vec![b'F'];
        nested.extend(frame(b"Nhello", 5));
        assert_eq!(decode(&frame(&nested, 5), l, &keys).unwrap(), b"hello");
        let mut bad = frame(b"Nhello", 5);
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(
            decode(&bad, l, &keys).unwrap_err().code,
            "container.integrity_mismatch"
        );
    }
    #[test]
    fn encrypted_keys_partial_and_no_placeholder_read() {
        for width in [16, 32] {
            let key = vec![0; width];
            let id = 42u64;
            let mut e = vec![b'E', 8];
            e.extend(id.to_le_bytes());
            e.push(4);
            e.extend([0; 4]);
            e.push(b'S');
            e.extend(salsa(b"Nhello", &key, [0; 8], None).unwrap());
            let b = frame(&e, 5);
            assert_eq!(
                decode(&b, Limits::default(), &BTreeMap::from([(id, key)])).unwrap(),
                b"hello"
            );
            let c = decode_available(&b, Limits::default(), &BTreeMap::new(), true).unwrap();
            assert!(!c.complete());
            assert_eq!(c.missing[0].key_id, "000000000000002a");
            assert!(c.checked_span(0, 1).is_err());
            assert!(decode(&b, Limits::default(), &BTreeMap::new()).is_err());
        }
    }
    #[test]
    fn budgets_and_external_ckey() {
        assert!(
            decode(
                &frame(b"Nhello", 5),
                Limits {
                    decoded_bytes: 4,
                    ..Default::default()
                },
                &BTreeMap::new()
            )
            .is_err()
        );
        let mut b = frame(b"Nhello", 5);
        b.push(0);
        assert!(decode(&b, Limits::default(), &BTreeMap::new()).is_err());
    }

    #[test]
    fn framed_zlib_range_keeps_the_budget_inside_the_decoder() {
        use std::time::{Duration, Instant};

        let expected = vec![7u8; 128 << 10];
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&expected).unwrap();
        let mut payload = vec![b'Z'];
        payload.extend(z.finish().unwrap());
        assert!(payload.len() < 65536);
        let raw = frame(&payload, expected.len());
        let keys = BTreeMap::new();

        let budget = Budget::new(Cost::defaults(), Instant::now() + Duration::from_secs(5), None);
        let ranges = Ranges::from_input(RangeInput::Slice(&raw), Limits::default(), &keys, Some(&budget)).unwrap();
        assert_eq!(&*ranges.read_span(0, expected.len()).unwrap(), expected);

        // Framing and extent admission fit. Actual zlib-loop work must still
        // stop under the same budget rather than silently decoding unmetered.
        let limited = Budget::new(Cost { decode_work: 3, ..Cost::defaults() }, Instant::now() + Duration::from_secs(5), None);
        let ranges = Ranges::from_input(RangeInput::Slice(&raw), Limits::default(), &keys, Some(&limited)).unwrap();
        let error = ranges.read_span(0, expected.len()).unwrap_err();
        assert_eq!(error.code, "records.query_resource_budget");
        assert_eq!(error.message, "Whole query decode_work budget exceeded");
    }
}
