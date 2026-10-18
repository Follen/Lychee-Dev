use super::*;
use lycheedev_core::{
    control::TrackedCancellation,
    storage,
    workspace::{
        self,
        data_resources::{self, Policy},
    },
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Duration,
};
const MIB: u64 = 1 << 20;

struct Probe {
    bytes: AtomicU64,
    calls: AtomicU64,
    fail: AtomicBool,
}
impl Probe {
    fn new(bytes: u64) -> Arc<Self> {
        Arc::new(Self {
            bytes: AtomicU64::new(bytes),
            calls: AtomicU64::new(0),
            fail: AtomicBool::new(false),
        })
    }
    fn observe(self: &Arc<Self>) -> MemoryProbe {
        let probe = self.clone();
        Arc::new(move || {
            probe.calls.fetch_add(1, Ordering::SeqCst);
            if probe.fail.load(Ordering::SeqCst) {
                return Err(fault("test.memory_status", 5, "memory status unavailable"));
            }
            Ok(probe.bytes.load(Ordering::SeqCst))
        })
    }
}
fn home(shares: usize) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/adaptive-data-budget-fixtures")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    workspace::initialize(&home).unwrap();
    let mut config = workspace::default_config();
    config["download"]["workers"] = serde_json::json!(shares);
    storage::atomic_json(&home.join("config.json"), &config).unwrap();
    home
}
struct Fixture {
    budget: Budget,
    probe: Arc<Probe>,
    admission: DataResources,
}
fn fixture(shares: usize, bytes: u64) -> Fixture {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(shares), Policy::Ordinary, &control).unwrap();
    let probe = Probe::new(bytes);
    let budget = Budget::query_with_probe(control.deadline(), control, probe.observe());
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    budget.initialize_admitted(&admission).unwrap();
    Fixture {
        budget,
        probe,
        admission,
    }
}
fn memory_cost(retained: u64, metadata: u64) -> Cost {
    Cost {
        retained_bytes: retained * MIB,
        metadata_bytes: metadata * MIB,
        ..Cost::default()
    }
}

// Go: TestAdaptiveMemoryHandlesLargeTablesAndDecoderScratch.
#[test]
fn large_tables_and_decoder_scratch_grow_only_on_demand() {
    let f = fixture(2, 8 << 30);
    let b = &f.budget;
    assert_eq!(
        serde_json::to_value(b.memory_snapshot().unwrap()).unwrap(),
        serde_json::json!({
            "mode":"windows_adaptive", "concurrentQueries":2, "ceilingBytes":2u64 << 30,
            "retainedBytes":512 * MIB, "metadataBytes":128 * MIB, "peakScratchBytes":0, "growths":0
        })
    );
    for (retained, metadata) in [(96, 96), (38, 38), (250, 151)] {
        b.charge(memory_cost(retained, metadata)).unwrap();
    }
    let report = b.memory_snapshot().unwrap();
    assert_eq!(
        (
            report.retained_bytes,
            report.metadata_bytes,
            report.ceiling_bytes,
            report.growths
        ),
        (512 * MIB, 320 * MIB, 2 << 30, 2)
    );
    let calls = f.probe.calls.load(Ordering::SeqCst);
    b.charge(Cost::work(1)).unwrap();
    assert_eq!(f.probe.calls.load(Ordering::SeqCst), calls);
    let scratch = b.reserve_logical_scratch(180 * MIB).unwrap();
    assert_eq!(
        b.remaining_retained().unwrap(),
        b.memory_snapshot().unwrap().retained_bytes - 564 * MIB
    );
    drop(scratch);
    b.charge(memory_cost(200, 160)).unwrap();
    let usage = b.logical_snapshot().unwrap();
    assert_eq!(
        (usage.retained_bytes, usage.metadata_bytes),
        (584 * MIB, 445 * MIB)
    );
    assert_eq!(b.memory_snapshot().unwrap().peak_scratch_bytes, 180 * MIB);
    // Fixed Go growMemory rounds 584 MiB to the next 64 MiB step (640),
    // after the prior 180 MiB scratch reservation has been released.
    let report = b.memory_snapshot().unwrap();
    assert_eq!(report.retained_bytes, 640 * MIB);
    assert_eq!(b.remaining_retained().unwrap(), 56 * MIB);
    assert_eq!(
        b.remaining_retained().unwrap(),
        report.retained_bytes - usage.retained_bytes
    );
    f.admission.close().unwrap();
}

