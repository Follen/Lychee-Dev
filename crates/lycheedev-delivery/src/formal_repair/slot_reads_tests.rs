use crate::slots::*;
use lycheedev_core::control::OperationControl;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "formal-slot-reads-{}-{}-{}",
            std::process::id(),
            lycheedev_core::storage::utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn formal_slot_read_preserves_exact_bytes_and_closes_before_return() {
    let f = Fixture::new();
    let path = f.0.join("file");
    fs::write(&path, b"payload").unwrap();
    assert_eq!(
        read_bounded_controlled(&path, 7, &OperationControl::default()).unwrap(),
        b"payload"
    );
    fs::write(&path, b"changed").unwrap();
    assert_eq!(
        read_bounded_controlled(&path, 7, &OperationControl::default()).unwrap(),
        b"changed"
    );
}
#[test]
fn formal_slot_read_uses_original_cancel_and_never_adopts_oversize() {
    let f = Fixture::new();
    let path = f.0.join("file");
    fs::write(&path, b"payload").unwrap();
    let c = OperationControl::default();
    c.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        read_bounded_controlled(&path, 7, &c).unwrap_err().code,
        "command.cancelled"
    );
    assert!(read_bounded_controlled(&path, 6, &OperationControl::default()).is_err());
    assert_eq!(fs::read(path).unwrap(), b"payload");
}
#[test]
fn formal_slot_pool_read_uses_original_control_before_missing_marker() {
    let f = Fixture::new();
    let c = OperationControl::default();
    c.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        read_pool_controlled(&f.0, &c).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 0);
}
