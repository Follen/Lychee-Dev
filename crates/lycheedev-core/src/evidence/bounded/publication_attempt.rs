//! Typed disk settlement from the existing byte publisher. The old public
//! Result/diagnostics remain unchanged. No caller infers cleanup from an error
//! code, path absence, or the text/shape of nested error.details.
use super::*;
use crate::storage::StagedPublication;

pub(crate) struct BlobPublishAttempt {
    pub result: Result<Value>,
    pub settlement: BlobPublicationSettlement,
}
pub(crate) struct BlobPublicationSettlement {
    pub publication: StagedPublication,
    pub reused: bool,
    pub reference: Option<BlobRef>,
    pub(super) managed_stage: Option<lycheedev_windows::filesystem::destination::ManagedStageName>,
    stage_created: bool,
    stage_cleanup_complete: bool,
}
impl BlobPublicationSettlement {
    pub(super) fn new() -> Self {
        Self {
            publication: StagedPublication::NotPublished,
            reused: false,
            reference: None,
            managed_stage: None,
            stage_created: false,
            // StagedFile::create has no fallible work after native acquisition;
            // it returns ownership before a post-open check. Covered by tests.
            stage_cleanup_complete: false,
        }
    }
    pub(super) fn stage_acquired(&mut self) {
        self.stage_created = true;
        self.stage_cleanup_complete = false;
    }
    pub(super) fn stage_closed(&mut self, complete: bool) {
        self.stage_cleanup_complete = complete;
    }
    /// Only the caller's newly created attribution intent may be withdrawn.
    /// Never implies permission to remove a shared CAS body or someone else's
    /// intent, even if the byte publisher reused an older shared object.
    pub(crate) fn may_release_unpublished_intent(&self) -> bool {
        self.publication == StagedPublication::NotPublished
            && !self.reused
            && (!self.stage_created || self.stage_cleanup_complete)
    }
}

pub(crate) fn publish_blob_attempt_with_check(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    control: &OperationControl,
    mut supplemental: impl FnMut() -> Result<()>,
) -> BlobPublishAttempt {
    let mut settlement = BlobPublicationSettlement::new();
    let result = publish_internal_observed(
        home,
        bytes,
        maximum,
        Some(control),
        &mut supplemental,
        None,
        &mut settlement,
    );
    BlobPublishAttempt { result, settlement }
}

#[cfg(test)]
mod tests;

/// Data quota has durably predeclared this finite name before entering the
/// ordinary canonical blob lock. No quota acquisition occurs in this helper.
/// The caller computed `reference` from these same borrowed immutable bytes;
/// external expected identities must have been compared at their public entry.
pub(crate) fn publish_blob_attempt_predeclared_with_check(
    home: &Path,
    bytes: &[u8],
    reference: &BlobRef,
    maximum: usize,
    control: &OperationControl,
    mut supplemental: impl FnMut() -> Result<()>,
    name: &lycheedev_windows::filesystem::destination::ManagedStageName,
) -> BlobPublishAttempt {
    let mut settlement = BlobPublicationSettlement::new();
    settlement.managed_stage = Some(name.clone());
    let result = publish_internal_observed(
        home,
        bytes,
        maximum,
        Some(control),
        &mut supplemental,
        Some(reference),
        &mut settlement,
    );
    BlobPublishAttempt { result, settlement }
}