// Go: TestAdaptiveMemoryPressureAndOtherDimensionsRejectAtomically.
#[test]
fn pressure_hard_dimensions_and_overflow_are_atomic() {
    let f = fixture(2, 4 << 30);
    let b = &f.budget;
    b.charge(memory_cost(496, 112)).unwrap();
    let before = (
        b.logical_snapshot().unwrap(),
        b.snapshot().unwrap(),
        b.memory_snapshot().unwrap(),
    );
    f.probe.bytes.store(32 * MIB, Ordering::SeqCst);
    for (cost, dimension, used, requested, limit) in [
        (
            memory_cost(32, 32),
            "retained_bytes",
            496 * MIB,
            32 * MIB,
            504 * MIB,
        ),
        (
            memory_cost(0, 32),
            "metadata_bytes",
            112 * MIB,
            32 * MIB,
            120 * MIB,
        ),
        (
            Cost {
                retained_bytes: u64::MAX,
                ..Cost::default()
            },
            "retained_bytes",
            496 * MIB,
            u64::MAX,
            1024 * MIB,
        ),
        (
            Cost {
                metadata_bytes: u64::MAX,
                ..Cost::default()
            },
            "metadata_bytes",
            112 * MIB,
            u64::MAX,
            1024 * MIB,
        ),
    ] {
        let error = b.charge(cost).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("records.query_resource_budget", 3)
        );
        assert_eq!(
            error.message,
            format!(
                "records.query_resource_budget: {dimension} (used={used} requested={requested} limit={limit})"
            )
        );
        assert_eq!(
            (
                b.logical_snapshot().unwrap(),
                b.snapshot().unwrap(),
                b.memory_snapshot().unwrap()
            ),
            before
        );
    }
    let error = b.reserve_logical_scratch(32 * MIB).err().unwrap();
    assert_eq!(
        error.message,
        "records.query_resource_budget: retained_bytes (used=520093696 requested=33554432 limit=528482304)"
    );
    assert_eq!(b.memory_snapshot().unwrap(), before.2);
    f.probe.bytes.store(4 << 30, Ordering::SeqCst);
    for cost in [
        Cost {
            metadata_bytes: 32 * MIB,
            decode_work: Cost::defaults().decode_work + 1,
            ..Cost::default()
        },
        Cost {
            metadata_bytes: 32 * MIB,
            network_bytes: Cost::defaults().network_bytes + 1,
            ..Cost::default()
        },
        Cost {
            metadata_bytes: 32 * MIB,
            network_requests: Cost::defaults().network_requests + 1,
            ..Cost::default()
        },
    ] {
        let calls = f.probe.calls.load(Ordering::SeqCst);
        assert!(b.charge(cost).is_err());
        assert_eq!(f.probe.calls.load(Ordering::SeqCst), calls);
        assert_eq!(
            (
                b.logical_snapshot().unwrap(),
                b.snapshot().unwrap(),
                b.memory_snapshot().unwrap()
            ),
            before
        );
    }
    b.charge(memory_cost(32, 32)).unwrap();
    f.admission.close().unwrap();
}

