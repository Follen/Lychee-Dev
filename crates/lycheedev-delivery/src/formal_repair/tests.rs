use super::*;
use installation::Resource;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    client: Client,
    release: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "formal-repair-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let directory = root.join("_classic_titan_");
        fs::create_dir_all(directory.join("Interface/AddOns")).unwrap();
        fs::write(directory.join(".flavor.info"), "wow_classic_titan").unwrap();
        fs::write(directory.join("version.txt"), "3.80.2.69874").unwrap();
        let client = Client::from(
            installation_selection::select_with(
                &directory,
                "",
                &mut InvocationMetadata(&OperationControl::default()),
            )
            .unwrap(),
        );
        let release = root.join("release");
        fs::create_dir(&release).unwrap();
        let mut resources = vec![];
        for (path, bytes) in [
            (
                "addon/Lychee Dev.toc",
                b"Bridge/SlotRuntime.lua\n".as_slice(),
            ),
            ("addon/Bridge/SlotRuntime.lua", b"return true\n".as_slice()),
            ("skill/SKILL.md", b"test fixture\n".as_slice()),
            ("tool/luac/test", b"validator fixture only\n".as_slice()),
        ] {
            let target = release.join("payload").join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, bytes).unwrap();
            resources.push(Resource {
                path: path.into(),
                bytes: bytes.len() as u64,
                sha256: storage::sha256(bytes),
            });
        }
        let manifest = release::Release {
            schema: "lycheedev.release.v1".into(),
            version: lycheedev_core::VERSION.into(),
            commit: "a".repeat(40),
            resources,
            binaries: BTreeMap::from([(
                "windows-amd64".into(),
                Resource {
                    path: "native/windows-amd64/lycheedev.exe".into(),
                    bytes: 1,
                    sha256: "a".repeat(64),
                },
            )]),
            runtime: None,
            npm_channel: None,
            corresponding_source: None,
        };
        fs::write(
            release.join("release.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        Self {
            root,
            client,
            release,
        }
    }
    fn parent(&self) -> PathBuf {
        Path::new(&self.client.directory).join("Interface/AddOns")
    }
    fn target(&self) -> PathBuf {
        self.parent().join("Lychee Dev")
    }
    fn run(&self, archive: Option<&Path>) -> Result<Receipt> {
        let control = OperationControl::default();
        let phase = crate::slot_test_diagnostics::SlotPhase::begin("repair.apply", Some(&control));
        let result = apply(
            &self.client,
            &self.release,
            archive,
            lycheedev_core::VERSION,
            &mut FixtureValidator,
            &control,
        );
        phase.finish(result.as_ref().err());
        result
    }
    fn old(&self) -> Receipt {
        fs::create_dir(self.target()).unwrap();
        fs::write(self.target().join("old.lua"), b"old\n").unwrap();
        let receipt = Receipt {
            schema: "lycheedev.installation.v1".into(),
            component: "addon".into(),
            version: "3.0.0".into(),
            commit: "b".repeat(40),
            resources: vec![Resource {
                path: "addon/old.lua".into(),
                bytes: 4,
                sha256: storage::sha256(b"old\n"),
            }],
        };
        fs::write(
            self.target().join(installation::MARKER),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        receipt
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
/// Only the fixture substitutes syntax validation. The production CLI adapter
/// must use the real authenticated compile-only validator.
struct FixtureValidator;
impl AddonValidator for FixtureValidator {
    fn validate(&mut self, root: &Path, version: &str, control: &OperationControl) -> Result<()> {
        release::inspect_with_control(root, version, control)?;
        assert!(root.join("payload/tool/luac/test").is_file());
        assert!(root.join("payload/skill/SKILL.md").is_file());
        Ok(())
    }
}
#[test]
fn formal_repair_fresh_installs_full_five_hundred_slots_and_cleans_snapshot() {
    let f = Fixture::new();
    let receipt = f.run(None).unwrap();
    require_receipt(
        &installation::inspect_main_controlled(&f.target(), "addon", &OperationControl::default())
            .unwrap(),
        &receipt,
        "test",
    )
    .unwrap();
    let pool = slots::inspect_controlled(
        &f.parent(),
        lycheedev_core::VERSION,
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(pool["files"].as_array().unwrap().len(), 500);
    assert!(!fs::read_dir(f.parent()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lycheedev-release-")
    }));
}
#[test]
fn formal_repair_managed_upgrade_archives_original_and_exact_replacement_journal() {
    let f = Fixture::new();
    let old = f.old();
    let archive = f.root.join("archive");
    let next = f.run(Some(&archive)).unwrap();
    require_receipt(
        &installation::inspect_main_controlled(
            &archive.join("previous"),
            "addon",
            &OperationControl::default(),
        )
        .unwrap(),
        &old,
        "test",
    )
    .unwrap();
    let journal: Value =
        serde_json::from_slice(&fs::read(archive.join("replacement.json")).unwrap()).unwrap();
    assert_eq!(journal["previous"], serde_json::to_value(old).unwrap());
    assert_eq!(journal["next"], serde_json::to_value(next).unwrap());
    assert!(!archive.join("next").exists());
}
#[test]
fn formal_repair_preserves_modified_main_and_foreign_files() {
    let f = Fixture::new();
    f.old();
    fs::write(f.target().join("foreign.lua"), b"do not delete").unwrap();
    let archive = f.root.join("archive");
    assert_eq!(
        f.run(Some(&archive)).unwrap_err().code,
        "delivery.installation_conflict"
    );
    assert_eq!(
        fs::read(f.target().join("foreign.lua")).unwrap(),
        b"do not delete"
    );
    assert!(!archive.exists());
    assert!(!f.parent().join(slots::MARKER).exists());
}
#[test]
fn formal_repair_rejects_foreign_slot_before_main_publication() {
    let f = Fixture::new();
    let foreign = slots::directory(&f.parent(), 500);
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("mine"), b"keep").unwrap();
    assert!(f.run(None).is_err());
    assert!(!f.target().exists());
    assert_eq!(fs::read(foreign.join("mine")).unwrap(), b"keep");
}
#[test]
fn formal_repair_rejects_archive_inside_discovery_or_existing_archive() {
    let f = Fixture::new();
    f.old();
    assert_eq!(
        f.run(Some(&f.parent().join("archive"))).unwrap_err().code,
        "delivery.invalid_installation"
    );
    let archive = f.root.join("archive");
    fs::create_dir(&archive).unwrap();
    fs::write(archive.join("keep"), b"mine").unwrap();
    assert!(f.run(Some(&archive)).is_err());
    assert_eq!(fs::read(archive.join("keep")).unwrap(), b"mine");
    assert!(f.target().join("old.lua").exists());
}
#[test]
fn formal_repair_wrong_selected_identity_rejects_before_prepare() {
    let mut f = Fixture::new();
    f.client.full_build = "changed".into();
    assert!(f.run(None).is_err());
    assert_eq!(fs::read_dir(f.parent()).unwrap().count(), 0);
}
#[test]
fn formal_repair_precancelled_never_creates_stage_or_locks() {
    let f = Fixture::new();
    let c = OperationControl::default();
    c.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        apply(
            &f.client,
            &f.release,
            None,
            lycheedev_core::VERSION,
            &mut FixtureValidator,
            &c
        )
        .unwrap_err()
        .code,
        "command.cancelled"
    );
    assert_eq!(fs::read_dir(f.parent()).unwrap().count(), 0);
}
struct Reject;
impl AddonValidator for Reject {
    fn validate(&mut self, _: &Path, _: &str, _: &OperationControl) -> Result<()> {
        Err(ToolkitError::new("source.lua_syntax", "test rejection", 4))
    }
}
#[test]
fn formal_repair_validator_rejection_preserves_original_and_requested_archive() {
    let f = Fixture::new();
    f.old();
    let archive = f.root.join("archive");
    let e = apply(
        &f.client,
        &f.release,
        Some(&archive),
        lycheedev_core::VERSION,
        &mut Reject,
        &OperationControl::default(),
    )
    .unwrap_err();
    assert_eq!(e.code, "source.lua_syntax");
    assert!(f.target().join("old.lua").exists());
    assert!(!archive.exists());
    assert!(Path::new(e.details["retainedPreparation"].as_str().unwrap()).is_dir());
}
struct ChangeClient(PathBuf);
impl AddonValidator for ChangeClient {
    fn validate(&mut self, _: &Path, _: &str, c: &OperationControl) -> Result<()> {
        c.check()?;
        fs::write(&self.0, "3.80.2.69875")?;
        Ok(())
    }
}
#[test]
fn formal_repair_revalidates_client_after_validator_before_mutation() {
    let f = Fixture::new();
    let mut validator = ChangeClient(Path::new(&f.client.directory).join("version.txt"));
    assert!(
        apply(
            &f.client,
            &f.release,
            None,
            lycheedev_core::VERSION,
            &mut validator,
            &OperationControl::default()
        )
        .is_err()
    );
    assert!(!f.target().exists());
    assert!(!f.parent().join(slots::MARKER).exists());
}
#[test]
fn formal_repair_failed_previous_move_boundary_preserves_recovery_without_replay() {
    let f = Fixture::new();
    let old = f.old();
    let c = OperationControl::default();
    let manifest = release::inspect_with_control(&f.release, lycheedev_core::VERSION, &c).unwrap();
    let mut snapshot = snapshot::Snapshot::prepare(&f.release, &f.parent(), &manifest, &c).unwrap();
    let wanted = release::receipt(&manifest, "addon");
    let prepared = snapshot.prepare_addon(&wanted, &c).unwrap();
    let archive = f.root.join("archive");
    let current = installation::inspect_main_controlled(&f.target(), "addon", &c).unwrap();
    let e = publication::publish_with(
        &f.target(),
        &prepared,
        Some(&archive),
        &wanted,
        &current,
        &c,
        &mut |point| {
            if point == publication::Point::PreviousArchived {
                c.tracked_cancel_handle()?.cancel();
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(e.code, "command.cancelled");
    assert!(!f.target().exists());
    let fresh = OperationControl::default();
    require_receipt(
        &installation::inspect_main_controlled(&archive.join("previous"), "addon", &fresh).unwrap(),
        &old,
        "test",
    )
    .unwrap();
    require_receipt(
        &installation::inspect_main_controlled(&archive.join("next"), "addon", &fresh).unwrap(),
        &wanted,
        "test",
    )
    .unwrap();
    assert!(archive.join("replacement.json").is_file());
}
#[test]
fn formal_repair_prepublication_fault_leaves_archive_and_original_untouched() {
    let f = Fixture::new();
    f.old();
    let c = OperationControl::default();
    let manifest = release::inspect_with_control(&f.release, lycheedev_core::VERSION, &c).unwrap();
    let mut snapshot = snapshot::Snapshot::prepare(&f.release, &f.parent(), &manifest, &c).unwrap();
    let wanted = release::receipt(&manifest, "addon");
    let prepared = snapshot.prepare_addon(&wanted, &c).unwrap();
    let archive = f.root.join("archive");
    let current = installation::inspect_main_controlled(&f.target(), "addon", &c).unwrap();
    assert!(
        publication::publish_with(
            &f.target(),
            &prepared,
            Some(&archive),
            &wanted,
            &current,
            &c,
            &mut |point| if point == publication::Point::Prepared {
                Err(ToolkitError::io(std::io::Error::other(
                    "injected preparation fault",
                )))
            } else {
                Ok(())
            }
        )
        .is_err()
    );
    assert!(!archive.exists());
    assert_eq!(fs::read(f.target().join("old.lua")).unwrap(), b"old\n");
}
#[test]
fn formal_repair_cleanup_refuses_foreign_snapshot_bytes() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let manifest = release::inspect_with_control(&f.release, lycheedev_core::VERSION, &c).unwrap();
    let snapshot = snapshot::Snapshot::prepare(&f.release, &f.parent(), &manifest, &c).unwrap();
    let root = snapshot.root().to_path_buf();
    fs::write(root.join("foreign"), b"keep").unwrap();
    assert!(snapshot.cleanup(&c).is_err());
    assert!(root.join("release.json").exists());
    assert_eq!(fs::read(root.join("foreign")).unwrap(), b"keep");
}
#[test]
fn formal_repair_lock_names_contend_with_existing_delivery_guards() {
    let f = Fixture::new();
    let target = deploy::destination(&f.target()).unwrap();
    let canonical = target
        .to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .to_ascii_lowercase();
    let scope = f.parent().join(".lycheedev-locks");
    let maintenance =
        lycheedev_core::coordination::InstallationMaintenanceLease::acquire(&target).unwrap();
    assert!(
        matches!(ExclusiveLock::resource_controlled(&scope,&format!("installation-maintenance:{canonical}"),&OperationControl::default()),Err(e) if e.code=="workspace.busy")
    );
    maintenance.close().unwrap();
    let installation = deploy::installation_lock_controlled(&target, &Default::default()).unwrap();
    assert!(
        matches!(ExclusiveLock::resource_controlled(&scope,&format!("installation:{canonical}"),&OperationControl::default()),Err(e) if e.code=="workspace.busy")
    );
    installation.close().unwrap();
}
#[test]
fn formal_repair_cleanup_joins_primary_close_and_original_cancel() {
    let c = OperationControl::default();
    c.tracked_cancel_handle().unwrap().cancel();
    let primary = ToolkitError::new("delivery.installation_conflict", "changed", 4)
        .with_details(json!({"evidence":"keep"}));
    let e = finish::<()>(
        Err(primary),
        [Err(ToolkitError::io(std::io::Error::other("close failed")))],
        &c,
    )
    .unwrap_err();
    assert_eq!(e.code, "delivery.installation_conflict");
    assert_eq!(e.details["causes"].as_array().unwrap().len(), 3);
    assert_eq!(e.details["causes"][0]["details"]["evidence"], "keep");
}

#[test]
fn formal_repair_pending_old_slot_refuses_main_upgrade_without_replay() {
    let f = Fixture::new();
    f.old();
    let c = OperationControl::default();
    {
        let phase =
            crate::slot_test_diagnostics::SlotPhase::begin("repair.fixture_install", Some(&c));
        let result = slots::install_controlled(&f.parent(), "3.0.0", &c);
        phase.finish(result.as_ref().err());
        result.unwrap();
    }
    let mut pool = slots::read_pool_controlled(&f.parent(), &c).unwrap();
    pool.files[0].nonce = "original-unknown".into();
    pool.files[0].consumer = "original-consumer".into();
    slots::save_pool(&f.parent(), &pool, &c).unwrap();
    let before = fs::read(f.parent().join(slots::MARKER)).unwrap();
    let archive = f.root.join("archive");
    assert!(f.run(Some(&archive)).is_err());
    assert_eq!(fs::read(f.parent().join(slots::MARKER)).unwrap(), before);
    assert!(!archive.exists());
    assert_eq!(fs::read(f.target().join("old.lua")).unwrap(), b"old\n");
}
#[test]
fn formal_repair_cleanup_refuses_foreign_empty_directory() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let manifest = release::inspect_with_control(&f.release, lycheedev_core::VERSION, &c).unwrap();
    let snapshot = snapshot::Snapshot::prepare(&f.release, &f.parent(), &manifest, &c).unwrap();
    let root = snapshot.root().to_path_buf();
    fs::create_dir(root.join("foreign-empty")).unwrap();
    assert!(snapshot.cleanup(&c).is_err());
    assert!(root.join("release.json").exists());
    assert!(root.join("foreign-empty").is_dir());
}

#[test]
fn formal_repair_reordered_release_resources_are_fresh_retry_idempotent() {
    let f = Fixture::new();
    f.run(None).unwrap();
    let before = fs::read(f.target().join(installation::MARKER)).unwrap();
    let mut manifest: release::Release =
        serde_json::from_slice(&fs::read(f.release.join("release.json")).unwrap()).unwrap();
    manifest.resources.reverse();
    fs::write(
        f.release.join("release.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    f.run(None).unwrap();
    assert_eq!(
        fs::read(f.target().join(installation::MARKER)).unwrap(),
        before
    );
}
#[test]
fn formal_repair_same_receipt_is_order_insensitive_but_rejects_duplicates_and_identity_changes() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let manifest = release::inspect_with_control(&f.release, lycheedev_core::VERSION, &c).unwrap();
    let wanted = release::receipt(&manifest, "addon");
    let mut reordered = wanted.clone();
    reordered.resources.reverse();
    assert!(same_receipt(&wanted, &reordered).unwrap());
    require_receipt(
        &json!({"state":"managed","receipt":reordered}),
        &wanted,
        "test",
    )
    .unwrap();
    let mut duplicate = wanted.clone();
    duplicate.resources.push(wanted.resources[0].clone());
    assert!(same_receipt(&wanted, &duplicate).is_err());
    assert!(same_receipt(&duplicate, &duplicate).is_err());
    for field in [
        "schema",
        "component",
        "version",
        "commit",
        "digest",
        "bytes",
        "path",
    ] {
        let mut changed = wanted.clone();
        match field {
            "schema" => changed.schema.push('x'),
            "component" => changed.component.push('x'),
            "version" => changed.version.push('x'),
            "commit" => changed.commit = "b".repeat(40),
            "digest" => changed.resources[0].sha256 = "b".repeat(64),
            "bytes" => changed.resources[0].bytes += 1,
            _ => changed.resources[0].path.push('x'),
        };
        assert!(!same_receipt(&wanted, &changed).unwrap(), "{field}");
    }
}
#[test]
fn formal_repair_cleanup_blocks_admission_to_delete_regular_file_replacement() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let manifest = release::inspect_with_control(&f.release, lycheedev_core::VERSION, &c).unwrap();
    let snapshot = snapshot::Snapshot::prepare(&f.release, &f.parent(), &manifest, &c).unwrap();
    let root = snapshot.root().to_path_buf();
    let path = root.join("payload/skill/SKILL.md");
    snapshot
        .cleanup_with(&c, &mut |_, _| Ok(()), &mut || {
            assert!(fs::rename(&path, root.join("replacement-original")).is_err());
            assert!(fs::write(&path, b"foreign replacement").is_err());
            Ok(())
        })
        .unwrap();
    assert!(!root.exists());
}
#[test]
fn formal_repair_cleanup_cancel_halfway_closes_all_handles_without_deleting() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let manifest = release::inspect_with_control(&f.release, lycheedev_core::VERSION, &c).unwrap();
    let snapshot = snapshot::Snapshot::prepare(&f.release, &f.parent(), &manifest, &c).unwrap();
    let root = snapshot.root().to_path_buf();
    let mut admitted = vec![];
    let e = snapshot
        .cleanup_with(
            &c,
            &mut |path, count| {
                admitted.push(path.to_path_buf());
                if count >= 3 {
                    c.tracked_cancel_handle()?.cancel();
                }
                Ok(())
            },
            &mut || panic!("cancelled admission must not begin deletion"),
        )
        .unwrap_err();
    assert_eq!(e.code, "command.cancelled");
    assert!(root.join("release.json").exists());
    for path in admitted {
        if path.is_file() {
            let bytes = fs::read(&path).unwrap();
            fs::write(&path, &bytes).unwrap();
        }
    }
    let moved = root.with_extension("closed-handles");
    fs::rename(&root, &moved).unwrap();
    assert!(moved.join("release.json").exists());
}
#[test]
fn formal_repair_retained_preparation_capacity_preserves_owned_evidence() {
    let f = Fixture::new();
    let c = OperationControl::default();
    for _ in 0..4 {
        let e = apply(
            &f.client,
            &f.release,
            None,
            lycheedev_core::VERSION,
            &mut Reject,
            &c,
        )
        .unwrap_err();
        let root = Path::new(e.details["retainedPreparation"].as_str().unwrap());
        let owner: Value =
            serde_json::from_slice(&fs::read(root.join(snapshot::OWNER)).unwrap()).unwrap();
        assert_eq!(owner["schema"], "lycheedev.repair-preparation-owner.v1");
        assert_eq!(owner["maximumPayloadBytes"], 256u64 << 20);
    }
    let error = apply(
        &f.client,
        &f.release,
        None,
        lycheedev_core::VERSION,
        &mut Reject,
        &c,
    )
    .unwrap_err();
    assert_eq!(error.code, "delivery.preparation_capacity");
    assert_eq!(error.details["maximumRetainedRoots"], 4);
    assert!(!f.target().exists());
}

