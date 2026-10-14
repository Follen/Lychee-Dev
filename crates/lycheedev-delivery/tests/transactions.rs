use lycheedev_core::{
    VERSION,
    coordination::{Claim, InstallationGuard},
    storage,
};
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
    let first = deploy::fresh(&root, &target, "skill", VERSION).unwrap();
    let second = deploy::fresh(&root, &target, "skill", VERSION).unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    fs::write(target.join("notes.txt"), b"owner text").unwrap();
    assert_eq!(
        installation::inspect(&target, "skill").unwrap()["state"],
        "modified"
    );
    assert!(deploy::fresh(&root, &target, "skill", VERSION).is_err());
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
    deploy::fresh(&old, &target, "skill", VERSION).unwrap();
    let archive = temp.0.join("recovery");
    let result = deploy::upgrade(Some(&next), &target, &archive, "skill", VERSION, false).unwrap();
    assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"new");
    assert_eq!(fs::read(archive.join("previous/SKILL.md")).unwrap(), b"old");
    assert_eq!(
        deploy::upgrade(None, &target, &archive, "skill", VERSION, true).unwrap(),
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
    let previous = deploy::fresh(&old, &target, "skill", VERSION).unwrap();
    let archive = temp.0.join("recovery");
    fs::create_dir(&archive).unwrap();
    let next = deploy::fresh(&next, &archive.join("next"), "skill", VERSION).unwrap();
    let canonical_target = fs::canonicalize(&target).unwrap();
    storage::atomic_json(&archive.join("replacement.json"),&json!({"schema":"lycheedev.replacement.v1","target":canonical_target,"previous":previous,"next":next})).unwrap();
    fs::rename(&target, archive.join("previous")).unwrap();
    deploy::upgrade(None, &target, &archive, "skill", VERSION, true).unwrap();
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
    deploy::fresh(&root, &target, "skill", VERSION).unwrap();
    assert!(
        deploy::upgrade(
            Some(&root),
            &target,
            &parent.join("recovery"),
            "skill",
            VERSION,
            false
        )
        .is_err()
    );
    assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"old");
    let archive = temp.0.join("recovery");
    let previous: Value = installation::inspect(&target, "skill").unwrap()["receipt"].clone();
    fs::create_dir(&archive).unwrap();
    let next = deploy::fresh(&root, &archive.join("next"), "skill", VERSION).unwrap();
    storage::atomic_json(&archive.join("replacement.json"),&json!({"schema":"lycheedev.replacement.v1","target":fs::canonicalize(&target).unwrap(),"previous":previous,"next":next})).unwrap();
    fs::write(archive.join("next/SKILL.md"), b"tampered").unwrap();
    assert_eq!(
        deploy::upgrade(None, &target, &archive, "skill", VERSION, true)
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
        release::inspect(&root, VERSION).err().unwrap().code,
        "delivery.invalid_payload"
    );
}
#[test]
fn pending_claim_survives_process_id_reuse_and_blocks_maintenance() {
    let temp = Temp::new();
    let target = temp.0.join("Lychee Dev");
    let mut guard = InstallationGuard::acquire(&target).unwrap();
    guard
        .claim(Claim {
            owner: "CON-owner".into(),
            kind: "native-live".into(),
            resource: "installation".into(),
            process_id: 1,
            created_at: "2000-01-01T00:00:00Z".into(),
            lifecycle_ref: "project/.lycheedev/live/CON-owner.json".into(),
            pending: true,
        })
        .unwrap();
    assert_eq!(guard.require_idle().err().unwrap().exit_code, 6);
    assert!(
        guard
            .retire("CON-other", "project/.lycheedev/live/CON-owner.json")
            .is_err()
    );
    guard
        .retire("CON-owner", "project/.lycheedev/live/CON-owner.json")
        .unwrap();
    assert!(guard.require_idle().is_ok());
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
    slots::install(&temp.0, VERSION).unwrap();
    let pool = slots::inspect(&temp.0, VERSION).unwrap();
    assert_eq!(pool["files"].as_array().unwrap().len(), 500);
    assert!(
        slots::directory(&temp.0, 500)
            .join("Lychee Dev Slot 500.toc")
            .exists()
    );
    fs::write(first.join("Loader.lua"), b"foreign").unwrap();
    assert!(slots::install(&temp.0, VERSION).is_err());
    fs::write(first.join("Loader.lua"), &slots::files(1, VERSION)[1].1).unwrap();
    let root = temp.0.join("candidate");
    candidate(&root, b"skill");
    let target = temp.0.join("Lychee Dev");
    deploy::fresh(&root, &target, "addon", VERSION).unwrap();
    let identity = installation::verify_clean(&target, VERSION, slots::TRANSPORT, 500).unwrap();
    let before = fs::read(temp.0.join(slots::MARKER)).unwrap();
    let payload_before = fs::read(first.join("Payload.lua")).unwrap();
    let error = slots::publish(&identity, 1, None, b"arbitrary historical bytes").unwrap_err();
    assert_eq!(error.code, "live.mailbox_v1_migration_incomplete");
    assert_eq!(fs::read(temp.0.join(slots::MARKER)).unwrap(), before);
    assert_eq!(fs::read(first.join("Payload.lua")).unwrap(), payload_before);
    let recovery = Temp::new();
    let slot_archive = recovery.0.join("addon-archive.slots");
    fs::create_dir(&slot_archive).unwrap();
    let parent = fs::canonicalize(&temp.0).unwrap();
    let target = fs::canonicalize(&target).unwrap();
    let archive = fs::canonicalize(&recovery.0).unwrap().join("addon-archive");
    let slot_archive = fs::canonicalize(&slot_archive).unwrap();
    let receipt = installation::inspect(&target, "addon").unwrap()["receipt"].clone();
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
    let removed = lycheedev_delivery::removal::addon(&target, &archive).unwrap();
    assert_eq!(removed["state"], "archived");
    assert!(archive.join(installation::MARKER).exists());
    assert!(slots::directory(&slot_archive, 500).exists());
    assert!(!temp.0.join(".lycheedev-slots.json").exists());
    assert_eq!(
        fs::read(temp.0.join("SavedVariables-owned-by-player.lua")).unwrap(),
        b"player profile"
    );
    assert_eq!(
        lycheedev_delivery::removal::addon(&target, &archive).unwrap(),
        removed
    );
}
#[test]
fn managed_200_slot_pool_migrates_to_500_and_does_not_erase_unknown_reservations() {
    let temp = Temp::new();
    let payload = b"LycheeDevSlotEnvelope = nil\n";
    let mut entries = vec![];
    for slot in 1..=200u16 {
        let target = slots::directory(&temp.0, slot);
        fs::create_dir(&target).unwrap();
        let toc = format!(
            "## Interface: 120100, 50504, 38002, 16001\n## Title: Lychee Dev input slot {slot:02}\n## Version: {VERSION}\n## LoadOnDemand: 1\n## X-Lychee-Slot: {slot}\n## X-Lychee-Transport: memory-slot-v3\nPayload.lua\nLoader.lua\n"
        );
        let loader = format!(
            "local envelope = LycheeDevSlotEnvelope\nLycheeDevSlotEnvelope = nil\nlocal ns = LycheeDevInternal\nif ns and ns.SlotRuntime then ns.SlotRuntime.Receive({slot}, envelope) end\n"
        );
        fs::write(target.join(format!("Lychee Dev Slot {slot:02}.toc")), toc).unwrap();
        fs::write(target.join("Loader.lua"), loader).unwrap();
        fs::write(target.join("Payload.lua"), payload).unwrap();
        entries.push(json!({"payloadHash":storage::sha256(payload),"consumed":false}));
    }
    let mut pool =
        json!({"schema":"lycheedev.slots.v3","version":VERSION,"state":"ready","files":entries});
    pool["files"][0]["nonce"] = json!("unknown");
    storage::atomic_json(&temp.0.join(".lycheedev-slots.json"), &pool).unwrap();
    assert_eq!(
        slots::install(&temp.0, VERSION).err().unwrap().code,
        "delivery.installation_conflict"
    );
    assert!(!slots::directory(&temp.0, 201).exists());
    pool["files"][0].as_object_mut().unwrap().remove("nonce");
    storage::atomic_json(&temp.0.join(".lycheedev-slots.json"), &pool).unwrap();
    slots::install(&temp.0, VERSION).unwrap();
    assert_eq!(
        slots::inspect(&temp.0, VERSION).unwrap()["files"]
            .as_array()
            .unwrap()
            .len(),
        500
    );
    assert!(!temp.0.join(".lycheedev-slots.migration-v4.json").exists());
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
    deploy::fresh(&old, &target, "skill", VERSION).unwrap();
    let state = temp.0.join("delivery-state");
    let targets = [lycheedev_delivery::update::UpdateTarget {
        component: "skill".into(),
        path: target.clone(),
    }];
    let planned =
        lycheedev_delivery::update::managed(&next, VERSION, &state, &targets, true).unwrap();
    assert_eq!(planned.data["plan"], true);
    assert_eq!(planned.data["complete"], false);
    assert!(!state.exists());
    assert_eq!(fs::read(target.join("SKILL.md")).unwrap(), b"old skill");
    let applied =
        lycheedev_delivery::update::managed(&next, VERSION, &state, &targets, false).unwrap();
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
    assert!(lycheedev_delivery::update::managed(&next, VERSION, &state, &targets, false).is_ok());
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
        deploy::fresh(&old, &path, "skill", VERSION).unwrap();
        targets.push(lycheedev_delivery::update::UpdateTarget {
            component: "skill".into(),
            path,
        });
    }
    fs::write(targets[1].path.join("SKILL.md"), b"owner edit").unwrap();
    assert_eq!(
        lycheedev_delivery::update::managed(&next, VERSION, &temp.0.join("state"), &targets, false)
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
