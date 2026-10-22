use super::*;
use std::fs;
struct Checks {
    calls: usize,
    fail_at: usize,
}
impl FileControl for Checks {
    type Error = io::Error;
    fn charge(&mut self, _: FileCost) -> io::Result<()> {
        self.check()
    }
    fn check(&mut self) -> io::Result<()> {
        self.calls += 1;
        if self.calls == self.fail_at {
            Err(io::Error::other("fixture.stop"))
        } else {
            Ok(())
        }
    }
}
fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lychee-shared-admission-{}",
        crate::random::token().unwrap()
    ));
    fs::create_dir_all(root.join("parent/selected")).unwrap();
    root
}
#[test]
fn shared_controlled_directory_holds_identity_allows_metadata_and_checked_close_releases_it() {
    let root = root();
    let selected = root.join("parent/selected");
    let mut check = Checks {
        calls: 0,
        fail_at: usize::MAX,
    };
    let directory = Directory::open_shared_controlled(&selected, &mut check).unwrap();
    assert!(check.calls > 1);
    fs::write(selected.join("metadata"), b"one").unwrap();
    fs::write(selected.join("metadata.new"), b"two").unwrap();
    fs::rename(
        selected.join("metadata.new"),
        selected.join("metadata.second"),
    )
    .unwrap();
    assert!(fs::rename(&selected, root.join("replacement")).is_err());
    assert!(fs::rename(root.join("parent"), root.join("renamed")).is_err());
    directory.verify_controlled(&mut check).unwrap();
    directory.close().unwrap();
    fs::rename(&selected, root.join("replacement")).unwrap();
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn shared_controlled_directory_stop_precedes_open_and_partial_open_handles_are_released() {
    let root = root();
    let selected = root.join("parent/selected");
    for fail_at in [1, 9] {
        let error = Directory::open_shared_controlled(&selected, &mut Checks { calls: 0, fail_at })
            .err()
            .unwrap();
        assert_eq!(error.to_string(), "fixture.stop");
        let moved = root.join("moved");
        fs::rename(&selected, &moved).unwrap();
        fs::rename(&moved, &selected).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn consuming_directory_close_preserves_identical_errors_and_releases_every_handle() {
    let root = root();
    let selected = root.join("parent/selected");
    let directory = Directory::open_shared(&selected).unwrap();
    let count = directory._directories.len();
    let mut closed = 0;
    let errors = directory
        .close_with(|file| {
            super::super::close_file(file)?;
            closed += 1;
            Err(io::Error::other("fixture.same_close"))
        })
        .unwrap_err();
    assert_eq!(closed, count);
    assert_eq!(errors.len(), count);
    assert!(
        errors
            .iter()
            .all(|error| error.to_string() == "fixture.same_close")
    );
    fs::rename(&selected, root.join("moved")).unwrap();
    fs::remove_dir_all(root).unwrap();
}
