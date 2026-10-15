use super::*;
use std::{
    fs,
    io::Read,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "owned-cleanup-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn authenticated_regular_file_blocks_substitution_until_original_handle_delete() {
    let f = Fixture::new();
    let path = f.0.join("owned");
    fs::write(&path, b"expected").unwrap();
    let candidate = CleanupCandidate::open(&path, false).unwrap();
    let held = candidate
        .authenticate::<io::Error>(|file| {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            assert_eq!(bytes, b"expected");
            Ok(())
        })
        .ok()
        .unwrap();
    assert!(fs::rename(&path, f.0.join("moved")).is_err());
    assert!(fs::write(&path, b"foreign replacement").is_err());
    let (file, deleted) = held.discard();
    deleted.unwrap();
    crate::filesystem::close_file(file).unwrap();
    assert!(!path.exists());
    fs::write(&path, b"foreign after close").unwrap();
    assert_eq!(fs::read(path).unwrap(), b"foreign after close");
}
#[test]
fn failed_authentication_never_deletes_foreign_regular_file() {
    let f = Fixture::new();
    let path = f.0.join("foreign");
    fs::write(&path, b"foreign").unwrap();
    let candidate = CleanupCandidate::open(&path, false).unwrap();
    let (_, candidate) = candidate
        .authenticate::<io::Error>(|_| Err(io::Error::other("digest mismatch")))
        .err()
        .unwrap();
    crate::filesystem::close_file(candidate.into_file()).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"foreign");
}
#[test]
fn temporary_discard_targets_original_object_after_pathname_substitution() {
    let f = Fixture::new();
    let path = f.0.join("stage");
    let mut owned = OwnedTemporary::create(&path).unwrap();
    use std::io::Write;
    owned.file_mut().write_all(b"owned").unwrap();
    fs::rename(&path, f.0.join("original-moved")).unwrap();
    fs::write(&path, b"foreign replacement").unwrap();
    let (file, deleted) = owned.discard();
    deleted.unwrap();
    crate::filesystem::close_file(file).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"foreign replacement");
    assert!(!f.0.join("original-moved").exists());
}
#[test]
fn held_directory_blocks_parent_substitution_and_refuses_foreign_children() {
    let f = Fixture::new();
    let path = f.0.join("owned");
    fs::create_dir(&path).unwrap();
    let held = CleanupCandidate::open(&path, true)
        .unwrap()
        .authenticate::<io::Error>(|_| Ok(()))
        .ok()
        .unwrap();
    assert!(fs::rename(&path, f.0.join("replaced")).is_err());
    fs::write(path.join("foreign"), b"keep").unwrap();
    let (file, result) = held.discard();
    assert!(result.is_err());
    crate::filesystem::close_file(file).unwrap();
    assert_eq!(fs::read(path.join("foreign")).unwrap(), b"keep");
}
