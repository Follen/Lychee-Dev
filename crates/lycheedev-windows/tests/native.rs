use lycheedev_windows::{JobGuard, Process, WindowIdentity, durable_replace};
use std::{fs, os::windows::process::CommandExt, process::Command};
#[test]
fn own_process_identity_and_memory_usage() {
    let p = Process::open(std::process::id()).unwrap();
    p.verify().unwrap();
    let memory = p.memory().unwrap();
    assert_eq!(memory.identity, *p.identity());
    assert!(memory.private_commit > 0 && memory.working_set > 0);
    assert!(
        memory.peak_working_set >= memory.working_set
            && memory.peak_private_commit >= memory.private_commit
    );
}
#[test]
fn window_identity_mismatch_rejected() {
    let p = Process::open(std::process::id()).unwrap();
    let w = WindowIdentity {
        process: p.identity().clone(),
        hwnd: 0,
    };
    assert!(w.verify().is_err());
}
#[test]
fn durable_replace_updates_existing() {
    let root = std::env::temp_dir().join(format!("lycheedev-replace-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let src = root.join("new");
    let dst = root.join("state");
    fs::write(&dst, b"old").unwrap();
    fs::write(&src, b"new").unwrap();
    durable_replace(&src, &dst).unwrap();
    assert_eq!(fs::read(&dst).unwrap(), b"new");
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn job_drop_stops_owned_child() {
    let mut child = Command::new("cmd.exe")
        .args(["/c", "ping", "-n", "30", "127.0.0.1"])
        .creation_flags(0x08000000)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let guard = JobGuard::attach(&child).unwrap();
    assert!(child.try_wait().unwrap().is_none());
    guard.terminate().unwrap();
    child.wait().unwrap();
}
