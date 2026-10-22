use super::*;
use lycheedev_core::{control::OperationControl, workspace};
use std::{
    time::{Duration, Instant},
};
fn fixture() -> (PathBuf, DataContext) {
    let root = std::env::temp_dir().join(format!(
        "lychee-assets-scoped-{}",
        lycheedev_core::storage::sha256(
            format!("{}-{:?}", std::process::id(), std::time::SystemTime::now()).as_bytes()
        )
    ));
    fs::create_dir(&root).unwrap();
    let home = root.join("home");
    workspace::initialize(&home).unwrap();
    let inv = DataContext {
        home,
        cwd: root.clone(),
        control: OperationControl::default(),
        source: Default::default(),
    };
    (root, inv)
}

#[test]
fn external_stage_respects_data_capacity_and_releases_after_publication() {
    let (root, ctx) = fixture();
    let mut config = workspace::default_config();
    config["dataCache"]["maxBytes"] = serde_json::json!(3);
    lycheedev_core::storage::atomic_json(&ctx.home.join("config.json"), &config).unwrap();
    let path = root.join("body");
    fs::write(&path, b"prior").unwrap();
    let dst = destination_scoped(&ctx, &path, true).unwrap();
    let mut observed = lycheedev_core::storage::StagedPublication::NotPublished;
    let error = publish_reader_scoped(
        &ctx,
        &dst,
        &mut io::Cursor::new(b"body"),
        4,
        4,
        None,
        &mut |state| observed = state,
    ).unwrap_err();
    assert_eq!(error.code, "vault.data_cache_budget_exceeded");
    assert_eq!(observed, lycheedev_core::storage::StagedPublication::NotPublished);
    assert_eq!(fs::read(&path).unwrap(), b"prior");
    assert!(fs::read_dir(&root).unwrap().all(|entry| {
        !entry.unwrap().file_name().to_string_lossy().starts_with(".lycheedev-export-")
    }), "no external stage was created");

    // The 128-byte immutable reservation record shares the physical budget.
    config["dataCache"]["maxBytes"] = serde_json::json!(4 + 128);
    lycheedev_core::storage::atomic_json(&ctx.home.join("config.json"), &config).unwrap();
    for raw in [b"body", b"next"] {
        publish_reader_scoped(
            &ctx, &dst, &mut io::Cursor::new(raw), 4, 4, None,
            &mut |state| observed = state,
        ).unwrap();
        assert_eq!(observed, lycheedev_core::storage::StagedPublication::Published);
        assert_eq!(fs::read(&path).unwrap(), raw);
    }
    dst.close().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn external_export_process_exit_preserves_pending_capacity_and_user_files() {
    use std::os::windows::process::CommandExt;
    const CHILD_ROOT: &str = "LYCHEEDEV_EXPORT_EXIT_ROOT";
    const CHILD_CASE: &str = "LYCHEEDEV_EXPORT_EXIT_CASE";
    const LENGTH: u64 = 16;
    const EXIT: i32 = 76;
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        let ctx = DataContext {
            home: root.join("home"), cwd: root.clone(),
            control: OperationControl::default(), source: Default::default(),
        };
        let destination = destination_scoped(&ctx, &root.join("body"), true).unwrap();
        if std::env::var(CHILD_CASE).unwrap() == "during-write" {
            struct InterruptedReader(bool);
            impl io::Read for InterruptedReader {
                fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                    if self.0 {
                        // Exit releases OS handles without running Stage/record Drop.
                        std::process::exit(EXIT);
                    }
                    self.0 = true;
                    output[..8].copy_from_slice(b"partial!");
                    Ok(8)
                }
            }
            publish_reader_scoped(
                &ctx, &destination, &mut InterruptedReader(false), LENGTH, LENGTH as usize,
                None, &mut |_| {},
            ).unwrap();
        } else {
            publish_reader_scoped(
                &ctx, &destination, &mut io::Cursor::new([b'x'; LENGTH as usize]),
                LENGTH, LENGTH as usize, None,
                &mut |state| {
                    assert_eq!(state, lycheedev_core::storage::StagedPublication::Published);
                    std::process::exit(EXIT);
                },
            ).unwrap();
        }
        panic!("child must exit at the publication boundary");
    }

    for case in ["during-write", "after-publication"] {
        let (root, ctx) = fixture();
        let path = root.join("body");
        fs::write(&path, b"prior").unwrap();
        let mut config = workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(LENGTH + 128);
        lycheedev_core::storage::atomic_json(&ctx.home.join("config.json"), &config).unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "assets::output::scoped_tests::external_export_process_exit_preserves_pending_capacity_and_user_files",
                "--nocapture", "--test-threads=1",
            ])
            .env(CHILD_ROOT, &root).env(CHILD_CASE, case)
            .creation_flags(0x0800_0000)
            .output().unwrap();
        assert_eq!(child.status.code(), Some(EXIT), "{}\n{}",
            String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
        let stages = fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().path())
            .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with(".lycheedev-export-"))
            .collect::<Vec<_>>();
        if case == "during-write" {
            assert_eq!(fs::read(&path).unwrap(), b"prior");
            assert_eq!(stages.len(), 1);
            assert_eq!(fs::read(&stages[0]).unwrap(), b"partial!");
        } else {
            assert_eq!(fs::read(&path).unwrap(), [b'x'; LENGTH as usize]);
            assert!(stages.is_empty());
        }
        let check = || Ok(());
        let error = match workspace::data_disk::reserve_export_temporary_with_check(
            &ctx.home, 1, &ctx.control, &check,
        ) {
            Err(error) => error,
            Ok(owner) => {
                owner.finish(Ok(())).unwrap();
                panic!("unsettled external publication must remain charged");
            }
        };
        assert_eq!(error.code, "vault.data_cache_budget_exceeded");
        assert_eq!(error.details["reservation"]["existing"]["pendingExternalExportBytes"], LENGTH);
        fs::remove_dir_all(root).unwrap();
    }
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
