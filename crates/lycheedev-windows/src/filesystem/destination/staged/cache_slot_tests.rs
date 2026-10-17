//! Bounded rebuildable-cache slots are private CREATE owners, not a lock table.
use super::*;
use std::{cell::Cell, fs, path::PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lychee-cache-slots-{}-{}",
            std::process::id(),
            crate::random::token().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let removed = fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            removed.unwrap();
        }
    }
}
const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_KEY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
fn stage(directory: &Directory) -> StagedFile<'_> {
    StagedFile::create_rebuildable_cache(directory, KEY, &mut Uncontrolled)
        .unwrap()
        .expect("free cache slot")
}
fn unknown(mut stage: StagedFile<'_>) -> String {
    stage.write_all(b"unknown original").unwrap();
    stage.seal_rebuildable_cache().unwrap();
    let error = stage
        .publish_name_with("entry".as_ref(), &mut Uncontrolled, |_, _| {
            Err(io::Error::other("completion unavailable"))
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "completion unavailable");
    assert_eq!(stage.publication(), StagedPublication::Unknown);
    let name = stage.name().to_owned();
    assert!(stage.close().is_empty());
    name
}

#[test]
fn rebuildable_live_slot_is_skipped_and_cancelled_owner_cannot_delete_another_stage() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let mut first = stage(&directory);
    first.write_all(b"first private").unwrap();
    let first_name = first.name().to_owned();
    let mut second = stage(&directory);
    second.write_all(b"second private").unwrap();
    let second_name = second.name().to_owned();
    assert_ne!(first_name, second_name);
    assert!(first_name.ends_with("-0"));
    assert!(second_name.ends_with("-1"));
    assert!(second.close().is_empty());
    assert!(!f.0.join(&second_name).exists());
    // Read through the still-owned original handle; a path reopen may rightly
    // be refused by Stage's existing deny-write/delete sharing policy.
    first.seal_rebuildable_cache().unwrap();
    let mut bytes = Vec::new();
    first.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"first private");
    assert!(f.0.join(&first_name).exists());
    let next = stage(&directory);
    assert_eq!(next.name(), second_name);
    assert!(next.close().is_empty());
    assert!(first.close().is_empty());
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 0);
    directory.close().unwrap();
}

#[test]
fn rebuildable_all_sixty_four_active_slots_are_bounded_and_release_only_owned_entries() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let mut active = Vec::new();
    for slot in 0..64 {
        let owner = stage(&directory);
        assert_eq!(owner.name(), format!(".lycheedev-cache-{KEY}-{slot}"));
        active.push(owner);
    }
    assert!(
        StagedFile::create_rebuildable_cache(&directory, KEY, &mut Uncontrolled)
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 64);
    let other = StagedFile::create_rebuildable_cache(&directory, OTHER_KEY, &mut Uncontrolled)
        .unwrap()
        .unwrap();
    assert!(other.close().is_empty());
    let released = active.pop().unwrap();
    let name = released.name().to_owned();
    assert!(released.close().is_empty());
    let replacement = stage(&directory);
    assert_eq!(replacement.name(), name);
    assert!(replacement.close().is_empty());
    for owner in active {
        assert!(owner.close().is_empty());
    }
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 0);
    directory.close().unwrap();
}

#[test]
fn rebuildable_mixed_active_unknown_and_foreign_slots_are_preserved_on_saturation() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let active: Vec<_> = (0..16).map(|_| stage(&directory)).collect();
    let mut retained = Vec::new();
    for _ in 16..32 {
        retained.push(unknown(stage(&directory)));
    }
    for slot in 32..64 {
        let name = format!(".lycheedev-cache-{KEY}-{slot}");
        fs::write(f.0.join(&name), b"foreign original").unwrap();
    }
    assert!(
        StagedFile::create_rebuildable_cache(&directory, KEY, &mut Uncontrolled)
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 64);
    for name in &retained {
        assert_eq!(fs::read(f.0.join(name)).unwrap(), b"unknown original");
    }
    for slot in 32..64 {
        assert_eq!(
            fs::read(f.0.join(format!(".lycheedev-cache-{KEY}-{slot}"))).unwrap(),
            b"foreign original"
        );
    }
    for owner in active {
        assert!(owner.close().is_empty());
    }
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 48);
    // Only an actually released owned slot can be reused. Retained entries
    // remain untouched even after an unrelated successful publication.
    let mut next = stage(&directory);
    next.write_all(b"complete replacement").unwrap();
    next.seal_rebuildable_cache().unwrap();
    assert_eq!(
        next.replace_name("entry".as_ref(), &mut Uncontrolled)
            .unwrap(),
        StagedPublication::Published
    );
    assert!(next.close().is_empty());
    assert_eq!(
        fs::read(f.0.join("entry")).unwrap(),
        b"complete replacement"
    );
    for name in retained {
        assert_eq!(fs::read(f.0.join(name)).unwrap(), b"unknown original");
    }
    directory.close().unwrap();
}

