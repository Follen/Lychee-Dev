use super::*;
use std::{
    cell::Cell,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-private-spool-{}-{}-{}",
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
        let result = fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn staged(parent: &Directory) -> StagedFile<'_> {
    let mut stage = StagedFile::create(parent, &mut Uncontrolled).unwrap();
    stage.write_all(b"body").unwrap();
    stage.seal().unwrap();
    stage
}
#[test]
fn private_spool_same_directory_native_publication_keeps_strict_parent() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let mut stage = staged(&directory);
    let temp = stage.name().to_owned();
    assert!(fs::rename(&f.0, f.0.with_extension("moved")).is_err());
    assert_eq!(
        stage
            .publish_name(std::ffi::OsStr::new("final"), &mut Uncontrolled)
            .unwrap(),
        StagedPublication::Published
    );
    assert!(!f.0.join(temp).exists());
    assert!(stage.close().is_empty());
    assert_eq!(fs::read(f.0.join("final")).unwrap(), b"body");
    directory.close().unwrap();
}
#[test]
fn private_spool_seal_retains_same_handle_read_and_refuses_later_writes() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let mut stage = StagedFile::create(&directory, &mut Uncontrolled).unwrap();
    let path = f.0.join(stage.name());
    assert!(stage.read(&mut [0u8; 1]).is_err());
    stage.write_all(&vec![3u8; 131072]).unwrap();
    stage.seal().unwrap();
    assert!(stage.write_all(b"changed").is_err());
    assert!(stage.seal().is_err());
    let mut block = vec![0u8; 131072];
    assert_eq!(stage.read(&mut block).unwrap(), 65536);
    assert!(block[..65536].iter().all(|&b| b == 3));
    assert_eq!(stage.read(&mut block).unwrap(), 65536);
    assert_eq!(stage.read(&mut block).unwrap(), 0);
    assert!(stage.close().is_empty());
    assert!(!path.exists());
    directory.close().unwrap();
}
#[test]
fn private_spool_real_no_replace_collision_preserves_the_winner() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let mut stage = staged(&directory);
    let path = f.0.join(stage.name());
    fs::write(f.0.join("final"), b"winner").unwrap();
    assert_eq!(
        stage
            .publish_name(std::ffi::OsStr::new("final"), &mut Uncontrolled)
            .unwrap(),
        StagedPublication::NotPublished
    );
    assert!(stage.close().is_empty());
    assert!(!path.exists());
    assert_eq!(fs::read(f.0.join("final")).unwrap(), b"winner");
    directory.close().unwrap();
}
#[test]
fn private_spool_real_rename_error_retains_unknown_original_temp() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let mut stage = staged(&directory);
    let path = f.0.join(stage.name());
    let oversized_component = "x".repeat(8192);
    assert!(
        stage
            .publish_name(
                std::ffi::OsStr::new(&oversized_component),
                &mut Uncontrolled
            )
            .is_err()
    );
    assert_eq!(stage.publication(), StagedPublication::Unknown);
    assert!(stage.write_all(b"must not replay").is_err());
    assert!(stage.close().is_empty());
    assert_eq!(fs::read(path).unwrap(), b"body");
    directory.close().unwrap();
}
#[test]
fn private_spool_uncertain_error_after_real_rename_never_deletes_published_object() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let mut stage = staged(&directory);
    let old = f.0.join(stage.name());
    let error = stage
        .publish_name_with(
            std::ffi::OsStr::new("final"),
            &mut Uncontrolled,
            |stage, name| {
                stage.rename(name, false)?;
                Err(io::Error::other("injected loss of native completion"))
            },
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "injected loss of native completion");
    assert_eq!(stage.publication(), StagedPublication::Unknown);
    assert!(!old.exists());
    assert!(stage.close().is_empty());
    assert_eq!(fs::read(f.0.join("final")).unwrap(), b"body");
    directory.close().unwrap();
}
#[test]
fn private_spool_checked_close_consumes_once_and_preserves_published_or_discarded_fact() {
    for publish in [false, true] {
        let f = Fixture::new();
        let directory = Directory::open(&f.0).unwrap();
        let mut stage = staged(&directory);
        let old = f.0.join(stage.name());
        if publish {
            stage
                .publish_name(std::ffi::OsStr::new("final"), &mut Uncontrolled)
                .unwrap();
        }
        let closes = Cell::new(0);
        let errors = stage.close_with(|file| {
            closes.set(closes.get() + 1);
            super::super::super::close_file(file)?;
            Err(io::Error::other("checked close failure"))
        });
        assert_eq!(closes.get(), 1);
        assert_eq!(errors.len(), 1);
        assert!(!old.exists());
        assert_eq!(f.0.join("final").exists(), publish);
        directory.close().unwrap();
    }
}
#[test]
fn private_spool_rejects_path_escape_stream_and_directory_names_before_commit() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    for name in [
        "../outside",
        "child/file",
        "child\\file",
        "C:\\outside",
        "..",
        "",
        "body:stream",
    ] {
        let mut stage = staged(&directory);
        assert!(
            stage
                .publish_name(std::ffi::OsStr::new(name), &mut Uncontrolled)
                .is_err()
        );
        assert_eq!(stage.publication(), StagedPublication::NotPublished);
        assert!(stage.close().is_empty());
    }
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 0);
    directory.close().unwrap();
}
#[test]
fn private_spool_post_acquisition_stop_still_has_discard_and_close_owner() {
    struct Control(bool);
    impl FileControl for Control {
        type Error = io::Error;
        fn charge(&mut self, _: FileCost) -> io::Result<()> {
            self.check()
        }
        fn check(&mut self) -> io::Result<()> {
            if self.0 {
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "original stop after open",
                ))
            } else {
                Ok(())
            }
        }
    }
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    let mut control = Control(false);
    let stage = StagedFile::create(&directory, &mut control).unwrap();
    let path = f.0.join(stage.name());
    assert!(path.exists());
    control.0 = true;
    assert_eq!(
        control.check().unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
    assert!(stage.close().is_empty());
    assert!(!path.exists());
    directory.close().unwrap();
}

