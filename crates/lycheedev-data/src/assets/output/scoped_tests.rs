use super::*;
use lycheedev_core::{control::OperationControl, workspace};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
fn fixture() -> (PathBuf, Invocation) {
    let root = std::env::temp_dir().join(format!(
        "lychee-assets-scoped-{}",
        lycheedev_core::storage::sha256(
            format!("{}-{:?}", std::process::id(), std::time::SystemTime::now()).as_bytes()
        )
    ));
    fs::create_dir(&root).unwrap();
    let home = root.join("home");
    workspace::initialize(&home).unwrap();
    let inv = Invocation {
        command: "asset export".into(),
        positional: None,
        flags: BTreeMap::new(),
        home,
        cwd: root.clone(),
        control: OperationControl::default(),
    };
    (root, inv)
}
#[test]
fn cancellation_after_real_publication_preserves_state_and_checked_destination_close() {
    let (root, inv) = fixture();
    let path = root.join("body");
    let dst = destination_scoped(&inv, &path, false).unwrap();
    let mut observations = 0;
    let result = publish_reader_scoped(
        &inv,
        &dst,
        &mut io::Cursor::new(b"body"),
        4,
        4,
        None,
        &mut |state| {
            observations += 1;
            assert_eq!(state, lycheedev_core::storage::StagedPublication::Published);
            inv.control.tracked_cancel_handle().unwrap().cancel();
        },
    );
    let error = finish_closes(result, dst.close(), &inv.control).unwrap_err();
    assert_eq!(observations, 1);
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(fs::read(path).unwrap(), b"body");
    let rendered = serde_json::to_string(&error.details).unwrap();
    assert!(rendered.contains("lycheedev.asset-publication-error.v1"));
    assert!(rendered.contains("published"));
    assert!(error.message.contains("already published"));
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn two_identical_injected_close_causes_are_preserved_with_primary() {
    let inv = OperationControl::default();
    let primary = ToolkitError::new("records.metadata_limit", "primary", 3);
    let error = finish_closes(
        Err::<(), _>(primary.clone()),
        Err(vec![
            io::Error::other("same-close"),
            io::Error::other("same-close"),
        ]),
        &inv,
    )
    .unwrap_err();
    assert_eq!(error.code, primary.code);
    assert_eq!(error.exit_code, 3);
    assert_eq!(error.details["cleanup"]["message"], "same-close");
    assert_eq!(
        error.details["primary"]["details"]["cleanup"]["message"],
        "same-close"
    );
}
#[test]
fn directory_lease_wait_observes_independent_budget_and_releases_after_timeout() {
    let (root, inv) = fixture();
    let out = root.join("out");
    fs::create_dir(&out).unwrap();
    let dir = OutputDirectory::open_scoped(&inv, &out, false).unwrap();
    let held = dir.lease(&inv, None).unwrap();
    let control = OperationControl::default();
    let budget = crate::resource::Budget::controlled(
        crate::resource::Cost::defaults(),
        Instant::now() + Duration::from_millis(40),
        control,
    );
    let error = match dir.lease(&inv, Some(&budget)) {
        Err(e) => e,
        Ok(owner) => {
            owner.close().unwrap();
            panic!("contended lease acquired")
        }
    };
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(error.exit_code, 3);
    inv.control.check().unwrap();
    held.close().unwrap();
    dir.lease(&inv, None).unwrap().close().unwrap();
    dir.close().unwrap();
    fs::remove_dir_all(root).unwrap();
}
