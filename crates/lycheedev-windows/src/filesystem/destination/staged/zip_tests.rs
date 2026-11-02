use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
fn home() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "lychee-stage-seek-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}
#[test]
fn private_seek_patches_only_the_original_unsealed_stage_and_seal_locks_it() {
    let path = home();
    let parent = Directory::open(&path).unwrap();
    let mut stage = StagedFile::create(&parent, &mut Uncontrolled).unwrap();
    stage.write_all(b"0000payload").unwrap();
    assert_eq!(stage.seek_private(SeekFrom::Start(0)).unwrap(), 0);
    stage.write_all(b"head").unwrap();
    assert_eq!(stage.seek_private(SeekFrom::End(0)).unwrap(), 11);
    stage.seal().unwrap();
    assert!(stage.seek_private(SeekFrom::Start(0)).is_err());
    assert!(stage.write_all(b"replay").is_err());
    let mut raw = Vec::new();
    stage.read_to_end(&mut raw).unwrap();
    assert_eq!(raw, b"headpayload");
    assert!(stage.close().is_empty());
    parent.close().unwrap();
    fs::remove_dir_all(path).unwrap();
}
#[test]
fn committed_and_unknown_stage_never_regain_seek_or_write_authority() {
    for uncertain in [false, true] {
        let path = home();
        let parent = Directory::open(&path).unwrap();
        let mut stage = StagedFile::create(&parent, &mut Uncontrolled).unwrap();
        stage.write_all(b"verified").unwrap();
        stage.seal().unwrap();
        let result = stage.publish_name_with(
            std::ffi::OsStr::new("output"),
            &mut Uncontrolled,
            |stage, name| {
                stage.rename(name, false)?;
                if uncertain {
                    Err(io::Error::other("completion lost"))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result.is_err(), uncertain);
        assert_eq!(
            stage.publication(),
            if uncertain {
                StagedPublication::Unknown
            } else {
                StagedPublication::Published
            }
        );
        assert!(stage.seek_private(SeekFrom::Start(0)).is_err());
        assert!(stage.write_all(b"replay").is_err());
        assert!(stage.close().is_empty());
        parent.close().unwrap();
        assert_eq!(fs::read(path.join("output")).unwrap(), b"verified");
        fs::remove_dir_all(path).unwrap();
    }
}
