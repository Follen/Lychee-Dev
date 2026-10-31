//! Query-scoped owned Encoding/Root preparation. Temporary range views borrow
//! immutable geometry; no self-reference or whole-Encoding materialization.
use super::{
    admission::AdmittedWorkspace,
    availability_failure,
    cache::{MetadataFetcher, PublicMetadata},
    files::{Files, OwnedEncoded},
    fragments::{self, Fragments, PublicRanges, RangeFetcher},
    invalid, limit,
};
#[cfg(test)]
use crate::metadata_format::Encoding;
use crate::{
    blte,
    casc,
    decoded_cache::Stats,
    fault, keys,
    metadata_format::{self, PreparedEncoding, RootIndex},
    resource::{Budget, Cost},
};
use lycheedev_core::{Result, ToolkitError, evidence, selection::DataPin};
use std::sync::Arc;
mod content;
use content::Materialized;
pub(crate) use content::SharedContent;

pub(super) struct Options {
    pub offline: bool,
    pub key_file: Option<String>,
    pub variant: String,
    pub metadata_bytes: usize,
    pub content_bytes: usize,
}
pub(super) struct File {
    pub reading: serde_json::Value,
    materialized: Materialized,
}
impl File {
    pub fn into_parts(self) -> (Arc<SharedContent>, serde_json::Value) {
        (self.materialized.content, self.reading)
    }
}
/// Kept private to the scoped dispatch. Existing FileBytes consumers continue
/// to receive only materialized File; a verified handle cannot escape by Vec.
#[derive(Clone, Copy)]
enum ContentMode {
    Materialized(blte::DecodePolicy),
    Verified,
}
enum SelectedBody {
    Materialized(Materialized),
    Verified(content::Verified),
}
struct SelectedFile {
    reading: serde_json::Value,
    body: SelectedBody,
}
pub(super) struct VerifiedFile {
    pub reading: serde_json::Value,
    pub blob: evidence::VerifiedBlob,
    _descriptor: crate::resource::OwnedRetainedReservation,
}
impl SelectedBody {
    fn reference(&self) -> &evidence::BlobRef {
        match self {
            Self::Materialized(value) => &value.blob,
            Self::Verified(value) => value.blob.reference(),
        }
    }
    fn encoding_key(&self) -> &str {
        match self {
            Self::Materialized(value) => &value.encoding_key,
            Self::Verified(value) => &value.encoding_key,
        }
    }
}
struct Prepared<'a> {
    index: PreparedEncoding<OwnedEncoded<'a>>,
    root_index: RootIndex<Arc<SharedContent>>,
    root: Materialized,
    root_stats: Stats,
    root_reported: bool,
    decoded: content::Memo,
}
impl metadata_format::RootContent for Arc<SharedContent> {
    fn content(&self) -> &blte::Content {
        self.as_ref()
    }
}
pub(super) struct Session<'a, 'scope> {
    admitted: &'a AdmittedWorkspace<'scope>,
    pin: &'a DataPin,
    options: Options,
    budget: &'a Arc<Budget>,
    keys: Option<keys::Context>,
    prepared: Option<Prepared<'a>>,
    files: Option<Arc<Files<'a>>>,
    metadata: &'a (dyn MetadataFetcher + Sync),
    ranges: &'a dyn RangeFetcher,
    closed: bool,
}
impl<'a, 'scope> Session<'a, 'scope> {
    pub fn borrowed(
        admitted: &'a mut AdmittedWorkspace<'scope>,
        pin: &'a DataPin,
        options: Options,
        budget: &'a Arc<Budget>,
    ) -> Result<Self> {
        Self::borrowed_with(
            admitted,
            pin,
            options,
            budget,
            &PublicMetadata,
            &PublicRanges,
        )
    }
    fn borrowed_with(
        admitted: &'a mut AdmittedWorkspace<'scope>,
        pin: &'a DataPin,
        options: Options,
        budget: &'a Arc<Budget>,
        metadata: &'a (dyn MetadataFetcher + Sync),
        ranges: &'a dyn RangeFetcher,
    ) -> Result<Self> {
        admitted.check()?;
        fragments::data_identity(pin)?;
        if !admitted
            .budget()
            .is_some_and(|original| std::ptr::eq(original, budget.as_ref()))
        {
            return Err(fault(
                "records.file_query",
                4,
                "Session requires its admitted query budget",
            ));
        }
        // Bounds and content-variant admission are intentionally lazy, matching
        // prepareFileSource rather than making definition-only work prepare CASC.
        Ok(Self {
            admitted,
            pin,
            options,
            budget,
            keys: None,
            prepared: None,
            files: None,
            metadata,
            ranges,
            closed: false,
        })
    }
    pub fn reserve_derived_read<'reservation>(
        &'reservation self,
        check: &'reservation dyn Fn() -> Result<()>,
    ) -> Result<lycheedev_core::workspace::data_disk::DerivedReadLease<'reservation>> {
        self.check()?;
        self.admitted.reserve_derived_read(check)
    }
    pub fn reserve_derived_read_owned<'reservation>(
        &'reservation self,
        check: &'reservation lycheedev_core::workspace::data_disk::DerivedReadCheck,
    ) -> Result<lycheedev_core::workspace::data_disk::OwnedDerivedReadLease<'reservation>> {
        self.check()?;
        self.admitted.reserve_derived_read_owned(check)
    }
    pub fn reserve_derived_cache<'reservation>(
        &'reservation self,
        peak: lycheedev_core::workspace::data_disk::AdditionalPeak,
        check: &'reservation dyn Fn() -> Result<()>,
        budget: &'reservation crate::resource::Budget,
    ) -> Result<crate::generation::retirement::Admission<'reservation>> {
        self.check()?;
        self.admitted.reserve_derived_cache(peak, check, budget)
    }
    pub fn check(&self) -> Result<()> {
        self.admitted.check()?;
        if self.closed {
            return Err(fault("records.file_query", 4, "Session is closed"));
        }
        if let Some(prepared) = &self.prepared {
            prepared.index.check()?;
        }
        Ok(())
    }
    fn prepare(&mut self) -> Result<()> {
        self.check()?;
        if self.files.is_some() {
            return Ok(());
        }
        if self.keys.is_none() {
            self.keys = Some(keys::Context::prepare_scoped(
                self.admitted.home(),
                self.options.key_file.as_deref(),
                self.options.offline,
                self.admitted.control(),
                self.budget.deadline,
                Some(self.budget.clone()),
            )?);
        }
        if !["", "standard", "low-violence"].contains(&self.options.variant.as_str()) {
            return Err(file_query());
        }
        if self.options.metadata_bytes == 0
            || self.options.metadata_bytes > 1 << 30
            || self.options.content_bytes == 0
            || self.options.content_bytes as u64 > 1 << 40
        {
            return Err(limit());
        }
        let admitted = self.admitted;
        let (fragments, build) = Fragments::prepare_with(
            admitted.home(),
            self.pin,
            self.options.offline,
            admitted.control(),
            Some(self.budget),
            self.metadata,
            self.ranges,
        )?;
        if build.installed.build_config != self.pin.build_config
            || build.installed.cdn_config != self.pin.cdn_config
        {
            return Err(fault(
                "records.pinned_build_changed",
                4,
                "CDN configuration differs from fixed pin",
            ));
        }
        if build.encoding_bytes > self.options.metadata_bytes
            || build.content_bytes > self.options.metadata_bytes
        {
            return Err(limit());
        }
        self.files = Some(Arc::new(Files::new(
            fragments,
            admitted.download_workers(),
        )?));
        self.check()
    }
    /// Each callback owns complete shared bytes and may request one subsequent
    /// bounded batch. Once any callback ran, errors never replay that callback.
    pub fn read_batches(
        &mut self,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.read_batches_policy(ids, blte::DecodePolicy::Strict, callback)
    }
    pub fn read_table_batches(
        &mut self,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.read_batches_policy(ids, blte::DecodePolicy::TableAllowMissing, callback)
    }
    fn read_batches_policy(
        &mut self,
        ids: &[u32],
        policy: blte::DecodePolicy,
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        let budget = self.budget;
        self.read_selected(ids, ContentMode::Materialized(policy), &mut |selected| {
            let _output = budget.reserve_retained(
                selected
                    .len()
                    .checked_mul(std::mem::size_of::<File>())
                    .ok_or_else(limit)?,
            )?;
            let mut files = Vec::with_capacity(selected.len());
            for selected in selected {
                let SelectedFile {
                    reading,
                    body,
                } = selected;
                match body {
                    SelectedBody::Materialized(materialized) => files.push(File {
                        reading,
                        materialized,
                    }),
                    SelectedBody::Verified(value) => {
                        return value.blob.close_after(Err(internal()));
                    }
                }
            }
            callback(files)
        })
    }
    /// One callback borrows one immutable owner through consumer publication.
    /// The handle is checked-closed before success; a delivered callback is never
    /// repeated for key recovery, close errors, capture errors, or sink failure.
    pub fn with_verified<R>(
        &mut self,
        id: u32,
        callback: &mut dyn FnMut(&mut VerifiedFile) -> Result<R>,
    ) -> Result<R> {
        let mut delivered = None;
        let mut completed_reference = None;
        let result = self.read_selected(&[id], ContentMode::Verified, &mut |mut selected| {
            if selected.len() != 1 {
                return close_selected(selected, Err(internal()));
            }
            let selected = selected.pop().ok_or_else(internal)?;
            let SelectedFile {
                reading,
                body,
            } = selected;
            let SelectedBody::Verified(value) = body else {
                return Err(internal());
            };
            let mut file = VerifiedFile {
                reading,
                blob: value.blob,
                _descriptor: value.descriptor,
            };
            let reference = file.blob.reference().clone();
            let result = callback(&mut file);
            let result = file
                .blob
                .close_after(result)
                .map_err(|error| content::completed_body(error, &reference))?;
            completed_reference = Some(reference);
            delivered = Some(result);
            Ok(Vec::new())
        });
        result.map_err(|error| match &completed_reference {
            Some(reference) => content::completed_body(error, reference),
            None => error,
        })?;
        delivered.ok_or_else(internal)
    }
    fn read_selected(
        &mut self,
        ids: &[u32],
        mode: ContentMode,
        callback: &mut dyn FnMut(Vec<SelectedFile>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.check()?;
        if matches!(mode, ContentMode::Verified) && ids.len() != 1 {
            return Err(internal());
        }
        if ids.is_empty() || ids.len() > 32 || ids.contains(&0) {
            return Err(file_query());
        }
        self.prepare()?;
        let may_load_public = self
            .keys
            .as_ref()
            .is_some_and(|keys| keys.source.kind == "public" && keys.source.state != "loaded");
        recover_batches(ids, may_load_public, |progress, rebuild| {
            self.check()?;
            if rebuild {
                let keys = self.keys.as_mut().expect("prepared keys");
                // Retry only the undelivered batch. Every range view borrows
                // this same query Context at the time of its read.
                if keys.source.state != "loaded" {
                    keys.ensure_public()?;
                }
            }
            self.read_prepared(progress, mode, callback)
        })
    }
    fn prepare_ready(&mut self) -> Result<()> {
        self.check()?;
        if let Some(prepared) = &self.prepared {
            // Prepared Root owns authenticated immutable bytes, independently
            // of its rebuildable disk copy. Later payload/config publications
            // inventory all attributed bodies, so restore this exact missing
            // Root before they run. The stat is only a missing-body hint: Core
            // revalidates SHA/length, attribution, quota peak and the canonical
            // blob lease, and rejects an existing corrupt concurrent winner.
            // The ordinary warm path never rereads/hashes the Root body.
            let path = evidence::blob_path(self.admitted.home(), &prepared.root.blob.sha256)?;
            match std::fs::symlink_metadata(path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.files
                        .as_ref()
                        .expect("prepared files")
                        .store
                        .rebuild_blob(
                            &prepared.root.content.bytes,
                            self.options.metadata_bytes,
                            &prepared.root.blob,
                        )?;
                }
                Err(error) => return Err(ToolkitError::io(error)),
            }
            return self.check();
        }
        let admitted = self.admitted;
        let budget = self.budget;
        let keys = self.keys.as_mut().expect("prepared keys");
        let files = self.files.as_ref().expect("prepared files");
        let build = &files.build;
        let before = files.counters()?;
        let encoded = files.open_owned(
            &build.encoding_key,
            build.encoding_bytes.try_into().map_err(|_| limit())?,
        )?;
        let ranges = blte::PreparedRanges::open(
            encoded,
            blte::Limits {
                encoded_bytes: build.encoding_bytes,
                decoded_bytes: build.content_bytes,
                ..Default::default()
            },
            budget.clone(),
        )?;
        if ranges.size() != build.content_bytes {
            return Err(invalid());
        }
        let limits = casc::Limits {
            encoded_bytes: self.options.metadata_bytes,
            deadline: budget.deadline.min(admitted.control().deadline()),
            ..Default::default()
        };
        let index = PreparedEncoding::open(ranges, keys.keys(), limits, budget.clone())?;
        let root_mapping = index.find(keys.keys(), &build.root_content_key, limits, || {
            admitted.check()
        })?;
        let mut stats = Stats::default();
        let mut decoded = content::Memo::default();
        let root = content::materialize(
            files,
            &index,
            &root_mapping,
            self.options.metadata_bytes,
            true,
            blte::DecodePolicy::Strict,
            limits,
            keys,
            &mut stats,
            budget,
            &mut decoded,
        )?;
        let root_index = RootIndex::open(
            root.content.clone(),
            metadata_format::RootLimits {
                bytes: self.options.metadata_bytes,
                records: 10_000_000,
                groups: 65536,
                matches: 4096,
                operation: limits,
            },
            budget.clone(),
            || admitted.check(),
        )?;
        let counters = files.counters()?;
        stats.fragment_loads = counters
            .fragment_loads
            .checked_sub(before.fragment_loads)
            .ok_or_else(internal)?;
        stats.block_cache_hits = counters
            .block_cache_hits
            .checked_sub(before.block_cache_hits)
            .ok_or_else(internal)?;
        admitted.check()?;
        self.prepared = Some(Prepared {
            index,
            root_index,
            root,
            root_stats: stats,
            root_reported: false,
            decoded,
        });
        Ok(())
    }
    fn read_prepared(
        &mut self,
        progress: &mut BatchProgress,
        mode: ContentMode,
        callback: &mut dyn FnMut(Vec<SelectedFile>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.prepare_ready()?;
        let admitted = self.admitted;
        let budget = self.budget;
        let keys = self.keys.as_mut().expect("prepared keys");
        let files = self.files.as_ref().expect("prepared files");
        let build = &files.build;
        let prepared = self.prepared.as_mut().expect("ready session");
        let index = &prepared.index;
        let root = &prepared.root;
        let limits = casc::Limits {
            encoded_bytes: self.options.metadata_bytes,
            deadline: budget.deadline.min(admitted.control().deadline()),
            ..Default::default()
        };
        let mask = locale(&self.pin.language)?;
        while !progress.pending.is_empty() {
            admitted.check()?;
            progress.validate()?;
            let _output = budget.reserve_retained(
                progress
                    .pending
                    .len()
                    .checked_mul(std::mem::size_of::<SelectedFile>())
                    .ok_or_else(limit)?,
            )?;
            let mut output = Vec::with_capacity(progress.pending.len());
            for id in &progress.pending {
                admitted.check()?;
                let mut stats = if prepared.root_reported {
                    Stats::default()
                } else {
                    prepared.root_stats.clone()
                };
                let before = files.counters()?;
                let entries = prepared.root_index.lookup(*id, || files.store.check())?;
                let matching = entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| {
                        entry.file_data_id == *id
                            && entry.locale_mask & mask != 0
                            && (self.options.variant != "standard"
                                || entry.content_flags & 0x80 == 0)
                            && (self.options.variant != "low-violence"
                                || entry.content_flags & 0x80 != 0)
                    })
                    .map(|(at, _)| at)
                    .collect::<Vec<_>>();
                if matching.len() != 1 {
                    return Err(fault(
                        if matching.is_empty() {
                            "records.content_missing"
                        } else {
                            "records.file_ambiguous"
                        },
                        if matching.is_empty() { 3 } else { 4 },
                        "File locale/content variant does not select exactly one Root record",
                    ));
                }
                let entry = entries[matching[0]].clone();
                let mapping =
                    index.find(keys.keys(), &entry.content_key, limits, || admitted.check())?;
                let content = match mode {
                    ContentMode::Materialized(policy) => {
                        SelectedBody::Materialized(content::materialize(
                            files,
                            index,
                            &mapping,
                            self.options.content_bytes,
                            false,
                            policy,
                            limits,
                            keys,
                            &mut stats,
                            budget,
                            &mut prepared.decoded,
                        )?)
                    }
                    ContentMode::Verified => SelectedBody::Verified(content::read_verified(
                        files,
                        index,
                        &mapping,
                        self.options.content_bytes,
                        limits,
                        keys,
                        &mut stats,
                        budget,
                    )?),
                };
                // Go publishes both configurations after each extracted body.
                // Close a streaming owner on every failure after its publication.
                let after_content = (|| {
                    let build_blob = files
                        .store
                        .publish_blob(&build.build_document.raw, 4 << 20)?;
                    let cdn_blob = files.store.publish_blob(&build.cdn_document.raw, 4 << 20)?;
                    admitted.check()?;
                    let after = files.counters()?;
                    stats.fragment_loads = stats
                        .fragment_loads
                        .checked_add(
                            after
                                .fragment_loads
                                .checked_sub(before.fragment_loads)
                                .ok_or_else(internal)?,
                        )
                        .ok_or_else(internal)?;
                    stats.block_cache_hits = stats
                        .block_cache_hits
                        .checked_add(
                            after
                                .block_cache_hits
                                .checked_sub(before.block_cache_hits)
                                .ok_or_else(internal)?,
                        )
                        .ok_or_else(internal)?;
                    let mut reading = serde_json::json!({"contentVerified":true,"source":"cdn","pin":self.pin,"buildConfiguration":build_blob,"cdnConfiguration":cdn_blob,"encodingKey":build.encoding_key,"root":root.blob,"entry":entry,"payloadEncodingKey":content.encoding_key(),"content":content.reference(),"verification":{"encodingFramingEKey":true,"encodingRequestedPagesMD5":true,"encodingWholeDecodedCKey":false,"rootWholeDecodedCKey":true,"contentWholeDecodedCKey":true},"keySource":keys.value(),"decodedCache":stats});
                    if let SelectedBody::Materialized(materialized) = &content {
                        crate::query_source::report_partial(
                            &mut reading,
                            &materialized.content.missing,
                        )?;
                    }
                    Ok(reading)
                })();
                let reading = match after_content {
                    Ok(reading) => reading,
                    Err(error) => {
                        output.push(SelectedFile {
                            reading: serde_json::Value::Null,
                            body: content,
                        });
                        return close_selected(output, Err(error));
                    }
                };
                // First successful FileReading consumes root statistics even
                // when the caller's following callback subsequently fails.
                prepared.root_reported = true;
                output.push(SelectedFile {
                    reading,
                    body: content,
                });
            }
            if let Err(error) = admitted.check() {
                return close_selected(output, Err(error));
            }
            progress.deliver(|| callback(output))?;
            admitted.check()?;
        }
        admitted.check()
    }
    pub fn close(mut self) -> Result<()> {
        self.closed = true;
        self.prepared = None;
        self.files = None;
        self.keys = None;
        // The external root/lease owner is only borrowed, never closed here.
        self.admitted.check()
    }
}

