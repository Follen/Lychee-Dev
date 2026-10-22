//! Automated Windows process regression; compile-only authenticated Lua 5.1,
//! not game acceptance. The explicit fixture stage must be supplied by CI.
use lycheedev_core::control::OperationControl;
use lycheedev_source::luac::Runtime;
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};
#[test]
#[ignore = "requires authenticated bundled luac dev stage; run explicitly in controlled Windows CI"]
fn actual_ordered_luac_preserves_parent_and_reaps_compile_children() {
    let stage = PathBuf::from(std::env::var_os("LYCHEEDEV_LUAC_STAGE").expect("fixed luac stage"));
    let identity: Value =
        serde_json::from_str(include_str!("../../../release/tools/luac.json")).unwrap();
    let control = OperationControl::default();
    let runtime = Runtime::verified_with_ordered_control(
        &stage,
        serde_json::from_value(identity["runtime"].clone()).unwrap(),
        &control,
    )
    .unwrap();
    let root = std::env::temp_dir().join(format!(
        "lycheedev-ordered-real-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let good = runtime
        .check_with_control(
            &root,
            &BTreeMap::from([("good.lua".into(), b"local goto = 1; return goto\n".to_vec())]),
            &control,
        )
        .unwrap();
    assert_eq!(good["passed"], true);
    let bad = runtime
        .check_with_control(
            &root,
            &BTreeMap::from([("bad.lua".into(), b"local =\n".to_vec())]),
            &control,
        )
        .unwrap();
    assert_eq!(bad["passed"], false);
    assert_eq!(control.stop_cause().unwrap(), None);
    assert!(control.ordered_child(control.deadline()).is_ok());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    assert!(!root.join("luac.out").exists());
    fs::remove_dir(&root).unwrap();
}
