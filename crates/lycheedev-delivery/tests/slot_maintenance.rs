//! Fixed Go 3.2.0 slots.go/slot_migration_test.go/slot_remove.go maintenance
//! fixtures. These do not authorize input or claim the byte-only API is v1.
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
fn fixture(parent: &Path, schema: &str, count: usize) -> SlotPool {
    let phase = SlotPhase::begin("maintenance.fixture", None);
    let pool = SlotPool {
        schema: schema.into(),
        version: OLD.into(),
        state: "ready".into(),
        files: vec![
            SlotFile {
                payload_hash: storage::sha256(INERT),
                ..Default::default()
            };
            count
        ],
        ..Default::default()
    };
    for index in 1..=count {
        let directory = slots::directory(parent, index as u16);
        fs::create_dir(&directory).unwrap();
        let mut files = slots::files(index as u16, OLD);
        if schema != slots::SCHEMA {
            let transport = if schema == "lycheedev.slots.v1" {
                "memory-slot-v1"
            } else {
                "memory-slot-v2"
            };
            files[0].1 = String::from_utf8(files[0].1.clone())
                .unwrap()
                .replace(slots::INTERFACES, "120100, 50504, 38002, 16001")
                .replace(slots::TRANSPORT, transport)
                .into_bytes();
        }
        for (name, bytes) in files {
            fs::write(directory.join(name), bytes).unwrap();
        }
    }
    save(parent, &pool);
    phase.finish(None);
    pool
}
// Match slots::install's original per-call default budget. Diagnostic output
// is inside that same budget; fixture construction remains outside it.
fn install(parent: &Path, version: &str) -> lycheedev_core::Result<Value> {
    let control = OperationControl::default();
    let phase = SlotPhase::begin("maintenance.install", Some(&control));
    let result = slots::install_controlled(parent, version, &control);
    phase.finish(result.as_ref().err());
    result
}
fn migrating(mut pool: SlotPool, previous: bool) -> SlotPool {
    pool.source_schema = pool.schema;
    pool.schema = if previous {
        "lycheedev.slots.migration.v3"
    } else {
        "lycheedev.slots.migration.v4"
    }
    .into();
    pool.state = "upgrading".into();
    pool.pending_version = NEW.into();
    pool
}
fn assert_current(parent: &Path, version: &str) -> Value {
    let pool = slots::inspect(parent, version).unwrap();
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
fn canonical_legacy_64_and_previous_200_pools_migrate_without_business_admission() {
    for (schema, count) in [
        ("lycheedev.slots.v1", 64),
        ("lycheedev.slots.v2", 200),
        (slots::SCHEMA, 200),
    ] {
        let temp = Temp::new();
        fixture(&temp.0, schema, count);
        assert!(slots::inspect(&temp.0, OLD).is_err());
        let before = raw(&temp.0);
        slots::preflight(&temp.0, NEW).unwrap();
        assert_eq!(raw(&temp.0), before);
        install(&temp.0, NEW).unwrap();
        assert_current(&temp.0, NEW);
    }
}
#[test]
fn same_version_200_pool_expands_but_unowned_extension_is_never_adopted() {
    let temp = Temp::new();
    fixture(&temp.0, slots::SCHEMA, 200);
    let foreign = slots::directory(&temp.0, 500);
    fs::create_dir(&foreign).unwrap();
    let before = raw(&temp.0);
    let error = install(&temp.0, OLD).unwrap_err();
    assert_eq!(error.code, "delivery.invalid_installation");
    assert_eq!(error.exit_code, 4);
    assert_eq!(raw(&temp.0), before);
    assert!(!slots::directory(&temp.0, 201).exists());
    fs::remove_dir(foreign).unwrap();
    install(&temp.0, OLD).unwrap();
    assert_current(&temp.0, OLD);
}
#[test]
fn unresolved_pending_or_unknown_nonce_blocks_without_rewriting_marker() {
    for pending in [false, true] {
        let temp = Temp::new();
        let mut pool = fixture(&temp.0, "lycheedev.slots.v1", 64);
        if pending {
            pool.files[0].pending_hash = pool.files[0].payload_hash.clone();
        } else {
            pool.files[0].nonce = "a".repeat(32);
        }
        save(&temp.0, &pool);
        let before = raw(&temp.0);
        for error in [
            slots::preflight(&temp.0, NEW).unwrap_err(),
            install(&temp.0, NEW).unwrap_err(),
        ] {
            assert_eq!(error.code, "delivery.installation_conflict");
            assert_eq!(error.exit_code, 3);
        }
        assert_eq!(raw(&temp.0), before);
        assert!(!slots::directory(&temp.0, 65).exists());
    }
}
#[test]
fn consumed_and_explicitly_retired_reservations_can_migrate_but_pending_always_blocks() {
    for mode in ["consumed", "runtime", "process"] {
        let temp = Temp::new();
        let mut pool = fixture(&temp.0, "lycheedev.slots.v1", 64);
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
fn current_pool_recognizes_only_exact_previous_interface_and_transport_tocs() {
    for (interfaces, transport) in [
        ("120100, 50504, 38002, 16001", "memory-slot-v3"),
        ("120100, 50504, 38002, 16001, 11509", "lycheedev-mailbox-v1"),
        (
            "120100, 50504, 38002, 16001, 11509, 20506",
            slots::TRANSPORT,
        ),
        (slots::INTERFACES, slots::TRANSPORT),
    ] {
        let temp = Temp::new();
        let mut pool = fixture(&temp.0, slots::SCHEMA, 500);
        let toc = slots::directory(&temp.0, 1).join("Lychee Dev Slot 01.toc");
        let previous = String::from_utf8(slots::files(1, OLD)[0].1.clone())
            .unwrap()
            .replace(slots::INTERFACES, interfaces)
            .replace(slots::TRANSPORT, transport);
        fs::write(&toc, &previous).unwrap();
        slots::inspect(&temp.0, OLD).unwrap();
        if interfaces != slots::INTERFACES || transport != slots::TRANSPORT {
            pool.files[0].nonce = "a".repeat(32);
            save(&temp.0, &pool);
            assert_eq!(install(&temp.0, OLD).unwrap_err().exit_code, 3);
            pool.files[0].consumed = true;
            save(&temp.0, &pool);
        }
        install(&temp.0, OLD).unwrap();
        assert_eq!(fs::read(&toc).unwrap(), slots::files(1, OLD)[0].1);
        let unverified = String::from_utf8(slots::files(1, OLD)[0].1.clone())
            .unwrap()
            .replace("120105", "120106");
        fs::write(&toc, unverified).unwrap();
        assert_eq!(slots::inspect(&temp.0, OLD).unwrap_err().exit_code, 4);
        fs::write(&toc, previous.replace("## Title:", "## WrongTitle:")).unwrap();
        assert_eq!(slots::inspect(&temp.0, OLD).unwrap_err().exit_code, 4);
    }
}
#[test]
fn current_and_previous_migration_markers_resume_partial_static_and_extension() {
    for previous in [false, true] {
        let temp = Temp::new();
        let pool = fixture(&temp.0, slots::SCHEMA, 200);
        save(&temp.0, &migrating(pool, previous));
        fs::write(
            slots::directory(&temp.0, 1).join("Lychee Dev Slot 01.toc"),
            &slots::files(1, NEW)[0].1,
        )
        .unwrap();
        let extension = slots::directory(&temp.0, 201);
        fs::create_dir(&extension).unwrap();
        fs::write(extension.join("Loader.lua"), &slots::files(201, NEW)[1].1).unwrap();
        assert!(slots::inspect(&temp.0, NEW).is_err());
        install(&temp.0, NEW).unwrap();
        assert_current(&temp.0, NEW);
    }
}
#[test]
fn migration_accepts_previous_target_static_only_with_authenticated_intent() {
    let temp = Temp::new();
    let pool = fixture(&temp.0, "lycheedev.slots.v1", 64);
    save(&temp.0, &migrating(pool, false));
    let toc = String::from_utf8(slots::files(1, NEW)[0].1.clone())
        .unwrap()
        .replace(slots::INTERFACES, "120100, 50504, 38002, 16001")
        .replace(slots::TRANSPORT, "memory-slot-v1");
    fs::write(
        slots::directory(&temp.0, 1).join("Lychee Dev Slot 01.toc"),
        toc,
    )
    .unwrap();
    install(&temp.0, NEW).unwrap();
    assert_current(&temp.0, NEW);
}
#[test]
fn migration_preflights_last_foreign_byte_before_changing_any_first_slot_byte() {
    let temp = Temp::new();
    let pool = fixture(&temp.0, "lycheedev.slots.v1", 64);
    save(&temp.0, &migrating(pool, false));
    let extension = slots::directory(&temp.0, 500);
    fs::create_dir(&extension).unwrap();
    fs::write(extension.join("Loader.lua"), b"foreign").unwrap();
    let first = slots::directory(&temp.0, 1).join("Lychee Dev Slot 01.toc");
    let before = fs::read(&first).unwrap();
    let marker = raw(&temp.0);
    assert_eq!(install(&temp.0, NEW).unwrap_err().exit_code, 4);
    assert_eq!(raw(&temp.0), marker);
    assert_eq!(fs::read(first).unwrap(), before);
    assert!(!slots::directory(&temp.0, 65).exists());
}
#[test]
fn migration_rejects_missing_old_members_and_wrong_resume_version() {
    let temp = Temp::new();
    let pool = fixture(&temp.0, "lycheedev.slots.v1", 64);
    save(&temp.0, &migrating(pool, false));
    let before = raw(&temp.0);
    assert_eq!(install(&temp.0, "different").unwrap_err().exit_code, 3);
    assert_eq!(raw(&temp.0), before);
    fs::remove_file(slots::directory(&temp.0, 64).join("Payload.lua")).unwrap();
    assert!(install(&temp.0, NEW).is_err());
    assert_eq!(raw(&temp.0), before);
}
#[test]
fn malformed_or_historical_rust_metadata_is_rejected_without_write() {
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
    let mut pool = fixture(&temp.0, slots::SCHEMA, 500);
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
    slots::inspect(&temp.0, OLD).unwrap();
    let mut raw = raw(&temp.0);
    raw.resize((256 << 10) + 1, b' ');
    fs::write(temp.0.join(slots::MARKER), &raw).unwrap();
    assert_eq!(slots::inspect(&temp.0, OLD).unwrap_err().exit_code, 4);
    assert_eq!(fs::read(temp.0.join(slots::MARKER)).unwrap(), raw);
}
#[test]
fn inspection_recognizes_both_torn_payload_hashes_without_confirming_consumption() {
    let temp = Temp::new();
    let mut pool = fixture(&temp.0, slots::SCHEMA, 500);
    let pending = b"LycheeDevSlotEnvelope = {fixture = true}\n";
    pool.files[499].pending_hash = storage::sha256(pending);
    pool.files[499].nonce = "a".repeat(32);
    save(&temp.0, &pool);
    slots::inspect(&temp.0, OLD).unwrap();
    fs::write(slots::directory(&temp.0, 500).join("Payload.lua"), pending).unwrap();
    slots::inspect(&temp.0, OLD).unwrap();
    assert!(!load(&temp.0).files[499].consumed);
    fs::write(
        slots::directory(&temp.0, 500).join("Payload.lua"),
        b"foreign",
    )
    .unwrap();
    assert_eq!(slots::inspect(&temp.0, OLD).unwrap_err().exit_code, 4);
}
#[test]
fn cancellation_preserves_original_marker_and_never_creates_slots() {
    let temp = Temp::new();
    let control = OperationControl::default();
    control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(
        slots::install_controlled(&temp.0, NEW, &control)
            .unwrap_err()
            .exit_code,
        7
    );
    assert!(!temp.0.join(slots::MARKER).exists());
    let pool = fixture(&temp.0, "lycheedev.slots.v1", 64);
    save(&temp.0, &migrating(pool, false));
    let before = raw(&temp.0);
    assert_eq!(
        slots::install_controlled(&temp.0, NEW, &control)
            .unwrap_err()
            .exit_code,
        7
    );
    assert_eq!(raw(&temp.0), before);
    assert!(!slots::directory(&temp.0, 65).exists());
}
#[test]
fn unknown_extra_members_and_unmanaged_empty_directories_are_never_adopted() {
    let temp = Temp::new();
    fs::create_dir(slots::directory(&temp.0, 1)).unwrap();
    assert_eq!(install(&temp.0, NEW).unwrap_err().exit_code, 4);
    assert!(!temp.0.join(slots::MARKER).exists());
    fs::remove_dir(slots::directory(&temp.0, 1)).unwrap();
    fixture(&temp.0, slots::SCHEMA, 500);
    let foreign = slots::directory(&temp.0, 500).join("other.lua");
    fs::write(&foreign, b"owner notes").unwrap();
    let before = raw(&temp.0);
    assert!(install(&temp.0, NEW).is_err());
    assert_eq!(raw(&temp.0), before);
    assert_eq!(fs::read(foreign).unwrap(), b"owner notes");
}
#[test]
fn omitted_and_null_optional_ownership_fields_match_go_zero_values() {
    let row: SlotFile=serde_json::from_value(json!({"payloadHash":"abc","pendingHash":null,"nonce":null,"runtime":null,"consumer":null,"consumed":null,"retiredRuntime":null,"retiredProcess":null})).unwrap();
    assert_eq!(
        serde_json::to_value(row).unwrap(),
        json!({"payloadHash":"abc","consumed":false})
    );
    let temp = Temp::new();
    let pool = fixture(&temp.0, "lycheedev.slots.v1", 64);
    let mut value = serde_json::to_value(&pool).unwrap();
    value["sourceSchema"] = Value::Null;
    value["files"][0]["nonce"] = Value::Null;
    storage::atomic_json(&temp.0.join(slots::MARKER), &value).unwrap();
    install(&temp.0, NEW).unwrap();
    assert_current(&temp.0, NEW);
}
#[cfg(unix)]
#[test]
fn linked_slot_directory_or_static_member_is_rejected_without_following_it() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let outside = Temp::new();
    symlink(&outside.0, slots::directory(&temp.0, 1)).unwrap();
    assert!(install(&temp.0, NEW).is_err());
    assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 0);
    fs::remove_file(slots::directory(&temp.0, 1)).unwrap();
    fixture(&temp.0, slots::SCHEMA, 500);
    let member = slots::directory(&temp.0, 500).join("Loader.lua");
    fs::remove_file(&member).unwrap();
    let outside_file = outside.0.join("Loader.lua");
    fs::write(&outside_file, &slots::files(500, OLD)[1].1).unwrap();
    symlink(&outside_file, &member).unwrap();
    assert!(slots::inspect(&temp.0, OLD).is_err());
    assert_eq!(fs::read(outside_file).unwrap(), slots::files(500, OLD)[1].1);
}
#[test]
fn go_json_sequence_casing_duplicates_and_null_never_erase_reservations() {
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
    // Go reuses slice backing storage across duplicate field arrays. A later
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
        let pool = fixture(&temp.0, "lycheedev.slots.v1", 64);
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
        assert!(!slots::directory(&temp.0, 65).exists());
    }
}
