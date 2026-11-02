//! Exclusive installation mutation ownership for current managed delivery.
use crate::{Result, ToolkitError, control::OperationControl, storage::{self,ExclusiveLock}};
use std::{fs,path::{Path,PathBuf}};
pub struct InstallationMaintenanceLease {
    pub target: PathBuf,
    _lock: ExclusiveLock,
}
impl InstallationMaintenanceLease {
    pub fn acquire(target: &Path) -> Result<Self> {
        Self::acquire_controlled(target, &OperationControl::default())
    }
    pub fn acquire_controlled(target: &Path, control: &OperationControl) -> Result<Self> {
        control.check()?;
        let parent = fs::canonicalize(
            target
                .parent()
                .ok_or_else(|| ToolkitError::invalid("installation requires parent"))?,
        )?;
        let target = parent.join(
            target
                .file_name()
                .ok_or_else(|| ToolkitError::invalid("installation requires basename"))?,
        );
        storage::guard_path_with_check(&parent, &target, || control.check())?;
        let target = if target.exists() {
            fs::canonicalize(&target)?
        } else {
            target
        };
        let canonical = target
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_ascii_lowercase();
        let lock = ExclusiveLock::resource_controlled(
            &parent.join(".lycheedev-locks"),
            &format!("installation-maintenance:{canonical}"),
            control,
        )?;
        Ok(Self {
            target,
            _lock: lock,
        })
    }
    pub fn close(self) -> Result<()> {
        self._lock.close()
    }
}
