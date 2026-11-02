use super::*;
use std::{
    cell::RefCell,
    collections::BTreeSet,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

pub(super) struct Hooks {
    pub(super) reject_spawn_at: Option<usize>,
    before: Box<dyn Fn(usize) -> Result<()> + Send + Sync>,
}
thread_local! {
    static HOOKS: RefCell<Option<Arc<Hooks>>> = const { RefCell::new(None) };
}
pub(super) fn current_hooks() -> Option<Arc<Hooks>> {
    HOOKS.with(|hooks| hooks.borrow().clone())
}
pub(super) fn before_strip(hooks: Option<&Hooks>, row: usize) -> Result<()> {
    hooks.map_or(Ok(()), |hooks| (hooks.before)(row))
}
struct HookGuard;
impl Drop for HookGuard {
    fn drop(&mut self) {
        HOOKS.with(|hooks| {
            hooks.borrow_mut().take();
        });
    }
}
fn install(hooks: Hooks) -> HookGuard {
    HOOKS.with(|slot| assert!(slot.borrow_mut().replace(Arc::new(hooks)).is_none()));
    HookGuard
}
fn budget(bytes: usize, control: &OperationControl) -> Budget {
    Budget::controlled(
        Cost {
            retained_bytes: bytes as u64,
            ..Cost::defaults()
        },
        control.deadline(),
        control.clone(),
    )
}
fn fixture(
    storage: &'static str,
    alpha: u8,
    width: u32,
    height: u32,
) -> (Vec<u8>, Vec<u8>, Description) {
    let description = Description {
        format: "BLP2",
        storage,
        width,
        height,
        mipmap: 0,
        levels: 1,
        alpha_bits: alpha,
    };
    let count = width as usize * height as usize;
    let mut header = vec![0; 1172];
    for (index, color) in header[148..].chunks_exact_mut(4).enumerate() {
        color.copy_from_slice(&[index as u8, (index * 3) as u8, (index * 7) as u8, 99]);
    }
    let length = match storage {
        "palette" => count + (count * alpha as usize).div_ceil(8),
        "bgra8" => count * 4,
        "bc1" => (width as usize).div_ceil(4) * (height as usize).div_ceil(4) * 8,
        _ => (width as usize).div_ceil(4) * (height as usize).div_ceil(4) * 16,
    };
    let data = (0..length)
        .map(|i| ((i * 173 + i / 7) & 255) as u8)
        .collect();
    (header, data, description)
}
fn compare(storage: &'static str, alpha: u8, width: u32, height: u32) {
    let (header, data, description) = fixture(storage, alpha, width, height);
    let control = OperationControl::default();
    let budget = budget(64 << 20, &control);
    let length = width as usize * height as usize * 4;
    let output = budget.reserve_retained(length).unwrap();
    let input = budget.reserve_retained(data.len()).unwrap();
    let before = budget.snapshot().unwrap();
    let logical = budget.logical_snapshot().unwrap();
    let mut expected = vec![0; length];
    fill_strip(
        &mut expected,
        0,
        &header,
        &data,
        &description,
        &control,
        Some(&budget),
    )
    .unwrap();
    for cpus in [1, 2, 3, 8, 256] {
        let mut actual = vec![0; length];
        let workers = fill_with_limit(
            &mut actual,
            &header,
            &data,
            &description,
            &control,
            &budget,
            cpus,
        )
        .unwrap();
        assert!(workers <= cpus);
        if cpus > 1 {
            assert!(workers > 1);
        }
        assert_eq!(
            actual, expected,
            "{storage}/{alpha}/{width}x{height}/{cpus}"
        );
        assert_eq!(budget.snapshot().unwrap(), before);
        assert_eq!(budget.logical_snapshot().unwrap(), logical);
        assert_eq!(
            budget.refundable_retained_bytes(),
            (length + data.len()) as u64
        );
    }
    drop(input);
    drop(output);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn parallel_palette_bgra_and_all_bc_modes_match_serial_with_clipped_edges() {
    for (storage, alpha) in [
        ("palette", 0),
        ("palette", 1),
        ("palette", 4),
        ("palette", 8),
        ("bgra8", 8),
        ("bc1", 0),
        ("bc2", 4),
        ("bc3", 8),
    ] {
        compare(storage, alpha, 1031, 519);
    }
}

#[test]
fn parallelism_follows_cpu_work_size_rows_and_exact_shared_headroom() {
    let (header, data, description) = fixture("bc3", 8, 1024, 512);
    assert_eq!(wanted_workers(&description, 0), 1);
    assert_eq!(wanted_workers(&description, 3), 3);
    assert_eq!(wanted_workers(&description, 256), 8);
    let large = Description {
        width: 2048,
        height: 1024,
        ..description.clone()
    };
    assert_eq!(wanted_workers(&large, 16), 16);
    assert_eq!(wanted_workers(&large, usize::MAX), 32);
    assert_eq!(
        wanted_workers(
            &Description {
                width: 16,
                height: 16,
                ..description.clone()
            },
            8
        ),
        1
    );
    assert_eq!(
        wanted_workers(
            &Description {
                width: 1 << 20,
                height: 1,
                ..description.clone()
            },
            8
        ),
        1
    );
    for allowed in [0, 1, 2, 7] {
        let control = OperationControl::default();
        let budget = budget(allowed * WORKER_BYTES + 101, &control);
        let foreign = budget.reserve_retained(101).unwrap();
        let mut pixels = vec![0; 1024 * 512 * 4];
        assert_eq!(
            fill_with_limit(
                &mut pixels,
                &header,
                &data,
                &description,
                &control,
                &budget,
                8
            )
            .unwrap(),
            allowed + 1
        );
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 101);
        assert_eq!(budget.refundable_retained_bytes(), 101);
        assert_eq!(budget.logical_snapshot().unwrap(), Cost::default());
        drop(foreign);
    }
}

#[test]
fn one_team_runs_all_strips_and_reservation_outlives_the_workers() {
    let (header, data, description) = fixture("bc3", 8, 1024, 512);
    let control = OperationControl::default();
    let budget = Arc::new(budget(64 << 20, &control));
    let entered = Arc::new(Mutex::new(Vec::new()));
    let barrier = Arc::new(Barrier::new(4));
    let _hooks = install(Hooks {
        reject_spawn_at: None,
        before: Box::new({
            let entered = entered.clone();
            let barrier = barrier.clone();
            let budget = budget.clone();
            move |row| {
                assert_eq!(
                    budget.refundable_retained_bytes(),
                    (3 * WORKER_BYTES) as u64
                );
                entered
                    .lock()
                    .unwrap()
                    .push((row, format!("{:?}", thread::current().id())));
                barrier.wait();
                Ok(())
            }
        }),
    });
    let mut pixels = vec![0; 1024 * 512 * 4];
    assert_eq!(
        fill_with_limit(
            &mut pixels,
            &header,
            &data,
            &description,
            &control,
            &budget,
            4
        )
        .unwrap(),
        4
    );
    let entered = entered.lock().unwrap();
    assert_eq!(
        entered.iter().map(|(row, _)| *row).collect::<BTreeSet<_>>(),
        BTreeSet::from([0, 128, 256, 384])
    );
    assert_eq!(
        entered
            .iter()
            .map(|(_, thread)| thread)
            .collect::<BTreeSet<_>>()
            .len(),
        4
    );
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn rejected_spawn_joins_started_strips_and_only_fills_untouched_suffix() {
    let (header, data, description) = fixture("bc3", 8, 1024, 512);
    let control = OperationControl::default();
    let budget = budget(64 << 20, &control);
    let mut expected = vec![0; 1024 * 512 * 4];
    fill_strip(
        &mut expected,
        0,
        &header,
        &data,
        &description,
        &control,
        Some(&budget),
    )
    .unwrap();
    for rejected in [0, 1, 2] {
        let rows = Arc::new(Mutex::new(Vec::new()));
        let _hooks = install(Hooks {
            reject_spawn_at: Some(rejected),
            before: Box::new({
                let rows = rows.clone();
                move |row| {
                    rows.lock().unwrap().push(row);
                    Ok(())
                }
            }),
        });
        let mut actual = vec![0; expected.len()];
        fill_with_limit(
            &mut actual,
            &header,
            &data,
            &description,
            &control,
            &budget,
            4,
        )
        .unwrap();
        assert_eq!(actual, expected);
        let mut seen = rows.lock().unwrap().clone();
        seen.sort_unstable();
        assert_eq!(
            seen,
            (0..=rejected).map(|index| index * 128).collect::<Vec<_>>()
        );
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
}

#[test]
fn ordered_worker_errors_join_every_thread_and_release_only_team_storage() {
    let (header, data, description) = fixture("bc3", 8, 1024, 512);
    let control = OperationControl::default();
    let budget = budget(64 << 20, &control);
    let foreign = budget.reserve_retained(101).unwrap();
    let completed = Arc::new(AtomicUsize::new(0));
    let _hooks = install(Hooks {
        reject_spawn_at: None,
        before: Box::new({
            let completed = completed.clone();
            move |row| {
                if row == 0 {
                    thread::sleep(Duration::from_millis(30));
                }
                completed.fetch_add(1, Ordering::SeqCst);
                Err(lycheedev_core::ToolkitError::new(
                    format!("test.strip.{row}"),
                    "strip failed",
                    4,
                ))
            }
        }),
    });
    let mut pixels = vec![0; 1024 * 512 * 4];
    let error = fill_with_limit(
        &mut pixels,
        &header,
        &data,
        &description,
        &control,
        &budget,
        4,
    )
    .unwrap_err();
    assert_eq!(error.code, "test.strip.0");
    assert_eq!(completed.load(Ordering::SeqCst), 4);
    assert_eq!(budget.refundable_retained_bytes(), 101);
    drop(foreign);
}

#[test]
fn original_scope_cancellation_stops_all_started_workers_before_return() {
    let (header, data, description) = fixture("bc3", 8, 1024, 512);
    let control = OperationControl::default();
    let budget = Arc::new(budget(64 << 20, &control));
    let completed = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(4));
    let _hooks = install(Hooks {
        reject_spawn_at: None,
        before: Box::new({
            let control = control.clone();
            let completed = completed.clone();
            let barrier = barrier.clone();
            move |row| {
                barrier.wait();
                if row == 0 {
                    control.tracked_cancel_handle().unwrap().cancel();
                }
                barrier.wait();
                completed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }),
    });
    let mut pixels = vec![0; 1024 * 512 * 4];
    let error = fill_with_limit(
        &mut pixels,
        &header,
        &data,
        &description,
        &control,
        &budget,
        4,
    )
    .unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(completed.load(Ordering::SeqCst), 4);
    assert_eq!(budget.refundable_retained_bytes(), 0);
    assert!(pixels.iter().all(|&byte| byte == 0));
}

#[test]
fn worker_panic_is_joined_and_does_not_leak_optional_storage() {
    let (header, data, description) = fixture("bc3", 8, 1024, 512);
    let control = OperationControl::default();
    let budget = budget(64 << 20, &control);
    let completed = Arc::new(AtomicUsize::new(0));
    let _hooks = install(Hooks {
        reject_spawn_at: None,
        before: Box::new({
            let completed = completed.clone();
            move |row| {
                completed.fetch_add(1, Ordering::SeqCst);
                assert_ne!(row, 0, "injected worker panic");
                Ok(())
            }
        }),
    });
    let mut pixels = vec![0; 1024 * 512 * 4];
    assert_eq!(
        fill_with_limit(
            &mut pixels,
            &header,
            &data,
            &description,
            &control,
            &budget,
            4
        )
        .unwrap_err()
        .code,
        "records.internal"
    );
    assert_eq!(completed.load(Ordering::SeqCst), 4);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn more_than_eight_cpu_participants_are_allowed_when_work_and_budget_fit() {
    let (header, data, description) = fixture("bc3", 8, 2048, 1024);
    let control = OperationControl::default();
    let budget = budget(64 << 20, &control);
    let mut expected = vec![0; 2048 * 1024 * 4];
    fill_strip(
        &mut expected,
        0,
        &header,
        &data,
        &description,
        &control,
        Some(&budget),
    )
    .unwrap();
    let threads = Arc::new(Mutex::new(BTreeSet::new()));
    let _hooks = install(Hooks {
        reject_spawn_at: None,
        before: Box::new({
            let threads = threads.clone();
            move |_| {
                threads
                    .lock()
                    .unwrap()
                    .insert(format!("{:?}", thread::current().id()));
                Ok(())
            }
        }),
    });
    let mut pixels = vec![0; expected.len()];
    assert_eq!(
        fill_with_limit(
            &mut pixels,
            &header,
            &data,
            &description,
            &control,
            &budget,
            16
        )
        .unwrap(),
        16
    );
    assert_eq!(pixels, expected);
    assert_eq!(threads.lock().unwrap().len(), 16);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}
