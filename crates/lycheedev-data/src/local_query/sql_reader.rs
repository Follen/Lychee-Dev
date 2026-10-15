//! Query-local local CASC preparation. The immutable source and its parsed
//! Encoding/Root directories survive all resolver callbacks without self-borrows.
use super::*;
use crate::blte::EncodedRead;
use crate::metadata_format::{PreparedEncoding, RootContent, RootIndex};

struct SharedReader<'a>(Arc<casc::LocalReader<'a>>);
impl EncodedRead for SharedReader<'_> {
    fn check_encoded(&self) -> Result<()> {
        self.0.check_encoded()
    }
    fn encoded_size(&self) -> usize {
        self.0.encoded_size()
    }
    fn read_encoded(&self, offset: usize, bytes: usize) -> Result<blte::EncodedSpan<'_>> {
        self.0.read_encoded(offset, bytes)
    }
}
struct Root(blte::Content);
impl RootContent for Root {
    fn content(&self) -> &blte::Content {
        &self.0
    }
}
struct Ready<'a> {
    index: PreparedEncoding<SharedReader<'a>>,
    root: RootIndex<Root>,
    root_blob: Value,
    build_blob: Value,
    cdn_blob: Value,
    stats: decoded_cache::Stats,
}
pub(super) struct Reader<'a> {
    inv: &'a Invocation,
    pin: &'a selection::DataPin,
    budget: &'a Arc<Budget>,
    keys: Option<keys::Context>,
    target: Option<installed::ResolvedLocalTarget>,
    // The independent owner survives fallible Prepared constructors. Closing
    // cannot be lost through an early error consuming their source argument.
    source: Option<Arc<casc::LocalReader<'a>>>,
    ready: Option<Ready<'a>>,
}
impl<'a> Reader<'a> {
    pub fn new(
        inv: &'a Invocation,
        pin: &'a selection::DataPin,
        budget: &'a Arc<Budget>,
    ) -> Result<Self> {
        inv.control.check()?;
        budget.check()?;
        Ok(Self {
            inv,
            pin,
            budget,
            keys: None,
            target: None,
            source: None,
            ready: None,
        })
    }
    fn limits(&self) -> casc::Limits {
        casc::Limits {
            deadline: self.budget.deadline,
            ..Default::default()
        }
    }
    fn prepare(&mut self) -> Result<()> {
        self.inv.control.check()?;
        self.budget.check()?;
        if self.ready.is_some() {
            return self.check_source();
        }
        if self.keys.is_none() {
            self.keys = Some(keys::Context::prepare_budgeted(
                self.inv,
                self.budget.deadline,
                Some(self.budget.clone()),
            )?);
        }
        if self.target.is_none() {
            self.budget.charge(Cost::metadata(
                self.inv.required("--installation")?.len()
                    + self.pin.product.len()
                    + self.pin.full_build.len()
                    + self.pin.region.len()
                    + self.pin.language.len()
                    + self.pin.definition_commit.len(),
            ))?;
            let target = installed::resolve_pinned_target_budgeted(
                Path::new(self.inv.required("--installation")?),
                self.pin,
                Some(self.budget),
            )?;
            if &target.data != self.pin {
                return Err(fault(
                    "records.pinned_build_changed",
                    4,
                    "Local configurations differ from fixed DataPin",
                ));
            }
            self.target = Some(target);
        }
        let target = self.target.as_ref().expect("prepared local target");
        let limits = self.limits();
        if self.source.is_none() {
            self.budget.safety_only().charge(Cost::retained(
                size_of::<casc::LocalReader<'_>>() + 2 * size_of::<usize>(),
            ))?;
            self.source = Some(Arc::new(casc::open_local_reader_controlled(
                &target.common_root,
                &target.metadata.encoding_key,
                target.metadata.encoding_bytes,
                limits,
                &self.inv.control,
                Some(self.budget),
            )?));
        }
        let source = self.source.as_ref().expect("prepared Encoding source");
        let ranges = blte::PreparedRanges::open(
            SharedReader(source.clone()),
            blte::Limits {
                decoded_bytes: target.metadata.content_bytes,
                ..Default::default()
            },
            self.budget.clone(),
        )?;
        if ranges.size() != target.metadata.content_bytes {
            return Err(fault(
                "records.invalid_metadata",
                4,
                "Encoding logical size differs from authenticated config",
            ));
        }
        let keys = self.keys.as_mut().expect("prepared local keys");
        let index = PreparedEncoding::open(ranges, keys.keys(), limits, self.budget.clone())?;
        let check = || {
            self.inv.control.check()?;
            self.budget.check()
        };
        let mapping = index.find(
            keys.keys(),
            &target.metadata.root_content_key,
            limits,
            check,
        )?;
        let mut stats = decoded_cache::Stats::default();
        let (_, root) = decoded_cache::read_content_controlled(
            &self.inv.home,
            &target.common_root,
            &index,
            &mapping,
            limits,
            keys,
            &mut stats,
            self.budget,
            None,
            &self.inv.control,
        )?;
        let root_blob = evidence::publish_blob_bounded_with_check(
            &self.inv.home,
            &root.bytes,
            256 << 20,
            &self.inv.control,
            || self.budget.check(),
        )?;
        let build_blob = evidence::publish_blob_bounded_with_check(
            &self.inv.home,
            &target.metadata.build_document.raw,
            4 << 20,
            &self.inv.control,
            || self.budget.check(),
        )?;
        let cdn_blob = evidence::publish_blob_bounded_with_check(
            &self.inv.home,
            &target.metadata.cdn_document.raw,
            4 << 20,
            &self.inv.control,
            || self.budget.check(),
        )?;
        let root = RootIndex::open(
            Root(root),
            metadata_format::RootLimits {
                operation: limits,
                ..Default::default()
            },
            self.budget.clone(),
            check,
        )?;
        self.ready = Some(Ready {
            index,
            root,
            root_blob,
            build_blob,
            cdn_blob,
            stats,
        });
        Ok(())
    }
    pub fn read(&mut self, id: u32) -> Result<File> {
        self.inv.control.check()?;
        self.budget.check()?;
        let maximum = validate_local_file_request(self.inv, &[id])?;
        let first = self.read_once(id, maximum);
        match first {
            Err(error)
                if error.code == "container.key_unavailable"
                    && error.details["missingKey"] == true
                    && self.keys.as_ref().is_some_and(|keys| {
                        keys.source.kind == "public" && keys.source.state != "loaded"
                    }) =>
            {
                // No Source or row has escaped this failed resolver call.
                self.keys.as_mut().expect("retry keys").ensure_public()?;
                self.read_once(id, maximum)
            }
            result => result,
        }
    }
    fn read_once(&mut self, id: u32, maximum: usize) -> Result<File> {
        self.prepare()?;
        let limits = self.limits();
        let ready = self.ready.as_mut().expect("prepared local session");
        let target = self.target.as_ref().expect("prepared local target");
        let keys = self.keys.as_mut().expect("prepared local keys");
        let source = self.source.as_ref().expect("prepared Encoding source");
        let check = || {
            self.inv.control.check()?;
            self.budget.check()
        };
        let entries = ready.root.lookup(id, check)?;
        let mask = locale(&self.pin.language)?;
        let variant = self.inv.flag("--content-variant").unwrap_or("");
        let mut candidates = entries.iter().filter(|entry| {
            entry.locale_mask & mask != 0
                && (variant != "standard" || entry.content_flags & 0x80 == 0)
                && (variant != "low-violence" || entry.content_flags & 0x80 != 0)
        });
        let entry = candidates.next().ok_or_else(|| {
            fault(
                "records.content_missing",
                3,
                "Pinned file locale/content variant must select exactly one Root record",
            )
        })?;
        if candidates.next().is_some() {
            return Err(fault(
                "records.file_ambiguous",
                4,
                "Pinned file locale/content variant must select exactly one Root record",
            ));
        }
        let mapping = ready
            .index
            .find(keys.keys(), &entry.content_key, limits, check)?;
        if mapping.decoded_bytes > maximum as u64 {
            return Err(fault(
                "records.metadata_limit",
                3,
                "Selected content exceeds byte budget",
            ));
        }
        let (encoding_key, content) = decoded_cache::read_content_controlled(
            &self.inv.home,
            &target.common_root,
            &ready.index,
            &mapping,
            limits,
            keys,
            &mut ready.stats,
            self.budget,
            Some(maximum),
            &self.inv.control,
        )?;
        let content_blob = evidence::publish_blob_bounded_with_check(
            &self.inv.home,
            &content.bytes,
            maximum,
            &self.inv.control,
            || self.budget.check(),
        )?;
        let safety = self.budget.safety_only();
        let size = capture::json_size(
            &(&ready.root_blob, &ready.build_blob, &ready.cdn_blob),
            &safety,
        )?;
        safety.charge(Cost::retained(size.saturating_mul(4).saturating_add(4096)))?;
        let mut reading = json!({"contentVerified":true,"source":"installation","pin":self.pin,"catalogSHA256":target.metadata.catalog_sha256,"buildConfiguration":ready.build_blob,"cdnConfiguration":ready.cdn_blob,"encodingKey":target.metadata.encoding_key,"root":ready.root_blob,"entry":entry,"payloadEncodingKey":encoding_key,"content":content_blob,"verification":{"encodingFramingEKey":true,"encodingRequestedPagesMD5":true,"encodingWholeDecodedCKey":false,"rootWholeDecodedCKey":true,"contentWholeDecodedCKey":true},"encodingIO":source.io_stats()});
        if !variant.is_empty() {
            reading["contentVariant"] = json!(variant);
        }
        reading["keySource"] = keys.value();
        reading["decodedCache"] = serde_json::to_value(&ready.stats)?;
        ready.stats = decoded_cache::Stats::default();
        self.inv.control.check()?;
        self.budget.check()?;
        Ok(File {
            bytes: content.bytes.into(),
            reading,
        })
    }
    pub fn check_source(&self) -> Result<()> {
        self.inv.control.check()?;
        self.budget.check()?;
        let Some(target) = &self.target else {
            return Ok(());
        };
        let after = installed::resolve_local_budgeted(
            &target.common_root,
            &target.metadata.installed.product,
            &self.pin.full_build,
            Some(self.budget),
        )?;
        if after.catalog_sha256 != target.metadata.catalog_sha256
            || after.build_document.sha256 != target.metadata.build_document.sha256
            || after.cdn_document.sha256 != target.metadata.cdn_document.sha256
        {
            return Err(fault(
                "records.pinned_build_changed",
                4,
                "Local metadata changed during query",
            ));
        }
        if let Some(source) = &self.source {
            source.check_encoded()?;
        }
        self.budget.check()
    }
    pub fn close_after<T>(mut self, result: Result<T>) -> Result<T> {
        drop(self.ready.take());
        match self.source.take() {
            Some(source) => match Arc::try_unwrap(source) {
                Ok(source) => source.close_after(result),
                Err(_) => Err(fault(
                    "records.internal",
                    8,
                    "Local SQL source still has a live borrower",
                )),
            },
            None => result,
        }
    }
}
