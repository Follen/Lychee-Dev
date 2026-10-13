// Actual two-process platform fixture; shared child code is test-only.
mod support {
    pub mod owned_target;
}
use lycheedev_windows::{WindowIdentity, input};
use std::fs;
use support::owned_target::{self, Owned};
#[test]
fn owned_child_entry() {
    owned_target::child_entry();
}
#[test]
fn two_actual_pid_windows_keep_directed_input_memory_and_lifetime_isolated() {
    let mut a = Owned::spawn("owned_child_entry");
    let mut b = Owned::spawn("owned_child_entry");
    assert_ne!(a.target.process.pid, b.target.process.pid);
    assert_eq!(a.hash, b.hash, "same actual executable build");
    assert_ne!(a.target.hwnd, b.target.hwnd);
    assert_eq!(a.bytes(), format!("owned-instance:{}", a.marker).as_bytes());
    assert_eq!(b.bytes(), format!("owned-instance:{}", b.marker).as_bytes());
    assert_eq!((a.state(), b.state()), (0, 0));
    let chord = input::chord(&a.target, &[0x11, 0x12, 0x7b], || a.target.verify()).unwrap();
    assert!(chord.submission_complete && chord.release_complete);
    assert_eq!((a.state(), b.state()), (6, 0));
    let mixed = WindowIdentity {
        process: a.target.process.clone(),
        hwnd: b.target.hwnd,
    };
    let failure = input::chord(&mixed, &[0x11, 0x7b], || Ok(())).unwrap_err();
    assert_eq!(failure.receipt.messages_queued, 0);
    let mut stale = b.target.clone();
    stale.process.created += 1;
    let failure = input::chord(&stale, &[0x11, 0x7b], || Ok(())).unwrap_err();
    assert_eq!(failure.receipt.messages_queued, 0);
    let mut checks = 0;
    let partial = input::chord(&b.target, &[0x11, 0x12, 0x7b], || {
        checks += 1;
        if checks == 5 {
            Err("fixture.guard_changed".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(partial.receipt.messages_queued, 2);
    assert!(!partial.receipt.submission_complete && partial.receipt.release_complete);
    assert_eq!((a.state(), b.state()), (6, 2));
    let retired = a.target.clone();
    let proof = format!(
        "scope=Windows owned targets only; not Lua/WGC/game ABI\na_pid={}\na_created={}\na_hwnd={}\nb_pid={}\nb_created={}\nb_hwnd={}\nmodule_sha256={}\na_messages=6\nb_messages=2\nforeign_and_stale_messages=0\n",
        a.target.process.pid,
        a.target.process.created,
        a.target.hwnd,
        b.target.process.pid,
        b.target.process.created,
        b.target.hwnd,
        a.hash
    );
    let proof_path =
        std::env::temp_dir().join(format!("lycheedev-actual-multiinstance-{}.txt", a.marker));
    fs::write(&proof_path, proof).unwrap();
    println!("actual multiinstance proof {}", proof_path.display());
    drop(a);
    assert!(retired.verify().is_err());
    assert_eq!(b.state(), 2);
    assert_eq!(b.bytes(), format!("owned-instance:{}", b.marker).as_bytes());
    let (address, wire) = b.stage_wire(b"fixture-codec-record");
    assert_ne!(address, 0);
    assert_eq!(wire, b"fixture-codec-record");
}
