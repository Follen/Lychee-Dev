//! Read-only registry and publisher lookup. SDK bytes are pinned by their
//! Windows adapters at use; discovery does not load code or read account data.
use crate::desktop_runtime::{self, Lookup};
use lycheedev_core::{Result, control::OperationControl};
use std::path::PathBuf;

pub struct Installation {
    pub program_dir: PathBuf,
    pub executable: PathBuf,
    pub version: String,
}
impl Installation {
    pub fn discover(control: &OperationControl) -> Result<Self> {
        let installation = desktop_runtime::lookup(
            &Lookup {
                name_prefix: "网易DD",
                program_subdirectory: Some("100130"),
                executable: "netease_dd.exe",
                resource: "libnep.dll",
                publishers: &["NetEase (Hangzhou) Network Co., Ltd"],
            },
            control,
        )?;
        Ok(Self {
            program_dir: installation.directory,
            executable: installation.executable,
            version: installation.version,
        })
    }
}
