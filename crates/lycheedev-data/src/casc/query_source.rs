//! Fixed Go local_session.go: query-owned newest generations, validated index
//! cache, and original-path verification before a successful capture.
use super::*;
use crate::resource::OwnedRetainedReservation;
use sha2::Sha256;
use std::{cell::RefCell, ffi::OsStr, sync::Arc};
const CAPACITY: usize = 64 << 20;
fn changed() -> ToolkitError {
    fault(
        "records.pinned_build_changed",
        4,
        "Local CASC index source changed during query",
    )
}
fn internal() -> ToolkitError {
    fault("records.internal", 8, "Local index cache is reentered")
}
struct Cached {
    raw: Vec<u8>,
    length: usize,
    bits: u8,
    _allocation: OwnedRetainedReservation,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Observed {
    digest: [u8; 32],
    size: usize,
}
#[derive(Default)]
struct Cache {
    indexes: BTreeMap<String, Cached>,
    observed: BTreeMap<String, Observed>,
    retained: usize,
}
pub(crate) struct QuerySource<'a> {
    root: PathBuf,
    directory: storage::Directory,
    selected: BTreeMap<u8, (u32, String)>,
    cache: RefCell<Cache>,
    scope: Scope<'a>,
    budget: &'a Arc<Budget>,
}
impl<'a> QuerySource<'a> {
    pub(crate) fn open(
        root: &Path,
        limits: Limits,
        budget: &'a Arc<Budget>,
        control: &'a OperationControl,
    ) -> Result<Self> {
        let mut scope = Scope {
            limits,
            cancel: None,
            control: Some(control),
            budget: Some(budget),
        };
        scope.check()?;
        let data = root.join("Data/data");
        let directory =
            storage::Directory::open_controlled(&data, &mut scope).map_err(|mut error| {
                error.message = format!(
                    "Open local CASC directory '{}': {}",
                    data.display(),
                    error.message
                );
                error
            })?;
        let result = (|| {
            let selected = select_indices_with_check(root, limits, None, || scope.check())?;
            directory.verify_controlled(&mut scope)?;
            budget.safety_only().charge(Cost::retained(
                root.as_os_str().len().saturating_mul(4) + selected.len() * 512 + 512,
            ))?;
            Ok(selected)
        })();
        match result {
            Ok(selected) => Ok(Self {
                root: root.to_owned(),
                directory,
                selected,
                cache: RefCell::default(),
                scope,
                budget,
            }),
            Err(error) => scope.finish(
                Err(error),
                directory
                    .close()
                    .err()
                    .into_iter()
                    .flatten()
                    .map(Into::into)
                    .collect(),
            ),
        }
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(super) fn scope(&self) -> Scope<'a> {
        self.scope
    }
    pub(super) fn open_file(&self, name: &str) -> Result<fs::File> {
        let mut scope = self.scope;
        self.directory
            .open_shared_file_controlled(OsStr::new(name), &mut scope)
            .map_err(|mut error| {
                error.message = format!(
                    "Open local CASC file '{}': {}",
                    self.directory.path().join(name).display(),
                    error.message
                );
                error
            })
    }
    pub(super) fn check_consumer(&self, budget: &Budget, control: &OperationControl) -> Result<()> {
        self.scope.check()?;
        if !std::ptr::eq(self.budget.as_ref(), budget)
            || !self
                .scope
                .control
                .is_some_and(|owner| std::ptr::eq(owner, control))
        {
            return Err(fault(
                "records.file_query",
                4,
                "Local source differs from the original query control/budget",
            ));
        }
        Ok(())
    }
    pub(super) fn indices(&self, key: &[u8; 16]) -> Result<Vec<(u8, String)>> {
        self.scope.check()?;
        // Go starts with the XOR-folded prefix bucket, retaining the original
        // ascending order for every remaining bucket. Stop on the first match.
        let folded = key[..9].iter().fold(0u8, |n, b| n ^ b);
        let preferred = (folded >> 4) ^ (folded & 15);
        self.budget
            .safety_only()
            .charge(Cost::retained(self.selected.len() * 64))?;
        let mut ordered = Vec::with_capacity(self.selected.len());
        if let Some((_, name)) = self.selected.get(&preferred) {
            ordered.push((preferred, name.clone()));
        }
        ordered.extend(
            self.selected
                .iter()
                .filter(|(bucket, _)| **bucket != preferred)
                .map(|(bucket, (_, name))| (*bucket, name.clone())),
        );
        Ok(ordered)
    }
    pub(super) fn find(&self, bucket: u8, name: &str, key: &[u8; 16]) -> Result<Vec<Span>> {
        self.scope.check()?;
        if self
            .selected
            .get(&bucket)
            .is_none_or(|(_, selected)| selected != name)
        {
            return Err(internal());
        }
        let mut cache = self.cache.try_borrow_mut().map_err(|_| internal())?;
        if !cache.indexes.contains_key(name) {
            let (index, observed) = self.read_index(bucket, name)?;
            if cache
                .observed
                .get(name)
                .is_some_and(|prior| *prior != observed)
            {
                return Err(changed());
            }
            if !cache.observed.contains_key(name) {
                self.budget
                    .safety_only()
                    .charge(Cost::retained(name.len() * 2 + 1024))?;
                cache.observed.insert(name.to_owned(), observed);
            }
            if index.raw.len() > CAPACITY - cache.retained {
                cache.indexes.clear();
                cache.retained = 0;
            }
            cache.retained += index.raw.len();
            cache.indexes.insert(name.to_owned(), index);
        }
        let cached = cache.indexes.get(name).ok_or_else(internal)?;
        let index = Index {
            raw: &cached.raw[40..40 + cached.length],
            bits: cached.bits,
        };
        index.find_with_check(key, self.scope.limits, None, || self.scope.check())
    }
    fn read_index(&self, bucket: u8, name: &str) -> Result<(Cached, Observed)> {
        let mut scope = self.scope;
        let mut file = self.open_file(name)?;
        let result = (|| {
            scope.check()?;
            let meta = file.metadata()?;
            if !meta.is_file() || meta.len() < 40 {
                return Err(invalid());
            }
            if meta.len() > CAPACITY as u64 || meta.len() > scope.limits.index_bytes as u64 {
                return Err(exceeded());
            }
            let size = meta.len() as usize;
            // Re-reading after eviction charges Go's cumulative logical work
            // again. Only actual cache allocation is refunded on eviction.
            self.budget.charge_split(
                Cost {
                    retained_bytes: size as u64,
                    metadata_bytes: size as u64,
                    decode_work: size.div_ceil(65536) as u64,
                    ..Cost::default()
                },
                Cost {
                    metadata_bytes: size as u64,
                    ..Cost::default()
                },
            )?;
            let allocation = self.budget.reserve_retained(size)?;
            let mut raw = Vec::new();
            raw.try_reserve_exact(size).map_err(|_| exceeded())?;
            raw.resize(size, 0);
            let mut digest = Sha256::new();
            for block in raw.chunks_mut(65536) {
                scope.check()?;
                file.read_exact(block)?;
                digest.update(&*block);
                scope.check()?;
            }
            if file.read(&mut [0; 1])? != 0 || file.metadata()?.len() != meta.len() {
                return Err(changed());
            }
            let index = Index::open_with_check(&raw, bucket, scope.limits, None, || scope.check())?;
            let length = index.raw.len();
            let bits = index.bits;
            self.directory.verify_controlled(&mut scope)?;
            let observed = Observed {
                digest: digest.finalize().into(),
                size,
            };
            Ok((
                Cached {
                    raw,
                    length,
                    bits,
                    _allocation: allocation.into_owned(self.budget)?,
                },
                observed,
            ))
        })();
        scope.close(file, result)
    }
    pub(crate) fn check(&self) -> Result<()> {
        let mut scope = self.scope;
        scope.check()?;
        self.directory.verify_controlled(&mut scope)?;
        let current = select_indices_with_check(&self.root, scope.limits, None, || scope.check())?;
        if current != self.selected {
            return Err(changed());
        }
        Ok(())
    }
    pub(crate) fn verify(&self) -> Result<()> {
        self.check()?;
        let cache = self.cache.try_borrow().map_err(|_| internal())?;
        let safety = self.budget.safety_only();
        let _allocation = safety.reserve_retained(65536)?;
        let mut buffer = [0; 65536];
        for (name, observed) in &cache.observed {
            let mut scope = self.scope;
            let mut file = self.open_file(name)?;
            let result = (|| {
                let mut digest = Sha256::new();
                let mut size = 0usize;
                loop {
                    scope.check()?;
                    let n = file.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    size = size.checked_add(n).ok_or_else(changed)?;
                    safety.charge(Cost::work(1))?;
                    if size > observed.size {
                        return Err(changed());
                    }
                    digest.update(&buffer[..n]);
                }
                if size != observed.size || <[u8; 32]>::from(digest.finalize()) != observed.digest {
                    return Err(changed());
                }
                self.directory.verify_controlled(&mut scope)?;
                scope.check()
            })();
            scope.close(file, result)?;
        }
        Ok(())
    }
    pub(crate) fn close_after<T>(self, result: Result<T>) -> Result<T> {
        drop(self.cache);
        self.scope.finish(
            result,
            self.directory
                .close()
                .err()
                .into_iter()
                .flatten()
                .map(Into::into)
                .collect(),
        )
    }
}

#[cfg(test)]
#[path = "query_source_tests.rs"]
mod tests;
