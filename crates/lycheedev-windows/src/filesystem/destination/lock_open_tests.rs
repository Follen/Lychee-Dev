use super::*;
use std::{
    fs,
    io::{Seek, SeekFrom},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-relative-lock-{}",
            crate::random::token().unwrap()
        ));
        fs::create_dir_all(root.join("parent/selected")).unwrap();
        Self(root)
    }
    fn selected(&self) -> PathBuf {
        self.0.join("parent/selected")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn original_open(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

#[test]
fn relative_lock_owner_creates_missing_and_preserves_existing_bytes() {
    let f = Fixture::new();
    let directory = Directory::open(&f.selected()).unwrap();
    directory.verify().unwrap();
    let mut file = directory
        .open_lock_file_owner("entry.lock".as_ref())
        .unwrap();
    file.write_all(b"original").unwrap();
    super::super::close_file(file).unwrap();
    directory.verify().unwrap();
    let mut file = directory
        .open_lock_file_owner("entry.lock".as_ref())
        .unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original");
    super::super::close_file(file).unwrap();
    let mut original = original_open(&f.selected().join("entry.lock")).unwrap();
    bytes.clear();
    original.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original");
    super::super::close_file(original).unwrap();
    directory.close().unwrap();
}

#[test]
fn relative_lock_owner_keeps_read_write_delete_sharing_and_original_handle() {
    let f = Fixture::new();
    let path = f.selected().join("entry.lock");
    fs::write(&path, b"original").unwrap();
    let directory = Directory::open(&f.selected()).unwrap();
    directory.verify().unwrap();
    let mut relative = directory
        .open_lock_file_owner("entry.lock".as_ref())
        .unwrap();
    let original = original_open(&path).unwrap();
    let renamed = f.selected().join("renamed.lock");
    fs::rename(&path, &renamed).unwrap();
    fs::write(&path, b"replacement").unwrap();
    relative.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    relative.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original");
    assert_eq!(fs::read(path).unwrap(), b"replacement");
    super::super::close_file(original).unwrap();
    super::super::close_file(relative).unwrap();
    directory.close().unwrap();
}

#[test]
fn relative_lock_owner_rejects_non_names_and_directories_without_traversal() {
    let f = Fixture::new();
    let directory = Directory::open(&f.selected()).unwrap();
    for name in [
        "",
        "../escape",
        "child/escape",
        r"child\escape",
        "entry:stream",
        "NUL",
    ] {
        directory.verify().unwrap();
        assert_eq!(
            directory
                .open_lock_file_owner(name.as_ref())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
    let folder = f.selected().join("folder.lock");
    fs::create_dir(&folder).unwrap();
    let original = original_open(&folder).unwrap_err();
    directory.verify().unwrap();
    let relative = directory
        .open_lock_file_owner("folder.lock".as_ref())
        .unwrap_err();
    assert_eq!(relative.kind(), original.kind());
    assert_eq!(relative.raw_os_error(), original.raw_os_error());
    assert!(!f.0.join("parent/escape").exists());
    directory.close().unwrap();
}

#[test]
fn relative_lock_owner_uses_held_parent_while_ancestor_replacement_is_denied() {
    let f = Fixture::new();
    let mut directory = Directory::open(&f.selected()).unwrap();
    assert!(fs::rename(f.0.join("parent"), f.0.join("moved")).is_err());
    assert!(fs::rename(f.selected(), f.0.join("replacement")).is_err());
    directory.verify().unwrap();
    let decoy = f.0.join("decoy");
    fs::create_dir(&decoy).unwrap();
    // Inject a stale lexical label after verification. The already-held parent
    // handle, rather than this label, must remain the authority for the open.
    directory.path = decoy.clone();
    let file = directory
        .open_lock_file_owner("entry.lock".as_ref())
        .unwrap();
    assert!(f.selected().join("entry.lock").is_file());
    assert!(!decoy.join("entry.lock").exists());
    super::super::close_file(file).unwrap();
    directory.close().unwrap();
    fs::rename(f.0.join("parent"), f.0.join("moved")).unwrap();
}
