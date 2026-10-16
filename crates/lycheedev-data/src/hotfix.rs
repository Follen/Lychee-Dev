// SPDX-License-Identifier: MIT
// XFTH layout contracts ported from Lychee Dev's MIT wowdata-derived reader.
use crate::{
    fault,
    resource::{Budget, Cost, RetainedReservation},
    schema,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
mod local;
pub mod remote;
mod scan;
pub(crate) fn dispatch(
    inv: &lycheedev_core::Invocation,
) -> lycheedev_core::Result<lycheedev_core::Outcome> {
    if inv.flag_bool("--scan") {
        scan::dispatch(inv)
    } else {
        local::dispatch(inv)
    }
}
// The CLI contract accepts exactly eight unprefixed hexadecimal digits.
// Keep this separate from decimal record/region/index parsing.
pub(super) fn parse_table_hash(value: &str) -> lycheedev_core::Result<u32> {
    if value.len() != 8 {
        return Err(lycheedev_core::ToolkitError::arguments(
            "--table-hash requires eight hexadecimal digits",
        ));
    }
    if !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(lycheedev_core::ToolkitError::arguments(
            "invalid --table-hash",
        ));
    }
    u32::from_str_radix(value, 16)
        .map_err(|_| lycheedev_core::ToolkitError::arguments("invalid --table-hash"))
}
use lycheedev_core::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CacheFilter {
    pub build: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_hash: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u8>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub latest: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_index: Option<u32>,
    pub limit: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<u32>,
    pub push: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unique_id: Option<u32>,
    pub table_hash: u32,
    pub record_id: u32,
    pub status: u8,
    pub payload_offset: u64,
    pub payload_bytes: usize,
    pub payload_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub decode_state: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePage {
    pub version: u32,
    pub record_layout: u32,
    pub filter: CacheFilter,
    pub entries: Vec<CacheEntry>,
    pub scanned: u32,
    pub matched: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_push: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_index: Option<u32>,
    pub complete: bool,
    pub truncated: bool,
}
fn malformed<T>() -> Result<T> {
    Err(fault(
        "records.hotfix_format",
        4,
        "Invalid complete XFTH framing",
    ))
}
fn u32le(raw: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(raw[offset..offset + 4].try_into().unwrap())
}
pub(crate) fn walk<F: FnMut(CacheEntry) -> Result<()>>(
    raw: &[u8],
    start: usize,
    layout: u32,
    budget: Option<&Budget>,
    yield_entry: F,
) -> Result<()> {
    walk_with_check(raw, start, layout, budget, || Ok(()), yield_entry)
}
pub(crate) fn walk_with_check<F: FnMut(CacheEntry) -> Result<()>>(
    raw: &[u8],
    start: usize,
    layout: u32,
    budget: Option<&Budget>,
    mut supplemental: impl FnMut() -> Result<()>,
    mut yield_entry: F,
) -> Result<()> {
    let width = match layout {
        1 | 7 => 24,
        9 => 32,
        _ => 28,
    };
    let mut offset = start;
    let mut index = 0;
    while offset < raw.len() {
        supplemental()?;
        charge(budget, Cost::work(1))?;
        if index >= 8_000_000 {
            return Err(fault("records.hotfix_limit", 3, "XFTH record limit"));
        }
        if raw.len() - offset < width || &raw[offset..offset + 4] != b"XFTH" {
            return malformed();
        }
        let h = &raw[offset..offset + width];
        let (region, push, unique, table, id, size, status) = match layout {
            1 => (
                None,
                u32le(h, 4) as i32,
                None,
                u32le(h, 12),
                u32le(h, 16),
                u32le(h, 8),
                h[20],
            ),
            2..=6 => (
                None,
                u32le(h, 8) as i32,
                None,
                u32le(h, 16),
                u32le(h, 20),
                u32le(h, 12),
                h[24],
            ),
            7 => (
                None,
                u32le(h, 4) as i32,
                None,
                u32le(h, 8),
                u32le(h, 12),
                u32le(h, 16),
                h[20],
            ),
            8 => (
                None,
                u32le(h, 4) as i32,
                Some(u32le(h, 8)),
                u32le(h, 12),
                u32le(h, 16),
                u32le(h, 20),
                h[24],
            ),
            9 => (
                Some(u32le(h, 4)),
                u32le(h, 8) as i32,
                Some(u32le(h, 12)),
                u32le(h, 16),
                u32le(h, 20),
                u32le(h, 24),
                h[28],
            ),
            _ => return malformed(),
        };
        if size > 0x7fff_ffff || size as usize > raw.len() - offset - width {
            return malformed();
        }
        yield_entry(CacheEntry {
            index,
            region,
            push,
            unique_id: unique,
            table_hash: table,
            record_id: id,
            status,
            payload_offset: (offset + width) as u64,
            payload_bytes: size as usize,
            payload_hex: String::new(),
            fields: None,
            decode_state: String::new(),
        })?;
        index += 1;
        offset += width + size as usize;
    }
    Ok(())
}
fn matches(f: &CacheFilter, e: &CacheEntry) -> bool {
    f.table_hash.is_none_or(|v| e.table_hash == v)
        && f.record_id.is_none_or(|v| e.record_id == v)
        && f.push.is_none_or(|v| e.push == v)
        && f.region.is_none_or(|v| e.region == Some(v))
        && f.status.is_none_or(|v| e.status == v)
}
pub fn inspect(raw: &[u8], filter: CacheFilter) -> Result<CachePage> {
    inspect_budgeted(raw, filter, None)
}
pub fn inspect_budgeted(
    raw: &[u8],
    filter: CacheFilter,
    budget: Option<&Budget>,
) -> Result<CachePage> {
    check(budget)?;
    if !(1..=200).contains(&filter.limit) || raw.len() > 512 << 20 {
        return Err(fault("records.hotfix_limit", 3, "XFTH input/page budget"));
    }
    let (version, layout, start) = cache_layout(raw, filter.build, budget)?;
    if filter.region.is_some() && layout < 9 {
        return Err(fault(
            "records.hotfix_filter_unsupported",
            2,
            "XFTH layout carries no region",
        ));
    }
    charge(
        budget,
        Cost::retained(filter.limit * size_of::<CacheEntry>()),
    )?;
    let mut page = CachePage {
        version,
        record_layout: layout,
        filter: filter.clone(),
        entries: Vec::with_capacity(filter.limit),
        scanned: 0,
        matched: 0,
        selected_push: None,
        next_index: None,
        complete: false,
        truncated: false,
    };
    if filter.latest {
        walk(raw, start, layout, budget, |e| {
            page.scanned += 1;
            if matches(&filter, &e) && page.selected_push.is_none_or(|p| e.push > p) {
                page.selected_push = Some(e.push);
            }
            Ok(())
        })?;
        if page.selected_push.is_none() {
            page.complete = filter.after_index.is_none();
            return Ok(page);
        }
    }
    let mut payload_bytes = 0;
    walk(raw, start, layout, budget, |mut e| {
        if !filter.latest {
            page.scanned += 1;
        }
        if !matches(&filter, &e) || (filter.latest && Some(e.push) != page.selected_push) {
            return Ok(());
        }
        page.matched += 1;
        if filter.after_index.is_some_and(|a| e.index <= a) {
            return Ok(());
        }
        if page.entries.len() == filter.limit {
            page.truncated = true;
            return Ok(());
        }
        if e.payload_bytes > (8 << 20) - payload_bytes {
            return Err(fault("records.hotfix_limit", 3, "XFTH page payload budget"));
        }
        payload_bytes += e.payload_bytes;
        let bytes = &raw[e.payload_offset as usize..e.payload_offset as usize + e.payload_bytes];
        charge(budget, Cost::retained(bytes.len() * 2))?;
        e.payload_hex = String::with_capacity(bytes.len() * 2);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for chunk in bytes.chunks(65536) {
            charge(budget, Cost::work(chunk.len()))?;
            for &byte in chunk {
                e.payload_hex.push(HEX[(byte >> 4) as usize] as char);
                e.payload_hex.push(HEX[(byte & 15) as usize] as char);
            }
        }
        page.entries.push(e);
        Ok(())
    })?;
    if page.truncated {
        page.next_index = page.entries.last().map(|e| e.index);
    }
    page.complete = filter.after_index.is_none() && !page.truncated;
    Ok(page)
}

/// Shared framing validation for page inspection and immutable SQL snapshots.
/// Version 8 is accepted only when exactly one historical record layout fits.
pub(crate) fn cache_layout(
    raw: &[u8],
    expected_build: u32,
    budget: Option<&Budget>,
) -> Result<(u32, u32, usize)> {
    cache_layout_with_check(raw, expected_build, budget, || Ok(()))
}
pub(crate) fn cache_layout_with_check(
    raw: &[u8],
    expected_build: u32,
    budget: Option<&Budget>,
    mut supplemental: impl FnMut() -> Result<()>,
) -> Result<(u32, u32, usize)> {
    supplemental()?;
    check(budget)?;
    if raw.len() < 12 || &raw[..4] != b"XFTH" {
        return malformed();
    }
    let version = u32le(raw, 4);
    let build = u32le(raw, 8);
    if !(1..=9).contains(&version) {
        return Err(fault(
            "records.hotfix_version",
            3,
            "Unsupported XFTH version",
        ));
    }
    if build == 0 || build > 0x7fff_ffff || build != expected_build {
        return Err(fault(
            "records.hotfix_build_mismatch",
            4,
            "XFTH build differs from pin",
        ));
    }
    let start = if version >= 5 { 44 } else { 12 };
    if raw.len() < start {
        return malformed();
    }
    let mut layout = version;
    if version == 8 && raw.len() > start {
        let a = walk_with_check(raw, start, 8, budget, &mut supplemental, |_| Ok(()));
        let b = walk_with_check(raw, start, 7, budget, &mut supplemental, |_| Ok(()));
        for result in [&a, &b] {
            if let Err(error) = result {
                if error.code != "records.hotfix_format" {
                    return Err(error.clone());
                }
            }
        }
        match (a.is_ok(), b.is_ok()) {
            (true, false) => {}
            (false, true) => layout = 7,
            _ => return malformed(),
        }
    }
    Ok((version, layout, start))
}

fn check(budget: Option<&Budget>) -> Result<()> {
    budget.map_or(Ok(()), Budget::check)
}
fn charge(budget: Option<&Budget>, cost: Cost) -> Result<()> {
    budget.map_or(Ok(()), |budget| budget.charge(cost))
}
fn fields_error() -> lycheedev_core::ToolkitError {
    fault(
        "records.hotfix_fields",
        4,
        "Invalid sequential hotfix fields",
    )
}
fn limit_error() -> lycheedev_core::ToolkitError {
    fault("records.hotfix_limit", 3, "Hotfix field or payload limit")
}

// Rust 1.99 alloc::collections::btree: each non-root node has >=5
// entries; nodes have 11 key/value slots and <=12 child pointers. The
// 16 slots +128 bytes bound includes headers/alignment and spare nodes
// during splitting. Charge nodes once, not an entire node per field.
fn tree_bytes<K, V>(entries: usize) -> usize {
    (entries.div_ceil(5) + 2) * (16 * size_of::<(K, V)>() + 128)
}

/// Fixed Go cache_fields.go: XFTH sequential values, never WDC bitpacking.
/// Every byte must be consumed; failure returns no partial field map.
pub fn decode_fields(
    raw: &[u8],
    record_id: u32,
    definition: &schema::Definition,
    budget: Option<&Budget>,
) -> Result<BTreeMap<String, Value>> {
    let mut decoded = decode_fields_scoped(raw, record_id, definition, budget)?;
    if let Some(allocation) = decoded.allocation.take() {
        allocation.commit();
    }
    Ok(decoded.fields)
}

/// Own the typed map and its refundable allocation in the same object. Field
/// declaration order drops the actual map before releasing its accounting.
pub(crate) struct DecodedFields<'a> {
    fields: BTreeMap<String, Value>,
    allocation: Option<RetainedReservation<'a>>,
}
impl DecodedFields<'_> {
    pub(crate) fn fields(&self) -> &BTreeMap<String, Value> {
        &self.fields
    }
}
fn retain_fields(allocation: &mut Option<RetainedReservation<'_>>, bytes: usize) -> Result<()> {
    if let Some(allocation) = allocation {
        allocation.grow(bytes)?;
    }
    Ok(())
}
pub(crate) fn decode_fields_scoped<'a>(
    raw: &[u8],
    record_id: u32,
    definition: &schema::Definition,
    budget: Option<&'a Budget>,
) -> Result<DecodedFields<'a>> {
    check(budget)?;
    if raw.len() > 8 << 20 || definition.fields.len() > 4096 {
        return Err(limit_error());
    }
    if definition.fields.is_empty() {
        return Err(fields_error());
    }
    // Duplicate-name validation borrows names and is temporary; its tree
    // allocation must not accumulate as retained memory across page records.
    let names_allocation = budget
        .map(|budget| budget.reserve_retained(tree_bytes::<&str, ()>(definition.fields.len())))
        .transpose()?;
    let mut names = BTreeSet::new();
    let mut cells = 0u64;
    let mut identities = 0;
    for field in &definition.fields {
        charge(budget, Cost::work(field.name.len().saturating_add(1)))?;
        if field.name.is_empty()
            || !names.insert(field.name.as_str())
            || field.elements == 0
            || (!field.array && field.elements != 1)
        {
            return Err(fields_error());
        }
        cells += u64::from(field.elements);
        if cells > 65536 {
            return Err(limit_error());
        }
        match field.kind.as_str() {
            "int" if matches!(field.bits, 8 | 16 | 32 | 64) => {}
            "float" if field.bits == 32 => {}
            "string" | "locstring" => {}
            _ => return Err(fields_error()),
        }
        if field.identity {
            identities += 1;
            if identities > 1 || field.array || field.kind != "int" {
                return Err(fields_error());
            }
        }
        if !field.inline
            && (field.kind != "int" || field.array || (!field.identity && !field.relation))
        {
            return Err(fields_error());
        }
    }
    drop(names);
    drop(names_allocation);
    let node_bytes = tree_bytes::<String, Value>(definition.fields.len());
    let mut map_allocation = budget
        .map(|budget| budget.reserve_retained(node_bytes))
        .transpose()?;
    let mut result = BTreeMap::new();
    let mut offset = 0usize;
    let mut text_remaining = 1usize << 20;
    for field in &definition.fields {
        retain_fields(&mut map_allocation, field.name.len())?;
        let value = if field.identity && !field.inline {
            if field.signed {
                let value = i64::from(record_id as i32);
                if field.bits < 64
                    && (value < -(1i64 << (field.bits - 1))
                        || value > (1i64 << (field.bits - 1)) - 1)
                {
                    return Err(fields_error());
                }
                Value::from(value)
            } else {
                if field.bits < 64 && u64::from(record_id) >= 1u64 << field.bits {
                    return Err(fields_error());
                }
                Value::from(record_id)
            }
        } else {
            let _values_allocation = if field.array {
                retain_fields(
                    &mut map_allocation,
                    field.elements as usize * size_of::<Value>(),
                )?;
                None
            } else {
                budget
                    .map(|budget| budget.reserve_retained(size_of::<Value>()))
                    .transpose()?
            };
            let mut values = Vec::with_capacity(field.elements as usize);
            for _ in 0..field.elements {
                charge(budget, Cost::work(1))?;
                let value = if matches!(field.kind.as_str(), "string" | "locstring") {
                    let available = &raw[offset..];
                    let bound = available.len().min(text_remaining + 1);
                    let mut end = None;
                    for (index, chunk) in available[..bound].chunks(65536).enumerate() {
                        charge(budget, Cost::work(chunk.len()))?;
                        if let Some(found) = chunk.iter().position(|&byte| byte == 0) {
                            end = Some(index * 65536 + found);
                            break;
                        }
                    }
                    let end = end.ok_or_else(|| {
                        if available.len() > text_remaining {
                            limit_error()
                        } else {
                            fields_error()
                        }
                    })?;
                    let value =
                        std::str::from_utf8(&available[..end]).map_err(|_| fields_error())?;
                    retain_fields(&mut map_allocation, end)?;
                    offset += end + 1;
                    text_remaining -= end;
                    Value::String(value.to_owned())
                } else {
                    let width = usize::from(field.bits / 8);
                    let part = raw.get(offset..offset + width).ok_or_else(fields_error)?;
                    let mut value = 0u64;
                    for (index, &byte) in part.iter().enumerate() {
                        value |= u64::from(byte) << (8 * index);
                    }
                    offset += width;
                    if field.kind == "float" {
                        let value = f64::from(f32::from_bits(value as u32));
                        if !value.is_finite() {
                            return Err(fields_error());
                        }
                        Value::from(value)
                    } else if field.signed {
                        let shift = 64 - field.bits;
                        Value::from(((value << shift) as i64) >> shift)
                    } else {
                        Value::from(value)
                    }
                };
                if field.identity
                    && ((field.signed && value.as_i64() != Some(i64::from(record_id as i32)))
                        || (!field.signed && value.as_u64() != Some(u64::from(record_id))))
                {
                    return Err(fields_error());
                }
                values.push(value);
            }
            if field.array {
                Value::Array(values)
            } else {
                values.pop().ok_or_else(fields_error)?
            }
        };
        result.insert(field.name.clone(), value);
    }
    if offset != raw.len() {
        return Err(fields_error());
    }
    check(budget)?;
    Ok(DecodedFields {
        fields: result,
        allocation: map_allocation,
    })
}

