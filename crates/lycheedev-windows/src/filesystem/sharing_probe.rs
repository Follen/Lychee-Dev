//! Non-mutating open of an existing destination to observe current delete-sharing.
//! This grants no cleanup authority and reserves no future rename. The caller
//! consumes the returned File with checked close exactly once before publishing.
use std::{fs::File, fs::OpenOptions, io, os::windows::fs::OpenOptionsExt, path::Path};

pub fn open_delete_sharing_probe(path: &Path) -> io::Result<File> {
    // access_mode replaces the default desired access. With no create/truncate
    // option, OpenOptions uses OPEN_EXISTING. Request only DELETE, share reads,
    // writes and deletes, and open the directory entry rather than its reparse
    // target. In particular, never set FILE_FLAG_DELETE_ON_CLOSE or disposition.
    const DELETE: u32 = 0x00010000;
    const SHARE_READ_WRITE_DELETE: u32 = 1 | 2 | 4;
    const OPEN_REPARSE_POINT: u32 = 0x00200000;
    OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(SHARE_READ_WRITE_DELETE)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(test)]
#[path = "sharing_probe_tests.rs"]
mod tests;
