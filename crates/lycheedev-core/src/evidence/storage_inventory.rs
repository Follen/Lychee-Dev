//! Explicit protection inventory, never migration/deletion authority. Historical
//! producer labels identify candidates, not an authoritative storage domain.
use super::*;
use crate::{
    control::OperationControl,
    metadata::evidence_catalog::{self, EvidenceDomain},
    storage::{Directory, FileControl, FileCost},
};
use bounded::lifecycle::close_directory;
use std::{collections::BTreeMap, ffi::OsStr};

struct Checks<'a>(&'a OperationControl);
impl FileControl for Checks<'_> {
    type Error = ToolkitError;
    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
    fn charge(&mut self, _: FileCost) -> Result<()> {
        self.0.check()
    }
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Occupancy {
    files: u64,
    bytes: u64,
}
impl Occupancy {
    fn add(&mut self, bytes: u64) -> Result<()> {
        self.files = self.files.checked_add(1).ok_or_else(limit)?;
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(limit)?;
        Ok(())
    }
}
fn limit() -> ToolkitError {
    ToolkitError::new(
        "vault.data_inventory_limit",
        "Data protection inventory exceeds its bounded scan; nothing was adopted or deleted",
        4,
    )
}
fn candidate(kind: &str) -> bool {
    matches!(
        kind,
        "asset"
            | "asset-image"
            | "asset-export-manifest"
            | "asset-demux-source"
            | "asset-demux-frame"
            | "asset-demux-manifest"
            | "data-stream-manifest"
            | "data-query"
            | "data-query-csv"
            | "data-record"
            | "data-page"
            | "data-schema"
            | "data-search"
            | "data-foreign-key"
            | "hotfix-cache"
            | "hotfix-records"
            | "hotfix-scan"
            | "remote-target"
            | "local-target"
            | "spell-info"
            | "spell-auras"
            | "spell-summons"
            | "creature-display"
            | "creature-model"
            | "decor-list"
            | "decor-get"
            | "item-get"
            | "item-models"
            | "item-geosets"
            | "item-textures"
            | "encounter-get"
    )
}
const PREFIXES: &[&str] = &[
    "remote-catalog/",
    "remote-config/",
    "cdn-route/",
    "cdn-range/",
    "cdn-location/",
    "decoded-v1/",
    "definition-ref/",
    "definition/",
    "hotfix-wago-page/",
    "listfile/",
];

/// Optional metadata-only observation anchored to an already held directory.
fn file_length(
    directory: &Directory,
    name: &OsStr,
    control: &OperationControl,
) -> Result<Option<u64>> {
    let Some(file) =
        directory.open_optional_inspection_file_controlled(name, &mut Checks(control))?
    else {
        return Ok(None);
    };
    let result = (|| {
        control.check()?;
        let metadata = file.metadata()?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(ToolkitError::invalid(
                    "protected inventory entry is a reparse point",
                ));
            }
        }
        if !metadata.is_file() {
            return Err(ToolkitError::invalid(
                "protected inventory entry is not a regular file",
            ));
        }
        Ok(Some(metadata.len()))
    })();
    storage::finish_file(file, result, control)
}
fn visit_tree(
    path: &Path,
    root: &Path,
    skip: Option<&Path>,
    depth: usize,
    entries: &mut usize,
    canonical: &mut Occupancy,
    unknown: &mut Occupancy,
    shared_cas: bool,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    if depth > 16 {
        return Err(limit());
    }
    let directory = Directory::open_shared_controlled(path, &mut Checks(control))?;
    let result = (|| {
        for entry in fs::read_dir(directory.path())? {
            control.check()?;
            let entry = entry?;
            *entries = entries.checked_add(1).ok_or_else(limit)?;
            if *entries > 100_000 {
                return Err(limit());
            }
            let path = entry.path();
            if skip.is_some_and(|skip| path == skip) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                visit_tree(
                    &path,
                    root,
                    skip,
                    depth + 1,
                    entries,
                    canonical,
                    unknown,
                    shared_cas,
                    control,
                )?;
            } else if kind.is_file() {
                let bytes =
                    file_length(&directory, &entry.file_name(), control)?.ok_or_else(|| {
                        ToolkitError::invalid("protected inventory changed during scan")
                    })?;
                let components: Vec<_> = path
                    .strip_prefix(root)
                    .map_err(|_| limit())?
                    .components()
                    .collect();
                let is_cas = shared_cas
                    && components.len() == 2
                    && components[0].as_os_str().to_str().is_some_and(|name| {
                        name.len() == 2 && name.bytes().all(|b| b.is_ascii_hexdigit())
                    })
                    && components[1].as_os_str().to_str().is_some_and(|name| {
                        name.len() == 62 && name.bytes().all(|b| b.is_ascii_hexdigit())
                    });
                if is_cas {
                    canonical.add(bytes)?;
                } else {
                    unknown.add(bytes)?;
                }
            } else {
                return Err(ToolkitError::invalid(
                    "protected inventory refuses symlink, reparse or special entries",
                ));
            }
        }
        directory.verify_controlled(&mut Checks(control))?;
        control.check()
    })();
    close_directory(result, directory)
}

