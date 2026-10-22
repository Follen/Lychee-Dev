#![cfg(windows)]
use lycheedev_windows::{Process, broker::NativeSourcePeer};
use std::{
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "child process for exact native Source owner retirement"]
fn native_owner_fixture() {
    std::thread::sleep(Duration::from_secs(30));
}
#[test]
fn native_owner_retirement_checks_creation_and_reaps_exact_process() {
    let executable = std::env::current_exe().unwrap();
    let mut child = Command::new(&executable)
        .args(["native_owner_fixture", "--exact", "--ignored"])
        .creation_flags(0x08000000)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let owner = Process::open(child.id()).unwrap();
    let created = owner.identity().created;
    let deadline = Instant::now() + Duration::from_secs(3);
    assert_eq!(
        NativeSourcePeer::open(child.id(), created + 1, &executable)
            .and_then(|peer| peer.retire(deadline))
            .unwrap_err(),
        "source.owner_identity_changed"
    );
    assert!(child.try_wait().unwrap().is_none());
    NativeSourcePeer::open(child.id(), created, &executable)
        .and_then(|peer| peer.retire(deadline))
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(5));
    assert_eq!(
        owner.wait_exit_with_control(deadline, || Ok(())).unwrap(),
        5
    );
}

#[test]
fn authenticated_owner_exit_during_request_keeps_retirement_handle_usable() {
    let executable = std::env::current_exe().unwrap();
    let mut child = Command::new(&executable)
        .args(["native_owner_fixture", "--exact", "--ignored"])
        .creation_flags(0x08000000)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let owner = Process::open(child.id()).unwrap();
    let peer = NativeSourcePeer::open(child.id(), owner.identity().created, &executable).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    peer.retire(Instant::now() + Duration::from_secs(3))
        .unwrap();
    peer.retire(Instant::now() + Duration::from_secs(3))
        .unwrap();
}
