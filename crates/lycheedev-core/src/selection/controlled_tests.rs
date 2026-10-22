use super::*;
use crate::control::{OperationControl, TrackedCancellation};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-controlled-pin-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let f = Self(root);
        crate::workspace::initialize(&f.home()).unwrap();
        f
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn blocker(&self, sql: &str) -> rusqlite::Connection {
        let c = rusqlite::Connection::open(self.home().join("state/toolkit.sqlite")).unwrap();
        c.busy_timeout(Duration::ZERO).unwrap();
        c.execute_batch(sql).unwrap();
        c
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let r = fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            r.unwrap();
        }
    }
}
fn spec() -> SelectionSpec {
    SelectionSpec {
        source: Some(SourcePin {
            repository: "wow-ui-source".into(),
            product: "retail".into(),
            requested_ref: "fixture".into(),
            exact_commit: "a".repeat(40),
            parser_revision: "fixture".into(),
        }),
        ..Default::default()
    }
}
fn control() -> (OperationControl, TrackedCancellation) {
    let c = TrackedCancellation::new();
    (
        OperationControl::tracked(Instant::now() + Duration::from_secs(3), c.clone()),
        c,
    )
}
#[test]
fn pin_and_load_precancel_reject_before_filesystem_work() {
    let f = Fixture::new();
    let absent = f.0.join("absent");
    let (c, s) = control();
    s.cancel();
    assert_eq!(
        pin_selection_controlled(&absent, spec(), &c)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert_eq!(
        load_pinned_controlled(&absent, "PIN-missing", &c)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert!(!absent.exists());
    assert!(!f.home().join("state/toolkit.sqlite").exists());
}
#[test]
fn controlled_parent_read_keeps_original_deadline_under_exclusive_sqlite_lock() {
    let f = Fixture::new();
    let parent = pin_selection(&f.home(), spec()).unwrap();
    let blocker = f.blocker("BEGIN EXCLUSIVE");
    let deadline = Instant::now() + Duration::from_millis(300);
    let c = OperationControl::uncancelable(deadline);
    let error = pin_selection_controlled(
        &f.home(),
        SelectionSpec {
            parent: parent.id.clone(),
            ..Default::default()
        },
        &c,
    )
    .unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert!(Instant::now() < deadline + Duration::from_secs(2));
    assert_eq!(c.deadline(), deadline);
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    assert_eq!(
        load_pinned_controlled(&f.home(), &parent.id, &OperationControl::default())
            .unwrap()
            .id,
        parent.id
    );
    let metadata = Metadata::open(&f.home(), false).unwrap();
    assert_eq!(metadata.list::<PinnedSet>("pin/", 10).unwrap().len(), 1);
    metadata.close().unwrap();
}
#[test]
fn controlled_pin_write_deadline_does_not_publish() {
    let f = Fixture::new();
    Metadata::open(&f.home(), true).unwrap().close().unwrap();
    let blocker = f.blocker("BEGIN IMMEDIATE");
    let deadline = Instant::now() + Duration::from_millis(300);
    let c = OperationControl::uncancelable(deadline);
    assert_eq!(
        pin_selection_controlled(&f.home(), spec(), &c)
            .unwrap_err()
            .code,
        "command.deadline"
    );
    assert!(Instant::now() < deadline + Duration::from_secs(2));
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    let m = Metadata::open(&f.home(), false).unwrap();
    assert!(m.list::<PinnedSet>("pin/", 10).unwrap().is_empty());
    m.close().unwrap();
    pin_selection_controlled(&f.home(), spec(), &OperationControl::default()).unwrap();
}
#[test]
fn cancellation_after_pin_commit_preserves_once_published_identity() {
    let f = Fixture::new();
    let (c, s) = control();
    let mut id = None;
    let error = pin_selection_observed(&f.home(), spec(), Some(&c), |pin| {
        id = Some(pin.id.clone());
        s.cancel();
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    let id = id.unwrap();
    let stored = load_pinned_controlled(&f.home(), &id, &OperationControl::default()).unwrap();
    assert_eq!(stored.id, identity(&stored).unwrap());
    assert_eq!(
        pin_selection_controlled(&f.home(), spec(), &c)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert_eq!(
        pin_selection_controlled(&f.home(), spec(), &OperationControl::default())
            .unwrap()
            .id,
        id
    );
    let m = Metadata::open(&f.home(), false).unwrap();
    assert_eq!(m.list::<PinnedSet>("pin/", 10).unwrap().len(), 1);
    assert_eq!(m.generation(&format!("pin/{id}")).unwrap(), Some(1));
    m.close().unwrap();
}
#[test]
fn controlled_pin_read_preserves_integrity_and_missing_diagnostics() {
    let f = Fixture::new();
    let pin = pin_selection(&f.home(), spec()).unwrap();
    let c = OperationControl::default();
    assert_eq!(
        serde_json::to_value(load_pinned(&f.home(), &pin.id).unwrap()).unwrap(),
        serde_json::to_value(load_pinned_controlled(&f.home(), &pin.id, &c).unwrap()).unwrap()
    );
    let old = load_pinned(&f.home(), "PIN-missing").unwrap_err();
    let new = load_pinned_controlled(&f.home(), "PIN-missing", &c).unwrap_err();
    assert_eq!(
        serde_json::to_value(old).unwrap(),
        serde_json::to_value(new).unwrap()
    );
    let mut changed = pin.clone();
    changed.id = "PIN-forged".into();
    let mut m = Metadata::open(&f.home(), true).unwrap();
    m.put(&format!("pin/{}", pin.id), &changed, Some(1))
        .unwrap();
    m.close().unwrap();
    assert_eq!(
        load_pinned_controlled(&f.home(), &pin.id, &c)
            .unwrap_err()
            .code,
        "selection.pin_integrity"
    );
}