#[cfg(windows)]
#[test]
fn formal_repair_completed_receipt_survives_real_snapshot_deny_delete() {
    use std::os::windows::fs::OpenOptionsExt;
    struct Validator {
        deny: bool,
        held: Option<fs::File>,
    }
    impl AddonValidator for Validator {
        fn validate(
            &mut self,
            root: &Path,
            version: &str,
            control: &OperationControl,
        ) -> Result<()> {
            FixtureValidator.validate(root, version, control)?;
            if self.deny {
                // Read remains legal throughout verification. Only cleanup's
                // DELETE acquisition is denied on this unmoved snapshot file.
                self.held = Some(
                    fs::OpenOptions::new()
                        .read(true)
                        .share_mode(1)
                        .open(root.join("payload/skill/SKILL.md"))?,
                );
            }
            Ok(())
        }
    }
    for deny in [false, true] {
        let f = Fixture::new();
        let control = OperationControl::default();
        let mut validator = Validator { deny, held: None };
        let result = apply(
            &f.client,
            &f.release,
            None,
            lycheedev_core::VERSION,
            &mut validator,
            &control,
        );
        if let Some(file) = validator.held.take() {
            storage::finish_file(file, Ok(()), &control).unwrap();
        }
        let installed =
            installation::inspect_main_controlled(&f.target(), "addon", &control).unwrap();
        assert_eq!(installed["state"], "managed");
        let pool =
            slots::inspect_controlled(&f.parent(), lycheedev_core::VERSION, &control).unwrap();
        assert_eq!(
            pool["files"].as_array().unwrap().len(),
            slots::COUNT as usize
        );
        if deny {
            let error = result.unwrap_err();
            assert_eq!(error.code, "external.io");
            assert_eq!(error.exit_code, 5);
            assert_eq!(
                error.details["operationCompleted"], true,
                "main addon and all slots were verified before snapshot cleanup failed"
            );
            assert_eq!(
                error.details["result"], installed["receipt"],
                "the complete original Receipt remains available alongside cleanup failure"
            );
        } else {
            assert_eq!(
                serde_json::to_value(result.unwrap()).unwrap(),
                installed["receipt"]
            );
        }
    }
}
