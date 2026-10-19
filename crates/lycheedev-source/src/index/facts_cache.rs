//! Bounded, immutable cohorts of complete public Facts. A cohort is at most 64
//! ordered Git inputs within a stable path leaf, never a global key catalog.
//! Missing/corrupt cache bytes
//! recover from the caller's authenticated input, not from another snapshot.
use super::{
    Document, Facts, Phase, Profile, decode_facts, encode_facts, facts_cache_key, facts_pipeline,
    git, parse_document_facts,
};
use lycheedev_core::{
    Result, ToolkitError,
    cache::{SourceCachePublicationPhase, SourceFactsCache, SourceFactsReader, SourceFactsWriter},
    control::OperationControl,
    metadata::historical_write::finish_historical,
    storage,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};

const MAGIC: &[u8; 8] = b"LYCFPK01";
const HEADER: usize = 68;
const MAXIMUM: usize = 32 << 20;
const GROUP: usize = 64;
// An insertion/deletion may repartition one path leaf, never the corpus suffix.
// Packs inside that leaf keep the original 64-record and 32 MiB bounds.
const LEAF: usize = 512;
// Locator has only fixed arrays/integers (no String/Vec capacity). The document
// Vec is reserved once at its known final count, before workers start; its real
// capacity * 256 is held in the same 256 MiB admission throughout the invocation.
pub(super) const LOCATOR_RETAINED_BYTES: usize = 256;
// One exactly reserved 64 * 80-byte record directory, 64 usize input indices,
// fixed hashes, group/owner names and bookkeeping. No aggregate body Vec.
pub(super) const GROUP_RETAINED_BYTES: usize = 16 << 10;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Locator {
    #[serde(with = "digest_json")]
    pub group: [u8; 32],
    pub offset: u64,
    pub bytes: u32,
    #[serde(with = "digest_json")]
    pub sha256: [u8; 32],
}
fn hex(bytes: &[u8; 32]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in bytes {
        value.push(DIGITS[(byte >> 4) as usize] as char);
        value.push(DIGITS[(byte & 15) as usize] as char);
    }
    value
}
fn digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut output = [0; 32];
    for (i, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let digit = |b| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        };
        output[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(output)
}
mod digest_json {
    use serde::{
        Deserializer, Serializer,
        de::{Error, Visitor},
    };
    pub fn serialize<S: Serializer>(value: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::hex(value))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 32], D::Error> {
        struct Hex;
        impl Visitor<'_> for Hex {
            type Value = [u8; 32];
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("one lowercase SHA-256 digest")
            }
            fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
                super::digest(value).ok_or_else(|| E::custom("invalid Facts locator digest"))
            }
        }
        deserializer.deserialize_str(Hex)
    }
}
#[derive(Clone, Copy)]
struct Record {
    key: [u8; 32],
    offset: u64,
    bytes: u32,
    sha256: [u8; 32],
}
fn record(raw: &[u8; HEADER], offset: u64, total: u64) -> Option<Record> {
    let bytes = u32::from_le_bytes(raw[..4].try_into().ok()?);
    if bytes == 0
        || bytes as usize > MAXIMUM
        || offset < MAGIC.len() as u64
        || offset
            .checked_add(HEADER as u64)?
            .checked_add(bytes as u64)?
            > total
    {
        return None;
    }
    Some(Record {
        key: raw[4..36].try_into().ok()?,
        offset,
        bytes,
        sha256: raw[36..].try_into().ok()?,
    })
}
fn directory(
    reader: &mut SourceFactsReader,
    control: &OperationControl,
) -> Result<Option<Vec<Record>>> {
    let mut magic = [0; 8];
    if !reader.read_at(0, &mut magic, control)? || &magic != MAGIC {
        return Ok(None);
    }
    let mut records = Vec::new();
    records.try_reserve_exact(GROUP).map_err(|_| budget())?;
    if records.capacity() * size_of::<Record>() > GROUP_RETAINED_BYTES / 2 {
        return Err(budget());
    }
    let mut offset = MAGIC.len() as u64;
    while offset < reader.len() {
        if records.len() == GROUP {
            return Ok(None);
        }
        let mut header = [0; HEADER];
        if !reader.read_at(offset, &mut header, control)? {
            return Ok(None);
        }
        let Some(next) = record(&header, offset, reader.len()) else {
            return Ok(None);
        };
        if records.iter().any(|r: &Record| r.key == next.key) {
            return Ok(None);
        }
        offset += HEADER as u64 + next.bytes as u64;
        records.push(next);
    }
    Ok((!records.is_empty()).then_some(records))
}
fn read_record(
    reader: &mut SourceFactsReader,
    item: Record,
    sha256: &str,
    control: &OperationControl,
) -> Result<Option<Facts>> {
    let mut raw = Vec::new();
    raw.try_reserve_exact(item.bytes as usize).map_err(|_| {
        ToolkitError::new("vault.blob_limit", "body allocation was not admitted", 4)
    })?;
    raw.resize(item.bytes as usize, 0);
    if !reader.read_at(item.offset + HEADER as u64, &mut raw, control)?
        || <[u8; 32]>::from(Sha256::digest(&raw)) != item.sha256
    {
        return Ok(None);
    }
    decode_facts(&raw, sha256, control)
}
pub(super) fn read_locator(
    home: &Path,
    locator: &Locator,
    path: &str,
    sha256: &str,
    control: &OperationControl,
) -> Result<Option<Facts>> {
    let cache = SourceFactsCache::open(home, control)?;
    let result = (|| {
        let Some(mut reader) = cache.open_reader(&hex(&locator.group), control)? else {
            return Ok(None);
        };
        let result = (|| {
            let mut magic = [0; 8];
            let mut header = [0; HEADER];
            if !reader.read_at(0, &mut magic, control)?
                || &magic != MAGIC
                || !reader.read_at(locator.offset, &mut header, control)?
            {
                return Ok(None);
            }
            let Some(item) = record(&header, locator.offset, reader.len()) else {
                return Ok(None);
            };
            if item.bytes != locator.bytes
                || item.sha256 != locator.sha256
                || Some(item.key) != digest(&facts_cache_key(path, sha256))
            {
                return Ok(None);
            }
            read_record(&mut reader, item, sha256, control)
        })();
        reader.finish(result, control)
    })();
    cache.finish(result, control)
}
fn budget() -> ToolkitError {
    ToolkitError::new(
        "codebase.source_budget",
        "Facts cohort metadata was not admitted",
        3,
    )
}

