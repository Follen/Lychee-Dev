use lycheedev_windows::filesystem::{Destination, Directory};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
};
#[path = "support/reparse.rs"]
mod reparse;
use reparse::{reparse_writer, set_junction};
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
    let destination = Destination::open(&parent.join("body"), false).unwrap();
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
    destination
        .publish(b"complete", 8, || {
            assert!(reparse_writer(&parent).is_err());
            Ok(())
        })
        .unwrap();
    assert_eq!(fs::read(parent.join("body")).unwrap(), b"complete");
    assert!(entries(&outside).is_empty());
    drop(destination);
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
    let destination = Destination::open(&output, true).unwrap();
    destination.publish(b"new complete", 64, || Ok(())).unwrap();
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
                let destination = Destination::open(&output, false).unwrap();
                barrier.wait();
                let body = vec![value; 256 << 10];
                let result = destination.publish(&body, body.len() as u64, || Ok(()));
                (value, result)
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
    let destination = Destination::open(&output, false).unwrap();
    fs::write(&output, b"old").unwrap();
    struct NeverRead;
    impl Read for NeverRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("source must not be read")
        }
    }
    assert_eq!(
        destination
            .publish_reader(&mut NeverRead, 1, 1, || Ok(()))
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    drop(destination);
    let destination = Destination::open(&output, true).unwrap();
    assert_eq!(
        destination
            .publish_reader(&mut NeverRead, 9, 8, || Ok(()))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    let mut checks = 0;
    let body = vec![8u8; 128 << 10];
    assert_eq!(
        destination
            .publish(&body, body.len() as u64, || {
                checks += 1;
                if checks >= 5 {
                    Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "actual mid-publication cancellation",
                    ))
                } else {
                    Ok(())
                }
            })
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
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
            Destination::open(&temp.0.join(name), true).is_err(),
            "{name}"
        );
    }
    let directory = temp.0.join("directory");
    fs::create_dir(&directory).unwrap();
    assert!(Destination::open(&directory, true).is_err());
    let output = temp.0.join("link");
    let target = temp.0.join("target");
    fs::create_dir(&target).unwrap();
    let destination = Destination::open(&output, true).unwrap();
    fs::create_dir(&output).unwrap();
    set_junction(&reparse_writer(&output).unwrap(), &target).unwrap();
    assert_eq!(
        destination
            .publish(b"refuse", 8, || Ok(()))
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert!(entries(&target).is_empty());
    fs::remove_dir(&output).unwrap();
}
