use lycheedev_core::control::OperationControl;
use lycheedev_source::luals::Runtime;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires exact pinned LuaLS ZIP; real retained worker and retirement proof"]
fn actual_retained_worker_reuses_pid_and_retires_on_cancel_or_new_input() {
    let zip = PathBuf::from(std::env::var_os("LYCHEEDEV_LUALS_ZIP").expect("exact ZIP"));
    let staging = std::env::var_os("LYCHEEDEV_SOURCE_TEST_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/rust-source-session-real")
        });
    fs::create_dir_all(&staging).unwrap();
    let runtime_root = staging.join(format!("runtime-{}", std::process::id()));
    let runtime = Runtime::unpack_pinned(&zip, &runtime_root).unwrap();
    let files = BTreeMap::from([("界 面/One.lua".to_string(), b"local function compute(n) return n+1 end\nlocal alias=compute\nlocal a=alias(5)\nreturn a\n".to_vec())]);
    let first_control = OperationControl::default();
    let mut session = runtime
        .open_session_with_control(
            &staging,
            &files,
            b"---@param n number\n---@return number\nfunction FixtureAPI(n) end\n",
            &first_control,
        )
        .unwrap();
    let pid = session.pid().unwrap();
    let process = lycheedev_windows::Process::open(pid).unwrap();
    let definition = session.analyze_with_control(&[json!({"kind":"definition","path":"界 面/One.lua","position":{"line":2,"character":9}})], &first_control).unwrap();
    assert!(
        !definition["results"][0]["locations"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{definition}"
    );
    let second_control = OperationControl::default();
    let references = session.analyze_with_control(&[json!({"kind":"references","path":"界 面/One.lua","position":{"line":0,"character":17}}), json!({"kind":"hover","path":"@environment/api.d.lua","position":{"line":2,"character":12}})], &second_control).unwrap();
    assert_eq!(session.pid(), Some(pid));
    assert_eq!(session.batches(), 2);
    assert!(
        references["results"][0]["locations"]
            .as_array()
            .unwrap()
            .len()
            >= 2,
        "{references}"
    );
    assert!(
        !references["results"][1]["hover"]
            .as_str()
            .unwrap()
            .is_empty(),
        "{references}"
    );
    let cancelled = OperationControl::new(
        Instant::now() + Duration::from_secs(10),
        Arc::new(AtomicBool::new(true)),
    );
    let error = session.analyze_with_control(&[], &cancelled).unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.exit_code, 7);
    assert_eq!(session.pid(), None);
    assert!(process.verify().is_err(), "retired worker still alive");
    assert!(!session.usable());
    let mut changed = runtime
        .open_session_with_control(&staging, &files, b"", &OperationControl::default())
        .unwrap();
    let changed_pid = changed.pid().unwrap();
    let changed_process = lycheedev_windows::Process::open(changed_pid).unwrap();
    let active_root = fs::read_dir(&staging)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!("luals-{}-", std::process::id()))
                && p.join("workspace").exists()
        })
        .unwrap();
    assert!(
        fs::write(
            active_root.join("workspace/界 面/One.lua"),
            b"return injected"
        )
        .is_err(),
        "known input allowed writing"
    );
    fs::write(active_root.join("workspace/New.lua"), b"return injected").unwrap();
    let changed_error = changed
        .analyze_with_control(&[], &OperationControl::default())
        .unwrap_err();
    assert_eq!(changed_error.code, "luals.unavailable");
    assert_eq!(changed.pid(), None);
    assert!(changed_process.verify().is_err());
    let mut transient = runtime
        .open_session_with_control(&staging, &files, b"", &OperationControl::default())
        .unwrap();
    let transient_pid = transient.pid().unwrap();
    let transient_process = lycheedev_windows::Process::open(transient_pid).unwrap();
    let transient_root = fs::read_dir(&staging)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!("luals-{}-", std::process::id()))
                && p.join("workspace").exists()
        })
        .unwrap();
    let temporary = transient_root.join("workspace/Transient.lua");
    fs::write(&temporary, b"return injected").unwrap();
    fs::remove_file(&temporary).unwrap();
    assert!(!temporary.exists());
    let transient_error = transient
        .analyze_with_control(&[], &OperationControl::default())
        .unwrap_err();
    assert_eq!(transient_error.code, "luals.unavailable");
    assert_eq!(transient.pid(), None);
    assert!(transient_process.verify().is_err());
    fs::write(staging.join("session-report.json"), serde_json::to_vec_pretty(&json!({"definition":definition,"references":references,"workerPid":pid,"batches":2,"sameWorker":true,"cancel":{"code":error.code,"exit":error.exit_code,"reaped":process.verify().is_err()},"changedInput":{"code":changed_error.code,"reaped":changed_process.verify().is_err()},"transientInput":{"code":transient_error.code,"reaped":transient_process.verify().is_err(),"fileAlreadyRemoved":true},"scope":"SDK persistent worker and event watcher; no broker/public CLI proof"})).unwrap()).unwrap();
    drop(session);
    drop(changed);
    drop(transient);
    drop(runtime);
    assert!(runtime_root.starts_with(&staging));
    fs::remove_dir_all(runtime_root).unwrap();
}
