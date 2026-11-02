use lycheedev_core::{control::OperationControl, coordination::InstallationMaintenanceLease, storage};
use lycheedev_delivery::{
    addon_transaction::{self, Request},
    installation, slots,
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
fn candidate(root: &Path, version: &str, body: &[u8]) {
    let files=[("addon/Lychee Dev.toc",format!("## Interface: 120100, 50504, 38002, 16001\n## Version: {version}\nBridge/SlotRuntime.lua\n").into_bytes()),("addon/Bridge/SlotRuntime.lua",body.to_vec()),("skill/SKILL.md",b"fixture skill\n".to_vec())];
    for (name, bytes) in &files {
        let path = root.join("payload").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let resources:Vec<_>=files.iter().map(|(path,bytes)|json!({"path":path,"bytes":bytes.len(),"sha256":storage::sha256(bytes)})).collect();
    storage::atomic_json(&root.join("release.json"),&json!({"schema":"lycheedev.release.v1","version":version,"commit":"1".repeat(40),"binaries":{"windows-amd64":{"path":"native/windows-amd64/lycheedev.exe","bytes":1,"sha256":"2".repeat(64)}},"resources":resources})).unwrap();
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[cfg(windows)]
#[test]
fn combined_upgrade_freezes_release_blocks_reserved_slots_and_recovers_main_before_slot_cutpoint() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = Temp(std::env::temp_dir().join(format!(
            "lycheedev-combined-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    fs::create_dir_all(temp.0.join("client/Interface/AddOns")).unwrap();
    let parent = temp.0.join("client/Interface/AddOns");
    let target = parent.join("Lychee Dev");
    let old = temp.0.join("old");
    let next = temp.0.join("next");
    let archive = temp.0.join("archive");
    candidate(&old, "3.0.2", b"-- fixture old runtime\n");
    candidate(&next, "3.1.2", b"-- fixture new runtime\n");
    let control = OperationControl::default();
    let guard = InstallationMaintenanceLease::acquire(&target).unwrap();
    let original = addon_transaction::apply(
        &target,
        Request {
            release: Some(&old),
            archive: None,
            version: "3.0.2",
            resume: false,
            control: &control,
        },
        &guard,
    )
    .unwrap();
    assert_eq!(original["version"], "3.0.2");
    assert_eq!(
        slots::inspect_controlled(&parent, "3.0.2", &Default::default()).unwrap()["files"]
            .as_array()
            .unwrap()
            .len(),
        500
    );
    let mut pool = slots::inspect_controlled(&parent, "3.0.2", &Default::default()).unwrap();
    pool["files"][0]["nonce"] = json!("unresolved owner");
    storage::atomic_json(&parent.join(".lycheedev-slots.json"), &pool).unwrap();
    assert_eq!(
        addon_transaction::apply(
            &target,
            Request {
                release: Some(&next),
                archive: Some(&archive),
                version: "3.1.2",
                resume: false,
                control: &control
            },
            &guard
        )
        .unwrap_err()
        .exit_code,
        3
    );
    assert_eq!(
        fs::read(target.join("Bridge/SlotRuntime.lua")).unwrap(),
        b"-- fixture old runtime\n"
    );
    assert!(!archive.exists());
    pool["files"][0].as_object_mut().unwrap().remove("nonce");
    storage::atomic_json(&parent.join(".lycheedev-slots.json"), &pool).unwrap();
    // Go only replaces changed bytes. The loader is version-independent;
    // hold the versioned TOC to create the same main-before-slots cutpoint.
    let loader = slots::directory(&parent, 1).join("Lychee Dev Slot 01.toc");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&loader)
        .unwrap();
    let error = addon_transaction::apply(
        &target,
        Request {
            release: Some(&next),
            archive: Some(&archive),
            version: "3.1.2",
            resume: false,
            control: &control,
        },
        &guard,
    )
    .unwrap_err();
    // Preserve the first cause in the test log. Historical failures did not
    // record this raw code; only this new execution can establish it.
    eprintln!(
        "combined addon first slot failure: {}",
        serde_json::to_string(&error).unwrap()
    );
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.code, "external.io");
    assert_eq!(
        error.details["cause"]["schema"],
        "lycheedev.slot-sharing-preflight.v1"
    );
    assert_eq!(error.details["cause"]["stage"], "open");
    assert_eq!(error.details["cause"]["rawOsError"], 32);
    assert_eq!(error.details["cause"]["renameAttempted"], false);
    assert_eq!(error.details["cause"]["published"], false);
    let mut members: Vec<_> = fs::read_dir(slots::directory(&parent, 1))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    members.sort();
    assert_eq!(
        members,
        ["Loader.lua", "Lychee Dev Slot 01.toc", "Payload.lua"]
    );
    assert_eq!(error.details["result"]["phase"], "main");
    assert_eq!(error.details["result"]["complete"], false);
    assert_eq!(
        fs::read(target.join("Bridge/SlotRuntime.lua")).unwrap(),
        b"-- fixture new runtime\n"
    );
    assert_eq!(
        fs::read(archive.join("previous/Bridge/SlotRuntime.lua")).unwrap(),
        b"-- fixture old runtime\n"
    );
    let journal = PathBuf::from(error.details["result"]["journal"].as_str().unwrap());
    assert!(journal.exists());
    assert!(installation::verify_clean_controlled(&target, "3.1.2", slots::TRANSPORT, 500, &Default::default()).is_err());
    let changed = temp.0.join("changed");
    candidate(&changed, "3.1.2", b"-- alternate candidate\n");
    assert_eq!(
        addon_transaction::apply(
            &target,
            Request {
                release: Some(&changed),
                archive: Some(&archive),
                version: "3.1.2",
                resume: false,
                control: &control
            },
            &guard
        )
        .unwrap_err()
        .exit_code,
        6
    );
    drop(held);
    fs::write(target.join("Bridge/SlotRuntime.lua"), b"owner edit").unwrap();
    assert_eq!(
        addon_transaction::apply(
            &target,
            Request {
                release: None,
                archive: Some(&archive),
                version: "3.1.2",
                resume: true,
                control: &control
            },
            &guard
        )
        .unwrap_err()
        .exit_code,
        4
    );
    fs::write(
        target.join("Bridge/SlotRuntime.lua"),
        b"-- fixture new runtime\n",
    )
    .unwrap();
    let result = addon_transaction::apply(
        &target,
        Request {
            release: None,
            archive: Some(&archive),
            version: "3.1.2",
            resume: true,
            control: &control,
        },
        &guard,
    )
    .unwrap();
    assert_eq!(result["receipt"]["version"], "3.1.2");
    assert_eq!(
        slots::inspect_controlled(&parent, "3.1.2", &Default::default()).unwrap()["schema"],
        slots::SCHEMA
    );
    let after: serde_json::Value = storage::read_json(&journal).unwrap();
    assert_eq!(after["phase"], "complete");
    assert_eq!(
        addon_transaction::apply(
            &target,
            Request {
                release: None,
                archive: Some(&archive),
                version: "3.1.2",
                resume: true,
                control: &control
            },
            &guard
        )
        .unwrap(),
        result
    );
    assert!(archive.join("previous/Bridge/SlotRuntime.lua").exists());
    assert!(!parent.join(".lycheedev-slots.migration-v4.json").exists());
    let third = temp.0.join("third");
    let archive2 = temp.0.join("archive2");
    candidate(&third, "3.1.3", b"-- fixture third runtime\n");
    let third_result = addon_transaction::apply(
        &target,
        Request {
            release: Some(&third),
            archive: Some(&archive2),
            version: "3.1.3",
            resume: false,
            control: &control,
        },
        &guard,
    )
    .unwrap();
    assert_eq!(third_result["receipt"]["version"], "3.1.3");
    let final_pool = slots::inspect_controlled(&parent, "3.1.3", &Default::default()).unwrap();
    assert_eq!(final_pool["schema"], slots::SCHEMA);
    assert!(final_pool.get("pendingVersion").is_none());
    assert!(final_pool.get("sourceSchema").is_none());
    assert!(final_pool.get("generation").is_none());
    assert!(!parent.join(".lycheedev-slots.migration-v4.json").exists());
    assert_eq!(
        fs::read(archive2.join("previous/Bridge/SlotRuntime.lua")).unwrap(),
        b"-- fixture new runtime\n"
    );
}