// Go: TestAdaptiveMemoryDividesConcurrentCapacityAndBoundsLowMemory.
#[test]
fn capacity_low_memory_and_zero_have_no_fixed_floor() {
    let first = fixture(2, 512 * MIB);
    let second = fixture(2, 512 * MIB);
    for f in [&first, &second] {
        assert_eq!(
            f.budget.memory_snapshot().unwrap().retained_bytes,
            128 * MIB
        );
        f.budget.charge(memory_cost(128, 128)).unwrap();
        assert!(
            f.budget
                .charge(Cost {
                    metadata_bytes: 1,
                    ..Cost::default()
                })
                .is_err()
        );
    }
    assert_eq!(
        first.budget.snapshot().unwrap().retained_bytes
            + second.budget.snapshot().unwrap().retained_bytes,
        256 * MIB
    );
    let single = fixture(1, 512 * MIB);
    single.budget.charge(memory_cost(256, 256)).unwrap();
    assert_eq!(
        single.budget.memory_snapshot().unwrap().concurrent_queries,
        1
    );
    let empty = fixture(2, 0);
    assert!(empty.budget.reserve_logical_scratch(1).is_err());
    assert!(empty.budget.charge(Cost::retained(1)).is_err());
    let json = serde_json::to_value(empty.budget.memory_snapshot().unwrap()).unwrap();
    assert_eq!(json["concurrentQueries"], 2);
    assert!(json.get("ceilingBytes").is_none());
}

// Go: TestAdaptiveMemoryConcurrentGrowthAndProbeFailure.
#[test]
fn concurrent_growth_and_failed_probe_preserve_both_ledgers() {
    let f = fixture(2, 4 << 30);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| f.budget.charge(memory_cost(80, 80)).unwrap());
        }
    });
    assert_eq!(f.budget.snapshot().unwrap().retained_bytes, 640 * MIB);
    assert_eq!(
        f.budget.logical_snapshot().unwrap().metadata_bytes,
        640 * MIB
    );
    let before = (
        f.budget.snapshot().unwrap(),
        f.budget.memory_snapshot().unwrap(),
    );
    f.probe.fail.store(true, Ordering::SeqCst);
    let error = f.budget.charge(memory_cost(80, 0)).unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(error.details["cause"]["code"], "test.memory_status");
    assert_eq!(
        (
            f.budget.snapshot().unwrap(),
            f.budget.memory_snapshot().unwrap()
        ),
        before
    );
}

#[test]
fn public_report_has_exact_fixed_fields_and_shared_peak() {
    let b = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    );
    let scratch = b.reserve_logical_scratch(19).unwrap();
    let other = b.safety_only();
    assert_eq!(
        serde_json::to_value(other.memory_snapshot().unwrap()).unwrap(),
        serde_json::json!({
            "mode":"fixed", "retainedBytes":512 * MIB, "metadataBytes":128 * MIB, "peakScratchBytes":19, "growths":0
        })
    );
    drop(scratch);
    assert_eq!(b.memory_snapshot().unwrap().peak_scratch_bytes, 19);
    assert_eq!(b.remaining_retained().unwrap(), 512 * MIB);
}

#[test]
fn safety_growth_never_inflates_go_report_and_uses_same_ceiling() {
    let f = fixture(2, 4 << 30);
    let b = &f.budget;
    let safety = b.safety_only();
    let before = b.memory_snapshot().unwrap();
    safety.charge(memory_cost(640, 160)).unwrap();
    assert_eq!(b.memory_snapshot().unwrap(), before);
    assert_eq!(b.logical_snapshot().unwrap(), Cost::default());
    let calls = f.probe.calls.load(Ordering::SeqCst);
    b.charge_split(memory_cost(640, 160), Cost::default())
        .unwrap();
    assert_eq!(f.probe.calls.load(Ordering::SeqCst), calls + 1);
    let report = b.memory_snapshot().unwrap();
    assert_eq!(
        (report.retained_bytes, report.metadata_bytes, report.growths),
        (640 * MIB, 192 * MIB, 1)
    );
    assert_eq!(safety.memory_snapshot().unwrap(), report);
    assert!(safety.charge(memory_cost(385, 0)).is_err());
    assert!(
        b.charge_split(memory_cost(385, 0), Cost::default())
            .is_err()
    );
    assert_eq!(b.memory_snapshot().unwrap(), report);
}

