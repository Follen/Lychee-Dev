use super::*;
use crate::{
    mailbox::{
        InputDiagnostic,
        admission::{ClientRecord, WindowRecord},
        checksum,
        recovery::test_support::identity,
    },
    mailbox::lookup::Record,
};
use std::collections::VecDeque;
fn record(kind: Kind, runtime: &str, payload: &impl Serialize) -> Record {
    let payload = serde_json::to_vec(payload).unwrap();
    Record {
        address: 100,
        nonce: if kind == Kind::Input {
            runtime.into()
        } else {
            "0".repeat(32)
        },
        runtime: runtime.into(),
        ticket: "0".repeat(32),
        kind: kind as u8,
        state: 1,
        sequence: 1,
        checksum: checksum(&payload),
        payload,
    }
}
fn input(i: &Identity, sample: i64) -> InputObservation {
    InputObservation {
        bindings: Some(InputBindings {
            wake: "ALT-CTRL-F12".into(),
            reload: "ALT-CTRL-F11".into(),
            ..Default::default()
        }),
        schema: i.input_state.clone(),
        runtime: i.runtime.clone(),
        owner: i.owner.clone(),
        fence: i.fence,
        next_slot: i.next_slot,
        guid: i.guid.clone(),
        build: i.build.clone(),
        sample_millis: sample,
        input_blocked: Some(false),
        ..Default::default()
    }
}
struct Reader {
    identity: Vec<Record>,
    boot: Vec<Record>,
    inputs: VecDeque<Vec<Record>>,
    reads: Vec<Kind>,
    closes: usize,
    close_error: bool,
}
impl Reader {
    fn new() -> Self {
        let mut i = identity('2');
        i.inventory = Some(SLOT_COUNT);
        Self {
            identity: vec![record(Kind::Identity, &i.runtime, &i)],
            boot: vec![record(
                Kind::Bootstrap,
                &"3".repeat(32),
                &BootstrapState {
                    schema: SCHEMA.into(),
                    bootstrap: "3".repeat(32),
                    release: "3.2.0".into(),
                    phase: "ready".into(),
                    ..Default::default()
                },
            )],
            inputs: [vec![record(Kind::Input, &i.runtime, &input(&i, 1000))]].into(),
            reads: vec![],
            closes: 0,
            close_error: false,
        }
    }
}
impl RecordReader for Reader {
    fn find(
        &mut self,
        c: &OperationControl,
        s: &Selector,
        _: Option<&mut dyn FnMut(&Record) -> bool>,
        _: bool,
    ) -> ChannelResult<Vec<Record>> {
        check_control(c)?;
        self.reads.push(s.kind);
        match s.kind {
            Kind::Identity => Ok(self.identity.clone()),
            Kind::Bootstrap => Ok(self.boot.clone()),
            Kind::Input => {
                if self.inputs.len() > 1 {
                    Ok(self.inputs.pop_front().unwrap())
                } else {
                    Ok(self.inputs.front().cloned().unwrap_or_default())
                }
            }
            _ => panic!("doctor must not read business receipt/body or claim execution"),
        }
    }
}
impl DoctorRuntime for Reader {
    fn uptime_millis(&self) -> i64 {
        1000
    }
    fn checked_close(&mut self) -> ChannelResult<()> {
        self.closes += 1;
        if self.close_error {
            Err(error("fixture.close_failed"))
        } else {
            Ok(())
        }
    }
}
#[derive(Default)]
struct Schedule {
    millis: u64,
    waits: usize,
}
impl ContinuationScheduler for Schedule {
    fn monotonic_millis(&self) -> u64 {
        self.millis
    }
    fn wait(&mut self, c: &OperationControl, duration: Duration) -> ChannelResult<()> {
        check_control(c)?;
        self.millis += duration.as_millis() as u64;
        self.waits += 1;
        Ok(())
    }
}
fn target() -> ConnectionTarget {
    ConnectionTarget {
        client: ClientRecord {
            full_build: "b".into(),
            product: "retail".into(),
            ..Default::default()
        },
        window: WindowRecord::default(),
    }
}
fn run(reader: &mut Reader, schedule: &mut Schedule) -> DoctorResult {
    let mut r = DoctorResult::default();
    inspect_runtime(
        &mut r,
        reader,
        &target(),
        "3.2.0",
        "",
        "",
        &OperationControl::default(),
        schedule,
    );
    r
}
#[test]
fn doctor_reads_bootstrap_identity_input_only_and_checked_closes() {
    let mut reader = Reader::new();
    let r = run(&mut reader, &mut Schedule::default());
    assert!(r.healthy, "{:?}", r.checks);
    assert!(r.bootstrap.is_some());
    assert!(r.input.is_some());
    assert_eq!(reader.reads, [Kind::Bootstrap, Kind::Identity, Kind::Input]);
    assert_eq!(reader.closes, 1);
    assert!(
        r.checks.iter().any(|c| c.id == "live.inventory"
            && c.detail.contains("enabled/loadable state is not checked"))
    );
}
#[test]
fn doctor_bootstrap_crosschecks_header_schema_release_and_never_authorizes_input() {
    for field in ["schema", "bootstrap", "release"] {
        let mut reader = Reader::new();
        let mut boot: serde_json::Value = serde_json::from_slice(&reader.boot[0].payload).unwrap();
        boot[field] = "wrong".into();
        reader.boot = vec![record(Kind::Bootstrap, &"3".repeat(32), &boot)];
        reader.identity.clear();
        let r = run(&mut reader, &mut Schedule::default());
        assert!(!r.healthy);
        assert!(r.bootstrap.is_none());
        assert!(r.input.is_none());
        assert!(
            r.checks
                .iter()
                .any(|c| c.id == "live.bootstrap" && c.state == "error")
        );
        assert_eq!(reader.closes, 1);
        assert!(!reader.reads.contains(&Kind::Input));
    }
}
#[test]
fn doctor_failed_bootstrap_remains_visible_with_empty_business_map() {
    let mut reader = Reader::new();
    reader.identity.clear();
    reader.boot = vec![record(
        Kind::Bootstrap,
        &"3".repeat(32),
        &BootstrapState {
            schema: SCHEMA.into(),
            bootstrap: "3".repeat(32),
            release: "3.2.0".into(),
            phase: "failed".into(),
            reason: "module_failed".into(),
            ..Default::default()
        },
    )];
    let r = run(&mut reader, &mut Schedule::default());
    assert!(!r.healthy);
    assert!(r.bootstrap.is_some());
    assert!(r.identity.is_none());
    assert!(
        r.checks
            .iter()
            .any(|c| c.id == "live.runtime" && c.detail == "no current runtime publication")
    );
    assert_eq!(reader.reads, [Kind::Bootstrap, Kind::Identity]);
}
#[test]
fn doctor_waits_for_fresh_input_for_at_most_1500ms() {
    let i = identity('2');
    let mut reader = Reader::new();
    reader.inputs = [vec![record(Kind::Input, &i.runtime, &input(&i, 100))]].into();
    let mut scheduler = Schedule::default();
    let r = run(&mut reader, &mut scheduler);
    assert!(!r.healthy);
    assert!(r.input.is_none());
    assert_eq!(scheduler.millis, 1500);
    assert_eq!(scheduler.waits, 60);
    assert_eq!(reader.closes, 1);
    assert!(
        r.checks
            .iter()
            .any(|c| c.id == "live.input" && c.state == "blocked")
    );
}
#[test]
fn doctor_accepts_new_sample_after_stale_without_effects() {
    let i = identity('2');
    let mut reader = Reader::new();
    reader
        .inputs
        .push_front(vec![record(Kind::Input, &i.runtime, &input(&i, 100))]);
    let mut scheduler = Schedule::default();
    let r = run(&mut reader, &mut scheduler);
    assert!(r.healthy);
    assert_eq!(scheduler.waits, 1);
    assert_eq!(r.input.unwrap().sample_millis, 1000);
}
#[test]
fn doctor_disabled_slot_failure_survives_ready_input_and_is_not_a_receipt() {
    for (stage, reason) in [
        ("load_failed", "slot_addon_disabled"),
        ("load_failed", "slot_load_failed"),
        ("no_dispatch", "slot_loader_no_dispatch"),
        ("exception", "slot_wake_failed"),
        ("rejected", "input_protection_busy"),
    ] {
        let i = identity('2');
        let mut sample = input(&i, 1000);
        sample.input_attempt = Some(InputDiagnostic {
            schema: SCHEMA.into(),
            runtime: i.runtime.clone(),
            attempt_seq: 1,
            slot: 7,
            stage: stage.into(),
            reason: reason.into(),
            received: false,
            sample_millis: 1000,
        });
        let original = sample.clone();
        let mut report = DoctorResult::default();
        report.inspect_input(sample);
        assert!(!report.healthy);
        let failure = report
            .checks
            .iter()
            .find(|c| c.id == "live.input_attempt")
            .unwrap();
        assert_eq!(failure.state, "error");
        assert!(failure.detail.contains(reason));
        assert!(failure.detail.contains("slot 7"));
        assert_eq!(report.input.unwrap(), original);
        if reason == "slot_addon_disabled" {
            assert!(failure.next_step.contains("do not replay submitted input"));
        }
    }
}
#[test]
fn doctor_nonfailure_diagnostic_does_not_override_readiness() {
    for stage in ["entered", "loading", "received"] {
        for blocked in [true, false] {
            let i = identity('2');
            let mut sample = input(&i, 1000);
            sample.input_blocked = Some(blocked);
            sample.reason = if blocked { "input_combat_lockdown" } else { "" }.into();
            sample.input_attempt = Some(InputDiagnostic {
                schema: SCHEMA.into(),
                runtime: i.runtime.clone(),
                attempt_seq: 1,
                slot: 7,
                stage: stage.into(),
                reason: String::new(),
                received: stage == "received",
                sample_millis: 1000,
            });
            let mut report = DoctorResult::default();
            report.inspect_input(sample);
            assert_eq!(report.healthy, !blocked);
            assert_eq!(report.checks.len(), 1);
        }
    }
}
#[test]
fn doctor_missing_inventory_requires_restart_not_more_reload_or_false_enabled_claim() {
    let mut reader = Reader::new();
    let mut i = identity('2');
    i.inventory = Some(200);
    reader.identity = vec![record(Kind::Identity, &i.runtime, &i)];
    let r = run(&mut reader, &mut Schedule::default());
    assert!(!r.healthy);
    assert!(r.checks.iter().any(|c| c.id == "live.inventory"
        && c.next_step == "restart the selected client once after installation"));
}
#[test]
fn doctor_preserves_close_failure_even_after_successful_reads() {
    let mut reader = Reader::new();
    reader.close_error = true;
    let r = run(&mut reader, &mut Schedule::default());
    assert!(!r.healthy);
    assert!(r.input.is_some());
    assert_eq!(reader.closes, 1);
    assert!(
        r.errors
            .iter()
            .any(|e| e.to_string().contains("fixture.close_failed"))
    );
}
#[test]
fn doctor_validates_mutating_flags_before_target_discovery() {
    use crate::mailbox::connection::{
        TargetRequest, doctor_service::validate_doctor_options,
    };
    for args in [("release", "", ""), ("", "archive", ""), ("", "", "repair")] {
        assert!(
            validate_doctor_options(&TargetRequest::default(), "", false, args.0, args.1, args.2)
                .is_err()
        );
    }
    assert!(
        validate_doctor_options(
            &TargetRequest {
                pid: 42,
                ..Default::default()
            },
            "CON-test",
            false,
            "",
            "",
            ""
        )
        .is_err()
    );
    assert!(
        validate_doctor_options(
            &TargetRequest::default(),
            "",
            true,
            "release",
            "archive",
            "repair"
        )
        .is_ok()
    );
}