pub(crate) fn report(home: &Path, control: &OperationControl) -> Result<Value> {
    control.check()?;
    let absolute_home = std::path::absolute(home)?;
    control.check()?;
    let home = absolute_home.as_path();
    evidence_catalog::with_catalog(
        home,
        Some(EvidenceDomain::Data),
        control,
        &|| Ok(()),
        |catalog, quota| {
            let mut captures = 0u64;
            let mut candidate_captures = 0u64;
            let mut unsupported = BTreeMap::<&str, u64>::new();
            let mut bodies = BTreeMap::<String, u64>::new();
            let mut namespaces = BTreeMap::<&str, u64>::new();
            let mut after = String::new();
            let mut documents = 0usize;
            loop {
                let (count, last) =
                    catalog.visit_general("", &after, control, |key, _, raw| {
                        documents = documents.checked_add(1).ok_or_else(limit)?;
                        if documents > 100_000 {
                            return Err(limit());
                        }
                        if let Some(prefix) =
                            PREFIXES.iter().find(|prefix| key.starts_with(**prefix))
                        {
                            *namespaces.entry(prefix).or_default() += 1;
                        }
                        let Some(id) = key.strip_prefix("capture/") else {
                            return Ok(());
                        };
                        captures += 1;
                        let capture = dependencies::inspect_capture_ref(raw, id, control)?;
                        if candidate(&capture.provenance.kind) {
                            candidate_captures += 1;
                            let unsupported_reason = if !valid_digest(&capture.blob.sha256, 64) {
                                Some("noncanonicalDigest")
                            } else {
                                None
                            };
                            if let Some(reason) = unsupported_reason {
                                *unsupported.entry(reason).or_default() += 1;
                                return Ok(());
                            }
                            match bodies.entry(capture.blob.sha256) {
                                std::collections::btree_map::Entry::Vacant(entry) => {
                                    entry.insert(capture.blob.bytes);
                                }
                                std::collections::btree_map::Entry::Occupied(entry)
                                    if *entry.get() != capture.blob.bytes =>
                                {
                                    return Err(ToolkitError::invalid(
                                        "captures disagree on shared body length",
                                    ));
                                }
                                _ => {}
                            }
                        }
                        Ok(())
                    })?;
                if count < 1000 {
                    break;
                }
                after = last;
            }
            let mut verified = Occupancy::default();
            let mut missing = 0u64;
            for (digest, bytes) in &bodies {
                // Original streaming hash/EOF validation, no full materialization.
                if bounded::verify_capture_blob_optional(home, digest, *bytes, control)? {
                    verified.add(*bytes)?;
                } else {
                    missing += 1;
                }
            }
            let state =
                Directory::open_shared_controlled(&home.join("state"), &mut Checks(control))?;
            let measured = (|| {
                let mut files = BTreeMap::new();
                for name in [
                    "toolkit.sqlite",
                    "toolkit.sqlite-wal",
                    "toolkit.sqlite-shm",
                    "toolkit.sqlite-journal",
                ] {
                    if let Some(bytes) = file_length(&state, OsStr::new(name), control)? {
                        files.insert(name, bytes);
                    }
                }
                Ok(files)
            })();
            let shared_database = close_directory(measured, state)?;
            let mut shared = Occupancy::default();
            let mut shared_unknown = Occupancy::default();
            let mut legacy_tmp = Occupancy::default();
            let mut ignored = Occupancy::default();
            let mut entries = 0;
            visit_tree(
                &home.join("blobs"),
                &home.join("blobs"),
                None,
                0,
                &mut entries,
                &mut shared,
                &mut shared_unknown,
                true,
                control,
            )?;
            visit_tree(
                &home.join("tmp"),
                &home.join("tmp"),
                Some(&home.join("tmp/data-derived-v1")),
                0,
                &mut entries,
                &mut ignored,
                &mut legacy_tmp,
                false,
                control,
            )?;
            let managed = quota.snapshot()?;
            let managed_within_limit = managed.existing.total()? <= managed.configured_max_bytes;
            let result = json!({
                "schema":"lycheedev.data-protection-inventory.v1", "totalDataCoverageComplete":false,
                "accounting":"logical-file-lengths-not-allocated-blocks", "managedData":managed, "managedWithinLimit":managed_within_limit,
                "historicalProtected":{
                    "sharedDatabaseFiles":shared_database, "sharedDatabaseAttribution":"whole files protected; never prorated by domain or key count",
                    "historicalCaptureCount":captures, "knownDataNamespaceRecordCounts":namespaces,
                    "cacheNamespaceCoverage":"record counts only; legacy cache payload references are not authenticated or attributed",
                    "dataProducerCandidates":{"captureCount":candidate_captures,"verifiedDistinctBodies":verified,"missingDistinctBodies":missing,"unsupportedAuthenticatedCaptureCounts":unsupported,"classification":"authenticated capture schema and known producer label; not authoritative domain ownership"},
                    "sharedCas":shared,"unknownSharedShardEntries":shared_unknown,"legacyTmpOutsideManagedNamespace":legacy_tmp,
                    "totalsAreNonAdditive":"candidate bodies are a subset of shared CAS and may overlap managed registrations",
                    "retention":"all historical data and unknown entries retained; no adoption, migration, repair or deletion"
                },
                "limits":{"maximumDocuments":100000,"maximumFilesystemEntries":100000,"maximumDepth":16},
                "coordination":"quota and evidence gate held; lock bookkeeping and normal existing SQLite journal recovery may occur",
                "unclassifiedScope":"other historical cache/source/export locations and custom producer labels remain protected outside authoritative Data attribution"
            });
            control.check()?;
            Ok(result)
        },
    )
}

#[cfg(test)]
mod tests;
