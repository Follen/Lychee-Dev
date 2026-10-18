//! Go 3766f9c dispatch_workspace_test.go:369 and command_contract_test.go:289.
//! New Rust regressions are NOT_RUN until the controlled verification batch.
use super::*;
use lycheedev_core::{archive, storage, workspace};
use std::{fs, path::Path};

static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-init-contract-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn run(&self, root: &Path, mode: &str) -> Result<Outcome> {
        let mut args = vec![
            "init".to_owned(),
            "--home".into(),
            root.to_string_lossy().into_owned(),
        ];
        if !mode.is_empty() {
            args.push(mode.into());
        }
        let invocation = parse(&args)?;
        dispatch(&invocation)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (bool, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, result: &mut BTreeMap<PathBuf, (bool, Vec<u8>)>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let directory = entry.file_type().unwrap().is_dir();
            result.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                (
                    directory,
                    if directory {
                        vec![]
                    } else {
                        fs::read(&path).unwrap()
                    },
                ),
            );
            if directory {
                visit(root, &path, result);
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
#[test]
fn init_rejects_every_mode_combination_before_dispatch_and_preserves_help() {
    for modes in [
        vec!["--plan", "--fresh"],
        vec!["--plan", "--resume"],
        vec!["--fresh", "--resume"],
        vec!["--plan", "--fresh", "--resume"],
    ] {
        let mut args = vec!["init".into()];
        args.extend(modes.into_iter().map(str::to_owned));
        let error = parse(&args).unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(error.exit_code, 2);
        assert_eq!(
            error.message,
            "init accepts at most one of --plan, --fresh, --resume"
        );
        args.push("--help".into());
        let invocation = parse(&args).unwrap();
        assert!(dispatch(&invocation).unwrap().complete);
    }
    for mode in ["--plan", "--fresh", "--resume"] {
        assert!(parse(&["init".into(), mode.into()]).is_ok());
    }
}
#[test]
fn standalone_init_plan_is_read_only_for_absent_empty_and_opaque_legacy_roots() {
    let f = Fixture::new();
    let absent = f.0.join("missing-parent/home");
    let empty = f.0.join("empty");
    let legacy = f.0.join("legacy");
    fs::create_dir(&empty).unwrap();
    fs::create_dir(&legacy).unwrap();
    fs::write(legacy.join("state"), [0xff, 0, 0x80]).unwrap();
    let before = snapshot(&f.0);
    for root in [&absent, &empty, &legacy] {
        let result = f.run(root, "--plan").unwrap();
        assert_eq!(result.data["state"], "planned");
        assert_eq!(result.data["root"], serde_json::to_value(root).unwrap());
        assert!(!archive::journal_path(root).unwrap().exists());
        assert_eq!(snapshot(&f.0), before);
    }
}
#[test]
fn init_resume_without_journal_never_initializes_or_creates_parent_locks() {
    let f = Fixture::new();
    let absent = f.0.join("missing-parent/home");
    let empty = f.0.join("empty");
    let legacy = f.0.join("legacy");
    let existing = f.0.join("existing");
    fs::create_dir(&empty).unwrap();
    fs::create_dir(&legacy).unwrap();
    fs::write(legacy.join("automation.py"), b"old").unwrap();
    workspace::initialize(&existing).unwrap();
    let before = snapshot(&f.0);
    for root in [&absent, &empty, &legacy, &existing] {
        let error = f.run(root, "--resume").unwrap_err();
        assert_eq!(error.code, "vault.archive_state");
        assert_eq!(error.exit_code, 3);
        assert_eq!(snapshot(&f.0), before);
    }
}
#[test]
fn init_resume_recovers_planned_moved_and_created_checkpoints_without_reimport() {
    for (state, already_moved, already_created) in [
        ("planned", false, false),
        ("planned", true, false),
        ("moved", true, false),
        ("moved", true, true),
    ] {
        let f = Fixture::new();
        let root = f.0.join("interrupted");
        fs::create_dir(&root).unwrap();
        let opaque = [0xff, 0, 0x80, 1, 2];
        fs::write(root.join("automation.py"), opaque).unwrap();
        let mut plan = f.run(&root, "--plan").unwrap().data;
        plan["state"] = json!(state);
        let journal = archive::journal_path(&root).unwrap();
        let archive = PathBuf::from(plan["archivePath"].as_str().unwrap());
        storage::atomic_json(&journal, &plan).unwrap();
        if already_moved {
            fs::rename(&root, &archive).unwrap();
        }
        let previous_identity = if already_created {
            Some(workspace::initialize(&root).unwrap())
        } else {
            None
        };
        let before = snapshot(&f.0);
        assert_eq!(f.run(&root, "--plan").unwrap().data, plan);
        assert_eq!(snapshot(&f.0), before);
        let result = f.run(&root, "--resume").unwrap().data;
        assert_eq!(result["plan"]["state"], "complete");
        assert_eq!(result["plan"]["archivePath"], plan["archivePath"]);
        assert_eq!(fs::read(archive.join("automation.py")).unwrap(), opaque);
        assert!(!root.join("automation.py").exists());
        assert!(root.join("workspace.json").is_file());
        assert_eq!(
            storage::read_json::<Value>(&journal).unwrap(),
            result["plan"]
        );
        if let Some(identity) = previous_identity {
            assert_eq!(result["summary"]["identity"], identity);
        }
        let before_repeat = snapshot(&f.0);
        assert_eq!(f.run(&root, "--resume").unwrap().data, result);
        assert_eq!(f.run(&root, "--fresh").unwrap().data, result);
        assert_eq!(snapshot(&f.0), before_repeat);
    }
}
#[test]
fn init_resume_invalid_journal_is_read_only_and_never_falls_back_to_initialize() {
    let f = Fixture::new();
    let root = f.0.join("interrupted");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("opaque.bin"), b"preserve").unwrap();
    let mut plan = f.run(&root, "--plan").unwrap().data;
    plan["archivePath"] = json!(f.0.join("unrelated-owner"));
    storage::atomic_json(&archive::journal_path(&root).unwrap(), &plan).unwrap();
    let before = snapshot(&f.0);
    assert_eq!(
        f.run(&root, "--resume").unwrap_err().code,
        "vault.archive_state"
    );
    assert_eq!(snapshot(&f.0), before);
}

#[test]
fn init_original_stop_scope_is_checked_before_any_mode_touches_the_filesystem() {
    use lycheedev_core::control::OperationControl;
    use std::{
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    let root = f.0.join("missing-parent/home");
    let before = snapshot(&f.0);
    for mode in ["", "--plan", "--fresh", "--resume"] {
        for (cancelled, expired, expected) in [
            (true, false, "command.cancelled"),
            (false, true, "command.deadline"),
        ] {
            let mut args = vec![
                "init".into(),
                "--home".into(),
                root.to_string_lossy().into_owned(),
            ];
            if !mode.is_empty() {
                args.push(mode.into());
            }
            let mut invocation = parse(&args).unwrap();
            invocation.control = OperationControl::new(
                if expired {
                    Instant::now() - Duration::from_secs(1)
                } else {
                    Instant::now() + Duration::from_secs(60)
                },
                Arc::new(AtomicBool::new(cancelled)),
            );
            assert_eq!(dispatch(&invocation).unwrap_err().code, expected);
            assert_eq!(snapshot(&f.0), before);
        }
    }
}
