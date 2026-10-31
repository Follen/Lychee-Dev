// SPDX-License-Identifier: MIT
// CASC layout ported from Lychee Dev's MIT wowdata-derived archive reader.
//! Authenticated local CASC indices and objects. A nine-byte index prefix is
//! only a candidate; every returned object verifies its complete BLTE EKey.
use crate::{
    blte, fault,
    resource::{Budget, Cost},
};
use lycheedev_core::{
    Result, ToolkitError,
    control::{OperationControl, TrackedCancellation},
    storage,
};
mod controlled;
use controlled::Scope;
use md5::{Digest, Md5};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    sync::{Mutex, atomic::AtomicU64},
    time::{Duration, Instant},
};
mod query_source;
mod selected;
pub(crate) use query_source::QuerySource;
pub(crate) use selected::{open_selected, open_selected_from_source};
/// An authenticated physical section; only the BLTE framing is read at open.
/// The shared range decoder independently authenticates requested chunks/pages.
pub struct LocalReader<'a> {
    file: Mutex<fs::File>,
    root: PathBuf,
    path: PathBuf,
    archive_bytes: u64,
    pub index: String,
    pub span: Span,
    pub encoding_key: String,
    pub header_sha256: String,
    limits: Limits,
    io_bytes: AtomicU64,
    io_reads: AtomicU64,
    cancel: Option<&'a TrackedCancellation>,
    budget: Option<&'a Budget>,
    control: Option<&'a OperationControl>,
}
impl LocalReader<'_> {
    fn scope(&self) -> Scope<'_> {
        Scope {
            limits: self.limits,
            cancel: self.cancel,
            control: self.control,
            budget: self.budget,
        }
    }
    pub fn io_stats(&self) -> serde_json::Value {
        serde_json::json!({"bytes":self.io_bytes.load(Ordering::Relaxed),"reads":self.io_reads.load(Ordering::Relaxed),"wholeEncoded":false})
    }
    /// All borrowed ranges must have ended before consuming the actual handle.
    pub(crate) fn close_after<T>(self, result: Result<T>) -> Result<T> {
        let scope = Scope {
            limits: self.limits,
            cancel: self.cancel,
            control: self.control,
            budget: self.budget,
        };
        let (file, poison) = match self.file.into_inner() {
            Ok(file) => (file, None),
            Err(error) => (
                error.into_inner(),
                Some(fault("records.internal", 8, "CASC reader lock poisoned")),
            ),
        };
        let mut cleanup: Vec<_> = poison.into_iter().collect();
        if let Err(error) = storage::close_file(file) {
            cleanup.push(error.into());
        }
        scope.finish(result, cleanup)
    }
}
impl blte::EncodedRead for LocalReader<'_> {
    fn check_encoded(&self) -> Result<()> {
        self.scope().check()
    }
    fn encoded_size(&self) -> usize {
        (self.span.bytes - 30) as usize
    }
    fn read_encoded(&self, offset: usize, bytes: usize) -> Result<blte::EncodedSpan<'_>> {
        let scope = self.scope();
        scope.check()?;
        if let Some(budget) = self.budget {
            budget.charge(Cost {
                decode_work: bytes.div_ceil(65536) as u64,
                ..Default::default()
            })?;
        }
        let end = offset
            .checked_add(bytes)
            .filter(|end| *end <= self.encoded_size())
            .ok_or_else(invalid)?;
        if end > self.limits.encoded_bytes || bytes > 64 << 20 {
            return Err(exceeded());
        }
        scope.guard(&self.root, &self.path)?;
        let mut file = loop {
            scope.check()?;
            match self.file.try_lock() {
                Ok(file) => break file,
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(fault("records.internal", 8, "CASC reader lock poisoned"));
                }
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(1))
                }
            }
        };
        if file.metadata()?.len() != self.archive_bytes {
            return Err(fault(
                "records.pinned_build_changed",
                4,
                "Local archive size changed",
            ));
        }
        scope.check()?;
        file.seek(SeekFrom::Start(self.span.offset + 30 + offset as u64))?;
        blte::EncodedSpan::read_owned(bytes, self.budget, |raw| {
            for chunk in raw.chunks_mut(65536) {
                let mut written = 0;
                while written < chunk.len() {
                    scope.check()?;
                    let n = file.read(&mut chunk[written..])?;
                    // Account successful OS reads before cancellation classification.
                    self.io_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    if n == 0 {
                        return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
                    }
                    written += n;
                    scope.check()?;
                }
            }
            self.io_reads.fetch_add(1, Ordering::Relaxed);
            scope.guard(&self.root, &self.path)
        })
    }
}
pub(crate) fn open_local_reader_controlled<'a>(
    root: &Path,
    encoding_key: &str,
    encoded_bytes: usize,
    limits: Limits,
    control: &'a OperationControl,
    budget: Option<&'a Budget>,
) -> Result<LocalReader<'a>> {
    open_reader(
        root,
        encoding_key,
        encoded_bytes,
        Scope {
            limits,
            cancel: None,
            control: Some(control),
            budget,
        },
    )
}
fn open_reader<'a>(
    root: &Path,
    encoding_key: &str,
    encoded_bytes: usize,
    scope: Scope<'a>,
) -> Result<LocalReader<'a>> {
    open_reader_source(root, encoding_key, encoded_bytes, scope, None)
}
pub(crate) fn open_local_reader_from_source<'a>(
    source: &QuerySource<'a>,
    encoding_key: &str,
    encoded_bytes: usize,
) -> Result<LocalReader<'a>> {
    open_reader_source(
        source.root(),
        encoding_key,
        encoded_bytes,
        source.scope(),
        Some(source),
    )
}
fn open_reader_source<'a>(
    root: &Path,
    encoding_key: &str,
    encoded_bytes: usize,
    scope: Scope<'a>,
    source: Option<&QuerySource<'a>>,
) -> Result<LocalReader<'a>> {
    scope.check()?;
    let Scope {
        limits,
        cancel,
        budget,
        control,
    } = scope;
    let key = key(encoding_key)?;
    if !(12..=limits.encoded_bytes).contains(&encoded_bytes)
        || limits.encoded_bytes > 256 << 20
        || limits.index_bytes == 0
        || limits.index_bytes > 64 << 20
        || limits.directory_entries == 0
        || limits.directory_entries > 4096
    {
        return Err(exceeded());
    }
    let directory = root.join("Data/data");
    let selected = if let Some(source) = source {
        source.indices(&key)?
    } else {
        select_indices_with_check(root, limits, cancel, || scope.check())?
            .into_iter()
            .map(|(bucket, (_, name))| (bucket, name))
            .collect()
    };
    let mut collision = false;
    for (bucket, name) in selected {
        let spans = if let Some(source) = source {
            source.find(bucket, &name, &key)?
        } else {
            let raw = bounded_read_with_check(
                &directory.join(&name),
                root,
                limits.index_bytes,
                limits,
                cancel,
                budget,
                || scope.check(),
            )?;
            Index::open_with_check(&raw, bucket, limits, cancel, || scope.check())?
                .find_with_check(&key, limits, cancel, || scope.check())?
        };
        for span in spans {
            if span.bytes != encoded_bytes as u64 + 30 {
                collision = true;
                continue;
            }
            let path = directory.join(span.filename());
            scope.guard(root, &path)?;
            let mut file = if let Some(source) = source {
                source.open_file(&span.filename())?
            } else {
                fs::File::open(&path)?
            };
            let result = (|| {
                scope.check()?;
                let meta = file.metadata()?;
                if !meta.is_file()
                    || span.offset > meta.len()
                    || span.bytes > meta.len() - span.offset
                {
                    return Err(invalid());
                }
                scope.check()?;
                file.seek(SeekFrom::Start(span.offset))?;
                let mut prefix = [0; 42];
                scope.check()?;
                file.read_exact(&mut prefix)?;
                scope.check()?;
                if prefix[..30].iter().any(|b| *b != 0) && (0..9).any(|i| prefix[15 - i] != key[i])
                {
                    return Ok(None);
                }
                if &prefix[30..34] != b"BLTE" {
                    return Err(invalid());
                }
                let header = u32::from_be_bytes(prefix[34..38].try_into().unwrap()) as usize;
                if header == 0 {
                    return Err(fault(
                        "container.unsupported_encoding",
                        3,
                        "Headerless objects require sequential content authentication",
                    ));
                }
                if header < 12 || header > encoded_bytes || header > 12 + 65536 * 24 {
                    return Err(invalid());
                }
                if let Some(budget) = budget {
                    budget.charge(Cost::metadata(header))?;
                }
                let mut framing = vec![0; header];
                framing[..12].copy_from_slice(&prefix[30..]);
                for chunk in framing[12..].chunks_mut(65536) {
                    scope.check()?;
                    file.read_exact(chunk)?;
                    scope.check()?;
                }
                if Md5::digest(&framing).as_slice() != key {
                    return Ok(None);
                }
                scope.guard(root, &path)?;
                Ok(Some((meta.len(), header, storage::sha256(&framing))))
            })();
            match result {
                Ok(Some((archive_bytes, header, header_sha256))) => {
                    return Ok(LocalReader {
                        file: Mutex::new(file),
                        root: root.to_owned(),
                        path,
                        archive_bytes,
                        index: name,
                        span,
                        encoding_key: encoding_key.into(),
                        header_sha256,
                        limits,
                        io_bytes: AtomicU64::new(header as u64 + 30),
                        io_reads: AtomicU64::new(1),
                        cancel,
                        budget,
                        control,
                    });
                }
                other => {
                    scope.close(file, other)?;
                    collision = true;
                }
            }
        }
    }
    Err(if collision {
        integrity()
    } else {
        fault(
            "records.local_object_unavailable",
            3,
            "Full EKey unavailable in newest local CASC indices",
        )
    })
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub archive: u16,
    pub offset: u64,
    pub bytes: u64,
}
impl Span {
    pub fn filename(self) -> String {
        format!("data.{:03}", self.archive)
    }
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub index_bytes: usize,
    pub encoded_bytes: usize,
    pub directory_entries: usize,
    pub deadline: Instant,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            index_bytes: 64 << 20,
            encoded_bytes: 256 << 20,
            directory_entries: 4096,
            deadline: Instant::now() + Duration::from_secs(180),
        }
    }
}
fn check(limits: Limits, cancel: Option<&TrackedCancellation>) -> Result<()> {
    if cancel.is_some_and(|c| c.is_cancelled()) {
        return Err(fault("operation.cancelled", 7, "CASC operation cancelled"));
    }
    if Instant::now() >= limits.deadline {
        return Err(fault("records.resource_limit", 3, "CASC deadline exceeded"));
    }
    Ok(())
}
fn invalid() -> lycheedev_core::ToolkitError {
    fault("archive.invalid_index", 4, "Invalid CASC index or extent")
}
fn integrity() -> lycheedev_core::ToolkitError {
    fault(
        "archive.index_integrity",
        4,
        "CASC checksum or full object identity differs",
    )
}
fn exceeded() -> lycheedev_core::ToolkitError {
    fault("archive.index_limit", 3, "CASC resource budget exceeded")
}
fn le32(raw: &[u8]) -> u32 {
    u32::from_le_bytes(raw[..4].try_into().unwrap())
}
/// Jenkins public-domain lookup3 little-endian two-word checksum. This format
/// guard detects accidental corruption; it is not a content identity.
pub fn guard_sum(mut raw: &[u8], primary: u32, secondary: u32) -> (u32, u32) {
    let mut a = 0xdeadbeefu32
        .wrapping_add(raw.len() as u32)
        .wrapping_add(primary);
    let mut b = a;
    let mut c = a.wrapping_add(secondary);
    macro_rules! mix {
        ($x:ident,$y:ident,$r:expr) => {{
            $x = $x.wrapping_sub($y);
            $x ^= $y.rotate_left($r);
        }};
    }
    while raw.len() > 12 {
        a = a.wrapping_add(le32(raw));
        b = b.wrapping_add(le32(&raw[4..]));
        c = c.wrapping_add(le32(&raw[8..]));
        mix!(a, c, 4);
        c = c.wrapping_add(b);
        mix!(b, a, 6);
        a = a.wrapping_add(c);
        mix!(c, b, 8);
        b = b.wrapping_add(a);
        mix!(a, c, 16);
        c = c.wrapping_add(b);
        mix!(b, a, 19);
        a = a.wrapping_add(c);
        mix!(c, b, 4);
        b = b.wrapping_add(a);
        raw = &raw[12..];
    }
    if raw.is_empty() {
        return (c, b);
    }
    for (i, &v) in raw.iter().enumerate() {
        let n = (v as u32) << ((i % 4) * 8);
        match i {
            0..=3 => a = a.wrapping_add(n),
            4..=7 => b = b.wrapping_add(n),
            _ => c = c.wrapping_add(n),
        }
    }
    macro_rules! final_mix {
        ($x:ident,$y:ident,$r:expr) => {{
            $x ^= $y;
            $x = $x.wrapping_sub($y.rotate_left($r));
        }};
    }
    final_mix!(c, b, 14);
    final_mix!(a, c, 11);
    final_mix!(b, a, 25);
    final_mix!(c, b, 16);
    final_mix!(a, c, 4);
    final_mix!(b, a, 14);
    final_mix!(c, b, 24);
    (c, b)
}
pub struct Index<'a> {
    raw: &'a [u8],
    bits: u8,
}
impl<'a> Index<'a> {
    pub fn open(
        raw: &'a [u8],
        bucket: u8,
        limits: Limits,
        cancel: Option<&TrackedCancellation>,
    ) -> Result<Self> {
        Self::open_with_check(raw, bucket, limits, cancel, || Ok(()))
    }
    pub(crate) fn open_with_check(
        raw: &'a [u8],
        bucket: u8,
        limits: Limits,
        cancel: Option<&TrackedCancellation>,
        mut supplemental: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        supplemental()?;
        check(limits, cancel)?;
        if limits.index_bytes == 0
            || limits.index_bytes > 64 << 20
            || raw.len() > limits.index_bytes
        {
            return Err(exceeded());
        }
        if raw.len() < 40 || bucket > 15 || le32(raw) != 16 {
            return Err(invalid());
        }
        let meta = &raw[8..24];
        if u16::from_le_bytes(meta[..2].try_into().unwrap()) != 7
            || meta[2] != bucket
            || meta[3] != 0
            || meta[4..7] != [4, 5, 9]
            || !(1..=39).contains(&meta[7])
            || u64::from_le_bytes(meta[8..].try_into().unwrap()) == 0
        {
            return Err(invalid());
        }
        if guard_sum(meta, 0, 0).0 != le32(&raw[4..]) {
            return Err(integrity());
        }
        let length = le32(&raw[32..]) as usize;
        if !length.is_multiple_of(18) || length > raw.len() - 40 {
            return Err(invalid());
        }
        if length / 18 > 1 << 20 {
            return Err(exceeded());
        }
        let mut primary = 0;
        let mut secondary = 0;
        let mut alternate = 0;
        let entries = &raw[40..40 + length];
        let mut previous: Option<&[u8]> = None;
        for (i, entry) in entries.as_chunks::<18>().0.iter().enumerate() {
            if i.is_multiple_of(4096) {
                supplemental()?;
                check(limits, cancel)?;
            }
            if previous.is_some_and(|p| p > &entry[..9]) {
                return Err(invalid());
            }
            previous = Some(&entry[..9]);
            (primary, secondary) = guard_sum(entry, primary, secondary);
            alternate = guard_sum(entry, alternate, 0).0;
        }
        let expected = le32(&raw[36..]);
        if expected != primary && expected != alternate {
            return Err(integrity());
        }
        supplemental()?;
        check(limits, cancel)?;
        Ok(Self {
            raw: entries,
            bits: meta[7],
        })
    }
    pub fn find(
        &self,
        key: &[u8; 16],
        limits: Limits,
        cancel: Option<&TrackedCancellation>,
    ) -> Result<Vec<Span>> {
        self.find_with_check(key, limits, cancel, || Ok(()))
    }
    pub(crate) fn find_with_check(
        &self,
        key: &[u8; 16],
        limits: Limits,
        cancel: Option<&TrackedCancellation>,
        mut supplemental: impl FnMut() -> Result<()>,
    ) -> Result<Vec<Span>> {
        supplemental()?;
        check(limits, cancel)?;
        let count = self.raw.len() / 18;
        let mut low = 0;
        let mut high = count;
        while low < high {
            supplemental()?;
            let mid = low + (high - low) / 2;
            if self.raw[mid * 18..mid * 18 + 9] < key[..9] {
                low = mid + 1
            } else {
                high = mid
            }
        }
        let mut result = vec![];
        for entry in self.raw[low * 18..].as_chunks::<18>().0 {
            supplemental()?;
            if entry[..9] != key[..9] {
                break;
            }
            let location = ((entry[9] as u64) << 32)
                | u32::from_be_bytes(entry[10..14].try_into().unwrap()) as u64;
            let archive = location >> self.bits;
            let offset = location & ((1u64 << self.bits) - 1);
            let bytes = le32(&entry[14..]) as u64;
            if archive > 4095 || bytes < 38 || bytes > (1u64 << self.bits) - offset {
                return Err(invalid());
            }
            if result.len() == 64 {
                return Err(exceeded());
            }
            result.push(Span {
                archive: archive as u16,
                offset,
                bytes,
            });
        }
        supplemental()?;
        check(limits, cancel)?;
        Ok(result)
    }
}
pub fn key(value: &str) -> Result<[u8; 16]> {
    if !lycheedev_core::selection::valid_digest(value, 32) {
        return Err(fault(
            "records.metadata_invalid",
            4,
            "Expected lowercase 32-hex metadata key",
        ));
    }
    let mut key = [0; 16];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|_| invalid())?;
    }
    Ok(key)
}
pub fn hex(raw: &[u8]) -> String {
    raw.iter().map(|b| format!("{b:02x}")).collect()
}
fn bounded_read_with_check(
    path: &Path,
    root: &Path,
    max: usize,
    limits: Limits,
    cancel: Option<&TrackedCancellation>,
    budget: Option<&Budget>,
    mut supplemental: impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    supplemental()?;
    storage::guard_path_with_check(root, path, &mut supplemental)?;
    let mut file = fs::File::open(path)?;
    let result = (|| {
        let meta = file.metadata()?;
        if !meta.is_file() || meta.len() > max as u64 {
            return Err(exceeded());
        }
        if let Some(budget) = budget {
            budget.charge_once(
                &format!("index:{}:{}", path.display(), meta.len()),
                Cost::metadata(meta.len() as usize * 2),
            )?;
            budget.charge(Cost::work(meta.len() as usize / 18 + 1))?;
        }
        let mut raw = Vec::with_capacity(meta.len() as usize);
        let mut buf = [0; 65536];
        loop {
            supplemental()?;
            check(limits, cancel)?;
            let n = file.read(&mut buf)?;
            supplemental()?;
            if n == 0 {
                break;
            }
            if n > max - raw.len() {
                return Err(exceeded());
            }
            raw.extend_from_slice(&buf[..n]);
        }
        storage::guard_path_with_check(root, path, &mut supplemental)?;
        if raw.len() as u64 != meta.len() || file.metadata()?.len() != meta.len() {
            return Err(integrity());
        }
        supplemental()?;
        Ok(raw)
    })();
    let cleanup = storage::close_file(file)
        .err()
        .map(Into::into)
        .into_iter()
        .collect();
    controlled::finish(
        result,
        cleanup,
        [
            supplemental().err(),
            budget.and_then(|budget| budget.check().err()),
            check(limits, cancel).err(),
        ]
        .into_iter()
        .flatten(),
    )
}
fn select_indices_with_check(
    root: &Path,
    limits: Limits,
    cancel: Option<&TrackedCancellation>,
    mut supplemental: impl FnMut() -> Result<()>,
) -> Result<BTreeMap<u8, (u32, String)>> {
    supplemental()?;
    let directory = root.join("Data/data");
    storage::guard_path_with_check(root, &directory, &mut supplemental)?;
    let mut selected = BTreeMap::<u8, (u32, String)>::new();
    for (n, entry) in fs::read_dir(&directory)?.enumerate() {
        supplemental()?;
        check(limits, cancel)?;
        if n >= limits.directory_entries {
            return Err(exceeded());
        }
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.len() != 14
            || !name.is_ascii()
            || !name[10..].eq_ignore_ascii_case(".idx")
            || entry.file_type()?.is_dir()
        {
            continue;
        }
        let Ok(bucket) = u8::from_str_radix(&name[..2], 16) else {
            continue;
        };
        let Ok(generation) = u32::from_str_radix(&name[2..10], 16) else {
            continue;
        };
        if bucket > 15 {
            continue;
        }
        match selected.get(&bucket) {
            Some((old, _)) if *old == generation => {
                return Err(fault(
                    "records.build_ambiguous",
                    4,
                    "Duplicate CASC index generation",
                ));
            }
            Some((old, _)) if *old > generation => {}
            _ => {
                selected.insert(bucket, (generation, name.into()));
            }
        }
    }
    supplemental()?;
    Ok(selected)
}

#[cfg(test)]
mod span_tests;
