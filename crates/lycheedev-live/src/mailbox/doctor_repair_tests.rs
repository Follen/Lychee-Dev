//! Public repair service + real project reset reference, with an explicit
//! Delivery seam. These tests never install an addon or open a game process.
use super::*;
use crate::mailbox::{
    SLOT_COUNT,
    connection::doctor_repair::{DoctorFixOptions, RepairDelivery},
    doctor::{DoctorDeployment, DoctorResult, SlotAssessment},
};

#[derive(Default)]
struct ResetDelivery {
    resets: Vec<(PathBuf, String)>,
    upgrades: usize,
    upgrade_archive: Option<PathBuf>,
    interrupt: bool,
    cancel_after_complete: bool,
}
impl RepairDelivery for ResetDelivery {
    fn reset_slots(
        &mut self,
        target: &ConnectionTarget,
        archive: &Path,
        version: &str,
        control: &OperationControl,
    ) -> ChannelResult<bool> {
        assert!(
            !inspect_window_owner(&target.addon_parent(), &target.resource(), control).occupied
        );
        self.resets.push((archive.to_path_buf(), version.into()));
        if self.interrupt {
            Err(error("fixture.reset_interrupted"))
        } else {
            if self.cancel_after_complete {
                control.tracked_cancel_handle().unwrap().cancel();
            }
            Ok(true)
        }
    }
    fn apply(
        &mut self,
        _: &ConnectionTarget,
        _: &DoctorDeployment,
        _: &Path,
        archive: Option<&Path>,
        _: &OperationControl,
    ) -> ChannelResult<()> {
        self.upgrades += 1;
        self.upgrade_archive = archive.map(Path::to_path_buf);
        Err(error("fixture.upgrade_selected"))
    }
}
fn managed_report(f: &Fixture, state: &str) -> DoctorResult {
    DoctorResult {
        target: Some(f.target.clone()),
        deployment: Some(DoctorDeployment {
            state: "managed".into(),
            version: "3.2.0".into(),
            slots: Some(SlotAssessment {
                state: state.into(),
                count: SLOT_COUNT,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}
fn pending_reset(f: &Fixture) -> Vec<PathBuf> {
    let directory = f.root.join(".lycheedev/live/repairs");
    fs::read_dir(directory)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".pending-reset.json")
                .then_some(path)
        })
        .collect()
}

#[test]
fn doctor_slot_reset_recovers_default_archive_and_rejects_changed_output_without_release() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let mut service = service(&f, &counts, &clock);
    let report = managed_report(&f, "incomplete");
    let mut delivery = ResetDelivery {
        cancel_after_complete: true,
        ..Default::default()
    };
    let result = service.doctor_fix(
        &report,
        &DoctorFixOptions::default(),
        &mut delivery,
        &OperationControl::default(),
    );
    assert!(result.error().is_some());
    assert_eq!(delivery.resets.len(), 1);
    assert_eq!(delivery.resets[0].1, "3.2.0");
    assert_eq!(delivery.upgrades, 0);
    assert_eq!(counts.borrow().inputs, 0);
    assert_eq!(pending_reset(&f).len(), 1);
    let changed = f.root.join("different");
    assert!(
        service
            .doctor_fix(
                &report,
                &DoctorFixOptions {
                    output: Some(&changed),
                    ..Default::default()
                },
                &mut delivery,
                &OperationControl::default()
            )
            .error()
            .unwrap()
            .to_string()
            .contains("--output differs")
    );
    assert_eq!(delivery.resets.len(), 1);
    let mut changed_version = managed_report(&f, "managed");
    changed_version.deployment.as_mut().unwrap().version = "changed-installed".into();
    assert!(
        service
            .doctor_fix(
                &changed_version,
                &DoctorFixOptions::default(),
                &mut delivery,
                &OperationControl::default()
            )
            .error()
            .unwrap()
            .to_string()
            .contains("version")
    );
    assert_eq!(delivery.resets.len(), 1);
    clock.set(4000);
    delivery.cancel_after_complete = false;
    service = super::service(&f, &counts, &clock);
    // The healthy read-only pool assessment cannot hide a durable unfinished
    // reset. Complete exactly its original archive before activation.
    let resumed = service.doctor_fix(
        &managed_report(&f, "managed"),
        &DoctorFixOptions::default(),
        &mut delivery,
        &OperationControl::default(),
    );
    assert_eq!(delivery.resets.len(), 2);
    assert_eq!(delivery.resets[0], delivery.resets[1]);
    assert_eq!(delivery.upgrades, 0);
    assert!(pending_reset(&f).is_empty());
    assert_eq!(
        resumed.into_report().unwrap().stage,
        "activation_waiting_manual_reload"
    );
    assert_eq!(counts.borrow().inputs, 0);
}

#[test]
fn doctor_hard_reset_preserves_current_release_and_old_non_slot_addon_takes_upgrade() {
    for has_slots in [false, true] {
        let f = Fixture::new();
        let counts = Rc::new(RefCell::new(Counts::default()));
        let clock = Rc::new(Cell::new(1000));
        let mut service = service(&f, &counts, &clock);
        let mut report = managed_report(&f, "managed");
        if !has_slots {
            report.deployment.as_mut().unwrap().slots = None;
        }
        let mut delivery = ResetDelivery {
            interrupt: true,
            ..Default::default()
        };
        let result = service.doctor_fix(
            &report,
            &DoctorFixOptions {
                hard_reset: true,
                bundled_release: Some(Path::new("bundled")),
                ..Default::default()
            },
            &mut delivery,
            &OperationControl::default(),
        );
        assert!(result.error().is_some());
        assert_eq!(delivery.resets.len(), usize::from(has_slots));
        assert_eq!(delivery.upgrades, usize::from(!has_slots));
        assert_eq!(counts.borrow().inputs, 0);
    }
}

#[test]
fn doctor_reset_error_report_keeps_selected_archive_and_does_not_activate() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let mut service = service(&f, &counts, &clock);
    let mut delivery = ResetDelivery {
        interrupt: true,
        ..Default::default()
    };
    let (report, result) = service.doctor_fix_report(
        managed_report(&f, "incomplete"),
        &DoctorFixOptions::default(),
        &mut delivery,
        &OperationControl::default(),
    );
    assert!(result.error().is_some());
    assert!(!report.healthy);
    assert!(!report.repair_archive.is_empty());
    assert_eq!(
        format!("{}.slots", report.repair_archive),
        delivery.resets[0].0.to_string_lossy()
    );
    assert_eq!(counts.borrow().opens, 0);
    assert_eq!(counts.borrow().inputs, 0);
}

