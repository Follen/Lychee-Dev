//! Narrow installation capability supplied by the CLI's Delivery adapter.
use lycheedev_core::Result;
use std::path::{Path, PathBuf};
#[derive(Debug, Clone)]
pub struct ManagedIdentity {
    pub canonical_path: PathBuf,
    pub manifest_sha256: String,
    pub generation: u64,
}
pub trait ManagedInstallation {
    fn verify_clean(
        &self,
        path: &Path,
        release: &str,
        protocol: &str,
        slots: u16,
    ) -> Result<ManagedIdentity>;
    fn publish_slot(
        &self,
        identity: &ManagedIdentity,
        slot: u16,
        expected_sha256: Option<&str>,
        bytes: &[u8],
    ) -> Result<()>;
}
