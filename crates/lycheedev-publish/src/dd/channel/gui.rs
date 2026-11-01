//! Fixed DD100130 runtime resources, original GUI/profile and read-only handle.
use super::super::desktop::ChannelAnchor;
use super::{Choices, fault, gui_with};
use crate::transport::Http;
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use lycheedev_windows::process::bound_memory::{self, BoundMemory};
use std::fs::File;
const RESOURCES: [(&str, &str); 3] = [
    (
        "netease_dd.exe",
        "4d6609e58d639f8ea6c513d3806904ae60896251ffbd4d9bc0e2361779dd947f",
    ),
    (
        "python37.dll",
        "268af469cc5b487d32848142d17f3640adc276bd85935e602cc9a295f8d7703d",
    ),
    (
        "ccvoicehub.res",
        "953b1bd893adb53c8b003d490ee219fe4a0e075efbed960fc8bc1bcff2a26892",
    ),
];
pub(super) const ACCESS_DENIED: &str = "publish.dd_channel_memory_denied";
fn native(error: String, control: &OperationControl) -> ToolkitError {
    control.check().err().unwrap_or_else(|| {
        let _ = error;
        fault("the original DD GUI memory is unavailable or changed")
    })
}
struct Memory<'a> {
    bound: &'a BoundMemory,
    control: &'a OperationControl,
}
impl super::managed::Memory for Memory<'_> {
    fn read(&mut self, address: u64, size: usize) -> Result<Vec<u8>> {
        self.bound
            .read(address, size, || {
                self.control
                    .check()
                    .map_err(|_| "DD channel operation stopped".into())
            })
            .map_err(|e| native(e, self.control))
    }
}
fn finish<T>(mut files: Vec<File>, mut result: Result<T>, control: &OperationControl) -> Result<T> {
    while let Some(file) = files.pop() {
        result = storage::finish_file(file, result, control);
    }
    result
}
pub(super) fn read(
    anchor: &ChannelAnchor,
    control: &OperationControl,
    mut verify_parent: impl FnMut() -> Result<()>,
) -> Result<Choices> {
    control.check()?;
    verify_parent()?;
    anchor.verify(control)?;
    let ccid = anchor.ccid()?;
    let mut files = Vec::new();
    let result = (|| {
        for (name, expected) in RESOURCES {
            let (file, digest) =
                storage::locked_sha256(&anchor.program_dir().join(name), 512 << 20, control)?;
            files.push(file);
            if digest != expected {
                return Err(fault(
                    "DD GUI channels require the pinned official DD100130 runtime resources",
                ));
            }
        }
        let bound = BoundMemory::open(
            anchor.gui(),
            anchor.executable(),
            &anchor.program_dir().join("python37.dll"),
            || {
                control
                    .check()
                    .map_err(|_| "DD channel operation stopped".into())
            },
        )
        .map_err(|error| {
            control.check().err().unwrap_or_else(|| {
                if error == bound_memory::ACCESS_DENIED {
                    ToolkitError::new(
                        ACCESS_DENIED,
                        "the existing DD GUI requires the one-shot same-user channel helper",
                        3,
                    )
                } else {
                    native(error, control)
                }
            })
        })?;
        let result = (|| {
            let base = bound.module_base();
            if super::managed::RUNTIME_RVA
                .checked_add(440)
                .is_none_or(|end| end > bound.module_size())
            {
                return Err(fault(
                    "DD managed runtime globals are outside the pinned module",
                ));
            }
            let http = Http::new(control)?;
            gui_with(
                &mut Memory {
                    bound: &bound,
                    control,
                },
                base,
                ccid,
                &http,
                control,
                || {
                    verify_parent()?;
                    anchor.verify(control)
                },
                || chrono::Utc::now().timestamp_millis() as f64 / 1000.0,
            )
        })();
        lycheedev_core::metadata::owned_write::finish_owned(
            result,
            bound
                .close()
                .map_err(|_| fault("DD GUI read-only process handle could not be closed")),
        )
    })();
    finish(files, result, control)
}