#[test]
fn doctor_resets_old_pool_at_installed_version_before_managed_upgrade() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let mut service = service(&f, &counts, &clock);
    let mut report = managed_report(&f, "incomplete");
    report.deployment.as_mut().unwrap().version = "old-installed".into();
    let mut delivery = ResetDelivery::default();
    let outcome = service.doctor_fix(
        &report,
        &DoctorFixOptions {
            bundled_release: Some(Path::new("bundled")),
            ..Default::default()
        },
        &mut delivery,
        &OperationControl::default(),
    );
    assert!(
        outcome
            .error()
            .unwrap()
            .to_string()
            .contains("fixture.upgrade_selected")
    );
    assert_eq!(delivery.resets.len(), 1);
    assert_eq!(delivery.resets[0].1, "old-installed");
    assert_eq!(delivery.upgrades, 1);
    assert_eq!(
        format!(
            "{}.slots",
            delivery.upgrade_archive.unwrap().to_string_lossy()
        ),
        delivery.resets[0].0.to_string_lossy()
    );
    assert!(pending_reset(&f).is_empty());
    assert_eq!(counts.borrow().opens, 0);
}

#[test]
fn managed_doctor_slot_reset_rechecks_other_process_ownership_before_delivery() {
    use crate::mailbox::connection::doctor_delivery::ManagedDoctorDelivery;
    struct NeverValidator;
    impl lycheedev_delivery::formal_repair::AddonValidator for NeverValidator {
        fn validate(
            &mut self,
            _: &Path,
            _: &str,
            _: &OperationControl,
        ) -> lycheedev_core::Result<()> {
            panic!("shared installation occupancy must stop before Delivery validation")
        }
    }
    let f = Fixture::new();
    let mut other = f.target.clone();
    other.window.process_id += 1;
    other.window.process_started_at += 1;
    other.window.handle += 1;
    let control = OperationControl::default();
    let mut book = ConnectionBook::open(&f.root).unwrap();
    let record = book
        .create(
            other.clone(),
            identity('4'),
            &FakeClock(Rc::new(Cell::new(1000))),
            &mut Tokens(900),
            &control,
        )
        .unwrap();
    let mut delivery = ManagedDoctorDelivery::new("3.2.0", NeverValidator).unwrap();
    let archive = f.root.join("must-remain-absent.slots");
    assert!(
        delivery
            .reset_slots(&f.target, &archive, "3.2.0", &control)
            .is_err()
    );
    assert!(!archive.exists());
    assert_eq!(
        inspect_window_owner(&other.addon_parent(), &other.resource(), &control).owner,
        Some(record.metadata.owner)
    );
}
