//! Query-owned Encoding geometry and verified pages. Public Encoding stays uncached.
use super::*;
use crate::{
    blte::{EncodedRead, OwnedDecodedSpan, PreparedRanges},
    resource::{Budget, Cost, OwnedRetainedReservation},
};
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};
const CAPACITY: usize = 8 << 20;
// Separate from Go's fixed 128-byte logical charge. On the pinned 64-bit
// standard library, one BTree node is at most 288 bytes and Arc<CachedPage>
// owns 72 bytes of descriptor/control backing. At most one node per live key
// (plus a transient empty split node, covered by the incoming reservation)
// keeps 512 conservative even for the first sparse node. Allocator bookkeeping
// is outside the allocation ledger, as elsewhere; this is not an RSS bound.
const ENTRY_ALLOCATION: usize = 512;
struct CachedPage {
    bytes: OwnedDecodedSpan,
    _overhead: OwnedRetainedReservation,
}
#[derive(Clone)]
struct Page(Arc<CachedPage>);
impl AsRef<[u8]> for Page {
    fn as_ref(&self) -> &[u8] {
        &self.0.bytes
    }
}
#[derive(Default)]
struct Cache {
    physical: Option<Arc<OwnedDecodedSpan>>,
    pages: BTreeMap<usize, Page>,
    bytes: usize,
}
pub(crate) struct PreparedEncoding<S: EncodedRead> {
    directory: OwnedDecodedSpan,
    cache: RefCell<Cache>,
    ranges: PreparedRanges<S>,
    geometry: EncodingGeometry,
    _overhead: OwnedRetainedReservation,
    budget: Arc<Budget>,
}
impl<S: EncodedRead> PreparedEncoding<S> {
    pub(crate) fn open(
        ranges: PreparedRanges<S>,
        keys: &BTreeMap<u64, Vec<u8>>,
        limits: Limits,
        budget: Arc<Budget>,
    ) -> Result<Self> {
        ranges.check()?;
        let view = ranges.view(keys)?;
        let h = view.read_owned_span(0, 22, &budget)?;
        let geometry = encoding_geometry(&h, view.size(), limits)?;
        drop(h);
        budget.charge_split(
            Cost::metadata(
                geometry
                    .directory_bytes
                    .checked_add(512)
                    .ok_or_else(exceeded)?,
            ),
            Cost {
                metadata_bytes: geometry.directory_bytes as u64 + 512,
                ..Cost::default()
            },
        )?;
        let overhead = budget.reserve_retained(512)?.into_owned(&budget)?;
        let directory =
            view.read_owned_span(geometry.directory_start, geometry.directory_bytes, &budget)?;
        validate_content_directory(&directory, limits, None, || ranges.check())?;
        ranges.check()?;
        drop(view);
        Ok(Self {
            directory,
            cache: RefCell::new(Cache::default()),
            ranges,
            geometry,
            _overhead: overhead,
            budget,
        })
    }
    pub(crate) fn check(&self) -> Result<()> {
        self.ranges.check()
    }
    fn page(
        &self,
        keys: &BTreeMap<u64, Vec<u8>>,
        offset: usize,
        bytes: usize,
        expected: &[u8],
    ) -> Result<Page> {
        self.check()?;
        let mut cache = self
            .cache
            .try_borrow_mut()
            .map_err(|_| fault("records.internal", 8, "Encoding cache is reentered"))?;
        if let Some(page) = cache.pages.get(&offset) {
            return Ok(page.clone());
        }
        let cost = bytes.checked_add(128).ok_or_else(exceeded)?;
        self.budget.charge_split(
            Cost::metadata(cost),
            Cost {
                metadata_bytes: cost as u64,
                ..Cost::default()
            },
        )?;
        let overhead = self
            .budget
            .reserve_retained(ENTRY_ALLOCATION)?
            .into_owned(&self.budget)?;
        let raw = self.ranges.view(keys)?.read_checked_owned_span(
            offset,
            bytes,
            expected,
            &self.budget,
        )?;
        self.check()?;
        let page = Page(Arc::new(CachedPage {
            bytes: raw,
            _overhead: overhead,
        }));
        // Oversized reads never evict. At the exact cap the new page is retained.
        if bytes <= CAPACITY {
            if bytes > CAPACITY - cache.bytes {
                cache.pages.clear();
                cache.bytes = 0;
            }
            cache.pages.insert(offset, page.clone());
            cache.bytes += bytes;
        }
        Ok(page)
    }
    fn physical_directory(&self, keys: &BTreeMap<u64, Vec<u8>>) -> Result<Arc<OwnedDecodedSpan>> {
        self.check()?;
        let mut cache = self
            .cache
            .try_borrow_mut()
            .map_err(|_| fault("records.internal", 8, "Encoding cache is reentered"))?;
        if let Some(directory) = &cache.physical {
            return Ok(directory.clone());
        }
        let bytes = product(self.geometry.encoded_page_count, 32)?;
        self.budget.charge_split(
            Cost::metadata(bytes),
            Cost {
                metadata_bytes: bytes as u64,
                ..Cost::default()
            },
        )?;
        let raw = self.ranges.view(keys)?.read_owned_span(
            self.geometry.encoded_directory_start,
            bytes,
            &self.budget,
        )?;
        self.check()?;
        let raw = Arc::new(raw);
        cache.physical = Some(raw.clone());
        Ok(raw)
    }
    pub(crate) fn find(
        &self,
        keys: &BTreeMap<u64, Vec<u8>>,
        key: &str,
        limits: Limits,
        mut supplemental: impl FnMut() -> Result<()>,
    ) -> Result<EncodingRecord> {
        self.check()?;
        find_content(
            &self.directory,
            self.geometry.page_size,
            self.geometry.pages_start,
            key,
            limits,
            None,
            || {
                self.check()?;
                supplemental()
            },
            |offset, bytes, expected| self.page(keys, offset, bytes, expected),
        )
    }
}
impl<S: EncodedRead> EncodingLookup for PreparedEncoding<S> {
    fn physical(
        &self,
        keys: &BTreeMap<u64, Vec<u8>>,
        key: &str,
        limits: Limits,
        supplemental: &mut dyn FnMut() -> Result<()>,
    ) -> Result<EncodedRecord> {
        self.check()?;
        supplemental()?;
        check(limits, None)?;
        casc::key(key)?;
        if self.geometry.encoded_page_count == 0 {
            return Err(fault(
                "records.encoding_not_found",
                3,
                "Encoding key not found in verified metadata",
            ));
        }
        let directory = self.physical_directory(keys)?;
        find_physical(
            &directory,
            self.geometry.encoded_page_size,
            self.geometry.encoded_pages_start,
            key,
            limits,
            None,
            || {
                self.check()?;
                supplemental()
            },
            |offset, bytes, expected| self.page(keys, offset, bytes, expected),
        )
    }
}

#[cfg(test)]
mod tests;
