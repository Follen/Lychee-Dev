use super::*;
const HOLDER: &str = "LYCHEEDEV_FILE_SNAPSHOT_HOLDER_TEST";
use std::{
    fs::{File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    os::windows::fs::OpenOptionsExt,
    path::PathBuf,
};
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-file-snapshot-{}-{}",
            std::process::id(),
            crate::random::token().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn identity() -> ProcessIdentity {
    let process = crate::Process::open(std::process::id()).unwrap();
    let identity = process.identity().clone();
    process.close().unwrap();
    identity
}
fn exclusive(path: &Path) -> File {
    OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .unwrap()
}
#[test]
fn selected_exclusive_file_is_copied_without_reopening_it_or_changing_the_shared_cursor() {
    let dir = Dir::new();
    let path = dir.0.join("Cookies-读取");
    let missing = dir.0.join("not-held-Cookies");
    let mut file = exclusive(&path);
    file.write_all(b"synthetic-encrypted-browser-database-only")
        .unwrap();
    file.sync_all().unwrap();
    file.seek(SeekFrom::Start(7)).unwrap();
    assert!(
        std::fs::read(&path).is_err(),
        "the intended fixture must reject an ordinary reopen"
    );
    let copied = snapshot_process_files(
        &identity(),
        &std::env::current_exe().unwrap(),
        &[&path, &missing],
        || Ok(()),
    )
    .unwrap();
    assert_eq!(
        copied.get(0).unwrap(),
        b"synthetic-encrypted-browser-database-only"
    );
    assert!(copied.get(1).is_none());
    assert_eq!(file.stream_position().unwrap(), 7);
    crate::filesystem::close_file(file).unwrap();
    let moved = dir.0.join("released");
    std::fs::rename(&path, &moved).unwrap();
    assert_eq!(std::fs::read(&moved).unwrap(), copied.get(0).unwrap());
}
#[test]
fn process_identity_image_budget_and_cancellation_are_checked_before_returning_private_bytes() {
    let dir = Dir::new();
    let path = dir.0.join("Cookies");
    let mut file = exclusive(&path);
    file.write_all(b"synthetic-not-a-real-cookie").unwrap();
    file.sync_all().unwrap();
    let own = identity();
    let executable = std::env::current_exe().unwrap();
    let wrong = ProcessIdentity {
        pid: own.pid,
        created: own.created + 1,
    };
    assert!(snapshot_process_files(&wrong, &executable, &[&path], || Ok(())).is_err());
    assert!(snapshot_process_files(&own, &path, &[&path], || Ok(())).is_err());
    let mut calls = 0;
    let error = snapshot_process_files(&own, &executable, &[&path], || {
        calls += 1;
        if calls >= 8 {
            Err("synthetic-stop".into())
        } else {
            Ok(())
        }
    })
    .err()
    .unwrap();
    assert!(error.contains("synthetic-stop"));
    file.set_len((MAX_FILE + 1) as u64).unwrap();
    let error = snapshot_process_files(&own, &executable, &[&path], || Ok(()))
        .err()
        .unwrap();
    assert!(error.contains("budget"));
    crate::filesystem::close_file(file).unwrap();
    std::fs::remove_file(&path).unwrap();
}

#[test]
#[ignore = "private file-holder process; its parent supplies the complete fixture role"]
fn holder_process() {
    let root =
        PathBuf::from(std::env::var_os(HOLDER).expect("file-holder fixture role is required"));
    assert!(root.is_absolute() && root.is_dir());
    let mut database = exclusive(&root.join("Cookies"));
    let mut wal = exclusive(&root.join("Cookies-wal"));
    database.write_all(b"synthetic-child-database").unwrap();
    wal.write_all(b"synthetic-child-wal").unwrap();
    database.sync_all().unwrap();
    wal.sync_all().unwrap();
    std::fs::write(root.join("ready"), b"ready").unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn files_held_by_a_different_original_process_are_copied_and_its_exact_custody_is_reaped() {
    use std::time::{Duration, Instant};
    let dir = Dir::new();
    let executable = std::env::current_exe().unwrap();
    let child = crate::detached::spawn_hidden_no_inherit(
        &executable,
        &[
            "--ignored".into(),
            "--exact".into(),
            "process_file_snapshot::tests::holder_process".into(),
            "--nocapture".into(),
        ],
        &std::env::current_dir().unwrap(),
        &[(HOLDER.into(), dir.0.as_os_str().to_owned())],
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let check = || {
        if Instant::now() < deadline {
            Ok(())
        } else {
            Err("file-holder fixture deadline".into())
        }
    };
    let result = (|| {
        while !dir.0.join("ready").exists() {
            check()?;
            if child.try_wait()?.is_some() {
                return Err("file-holder fixture exited".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        snapshot_process_files(
            child.identity(),
            &executable,
            &[&dir.0.join("Cookies"), &dir.0.join("Cookies-wal")],
            check,
        )
    })();
    let retired = child.terminate_and_close(Instant::now() + Duration::from_secs(5));
    let copied = result.unwrap();
    retired.unwrap();
    assert_eq!(copied.get(0).unwrap(), b"synthetic-child-database");
    assert_eq!(copied.get(1).unwrap(), b"synthetic-child-wal");
    std::fs::remove_file(dir.0.join("Cookies")).unwrap();
    std::fs::remove_file(dir.0.join("Cookies-wal")).unwrap();
}
