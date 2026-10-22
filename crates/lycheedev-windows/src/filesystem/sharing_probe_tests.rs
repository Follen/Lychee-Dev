use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-sharing-probe-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("target.toc")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn missing_probe_uses_open_existing_and_never_creates_a_file() {
    let f = Fixture::new();
    assert_eq!(
        open_delete_sharing_probe(&f.path())
            .unwrap_err()
            .raw_os_error(),
        Some(2)
    );
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 0);
}
#[test]
fn deny_delete_handle_produces_raw_sharing_violation_without_mutation() {
    let f = Fixture::new();
    fs::write(f.path(), b"original").unwrap();
    let held = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(f.path())
        .unwrap();
    assert_eq!(
        open_delete_sharing_probe(&f.path())
            .unwrap_err()
            .raw_os_error(),
        Some(32)
    );
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
    crate::filesystem::close_file(held).unwrap();
    let probe = open_delete_sharing_probe(&f.path()).unwrap();
    crate::filesystem::close_file(probe).unwrap();
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
}
#[test]
fn probe_requests_only_delete_and_shares_existing_read_access() {
    let f = Fixture::new();
    fs::write(f.path(), b"original").unwrap();
    // Existing reader permits only DELETE access by later opens. A probe that
    // also requested read/write access would fail, as would one denying reads.
    let held = OpenOptions::new()
        .read(true)
        .share_mode(4)
        .open(f.path())
        .unwrap();
    let probe = open_delete_sharing_probe(&f.path()).unwrap();
    crate::filesystem::close_file(probe).unwrap();
    crate::filesystem::close_file(held).unwrap();
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
}
#[test]
fn successful_probe_allows_read_write_rename_and_never_deletes_on_close() {
    let f = Fixture::new();
    fs::write(f.path(), b"original").unwrap();
    let probe = open_delete_sharing_probe(&f.path()).unwrap();
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
    fs::write(f.path(), b"same object").unwrap();
    let renamed = f.0.join("renamed.toc");
    fs::rename(f.path(), &renamed).unwrap();
    crate::filesystem::close_file(probe).unwrap();
    assert_eq!(fs::read(&renamed).unwrap(), b"same object");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}
