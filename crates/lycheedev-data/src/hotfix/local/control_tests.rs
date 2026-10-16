use super::*;
use lycheedev_core::{
    control::{OperationControl, TrackedCancellation},
    selection::{DataPin, SelectionSpec},
    workspace,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Fixture {
    root: PathBuf,
    inv: Invocation,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-hotfix-control-r38-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        let pin = selection::pin_selection(
            &home,
            SelectionSpec {
                data: Some(DataPin {
                    product: "retail".into(),
                    region: "us".into(),
                    language: "enUS".into(),
                    full_build: "12.1.0.69875".into(),
                    build_config: "b".repeat(32),
                    cdn_config: "c".repeat(32),
                    definition_commit: "a".repeat(40),
                }),
                ..Default::default()
            },
        )
        .unwrap();
        let mut raw = b"XFTH".to_vec();
        raw.extend(8u32.to_le_bytes());
        raw.extend(69875u32.to_le_bytes());
        raw.resize(44, 0);
        let control = OperationControl::tracked(
            Instant::now() + Duration::from_secs(30),
            TrackedCancellation::new(),
        );
        let source=evidence::commit_capture_bounded(&home,evidence::CaptureInput {bytes:&raw,maximum_bytes:44,media_type:"application/octet-stream",provenance:json!({"kind":"hotfix-cache","locator":format!("dbcache:sha256:{}",storage::sha256(&raw)),"snapshot":pin.id,"dataBuild":"12.1.0.69875"}),complete:true,truncated:false},&control).unwrap();
        let inv = Invocation {
            command: "data hotfix".into(),
            positional: None,
            cwd: root.clone(),
            home,
            control,
            flags: BTreeMap::from([
                ("--source".into(), "dbcache".into()),
                ("--snapshot".into(), pin.id),
                ("--from".into(), source["id"].as_str().unwrap().into()),
                ("--offline".into(), "true".into()),
            ]),
        };
        Self { root, inv }
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.root).unwrap()
    }
}
#[test]
fn archived_default_hotfix_keeps_original_scope_through_pin_blob_derived_pin_and_capture() {
    let f = Fixture::new();
    let result = dispatch(&f.inv).unwrap();
    assert!(result.complete);
    assert_eq!(result.provenance.len(), 2);
    assert_eq!(result.provenance[0]["id"], f.inv.flag("--from").unwrap());
    assert_eq!(f.inv.control.stop_cause().unwrap(), None);
    f.inv
        .control
        .ordered_child(f.inv.control.deadline())
        .unwrap()
        .check()
        .unwrap();
    f.cleanup();
}
#[test]
fn local_source_file_success_and_failed_read_release_their_original_handles() {
    let f = Fixture::new();
    let path = f.root.join("input");
    fs::write(&path, b"body").unwrap();
    let budget = Budget::controlled(
        Cost::defaults(),
        f.inv.control.deadline(),
        f.inv.control.clone(),
    );
    assert_eq!(read_file(&path, 4, &budget).unwrap(), b"body");
    fs::rename(&path, f.root.join("moved")).unwrap();
    fs::rename(f.root.join("moved"), &path).unwrap();
    let budget = Budget::controlled(
        Cost {
            metadata_bytes: 0,
            ..Cost::defaults()
        },
        f.inv.control.deadline(),
        f.inv.control.clone(),
    );
    assert_eq!(
        read_file(&path, 4, &budget).unwrap_err().code,
        "records.query_resource_budget"
    );
    fs::rename(&path, f.root.join("failed-closed")).unwrap();
    f.cleanup();
}
