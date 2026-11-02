use super::*;
use crate::control::{OperationControl, TrackedCancellation};
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-pin-supplemental-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        crate::workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn blocker(&self, sql: &str) -> rusqlite::Connection {
        let connection = rusqlite::Connection::open(self.0.join("state/toolkit.sqlite")).unwrap();
        connection.busy_timeout(Duration::ZERO).unwrap();
        connection.execute_batch(sql).unwrap();
        connection
    }
    fn count(&self) -> usize {
        let metadata = Metadata::open(&self.0, false).unwrap();
        let count = metadata.list::<Value>("pin/", 100).unwrap().len();
        metadata.close().unwrap();
        count
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
fn alive() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn independent() -> (OperationControl, TrackedCancellation) {
    let token = TrackedCancellation::new();
    (
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone()),
        token,
    )
}
fn spec(marker: char) -> SelectionSpec {
    SelectionSpec {
        source: Some(SourcePin {
            repository: "wow-ui-source".into(),
            product: "retail".into(),
            requested_ref: "fixture".into(),
            exact_commit: marker.to_string().repeat(40),
            parser_revision: "fixture".into(),
        }),
        ..Default::default()
    }
}
fn stopped(error: &ToolkitError, invocation: &OperationControl) {
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    invocation.check().unwrap();
}
fn release(blocker: rusqlite::Connection) {
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
}
fn same(left: &ToolkitError, right: &ToolkitError) -> bool {
    left.code == right.code
        && left.message == right.message
        && left.exit_code == right.exit_code
        && left.details == right.details
}
#[test]
fn supplemental_pin_pre_stop_and_invocation_precedence_have_no_side_effects() {
    let f = Fixture::new();
    let invocation = alive();
    let (budget, token) = independent();
    token.cancel();
    let absent = f.0.join("absent");
    stopped(
        &load_pinned_controlled_with_check(&absent, "PIN-missing", &invocation, || budget.check())
            .unwrap_err(),
        &invocation,
    );
    stopped(
        &pin_selection_controlled_with_check(&absent, spec('a'), &invocation, || budget.check())
            .unwrap_err(),
        &invocation,
    );
    assert!(!absent.exists());
    let expired = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    let called = Cell::new(false);
    let error = pin_selection_controlled_with_check(&absent, spec('a'), &expired, || {
        called.set(true);
        budget.check()
    })
    .unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert!(!called.get());
}
#[test]
fn supplemental_parent_read_stops_inside_sqlite_contention_without_child_pin() {
    let f = Fixture::new();
    let parent = pin_selection(&f.0, spec('a')).unwrap();
    let invocation = alive();
    let (budget, token) = independent();
    let blocker = f.blocker("BEGIN EXCLUSIVE");
    let checks = Cell::new(0);
    let error = pin_selection_controlled_with_check(
        &f.0,
        SelectionSpec {
            parent: parent.id.clone(),
            ..Default::default()
        },
        &invocation,
        || {
            checks.set(checks.get() + 1);
            if checks.get() == 24 {
                token.cancel();
            }
            budget.check()
        },
    )
    .unwrap_err();
    stopped(&error, &invocation);
    assert!(checks.get() >= 24);
    // The current controlled stop returns the plain typed command stop; the old
    // partial-result details are no longer part of the pin-read contract.
    assert!(error.details.is_null());
    release(blocker);
    assert_eq!(f.count(), 1);
    assert_eq!(load_pinned(&f.0, &parent.id).unwrap().id, parent.id);
}
#[test]
fn supplemental_pin_write_stops_during_sqlite_wait_and_rolls_back() {
    let f = Fixture::new();
    pin_selection(&f.0, spec('a')).unwrap();
    let invocation = alive();
    let (budget, token) = independent();
    let blocker = f.blocker("BEGIN IMMEDIATE");
    let checks = Cell::new(0);
    let error = pin_selection_controlled_with_check(&f.0, spec('b'), &invocation, || {
        checks.set(checks.get() + 1);
        if checks.get() == 40 {
            token.cancel();
        }
        budget.check()
    })
    .unwrap_err();
    stopped(&error, &invocation);
    assert!(checks.get() >= 40);
    release(blocker);
    assert_eq!(f.count(), 1);
    pin_selection_controlled_with_check(&f.0, spec('b'), &invocation, || Ok(())).unwrap();
    assert_eq!(f.count(), 2);
}
#[test]
fn supplemental_pin_after_publication_keeps_exact_generation_and_parent() {
    let f = Fixture::new();
    let invocation = alive();
    let (budget, token) = independent();
    let published = Cell::new(0);
    let error = pin_selection_observed_with_check(
        &f.0,
        spec('a'),
        &invocation,
        || budget.check(),
        |_| {
            published.set(published.get() + 1);
            token.cancel();
        },
    )
    .unwrap_err();
    stopped(&error, &invocation);
    assert_eq!(published.get(), 1);
    assert_eq!(f.count(), 1);
    let pin = pin_selection_controlled_with_check(&f.0, spec('a'), &invocation, || Ok(())).unwrap();
    let metadata = Metadata::open(&f.0, false).unwrap();
    assert_eq!(
        metadata.generation(&format!("pin/{}", pin.id)).unwrap(),
        Some(1)
    );
    metadata.close().unwrap();
    stopped(
        &pin_selection_controlled_with_check(&f.0, spec('b'), &invocation, || budget.check())
            .unwrap_err(),
        &invocation,
    );
    assert_eq!(f.count(), 1);
}
#[test]
fn supplemental_noop_preserves_typed_pin_identity_and_error_diagnostics() {
    let f = Fixture::new();
    let invocation = alive();
    let left = pin_selection_controlled(&f.0, spec('a'), &invocation).unwrap();
    let right =
        pin_selection_controlled_with_check(&f.0, spec('a'), &invocation, || Ok(())).unwrap();
    assert_eq!(
        serde_json::to_value(&left).unwrap(),
        serde_json::to_value(right).unwrap()
    );
    assert_eq!(
        serde_json::to_value(load_pinned_controlled(&f.0, &left.id, &invocation).unwrap()).unwrap(),
        serde_json::to_value(
            load_pinned_controlled_with_check(&f.0, &left.id, &invocation, || Ok(())).unwrap()
        )
        .unwrap()
    );
    for id in ["PIN-missing", ""] {
        let old = load_pinned_controlled(&f.0, id, &invocation).unwrap_err();
        let new = load_pinned_controlled_with_check(&f.0, id, &invocation, || Ok(())).unwrap_err();
        assert!(same(&old, &new));
        assert_eq!(old.exit_code, 3);
        assert_eq!(old.message, format!("document pin/{id} does not exist"));
    }
    let conflict = SelectionSpec {
        parent: left.id,
        source: spec('b').source,
        ..Default::default()
    };
    let old = pin_selection_controlled(&f.0, conflict.clone(), &invocation).unwrap_err();
    let new =
        pin_selection_controlled_with_check(&f.0, conflict, &invocation, || Ok(())).unwrap_err();
    assert!(same(&old, &new));
}
#[test]
fn supplemental_typed_row_read_observes_stop_inside_actual_sqlite_wait() {
    let f = Fixture::new();
    let pin = pin_selection(&f.0, spec('a')).unwrap();
    let invocation = alive();
    let (budget, token) = independent();
    let metadata = Metadata::open_controlled(&f.0, false, &invocation).unwrap();
    let blocker = f.blocker("BEGIN EXCLUSIVE");
    let checks = Cell::new(0);
    let error = metadata
        .read_controlled_with_check::<PinnedSet>(&format!("pin/{}", pin.id), &invocation, || {
            checks.set(checks.get() + 1);
            if checks.get() == 15 {
                token.cancel();
            }
            budget.check()
        })
        .unwrap_err();
    stopped(&error, &invocation);
    assert!(checks.get() >= 15);
    assert!(!error.details.is_null());
    metadata.close().unwrap();
    release(blocker);
    assert_eq!(load_pinned(&f.0, &pin.id).unwrap().id, pin.id);
}
#[test]
fn supplemental_typed_noop_preserves_missing_and_malformed_row_errors() {
    let f = Fixture::new();
    let invocation = alive();
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    metadata
        .put("value", &serde_json::json!({"wrong":"shape"}), None)
        .unwrap();
    for key in ["value", "missing"] {
        let left = metadata
            .read_controlled::<bool>(key, &invocation)
            .unwrap_err();
        let right = metadata
            .read_controlled_with_check::<bool>(key, &invocation, || Ok(()))
            .unwrap_err();
        assert!(same(&left, &right));
    }
    metadata.close().unwrap();
}
