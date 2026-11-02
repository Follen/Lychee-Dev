// SPDX-License-Identifier: MIT
//! Authenticated TACT index footer, TOC and one selected page. The index name
//! authenticates its 28-byte footer, never an unverified whole-file shortcut.
use super::{catalog, invalid, limit};
use crate::fault;
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

fn integrity() -> ToolkitError {
    fault(
        "records.configuration_integrity",
        4,
        "records.configuration_integrity",
    )
}
fn missing() -> ToolkitError {
    fault("records.content_missing", 3, "records.content_missing")
}
fn key_bytes(key: &str) -> Result<[u8; 16]> {
    if !catalog::valid_key(key) {
        return Err(invalid());
    }
    let mut result = [0u8; 16];
    for (output, pair) in result.iter_mut().zip(key.as_bytes().chunks_exact(2)) {
        let digit = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
        *output = digit(pair[0]) * 16 + digit(pair[1]);
    }
    Ok(result)
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Span {
    pub object: String,
    pub offset: i64,
    pub bytes: i64,
}
#[derive(Clone, Debug)]
pub(crate) struct Footer {
    page_bytes: usize,
    stride: usize,
    per_page: usize,
    entries: usize,
    offset_bytes: usize,
    pages: usize,
    toc_hash: [u8; 8],
}
impl Footer {
    pub fn parse(size: i64, key: &str, raw: &[u8]) -> Result<Self> {
        let expected = key_bytes(key)?;
        if size < 28 || raw.len() != 28 {
            return Err(invalid());
        }
        if size > 1 << 30 {
            return Err(limit());
        }
        if Md5::digest(raw).as_slice() != expected {
            return Err(integrity());
        }
        if raw[8] != 1
            || raw[9] != 0
            || raw[10] != 0
            || raw[13] != 4
            || raw[14] != 16
            || raw[15] != 8
            || !matches!(raw[12], 0 | 4 | 5 | 6)
        {
            return Err(invalid());
        }
        let mut check = [0u8; 20];
        check[..12].copy_from_slice(&raw[8..20]);
        if Md5::digest(check)[..8] != raw[20..] {
            return Err(integrity());
        }
        let page_bytes = raw[11] as usize * 1024;
        let stride = 20 + raw[12] as usize;
        let entries = u32::from_le_bytes(raw[16..20].try_into().map_err(|_| invalid())?) as usize;
        if page_bytes == 0 || entries > 10_000_000 {
            return Err(limit());
        }
        let per_page = page_bytes / stride;
        let pages = entries.div_ceil(per_page);
        if (pages as i64) * (page_bytes as i64 + 24) + 28 != size {
            return Err(invalid());
        }
        Ok(Self {
            page_bytes,
            stride,
            per_page,
            entries,
            offset_bytes: raw[12] as usize,
            pages,
            toc_hash: raw[..8].try_into().map_err(|_| invalid())?,
        })
    }
    pub fn toc_span(&self) -> (i64, usize) {
        ((self.pages * self.page_bytes) as i64, self.pages * 24)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageRequest {
    ordinal: usize,
    pub offset: i64,
    pub bytes: usize,
}
pub(crate) struct Directory {
    footer: Footer,
    toc: Vec<u8>,
}
impl Directory {
    pub fn parse(footer: Footer, toc: Vec<u8>, control: &OperationControl) -> Result<Self> {
        control.check()?;
        if toc.len() != footer.pages * 24 {
            return Err(invalid());
        }
        if Md5::digest(&toc)[..8] != footer.toc_hash {
            return Err(integrity());
        }
        let keys = &toc[..footer.pages * 16];
        for (i, key) in keys.chunks_exact(16).enumerate() {
            control.check()?;
            if key == [0u8; 16] || i > 0 && &keys[(i - 1) * 16..i * 16] >= key {
                return Err(invalid());
            }
        }
        Ok(Self { footer, toc })
    }
    fn key(&self, index: usize) -> &[u8] {
        &self.toc[index * 16..index * 16 + 16]
    }
    pub fn page(&self, key: &str, control: &OperationControl) -> Result<PageRequest> {
        control.check()?;
        let needle = key_bytes(key)?;
        let mut low = 0;
        let mut high = self.footer.pages;
        while low < high {
            let middle = low + (high - low) / 2;
            if self.key(middle) < needle.as_slice() {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low == self.footer.pages {
            return Err(missing());
        }
        Ok(PageRequest {
            ordinal: low,
            offset: (low * self.footer.page_bytes) as i64,
            bytes: self.footer.page_bytes,
        })
    }
    pub fn locate(
        &self,
        request: PageRequest,
        key: &str,
        page: &[u8],
        archives: &[String],
        object: &str,
        control: &OperationControl,
    ) -> Result<Span> {
        control.check()?;
        let expected = self.page(key, control)?;
        if request != expected || page.len() != request.bytes {
            return Err(invalid());
        }
        let needle = key_bytes(key)?;
        let p = request.ordinal;
        let f = &self.footer;
        let hash_start = f.pages * 16 + p * 8;
        if Md5::digest(page)[..8] != self.toc[hash_start..hash_start + 8] {
            return Err(integrity());
        }
        let count = f.per_page.min(f.entries - p * f.per_page);
        let mut found = None;
        for i in 0..count {
            control.check()?;
            let entry = &page[i * f.stride..(i + 1) * f.stride];
            if entry[..16] == [0u8; 16]
                || i > 0 && page[(i - 1) * f.stride..(i - 1) * f.stride + 16] >= entry[..16]
                || i == 0 && p > 0 && self.key(p - 1) >= &entry[..16]
            {
                return Err(invalid());
            }
            if entry[..16] == needle {
                let mut span = Span {
                    object: object.into(),
                    bytes: u32::from_be_bytes(entry[16..20].try_into().map_err(|_| invalid())?)
                        as i64,
                    offset: 0,
                };
                if f.offset_bytes == 0 {
                    span.object = key.into();
                } else {
                    span.offset = u32::from_be_bytes(
                        entry[entry.len() - 4..].try_into().map_err(|_| invalid())?,
                    ) as i64;
                    if f.offset_bytes > 4 {
                        let ordinal = if f.offset_bytes == 6 {
                            u16::from_be_bytes(entry[20..22].try_into().map_err(|_| invalid())?)
                                as usize
                        } else {
                            entry[20] as usize
                        };
                        span.object = archives.get(ordinal).ok_or_else(invalid)?.clone();
                    }
                }
                if span.bytes < 8 || !catalog::valid_key(&span.object) {
                    return Err(invalid());
                }
                found = Some(span);
            }
        }
        if count == 0 || &page[(count - 1) * f.stride..(count - 1) * f.stride + 16] != self.key(p) {
            return Err(invalid());
        }
        for bytes in page[count * f.stride..].chunks(4096) {
            control.check()?;
            if bytes.iter().any(|b| *b != 0) {
                return Err(invalid());
            }
        }
        found.ok_or_else(missing)
    }
}
#[cfg(test)]
mod tests;
