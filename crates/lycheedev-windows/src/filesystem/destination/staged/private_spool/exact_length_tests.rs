use super::*;
use std::{fs, os::windows::io::AsRawHandle};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lychee-exact-spool-{}",
            crate::random::token().unwrap()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
#[test]
fn exact_length_reserves_visible_bytes_and_preserves_native_handle_and_actual_hash_input() {
    let f = Fixture::new();
    let parent = Directory::open(&f.0).unwrap();
    let mut spool = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
    let handle = spool.stage.stage.file.as_ref().unwrap().as_raw_handle();
    let path = f.0.join(spool.name());
    spool.reserve_exact_len(131_079).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), 131_079);
    assert_eq!(spool.stage.stage.file_mut().stream_position().unwrap(), 0);
    assert_eq!(
        handle,
        spool.stage.stage.file.as_ref().unwrap().as_raw_handle()
    );
    let bytes: Vec<_> = (0..131_079).map(|n| (n % 251) as u8).collect();
    for chunk in bytes.chunks(65536) {
        spool.write_all(chunk).unwrap();
    }
    spool.finish_writes().unwrap();
    let mut actual = Vec::new();
    spool.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, bytes);
    assert!(spool.close().is_empty());
    assert!(!path.exists());
    parent.close().unwrap();
}
#[test]
fn unwritten_zero_tail_and_oversized_writes_cannot_become_a_complete_spool() {
    let f = Fixture::new();
    let parent = Directory::open(&f.0).unwrap();
    let mut spool = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
    spool.reserve_exact_len(4).unwrap();
    spool.write_all(b"ab").unwrap();
    assert!(spool.finish_writes().is_err());
    assert!(spool.read(&mut [0; 4]).is_err());
    assert!(spool.write_all(b"cde").is_err());
    assert_eq!(spool.stage.stage.file_mut().metadata().unwrap().len(), 4);
    spool.write_all(b"cd").unwrap();
    spool.finish_writes().unwrap();
    assert!(spool.write_all(b"x").is_err());
    assert!(spool.reserve_exact_len(4).is_err());
    let mut actual = Vec::new();
    spool.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, b"abcd");
    assert!(spool.close().is_empty());
    parent.close().unwrap();
}
#[test]
fn failed_length_or_sync_poison_the_same_owned_spool_and_checked_cleanup_remains_available() {
    for sync_failure in [false, true] {
        let f = Fixture::new();
        let parent = Directory::open(&f.0).unwrap();
        let mut spool = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
        let path = f.0.join(spool.name());
        let result = spool.reserve_exact_len_with(
            8,
            |file, len| {
                file.set_len(len)?;
                if !sync_failure {
                    Err(io::Error::other("length completion unknown"))
                } else {
                    Ok(())
                }
            },
            |file| {
                file.sync_all()?;
                Err(io::Error::other("sync completion failed"))
            },
        );
        assert!(result.is_err());
        assert_eq!(fs::metadata(&path).unwrap().len(), 8);
        assert!(spool.write_all(b"12345678").is_err());
        assert!(spool.finish_writes().is_err());
        assert!(spool.reserve_exact_len(8).is_err());
        assert!(spool.close().is_empty());
        assert!(!path.exists());
        parent.close().unwrap();
    }
}
#[test]
fn exact_reservation_refuses_existing_bytes_and_empty_body_still_requires_explicit_finish() {
    let f = Fixture::new();
    let parent = Directory::open(&f.0).unwrap();
    let mut spool = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
    spool.write_all(b"old").unwrap();
    assert!(spool.reserve_exact_len(100).is_err());
    assert!(spool.close().is_empty());
    let mut empty = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
    empty.reserve_exact_len(0).unwrap();
    assert!(empty.read(&mut [0; 1]).is_err());
    assert!(empty.write_all(b"x").is_err());
    empty.finish_writes().unwrap();
    assert_eq!(empty.read(&mut [0; 1]).unwrap(), 0);
    assert!(empty.close().is_empty());
    parent.close().unwrap();
}

#[test]
fn live_spool_inspection_transfers_owner_before_post_open_stop_and_grants_no_body_access() {
    struct StopAfterOpen {
        name_cost: u64,
        armed: bool,
        stopped: bool,
    }
    impl FileControl for StopAfterOpen {
        type Error = io::Error;
        fn charge(&mut self, cost: FileCost) -> io::Result<()> {
            if cost.allocation_bytes == self.name_cost {
                self.armed = true;
            }
            if self.armed && cost.io_operations == 3 {
                self.stopped = true;
            }
            Ok(())
        }
        fn check(&mut self) -> io::Result<()> {
            if self.stopped {
                Err(io::Error::other("post-open stopped"))
            } else {
                Ok(())
            }
        }
    }
    let f = Fixture::new();
    let parent = Directory::open(&f.0).unwrap();
    let mut spool = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
    spool.reserve_exact_len(16).unwrap();
    let name = std::ffi::OsStr::new(spool.name());
    let mut control = StopAfterOpen {
        name_cost: name.len() as u64 * 16 + 1024,
        armed: false,
        stopped: false,
    };
    let mut inspect = parent
        .open_optional_inspection_file_controlled(name, &mut control)
        .unwrap()
        .unwrap();
    assert!(control.stopped);
    assert!(control.check().is_err());
    assert_eq!(inspect.metadata().unwrap().len(), 16);
    assert!(inspect.read(&mut [0; 1]).is_err());
    crate::filesystem::close_file(inspect).unwrap();
    assert!(spool.close().is_empty());
    parent.close().unwrap();
}