#[test]
fn safety_failure_does_not_publish_logical_growth() {
    let f = fixture(2, 4 << 30);
    let b = &f.budget;
    b.safety_only().charge(memory_cost(1000, 0)).unwrap();
    let before = b.memory_snapshot().unwrap();
    let calls = f.probe.calls.load(Ordering::SeqCst);
    assert!(
        b.charge_split(memory_cost(600, 150), memory_cost(25, 0))
            .is_err()
    );
    assert_eq!(b.memory_snapshot().unwrap(), before);
    assert_eq!(f.probe.calls.load(Ordering::SeqCst), calls);
    assert_eq!(b.logical_snapshot().unwrap(), Cost::default());
}

#[test]
fn rounding_clips_to_fresh_headroom_and_original_ceiling() {
    let f = fixture(2, 4 << 30);
    let b = &f.budget;
    b.charge(memory_cost(496, 0)).unwrap();
    f.probe.bytes.store(4 * 33 * MIB, Ordering::SeqCst);
    b.charge(memory_cost(32, 0)).unwrap();
    assert_eq!(b.memory_snapshot().unwrap().retained_bytes, 529 * MIB);
    f.probe.bytes.store(64 << 30, Ordering::SeqCst);
    b.charge(memory_cost(496, 0)).unwrap();
    assert_eq!(b.memory_snapshot().unwrap().retained_bytes, 1024 * MIB);
    assert!(b.charge(Cost::retained(1)).is_err());
    assert_eq!(b.memory_snapshot().unwrap().ceiling_bytes, 1024 * MIB);
}

#[test]
fn deferred_state_keeps_original_stop_and_explicit_budget_fixed() {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(2), Policy::Ordinary, &control).unwrap();
    let token = TrackedCancellation::new();
    let original =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let probe = Probe::new(4 << 30);
    let b = Budget::query_with_probe(original.deadline(), original, probe.observe());
    assert_eq!(
        b.charge(Cost::work(1)).unwrap_err().code,
        "records.internal"
    );
    assert!(b.memory_snapshot().is_err());
    token.cancel();
    assert_eq!(
        b.initialize_admitted(&admission).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    let fixed = Budget::controlled(
        Cost {
            retained_bytes: 73,
            ..Cost::defaults()
        },
        control.deadline(),
        control,
    );
    fixed.initialize_admitted(&admission).unwrap();
    assert_eq!(fixed.memory_snapshot().unwrap().retained_bytes, 73);
    assert_eq!(fixed.memory_snapshot().unwrap().mode, "fixed");
}

#[test]
fn initialized_views_never_resample_or_reset_original_ceiling() {
    let f = fixture(2, 4 << 30);
    let view = f.budget.safety_only();
    f.probe.bytes.store(16 << 30, Ordering::SeqCst);
    view.initialize_admitted(&f.admission).unwrap();
    f.budget.initialize_admitted(&f.admission).unwrap();
    assert_eq!(f.probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(view.memory_snapshot().unwrap().ceiling_bytes, 1 << 30);
}

#[test]
fn deferred_safety_charges_survive_activation_and_low_ceiling_rejects() {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(2), Policy::Ordinary, &control).unwrap();
    let probe = Probe::new(128 * MIB);
    let b = Budget::query_with_probe(control.deadline(), control.clone(), probe.observe());
    b.safety_only().charge(memory_cost(64, 16)).unwrap();
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        b.initialize_admitted(&admission).unwrap_err().code,
        "records.query_resource_budget"
    );
    assert_eq!(b.snapshot().unwrap().retained_bytes, 64 * MIB);
    assert_eq!(b.logical_snapshot().unwrap(), Cost::default());
    assert!(b.memory_snapshot().is_err());
    probe.bytes.store(4 << 30, Ordering::SeqCst);
    b.initialize_admitted(&admission).unwrap();
    assert_eq!(b.snapshot().unwrap().retained_bytes, 64 * MIB);
    assert_eq!(b.memory_snapshot().unwrap().growths, 0);
}