// Source paths are nonempty UTF-8 with no NUL. Treat their byte strings as
// NUL-terminated binary keys: a proper prefix sorts before its continuation,
// including when the discriminating bit lies inside that terminal zero byte.
// Equal paths share every prefix; tree admission supplies unique Git paths.
fn shared_path_bits(left: &str, right: &str) -> usize {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut at = 0;
    while at < left.len().min(right.len()) && left[at] == right[at] {
        at += 1;
    }
    let difference = left.get(at).copied().unwrap_or(0) ^ right.get(at).copied().unwrap_or(0);
    if difference == 0 {
        usize::MAX
    } else {
        at * 8 + difference.leading_zeros() as usize
    }
}

// Find the next maximal <=512 leaf of the full binary path trie without
// constructing that trie or retaining a corpus-sized directory. The previous
// leaf's last path excludes already consumed leaves; the 513th future eligible
// path excludes every oversized ancestor. Their longest common prefix with the
// first path therefore gives exactly the next leaf. A second bounded scan finds
// its end. Assets and oversized inputs do not participate in the partition.
fn leaf_end(
    files: &[git::TreeFile],
    start: usize,
    previous: Option<usize>,
    control: &OperationControl,
) -> Result<usize> {
    let mut first = None;
    let mut previous_path = previous.map(|at| files[at].path.as_str());
    let mut count = 0;
    let mut prefix = 0;
    let mut end = files.len();
    for (at, file) in files.iter().enumerate().skip(start) {
        control.check()?;
        if file.asset || file.oversized {
            continue;
        }
        if previous_path.is_some_and(|path| path.as_bytes() >= file.path.as_bytes()) {
            return Err(ToolkitError::invalid("Facts path leaf input order differs"));
        }
        previous_path = Some(&file.path);
        let first = *first.get_or_insert(at);
        if count == 0 {
            if let Some(previous) = previous {
                prefix = shared_path_bits(&files[previous].path, &file.path) + 1;
            }
        } else if count == LEAF {
            prefix = prefix.max(shared_path_bits(&files[first].path, &file.path) + 1);
            end = at;
            break;
        }
        count += 1;
    }
    let Some(first) = first else {
        return Err(budget());
    };
    for (at, file) in files.iter().enumerate().take(end).skip(first + 1) {
        control.check()?;
        if !file.asset
            && !file.oversized
            && shared_path_bits(&files[first].path, &file.path) < prefix
        {
            return Ok(at);
        }
    }
    Ok(end)
}

