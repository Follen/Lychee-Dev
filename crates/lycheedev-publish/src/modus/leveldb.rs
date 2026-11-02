//! Read only the three fixed ModUs renderer keys from the current LevelDB view.
//! CURRENT/MANIFEST select tables; sequence numbers and deletions select values.
//! Formats: google/leveldb doc/log_format.md and doc/table_format.md.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};
const LIMIT: usize = 512 * 1024 * 1024;
const PREFIX: &[u8] = b"_http://tongji.modus.cool\0";
fn bad() -> ToolkitError {
    ToolkitError::new(
        "publish.modus_local_storage",
        "ModUs current local storage is incomplete or changed; close the main client and retry, or log in again",
        3,
    )
}
struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|e| *e <= self.data.len())
            .ok_or_else(bad)?;
        let value = &self.data[self.at..end];
        self.at = end;
        Ok(value)
    }
    fn var(&mut self) -> Result<u64> {
        let mut value = 0u64;
        for shift in (0..70).step_by(7) {
            let b = self.take(1)?[0];
            if shift == 63 && b > 1 {
                return Err(bad());
            }
            value |= u64::from(b & 127) << shift;
            if b & 128 == 0 {
                return Ok(value);
            }
        }
        Err(bad())
    }
    fn size(&mut self) -> Result<usize> {
        usize::try_from(self.var()?).map_err(|_| bad())
    }
    fn field(&mut self) -> Result<&'a [u8]> {
        let n = self.size()?;
        self.take(n)
    }
    fn done(&self) -> bool {
        self.at == self.data.len()
    }
}
fn u32le(bytes: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| bad())?))
}
fn u64le(bytes: &[u8]) -> Result<u64> {
    Ok(u64::from_le_bytes(bytes.try_into().map_err(|_| bad())?))
}
fn checksum(bytes: impl Iterator<Item = u8>) -> u32 {
    let mut crc = !0u32;
    for b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0x82f63b78 } else { 0 };
        }
    }
    (!crc).rotate_right(15).wrapping_add(0xa282ead8)
}
fn logs(
    data: &[u8],
    control: &OperationControl,
    mut visit: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let mut at = 0;
    let mut fragments: Option<Vec<u8>> = None;
    while at < data.len() {
        control.check()?;
        let remaining = 32768 - at % 32768;
        if remaining < 7 {
            at += remaining;
            continue;
        }
        if data.len() - at < 7 {
            break;
        }
        let crc = u32le(&data[at..at + 4])?;
        let n = usize::from(u16::from_le_bytes([data[at + 4], data[at + 5]]));
        let kind = data[at + 6];
        if crc == 0 && n == 0 && kind == 0 {
            at += remaining;
            continue;
        }
        if n > remaining - 7 {
            return Err(bad());
        }
        at += 7;
        if n > data.len() - at {
            break;
        }
        let payload = &data[at..at + n];
        at += n;
        if crc != checksum(std::iter::once(kind).chain(payload.iter().copied())) {
            return Err(bad());
        }
        match kind {
            1 if fragments.is_none() => visit(payload)?,
            2 if fragments.is_none() => fragments = Some(payload.to_vec()),
            3 | 4 if fragments.is_some() => {
                fragments.as_mut().unwrap().extend_from_slice(payload);
                if kind == 4 {
                    visit(&fragments.take().unwrap())?;
                }
            }
            _ => return Err(bad()),
        }
    }
    Ok(()) // An incomplete final physical record has not committed.
}
fn manifest(data: &[u8], control: &OperationControl) -> Result<(BTreeSet<u64>, u64, u64)> {
    let mut tables = BTreeSet::new();
    let mut log = None;
    let mut previous = 0;
    logs(data, control, |record| {
        let mut r = Cursor::new(record);
        while !r.done() {
            match r.var()? {
                1 => {
                    r.field()?;
                }
                2 => log = Some(r.var()?),
                3 | 4 => {
                    r.var()?;
                }
                9 => previous = r.var()?,
                5 => {
                    r.var()?;
                    r.field()?;
                }
                6 => {
                    r.var()?;
                    tables.remove(&r.var()?);
                }
                7 => {
                    r.var()?;
                    tables.insert(r.var()?);
                    r.var()?;
                    r.field()?;
                    r.field()?;
                }
                _ => return Err(bad()),
            }
        }
        Ok(())
    })?;
    Ok((tables, log.ok_or_else(bad)?, previous))
}
fn snappy(data: &[u8]) -> Result<Vec<u8>> {
    let mut r = Cursor::new(data);
    let expected = r.size()?;
    if expected > 64 * 1024 * 1024 {
        return Err(bad());
    }
    let mut out = Vec::with_capacity(expected);
    while !r.done() {
        let tag = r.take(1)?[0];
        let kind = tag & 3;
        if kind == 0 {
            let len = usize::from(tag >> 2);
            let n = if len < 60 {
                len + 1
            } else {
                let mut bytes = [0u8; 4];
                let width = len - 59;
                bytes[..width].copy_from_slice(r.take(width)?);
                usize::try_from(u32::from_le_bytes(bytes))
                    .map_err(|_| bad())?
                    .checked_add(1)
                    .ok_or_else(bad)?
            };
            if n > expected.saturating_sub(out.len()) {
                return Err(bad());
            }
            out.extend_from_slice(r.take(n)?);
        } else {
            let width = match kind {
                1 => 1,
                2 => 2,
                _ => 4,
            };
            let mut bytes = [0u8; 4];
            bytes[..width].copy_from_slice(r.take(width)?);
            let mut distance = u32::from_le_bytes(bytes) as usize;
            let len = if kind == 1 {
                distance |= usize::from(tag & 0xe0) << 3;
                4 + usize::from((tag >> 2) & 7)
            } else {
                1 + usize::from(tag >> 2)
            };
            if distance == 0 || distance > out.len() || len > expected.saturating_sub(out.len()) {
                return Err(bad());
            }
            for _ in 0..len {
                out.push(out[out.len() - distance]);
            }
        }
    }
    if out.len() != expected {
        return Err(bad());
    }
    Ok(out)
}
fn block(data: &[u8], handle: &[u8]) -> Result<Vec<u8>> {
    let mut r = Cursor::new(handle);
    let offset = r.size()?;
    let size = r.size()?;
    let end = offset
        .checked_add(size)
        .filter(|e| e.checked_add(5).is_some_and(|n| n <= data.len()))
        .ok_or_else(bad)?;
    let raw = &data[offset..end];
    let kind = data[end];
    if u32le(&data[end + 1..end + 5])? != checksum(raw.iter().copied().chain(std::iter::once(kind)))
    {
        return Err(bad());
    }
    match kind {
        0 => Ok(raw.to_vec()),
        1 => snappy(raw),
        _ => Err(bad()),
    }
}
fn entries(data: &[u8], mut visit: impl FnMut(&[u8], &[u8]) -> Result<()>) -> Result<()> {
    if data.len() < 8 {
        return Err(bad());
    }
    let count = u32le(&data[data.len() - 4..])? as usize;
    let end = count
        .checked_mul(4)
        .and_then(|n| data.len().checked_sub(n + 4))
        .filter(|_| count > 0)
        .ok_or_else(bad)?;
    let mut r = Cursor::new(&data[..end]);
    let mut key = Vec::new();
    while !r.done() {
        let shared = r.size()?;
        let n = r.size()?;
        let value_size = r.size()?;
        if shared > key.len() {
            return Err(bad());
        }
        key.truncate(shared);
        key.extend_from_slice(r.take(n)?);
        visit(&key, r.take(value_size)?)?;
    }
    Ok(())
}
type Latest = BTreeMap<String, (u64, Option<Vec<u8>>)>;
fn chromium(data: &[u8]) -> Result<String> {
    match data.split_first() {
        Some((1, bytes)) => Ok(bytes.iter().map(|b| char::from(*b)).collect()),
        Some((0, bytes)) if bytes.len() % 2 == 0 => String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| bad()),
        _ => Err(bad()),
    }
}
fn remember(latest: &mut Latest, seq: u64, key: &[u8], value: Option<&[u8]>) -> Result<()> {
    let Some(key) = key.strip_prefix(PREFIX) else {
        return Ok(());
    };
    let name = chromium(key)?;
    if !["user", "app", "wow"].contains(&name.as_str()) {
        return Ok(());
    }
    if let Some((old_seq, old)) = latest.get(&name) {
        if *old_seq > seq {
            return Ok(());
        }
        if *old_seq == seq && old.as_deref() != value {
            return Err(bad());
        }
    }
    latest.insert(name, (seq, value.map(<[u8]>::to_vec)));
    Ok(())
}
fn table(data: &[u8], latest: &mut Latest, control: &OperationControl) -> Result<()> {
    if data.len() < 48 || data[data.len() - 8..] != [0x57, 0xfb, 0x80, 0x8b, 0x24, 0x75, 0x47, 0xdb]
    {
        return Err(bad());
    }
    let footer = &data[data.len() - 48..data.len() - 8];
    let mut r = Cursor::new(footer);
    r.var()?;
    r.var()?;
    entries(&block(data, &footer[r.at..])?, |_, handle| {
        control.check()?;
        entries(&block(data, handle)?, |key, value| {
            if key.len() < 8 {
                return Err(bad());
            }
            let end = key.len() - 8;
            let tag = u64le(&key[end..])?;
            let value = match tag & 255 {
                0 => None,
                1 => Some(value),
                _ => return Err(bad()),
            };
            remember(latest, tag >> 8, &key[..end], value)
        })
    })
}
fn batches(data: &[u8], latest: &mut Latest, control: &OperationControl) -> Result<()> {
    logs(data, control, |bytes| {
        let mut r = Cursor::new(bytes);
        let seq = u64le(r.take(8)?)?;
        let n = u32le(r.take(4)?)?;
        for i in 0..n {
            let kind = r.take(1)?[0];
            let key = r.field()?;
            let value = match kind {
                0 => None,
                1 => Some(r.field()?),
                _ => return Err(bad()),
            };
            remember(
                latest,
                seq.checked_add(u64::from(i)).ok_or_else(bad)?,
                key,
                value,
            )?;
        }
        if !r.done() {
            return Err(bad());
        }
        Ok(())
    })
}
fn read_file(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path).map_err(|_| bad())?;
    if file.metadata().map_err(|_| bad())?.len() > LIMIT as u64 {
        return Err(bad());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| bad())?;
    if bytes.len() > LIMIT {
        return Err(bad());
    }
    Ok(bytes)
}
pub(super) fn read(root: &Path, control: &OperationControl) -> Result<BTreeMap<String, String>> {
    control.check()?;
    let current = read_file(&root.join("CURRENT"))?;
    let name = std::str::from_utf8(&current).map_err(|_| bad())?.trim();
    if !name
        .strip_prefix("MANIFEST-")
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(bad());
    }
    let manifest_bytes = read_file(&root.join(name))?;
    let (tables, min_log, prev_log) = manifest(&manifest_bytes, control)?;
    let mut files = Vec::new();
    for n in tables {
        let file = ["ldb", "sst"]
            .into_iter()
            .map(|ext| root.join(format!("{n:06}.{ext}")))
            .find(|p| p.is_file())
            .ok_or_else(bad)?;
        files.push((file, true));
    }
    let mut log_found = min_log == 0;
    for entry in fs::read_dir(root).map_err(|_| bad())? {
        let entry = entry.map_err(|_| bad())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(n) = name
            .strip_suffix(".log")
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<u64>().ok())
        {
            if n >= min_log || n == prev_log {
                log_found |= n == min_log;
                files.push((entry.path(), false));
            }
        }
    }
    if !log_found {
        return Err(bad());
    }
    let mut latest = Latest::new();
    let mut stamps = Vec::new();
    for (path, is_table) in files {
        control.check()?;
        let before = fs::metadata(&path).map_err(|_| bad())?;
        let bytes = read_file(&path)?;
        if is_table {
            table(&bytes, &mut latest, control)?;
        } else {
            batches(&bytes, &mut latest, control)?;
        }
        stamps.push((path, before.len(), before.modified().map_err(|_| bad())?));
    }
    if current != read_file(&root.join("CURRENT"))?
        || manifest_bytes != read_file(&root.join(name))?
    {
        return Err(bad());
    }
    for (path, size, modified) in stamps {
        let after = fs::metadata(path).map_err(|_| bad())?;
        if after.len() != size || after.modified().map_err(|_| bad())? != modified {
            return Err(bad());
        }
    }
    control.check()?;
    latest
        .into_iter()
        .filter_map(|(k, (_, v))| v.map(|v| chromium(&v).map(|v| (k, v))))
        .collect()
}

#[cfg(test)]
pub(super) mod tests;
