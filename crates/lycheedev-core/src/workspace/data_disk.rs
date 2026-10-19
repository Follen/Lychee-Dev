//! Data disk admission for derived roots plus explicitly registered shared blobs.
//!
//! This is NOT yet a total-Data hard cap. Shared Data download/decoded/evidence
//! blobs outside registered publishers and metadata need domain attribution before
//! they can join this budget without charging Source/Live or counting a blob
//! twice. No existing Data path is moved, adopted, evicted, or deleted here.
//!
//! Writers borrow DataResources, then hold one quota lease through their final
//! checked close. This first version serializes participating writers. Peak is
//! additional simultaneously live bytes, including final + staging copies,
//! generation metadata, spill and export temporaries. Unused reservation is
//! released only by close/drop. Actual growth consumes, never adds to, peak.
use super::data_resources::{self, DataResources};
use crate::{Result, ToolkitError, control::OperationControl, selection::valid_digest, storage};
use serde::Serialize;
use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
};
mod metadata;
mod publication;
mod reading;
pub(crate) use metadata::{MetadataQuota, MetadataReadQuota};
mod shared_blobs;
pub use publication::{DerivedObject, DerivedObjectWriter};
pub use reading::{
    DerivedObjectReader, DerivedReadCheck, DerivedReadLease, DerivedReadReport,
    MAX_GENERATION_BYTES, OwnedDerivedObjectReader, OwnedDerivedReadLease,
};
pub(crate) use shared_blobs::retention::protects_evidence_body;
pub(crate) use shared_blobs::stream::StreamQuota;
pub use shared_blobs::stream::{
    publish_shared_blob_stream_with_check, rebuild_shared_blob_stream_with_check,
};
pub use shared_blobs::{publish_shared_blob_with_check, rebuild_shared_blob_with_check};
use storage::{Directory, ExclusiveLock, FileControl, FileCost};

const QUOTA_RESOURCE: &str = "data-derived-v1:quota";
const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 16;

/// Names do not depend on snapshot IDs: one content hash has one body location.
/// Generation manifests refer to that body; evidence remains in existing blobs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataCachePaths {
    pub objects: PathBuf,
    pub generations: PathBuf,
    pub staging: PathBuf,
    pub spill: PathBuf,
    pub exports: PathBuf,
    /// Private rebuildable Data SQLite main, journal and retained residuals.
    pub metadata: PathBuf,
}
impl DataCachePaths {
    pub fn new(home: &Path) -> Self {
        Self {
            objects: home.join("cache/data-derived-v1/objects"),
            generations: home.join("indexes/data-derived-v1/generations"),
            staging: home.join("tmp/data-derived-v1/staging"),
            spill: home.join("tmp/data-derived-v1/spill"),
            exports: home.join("tmp/data-derived-v1/exports"),
            metadata: home.join("state/data"),
        }
    }
    pub fn object(&self, sha256: &str) -> Result<PathBuf> {
        if !valid_digest(sha256, 64) || sha256.bytes().any(|b| b.is_ascii_uppercase()) {
            return Err(ToolkitError::invalid(
                "Data derived object requires lowercase SHA-256",
            ));
        }
        Ok(self.objects.join(&sha256[..2]).join(&sha256[2..]))
    }
    /// Monotonic Data attribution and crash-retained reservations for shared CAS bodies.
    pub fn shared_blobs(&self) -> PathBuf {
        self.generations
            .parent()
            .expect("fixed generations parent")
            .join("shared-blobs-v1")
    }
    fn roots(&self) -> [&Path; 4] {
        [
            self.objects.parent().expect("fixed objects parent"),
            self.generations.parent().expect("fixed generations parent"),
            self.staging.parent().expect("fixed staging parent"),
            &self.metadata,
        ]
    }
}

/// Caller-proven upper bounds for NEW simultaneously live bytes, not expected
/// final output alone. The categories must be disjoint physical file copies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdditionalPeak {
    pub objects: u64,
    pub generations: u64,
    pub staging: u64,
    pub spill: u64,
    pub export_temporary: u64,
}
impl AdditionalPeak {
    pub fn total(self) -> Result<u64> {
        sum([
            self.objects,
            self.generations,
            self.staging,
            self.spill,
            self.export_temporary,
        ])
    }
}