// encoding/json uses shortest float64 digits, omits an integral .0, and
// switches to scientific form outside [1e-6, 1e21). Payload float32 values
// have already been promoted exactly to float64, as in Go's decoder.
struct GoFieldsFormatter;
impl serde_json::ser::Formatter for GoFieldsFormatter {
    fn write_number_str<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        value: &str,
    ) -> std::io::Result<()> {
        // With serde_json's workspace-wide arbitrary_precision feature, even
        // Number::from_f64 serializes through this hook, not write_f64. Only
        // floating-point lexemes may make the binary64 round trip: converting
        // integer fields here would round values such as u64::MAX.
        if value.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E')) {
            let number = value
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Hotfix float is not finite binary64",
                    )
                })?;
            self.write_f64(writer, number)
        } else {
            writer.write_all(value.as_bytes())
        }
    }
    fn write_f64<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        value: f64,
    ) -> std::io::Result<()> {
        let absolute = value.abs();
        if absolute != 0.0 && !(1e-6..1e21).contains(&absolute) {
            let text = format!("{value:e}");
            let (mantissa, exponent) = text.split_once('e').expect("scientific float");
            writer.write_all(mantissa.as_bytes())?;
            writer.write_all(b"e")?;
            if !exponent.starts_with('-') {
                writer.write_all(b"+")?;
            }
            writer.write_all(exponent.as_bytes())
        } else {
            write!(writer, "{value}")
        }
    }
    fn write_string_fragment<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> std::io::Result<()> {
        serde_json::ser::Formatter::write_string_fragment(
            &mut crate::capture::GoCaptureFormatter,
            writer,
            fragment,
        )
    }
}

