// SPDX-License-Identifier: MIT
// XFTH layout contracts ported from Lychee Dev's MIT wowdata-derived reader.
use crate::fault;
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
fn walk<F: FnMut(CacheEntry) -> Result<()>>(
    raw: &[u8],
    start: usize,
    layout: u32,
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
    if !(1..=200).contains(&filter.limit) || raw.len() > 512 << 20 {
        return Err(fault("records.hotfix_limit", 3, "XFTH input/page budget"));
    }
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
    if build == 0 || build > 0x7fff_ffff || build != filter.build {
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
        let a = walk(raw, start, 8, |_| Ok(()));
        let b = walk(raw, start, 7, |_| Ok(()));
        if a.as_ref().is_err_and(|e| e.code == "records.hotfix_limit")
            || b.as_ref().is_err_and(|e| e.code == "records.hotfix_limit")
        {
            return Err(fault("records.hotfix_limit", 3, "XFTH record budget"));
        }
        match (a.is_ok(), b.is_ok()) {
            (true, false) => {}
            (false, true) => layout = 7,
            _ => return malformed(),
        }
    }
    if filter.region.is_some() && layout < 9 {
        return Err(fault(
            "records.hotfix_filter_unsupported",
            2,
            "XFTH layout carries no region",
        ));
    }
    let mut page = CachePage {
        version,
        record_layout: layout,
        filter: filter.clone(),
        entries: vec![],
        scanned: 0,
        matched: 0,
        selected_push: None,
        next_index: None,
        complete: false,
        truncated: false,
    };
    if filter.latest {
        walk(raw, start, layout, |e| {
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
    walk(raw, start, layout, |mut e| {
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
        e.payload_hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
        page.entries.push(e);
        Ok(())
    })?;
    if page.truncated {
        page.next_index = page.entries.last().map(|e| e.index);
    }
    page.complete = filter.after_index.is_none() && !page.truncated;
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