#[cfg(windows)]
#[test]
fn native_query_activates_after_real_admission() {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(2), Policy::Ordinary, &control).unwrap();
    let b = Budget::query_controlled(control.deadline(), control);
    b.initialize_admitted(&admission).unwrap();
    let report = b.memory_snapshot().unwrap();
    assert_eq!(
        (report.mode, report.concurrent_queries),
        ("windows_adaptive", 2)
    );
    assert!(report.ceiling_bytes > 0);
    assert!(report.retained_bytes <= report.ceiling_bytes);
}

#[cfg(not(windows))]
#[test]
fn development_platform_preserves_fixed_defaults_after_real_admission() {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(2), Policy::Ordinary, &control).unwrap();
    let b = Budget::query_controlled(control.deadline(), control);
    b.initialize_admitted(&admission).unwrap();
    let report = b.memory_snapshot().unwrap();
    assert_eq!(
        (report.mode, report.retained_bytes, report.metadata_bytes),
        ("fixed", 512 * MIB, 128 * MIB)
    );
}

#[test]
fn initial_probe_failure_and_unsigned_overflow_never_activate() {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(2), Policy::Ordinary, &control).unwrap();
    for initial in [0, u64::MAX] {
        let probe = Probe::new(initial);
        if initial == 0 {
            probe.fail.store(true, Ordering::SeqCst);
        }
        let b = Budget::query_with_probe(control.deadline(), control.clone(), probe.observe());
        let error = b.initialize_admitted(&admission).unwrap_err();
        assert_eq!(error.code, "records.query_resource_budget");
        if initial == 0 {
            assert_eq!(error.details["cause"]["code"], "test.memory_status");
        }
        assert_eq!(b.snapshot().unwrap(), Cost::default());
        assert_eq!(b.logical_snapshot().unwrap(), Cost::default());
        assert!(b.memory_snapshot().is_err());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn live_scratch_peak_is_shared_and_each_guard_releases_only_itself() {
    let f = fixture(2, 4 << 30);
    f.budget.charge(memory_cost(496, 0)).unwrap();
    let first = f.budget.reserve_logical_scratch(20 * MIB).unwrap();
    let second = f.budget.reserve_logical_scratch(30 * MIB).unwrap();
    assert_eq!(
        f.budget.memory_snapshot().unwrap().peak_scratch_bytes,
        50 * MIB
    );
    drop(first);
    assert_eq!(
        f.budget.remaining_retained().unwrap(),
        576 * MIB - 526 * MIB
    );
    drop(second);
    assert_eq!(
        f.budget.remaining_retained().unwrap(),
        576 * MIB - 496 * MIB
    );
    assert_eq!(
        f.budget.memory_snapshot().unwrap().peak_scratch_bytes,
        50 * MIB
    );
}

fn accounting_state(budget: &Budget) -> (Cost, Cost, Cost, MemoryBudget) {
    let safety_limits = budget.state.lock().unwrap().safety_limits;
    (
        budget.logical_snapshot().unwrap(),
        budget.snapshot().unwrap(),
        safety_limits,
        budget.memory_snapshot().unwrap(),
    )
}
fn growth_attempt(budget: &Budget, kind: u8) -> Result<()> {
    match kind {
        0 => budget.charge(memory_cost(600, 0)),
        1 => budget.reserve_logical_scratch(600 * MIB).map(drop),
        2 => budget.safety_only().charge(memory_cost(600, 0)),
        _ => unreachable!(),
    }
}

#[test]
fn growth_probe_tracked_cancellation_rejects_all_future_allocation_paths_atomically() {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(2), Policy::Ordinary, &control).unwrap();
    for kind in 0..3 {
        let token = TrackedCancellation::new();
        let original = OperationControl::tracked(control.deadline(), token.clone());
        let calls = Arc::new(AtomicU64::new(0));
        let b = Budget::query_with_probe(original.deadline(), original, {
            let calls = calls.clone();
            Arc::new(move || {
                if calls.fetch_add(1, Ordering::SeqCst) != 0 {
                    token.cancel();
                }
                Ok(4 << 30)
            })
        });
        b.initialize_admitted(&admission).unwrap();
        let before = accounting_state(&b);
        assert_eq!(
            growth_attempt(&b, kind).unwrap_err().code,
            "command.cancelled"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(accounting_state(&b), before);
    }
}

#[test]
fn growth_probe_independent_deadline_rejects_all_future_allocation_paths_atomically() {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(2), Policy::Ordinary, &control).unwrap();
    for kind in 0..3 {
        // The only deliberately slow boundary is the injected memory provider;
        // original invocation control remains live while the Budget expires.
        let deadline = Instant::now() + Duration::from_millis(500);
        let calls = Arc::new(AtomicU64::new(0));
        let b = Budget::query_with_probe(deadline, control.clone(), {
            let calls = calls.clone();
            Arc::new(move || {
                if calls.fetch_add(1, Ordering::SeqCst) != 0 {
                    std::thread::sleep(
                        deadline.saturating_duration_since(Instant::now())
                            + Duration::from_millis(1),
                    );
                }
                Ok(4 << 30)
            })
        });
        b.initialize_admitted(&admission).unwrap();
        let before = accounting_state(&b);
        assert_eq!(
            growth_attempt(&b, kind).unwrap_err().code,
            "records.query_resource_budget"
        );
        control.check().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(accounting_state(&b), before);
    }
}

