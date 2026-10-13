// SPDX-License-Identifier: MIT
// Encoding/Root layouts ported from the MIT Lychee Dev records reader.
//! Lazy checked Encoding pages and bounded Root projections. Parsers only
//! establish structure; callers authenticate the enclosing CASC content first.
use crate::{
    blte::{ByteSource, Content},
    casc::{self, Limits},
    fault,
};
use lycheedev_core::Result;
use md5::{Digest, Md5};
use serde::Serialize;
use std::{
    borrow::Cow,
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
fn invalid() -> lycheedev_core::ToolkitError {
    fault(
        "records.metadata_invalid",
        4,
        "Invalid Encoding/Root metadata",
    )
}
fn exceeded() -> lycheedev_core::ToolkitError {
    fault("records.metadata_limit", 3, "Metadata budget exceeded")
}
fn check(limits: Limits, cancel: Option<&AtomicBool>) -> Result<()> {
    if cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
        return Err(fault("operation.cancelled", 7, "Metadata query cancelled"));
    }
    if Instant::now() >= limits.deadline {
        return Err(exceeded());
    }
    Ok(())
}
fn be16(raw: &[u8]) -> usize {
    u16::from_be_bytes(raw[..2].try_into().unwrap()) as usize
}
fn be32(raw: &[u8]) -> usize {
    u32::from_be_bytes(raw[..4].try_into().unwrap()) as usize
}
fn le32(raw: &[u8]) -> u32 {
    u32::from_le_bytes(raw[..4].try_into().unwrap())
}
fn product(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(exceeded)
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EncodingRecord {
    pub content_key: String,
    pub decoded_bytes: u64,
    pub encoding_keys: Vec<String>,
    pub page: usize,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EncodedRecord {
    pub encoding_key: String,
    pub encoded_bytes: u64,
    pub specification: u32,
}
pub struct Encoding<'a> {
    content: &'a dyn ByteSource,
    directory: Cow<'a, [u8]>,
    page_size: usize,
    pages_start: usize,
    encoded_directory_start: usize,
    encoded_page_count: usize,
    encoded_page_size: usize,
    encoded_pages_start: usize,
}
impl<'a> Encoding<'a> {
    pub fn open(
        content: &'a dyn ByteSource,
        limits: Limits,
        cancel: Option<&AtomicBool>,
    ) -> Result<Self> {
        check(limits, cancel)?;
        let h = content.read_span(0, 22)?;
        if &h[..2] != b"EN" || h[2..5] != [1, 16, 16] || h[17] != 0 {
            return Err(invalid());
        }
        let page_size = be16(&h[5..]) * 1024;
        let encoded_size = be16(&h[7..]) * 1024;
        let pages = be32(&h[9..]);
        let encoded_pages = be32(&h[13..]);
        let spec = be32(&h[18..]);
        if page_size == 0
            || page_size > 1 << 20
            || pages > 1 << 20
            || spec > 64 << 20
            || encoded_pages > 1 << 20
            || encoded_size > 1 << 20
            || (encoded_pages > 0 && encoded_size == 0)
        {
            return Err(exceeded());
        }
        let directory_start = 22usize.checked_add(spec).ok_or_else(exceeded)?;
        let directory_bytes = product(pages, 32)?;
        let pages_start = directory_start
            .checked_add(directory_bytes)
            .ok_or_else(exceeded)?;
        let encoded_start = pages_start
            .checked_add(product(pages, page_size)?)
            .ok_or_else(exceeded)?;
        let end = encoded_start
            .checked_add(product(encoded_pages, 32 + encoded_size)?)
            .ok_or_else(exceeded)?;
        if end > content.size() {
            return Err(invalid());
        }
        if directory_bytes
            .checked_add(product(encoded_pages, 32)?)
            .is_none_or(|n| n > limits.index_bytes)
        {
            return Err(exceeded());
        }
        let directory = content.read_span(directory_start, directory_bytes)?;
        let mut previous: Option<&[u8]> = None;
        for (i, entry) in directory.as_chunks::<32>().0.iter().enumerate() {
            if i.is_multiple_of(1024) {
                check(limits, cancel)?;
            }
            if previous.is_some_and(|p| p >= &entry[..16]) {
                return Err(invalid());
            }
            if entry[16..].iter().all(|b| *b == 0) {
                return Err(fault(
                    "container.integrity_mismatch",
                    4,
                    "Encoding page digest omitted",
                ));
            }
            previous = Some(&entry[..16]);
        }
        Ok(Self {
            content,
            directory,
            page_size,
            pages_start,
            encoded_directory_start: encoded_start,
            encoded_page_count: encoded_pages,
            encoded_page_size: encoded_size,
            encoded_pages_start: encoded_start + encoded_pages * 32,
        })
    }
    /// Resolve the physical size from the independently authenticated EKey
    /// page; CKey logical sizes cannot substitute for this archive extent.
    pub fn find_encoding(
        &self,
        key: &str,
        limits: Limits,
        cancel: Option<&AtomicBool>,
    ) -> Result<EncodedRecord> {
        check(limits, cancel)?;
        let encoded_directory = self.content.read_span(
            self.encoded_directory_start,
            product(self.encoded_page_count, 32)?,
        )?;
        let mut previous: Option<&[u8]> = None;
        for (i, entry) in encoded_directory.as_chunks::<32>().0.iter().enumerate() {
            if i.is_multiple_of(1024) {
                check(limits, cancel)?;
            }
            if previous.is_some_and(|p| p >= &entry[..16]) {
                return Err(invalid());
            }
            if entry[16..].iter().all(|b| *b == 0) {
                return Err(fault(
                    "container.integrity_mismatch",
                    4,
                    "Encoding physical page digest omitted",
                ));
            }
            previous = Some(&entry[..16]);
        }
        let requested = casc::key(key)?;
        let pages = encoded_directory.len() / 32;
        let (mut low, mut high) = (0, pages);
        while low < high {
            let mid = low + (high - low) / 2;
            if encoded_directory[mid * 32..mid * 32 + 16] <= requested[..] {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let absent = || {
            fault(
                "records.encoding_not_found",
                3,
                "Encoding key not found in verified metadata",
            )
        };
        let page = low.checked_sub(1).ok_or_else(absent)?;
        let entry = &encoded_directory[page * 32..page * 32 + 32];
        let raw = self.content.read_checked_span(
            self.encoded_pages_start + page * self.encoded_page_size,
            self.encoded_page_size,
            &entry[16..],
        )?;
        if Md5::digest(&raw).as_slice() != &entry[16..] {
            return Err(fault(
                "container.integrity_mismatch",
                4,
                "Encoding physical page MD5 differs",
            ));
        }
        let mut pos = 0;
        let mut previous: Option<&[u8]> = None;
        let mut result = None;
        while pos < raw.len() {
            if raw[pos..].iter().all(|b| *b == 0) {
                break;
            }
            if raw.len() - pos < 25 {
                return Err(invalid());
            }
            let current = &raw[pos..pos + 16];
            if previous.is_none() && current != &entry[..16]
                || previous.is_some_and(|p| p >= current)
                || page + 1 < pages
                    && current >= &encoded_directory[(page + 1) * 32..(page + 1) * 32 + 16]
            {
                return Err(invalid());
            }
            let bytes = ((raw[pos + 20] as u64) << 32) | be32(&raw[pos + 21..]) as u64;
            if bytes < 8 {
                return Err(invalid());
            }
            if current == requested {
                result = Some(EncodedRecord {
                    encoding_key: key.into(),
                    encoded_bytes: bytes,
                    specification: be32(&raw[pos + 16..]) as u32,
                });
            }
            previous = Some(current);
            pos += 25;
        }
        check(limits, cancel)?;
        if previous.is_none() {
            return Err(invalid());
        }
        result.ok_or_else(absent)
    }
    pub fn find(
        &self,
        key: &str,
        limits: Limits,
        cancel: Option<&AtomicBool>,
    ) -> Result<EncodingRecord> {
        check(limits, cancel)?;
        let requested = casc::key(key)?;
        let pages = self.directory.len() / 32;
        let mut low = 0;
        let mut high = pages;
        while low < high {
            let mid = low + (high - low) / 2;
            if self.directory[mid * 32..mid * 32 + 16] <= requested[..] {
                low = mid + 1
            } else {
                high = mid
            }
        }
        let Some(page) = low.checked_sub(1) else {
            return Err(fault(
                "records.content_not_found",
                3,
                "Content key not found",
            ));
        };
        let entry = &self.directory[page * 32..page * 32 + 32];
        let raw = self.content.read_checked_span(
            self.pages_start + page * self.page_size,
            self.page_size,
            &entry[16..],
        )?;
        if Md5::digest(&raw).as_slice() != &entry[16..] {
            return Err(fault(
                "container.integrity_mismatch",
                4,
                "Encoding page MD5 differs",
            ));
        }
        let mut pos = 0;
        let mut previous: Option<&[u8]> = None;
        let mut result = None;
        while pos < raw.len() {
            let count = raw[pos] as usize;
            pos += 1;
            if count == 0 {
                if raw[pos..].iter().any(|b| *b != 0) {
                    return Err(invalid());
                }
                break;
            }
            let length = 21 + count * 16;
            if length > raw.len() - pos {
                return Err(invalid());
            }
            let current = &raw[pos + 5..pos + 21];
            if previous.is_none() {
                if current != &entry[..16] {
                    return Err(invalid());
                }
            } else if previous.is_some_and(|p| p >= current) {
                return Err(invalid());
            }
            if page + 1 < pages && current >= &self.directory[(page + 1) * 32..(page + 1) * 32 + 16]
            {
                return Err(invalid());
            }
            previous = Some(current);
            if current == requested {
                result = Some(EncodingRecord {
                    content_key: key.into(),
                    decoded_bytes: ((raw[pos] as u64) << 32) | be32(&raw[pos + 1..]) as u64,
                    encoding_keys: raw[pos + 21..pos + length]
                        .as_chunks::<16>()
                        .0
                        .iter()
                        .map(|chunk| casc::hex(chunk))
                        .collect(),
                    page,
                });
            }
            pos += length;
        }
        check(limits, cancel)?;
        if previous.is_none() {
            return Err(invalid());
        }
        result.ok_or_else(|| {
            fault(
                "records.content_not_found",
                3,
                "Content key not found in verified page",
            )
        })
    }
}
#[derive(Clone, Copy)]
pub struct RootLimits {
    pub bytes: usize,
    pub records: u64,
    pub groups: usize,
    pub matches: usize,
    pub operation: Limits,
}
impl Default for RootLimits {
    fn default() -> Self {
        Self {
            bytes: 256 << 20,
            records: 10_000_000,
            groups: 1 << 20,
            matches: 1 << 20,
            operation: Limits::default(),
        }
    }
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RootRecord {
    #[serde(rename = "fileDataID")]
    pub file_data_id: u32,
    pub content_key: String,
    pub locale_mask: u32,
    pub content_flags: u32,
    pub name_hash: u64,
    pub has_name_hash: bool,
    pub group: usize,
}
pub fn lookup_root(
    content: &Content,
    ids: &[u32],
    limits: RootLimits,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<RootRecord>> {
    check(limits.operation, cancel)?;
    if content.size() > limits.bytes
        || limits.bytes == 0
        || limits.records == 0
        || limits.records > 1u64 << 32
        || limits.groups == 0
        || limits.groups > 1 << 20
        || limits.matches == 0
        || limits.matches > 1 << 20
        || ids.is_empty()
        || ids.len() > 4096
    {
        return Err(exceeded());
    }
    let wanted: BTreeSet<_> = ids.iter().copied().collect();
    let first = content.checked_span(0, 4)?;
    let classic = first != b"TSFM";
    let mut version = 0;
    let mut pos = 0;
    let mut allow_nameless = false;
    if !classic {
        let h = content.checked_span(0, 12)?;
        let second = le32(&h[4..]);
        let third = le32(&h[8..]);
        let (mut total, mut named) = (second, third);
        pos = 12;
        if [20, 24].contains(&second) && [1, 2].contains(&third) {
            let h = content.checked_span(0, second as usize)?;
            version = third;
            pos = second as usize;
            total = le32(&h[12..]);
            named = le32(&h[16..]);
        }
        if named > total {
            return Err(invalid());
        }
        allow_nameless = total != named;
    }
    let mut records = 0u64;
    let mut result = vec![];
    let mut group = 0;
    while pos < content.size() {
        check(limits.operation, cancel)?;
        if group >= limits.groups {
            return Err(exceeded());
        }
        let header = if version == 2 { 17 } else { 12 };
        let h = content.checked_span(pos, header)?;
        let count = le32(h) as usize;
        let (mut flags, mut locale) = (le32(&h[4..]), le32(&h[8..]));
        if version == 2 {
            locale = flags;
            flags = le32(&h[8..]) | le32(&h[12..]) | ((h[16] as u32) << 17);
        }
        records = records.checked_add(count as u64).ok_or_else(exceeded)?;
        if records > limits.records {
            return Err(exceeded());
        }
        pos += header;
        let has_name = classic || !(allow_nameless && flags & 0x10000000 != 0);
        let key_stride = if classic { 24 } else { 16 };
        let stride = 20 + if has_name { 8 } else { 0 };
        let extent = product(count, stride)?;
        if extent > content.size() - pos {
            return Err(invalid());
        }
        let keys_start = pos + product(count, 4)?;
        let names_start = keys_start + product(count, key_stride)?;
        let deltas = content.checked_span(pos, product(count, 4)?)?;
        let mut next = 0u64;
        for (ordinal, delta) in deltas.as_chunks::<4>().0.iter().enumerate() {
            if ordinal.is_multiple_of(4096) {
                check(limits.operation, cancel)?;
            }
            let id = next + le32(delta) as u64;
            if id > u32::MAX as u64 {
                return Err(invalid());
            }
            next = id + 1;
            if !wanted.contains(&(id as u32)) {
                continue;
            }
            if result.len() >= limits.matches {
                return Err(exceeded());
            }
            let key = content.checked_span(keys_start + ordinal * key_stride, key_stride)?;
            let name_hash = if has_name {
                let name = if classic {
                    &key[16..24]
                } else {
                    content.checked_span(names_start + ordinal * 8, 8)?
                };
                u64::from_le_bytes(name.try_into().unwrap())
            } else {
                0
            };
            result.push(RootRecord {
                file_data_id: id as u32,
                content_key: casc::hex(&key[..16]),
                locale_mask: locale,
                content_flags: flags,
                name_hash,
                has_name_hash: has_name,
                group,
            });
        }
        pos += extent;
        group += 1;
    }
    check(limits.operation, cancel)?;
    Ok(result)
}
