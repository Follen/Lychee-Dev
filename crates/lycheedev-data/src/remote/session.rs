//! Scoped remote provider. The stack owns Encoded→Ranges→Encoding through all
//! callback batches, avoiding self-reference and whole-Encoding materialization.
use super::{
    admission::AdmittedWorkspace,
    availability_failure,
    cache::{MetadataFetcher, PublicMetadata},
    files::Files,
    fragments::{self, Fragments, PublicRanges, RangeFetcher},
    invalid, limit,
};
use crate::{
    blte::{self, ByteSource},
    casc,
    decoded_cache::Stats,
    fault, keys,
    metadata_format::{self, Encoding},
    resource::{Budget, Cost},
};
use lycheedev_core::{Result, ToolkitError, evidence, selection::DataPin};
use std::{borrow::Cow, sync::Arc};
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
    pub entry: metadata_format::RootRecord,
    pub reading: serde_json::Value,
    materialized: Materialized,
}
impl File {
    pub fn into_parts(self) -> (Arc<SharedContent>, serde_json::Value) {
        (self.materialized.content, self.reading)
    }
    pub fn bytes(&self) -> &[u8] {
        &self.materialized.content.bytes
    }
    pub fn reference(&self) -> &evidence::BlobRef {
        &self.materialized.blob
    }
}
/// Kept private to the scoped dispatch. Existing FileBytes consumers continue
/// to receive only materialized File; a verified handle cannot escape by Vec.
#[derive(Clone, Copy)]
enum ContentMode {
    Materialized,
    Verified,
}
enum SelectedBody {
    Materialized(Materialized),
    Verified(content::Verified),
}
struct SelectedFile {
    entry: metadata_format::RootRecord,
    reading: serde_json::Value,
    body: SelectedBody,
}
pub(super) struct VerifiedFile {
    pub entry: metadata_format::RootRecord,
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
pub(super) struct Session<'a, 'scope> {
    admitted: &'a AdmittedWorkspace<'scope>,
    pin: &'a DataPin,
    options: Options,
    budget: &'a Arc<Budget>,
    keys: Option<keys::Context>,
    files: Option<Files<'a>>,
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
            files: None,
            metadata,
            ranges,
            closed: false,
        })
    }
    pub fn check(&self) -> Result<()> {
        self.admitted.check()?;
        if self.closed {
            return Err(fault("records.file_query", 4, "Session is closed"));
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
        self.files = Some(Files::new(fragments, admitted.download_workers())?);
        self.check()
    }
    /// Each callback owns complete shared bytes and may request one subsequent
    /// bounded batch. Once any callback ran, errors never replay that callback.
    pub fn read_batches(
        &mut self,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        let budget = self.budget;
        self.read_selected(ids, ContentMode::Materialized, &mut |selected| {
            let _output = budget.reserve_retained(
                selected
                    .len()
                    .checked_mul(std::mem::size_of::<File>())
                    .ok_or_else(limit)?,
            )?;
            let mut files = Vec::with_capacity(selected.len());
            for selected in selected {
                let SelectedFile {
                    entry,
                    reading,
                    body,
                } = selected;
                match body {
                    SelectedBody::Materialized(materialized) => files.push(File {
                        entry,
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
                entry,
                reading,
                body,
            } = selected;
            let SelectedBody::Verified(value) = body else {
                return Err(internal());
            };
            let mut file = VerifiedFile {
                entry,
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
                // Content may already have loaded the document while the old
                // Encoding stack still borrowed its empty snapshot.
                if keys.source.state != "loaded" {
                    keys.ensure_public()?;
                }
            }
            self.read_prepared(progress, mode, callback)
        })
    }
    fn read_prepared(
        &mut self,
        progress: &mut BatchProgress,
        mode: ContentMode,
        callback: &mut dyn FnMut(Vec<SelectedFile>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        let admitted = self.admitted;
        let budget = self.budget;
        let keys = self.keys.as_mut().expect("prepared keys");
        let files = self.files.as_ref().expect("prepared files");
        let build = &files.build;
        // Immutable range keys must outlive Encoding while content can lazily
        // load public keys. Admit the actual snapshot including map/node slack.
        let snapshot_bytes = keys
            .keys()
            .iter()
            .try_fold(0usize, |sum, (_, value)| sum.checked_add(128 + value.len()))
            .ok_or_else(limit)?;
        let _key_snapshot = budget.reserve_retained(snapshot_bytes)?;
        admitted.check()?;
        let range_keys = keys.keys().clone();
        admitted.check()?;
        let encoded = files.open(
            &build.encoding_key,
            build.encoding_bytes.try_into().map_err(|_| limit())?,
        )?;
        let ranges = blte::Ranges::open_reader_budgeted(
            &encoded,
            blte::Limits {
                encoded_bytes: build.encoding_bytes,
                decoded_bytes: build.content_bytes,
                ..Default::default()
            },
            &range_keys,
            budget,
        )?;
        if ranges.size() != build.content_bytes {
            return Err(invalid());
        }
        let metered = MetadataSource {
            inner: &ranges,
            files,
            budget,
        };
        let limits = casc::Limits {
            encoded_bytes: self.options.metadata_bytes,
            deadline: budget.deadline.min(admitted.control().deadline()),
            ..Default::default()
        };
        // Each metadata check point already runs the original scope callback.
        // Exporting its writable AtomicBool would irreversibly discard ordered
        // cancellation provenance even though the parser only reads the token.
        let index = Encoding::open_with_check(&metered, limits, None, || files.store.check())?;
        let root_mapping =
            index.find_with_check(&build.root_content_key, limits, None, || admitted.check())?;
        let mut stats = Stats::default();
        let mut decoded = content::Memo::default();
        let root = content::materialize(
            files,
            &index,
            &root_mapping,
            self.options.metadata_bytes,
            true,
            limits,
            keys,
            &mut stats,
            budget,
            &mut decoded,
        )?;
        let build_blob = files
            .store
            .publish_blob(&build.build_document.raw, 4 << 20)?;
        let cdn_blob = files.store.publish_blob(&build.cdn_document.raw, 4 << 20)?;
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
                // Fixed Go RootIndex.Lookup caps matches for one requested ID,
                // not the combined number of locale candidates in the batch.
                let entries = metadata_format::lookup_root_with_check(
                    &root.content,
                    &[*id],
                    metadata_format::RootLimits {
                        bytes: self.options.metadata_bytes,
                        records: 10_000_000,
                        groups: 65536,
                        matches: 4096,
                        operation: limits,
                    },
                    None,
                    || files.store.check(),
                )?;
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
                    index.find_with_check(&entry.content_key, limits, None, || admitted.check())?;
                let content = match mode {
                    ContentMode::Materialized => SelectedBody::Materialized(content::materialize(
                        files,
                        &index,
                        &mapping,
                        self.options.content_bytes,
                        false,
                        limits,
                        keys,
                        &mut stats,
                        budget,
                        &mut decoded,
                    )?),
                    ContentMode::Verified => SelectedBody::Verified(content::read_verified(
                        files,
                        &index,
                        &mapping,
                        self.options.content_bytes,
                        limits,
                        keys,
                        &mut stats,
                        budget,
                    )?),
                };
                let reading = serde_json::json!({"contentVerified":true,"source":"cdn","pin":self.pin,"buildConfiguration":build_blob,"cdnConfiguration":cdn_blob,"encodingKey":build.encoding_key,"root":root.blob,"entry":entry,"payloadEncodingKey":content.encoding_key(),"content":content.reference(),"verification":{"encodingFramingEKey":true,"encodingRequestedPagesMD5":true,"encodingWholeDecodedCKey":false,"rootWholeDecodedCKey":true,"contentWholeDecodedCKey":true},"keySource":keys.value(),"decodedCache":stats});
                output.push(SelectedFile {
                    entry,
                    reading,
                    body: content,
                });
                stats = Stats::default();
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
struct MetadataSource<'a, 'f> {
    inner: &'a dyn ByteSource,
    files: &'a Files<'f>,
    budget: &'a Budget,
}
impl ByteSource for MetadataSource<'_, '_> {
    fn size(&self) -> usize {
        self.inner.size()
    }
    fn read_span(&self, offset: usize, bytes: usize) -> Result<Cow<'_, [u8]>> {
        self.files.store.check()?;
        self.budget.charge(Cost {
            metadata_bytes: bytes as u64,
            ..Cost::default()
        })?;
        self.inner.read_span(offset, bytes)
    }
    fn read_checked_span(
        &self,
        offset: usize,
        bytes: usize,
        expected: &[u8],
    ) -> Result<Cow<'_, [u8]>> {
        self.files.store.check()?;
        self.budget.charge(Cost {
            metadata_bytes: bytes as u64,
            ..Cost::default()
        })?;
        self.inner.read_checked_span(offset, bytes, expected)
    }
}
#[cfg(test)]
mod tests;