/// Accounting is logical file length, not filesystem allocation or free space.
/// Unknown regular files within these roots still charge occupancy; accounting
/// does not establish reconstructibility, reference ownership or deletion rights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedUsage {
    pub object_namespace_bytes: u64,
    pub generation_namespace_bytes: u64,
    pub temporary_namespace_bytes: u64,
    pub private_metadata_namespace_bytes: u64,
    /// One logical shared body for each completed Data attribution.
    pub shared_blob_bytes: u64,
    /// Unfinished publication retains its entire body/stage upper bound.
    pub retained_shared_peak_bytes: u64,
    pub files: usize,
}
impl DerivedUsage {
    pub fn total(self) -> Result<u64> {
        sum([
            self.object_namespace_bytes,
            self.generation_namespace_bytes,
            self.temporary_namespace_bytes,
            self.private_metadata_namespace_bytes,
            self.shared_blob_bytes,
            self.retained_shared_peak_bytes,
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub total_data_coverage_complete: bool,
    pub accounted_scope: &'static str,
    pub not_yet_attributed: [&'static str; 4],
    pub automatic_eviction_enabled: bool,
}
impl Default for Coverage {
    fn default() -> Self {
        Self {
            total_data_coverage_complete: false,
            accounted_scope: "versioned-derived-roots-private-metadata-registered-shared-blobs-and-pending-peaks",
            not_yet_attributed: [
                "historical/unregistered Data shared blobs and writers not using shared-blob admission",
                "historical shared metadata database, sidecars and legacy raw-source references",
                "legacy Data staging/spill paths outside tmp/data-derived-v1",
                "export temporaries outside tmp/data-derived-v1/exports",
            ],
            automatic_eviction_enabled: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationReport {
    pub configured_max_bytes: u64,
    pub existing: DerivedUsage,
    pub additional_peak: AdditionalPeak,
    /// Additional shared-body/stage peak, disjoint from the derived categories.
    pub shared_blob_additional_peak: u64,
    pub private_metadata_additional_peak: u64,
    pub admitted_peak_bytes: u64,
    pub coverage: Coverage,
}

#[derive(Clone, Copy)]
struct Checks<'a> {
    control: &'a OperationControl,
    supplemental: Option<&'a dyn Fn() -> Result<()>>,
}
impl Checks<'_> {
    fn extra(&self) -> Result<()> {
        self.supplemental.map_or(Ok(()), |check| check())
    }
    fn check(&self) -> Result<()> {
        self.control.check()?;
        self.extra()
    }
}
struct Controlled<'a, 'b>(&'a Checks<'b>);
impl FileControl for Controlled<'_, '_> {
    type Error = ToolkitError;
    fn charge(&mut self, _: FileCost) -> Result<()> {
        self.check()
    }
    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
}
fn finish_file<T>(file: fs::File, result: Result<T>, checks: &Checks<'_>) -> Result<T> {
    let cleanup = storage::close_file(file).err().map(ToolkitError::io);
    #[cfg(test)]
    inventory_tests::file_closed();
    join(result, cleanup, checks)
}
fn sum(values: impl IntoIterator<Item = u64>) -> Result<u64> {
    values.into_iter().try_fold(0u64, |total, value| {
        total.checked_add(value).ok_or_else(|| {
            ToolkitError::new(
                "vault.data_cache_size_overflow",
                "Data disk accounting overflow",
                4,
            )
        })
    })
}
fn close_directory(directory: Directory) -> Vec<ToolkitError> {
    #[cfg(test)]
    let path = directory.path().to_owned();
    let errors = directory
        .close()
        .err()
        .into_iter()
        .flatten()
        .map(ToolkitError::io)
        .collect();
    #[cfg(test)]
    let errors = inventory_tests::directory_closed(path, errors);
    errors
}
fn join<T>(
    result: Result<T>,
    cleanup: impl IntoIterator<Item = ToolkitError>,
    control: &Checks<'_>,
) -> Result<T> {
    data_resources::finish(result, cleanup, control.control.check(), control.extra())
}
fn budget_error(report: &ReservationReport) -> ToolkitError {
    ToolkitError::new(
        "vault.data_cache_budget_exceeded",
        "Data accounted-disk admission exceeds configured budget; existing data is preserved",
        3,
    )
    .with_details(
        serde_json::json!({"schema":"lycheedev.data-derived-admission.v1","reservation":report}),
    )
}

/// Lifetime-borrowed Data admission, with one kernel quota lease. This does not
/// grant deletion rights or silently enroll legacy writers. Callers must stop
/// writing before finish; dropping is fallback release, never filesystem cleanup.
pub struct DerivedReservation<'a> {
    _resources: Option<&'a DataResources>,
    home: PathBuf,
    control: Checks<'a>,
    paths: DataCachePaths,
    report: ReservationReport,
    lease: Option<ExclusiveLock>,
    directories: Vec<Directory>,
    charged: Cell<AdditionalPeak>,
    publication_receipts: RefCell<Vec<serde_json::Value>>,
    metadata_read_only: bool,
    // A single authenticated decoded alias may rebuild only this absent body.
    // Its completed charge is retained; ordinary owners never carry this field.
    recovery: Option<crate::evidence::BlobRef>,
}
impl<'a> DerivedReservation<'a> {
    pub(super) fn acquire(
        resources: &'a DataResources,
        home: &Path,
        limit: u64,
        peak: AdditionalPeak,
        control: &'a OperationControl,
        supplemental: Option<&'a dyn Fn() -> Result<()>>,
    ) -> Result<Self> {
        Self::acquire_inner(
            Some(resources),
            home,
            limit,
            peak,
            control,
            supplemental,
            false,
        )
    }
    // Shared-byte publication already runs in its caller's admitted session (or
    // a target/config operation). Never reacquire query slots from inside it.
    fn acquire_inner(
        resources: Option<&'a DataResources>,
        home: &Path,
        limit: u64,
        peak: AdditionalPeak,
        control: &'a OperationControl,
        supplemental: Option<&'a dyn Fn() -> Result<()>>,
        defer_admission: bool,
    ) -> Result<Self> {
        Self::acquire_scoped(
            resources,
            home,
            limit,
            peak,
            control,
            supplemental,
            defer_admission,
            false,
        )
    }
    fn acquire_scoped(
        resources: Option<&'a DataResources>,
        home: &Path,
        limit: u64,
        peak: AdditionalPeak,
        control: &'a OperationControl,
        supplemental: Option<&'a dyn Fn() -> Result<()>>,
        defer_admission: bool,
        metadata_read_only: bool,
    ) -> Result<Self> {
        Self::acquire_scoped_recovering(
            resources,
            home,
            limit,
            peak,
            control,
            supplemental,
            defer_admission,
            metadata_read_only,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn acquire_inner_recovering(
        resources: Option<&'a DataResources>,
        home: &Path,
        limit: u64,
        peak: AdditionalPeak,
        control: &'a OperationControl,
        supplemental: Option<&'a dyn Fn() -> Result<()>>,
        defer_admission: bool,
        recovery: Option<&crate::evidence::BlobRef>,
    ) -> Result<Self> {
        Self::acquire_scoped_recovering(
            resources,
            home,
            limit,
            peak,
            control,
            supplemental,
            defer_admission,
            false,
            recovery,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn acquire_scoped_recovering(
        resources: Option<&'a DataResources>,
        home: &Path,
        limit: u64,
        peak: AdditionalPeak,
        control: &'a OperationControl,
        supplemental: Option<&'a dyn Fn() -> Result<()>>,
        defer_admission: bool,
        metadata_read_only: bool,
        recovery: Option<&crate::evidence::BlobRef>,
    ) -> Result<Self> {
        let checks = Checks {
            control,
            supplemental,
        };
        let control = &checks;
        control.check()?;
        let additional = peak.total()?;
        let absolute_home = std::path::absolute(home)?;
        control.check()?;
        let paths = DataCachePaths::new(&absolute_home);
        let mut owner = Self {
            _resources: resources,
            home: absolute_home,
            control: checks,
            paths,
            report: ReservationReport {
                configured_max_bytes: limit,
                existing: DerivedUsage::default(),
                additional_peak: peak,
                shared_blob_additional_peak: 0,
                private_metadata_additional_peak: 0,
                admitted_peak_bytes: 0,
                coverage: Coverage::default(),
            },
            lease: None,
            directories: Vec::new(),
            charged: Cell::new(AdditionalPeak::default()),
            publication_receipts: RefCell::new(Vec::new()),
            metadata_read_only,
            recovery: recovery.cloned(),
        };
        let result = (|| {
            // Existing directory-owner lock acquisition anchors the leaf to its
            // held parent. Same invocation/deadline; no new independent timer.
            let locks = Directory::open_controlled(&home.join("locks"), &mut Controlled(control))?;
            owner.directories.push(locks);
            owner.lease = Some(ExclusiveLock::resource_wait_in_directory_checked(
                owner.directories.last().expect("held locks directory"),
                QUOTA_RESOURCE,
                control.control.deadline(),
                || control.check(),
                None,
            )?);
            #[cfg(test)]
            inventory_tests::quota_acquired();
            // Keep every managed root's identity through the reservation. Only
            // these new namespaces are prepared; no migration or reclamation.
            for root in owner.paths.roots() {
                owner.directories.push(prepare_root(root, control)?);
            }
            for child in [
                &owner.paths.objects,
                &owner.paths.generations,
                &owner.paths.staging,
                &owner.paths.spill,
                &owner.paths.exports,
            ] {
                owner.directories.push(prepare_root(child, control)?);
            }
            owner
                .directories
                .push(prepare_root(&owner.paths.shared_blobs(), control)?);
            // Config may have changed since query admission. A new disk owner
            // always consults the same current strict parser while holding quota.
            owner.report.configured_max_bytes =
                super::configuration::read(home, control.control, &mut || control.extra())?
                    .data_cache_max_bytes;
            owner.report.existing = if metadata_read_only {
                owner.report.coverage.accounted_scope = "private-metadata-only-zero-growth-read";
                metadata_inventory(&owner.paths, control)?
            } else {
                if let Some(reference) = recovery {
                    shared_blobs::validate_recovery(&owner.home, &owner.paths, reference, control)?;
                }
                inventory_recovering(
                    &owner.home,
                    &owner.paths,
                    control,
                    recovery,
                    &owner.directories,
                )?
            };
            owner.report.admitted_peak_bytes = sum([owner.report.existing.total()?, additional])?;
            if !defer_admission
                && owner.report.admitted_peak_bytes > owner.report.configured_max_bytes
            {
                return Err(budget_error(&owner.report));
            }
            control.check()
        })();
        match result {
            Ok(()) => Ok(owner),
            Err(error) => {
                let cleanup = owner.cleanup();
                join(Err(error), cleanup, control)
            }
        }
    }
    pub fn paths(&self) -> &DataCachePaths {
        &self.paths
    }
    pub fn report(&self) -> &ReservationReport {
        &self.report
    }
    /// Rescan while still holding the lease. This is a post-write verification,
    /// not a replacement for reserving the caller's full peak BEFORE writing.
    /// Active bytes replace their reserved allowance: never actual + full peak.
    pub fn verify_usage(&self) -> Result<DerivedUsage> {
        let current = if self.metadata_read_only {
            metadata_inventory(&self.paths, &self.control)?
        } else {
            inventory_recovering(
                &self.home,
                &self.paths,
                &self.control,
                self.recovery.as_ref(),
                &self.directories,
            )?
        };
        if current.total()? > self.report.admitted_peak_bytes {
            return Err(ToolkitError::new(
                "vault.data_cache_reservation_exceeded",
                "Data writer exceeded its admitted peak; files retained",
                3,
            )
            .with_details(serde_json::json!({
                "schema":"lycheedev.data-derived-admission.v1",
                "reservation":self.report,
                "observed":current,
            })));
        }
        self.control.check()?;
        Ok(current)
    }
    fn cleanup(&mut self) -> Vec<ToolkitError> {
        self.cleanup_with(close_directory, ExclusiveLock::close)
    }
    fn cleanup_with(
        &mut self,
        mut directory_close: impl FnMut(Directory) -> Vec<ToolkitError>,
        lease_close: impl FnOnce(ExclusiveLock) -> Result<()>,
    ) -> Vec<ToolkitError> {
        let mut errors = Vec::new();
        // Keep quota ownership through directory close. Other writers cannot
        // inspect a half-closed owner. Close lock before its own directory.
        while self.directories.len() > 1 {
            errors.extend(directory_close(
                self.directories.pop().expect("held directory"),
            ));
        }
        if let Some(lease) = self.lease.take() {
            errors.extend(lease_close(lease).err());
        }
        while let Some(directory) = self.directories.pop() {
            errors.extend(directory_close(directory));
        }
        errors
    }
    /// Consume once. Cleanup occurs even after operation failure/cancellation;
    /// every close event is preserved, while repeated stop observations dedupe.
    pub fn finish<T>(mut self, result: Result<T>) -> Result<T> {
        let result = result.and_then(|value| self.verify_usage().map(|_| value));
        let cleanup = self.cleanup();
        self.with_publications(join(result, cleanup, &self.control))
    }
    fn with_publications<T>(&self, result: Result<T>) -> Result<T> {
        result.map_err(|mut error| {
            let receipts = self.publication_receipts.borrow();
            if !receipts.is_empty() {
                error.details = serde_json::json!({
                    "schema":"lycheedev.data-derived-owner-error.v1",
                    "cause":error.clone(), "publicationReceipts":&*receipts,
                    "reservation":self.report,
                });
            }
            error
        })
    }
}
impl Drop for DerivedReservation<'_> {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn prepare_root(root: &Path, control: &Checks<'_>) -> Result<Directory> {
    let parent = Directory::open_controlled(
        root.parent().expect("fixed managed parent"),
        &mut Controlled(control),
    )?;
    let mut opened = None;
    let result = (|| {
        parent.verify_controlled(&mut Controlled(control))?;
        control.check()?;
        match fs::create_dir(root) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        control.check()?;
        opened = Some(Directory::open_controlled(root, &mut Controlled(control))?);
        control.check()
    })();
    let result = join(result, close_directory(parent), control);
    match result {
        Ok(()) => Ok(opened.expect("prepared managed directory")),
        Err(error) => join(
            Err(error),
            opened.into_iter().flat_map(close_directory),
            control,
        ),
    }
}
fn metadata_inventory(paths: &DataCachePaths, control: &Checks<'_>) -> Result<DerivedUsage> {
    let mut entries = 0;
    let mut files = 0;
    let bytes = scan(
        &paths.metadata,
        &paths.staging,
        &paths.metadata,
        0,
        &mut entries,
        &mut files,
        control,
        &[],
    )?;
    Ok(DerivedUsage {
        private_metadata_namespace_bytes: bytes,
        files,
        ..Default::default()
    })
}
fn inventory(home: &Path, paths: &DataCachePaths, control: &Checks<'_>) -> Result<DerivedUsage> {
    inventory_recovering(home, paths, control, None, &[])
}
fn inventory_recovering(
    home: &Path,
    paths: &DataCachePaths,
    control: &Checks<'_>,
    recovery: Option<&crate::evidence::BlobRef>,
    held: &[Directory],
) -> Result<DerivedUsage> {
    #[cfg(test)]
    inventory_tests::inventory_entered();
    let mut entries = 0;
    let mut files = 0;
    let mut bytes = [0; 4];
    for (index, root) in paths.roots().into_iter().enumerate() {
        bytes[index] = scan(
            root,
            &paths.staging,
            &paths.metadata,
            0,
            &mut entries,
            &mut files,
            control,
            held,
        )?;
    }
    let (shared_blob_bytes, retained_shared_peak_bytes) =
        shared_blobs::inventory_recovering(home, paths, control, recovery, held)?;
    Ok(DerivedUsage {
        shared_blob_bytes,
        retained_shared_peak_bytes,
        object_namespace_bytes: bytes[0],
        generation_namespace_bytes: bytes[1],
        temporary_namespace_bytes: bytes[2],
        private_metadata_namespace_bytes: bytes[3],
        files,
    })
}
// The quota owner already pins these exact absolute managed paths and their
// ancestors. Borrow only that live owner, never a prior inventory or lease.
// Unknown descendants retain the original owned-open/checked-close path.
fn with_inventory_directory<T>(
    path: &Path,
    held: &[Directory],
    control: &Checks<'_>,
    run: impl FnOnce(&Directory) -> Result<T>,
) -> Result<T> {
    if let Some(directory) = held.iter().find(|directory| directory.path() == path) {
        directory.verify_controlled(&mut Controlled(control))?;
        #[cfg(test)]
        inventory_tests::directory_borrowed(directory.path());
        // Preserve the terminal primary/stop aggregation without closing the
        // borrowed owner. Its reservation alone owns checked cleanup.
        return join(run(directory), std::iter::empty(), control);
    }
    let directory = Directory::open_controlled(path, &mut Controlled(control))?;
    #[cfg(test)]
    inventory_tests::directory_opened(directory.path());
    let result = run(&directory);
    join(result, close_directory(directory), control)
}

#[allow(clippy::too_many_arguments)]
fn scan(
    path: &Path,
    spool_root: &Path,
    metadata_root: &Path,
    depth: usize,
    entries: &mut usize,
    files: &mut usize,
    control: &Checks<'_>,
    held: &[Directory],
) -> Result<u64> {
    control.check()?;
    if depth > MAX_DEPTH {
        return Err(ToolkitError::new(
            "vault.data_cache_scan_limit",
            "Data disk directory depth exceeds admission",
            4,
        ));
    }
    with_inventory_directory(path, held, control, |directory| {
        let mut total = 0;
        for entry in fs::read_dir(directory.path())? {
            control.check()?;
            let entry = entry?;
            *entries += 1;
            if *entries > MAX_ENTRIES {
                return Err(ToolkitError::new(
                    "vault.data_cache_scan_limit",
                    "Data disk inventory exceeds 100000 entries",
                    4,
                ));
            }
            let kind = entry.file_type()?;
            let amount = if kind.is_dir() {
                scan(
                    &entry.path(),
                    spool_root,
                    metadata_root,
                    depth + 1,
                    entries,
                    files,
                    control,
                    held,
                )?
            } else if kind.is_file() {
                // Open relative to the held directory, never follow symlinks or
                // reparse leaves. Unknown files are charged and never reclaimed.
                let name = entry.file_name();
                let spool = path == spool_root
                    && name.to_str().is_some_and(|name| {
                        name.strip_prefix(".lycheedev-export-")
                            .is_some_and(|token| {
                                token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())
                            })
                    });
                let file = if spool || path.starts_with(metadata_root) {
                    // A Data producer may hold this original WRITE/DELETE
                    // handle outside quota. Its pre-sized length cannot grow;
                    // private SQLite files use the same inspection rights so
                    // SQLite contention is not misclassified as immutable CAS.
                    // All owned metadata mutation is serialized by this quota.
                    // Inspect attributes without weakening immutable readers.
                    let Some(file) = directory.open_optional_inspection_file_controlled(
                        &name,
                        &mut Controlled(control),
                    )?
                    else {
                        if spool {
                            continue;
                        }
                        return Err(ToolkitError::invalid(
                            "private Data metadata entry disappeared during quota inventory",
                        ));
                    };
                    file
                } else {
                    directory.open_file_controlled(&name, &mut Controlled(control))?
                };
                let measured = control
                    .check()
                    .and_then(|()| file.metadata().map_err(ToolkitError::io))
                    .and_then(|m| {
                        if !m.is_file() {
                            return Err(ToolkitError::invalid("Data inventory entry changed type"));
                        }
                        Ok(m.len())
                    });
                let bytes = finish_file(file, measured, control)?;
                *files += 1;
                bytes
            } else {
                return Err(ToolkitError::invalid(
                    "Data inventory contains a redirected or unsupported entry",
                ));
            };
            total = sum([total, amount])?;
        }
        control.check()?;
        Ok(total)
    })
}

#[cfg(test)]
mod inventory_tests;
#[cfg(test)]
mod tests;
