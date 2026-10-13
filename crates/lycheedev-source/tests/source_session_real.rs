use lycheedev_core::{control::OperationControl, selection, storage::ExclusiveLock};
use lycheedev_source::{environment, git, index, luals::Runtime};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[test]
#[ignore = "requires fixed Source pin and exact LuaLS ZIP; authenticated complete Lua workspace"]
fn actual_complete_source_workspace_navigation_holds_and_releases_selected_lease() {
    let home =
        PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("fixed task home"));
    let pin = selection::load_pinned(
        &home,
        &std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin"),
    )
    .unwrap()
    .spec
    .source
    .unwrap();
    let zip = PathBuf::from(std::env::var_os("LYCHEEDEV_LUALS_ZIP").expect("exact ZIP"));
    let staging =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/rust-source-whole-session-real");
    fs::create_dir_all(&staging).unwrap();
    let runtime_root = staging.join(format!("runtime-{}", std::process::id()));
    let runtime = Runtime::unpack_pinned(&zip, &runtime_root).unwrap();
    let control = OperationControl::default();
    let indexed = index::build_with_control(&home, &pin, &control).unwrap();
    let env = environment::from_pin(&home, &pin, &control).unwrap();
    let symbol = indexed
        .symbols
        .iter()
        .find(|s| {
            s.kind == "declaration"
                && s.name.contains("CallbackRegistryMixin")
                && s.name.ends_with("RegisterCallback")
        })
        .expect("actual source declaration");
    let (original, _) =
        git::blob_with_control(&home, &pin, &symbol.path, git::MAX_SOURCE_BYTES, &control).unwrap();
    let line = std::str::from_utf8(&original)
        .unwrap()
        .lines()
        .nth(symbol.line - 1)
        .unwrap();
    let column = line[..line.find("RegisterCallback").unwrap()]
        .encode_utf16()
        .count()
        + 2;
    let before = std::time::Instant::now();
    let mut session = runtime
        .open_source_session_with_control(
            &home,
            &pin,
            &indexed,
            env.definitions.as_bytes(),
            &control,
        )
        .unwrap();
    let startup_ms = before.elapsed().as_secs_f64() * 1000.0;
    let pid = session.pid().unwrap();
    let process = lycheedev_windows::Process::open(pid).unwrap();
    let key = format!("worktree:{}:{}", pin.repository, pin.exact_commit);
    assert!(
        matches!(ExclusiveLock::resource(&home.join("locks"), &key),Err(e) if e.exit_code == 6),
        "selected workspace lease not held"
    );
    let analysis = session.analyze_with_control(&[json!({"kind":"definition","path":symbol.path,"position":{"line":symbol.line-1,"character":column}}),json!({"kind":"references","path":symbol.path,"position":{"line":symbol.line-1,"character":column}}),json!({"kind":"hover","path":symbol.path,"position":{"line":symbol.line-1,"character":column}})], &control).unwrap();
    assert!(
        analysis["coverage"]["luaFiles"].as_u64().unwrap() > 2048,
        "did not exercise whole workspace beyond SDK small-input bounds"
    );
    assert_eq!(analysis["coverage"]["excludedFiles"], 0);
    assert!(
        !analysis["results"][0]["locations"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{analysis}"
    );
    assert!(
        analysis["results"][1]["locations"]
            .as_array()
            .unwrap()
            .len()
            >= 2,
        "{analysis}"
    );
    assert!(
        !analysis["results"][2]["hover"].as_str().unwrap().is_empty(),
        "{analysis}"
    );
    assert_eq!(session.pid(), Some(pid));
    session.close();
    assert!(process.verify().is_err());
    assert!(
        ExclusiveLock::resource(&home.join("locks"), &key).is_ok(),
        "selected lease was not released after reaping"
    );
    fs::write(staging.join("whole-session-report.json"), serde_json::to_vec_pretty(&json!({"pin":pin,"symbol":symbol,"startupMs":startup_ms,"analysis":analysis,"workerPid":pid,"reaped":process.verify().is_err(),"leaseHeld":true,"leaseReleased":true,"scope":"SDK authenticated whole source Lua workspace; not broker/public CLI or complete precision/performance acceptance"})).unwrap()).unwrap();
    drop(session);
    drop(runtime);
    assert!(runtime_root.starts_with(&staging));
    fs::remove_dir_all(runtime_root).unwrap();
}
