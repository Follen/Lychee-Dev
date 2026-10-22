use super::*;
use std::{cell::Cell, fs};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("lychee-private-spool-{}", crate::random::token().unwrap()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let result = fs::remove_dir_all(&self.0);
        if !std::thread::panicking() { result.unwrap(); }
    }
}

#[test]
fn private_spool_read_ready_preserves_original_handle_bounded_reads_and_no_mutation() {
    let fixture = Fixture::new();
    let parent = Directory::open(&fixture.0).unwrap();
    let mut spool = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
    let path = fixture.0.join(spool.name());
    let handle = spool.stage.stage.file.as_ref().unwrap().as_raw_handle();
    assert!(spool.read(&mut [0; 1]).is_err());
    let body: Vec<_> = (0..131_089).map(|i| (i % 251) as u8).collect();
    spool.write_all(&body).unwrap();
    spool.finish_writes().unwrap();
    assert_eq!(spool.stage.stage.file.as_ref().unwrap().as_raw_handle(), handle);
    assert!(spool.write_all(b"late").is_err());
    assert!(spool.finish_writes().is_err());
    assert!(fs::rename(&path, path.with_extension("moved")).is_err());
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    let mut buffer = vec![0; 131_072];
    let mut actual = Vec::new();
    loop {
        let n = spool.read(&mut buffer).unwrap();
        assert!(n <= 65_536);
        if n == 0 { break; }
        actual.extend_from_slice(&buffer[..n]);
    }
    assert_eq!(actual, body);
    assert_eq!(spool.stage.publication(), StagedPublication::NotPublished);
    assert!(spool.close().is_empty());
    assert!(!path.exists());
    parent.close().unwrap();
}

#[test]
fn private_spool_rewind_failure_retains_cleanup_owner_and_rejects_read() {
    let fixture = Fixture::new();
    let parent = Directory::open(&fixture.0).unwrap();
    let mut spool = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
    let path = fixture.0.join(spool.name());
    spool.write_all(b"body").unwrap();
    let error = spool.finish_writes_with(|_| Err(io::Error::other("rewind failed"))).unwrap_err();
    assert_eq!(error.to_string(), "rewind failed");
    assert!(!spool.stage.sealed);
    assert!(spool.read(&mut [0; 4]).is_err());
    assert!(spool.close().is_empty());
    assert!(!path.exists());
    parent.close().unwrap();
}

#[test]
fn private_spool_checked_close_keeps_failure_and_discards_once() {
    let fixture = Fixture::new();
    let parent = Directory::open(&fixture.0).unwrap();
    let mut spool = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
    let path = fixture.0.join(spool.name());
    spool.write_all(b"body").unwrap();
    spool.finish_writes().unwrap();
    let calls = Cell::new(0);
    let errors = spool.stage.close_with(|file| {
        calls.set(calls.get() + 1);
        crate::filesystem::close_file(file)?;
        Err(io::Error::other("private checked close failed"))
    });
    assert_eq!(calls.get(), 1);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].to_string(), "private checked close failed");
    assert!(!path.exists());
    parent.close().unwrap();
}

#[test]
fn private_spool_and_durable_stage_keep_distinct_io_boundaries() {
    let fixture = Fixture::new();
    let parent = Directory::open(&fixture.0).unwrap();
    let mut spool = PrivateSpool::create(&parent, &mut Uncontrolled).unwrap();
    spool.write_all(b"verified bytes").unwrap();
    // Read-ready performs only the real same-handle rewind. No sync callback is
    // available on this type; the separately owned final stage still requires it.
    let rewinds = Cell::new(0);
    spool.finish_writes_with(|file| {
        rewinds.set(rewinds.get() + 1);
        file.seek(SeekFrom::Start(0)).map(|_| ())
    }).unwrap();
    let mut body = Vec::new();
    spool.read_to_end(&mut body).unwrap();
    assert_eq!(body, b"verified bytes");
    let mut final_stage = StagedFile::create(&parent, &mut Uncontrolled).unwrap();
    final_stage.write_all(&body).unwrap();
    let syncs = Cell::new(0);
    final_stage.seal_with(|file| {
        syncs.set(syncs.get() + 1);
        file.sync_all()
    }).unwrap();
    assert_eq!((rewinds.get(), syncs.get()), (1, 1));
    assert_eq!(final_stage.publish_name(std::ffi::OsStr::new("final"), &mut Uncontrolled).unwrap(), StagedPublication::Published);
    assert!(final_stage.close().is_empty());
    assert!(spool.close().is_empty());
    assert_eq!(fs::read(fixture.0.join("final")).unwrap(), body);
    parent.close().unwrap();
}

#[test]
fn durable_stage_sync_failure_cannot_become_publishable() {
    let fixture = Fixture::new();
    let parent = Directory::open(&fixture.0).unwrap();
    let mut stage = StagedFile::create(&parent, &mut Uncontrolled).unwrap();
    let path = fixture.0.join(stage.name());
    stage.write_all(b"body").unwrap();
    let error = stage.seal_with(|_| Err(io::Error::other("durable sync failed"))).unwrap_err();
    assert_eq!(error.to_string(), "durable sync failed");
    assert!(stage.publish_name(std::ffi::OsStr::new("final"), &mut Uncontrolled).is_err());
    assert_eq!(stage.publication(), StagedPublication::NotPublished);
    assert!(stage.close().is_empty());
    assert!(!path.exists());
    assert!(!fixture.0.join("final").exists());
    parent.close().unwrap();
}
