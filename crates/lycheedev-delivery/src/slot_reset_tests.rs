use super::*;
use crate::installation::Resource;
const VERSION: &str = "3.2.6";
const INERT: &[u8] = b"LycheeDevSlotEnvelope = nil\n";
struct Fixture {
    root: PathBuf,
    client: PathBuf,
    parent: PathBuf,
    archive: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "slot-reset-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let client = root.join("_classic_titan_");
        let parent = client.join("Interface/AddOns");
        let addon = parent.join("Lychee Dev");
        fs::create_dir_all(addon.join("Bridge")).unwrap();
        fs::write(client.join(".flavor.info"), "wow_classic_titan").unwrap();
        fs::write(client.join("version.txt"), "3.80.2.69874").unwrap();
        let bytes = b"-- managed reset fixture\n";
        fs::write(addon.join("Bridge/SlotRuntime.lua"), bytes).unwrap();
        let receipt = Receipt {
            schema: "lycheedev.installation.v1".into(),
            component: "addon".into(),
            version: VERSION.into(),
            commit: "a".repeat(40),
            resources: vec![Resource {
                path: "addon/Bridge/SlotRuntime.lua".into(),
                bytes: bytes.len() as u64,
                sha256: storage::sha256(bytes),
            }],
        };
        fs::write(
            addon.join(installation::MARKER),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        // Fixture setup, not installation coverage: production reset below
        // must archive arbitrary old bytes and construct the actual new pool.
        for index in 1..=slots::COUNT {
            let path = slots::directory(&parent, index);
            fs::create_dir(&path).unwrap();
            for (name, bytes) in slots::files(index, VERSION) {
                fs::write(path.join(name), bytes).unwrap();
            }
        }
        let pool = slots::SlotPool {
            schema: slots::SCHEMA.into(),
            version: VERSION.into(),
            state: "ready".into(),
            files: vec![
                slots::SlotFile {
                    payload_hash: storage::sha256(INERT),
                    ..Default::default()
                };
                slots::COUNT as usize
            ],
            ..Default::default()
        };
        fs::write(
            parent.join(slots::MARKER),
            serde_json::to_vec(&pool).unwrap(),
        )
        .unwrap();
        let archive = root.join("reset.slots");
        Self {
            root,
            client,
            parent,
            archive,
        }
    }
    fn run(&self) -> Result<SlotReset> {
        reset(
            &self.client,
            &self.archive,
            VERSION,
            &OperationControl::default(),
        )
    }
    fn initial(&self) {
        let pool =
            slots::inspect_typed(&self.parent, VERSION, &OperationControl::default()).unwrap();
        assert_eq!(pool.files.len(), 500);
        assert!(pool.files.iter().all(|slot| *slot
            == slots::SlotFile {
                payload_hash: storage::sha256(INERT),
                ..Default::default()
            }));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn reset_archives_missing_or_corrupt_manifest_and_arbitrary_original_bytes() {
    for missing in [true, false] {
        let f = Fixture::new();
        let original = b"{broken manifest\0\xff\n";
        if missing {
            fs::remove_file(f.parent.join(slots::MARKER)).unwrap();
        } else {
            fs::write(f.parent.join(slots::MARKER), original).unwrap();
        }
        let unknown = b"unknown unresolved bytes\0\xff\n";
        fs::write(slots::directory(&f.parent, 1).join("Payload.lua"), unknown).unwrap();
        fs::write(slots::directory(&f.parent, 2).join("Loader.lua"), unknown).unwrap();
        fs::remove_file(slots::directory(&f.parent, 3).join("Payload.lua")).unwrap();
        fs::create_dir(slots::directory(&f.parent, 4).join("unknown")).unwrap();
        fs::write(
            slots::directory(&f.parent, 4).join("unknown/bytes.bin"),
            unknown,
        )
        .unwrap();
        fs::remove_dir_all(slots::directory(&f.parent, 500)).unwrap();
        fs::write(f.parent.join("other-addon.bin"), unknown).unwrap();
        assert!(f.run().unwrap().complete);
        f.initial();
        assert_eq!(
            fs::read(slots::directory(&f.archive, 1).join("Payload.lua")).unwrap(),
            unknown
        );
        assert_eq!(
            fs::read(slots::directory(&f.archive, 4).join("unknown/bytes.bin")).unwrap(),
            unknown
        );
        assert_eq!(fs::read(f.parent.join("other-addon.bin")).unwrap(), unknown);
        assert!(!slots::directory(&f.archive, 500).exists());
        assert!(!slots::directory(&f.archive, 3).join("Payload.lua").exists());
        if missing {
            assert!(!f.archive.join(slots::MARKER).exists());
        } else {
            assert_eq!(fs::read(f.archive.join(slots::MARKER)).unwrap(), original);
        }
    }
}
#[test]
fn reset_resumes_durable_archiving_and_installing_intents() {
    for installing in [false, true] {
        let f = Fixture::new();
        let control = OperationControl::default();
        let cancelled = control.tracked_cancel_handle().unwrap();
        let mut moved = 0;
        let error = reset_with_observer(&f.client, &f.archive, VERSION, &control, |step| {
            if matches!(step, Step::Archived) {
                moved += 1;
            }
            if !installing && moved == 2 || installing && matches!(step, Step::Installing) {
                cancelled.cancel();
            }
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        let raw = fs::read(companion(&f.archive)).unwrap();
        let intent: Intent = serde_json::from_slice(&raw).unwrap();
        assert_eq!(
            intent.phase,
            if installing {
                "installing"
            } else {
                "archiving"
            }
        );
        assert!(f.run().unwrap().complete);
        f.initial();
    }
}
#[test]
fn reset_completed_retry_preserves_later_reservation_and_checks_original_archive() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let mut pool = slots::read_pool(&f.parent).unwrap();
    pool.files[0].nonce = "a".repeat(32);
    pool.files[0].pending_hash = "b".repeat(64);
    slots::save_pool(&f.parent, &pool, &control).unwrap();
    let original = fs::read(f.parent.join(slots::MARKER)).unwrap();
    f.run().unwrap();
    assert_eq!(fs::read(f.archive.join(slots::MARKER)).unwrap(), original);
    let mut pool = slots::read_pool(&f.parent).unwrap();
    pool.files[1].nonce = "c".repeat(32);
    slots::save_pool(&f.parent, &pool, &control).unwrap();
    let later = fs::read(f.parent.join(slots::MARKER)).unwrap();
    f.run().unwrap();
    assert_eq!(fs::read(f.parent.join(slots::MARKER)).unwrap(), later);
    fs::write(
        slots::directory(&f.archive, 1).join("Payload.lua"),
        b"changed archive",
    )
    .unwrap();
    assert_eq!(f.run().unwrap_err().code, "delivery.installation_conflict");
    assert_eq!(fs::read(f.parent.join(slots::MARKER)).unwrap(), later);
}
#[test]
fn reset_rejects_unowned_archive_modified_main_and_version_without_mutation() {
    let f = Fixture::new();
    fs::create_dir(&f.archive).unwrap();
    assert_eq!(f.run().unwrap_err().code, "delivery.installation_conflict");
    assert!(
        reset(
            &f.client,
            &f.root.join("wrong-version"),
            "0.0.0",
            &OperationControl::default()
        )
        .is_err()
    );
    assert!(
        reset(
            &f.client,
            &f.parent.join("archive"),
            VERSION,
            &OperationControl::default()
        )
        .is_err()
    );
    fs::write(
        f.parent.join("Lychee Dev/Bridge/SlotRuntime.lua"),
        b"external edit",
    )
    .unwrap();
    assert!(
        reset(
            &f.client,
            &f.root.join("modified-main"),
            VERSION,
            &OperationControl::default()
        )
        .is_err()
    );
    assert!(!companion(&f.archive).exists());
    f.initial();
}
#[test]
fn reset_cancellation_precedes_intent_and_maintenance_excludes_mutation() {
    let f = Fixture::new();
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        reset(&f.client, &f.archive, VERSION, &control)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    let maintenance = InstallationMaintenanceLease::acquire(&f.parent.join("Lychee Dev")).unwrap();
    assert_eq!(f.run().unwrap_err().code, "workspace.busy");
    maintenance.close().unwrap();
    assert!(!f.archive.exists());
    assert!(!companion(&f.archive).exists());
    f.initial();
}
