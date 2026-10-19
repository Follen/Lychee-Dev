//! Request-owned disposable originals. Durable capture publication remains at
//! the callers that actually expose BlobRefs.
use super::{Document, Index, git};
use lycheedev_core::{
    Result, ToolkitError,
    cache::SourceOriginalCache,
    control::OperationControl,
    selection::{SourcePin, valid_digest},
};
use sha2::{Digest, Sha256};
use std::borrow::Cow;

#[cfg(test)]
std::thread_local! {
    static READ_OBSERVATIONS: std::cell::RefCell<Vec<(String, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}
#[cfg(test)]
pub(crate) fn take_read_observations() -> Vec<(String, bool)> {
    READ_OBSERVATIONS.with(|observations| std::mem::take(&mut *observations.borrow_mut()))
}

fn matches(raw: &[u8], doc: &Document, control: &OperationControl) -> Result<bool> {
    if raw.len() != doc.bytes || std::str::from_utf8(raw).is_err() {
        return Ok(false);
    }
    let mut hash = Sha256::new();
    for block in raw.chunks(65536) {
        control.check()?;
        hash.update(block);
    }
    control.check()?;
    Ok(format!("{:x}", hash.finalize()) == doc.sha256)
}

pub(crate) struct OriginalReader<'a> {
    index: &'a Index,
    cache: Option<SourceOriginalCache>,
    git: git::Originals<'a>,
    text_budget_exceeded: bool,
}
impl<'a> OriginalReader<'a> {
    pub(super) fn new(index: &'a Index) -> Self {
        let summary = &index.summary;
        // The authenticated index fixes these fields. A moving requested ref
        // can never become a recovery source.
        let pin = SourcePin {
            repository: summary.repository.clone(),
            product: summary.product.clone(),
            exact_commit: summary.commit.clone(),
            parser_revision: summary.parser.clone(),
            requested_ref: String::new(),
        };
        // Reserve the entire existing source-text allowance, including later
        // consumers, before permitting a tree memo. Cold text is borrowed, not
        // copied or charged a second time. This charges retained original and
        // memo capacities. The existing bounded cache scratch and one Git
        // frame remain independent; this is not an all-memory/RSS bound.
        let text_bytes = index.documents.iter().fold(0usize, |total, doc| {
            total.saturating_add(doc.text.as_ref().map_or(doc.bytes, Vec::capacity))
        });
        Self {
            index,
            cache: None,
            text_budget_exceeded: text_bytes > (256usize << 20),
            git: git::Originals::new(
                &index.home,
                pin,
                (256usize << 20).saturating_sub(text_bytes),
            ),
        }
    }
    pub(crate) fn read(
        &mut self,
        doc: &'a Document,
        control: &OperationControl,
    ) -> Result<Cow<'a, [u8]>> {
        #[cfg(test)]
        READ_OBSERVATIONS.with(|observations| {
            observations
                .borrow_mut()
                .push((doc.path.clone(), doc.text.is_some()))
        });
        control.check()?;
        if self.text_budget_exceeded {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "Retained original byte budget exceeded",
                3,
            ));
        }
        if doc.bytes > git::MAX_SOURCE_BYTES {
            return Err(ToolkitError::new(
                "vault.blob_limit",
                "blob exceeds read budget",
                4,
            ));
        }
        if !valid_digest(&doc.sha256, 64) || !git::source_path(&doc.path) {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Invalid indexed original identity",
                4,
            ));
        }
        if let Some(raw) = &doc.text {
            if !matches(raw, doc, control)? {
                return Err(ToolkitError::new(
                    "codebase.source_integrity",
                    "Indexed in-memory original identity differs",
                    4,
                ));
            }
            // Populate only an actual consumer's original. A valid old entry
            // is read/authenticated without rewriting it, preserving warm reuse.
            let cache = self.cache(control)?;
            if !cache
                .read(&doc.sha256, control)?
                .is_some_and(|cached| cached == *raw)
            {
                cache.publish(&doc.sha256, raw, control)?;
            }
            return Ok(Cow::Borrowed(raw));
        }
        if let Some(raw) = self.cache(control)?.read(&doc.sha256, control)?
            && matches(&raw, doc, control)?
        {
            self.git
                .charge_original_capacity(raw.capacity().saturating_sub(doc.bytes))?;
            return Ok(Cow::Owned(raw));
        }
        let raw = self.git.read(&doc.path, git::MAX_SOURCE_BYTES, control)?;
        if !matches(&raw, doc, control)? {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Indexed original differs from the fixed Git object",
                4,
            ));
        }
        self.git
            .charge_original_capacity(raw.capacity().saturating_sub(doc.bytes))?;
        self.cache(control)?.publish(&doc.sha256, &raw, control)?;
        Ok(Cow::Owned(raw))
    }
    fn cache(&mut self, control: &OperationControl) -> Result<&SourceOriginalCache> {
        if self.cache.is_none() {
            self.cache = Some(SourceOriginalCache::open(&self.index.home, control)?);
        }
        Ok(self.cache.as_ref().expect("opened original cache"))
    }
    pub(crate) fn finish<T>(self, result: Result<T>, control: &OperationControl) -> Result<T> {
        let result = self.git.finish(result, control);
        match self.cache {
            Some(cache) => cache.finish(result, control),
            None => crate::persistence::finish_control(result, control.check()),
        }
    }
    #[cfg(test)]
    pub(super) fn git_started(&self) -> bool {
        self.git.started()
    }
}
