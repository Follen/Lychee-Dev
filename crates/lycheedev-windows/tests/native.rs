use lycheedev_windows::{JobGuard, NamedLock, Process, WindowIdentity, durable_replace};
use std::{fs, os::windows::process::CommandExt, process::Command};
#[test]
fn own_process_identity_and_sealed_module() {
    let p = Process::open(std::process::id()).unwrap();
    p.verify().unwrap();
    let memory = p.memory().unwrap();
    assert_eq!(memory.identity, *p.identity());
    assert!(memory.private_commit > 0 && memory.working_set > 0);
    assert!(
        memory.peak_working_set >= memory.working_set
            && memory.peak_private_commit >= memory.private_commit
    );
    let plan = p.main_module().unwrap();
    assert_eq!(plan.hash().len(), 64);
    assert_eq!(&plan.disk()[..2], b"MZ");
    let mut h = [0u8; 2];
    p.read_module(&plan, plan.base(), &mut h).unwrap();
    assert_eq!(&h, b"MZ");
    let bytes = [7u8; 8];
    assert!(p.read_module(&plan, bytes.as_ptr() as u64, &mut h).is_err());
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
fn mutex_serializes_other_thread() {
    let name = format!("Local\\LycheeDev.Test.{}", std::process::id());
    let a = NamedLock::acquire(&name).unwrap();
    let other = name.clone();
    let busy = std::thread::spawn(move || NamedLock::acquire(&other).is_err())
        .join()
        .unwrap();
    assert!(busy);
    drop(a);
    assert!(NamedLock::acquire(&name).is_ok());
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
