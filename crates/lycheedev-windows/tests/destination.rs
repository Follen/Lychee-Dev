use lycheedev_windows::filesystem::{
    Destination, Directory,
    destination::{FileControl, FileCost, StagedPublication},
};
use std::{
    fs::{self, File},
    io::{self, Read},
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
};
#[path = "support/reparse.rs"]
mod reparse;
use reparse::{reparse_writer, set_junction};
struct Control<F>(F);
impl<F: FnMut() -> io::Result<()>> FileControl for Control<F> {
    type Error = io::Error;
    fn check(&mut self) -> io::Result<()> {
        (self.0)()
    }
    fn charge(&mut self, _: FileCost) -> io::Result<()> {
        self.check()
    }
}
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-destination-{}-{}-{}",
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
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn entries(path: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
#[test]
fn held_directory_blocks_late_reparse_conversion_and_existing_writer() {
    let temp = Temp::new();
    let outside = temp.0.join("outside");
    let baseline = temp.0.join("baseline");
    let parent = temp.0.join("parent");
    let preheld = temp.0.join("preheld");
    for path in [&outside, &baseline, &parent, &preheld] {
        fs::create_dir(path).unwrap();
    }
    // Prove this host and fixture can convert an already-existing normal directory.
    set_junction(&reparse_writer(&baseline).unwrap(), &outside).unwrap();
    assert!(Directory::open(&baseline).is_err());
    let writer = reparse_writer(&preheld).unwrap();
    assert!(
        Directory::open(&preheld).is_err(),
        "an existing reparse writer must block admission"
    );
    drop(writer);
    let mut control = Control(|| {
        assert!(reparse_writer(&parent).is_err());
        Ok(())
    });
    // Admission acquires the parent before the control can assert it is held.
    let destination = Destination::prepare_controlled(
        &parent.join("body"), false, &mut Control(|| Ok(())),
    ).unwrap();
    destination.check_controlled(&mut control).unwrap();
    assert!(
        reparse_writer(&parent).is_err(),
        "late reparse writer was admitted"
    );
    assert!(fs::rename(&parent, temp.0.join("redirected")).is_err());
    // A directory guard must still allow normal files to be created and written.
    fs::write(parent.join("ordinary"), b"allowed").unwrap();
    // Protecting this output parent must not block unrelated sibling workspaces.
    let sibling = temp.0.join("sibling-before");
    fs::create_dir(&sibling).unwrap();
    fs::rename(&sibling, temp.0.join("sibling-after")).unwrap();
    let attempt = destination.publish_reader_controlled(
        &mut io::Cursor::new(b"complete"), 8, 8, &mut control,
    );
    attempt.result.unwrap();
    assert!(attempt.cleanup.is_empty());
    assert_eq!(attempt.publication, StagedPublication::Published);
    assert_eq!(fs::read(parent.join("body")).unwrap(), b"complete");
    assert!(entries(&outside).is_empty());
    destination.close().unwrap();
    fs::remove_dir(&baseline).unwrap(); // Remove only the junction, not its target.
}
#[test]
fn overwrite_preserves_hardlink_and_open_reader_on_old_bytes() {
    let temp = Temp::new();
    let output = temp.0.join("body");
    let alias = temp.0.join("alias");
    fs::write(&output, b"old original").unwrap();
    fs::hard_link(&output, &alias).unwrap();
    let mut held = File::open(&output).unwrap();
    let mut control = Control(|| Ok(()));
    let destination = Destination::prepare_controlled(&output, true, &mut control).unwrap();
    destination.check_controlled(&mut control).unwrap();
    let attempt = destination.publish_reader_controlled(
        &mut io::Cursor::new(b"new complete"), 12, 64, &mut control,
    );
    attempt.result.unwrap();
    assert!(attempt.cleanup.is_empty());
    assert_eq!(attempt.publication, StagedPublication::Published);
    destination.close().unwrap();
    assert_eq!(fs::read(&output).unwrap(), b"new complete");
    assert_eq!(fs::read(&alias).unwrap(), b"old original");
    let mut original = Vec::new();
    held.read_to_end(&mut original).unwrap();
    assert_eq!(original, b"old original");
    assert_eq!(entries(&temp.0), ["alias", "body"]);
}
#[test]
fn concurrent_no_overwrite_has_one_complete_winner() {
    let temp = Temp::new();
    let output = temp.0.join("winner");
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = [0x31u8, 0x72]
        .into_iter()
        .map(|value| {
            let output = output.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut control = Control(|| Ok(()));
                let destination =
                    Destination::prepare_controlled(&output, false, &mut control).unwrap();
                destination.check_controlled(&mut control).unwrap();
                barrier.wait();
                let body = vec![value; 256 << 10];
                let attempt = destination.publish_reader_controlled(
                    &mut io::Cursor::new(&body), body.len() as u64,
                    body.len() as u64, &mut control,
                );
                assert!(attempt.cleanup.is_empty());
                assert_eq!(attempt.publication, if attempt.result.is_ok() {
                    StagedPublication::Published
                } else {
                    StagedPublication::NotPublished
                });
                destination.close().unwrap();
                (value, attempt.result)
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(
        results.iter().filter(|(_, result)| result.is_ok()).count(),
        1
    );
    for (_, result) in &results {
        if let Err(error) = result {
            assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        }
    }
    let winner = results.iter().find(|(_, result)| result.is_ok()).unwrap().0;
    assert_eq!(fs::read(&output).unwrap(), vec![winner; 256 << 10]);
    assert_eq!(entries(&temp.0), ["winner"]);
}
#[test]
fn conflict_budget_and_cancellation_never_read_or_mutate_old_target() {
    let temp = Temp::new();
    let output = temp.0.join("body");
    let mut control = Control(|| Ok(()));
    let destination = Destination::prepare_controlled(&output, false, &mut control).unwrap();
    destination.check_controlled(&mut control).unwrap();
    fs::write(&output, b"old").unwrap();
    struct NeverRead;
    impl Read for NeverRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("source must not be read")
        }
    }
    let attempt = destination.publish_reader_controlled(&mut NeverRead, 1, 1, &mut control);
    assert_eq!(attempt.result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(attempt.publication, StagedPublication::NotPublished);
    assert!(attempt.cleanup.is_empty());
    destination.close().unwrap();
    let destination = Destination::prepare_controlled(&output, true, &mut control).unwrap();
    destination.check_controlled(&mut control).unwrap();
    let attempt = destination.publish_reader_controlled(&mut NeverRead, 9, 8, &mut control);
    assert_eq!(attempt.result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    assert_eq!(attempt.publication, StagedPublication::NotPublished);
    assert!(attempt.cleanup.is_empty());
    struct CancellingReader<'a> {
        body: io::Cursor<Vec<u8>>,
        cancelled: &'a std::cell::Cell<bool>,
        reads: usize,
    }
    impl Read for CancellingReader<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            self.reads += 1;
            let count = self.body.read(out)?;
            if count != 0 {
                self.cancelled.set(true);
            }
            Ok(count)
        }
    }
    let cancelled = std::cell::Cell::new(false);
    let mut body = CancellingReader {
        body: io::Cursor::new(vec![8u8; 128 << 10]),
        cancelled: &cancelled,
        reads: 0,
    };
    let mut control = Control(|| {
        if cancelled.get() {
            Err(io::Error::new(
                io::ErrorKind::Interrupted, "actual mid-publication cancellation",
            ))
        } else {
            Ok(())
        }
    });
    let attempt = destination.publish_reader_controlled(&mut body, 128 << 10, 128 << 10, &mut control);
    assert_eq!(attempt.result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert_eq!(attempt.publication, StagedPublication::NotPublished);
    assert!(attempt.cleanup.is_empty());
    assert_eq!(body.reads, 1, "cancel only after real source access");
    destination.close().unwrap();
    assert_eq!(fs::read(&output).unwrap(), b"old");
    assert_eq!(entries(&temp.0), ["body"]);
}
#[test]
fn path_devices_directories_streams_and_post_prepare_reparse_are_refused() {
    let temp = Temp::new();
    for name in [
        "CON",
        "con.txt",
        "COM1.bin",
        "LPT³",
        "body:stream",
        "body.",
        "body ",
    ] {
        assert!(
            Destination::prepare_controlled(&temp.0.join(name), true, &mut Control(|| Ok(()))).is_err(),
            "{name}"
        );
    }
    let directory = temp.0.join("directory");
    fs::create_dir(&directory).unwrap();
    let mut control = Control(|| Ok(()));
    let destination = Destination::prepare_controlled(&directory, true, &mut control).unwrap();
    assert_eq!(
        destination.check_controlled(&mut control).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists,
    );
    destination.close().unwrap();
    let output = temp.0.join("link");
    let target = temp.0.join("target");
    fs::create_dir(&target).unwrap();
    let destination = Destination::prepare_controlled(&output, true, &mut control).unwrap();
    destination.check_controlled(&mut control).unwrap();
    fs::create_dir(&output).unwrap();
    set_junction(&reparse_writer(&output).unwrap(), &target).unwrap();
    let attempt = destination.publish_reader_controlled(
        &mut io::Cursor::new(b"refuse"), 6, 8, &mut control,
    );
    assert_eq!(attempt.result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(attempt.publication, StagedPublication::NotPublished);
    assert!(attempt.cleanup.is_empty());
    destination.close().unwrap();
    assert!(entries(&target).is_empty());
    fs::remove_dir(&output).unwrap();
}

#[test]
fn held_output_parent_refuses_nonempty_ancestor_reparse() {
    let temp = Temp::new();
    let root = &temp.0;
    let ancestor = root.join("ancestor");
    let parent = ancestor.join("parent");
    fs::create_dir_all(&parent).unwrap();
    let outside = root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::create_dir(outside.join("parent")).unwrap();
    fs::write(outside.join("parent").join("body"), b"outside original").unwrap();
    let mut control = Control(|| Ok(()));
    let selected = Destination::prepare_controlled(&parent.join("body"), true, &mut control).unwrap();
    selected.check_controlled(&mut control).unwrap();
    assert!(reparse_writer(&parent).is_err());
    assert!(fs::rename(&parent, ancestor.join("moved")).is_err());
    // Prove the real junction payload/writer works on an empty ordinary directory.
    // The held parent below is an existing child of ancestor, so that ancestor
    // cannot be made empty by renaming/deleting the child while its handle lives.
    let baseline = root.join("empty-junction-baseline");
    fs::create_dir(&baseline).unwrap();
    set_junction(&reparse_writer(&baseline).unwrap(), &outside).unwrap();
    assert_ne!(
        fs::symlink_metadata(&baseline).unwrap().file_attributes() & 0x400,
        0
    );
    assert_eq!(
        fs::read(baseline.join("parent").join("body")).unwrap(),
        b"outside original"
    );
    let mutation = set_junction(&reparse_writer(&ancestor).unwrap(), &outside).unwrap_err();
    assert_eq!(mutation.raw_os_error(), Some(145)); // ERROR_DIR_NOT_EMPTY, not an installed junction.
    assert_eq!(
        fs::symlink_metadata(&ancestor).unwrap().file_attributes() & 0x400,
        0
    );
    selected.check_controlled(&mut control).unwrap();
    let body = b"selected parent bytes";
    let attempt = selected.publish_reader_controlled(
        &mut io::Cursor::new(body), body.len() as u64, 64, &mut control,
    );
    attempt.result.unwrap();
    assert!(attempt.cleanup.is_empty());
    assert_eq!(attempt.publication, StagedPublication::Published);
    assert_eq!(
        fs::read(parent.join("body")).unwrap(),
        b"selected parent bytes"
    );
    assert_eq!(
        fs::read(outside.join("parent").join("body")).unwrap(),
        b"outside original"
    );
    assert_eq!(fs::read_dir(outside.join("parent")).unwrap().count(), 1);
    selected.close().unwrap();
    fs::remove_dir(&baseline).unwrap();
}
