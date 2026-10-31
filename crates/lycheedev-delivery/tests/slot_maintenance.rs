//! Current 500-slot installation, version upgrade and integrity maintenance
//! fixtures. Publication and input use separate current protocol boundaries.
use lycheedev_core::{control::OperationControl, storage};
use lycheedev_delivery::slots::{self, SlotFile, SlotPool};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
#[path = "../src/slot_test_diagnostics.rs"]
mod slot_test_diagnostics;
use slot_test_diagnostics::SlotPhase;

static NEXT: AtomicU64 = AtomicU64::new(0);
const OLD: &str = "3.1.2";
const NEW: &str = "3.2.0";
const INERT: &[u8] = b"LycheeDevSlotEnvelope = nil\n";
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-slot-maintenance-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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
fn save(parent: &Path, pool: &SlotPool) {
    storage::atomic_json(&parent.join(slots::MARKER), pool).unwrap();
}
fn load(parent: &Path) -> SlotPool {
    storage::read_json(&parent.join(slots::MARKER)).unwrap()
}
fn raw(parent: &Path) -> Vec<u8> {
    fs::read(parent.join(slots::MARKER)).unwrap()
}
fn fixture(parent: &Path) -> SlotPool {
    let phase = SlotPhase::begin("maintenance.fixture", None);
    let pool = SlotPool {
        schema: slots::SCHEMA.into(),
        version: OLD.into(),
        state: "ready".into(),
        files: vec![
            SlotFile {
                payload_hash: storage::sha256(INERT),
                ..Default::default()
            };
            slots::COUNT as usize
        ],
        ..Default::default()
    };
    for index in 1..=slots::COUNT {
        let directory = slots::directory(parent, index as u16);
        fs::create_dir(&directory).unwrap();
        let files = slots::files(index, OLD);
        for (name, bytes) in files {
            fs::write(directory.join(name), bytes).unwrap();
        }
    }
    save(parent, &pool);
    phase.finish(None);
    pool
}
// Each mutation and its diagnostics share one fixture control. Fixture
// construction remains outside that operation budget.
fn install(parent: &Path, version: &str) -> lycheedev_core::Result<Value> {
    let control = OperationControl::default();
    let phase = SlotPhase::begin("maintenance.install", Some(&control));
    let result = slots::install_controlled(parent, version, &control);
    phase.finish(result.as_ref().err());
    result
}
fn upgrading(mut pool: SlotPool) -> SlotPool {
    pool.source_schema = pool.schema;
    pool.schema = "lycheedev.slots.migration.v4".into();
    pool.state = "upgrading".into();
    pool.pending_version = NEW.into();
    pool
}
fn assert_current(parent: &Path, version: &str) -> Value {
    let pool = slots::inspect_controlled(parent, version, &Default::default()).unwrap();
    assert_eq!(pool["schema"], slots::SCHEMA);
    assert_eq!(pool["files"].as_array().unwrap().len(), 500);
    for absent in [
        "modules",
        "generation",
        "transport",
        "slotCount",
        "sourceSchema",
        "pendingVersion",
    ] {
        assert!(pool.get(absent).is_none(), "{absent}");
    }
    assert_eq!(
        pool["files"][0],
        json!({"payloadHash":storage::sha256(INERT),"consumed":false})
    );
    assert!(!parent.join(".lycheedev-slots.migration-v4.json").exists());
    pool
}
#[test]
fn fresh_pool_uses_exact_formal_shape_and_is_idempotent() {
    let temp = Temp::new();
    let first = install(&temp.0, NEW).unwrap();
    assert_eq!(first, assert_current(&temp.0, NEW));
    let before = raw(&temp.0);
    assert_eq!(install(&temp.0, NEW).unwrap(), first);
    assert_eq!(raw(&temp.0), before);
    for index in [1, 64, 200, 201, 500] {
        for (name, expected) in slots::files(index, NEW) {
            assert_eq!(
                fs::read(slots::directory(&temp.0, index).join(name)).unwrap(),
                expected
            );
        }
    }
}
#[test]
fn unresolved_pending_or_unknown_nonce_blocks_without_rewriting_marker() {
    for pending in [false, true] {
        let temp = Temp::new();
        let mut pool = fixture(&temp.0);
        if pending {
            pool.files[0].pending_hash = pool.files[0].payload_hash.clone();
        } else {
            pool.files[0].nonce = "a".repeat(32);
        }
        save(&temp.0, &pool);
        let before = raw(&temp.0);
        for error in [
            slots::preflight_controlled(&temp.0, NEW, &Default::default()).unwrap_err(),
            install(&temp.0, NEW).unwrap_err(),
        ] {
            assert_eq!(error.code, "delivery.installation_conflict");
            assert_eq!(error.exit_code, 3);
        }
        assert_eq!(raw(&temp.0), before);
    }
}
#[test]
fn consumed_and_explicitly_retired_reservations_can_migrate_but_pending_always_blocks() {
    for mode in ["consumed", "runtime", "process"] {
        let temp = Temp::new();
        let mut pool = fixture(&temp.0);
        pool.files[0].nonce = "a".repeat(32);
        match mode {
            "consumed" => pool.files[0].consumed = true,
            "runtime" => pool.files[0].retired_runtime = "b".repeat(32),
            _ => pool.files[0].retired_process = true,
        }
        pool.files[0].pending_hash = pool.files[0].payload_hash.clone();
        save(&temp.0, &pool);
        assert_eq!(install(&temp.0, NEW).unwrap_err().exit_code, 3);
        pool.files[0].pending_hash.clear();
        save(&temp.0, &pool);
        install(&temp.0, NEW).unwrap();
        assert_current(&temp.0, NEW);
    }
}
#[test]
fn migration_preflights_last_foreign_byte_before_changing_any_first_slot_byte() {
    let temp = Temp::new();
    let pool = fixture(&temp.0);
    save(&temp.0, &upgrading(pool));
    let extension = slots::directory(&temp.0, 500);
    fs::write(extension.join("Loader.lua"), b"foreign").unwrap();
    let first = slots::directory(&temp.0, 1).join("Lychee Dev Slot 01.toc");
    let before = fs::read(&first).unwrap();
    let marker = raw(&temp.0);
    assert_eq!(install(&temp.0, NEW).unwrap_err().exit_code, 4);
    assert_eq!(raw(&temp.0), marker);
    assert_eq!(fs::read(first).unwrap(), before);
}
#[test]
fn migration_rejects_missing_old_members_and_wrong_resume_version() {
    let temp = Temp::new();
    let pool = fixture(&temp.0);
    save(&temp.0, &upgrading(pool));
    let before = raw(&temp.0);
    assert_eq!(install(&temp.0, "different").unwrap_err().exit_code, 3);
    assert_eq!(raw(&temp.0), before);
    fs::remove_file(slots::directory(&temp.0, 64).join("Payload.lua")).unwrap();
    assert!(install(&temp.0, NEW).is_err());
    assert_eq!(raw(&temp.0), before);
}
#[test]
fn malformed_or_historical_rust_metadata_is_rejected_without_write() {
    // Negative evidence: older slot-pool schemas remain deliberate rejection
    // inputs; they are not migrated or rewritten by the current installer.
    for (schema, count, state, source) in [
        ("lycheedev.slots.v1", 200, "ready", ""),
        ("lycheedev.slots.v2", 64, "ready", ""),
        (slots::SCHEMA, 499, "ready", ""),
        ("lycheedev.slots.v4", 500, "ready", ""),
        (
            "lycheedev.slots.migration.v2",
            64,
            "upgrading",
            "lycheedev.slots.v1",
        ),
        ("lycheedev.slots.v1", 64, "upgrading", ""),
    ] {
        let temp = Temp::new();
        let pool = SlotPool {
            schema: schema.into(),
            version: OLD.into(),
            state: state.into(),
            source_schema: source.into(),
            pending_version: NEW.into(),
            files: vec![SlotFile::default(); count],
        };
        save(&temp.0, &pool);
        let before = raw(&temp.0);
        assert_eq!(
            install(&temp.0, NEW).unwrap_err().code,
            "delivery.invalid_installation"
        );
        assert_eq!(raw(&temp.0), before);
        assert!(!slots::directory(&temp.0, 1).exists());
    }
}
#[test]
fn expanded_500_entry_manifest_is_bounded_at_256_kib_not_64_kib() {
    let temp = Temp::new();
    let mut pool = fixture(&temp.0);
    for entry in &mut pool.files {
        entry.consumer = "4294967295/18446744073709551615".into();
        entry.nonce = "1".repeat(32);
        entry.runtime = "2".repeat(32);
        entry.pending_hash = entry.payload_hash.clone();
        entry.retired_runtime = "3".repeat(32);
    }
    save(&temp.0, &pool);
    assert!(raw(&temp.0).len() > 65536);
    assert!(raw(&temp.0).len() <= 256 << 10);
    slots::inspect_controlled(&temp.0, OLD, &Default::default()).unwrap();
    let mut raw = raw(&temp.0);
    raw.resize((256 << 10) + 1, b' ');
    fs::write(temp.0.join(slots::MARKER), &raw).unwrap();
    assert_eq!(slots::inspect_controlled(&temp.0, OLD, &Default::default()).unwrap_err().exit_code, 4);
    assert_eq!(fs::read(temp.0.join(slots::MARKER)).unwrap(), raw);
}
#[test]
fn inspection_recognizes_both_torn_payload_hashes_without_confirming_consumption() {
    let temp = Temp::new();
    let mut pool = fixture(&temp.0);
    let pending = b"LycheeDevSlotEnvelope = {fixture = true}\n";
    pool.files[499].pending_hash = storage::sha256(pending);
    pool.files[499].nonce = "a".repeat(32);
    save(&temp.0, &pool);
    slots::inspect_controlled(&temp.0, OLD, &Default::default()).unwrap();
    fs::write(slots::directory(&temp.0, 500).join("Payload.lua"), pending).unwrap();
    slots::inspect_controlled(&temp.0, OLD, &Default::default()).unwrap();
    assert!(!load(&temp.0).files[499].consumed);
    fs::write(
        slots::directory(&temp.0, 500).join("Payload.lua"),
        b"foreign",
    )
    .unwrap();
    assert_eq!(slots::inspect_controlled(&temp.0, OLD, &Default::default()).unwrap_err().exit_code, 4);
}
#[test]
fn cancellation_preserves_original_marker_and_never_creates_slots() {
    let temp = Temp::new();
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        slots::install_controlled(&temp.0, NEW, &control)
            .unwrap_err()
            .exit_code,
        7
    );
    assert!(!temp.0.join(slots::MARKER).exists());
    let pool = fixture(&temp.0);
    save(&temp.0, &upgrading(pool));
    let before = raw(&temp.0);
    assert_eq!(
        slots::install_controlled(&temp.0, NEW, &control)
            .unwrap_err()
            .exit_code,
        7
    );
    assert_eq!(raw(&temp.0), before);
}
#[test]
fn unknown_extra_members_and_unmanaged_empty_directories_are_never_adopted() {
    let temp = Temp::new();
    fs::create_dir(slots::directory(&temp.0, 1)).unwrap();
    assert_eq!(install(&temp.0, NEW).unwrap_err().exit_code, 4);
    assert!(!temp.0.join(slots::MARKER).exists());
    fs::remove_dir(slots::directory(&temp.0, 1)).unwrap();
    fixture(&temp.0);
    let foreign = slots::directory(&temp.0, 500).join("other.lua");
    fs::write(&foreign, b"owner notes").unwrap();
    let before = raw(&temp.0);
    assert!(install(&temp.0, NEW).is_err());
    assert_eq!(raw(&temp.0), before);
    assert_eq!(fs::read(foreign).unwrap(), b"owner notes");
}
#[test]
fn omitted_and_null_optional_ownership_fields_match_zero_values() {
    let row: SlotFile=serde_json::from_value(json!({"payloadHash":"abc","pendingHash":null,"nonce":null,"runtime":null,"consumer":null,"consumed":null,"retiredRuntime":null,"retiredProcess":null})).unwrap();
    assert_eq!(
        serde_json::to_value(row).unwrap(),
        json!({"payloadHash":"abc","consumed":false})
    );
    let temp = Temp::new();
    let pool = fixture(&temp.0);
    let mut value = serde_json::to_value(&pool).unwrap();
    value["sourceSchema"] = Value::Null;
    value["files"][0]["nonce"] = Value::Null;
    storage::atomic_json(&temp.0.join(slots::MARKER), &value).unwrap();
    install(&temp.0, NEW).unwrap();
    assert_current(&temp.0, NEW);
}
#[cfg(windows)]
#[test]
fn linked_slot_directory_is_rejected_without_following_it() {
    use std::os::windows::fs::MetadataExt;

    let temp = Temp::new();
    let outside = Temp::new();
    fixture(&temp.0);
    slots::preflight_controlled(&temp.0, NEW, &Default::default()).unwrap();
    let marker = raw(&temp.0);
    let first = slots::directory(&temp.0, 1).join("Lychee Dev Slot 01.toc");
    let first_bytes = fs::read(&first).unwrap();
    let directory = slots::directory(&temp.0, 64);
    let members = slots::files(64, OLD).map(|(name, _)| {
        let bytes = fs::read(directory.join(&name)).unwrap();
        (name, bytes)
    });
    let outside_directory = outside.0.join("original-slot");
    fs::rename(&directory, &outside_directory).unwrap();

    // A junction points to the fixture’s exact original slot directory.
    // The target is the admitted slot's exact original directory, not a malformed
    // substitute that could fail for an unrelated content or ownership reason.
    let junction = std::process::Command::new("node")
        .args([
            "-e",
            "require('node:fs').symlinkSync(process.argv[1], process.argv[2], 'junction');",
        ])
        .arg(&outside_directory)
        .arg(&directory)
        .output()
        .expect("create Windows slot junction fixture with Node");
    assert!(
        junction.status.success(),
        "junction fixture failed: {}",
        String::from_utf8_lossy(&junction.stderr)
    );
    let attributes = fs::symlink_metadata(&directory).unwrap().file_attributes();
    assert_ne!(attributes & 0x400, 0, "fixture must be a reparse point");
    assert_ne!(attributes & 0x10, 0, "fixture must redirect a directory");
    assert!(fs::metadata(&directory).unwrap().is_dir());
    assert_eq!(
        fs::canonicalize(&directory).unwrap(),
        fs::canonicalize(&outside_directory).unwrap()
    );
    for (name, bytes) in &members {
        assert_eq!(fs::read(directory.join(name)).unwrap(), *bytes);
    }

    for error in [
        slots::preflight_controlled(&temp.0, NEW, &Default::default()).unwrap_err(),
        install(&temp.0, NEW).unwrap_err(),
    ] {
        assert_eq!(error.code, "input.invalid");
        assert_eq!(error.exit_code, 4);
        assert_eq!(error.message, "reparse point in managed path");
    }
    assert_eq!(raw(&temp.0), marker);
    assert_eq!(fs::read(first).unwrap(), first_bytes);

    assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&outside_directory).unwrap().count(), 3);
    for (name, bytes) in &members {
        assert_eq!(fs::read(outside_directory.join(name)).unwrap(), *bytes);
    }
    fs::remove_dir(&directory).unwrap(); // Remove only the junction.
    for (name, bytes) in &members {
        assert_eq!(fs::read(outside_directory.join(name)).unwrap(), *bytes);
    }
}
#[cfg(windows)]
#[test]
#[ignore = "requires Windows file-symlink privilege or Developer Mode; run explicitly with --ignored"]
fn linked_static_slot_member_is_rejected_without_following_it() {
    use std::os::windows::fs::{MetadataExt, symlink_file};

    let temp = Temp::new();
    let outside = Temp::new();
    fixture(&temp.0);
    slots::inspect_controlled(&temp.0, OLD, &Default::default()).unwrap();
    let marker = raw(&temp.0);
    let member = slots::directory(&temp.0, 500).join("Loader.lua");
    let bytes = fs::read(&member).unwrap();
    assert_eq!(bytes, slots::files(500, OLD)[1].1);
    let outside_file = outside.0.join("Loader.lua");
    fs::rename(&member, &outside_file).unwrap();
    // A directory junction would change this into a non-file negative. Preserve
    // a genuine file link to the exact valid static bytes; selected setup errors
    // fail this test instead of silently passing without reaching the consumer.
    symlink_file(&outside_file, &member).expect(
        "create static-member file symlink (requires Developer Mode or file-symlink privilege)",
    );
    let metadata = fs::symlink_metadata(&member).unwrap();
    assert!(metadata.file_type().is_symlink());
    assert_ne!(metadata.file_attributes() & 0x400, 0);
    assert_eq!(metadata.file_attributes() & 0x10, 0);
    assert!(fs::metadata(&member).unwrap().is_file());
    assert_eq!(
        fs::canonicalize(&member).unwrap(),
        fs::canonicalize(&outside_file).unwrap()
    );
    assert_eq!(fs::read(&member).unwrap(), bytes);

    let error = slots::inspect_controlled(&temp.0, OLD, &Default::default()).unwrap_err();
    assert_eq!(error.code, "delivery.invalid_installation");
    assert_eq!(error.exit_code, 4);
    assert_eq!(error.message, "slot member is not a bounded regular file");
    assert_eq!(raw(&temp.0), marker);
    assert_eq!(fs::read(&outside_file).unwrap(), bytes);
    assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 1);
    fs::remove_file(&member).unwrap(); // Remove only the file symlink.
    assert_eq!(fs::read(outside_file).unwrap(), bytes);
}
#[test]
fn json_sequence_casing_duplicates_and_null_never_erase_reservations() {
    let row: SlotFile=serde_json::from_str(r#"{"Nonce":"reserved","nonce":null,"PENDINGHASH":"pending","pendingHash":null,"Consumed":true,"conſumed":null,"unknown":{"nonce":"ignored"}}"#).unwrap();
    assert_eq!(row.nonce, "reserved");
    assert_eq!(row.pending_hash, "pending");
    assert!(row.consumed);
    let row: SlotFile = serde_json::from_str(
        r#"{"nonce":"first","NoNcE":"last","retiredProcess":true,"RetiredProcess":null}"#,
    )
    .unwrap();
    assert_eq!(row.nonce, "last");
    assert!(row.retired_process);
    let row: SlotFile = serde_json::from_str("null").unwrap();
    assert_eq!(row, SlotFile::default());
    // The ordered decoder reuses slice backing storage across duplicate field arrays. A later
    // empty object/null leaves prior struct fields untouched, including rows
    // beyond an intervening shorter array's logical length.
    let pool: SlotPool = serde_json::from_str(
        r#"{"FILES":[{"nonce":"first"},{"nonce":"second"}],"files":[{}],"Files":[null,{}]}"#,
    )
    .unwrap();
    assert_eq!(pool.files.len(), 2);
    assert_eq!(pool.files[0].nonce, "first");
    assert_eq!(pool.files[1].nonce, "second");
    for reset in ["null", "[]"] {
        let pool: SlotPool = serde_json::from_str(&format!(
            r#"{{"files":[{{"nonce":"reserved"}}],"Files":{reset},"files":[{{}}]}}"#
        ))
        .unwrap();
        assert!(pool.files[0].nonce.is_empty());
    }
    for field in ["Nonce", "PendingHash"] {
        let temp = Temp::new();
        let pool = fixture(&temp.0);
        let original = serde_json::to_string(&pool).unwrap();
        let raw = original.replacen(
            "\"consumed\":false",
            &format!(
                "\"{field}\":\"reserved\",\"{}\":null,\"consumed\":false",
                if field == "Nonce" {
                    "nonce"
                } else {
                    "pendingHash"
                }
            ),
            1,
        );
        fs::write(temp.0.join(slots::MARKER), &raw).unwrap();
        assert_eq!(install(&temp.0, NEW).unwrap_err().exit_code, 3);
        assert_eq!(
            fs::read(temp.0.join(slots::MARKER)).unwrap(),
            raw.as_bytes()
        );
    }
}