// Go's fields budget counts encoding/json's escaped representation, including
// HTML and U+2028/U+2029. Stream-count it without allocating a second field map.
pub(crate) fn fields_json_size(fields: &BTreeMap<String, Value>, budget: &Budget) -> Result<usize> {
    struct Counter<'a> {
        budget: &'a Budget,
        bytes: usize,
        error: Option<lycheedev_core::ToolkitError>,
    }
    impl std::io::Write for Counter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Err(error) = self
                .budget
                .charge(Cost::work(bytes.len().saturating_add(1)))
            {
                self.error = Some(error);
                return Err(std::io::Error::other("hotfix JSON accounting stopped"));
            }
            self.bytes += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Counter {
        budget,
        bytes: 0,
        error: None,
    };
    let result = fields.serialize(&mut serde_json::Serializer::with_formatter(
        &mut count,
        GoFieldsFormatter,
    ));
    if let Err(error) = result {
        return Err(count.error.unwrap_or_else(|| error.into()));
    }
    Ok(count.bytes)
}
pub fn decode_page(
    raw: &[u8],
    mut page: CachePage,
    definition: &schema::Definition,
    budget: &Budget,
) -> Result<CachePage> {
    let mut fields_bytes = 0usize;
    // Only publish the staged page after every selected payload validates.
    for entry in &mut page.entries {
        budget.check()?;
        entry.fields = None;
        entry.decode_state = if entry.payload_bytes == 0 {
            "no_payload"
        } else if (page.record_layout >= 7 && entry.status != 1)
            || (page.record_layout < 7 && entry.status == 0)
        {
            "not_valid"
        } else {
            "decoded"
        }
        .into();
        if entry.decode_state != "decoded" {
            continue;
        }
        let start = usize::try_from(entry.payload_offset).map_err(|_| fields_error())?;
        let end = start
            .checked_add(entry.payload_bytes)
            .ok_or_else(fields_error)?;
        let fields = decode_fields(
            raw.get(start..end).ok_or_else(fields_error)?,
            entry.record_id,
            definition,
            Some(budget),
        )?;
        let bytes = fields_json_size(&fields, budget)?;
        if bytes > (8 << 20) - fields_bytes {
            return Err(limit_error());
        }
        fields_bytes += bytes;
        entry.fields = Some(fields);
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(version: u32, pushes: &[i32]) -> Vec<u8> {
        let mut b = b"XFTH".to_vec();
        b.extend(version.to_le_bytes());
        b.extend(69875u32.to_le_bytes());
        if version >= 5 {
            b.resize(44, 0);
        }
        for (i, p) in pushes.iter().enumerate() {
            let width = match version {
                1 | 7 => 24,
                9 => 32,
                _ => 28,
            };
            let mut h = vec![0; width];
            h[..4].copy_from_slice(b"XFTH");
            let (po, to, io, so, st) = match version {
                1 => (4, 12, 16, 8, 20),
                2..=6 => (8, 16, 20, 12, 24),
                7 => (4, 8, 12, 16, 20),
                8 => (4, 12, 16, 20, 24),
                _ => (8, 16, 20, 24, 28),
            };
            h[po..po + 4].copy_from_slice(&p.to_le_bytes());
            h[to..to + 4].copy_from_slice(&42u32.to_le_bytes());
            h[io..io + 4].copy_from_slice(&7u32.to_le_bytes());
            h[so..so + 4].copy_from_slice(&1u32.to_le_bytes());
            h[st] = (i % 3) as u8;
            if version == 9 {
                h[4..8].copy_from_slice(&3u32.to_le_bytes());
            }
            b.extend(h);
            b.push(i as u8);
        }
        b
    }
    #[test]
    fn all_layouts_and_complete_tail_validation() {
        for version in 1..=9 {
            let mut b = fixture(version, &[-2, 7, 7]);
            let f = CacheFilter {
                build: 69875,
                limit: 1,
                latest: true,
                ..Default::default()
            };
            let p = inspect(&b, f.clone()).unwrap();
            assert_eq!(p.matched, 2);
            assert_eq!(p.selected_push, Some(7));
            assert_eq!(p.next_index, Some(1));
            assert!(!p.complete);
            b.push(0);
            assert_eq!(inspect(&b, f).unwrap_err().code, "records.hotfix_format");
        }
    }
    #[test]
    fn repeated_ids_status_and_pagination() {
        let b = fixture(9, &[-7, -1, -1]);
        let mut f = CacheFilter {
            build: 69875,
            limit: 2,
            ..Default::default()
        };
        let p = inspect(&b, f.clone()).unwrap();
        assert_eq!(p.entries.len(), 2);
        f.after_index = p.next_index;
        let p = inspect(&b, f).unwrap();
        assert_eq!(p.entries[0].index, 2);
        assert!(!p.complete);
        assert!(!p.truncated);
    }
    #[test]
    fn v8_legacy_layout() {
        let mut b = fixture(7, &[1, 2]);
        b[4..8].copy_from_slice(&8u32.to_le_bytes());
        assert_eq!(
            inspect(
                &b,
                CacheFilter {
                    build: 69875,
                    limit: 20,
                    ..Default::default()
                }
            )
            .unwrap()
            .record_layout,
            7
        );
    }
}

#[cfg(test)]
mod fields_number_tests {
    use super::*;
    #[test]
    fn temporary_fields_release_map_strings_arrays_and_preserve_cumulative_work() {
        let definition = schema::Document::parse(
            b"COLUMNS\nstring Name\nint Values\n\nBUILD 12.1.0.69875\nName\nValues<u32>[2]\n",
        )
        .unwrap()
        .select("12.1.0.69875", "")
        .unwrap();
        let mut raw = b"hello\0".to_vec();
        raw.extend(1u32.to_le_bytes());
        raw.extend(2u32.to_le_bytes());
        let budget = Budget::new(
            Cost::defaults(),
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            None,
        );
        let unrelated = budget.reserve_retained(17).unwrap();
        budget
            .charge(Cost {
                metadata_bytes: 23,
                ..Default::default()
            })
            .unwrap();
        let decoded = decode_fields_scoped(&raw, 0, &definition, Some(&budget)).unwrap();
        assert_eq!(decoded.fields()["Values"], serde_json::json!([1, 2]));
        let held = budget.snapshot().unwrap();
        assert!(held.retained_bytes > 17);
        drop(decoded);
        let freed = budget.snapshot().unwrap();
        assert_eq!(freed.retained_bytes, 17);
        assert_eq!(freed.metadata_bytes, 23);
        assert_eq!(freed.decode_work, held.decode_work);
        raw.push(0);
        assert!(decode_fields_scoped(&raw, 0, &definition, Some(&budget)).is_err());
        assert_eq!(
            budget.snapshot().unwrap().retained_bytes,
            17,
            "Error also releases only the decoded temporary owner"
        );
        drop(unrelated);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }

    #[test]
    fn scalar_decode_retains_only_result_tree_and_owned_names() {
        let definition =
            schema::Document::parse(b"COLUMNS\nint A\nint B\n\nBUILD 12.1.0.69875\nA<u8>\nB<u8>\n")
                .unwrap()
                .select("12.1.0.69875", "")
                .unwrap();
        let budget = Budget::new(
            Cost::defaults(),
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            None,
        );
        let fields = decode_fields(&[1, 2], 0, &definition, Some(&budget)).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(
            budget.snapshot().unwrap().retained_bytes,
            (tree_bytes::<String, Value>(2) + 2) as u64
        );
    }

    #[test]
    fn go_json_float_digits_and_exponent_thresholds() {
        for (number, expected) in [
            (0.0, "0"),
            (1.0, "1"),
            (-0.0, "-0"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (f64::from(0.1f32), "0.10000000149011612"),
            (f64::from(1e-6f32), "9.999999974752427e-7"),
            (f64::from(1e20f32), "100000002004087730000"),
            (f64::from(f32::from_bits(1)), "1.401298464324817e-45"),
            (f64::from(f32::MAX), "3.4028234663852886e+38"),
        ] {
            let mut bytes = Vec::new();
            Value::from(number)
                .serialize(&mut serde_json::Serializer::with_formatter(
                    &mut bytes,
                    GoFieldsFormatter,
                ))
                .unwrap();
            assert_eq!(std::str::from_utf8(&bytes).unwrap(), expected);
            // Typed f64 and Value::Number use different serializer hooks when
            // arbitrary_precision is enabled; both must keep Go's spelling.
            bytes.clear();
            number
                .serialize(&mut serde_json::Serializer::with_formatter(
                    &mut bytes,
                    GoFieldsFormatter,
                ))
                .unwrap();
            assert_eq!(std::str::from_utf8(&bytes).unwrap(), expected);
        }
    }

    #[test]
    fn go_fields_count_matches_bounded_bytes_without_rounding_integers() {
        let fields = BTreeMap::from([
            (
                "Array".into(),
                Value::Array(vec![
                    Value::from(-0.0),
                    Value::from(1.0),
                    Value::from(f64::from(0.1f32)),
                    Value::from(1e21),
                ]),
            ),
            ("Signed".into(), Value::from(i64::MIN)),
            ("Text".into(), Value::from("<>&\u{2028}\u{2029}\"\\\0")),
            ("Unsigned".into(), Value::from(u64::MAX)),
            ("Wide".into(), Value::from(9_007_199_254_740_993u64)),
        ]);
        let expected = br#"{"Array":[-0,1,0.10000000149011612,1e+21],"Signed":-9223372036854775808,"Text":"\u003c\u003e\u0026\u2028\u2029\"\\\u0000","Unsigned":18446744073709551615,"Wide":9007199254740993}"#;
        let budget = Budget::new(
            Cost::defaults(),
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            None,
        );
        assert_eq!(fields_json_size(&fields, &budget).unwrap(), expected.len());
        assert_eq!(
            crate::capture::json_bytes_with_formatter(
                &fields,
                &budget,
                expected.len(),
                limit_error,
                GoFieldsFormatter,
            )
            .unwrap(),
            expected
        );
        assert_eq!(
            crate::capture::json_bytes_with_formatter(
                &fields,
                &budget,
                expected.len() - 1,
                limit_error,
                GoFieldsFormatter,
            )
            .unwrap_err()
            .code,
            "records.hotfix_limit"
        );
        // The optional formatter must not change unrelated capture producers.
        assert_eq!(
            crate::capture::json_bytes(&fields, &budget, 4096, limit_error).unwrap(),
            serde_json::to_vec(&fields).unwrap()
        );
    }
}

#[cfg(test)]
#[path = "hotfix/snapshot_control_tests.rs"]
mod snapshot_control_tests;