#[test]
fn rebuildable_unknown_after_real_replace_preserves_new_entry_and_old_reader() {
    let f = Fixture::new();
    fs::write(f.0.join("entry"), b"old complete").unwrap();
    let directory = Directory::open(&f.0).unwrap();
    let mut old = directory
        .open_optional_project_file_controlled("entry".as_ref(), &mut Uncontrolled)
        .unwrap()
        .unwrap();
    let mut first = stage(&directory);
    let old_stage = first.name().to_owned();
    first.write_all(b"new complete").unwrap();
    first.seal_rebuildable_cache().unwrap();
    let error = first
        .publish_name_with("entry".as_ref(), &mut Uncontrolled, |stage, name| {
            stage.rename(name, true)?;
            Err(io::Error::other("lost replacement completion"))
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "lost replacement completion");
    assert_eq!(first.publication(), StagedPublication::Unknown);
    assert!(first.write_all(b"never replay").is_err());
    let closes = Cell::new(0);
    let errors = first.close_with(|file| {
        closes.set(closes.get() + 1);
        crate::filesystem::close_file(file)?;
        Err(io::Error::other("late checked-close error"))
    });
    assert_eq!(closes.get(), 1);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].to_string(), "late checked-close error");
    assert!(!f.0.join(old_stage).exists());
    assert_eq!(fs::read(f.0.join("entry")).unwrap(), b"new complete");
    let mut original = Vec::new();
    old.read_to_end(&mut original).unwrap();
    assert_eq!(original, b"old complete");
    crate::filesystem::close_file(old).unwrap();
    let next = stage(&directory);
    assert!(next.close().is_empty());
    assert_eq!(fs::read(f.0.join("entry")).unwrap(), b"new complete");
    directory.close().unwrap();
}

#[test]
fn rebuildable_preacquisition_cancel_preserves_unknown_residue() {
    struct Control<'a>(&'a Cell<bool>);
    impl FileControl for Control<'_> {
        type Error = io::Error;
        fn charge(&mut self, _: FileCost) -> io::Result<()> {
            self.check()
        }
        fn check(&mut self) -> io::Result<()> {
            if self.0.get() {
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "original cancellation",
                ))
            } else {
                Ok(())
            }
        }
    }
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let retained = unknown(stage(&directory));
    let cancelled = Cell::new(true);
    let error =
        match StagedFile::create_rebuildable_cache(&directory, KEY, &mut Control(&cancelled)) {
            Err(error) => error,
            Ok(_) => panic!("cancelled scan acquired a stage"),
        };
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(fs::read(f.0.join(retained)).unwrap(), b"unknown original");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
    directory.close().unwrap();
}

#[test]
fn rebuildable_replace_succeeds_while_previous_published_writer_handle_is_still_open() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let mut first = stage(&directory);
    let mut second = stage(&directory);
    let first_name = first.name().to_owned();
    let second_name = second.name().to_owned();
    first.write_all(b"first complete").unwrap();
    second.write_all(b"second complete").unwrap();
    first.seal_rebuildable_cache().unwrap();
    second.seal_rebuildable_cache().unwrap();
    assert_eq!(
        first
            .replace_name("entry".as_ref(), &mut Uncontrolled)
            .unwrap(),
        StagedPublication::Published
    );
    let mut old_reader = directory
        .open_optional_project_file_controlled("entry".as_ref(), &mut Uncontrolled)
        .unwrap()
        .unwrap();
    // Crucially, first's original writable stage handle is still live and
    // retains Stage's share-READ-only policy. Publication must rely on the
    // existing POSIX replacement semantics, not on an accidental close gap.
    assert_eq!(
        second
            .replace_name("entry".as_ref(), &mut Uncontrolled)
            .unwrap(),
        StagedPublication::Published
    );
    assert_eq!(first.publication(), StagedPublication::Published);
    assert!(!f.0.join(first_name).exists());
    assert!(!f.0.join(second_name).exists());
    assert!(first.close().is_empty());
    assert_eq!(fs::read(f.0.join("entry")).unwrap(), b"second complete");
    assert!(second.close().is_empty());
    assert_eq!(fs::read(f.0.join("entry")).unwrap(), b"second complete");
    let mut old = Vec::new();
    old_reader.read_to_end(&mut old).unwrap();
    assert_eq!(old, b"first complete");
    crate::filesystem::close_file(old_reader).unwrap();
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
    directory.close().unwrap();
}