#[test]
fn project_replacement_is_same_directory_and_old_project_reader_keeps_original_bytes() {
    let f = Fixture::new();
    let name = std::ffi::OsStr::new("lycheedev.lock.json");
    fs::write(f.0.join(name), b"original").unwrap();
    let directory = Directory::open_shared_controlled(&f.0, &mut Uncontrolled).unwrap();
    let mut old = directory
        .open_optional_project_file_controlled(name, &mut Uncontrolled)
        .unwrap()
        .unwrap();
    let mut stage = staged(&directory);
    let temporary = f.0.join(stage.name());
    assert!(fs::rename(&f.0, f.0.with_extension("moved")).is_err());
    assert_eq!(
        stage.replace_name(name, &mut Uncontrolled).unwrap(),
        StagedPublication::Published
    );
    assert!(stage.close().is_empty());
    assert!(!temporary.exists());
    let mut original = Vec::new();
    old.read_to_end(&mut original).unwrap();
    assert_eq!(original, b"original");
    assert_eq!(fs::read(f.0.join(name)).unwrap(), b"body");
    super::super::super::close_file(old).unwrap();
    directory.close().unwrap();
}
#[test]
fn project_replacement_unknown_completion_never_replays_or_discards_the_new_entry() {
    let f = Fixture::new();
    let name = std::ffi::OsStr::new("lycheedev.lock.json");
    fs::write(f.0.join(name), b"old").unwrap();
    let directory = Directory::open_shared_controlled(&f.0, &mut Uncontrolled).unwrap();
    let mut stage = staged(&directory);
    let error = stage
        .publish_name_with(name, &mut Uncontrolled, |stage, path| {
            stage.rename(path, true)?;
            Err(io::Error::other("lost replacement completion"))
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "lost replacement completion");
    assert_eq!(stage.publication(), StagedPublication::Unknown);
    assert!(stage.replace_name(name, &mut Uncontrolled).is_err());
    assert!(stage.close().is_empty());
    assert_eq!(fs::read(f.0.join(name)).unwrap(), b"body");
    directory.close().unwrap();
}