/// Progress survives one public-key snapshot replacement. Only a batch whose
/// callback completed advances the cursor; earlier callbacks are never replayed.
struct BatchProgress {
    pending: Vec<u32>,
    delivered: usize,
    callback_failed: bool,
}
impl BatchProgress {
    fn validate(&self) -> Result<()> {
        if self.pending.contains(&0)
            || self
                .delivered
                .checked_add(self.pending.len())
                .is_none_or(|n| n > 32)
        {
            return Err(file_query());
        }
        Ok(())
    }
    fn deliver(&mut self, callback: impl FnOnce() -> Result<Vec<u32>>) -> Result<()> {
        self.validate()?;
        self.callback_failed = true;
        let pending = callback()?;
        self.callback_failed = false;
        self.delivered += self.pending.len();
        self.pending = pending;
        Ok(())
    }
}
/// A missing key may precede a private stage write, but a published/unknown
/// attempt or a composite cleanup/stop error must never be replayed.
fn recoverable_missing_key(error: &ToolkitError) -> bool {
    if error.code != "container.key_unavailable" || error.details["missingKey"] != true {
        return false;
    }
    let Some(details) = error.details.as_object() else {
        return false;
    };
    if details
        .keys()
        .any(|key| !matches!(key.as_str(), "missingKey" | "keyId" | "publication"))
    {
        return false;
    }
    details.get("publication").is_none_or(|publication| {
        publication["state"] == "not_published"
            && publication["reused"] != true
            && publication
                .get("prior")
                .is_none_or(serde_json::Value::is_null)
    })
}
fn recover_batches(
    ids: &[u32],
    may_load_public: bool,
    mut run: impl FnMut(&mut BatchProgress, bool) -> Result<()>,
) -> Result<()> {
    let mut progress = BatchProgress {
        pending: ids.to_vec(),
        delivered: 0,
        callback_failed: false,
    };
    let first = run(&mut progress, false);
    match first {
        Err(error)
            if recoverable_missing_key(&error) && !progress.callback_failed && may_load_public =>
        {
            run(&mut progress, true)
        }
        result => result,
    }
}
fn internal() -> ToolkitError {
    fault(
        "records.internal",
        8,
        "Selected body mode invariant differs",
    )
}
/// An authenticated body can already be published before the final source
/// check fails. Close every undelivered handle while retaining the primary
/// failure; do not let an early callback boundary turn this into silent Drop.
fn close_selected<T>(selected: Vec<SelectedFile>, mut result: Result<T>) -> Result<T> {
    for selected in selected.into_iter().rev() {
        if let SelectedBody::Verified(value) = selected.body {
            let reference = value.blob.reference().clone();
            result = value
                .blob
                .close_after(result)
                .map_err(|error| content::completed_body(error, &reference));
        }
    }
    result
}
fn file_query() -> ToolkitError {
    fault(
        "records.file_query",
        2,
        "Require 1..32 positive file IDs across all batches",
    )
}
fn locale(language: &str) -> Result<u32> {
    Ok(match language {
        "enUS" => 2,
        "koKR" => 4,
        "frFR" => 0x10,
        "deDE" => 0x20,
        "zhCN" => 0x40,
        "esES" => 0x80,
        "zhTW" => 0x100,
        "enGB" => 0x200,
        "esMX" => 0x1000,
        "ruRU" => 0x2000,
        "ptBR" => 0x4000,
        "itIT" => 0x8000,
        "ptPT" => 0x10000,
        _ => {
            return Err(fault(
                "selection.unsupported_data_language",
                2,
                "Unknown data locale",
            ));
        }
    })
}
#[cfg(test)]
mod tests;
