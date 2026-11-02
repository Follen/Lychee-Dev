//! An actual inheritable anonymous pipe must reach EOF while the owned child is still alive.
use lycheedev_windows::{Process, detached::spawn_hidden_no_inherit};
use std::{
    ffi::OsString,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_BROKEN_PIPE, HANDLE},
    Security::SECURITY_ATTRIBUTES,
    System::Pipes::{CreatePipe, PeekNamedPipe},
};
struct Pipe(HANDLE);
impl Drop for Pipe {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
fn pipe() -> (Pipe, Pipe) {
    let (mut reader, mut writer) = (HANDLE::default(), HANDLE::default());
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    unsafe {
        CreatePipe(&mut reader, &mut writer, Some(&security), 0).unwrap();
    }
    (Pipe(reader), Pipe(writer))
}
#[test]
#[ignore = "child fixture only; invoked by no-inherit parent regressions"]
fn owned_noinherit_entry() {
    assert_eq!(
        std::env::var("LYCHEEDEV_DETACHED_FIXTURE").expect("required owned child mode"),
        "owned-test"
    );
    std::thread::sleep(Duration::from_secs(3));
}
#[test]
fn no_inherit_releases_parent_pipe_before_owned_child_exit_and_preserves_identity() {
    let exe = std::env::current_exe().unwrap();
    let cwd = exe.parent().unwrap();
    let args = [
        OsString::from("--exact"),
        OsString::from("owned_noinherit_entry"),
        OsString::from("--ignored"),
        OsString::from("--nocapture"),
    ];
    // Control uses the same explicitly inheritable private pipe; it never changes global STD flags.
    let (reader, writer) = pipe();
    let mut control = Command::new(&exe)
        .args(&args)
        .env("LYCHEEDEV_DETACHED_FIXTURE", "owned-test")
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    drop(writer);
    assert!(
        unsafe { PeekNamedPipe(reader.0, None, 0, None, None, None) }.is_ok(),
        "broad std Command control keeps unused parent pipe open"
    );
    assert!(control.try_wait().unwrap().is_none());
    control.kill().unwrap();
    control.wait().unwrap();
    drop(reader);
    let (reader, writer) = pipe();
    let parent_env = std::env::var_os("LYCHEEDEV_DETACHED_FIXTURE");
    let child = spawn_hidden_no_inherit(
        &exe,
        &args,
        cwd,
        &[("LYCHEEDEV_DETACHED_FIXTURE".into(), "owned-test".into())],
    )
    .unwrap();
    assert_eq!(
        std::env::var_os("LYCHEEDEV_DETACHED_FIXTURE"),
        parent_env,
        "launch does not change global env"
    );
    let process = Process::open(child.id()).unwrap();
    assert_eq!(process.identity(), child.identity());
    assert_eq!(process.parent_pid().unwrap(), std::process::id());
    drop(writer);
    let eof = unsafe { PeekNamedPipe(reader.0, None, 0, None, None, None) }.unwrap_err();
    assert_eq!(eof.code(), ERROR_BROKEN_PIPE.to_hresult());
    assert!(
        child.try_wait().unwrap().is_none(),
        "EOF before daemon exit, not a delayed callback inference"
    );
    let identity = child.identity().clone();
    assert_eq!(
        child
            .terminate_and_close(Instant::now() + Duration::from_secs(2))
            .unwrap(),
        7
    );
    assert_eq!(
        process
            .wait_exit_with_control(Instant::now() + Duration::from_secs(1), || Ok(()))
            .unwrap(),
        7
    );
    assert!(process.verify().is_err());
    process.close().unwrap();
    println!(
        "actual no-inherit child pid={} created={} parent={} pipeEOFBeforeChildExit=true",
        identity.pid,
        identity.created,
        std::process::id()
    );
}
#[test]
fn failed_startup_drop_reaps_exact_owned_child_and_bounds_reject_before_spawn() {
    let exe = std::env::current_exe().unwrap();
    let cwd = exe.parent().unwrap();
    let args = [
        OsString::from("--exact"),
        OsString::from("owned_noinherit_entry"),
        OsString::from("--ignored"),
    ];
    let child = spawn_hidden_no_inherit(
        &exe,
        &args,
        cwd,
        &[("LYCHEEDEV_DETACHED_FIXTURE".into(), "owned-test".into())],
    )
    .unwrap();
    let process = Process::open(child.id()).unwrap();
    drop(child);
    assert!(process.verify().is_err());
    assert!(
        process
            .wait_exit_with_control(Instant::now() + Duration::from_secs(1), || Ok(()))
            .is_ok()
    );
    process.close().unwrap();
    let own = Process::open(std::process::id()).unwrap();
    assert!(
        own.wait_exit_with_control(Instant::now() + Duration::from_millis(5), || Ok(()))
            .unwrap_err()
            .starts_with("command.deadline")
    );
    assert!(
        own.wait_exit_with_control(Instant::now() + Duration::from_secs(1), || Err(
            "command.cancelled".into()
        ))
        .unwrap_err()
        .starts_with("command.cancelled")
    );
    own.close().unwrap();
    assert!(
        spawn_hidden_no_inherit(std::path::Path::new("relative.exe"), &args, cwd, &[]).is_err()
    );
    assert!(spawn_hidden_no_inherit(&exe, &[OsString::from("nul\0arg")], cwd, &[]).is_err());
    assert!(
        spawn_hidden_no_inherit(&exe, &args, cwd, &[("BAD=KEY".into(), "value".into())]).is_err()
    );
}
