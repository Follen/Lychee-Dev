use lycheedev_core::selection;
use lycheedev_source::index;
use serde_json::json;
use std::{fs, path::PathBuf};
#[test]
#[ignore = "explicit authenticated source home/pin; full XML/TOC subset reader"]
fn authenticated_warm_index_loads_match_source_hash_and_reject_index_tamper() {
    let home = PathBuf::from(std::env::var_os("LYCHEEDEV_ENVIRONMENT_HOME").expect("source home"));
    let id = std::env::var("LYCHEEDEV_ENVIRONMENT_PIN").expect("fixed pin");
    let selected = selection::load_pinned(&home, &id).unwrap();
    let pin = selected.source.as_ref().unwrap();
    let control = Default::default();
    let mut map = index::build_with_control(&home, pin, &control).unwrap();
    let loads = index::load_facts_with_control(&home, pin, &map, &control).unwrap();
    assert!(!loads.is_empty());
    for load in &loads {
        let document = map.documents.iter().find(|d| d.path == load.path).unwrap();
        assert_eq!(load.source_blob["sha256"], document.sha256);
        assert_eq!(load.source_blob["bytes"], document.bytes);
        assert!(load.line > 0);
    }
    let first = &loads[0].path;
    map.documents
        .iter_mut()
        .find(|d| d.path == *first)
        .unwrap()
        .sha256 = "0".repeat(64);
    assert_eq!(
        index::load_facts_with_control(&home, pin, &map, &control)
            .unwrap_err()
            .code,
        "codebase.source_integrity"
    );
    let report = home.parent().unwrap().join("load-facts-report.json");
    fs::write(report,serde_json::to_vec_pretty(&json!({"pin":selected,"loads":loads.len(),"loadSourceHashChecks":loads.len(),"tamperedIndexRejected":true,"scope":"authenticated raw XML/TOC parser load facts; runtime loadedness not proved, context CLI parity/performance not_run"})).unwrap()).unwrap();
}
