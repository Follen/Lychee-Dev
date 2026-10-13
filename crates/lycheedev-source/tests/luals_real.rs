use lycheedev_source::luals::Runtime;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[test]
#[ignore = "requires exact pinned LuaLS ZIP; explicit real process regression"]
fn actual_pinned_check_and_alias_navigation() {
    let zip =
        PathBuf::from(std::env::var_os("LYCHEEDEV_LUALS_ZIP").expect("set pinned runtime ZIP"));
    let staging = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/rust-source-luals-real");
    fs::create_dir_all(&staging).unwrap();
    let runtime_root = staging.join(format!("runtime-{}", std::process::id()));
    let runtime = Runtime::unpack_pinned(&zip, &runtime_root).unwrap();
    let files=BTreeMap::from([("One.lua".to_string(),b"local function compute(n) return n+1 end\nlocal alias=compute\nlocal a=alias(5)\nreturn a\n".to_vec())]);
    let checked = runtime.check(&staging, &files, b"").unwrap();
    assert_eq!(checked["state"], "checked");
    assert_eq!(checked["passed"], true, "{checked}");
    let bad = BTreeMap::from([("Bad.lua".to_string(), b"missing(5)\n".to_vec())]);
    let checked_bad = runtime.check(&staging, &bad, b"").unwrap();
    assert_eq!(checked_bad["passed"], false);
    assert!(
        checked_bad["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "undefined-global")
    );
    let queries = [
        json!({"kind":"definition","path":"One.lua","position":{"line":2,"character":9}}),
        json!({"kind":"references","path":"One.lua","position":{"line":0,"character":17}}),
        json!({"kind":"hover","path":"One.lua","position":{"line":2,"character":9}}),
    ];
    let analysis = runtime.analyze(&staging, &files, b"", &queries).unwrap();
    assert_eq!(analysis["state"], "complete", "{analysis}");
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
    fs::write(
        staging.join("real-luals-report.json"),
        serde_json::to_vec_pretty(
            &json!({"check":checked,"badCheck":checked_bad,"navigation":analysis}),
        )
        .unwrap(),
    )
    .unwrap();
    drop(runtime);
    assert!(runtime_root.starts_with(&staging));
    fs::remove_dir_all(runtime_root).unwrap();
}
