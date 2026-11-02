use lycheedev_core::{VERSION, storage};
use lycheedev_delivery::{deploy, installation, release, slots};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-delivery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn candidate(root: &Path, skill: &[u8]) {
    fs::create_dir_all(root.join("payload/addon")).unwrap();
    fs::create_dir_all(root.join("payload/skill")).unwrap();
    fs::write(
        root.join("payload/addon/Lychee Dev.toc"),
        b"## Interface: 120100, 50504, 38002, 16001\n## Version: 3.1.2\n",
    )
    .unwrap();
    fs::write(root.join("payload/skill/SKILL.md"), skill).unwrap();
    let resources = [
        (
            "addon/Lychee Dev.toc",
            fs::read(root.join("payload/addon/Lychee Dev.toc")).unwrap(),
        ),
        ("skill/SKILL.md", skill.to_vec()),
    ]
    .map(|(path, bytes)| json!({"path":path,"bytes":bytes.len(),"sha256":storage::sha256(&bytes)}));
    storage::atomic_json(&root.join("release.json"),&json!({"schema":"lycheedev.release.v1","version":VERSION,"commit":"1".repeat(40),"binaries":{"windows-amd64":{"path":"native/windows-amd64/lycheedev.exe","bytes":1,"sha256":"2".repeat(64)}},"resources":resources})).unwrap();
}
#[test]
fn fresh_is_idempotent_and_unmanaged_or_extra_files_are_never_adopted() {
    let temp = Temp::new();
    let root = temp.0.join("release");
    candidate(&root, b"old skill");
    let parent = temp.0.join("skills");
    fs::create_dir(&parent).unwrap();
    let target = parent.join("lycheedev");
    let first = deploy::fresh_controlled(&root, &target, "skill", VERSION, &Default::default()).unwrap();
    let second = deploy::fresh_controlled(&root, &target, "skill", VERSION, &Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    fs::write(target.join("notes.txt"), b"owner text").unwrap();
    assert_eq!(
        installation::inspect_main_controlled(&target, "skill", &Default::default()).unwrap()["state"],
        "modified"
    );
    assert!(deploy::fresh_controlled(&root, &target, "skill", VERSION, &Default::default()).is_err());
    assert_eq!(fs::read(target.join("notes.txt")).unwrap(), b"owner text");
}
#[test]
fn upgrade_preserves_previous_and_resume_is_read_only_after_success() {
    let temp = Temp::new();
    let old = temp.0.join("old");
    let next = temp.0.join("next");
    candidate(&old, b"old");
    candidate(&next, b"new");
    let parent = temp.0.join("skills");
    fs::create_dir(&parent).unwrap();
    let target = parent.join("lycheedev");
    deploy::fresh_controlled(&old, &target, "skill", VERSION, &Default::default()).unwrap();
    let archive = temp.0.join("recovery");
    let result = deploy::upgrade_controlled(Some(&next), &target, &archive, "skill", VERSION, false, &Default::default()).unwrap();
    assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"new");
    assert_eq!(fs::read(archive.join("previous/SKILL.md")).unwrap(), b"old");
    assert_eq!(
        deploy::upgrade_controlled(None, &target, &archive, "skill", VERSION, true, &Default::default()).unwrap(),
        result
    );
}
#[test]
fn interrupted_upgrade_resumes_exact_journal_without_release() {
    let temp = Temp::new();
    let old = temp.0.join("old");
    let next = temp.0.join("next-release");
    candidate(&old, b"old");
    candidate(&next, b"new");
    let parent = temp.0.join("skills");
    fs::create_dir(&parent).unwrap();
    let target = parent.join("lycheedev");
    let previous = deploy::fresh_controlled(&old, &target, "skill", VERSION, &Default::default()).unwrap();
    let archive = temp.0.join("recovery");
    fs::create_dir(&archive).unwrap();
    let next = deploy::fresh_controlled(&next, &archive.join("next"), "skill", VERSION, &Default::default()).unwrap();
    let canonical_target = fs::canonicalize(&target).unwrap();
    storage::atomic_json(&archive.join("replacement.json"),&json!({"schema":"lycheedev.replacement.v1","target":canonical_target,"previous":previous,"next":next})).unwrap();
    fs::rename(&target, archive.join("previous")).unwrap();
    deploy::upgrade_controlled(None, &target, &archive, "skill", VERSION, true, &Default::default()).unwrap();
    assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"new");
    assert_eq!(fs::read(archive.join("previous/SKILL.md")).unwrap(), b"old");
}
#[test]
fn edited_preparation_or_in_discovery_archive_blocks_without_destroying_current() {
    let temp = Temp::new();
    let root = temp.0.join("release");
    candidate(&root, b"old");
    let parent = temp.0.join("skills");
    fs::create_dir(&parent).unwrap();
    let target = parent.join("lycheedev");
    deploy::fresh_controlled(&root, &target, "skill", VERSION, &Default::default()).unwrap();
    assert!(
        deploy::upgrade_controlled(
            Some(&root),
            &target,
            &parent.join("recovery"),
            "skill",
            VERSION,
            false,
            &Default::default(),
        )
        .is_err()
    );
    assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"old");
    let archive = temp.0.join("recovery");
    let previous: Value = installation::inspect_main_controlled(&target, "skill", &Default::default()).unwrap()["receipt"].clone();
    fs::create_dir(&archive).unwrap();
    let next = deploy::fresh_controlled(&root, &archive.join("next"), "skill", VERSION, &Default::default()).unwrap();
    storage::atomic_json(&archive.join("replacement.json"),&json!({"schema":"lycheedev.replacement.v1","target":fs::canonicalize(&target).unwrap(),"previous":previous,"next":next})).unwrap();
    fs::write(archive.join("next/SKILL.md"), b"tampered").unwrap();
    assert_eq!(
        deploy::upgrade_controlled(None, &target, &archive, "skill", VERSION, true, &Default::default())
            .err()
            .unwrap()
            .code,
        "delivery.installation_conflict"
    );
    assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"old");
}
#[test]
fn release_rejects_unlisted_file_before_installation_write() {
    let temp = Temp::new();
    let root = temp.0.join("release");
    candidate(&root, b"skill");
    fs::write(root.join("payload/foreign.py"), b"unknown").unwrap();
    assert_eq!(
        release::inspect_with_control(&root, VERSION, &Default::default()).err().unwrap().code,
        "delivery.invalid_payload"
    );
}
#[test]
fn interrupted_500_slot_generation_finishes_only_owned_expected_bytes() {
    let temp = Temp::new();
    let files: Vec<_> = (1..=slots::COUNT)
        .map(|_| json!({"payloadHash":"", "consumed":false}))
        .collect();
    storage::atomic_json(
        &temp.0.join(".lycheedev-slots.json"),
        &json!({"schema":slots::SCHEMA,"version":VERSION,"state":"installing","files":files}),
    )
    .unwrap();
    let first = slots::directory(&temp.0, 1);
    fs::create_dir(&first).unwrap();
    let files = slots::files(1, VERSION);
    fs::write(first.join(&files[0].0), &files[0].1).unwrap();
    slots::install_controlled(&temp.0, VERSION, &Default::default()).unwrap();
    let pool = slots::inspect_controlled(&temp.0, VERSION, &Default::default()).unwrap();
    assert_eq!(pool["files"].as_array().unwrap().len(), 500);
    assert!(
        slots::directory(&temp.0, 500)
            .join("Lychee Dev Slot 500.toc")
            .exists()
    );
    fs::write(first.join("Loader.lua"), b"foreign").unwrap();
    assert!(slots::install_controlled(&temp.0, VERSION, &Default::default()).is_err());
    fs::write(first.join("Loader.lua"), &slots::files(1, VERSION)[1].1).unwrap();
    let root = temp.0.join("candidate");
    candidate(&root, b"skill");
    let target = temp.0.join("Lychee Dev");
    deploy::fresh_controlled(&root, &target, "addon", VERSION, &Default::default()).unwrap();
    installation::verify_clean_controlled(&target, VERSION, slots::TRANSPORT, 500, &Default::default()).unwrap();
    let recovery = Temp::new();
    let slot_archive = recovery.0.join("addon-archive.slots");
    fs::create_dir(&slot_archive).unwrap();
    let parent = fs::canonicalize(&temp.0).unwrap();
    let target = fs::canonicalize(&target).unwrap();
    let archive = fs::canonicalize(&recovery.0).unwrap().join("addon-archive");
    let slot_archive = fs::canonicalize(&slot_archive).unwrap();
    let receipt = installation::inspect_main_controlled(&target, "addon", &Default::default()).unwrap()["receipt"].clone();
    let journal = recovery.0.join("addon-archive.removal.json");
    storage::atomic_json(&journal,&json!({"schema":"lycheedev.addon-removal.v1","parent":parent,"archive":archive,"pool":pool,"receipt":receipt})).unwrap();
    fs::rename(
        slots::directory(&temp.0, 1),
        slots::directory(&slot_archive, 1),
    )
    .unwrap();
    fs::write(
        temp.0.join("SavedVariables-owned-by-player.lua"),
        b"player profile",
    )
    .unwrap();
    let removed = lycheedev_delivery::removal::addon_controlled(&target, &archive, &Default::default()).unwrap();
    assert_eq!(removed["state"], "archived");
    assert!(archive.join(installation::MARKER).exists());
    assert!(slots::directory(&slot_archive, 500).exists());
    assert!(!temp.0.join(".lycheedev-slots.json").exists());
    assert_eq!(
        fs::read(temp.0.join("SavedVariables-owned-by-player.lua")).unwrap(),
        b"player profile"
    );
    assert_eq!(
        lycheedev_delivery::removal::addon_controlled(&target, &archive, &Default::default()).unwrap(),
        removed
    );
}