fn observe(profile: &Profile, phase: SourceCachePublicationPhase, elapsed: Duration) {
    use SourceCachePublicationPhase as P;
    profile.record(
        match phase {
            P::Lock => Phase::FactsPublishLock,
            P::Stage => Phase::FactsPublishStage,
            P::Write => Phase::FactsPublishWrite,
            P::Seal => Phase::FactsPublishSeal,
            P::Rename => Phase::FactsPublishRename,
            P::StageClose => Phase::FactsPublishStageClose,
            P::LockClose => Phase::FactsPublishLockClose,
            P::LockVerify => Phase::FactsPublishLockVerify,
            P::LockOpen => Phase::FactsPublishLockOpen,
            P::LockKernelAttempt => Phase::FactsPublishLockKernelAttempt,
            P::LockContentionBackoff => Phase::FactsPublishLockContentionBackoff,
            P::LockFailureClose => Phase::FactsPublishLockFailureClose,
        },
        elapsed,
    );
}
type Observer<'a> = Option<&'a mut dyn FnMut(SourceCachePublicationPhase, Duration)>;
fn published<T>(profile: &Profile, run: impl FnOnce(Observer<'_>) -> Result<T>) -> Result<T> {
    let _phase = profile.span(Phase::FactsPublish);
    let mut observer = |phase, elapsed| observe(profile, phase, elapsed);
    run(if profile.is_enabled() {
        Some(&mut observer)
    } else {
        None
    })
}
struct GroupState<'a> {
    key: [u8; 32],
    inputs: [usize; GROUP],
    count: usize,
    seen: usize,
    // Only fixed metadata for read-ahead misses; decoded hits never queue.
    admitted: usize,
    missing: u64,
    first_document: usize,
    reader: Option<SourceFactsReader>,
    records: Vec<Record>,
    writer: Option<SourceFactsWriter<'a>>,
    offset: usize,
    overflow: bool,
}
pub(super) struct Groups<'a> {
    cache: &'a SourceFactsCache,
    files: &'a [git::TreeFile],
    control: &'a OperationControl,
    profile: &'a Profile,
    next_file: usize,
    leaf_end: usize,
    previous_file: Option<usize>,
    group: Option<GroupState<'a>>,
    documents: usize,
    admission_error: Option<(usize, ToolkitError)>,
}
/// Both callbacks borrow the same request-owned Groups; no RefCell, mutex or
/// independently advancing cache catalog is needed by the pipeline.
pub(super) struct CacheConsumer<'g, 'a, F> {
    groups: &'g mut Groups<'a>,
    consume: F,
}
impl<'a, F> facts_pipeline::Consumer for CacheConsumer<'_, 'a, F>
where
    F: FnMut(
        &mut Groups<'a>,
        &git::TreeFile,
        Vec<u8>,
        Option<facts_pipeline::PreparedFacts>,
    ) -> Result<()>,
{
    fn admit(
        &mut self,
        file: &git::TreeFile,
        data: &[u8],
        ordered: bool,
    ) -> facts_pipeline::Admission {
        self.groups.admit(file, data, ordered)
    }
    fn consume(
        &mut self,
        file: &git::TreeFile,
        data: Vec<u8>,
        prepared: Option<facts_pipeline::PreparedFacts>,
    ) -> Result<()> {
        (self.consume)(self.groups, file, data, prepared)
    }
}
impl<'a> Groups<'a> {
    pub fn new(
        cache: &'a SourceFactsCache,
        files: &'a [git::TreeFile],
        control: &'a OperationControl,
        profile: &'a Profile,
    ) -> Self {
        Self {
            cache,
            files,
            control,
            profile,
            next_file: 0,
            leaf_end: 0,
            previous_file: None,
            group: None,
            documents: 0,
            admission_error: None,
        }
    }
    pub fn consumer<F>(&mut self, consume: F) -> CacheConsumer<'_, 'a, F>
    where
        F: FnMut(
            &mut Groups<'a>,
            &git::TreeFile,
            Vec<u8>,
            Option<facts_pipeline::PreparedFacts>,
        ) -> Result<()>,
    {
        CacheConsumer {
            groups: self,
            consume,
        }
    }
    fn admit(
        &mut self,
        file: &git::TreeFile,
        data: &[u8],
        ordered: bool,
    ) -> facts_pipeline::Admission {
        use facts_pipeline::Admission;
        // Assets, invalid UTF-8 and non-worker inputs keep their full original
        // ordered path. They cannot move the cohort admission cursor ahead.
        if !facts_pipeline::eligible(file, data) {
            return Admission::Barrier;
        }
        if self
            .group
            .as_ref()
            .is_none_or(|group| group.admitted == group.count)
        {
            if !ordered {
                return Admission::Drain;
            }
            if let Err(error) = self.begin(self.documents) {
                self.admission_error = Some((self.documents, error));
                return Admission::Barrier;
            }
        }
        let group = self.group.as_mut().unwrap();
        if self.files[group.inputs[group.admitted]].path != file.path {
            self.admission_error = Some((
                group.first_document + group.admitted,
                ToolkitError::invalid("Facts cohort admission order differs"),
            ));
            return Admission::Barrier;
        }
        // A directory is only a possible hit. The actual record, source hash,
        // mode-specific key and complete envelope are still authenticated at
        // consume, after earlier documents have published successfully.
        if !group.records.is_empty() {
            return Admission::Barrier;
        }
        let _phase = self.profile.span(Phase::FactsRead);
        let key = facts_cache_key(&file.path, &storage::sha256(data));
        match self.cache.open_reader(&key, self.control) {
            Ok(None) => {
                // An authenticated absence is a bounded miss ticket, not a
                // cached Facts value. The original source owner moves to the
                // pure worker; publication remains in document order.
                group.missing |= 1 << group.admitted;
                group.admitted += 1;
                Admission::Parse
            }
            Ok(Some(reader)) => {
                // Presence is only a hint. Close this probe now, then let the
                // original read consume and authenticate the complete current
                // file at its ordered turn, including concurrent replacement
                // or same-handle length changes. No loose owner or body queues.
                if let Err(error) = reader.finish(Ok(()), self.control) {
                    self.admission_error = Some((group.first_document + group.admitted, error));
                }
                Admission::Barrier
            }
            Err(error) => {
                // Pipeline drains earlier documents first; document() returns
                // this only after the CURRENT original prelude has succeeded.
                self.admission_error = Some((group.first_document + group.admitted, error));
                Admission::Barrier
            }
        }
    }
    fn begin(&mut self, first_document: usize) -> Result<()> {
        let _phase = self.profile.span(Phase::FactsRead);
        // A full 64-record pack can stop before skipped trailing rows of its
        // leaf. Advance those rows before deciding whether a new leaf starts;
        // otherwise an asset between leaves would produce an empty cohort.
        while self.next_file < self.files.len()
            && (self.files[self.next_file].asset || self.files[self.next_file].oversized)
        {
            self.control.check()?;
            self.next_file += 1;
        }
        if self.next_file >= self.leaf_end {
            self.leaf_end = leaf_end(self.files, self.next_file, self.previous_file, self.control)?;
        }
        let mut hash = Sha256::new();
        hash.update(b"facts-pack-v2-path-radix512\0facts-layout-v13-postcard-1.1.3\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0");
        let mut inputs = [0; GROUP];
        let mut count = 0;
        while self.next_file < self.leaf_end && count < GROUP {
            self.control.check()?;
            let at = self.next_file;
            self.next_file += 1;
            let file = &self.files[at];
            if file.asset || file.oversized {
                continue;
            }
            inputs[count] = at;
            self.previous_file = Some(at);
            count += 1;
            for value in [file.path.as_bytes(), file.object.as_bytes()] {
                hash.update((value.len() as u64).to_le_bytes());
                hash.update(value);
            }
            hash.update((file.bytes as u64).to_le_bytes());
        }
        if count == 0 {
            return Err(budget());
        }
        let key: [u8; 32] = hash.finalize().into();
        // Store ownership before any later check or frame decode can fail.
        self.group = Some(GroupState {
            key,
            inputs,
            count,
            seen: 0,
            admitted: 0,
            missing: 0,
            first_document,
            reader: None,
            records: Vec::new(),
            writer: None,
            offset: MAGIC.len(),
            overflow: false,
        });
        let group = self.group.as_mut().unwrap();
        group.reader = self.cache.open_reader(&hex(&key), self.control)?;
        if let Some(reader) = &mut group.reader {
            if let Some(records) = directory(reader, self.control)? {
                group.records = records;
            } else {
                group.reader.take().unwrap().finish(Ok(()), self.control)?;
            }
        }
        Ok(())
    }
    pub fn document(
        &mut self,
        file: &git::TreeFile,
        raw: &[u8],
        sha256: &str,
        prepared: Option<facts_pipeline::PreparedFacts>,
        documents: &mut [Document],
    ) -> Result<(Facts, bool, Option<Locator>)> {
        self.control.check()?;
        if self
            .admission_error
            .as_ref()
            .is_some_and(|(ordinal, _)| *ordinal == documents.len())
        {
            return Err(self.admission_error.take().unwrap().1);
        }
        if self.group.is_none() {
            self.begin(documents.len())?;
        }
        let group = self.group.as_mut().unwrap();
        if self.files[group.inputs[group.seen]].path != file.path {
            return Err(ToolkitError::invalid("Facts cohort input order differs"));
        }
        let key = digest(&facts_cache_key(&file.path, sha256)).unwrap();
        let (cached, mut locator) = {
            let _phase = self.profile.span(Phase::FactsRead);
            let mut cached = None;
            let mut locator = None;
            if let Some(item) = group.records.iter().find(|item| item.key == key).copied()
                && let Some(reader) = &mut group.reader
            {
                cached = read_record(reader, item, sha256, self.control)?;
                if cached.is_some() {
                    locator = Some(Locator {
                        group: group.key,
                        offset: item.offset,
                        bytes: item.bytes,
                        sha256: item.sha256,
                    });
                }
            }
            if cached.is_none()
                && group.missing & (1 << group.seen) == 0
                && let Some(raw) = self.cache.read(&hex(&key), self.control)?
            {
                cached = decode_facts(&raw, sha256, self.control)?;
            }
            (cached, locator)
        };
        let cache_hit = cached.is_some();
        let facts = match cached {
            Some(facts) => facts,
            None => {
                parse_document_facts(&file.path, raw, self.control, Some(self.profile), prepared)?
            }
        };
        if !cache_hit && group.writer.is_none() && !group.overflow {
            group.writer = Some(published(self.profile, |observer| {
                self.cache
                    .begin_write(&hex(&group.key), self.control, observer)
            })?);
            published(self.profile, |observer| {
                group
                    .writer
                    .as_mut()
                    .unwrap()
                    .write(MAGIC, self.control, observer)
            })?;
            // Earlier cache hits are already retained in Index, so a partial
            // repair can backfill them without buffering a second group body.
            for earlier in &mut documents[group.first_document..] {
                earlier.facts_locator = append(
                    self.cache,
                    group,
                    &earlier.path,
                    &earlier.sha256,
                    &earlier.facts,
                    self.control,
                    self.profile,
                )?;
            }
        }
        if group.writer.is_some() || group.overflow {
            locator = append(
                self.cache,
                group,
                &file.path,
                sha256,
                &facts,
                self.control,
                self.profile,
            )?;
        }
        group.seen += 1;
        group.admitted = group.admitted.max(group.seen);
        self.documents += 1;
        if group.seen == group.count {
            self.close_group(Ok(()))?;
        }
        Ok((facts, cache_hit, locator))
    }
    fn close_group<T>(&mut self, result: Result<T>) -> Result<T> {
        let Some(mut group) = self.group.take() else {
            return result;
        };
        let mut result = result;
        if result.is_ok() && group.seen != group.count {
            result = Err(ToolkitError::invalid("Facts cohort input count differs"));
        }
        if let Some(writer) = group.writer.take() {
            result = if result.is_ok() && group.seen == group.count {
                finish_historical(
                    result,
                    published(self.profile, |observer| {
                        writer.commit(self.control, observer)
                    }),
                )
            } else {
                published(self.profile, |observer| {
                    writer.abort(result, self.control, observer)
                })
            };
        }
        if let Some(reader) = group.reader.take() {
            let _phase = self.profile.span(Phase::FactsRead);
            result = reader.finish(result, self.control);
        }
        result
    }
    pub fn finish<T>(mut self, result: Result<T>) -> Result<T> {
        self.close_group(result)
    }
}
fn append(
    cache: &SourceFactsCache,
    group: &mut GroupState<'_>,
    path: &str,
    sha256: &str,
    facts: &Facts,
    control: &OperationControl,
    profile: &Profile,
) -> Result<Option<Locator>> {
    let envelope = encode_facts(facts, sha256, Some(profile))?;
    let next = group
        .offset
        .checked_add(HEADER)
        .and_then(|n| n.checked_add(envelope.len()));
    if !group.overflow && next.is_none_or(|n| n > MAXIMUM) {
        // Commit the complete prefix. The current/remaining records keep the
        // original per-file contract, including its unchanged 32 MiB limit.
        if let Some(writer) = group.writer.take() {
            if group.offset > MAGIC.len() {
                published(profile, |observer| writer.commit(control, observer))?;
            } else {
                published(profile, |observer| writer.abort(Ok(()), control, observer))?;
            }
        }
        group.overflow = true;
    }
    let key = facts_cache_key(path, sha256);
    if group.overflow {
        published(profile, |observer| match observer {
            Some(observer) => cache.publish_profiled(&key, &envelope, control, observer),
            None => cache.publish(&key, &envelope, control),
        })?;
        return Ok(None);
    }
    let body_hash: [u8; 32] = Sha256::digest(&envelope).into();
    let mut header = [0; HEADER];
    header[..4].copy_from_slice(&(envelope.len() as u32).to_le_bytes());
    header[4..36].copy_from_slice(&digest(&key).unwrap());
    header[36..].copy_from_slice(&body_hash);
    let locator = Locator {
        group: group.key,
        offset: group.offset as u64,
        bytes: envelope.len() as u32,
        sha256: body_hash,
    };
    published(profile, |mut observer| {
        let writer = group.writer.as_mut().expect("private cohort stage");
        match observer.as_mut() {
            Some(observer) => {
                writer.write(&header, control, Some(&mut **observer))?;
                writer.write(&envelope, control, Some(&mut **observer))
            }
            None => {
                writer.write(&header, control, None)?;
                writer.write(&envelope, control, None)
            }
        }
    })?;
    group.offset = next.unwrap();
    Ok(Some(locator))
}

#[cfg(test)]
mod tests;