#[test]
fn cancelled_growth_does_not_extend_or_refund_existing_retained_owner() {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(2), Policy::Ordinary, &control).unwrap();
    let token = TrackedCancellation::new();
    let original = OperationControl::tracked(control.deadline(), token.clone());
    let calls = Arc::new(AtomicU64::new(0));
    let b = Budget::query_with_probe(original.deadline(), original, {
        let calls = calls.clone();
        Arc::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) != 0 {
                token.cancel();
            }
            Ok(4 << 30)
        })
    });
    b.initialize_admitted(&admission).unwrap();
    let mut owner = b.reserve_retained(496 << 20).unwrap();
    let before = accounting_state(&b);
    assert_eq!(owner.grow(32 << 20).unwrap_err().code, "command.cancelled");
    assert_eq!(accounting_state(&b), before);
    drop(owner);
    assert_eq!(b.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(b.memory_snapshot().unwrap(), before.3);
}

#[test]
fn observed_network_after_cancelled_growth_remains_recorded_without_another_probe() {
    let control = OperationControl::default();
    let admission = data_resources::acquire(&home(2), Policy::Ordinary, &control).unwrap();
    let token = TrackedCancellation::new();
    let original = OperationControl::tracked(control.deadline(), token.clone());
    let calls = Arc::new(AtomicU64::new(0));
    let b = Budget::query_with_probe(original.deadline(), original, {
        let calls = calls.clone();
        Arc::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) != 0 {
                token.cancel();
            }
            Ok(4 << 30)
        })
    });
    b.initialize_admitted(&admission).unwrap();
    let before = b.memory_snapshot().unwrap();
    assert_eq!(
        b.charge(memory_cost(600, 0)).unwrap_err().code,
        "command.cancelled"
    );
    b.charge_observed_network_bytes(3).unwrap();
    b.safety_only().charge_observed_network_bytes(2).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(b.logical_snapshot().unwrap().network_bytes, 3);
    assert_eq!(b.snapshot().unwrap().network_bytes, 5);
    assert_eq!(b.memory_snapshot().unwrap(), before);
    assert_eq!(
        b.charge(Cost {
            network_bytes: 1,
            ..Cost::default()
        })
        .unwrap_err()
        .code,
        "command.cancelled"
    );
}