#[test]
fn managed_update_plans_readonly_then_updates_and_cleans_exact_recovery() {
    let temp = Temp::new();
    let old = temp.0.join("old");
    let next = temp.0.join("next");
    candidate(&old, b"old skill");
    candidate(&next, b"new skill");
    let parent = temp.0.join("skills");
    fs::create_dir(&parent).unwrap();
    let target = parent.join("lycheedev");
    deploy::fresh_controlled(&old, &target, "skill", VERSION, &Default::default()).unwrap();
    let state = temp.0.join("delivery-state");
    let targets = [lycheedev_delivery::update::UpdateTarget {
        component: "skill".into(),
        path: target.clone(),
    }];
    let planned =
        lycheedev_delivery::update::managed_with_control(&next, VERSION, &state, &targets, true, &Default::default()).unwrap();
    assert_eq!(planned.data["plan"], true);
    assert_eq!(planned.data["complete"], false);
    assert!(!state.exists());
    assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"old skill");
    let applied =
        lycheedev_delivery::update::managed_with_control(&next, VERSION, &state, &targets, false, &Default::default()).unwrap();
    assert_eq!(applied.data["complete"], true);
    assert_eq!(applied.data["targets"][0]["state"], "current");
    assert_eq!(
        applied.data["targets"][0]["activation"],
        "new_agent_context_required"
    );
    assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"new skill");
    assert!(state.join("targets.json").exists());
    let recovery = PathBuf::from(planned.data["targets"][0]["recovery"].as_str().unwrap());
    assert!(!recovery.exists());
    assert!(lycheedev_delivery::update::managed_with_control(&next, VERSION, &state, &targets, false, &Default::default()).is_ok());
}
#[test]
fn managed_update_checks_all_targets_before_changing_first() {
    let temp = Temp::new();
    let old = temp.0.join("old");
    let next = temp.0.join("next");
    candidate(&old, b"old skill");
    candidate(&next, b"new skill");
    let mut targets = vec![];
    for name in ["one", "two"] {
        let parent = temp.0.join(name);
        fs::create_dir(&parent).unwrap();
        let path = parent.join("lycheedev");
        deploy::fresh_controlled(&old, &path, "skill", VERSION, &Default::default()).unwrap();
        targets.push(lycheedev_delivery::update::UpdateTarget {
            component: "skill".into(),
            path,
        });
    }
    fs::write(targets[1].path.join("SKILL.md"), b"owner edit").unwrap();
    assert_eq!(
        lycheedev_delivery::update::managed_with_control(&next, VERSION, &temp.0.join("state"), &targets, false, &Default::default())
            .err()
            .unwrap()
            .code,
        "delivery.installation_conflict"
    );
    assert_eq!(
        fs::read(targets[0].path.join("SKILL.md")).unwrap(),
        b"old skill"
    );
    assert_eq!(
        fs::read(targets[1].path.join("SKILL.md")).unwrap(),
        b"owner edit"
    );
}
