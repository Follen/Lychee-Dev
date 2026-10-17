use super::*;
use lycheedev_core::{control::OperationControl, selection::SelectionSpec, workspace};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    home: PathBuf,
    inv: Invocation,
    pin: PinnedSet,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
fn fixture() -> Fixture {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let home = std::env::temp_dir().join(format!(
        "lychee-selected-research-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&home).expect("fresh project-selection fixture");
    workspace::initialize(&home).unwrap();
    let mirror = git::mirror(&home, "elvui");
    fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&mirror)
            .output()
            .unwrap()
            .status
            .success()
    );
    let run = |args: &[&str], input: &str| {
        let mut child = Command::new("git")
            .arg(format!("--git-dir={}", mirror.display()))
            .args(args)
            .env("GIT_AUTHOR_NAME", "Research Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Research Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let blob = run(
        &["hash-object", "-w", "--stdin"],
        "function Target() end\nTarget()\n",
    );
    let tree = run(&["mktree"], &format!("100644 blob {blob}\tmain.lua\n"));
    let commit = run(&["commit-tree", &tree], "project selection fixture");
    let pin = selection::pin_selection(
        &home,
        SelectionSpec {
            source: Some(selection::SourcePin {
                repository: "elvui".into(),
                product: "main".into(),
                requested_ref: "fixed".into(),
                exact_commit: commit,
                parser_revision: "source-v1".into(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let project = home.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("lycheedev.json"),
        br#"{"schema":"lycheedev.project.v1","product":"main"}"#,
    )
    .unwrap();
    fs::write(
        project.join("lycheedev.lock.json"),
        serde_json::to_vec(&json!({"schema":"lycheedev.project-lock.v1","selection":pin})).unwrap(),
    )
    .unwrap();
    let inv = Invocation {
        command: "source context".into(),
        positional: None,
        flags: std::collections::BTreeMap::from([
            ("--symbol".into(), "Target".into()),
            ("--static-only".into(), "true".into()),
        ]),
        home: home.clone(),
        cwd: project,
        control: Default::default(),
    };
    Fixture { home, inv, pin }
}

#[test]
fn context_selected_pin_handoff_survives_replaced_project_without_reselection() {
    let f = fixture();
    let selected = resolve_research_pin(&f.inv).unwrap();
    // Deterministic lock change at the exact context -> relations boundary.
    // The relation evaluator must keep the cursor's original authenticated pin.
    fs::write(
        f.inv.cwd.join("lycheedev.lock.json"),
        b"replaced after selection",
    )
    .unwrap();
    let reading =
        with_selected_pin(&f.inv, &crate::SourceTools::default(), None, selected).unwrap();
    assert_eq!(reading.pin.id, f.pin.id);
    assert_eq!(reading.data["snapshot"], f.pin.id);
    assert_eq!(reading.data["symbol"]["name"], "Target");
    assert!(!reading.data["relations"].as_array().unwrap().is_empty());
    assert_eq!(
        resolve_research_pin(&f.inv).unwrap_err().code,
        "selection.project_format"
    );
}

#[test]
fn context_selected_pin_handoff_keeps_cancellation_and_deadline_after_selection() {
    let mut f = fixture();
    let selected = resolve_research_pin(&f.inv).unwrap();
    let deadline = f.inv.control.deadline();
    f.inv.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        with_selected_pin(&f.inv, &crate::SourceTools::default(), None, selected)
            .err()
            .unwrap()
            .code,
        "command.cancelled"
    );
    assert_eq!(f.inv.control.deadline(), deadline);
    f.inv.control = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        with_selected_pin(&f.inv, &crate::SourceTools::default(), None, f.pin.clone())
            .err()
            .unwrap()
            .code,
        "command.deadline"
    );
}
