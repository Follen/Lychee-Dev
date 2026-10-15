//! Authenticated CDN encoded objects. Range cache bytes are only inputs to the
//! footer/TOC/page/EKey verifiers; a successful open does not claim a whole CKey.
use super::{
    availability_failure,
    cache::{
        Store, pure_expected,
        wire::{Object, ObjectSeed, folded, scalar},
    },
    catalog,
    fragments::{Fragment, Fragments},
    index::{self, Span},
    invalid, limit,
    transport::RangeRequest,
};
use crate::{fault, installed::BuildMetadata};
use lycheedev_core::{Result, ToolkitError, evidence::GoSignedInteger};
use md5::{Digest, Md5};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess},
};
use std::{
    io::Read,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

const BLOCK: usize = 256 << 10;
fn internal() -> ToolkitError {
    fault("records.internal", 8, "CDN reader owner poisoned")
}
fn range() -> ToolkitError {
    fault("records.remote_range", 4, "records.remote_range")
}
fn missing() -> ToolkitError {
    fault(
        "records.remote_object_missing",
        3,
        "records.remote_object_missing",
    )
}
fn malformed() -> ToolkitError {
    fault("container.invalid_format", 4, "Malformed BLTE container")
}
fn integrity() -> ToolkitError {
    fault(
        "container.integrity_mismatch",
        4,
        "BLTE checksum/extent mismatch",
    )
}
fn terminal(error: &ToolkitError) -> bool {
    !pure_expected(error, "records.content_missing") && !availability_failure(error)
        || error.details.get("cleanup").is_some()
}
#[derive(Default, Serialize)]
struct Location {
    object: String,
    offset: i64,
    bytes: i64,
}
impl Object for Location {
    fn fields<'de, A: MapAccess<'de>>(&mut self, mut map: A) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match folded(&key).as_str() {
                "object" => scalar(&mut map, &mut self.object)?,
                "offset" => {
                    let mut n = GoSignedInteger(self.offset);
                    scalar(&mut map, &mut n)?;
                    self.offset = n.0;
                }
                "bytes" => {
                    let mut n = GoSignedInteger(self.bytes);
                    scalar(&mut map, &mut n)?;
                    self.bytes = n.0;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for Location {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        ObjectSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}
impl From<Span> for Location {
    fn from(span: Span) -> Self {
        Self {
            object: span.object,
            offset: span.offset,
            bytes: span.bytes,
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Counters {
    pub requests: u64,
    pub transferred: i64,
    pub fragment_loads: u64,
    pub block_cache_hits: u64,
}
pub(crate) struct Files<'a> {
    fragments: Mutex<Fragments<'a>>,
    pub store: Store<'a>,
    pub build: Arc<BuildMetadata>,
    offline: bool,
    workers: usize,
    block_hits: AtomicU64,
}
impl<'a> Files<'a> {
    /// `workers` is an already admitted per-query lease, not a workspace-wide
    /// configuration value. The caller must retain that lease for this owner.
    pub fn new(fragments: Fragments<'a>, workers: usize) -> Result<Self> {
        if !(1..=8).contains(&workers) {
            return Err(limit());
        }
        fragments.store.check()?;
        Ok(Self {
            store: Store {
                home: fragments.store.home,
                control: fragments.store.control,
                budget: fragments.store.budget,
            },
            build: fragments.build.clone(),
            offline: fragments.offline,
            fragments: Mutex::new(fragments),
            workers,
            block_hits: AtomicU64::new(0),
        })
    }
    fn locked(&self) -> Result<MutexGuard<'_, Fragments<'a>>> {
        self.fragments.lock().map_err(|_| internal())
    }
    pub fn counters(&self) -> Result<Counters> {
        let c = self.locked()?;
        Ok(Counters {
            requests: c.requests,
            transferred: c.transferred,
            fragment_loads: c.fragment_loads,
            block_cache_hits: self.block_hits.load(Ordering::Relaxed),
        })
    }
    /// Only after this worker's JoinHandle was consumed. A panic may poison
    /// its business mutex, but cannot erase work already recorded inside it.
    /// This snapshot grants no permission to resume a poisoned reader.
    fn joined_counters(&self) -> Counters {
        let c = self
            .fragments
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Counters {
            requests: c.requests,
            transferred: c.transferred,
            fragment_loads: c.fragment_loads,
            block_cache_hits: self.block_hits.load(Ordering::Relaxed),
        }
    }
    fn range(&self, object: &str, offset: i64, length: i64, total: i64) -> Result<Fragment<'a>> {
        self.store.check()?;
        self.locked()?.range(
            object,
            RangeRequest {
                offset,
                length,
                total,
            },
        )
    }
    fn source(&self, object: String, size: i64, sparse: bool) -> Result<Source<'_, 'a>> {
        let size = if size == 0 {
            self.range(&object, 0, 1, 0)?.object_bytes()
        } else {
            size
        };
        if size < 0 {
            return Err(range());
        }
        Ok(Source {
            files: self,
            object,
            size,
            sparse,
            block: Mutex::new(None),
        })
    }
    /// The returned section has authenticated BLTE EKey framing (or the whole
    /// encoded object for headerless BLTE). CKey/chunks are checked by BLTE later.
    pub fn open(&self, key: &str, size: i64) -> Result<Encoded<'_, 'a>> {
        self.store.check()?;
        if !catalog::valid_key(key) || !(8..=(1i64 << 40)).contains(&size) {
            return Err(invalid());
        }
        let location_key = format!("cdn-location/{}/{key}", self.build.cdn_document.key);
        let cached = self.store.read::<Location>(&location_key)?;
        let is_new = cached
            .as_ref()
            .is_none_or(|(_, generation)| *generation == 0);
        let location = if let Some((location, _)) = cached {
            if location.bytes != size
                || !catalog::valid_key(&location.object)
                || location.offset < 0
            {
                return Err(super::cache::identity());
            }
            location
        } else {
            self.locate(key, size)?.into()
        };
        let total = if location.object == key && location.offset == 0 {
            size
        } else {
            0
        };
        let source = self.source(location.object.clone(), total, false)?;
        if location.offset > source.size || location.bytes > source.size - location.offset {
            return Err(range());
        }
        let result = Encoded {
            source,
            offset: location.offset,
            size: size as usize,
            position: 0,
        };
        let mut prefix = [0; 8];
        result.read_at(&mut prefix, 0)?;
        if &prefix[..4] != b"BLTE" {
            return Err(malformed());
        }
        let header = u32::from_be_bytes(prefix[4..].try_into().expect("four bytes")) as usize;
        if header != 0 && (header < 12 || header > result.size || header > 12 + 65536 * 24) {
            return Err(malformed());
        }
        let length = if header == 0 { result.size } else { header };
        let mut digest = Md5::new();
        let mut scratch = [0; 32768];
        let mut at = 0;
        while at < length {
            self.store.check()?;
            let amount = (length - at).min(scratch.len());
            result.read_at(&mut scratch[..amount], at)?;
            digest.update(&scratch[..amount]);
            at += amount;
        }
        if crate::casc::hex(&digest.finalize()) != key {
            return Err(integrity());
        }
        if is_new {
            match self.store.put(&location_key, &location, None) {
                Err(error) if pure_expected(&error, "vault.generation_conflict") => (),
                result => result?,
            }
        }
        self.store.check()?;
        Ok(result)
    }
    fn locate(&self, key: &str, size: i64) -> Result<Span> {
        let mut unavailable = match self.range(key, 0, 8, size) {
            Ok(_) => {
                return Ok(Span {
                    object: key.into(),
                    offset: 0,
                    bytes: size,
                });
            }
            Err(error)
                if availability_failure(&error) && error.details.get("cleanup").is_none() =>
            {
                (error.code == "records.remote_http").then_some(error)
            }
            Err(error) => return Err(error),
        };
        let archives = self
            .build
            .cdn_document
            .fields
            .get("archives")
            .map_or(&[][..], Vec::as_slice);
        if archives.len() > 8192 {
            return Err(limit());
        }
        if archives.iter().any(|key| !catalog::valid_key(key)) {
            return Err(invalid());
        }
        if let Some(group) = self
            .build
            .cdn_document
            .fields
            .get("archive-group")
            .filter(|v| !v.is_empty())
        {
            if group.len() != 1 {
                return Err(invalid());
            }
            match self.lookup_index(key, size, archives, &group[0], "", 0) {
                Ok(span) => return Ok(span),
                Err(error) if terminal(&error) => return Err(error),
                Err(error) if error.code == "records.remote_http" => unavailable = Some(error),
                Err(_) => (),
            }
        }
        let width = if self.store.budget.is_some() && !self.offline {
            self.workers
        } else {
            1
        };
        for batch in archives.chunks(width) {
            for result in self.lookup_batch(key, size, archives, batch)? {
                match result {
                    Ok(span) => return Ok(span),
                    Err(error) if terminal(&error) => return Err(error),
                    Err(error)
                        if error.code == "records.remote_http"
                            || error.code == "records.remote_object_missing"
                                && unavailable.is_none() =>
                    {
                        unavailable = Some(error)
                    }
                    Err(_) => (),
                }
            }
        }
        Err(unavailable.unwrap_or_else(missing))
    }
    fn lookup_index(
        &self,
        key: &str,
        size: i64,
        archives: &[String],
        index_key: &str,
        object: &str,
        expected: i64,
    ) -> Result<Span> {
        self.store.check()?;
        if !catalog::valid_key(index_key) {
            return Err(invalid());
        }
        let source = self.source(format!("{index_key}.index"), expected, true)?;
        // Keep the existing Go logical scratch reservation distinct from the
        // real Fragment/TOC/page allocation owners in Rust's safety ledger.
        let _scratch = if source.size <= BLOCK as i64 {
            self.store
                .budget
                .map(|b| b.reserve_logical_scratch((source.size as u64) * 2))
                .transpose()?
        } else {
            None
        };
        let whole = if source.size <= BLOCK as i64 {
            match self.range(&source.object, 0, source.size, source.size) {
                Ok(raw) => Some(raw),
                Err(error)
                    if self.offline
                        && pure_expected(&error, "records.remote_unavailable_offline") =>
                {
                    None
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        if source.size < 28 {
            return Err(invalid());
        }
        if source.size > 1 << 30 {
            return Err(limit());
        }
        let read = |out: &mut [u8], offset: i64| -> Result<()> {
            if let Some(raw) = &whole {
                let at = usize::try_from(offset).map_err(|_| range())?;
                let end = at.checked_add(out.len()).ok_or_else(range)?;
                out.copy_from_slice(raw.bytes().get(at..end).ok_or_else(range)?);
                Ok(())
            } else {
                source.read_at(out, offset)
            }
        };
        let mut raw_footer = [0; 28];
        read(&mut raw_footer, source.size - 28)?;
        let footer = index::Footer::parse(source.size, index_key, &raw_footer)?;
        let (toc_at, toc_len) = footer.toc_span();
        let _toc_owner = self
            .store
            .budget
            .map(|b| b.reserve_retained(toc_len))
            .transpose()?;
        let mut toc = vec![0; toc_len];
        read(&mut toc, toc_at)?;
        let directory = index::Directory::parse(footer, toc, self.store.control)?;
        let wanted = directory.page(key, self.store.control)?;
        let _page_owner = self
            .store
            .budget
            .map(|b| b.reserve_retained(wanted.bytes))
            .transpose()?;
        let mut page = vec![0; wanted.bytes];
        read(&mut page, wanted.offset)?;
        let span = directory.locate(wanted, key, &page, archives, object, self.store.control)?;
        self.store.check()?;
        if span.bytes != size {
            return Err(invalid());
        }
        Ok(span)
    }
    fn lookup_batch(
        &self,
        key: &str,
        size: i64,
        archives: &[String],
        batch: &[String],
    ) -> Result<Vec<Result<Span>>> {
        if batch.len() == 1 {
            return Ok(vec![
                self.lookup_index(key, size, archives, &batch[0], &batch[0], 0),
            ]);
        }
        let (base_requests, base_transferred, workers) = {
            let parent = self.locked()?;
            let mut workers = Vec::with_capacity(batch.len());
            for _ in batch {
                workers.push(Self::new(parent.fork_for_lookup(), self.workers)?);
            }
            (parent.requests, parent.transferred, workers)
        };
        let results = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(workers.len());
            let mut spawn_error = None;
            for (worker, archive) in workers.iter().zip(batch) {
                match std::thread::Builder::new()
                    .name("cdn-index".into())
                    .spawn_scoped(scope, move || {
                        worker.lookup_index(key, size, archives, archive, archive, 0)
                    }) {
                    Ok(handle) => handles.push(handle),
                    Err(error) => {
                        spawn_error = Some(ToolkitError::io(error));
                        break;
                    }
                }
            }
            let mut results = Vec::with_capacity(handles.len());
            for handle in handles {
                results.push(handle.join().unwrap_or_else(|_| Err(internal())));
            }
            match spawn_error {
                Some(error) => Err(error),
                None => Ok(results),
            }
        });
        // Every started owner has joined before counters are merged or an error
        // is returned. Workers never publish their route state into the parent.
        let mut parent = self.locked()?;
        for worker in &workers {
            let c = worker.joined_counters();
            parent.requests = parent
                .requests
                .checked_add(c.requests.checked_sub(base_requests).ok_or_else(internal)?)
                .ok_or_else(limit)?;
            parent.transferred = parent
                .transferred
                .checked_add(
                    c.transferred
                        .checked_sub(base_transferred)
                        .ok_or_else(internal)?,
                )
                .ok_or_else(limit)?;
            parent.fragment_loads = parent
                .fragment_loads
                .checked_add(c.fragment_loads)
                .ok_or_else(limit)?;
            self.block_hits
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    n.checked_add(c.block_cache_hits)
                })
                .map_err(|_| limit())?;
        }
        results
    }
}
struct Source<'f, 'a> {
    files: &'f Files<'a>,
    object: String,
    size: i64,
    sparse: bool,
    block: Mutex<Option<(i64, Fragment<'a>)>>,
}
impl Source<'_, '_> {
    fn read_at(&self, out: &mut [u8], offset: i64) -> Result<()> {
        self.files.store.check()?;
        if out.is_empty() {
            return Ok(());
        }
        if offset < 0 || offset > self.size || out.len() as i64 > self.size - offset {
            return Err(range());
        }
        if self.sparse {
            let raw = self
                .files
                .range(&self.object, offset, out.len() as i64, self.size)?;
            out.copy_from_slice(raw.bytes());
            return Ok(());
        }
        let mut block = self.block.lock().map_err(|_| internal())?;
        let mut copied = 0;
        while copied < out.len() {
            self.files.store.check()?;
            let at = offset + copied as i64;
            let base = at / (BLOCK as i64) * (BLOCK as i64);
            if block.as_ref().is_some_and(|(saved, _)| *saved == base) {
                self.files
                    .block_hits
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                    .map_err(|_| limit())?;
            } else {
                // Preserve the old reusable block until replacement succeeds,
                // matching Go's failed-read behavior and true two-block peak.
                let raw = self.files.range(
                    &self.object,
                    base,
                    (BLOCK as i64).min(self.size - base),
                    self.size,
                )?;
                *block = Some((base, raw));
            }
            let bytes = block.as_ref().expect("loaded block").1.bytes();
            let within = (at - base) as usize;
            let n = (out.len() - copied).min(bytes.len() - within);
            if n == 0 {
                return Err(range());
            }
            out[copied..copied + n].copy_from_slice(&bytes[within..within + n]);
            copied += n;
        }
        Ok(())
    }
}
pub(crate) struct Encoded<'f, 'a> {
    source: Source<'f, 'a>,
    offset: i64,
    size: usize,
    position: usize,
}
impl Encoded<'_, '_> {
    pub fn encoded_size(&self) -> usize {
        self.size
    }
    pub fn rewind(&mut self) -> Result<()> {
        self.source.files.store.check()?;
        self.position = 0;
        Ok(())
    }
    pub fn read_at(&self, out: &mut [u8], offset: usize) -> Result<()> {
        offset
            .checked_add(out.len())
            .filter(|end| *end <= self.size)
            .ok_or_else(range)?;
        self.source.read_at(
            out,
            self.offset.checked_add(offset as i64).ok_or_else(range)?,
        )
    }
}
impl crate::blte::EncodedRead for Encoded<'_, '_> {
    fn check_encoded(&self) -> Result<()> {
        self.source.files.store.check()
    }
    fn encoded_size(&self) -> usize {
        self.size
    }
    fn read_encoded(&self, offset: usize, bytes: usize) -> Result<crate::blte::EncodedSpan<'_>> {
        self.source.files.store.check()?;
        offset
            .checked_add(bytes)
            .filter(|end| *end <= self.size)
            .ok_or_else(range)?;
        if bytes > 64 << 20 {
            return Err(fault(
                "container.resource_limit",
                3,
                "Encoded span allocation exceeds bound",
            ));
        }
        crate::blte::EncodedSpan::read_owned(bytes, self.source.files.store.budget, |out| {
            let result = self.read_at(out, offset);
            self.source.files.store.finish(result, Ok(()))
        })
    }
}
impl Read for Encoded<'_, '_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.source
            .files
            .store
            .check()
            .map_err(std::io::Error::other)?;
        // One physical block per Read avoids losing a successful prefix if the
        // following block fails; a later Read returns that unchanged error.
        let boundary = BLOCK - ((self.offset as usize + self.position) % BLOCK);
        let n = out.len().min(self.size - self.position).min(boundary);
        if n == 0 {
            return Ok(0);
        }
        self.read_at(&mut out[..n], self.position)
            .map_err(std::io::Error::other)?;
        self.position += n;
        Ok(n)
    }
}
#[cfg(test)]
mod tests;
