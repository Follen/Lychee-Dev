use super::*;
use std::time::Duration;

fn budget(limits: RuntimeLimits) -> RuntimeBudget {
    RuntimeBudget::new(
        limits,
        Instant::now() + Duration::from_secs(10),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
}
fn region(start: u64, end: u64) -> Region {
    Region {
        start,
        end,
        allocation: 0x1000,
        state: MEM_COMMIT.0,
        kind: MEM_IMAGE.0,
        protection: PAGE_READONLY.0,
    }
}
fn segment(start: u64, end: u64) -> Segment {
    Segment {
        start,
        end,
        region: region(start, end),
        eligible: true,
    }
}
#[test]
fn shared_budget_admission_is_atomic_and_never_refunds() {
    let mut b = budget(RuntimeLimits {
        read_calls: 2,
        requested_bytes: 7,
        ..RuntimeLimits::defaults()
    });
    b.charge(RuntimeCost::read(4)).unwrap();
    let first = b.usage();
    assert_eq!(
        b.charge(RuntimeCost::read(4)).unwrap_err(),
        "memory.runtime_budget:bytes"
    );
    assert_eq!(b.usage(), first);
    b.charge(RuntimeCost::read(3)).unwrap();
    assert_eq!((b.usage().read_calls, b.usage().requested_bytes), (2, 7));
    assert_eq!(
        b.charge(RuntimeCost::read(0)).unwrap_err(),
        "memory.runtime_budget:reads"
    );
    let another = budget(RuntimeLimits::defaults());
    assert_ne!(b.request_id(), another.request_id());
}
#[test]
fn all_budget_dimensions_reject_without_charging_other_dimensions() {
    for (cost, field) in [
        (
            RuntimeCost {
                os_call_units: 1,
                ..Default::default()
            },
            "os_calls",
        ),
        (
            RuntimeCost {
                query_calls: 1,
                ..Default::default()
            },
            "queries",
        ),
        (
            RuntimeCost {
                read_calls: 1,
                ..Default::default()
            },
            "reads",
        ),
        (
            RuntimeCost {
                requested_bytes: 1,
                ..Default::default()
            },
            "bytes",
        ),
        (RuntimeCost::allocation(1), "allocations"),
        (RuntimeCost::work(1), "work"),
    ] {
        let mut b = budget(RuntimeLimits::default());
        assert_eq!(
            b.charge(cost).unwrap_err(),
            format!("memory.runtime_budget:{field}")
        );
        assert_eq!(b.usage(), RuntimeUsage::default());
    }
}
#[test]
fn cancellation_and_deadline_do_no_work_but_cannot_replace_an_os_failure() {
    let cancelled = Arc::new(AtomicBool::new(true));
    let mut b =
        RuntimeBudget::new(RuntimeLimits::defaults(), Instant::now(), cancelled.clone()).unwrap();
    assert_eq!(
        b.charge(RuntimeCost::os()).unwrap_err(),
        "memory.runtime_cancelled"
    );
    assert_eq!(
        b.finish::<()>(Err("test.os_denied:5".into())).unwrap_err(),
        "test.os_denied:5"
    );
    cancelled.store(false, Ordering::Release);
    assert_eq!(
        b.charge(RuntimeCost::os()).unwrap_err(),
        "memory.runtime_deadline"
    );
    assert_eq!(
        b.finish::<()>(Err("test.os_denied:5".into())).unwrap_err(),
        "test.os_denied:5"
    );
    assert_eq!(b.usage(), RuntimeUsage::default());
}
#[test]
fn current_mapping_splits_preserve_allocation_identity_and_intersections() {
    let plan = [segment(0x1000, 0x5000)];
    let split = region(0x2000, 0x3000);
    assert_eq!(
        check_segment(
            &plan,
            split,
            0x2400,
            0x4400,
            0x1000,
            0x5000,
            RuntimeReadKind::Metadata
        )
        .unwrap(),
        0x3000
    );
    let expanded = region(0x1000, 0x5000);
    let observed = [segment(0x2000, 0x3000), segment(0x3000, 0x5000)];
    assert_eq!(
        check_segment(
            &observed,
            expanded,
            0x2400,
            0x4400,
            0x1000,
            0x5000,
            RuntimeReadKind::Metadata
        )
        .unwrap(),
        0x3000
    );
}
#[test]
fn an_expanded_mapping_cannot_authorize_an_issued_hole() {
    let mut denied = segment(0x3000, 0x4000);
    denied.eligible = false;
    let plan = [segment(0x1000, 0x3000), denied, segment(0x4000, 0x5000)];
    let expanded = region(0x1000, 0x5000);
    assert_eq!(
        check_segment(
            &plan,
            expanded,
            0x2000,
            0x4500,
            0x1000,
            0x5000,
            RuntimeReadKind::Metadata
        )
        .unwrap(),
        0x3000
    );
    assert_eq!(
        check_segment(
            &plan,
            expanded,
            0x3000,
            0x4500,
            0x1000,
            0x5000,
            RuntimeReadKind::Metadata
        )
        .unwrap_err(),
        "memory.runtime_mapping_changed"
    );
}
#[test]
fn mapping_mutation_external_allocation_private_guard_and_uncommitted_are_denied() {
    let plan = [segment(0x1000, 0x5000)];
    for mutation in 0..7 {
        let mut current = region(0x2000, 0x3000);
        match mutation {
            0 => current.allocation = 0,
            1 => current.allocation = 0x2000,
            2 => current.kind = MEM_PRIVATE.0,
            3 => current.state = MEM_RESERVE.0,
            4 => current.protection |= PAGE_GUARD.0,
            5 => current.protection = PAGE_READWRITE.0,
            _ => current.protection = PAGE_NOACCESS.0,
        }
        assert_eq!(
            check_segment(
                &plan,
                current,
                0x2400,
                0x2800,
                0x1000,
                0x5000,
                RuntimeReadKind::Metadata
            )
            .unwrap_err(),
            "memory.runtime_mapping_changed"
        );
    }
}
#[test]
fn mapped_module_pieces_are_permitted_but_code_and_root_roles_stay_distinct() {
    let mut mapped = segment(0x1000, 0x5000);
    mapped.region.kind = MEM_MAPPED.0;
    mapped.region.protection = PAGE_EXECUTE_READ.0;
    assert!(
        check_segment(
            &[mapped],
            mapped.region,
            0x1100,
            0x1108,
            0x1000,
            0x5000,
            RuntimeReadKind::Code
        )
        .is_ok()
    );
    assert_eq!(
        check_segment(
            &[mapped],
            mapped.region,
            0x1100,
            0x1108,
            0x1000,
            0x5000,
            RuntimeReadKind::WritableData
        )
        .unwrap_err(),
        "memory.runtime_mapping_role"
    );
    mapped.region.protection = PAGE_READWRITE.0;
    assert!(
        check_segment(
            &[mapped],
            mapped.region,
            0x1100,
            0x1108,
            0x1000,
            0x5000,
            RuntimeReadKind::WritableData
        )
        .is_ok()
    );
    assert_eq!(
        check_segment(
            &[mapped],
            mapped.region,
            0x1100,
            0x1108,
            0x1000,
            0x5000,
            RuntimeReadKind::Code
        )
        .unwrap_err(),
        "memory.runtime_mapping_role"
    );
}

#[test]
fn private_point_ranges_are_bounded_and_cannot_alias_the_module() {
    let range =
        |address, bytes| private_range(address, bytes, 0x1000, 0x7fffffff, 0x40000, 0x8000, 4096);
    assert_eq!(range(0x80001, MAX_READ).unwrap(), (0x180001, 257));
    for (address, bytes) in [
        (0, 1),
        (0x1000, 0),
        (u64::MAX, 2),
        (0x7fffffff, 2),
        (0x48000, MAX_READ + 1),
        (0x3ffff, 2),
        (0x40000, 8),
        (0x47fff, 2),
    ] {
        assert_eq!(
            range(address, bytes).unwrap_err(),
            "memory.runtime_private_range"
        );
    }
    assert_eq!(range(0x48000, 8).unwrap(), (0x48008, 1));
}

fn private_region(start: u64, end: u64) -> Region {
    Region {
        kind: MEM_PRIVATE.0,
        protection: PAGE_READWRITE.0,
        ..region(start, end)
    }
}

#[test]
fn private_point_mapping_requires_one_readable_nonexecutable_allocation() {
    let current = private_region(0x2000, 0x4000);
    assert_eq!(
        private_piece(current, 0x2800, 0x4800, 0x1000, 0xffff, None).unwrap(),
        0x4000
    );
    for mutation in 0..9 {
        let mut changed = current;
        match mutation {
            0 => changed.kind = MEM_IMAGE.0,
            1 => changed.kind = MEM_MAPPED.0,
            2 => changed.state = MEM_RESERVE.0,
            3 => changed.protection |= PAGE_GUARD.0,
            4 => changed.protection = PAGE_NOACCESS.0,
            5 => changed.protection = PAGE_EXECUTE_READWRITE.0,
            6 => changed.allocation = 0,
            7 => changed.allocation = 0x3000,
            _ => changed.allocation = 0x2000,
        }
        assert!(private_piece(changed, 0x2800, 0x3800, 0x1000, 0xffff, Some(0x1000)).is_err());
    }
}

#[test]
fn private_point_post_guard_covers_splits_and_rejects_changed_allocation_or_protection() {
    let observed = Segment {
        start: 0x2100,
        end: 0x4800,
        region: private_region(0x2000, 0x5000),
        eligible: true,
    };
    let split = private_region(0x3000, 0x4000);
    assert_eq!(
        check_private_piece(&[observed], split, 0x3400, 0x4800, 0x1000, 0xffff).unwrap(),
        0x4000
    );
    let mut changed = split;
    changed.allocation = 0x2000;
    assert!(check_private_piece(&[observed], changed, 0x3400, 0x4800, 0x1000, 0xffff).is_err());
    changed = split;
    changed.protection = PAGE_READONLY.0;
    assert_eq!(
        check_private_piece(&[observed], changed, 0x3400, 0x4800, 0x1000, 0xffff).unwrap_err(),
        "memory.runtime_private_mapping_changed"
    );
    assert!(check_private_piece(&[observed], split, 0x2000, 0x4800, 0x1000, 0xffff).is_err());
}

struct ExternalControl(Arc<AtomicBool>);
impl RuntimeControl for ExternalControl {
    fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Acquire) {
            Err("fixture.parent_cancelled".into())
        } else {
            Ok(())
        }
    }
}
#[test]
fn retained_control_observes_changes_but_preserves_original_os_failure() {
    let cancel = Arc::new(AtomicBool::new(false));
    let b = RuntimeBudget::with_control(
        RuntimeLimits::defaults(),
        Instant::now() + Duration::from_secs(10),
        ExternalControl(cancel.clone()),
    )
    .unwrap();
    cancel.store(true, Ordering::Release);
    assert_eq!(
        b.finish::<()>(Err("fixture.os_fatal:5".into()))
            .unwrap_err(),
        "fixture.os_fatal:5"
    );
    assert_eq!(b.finish(Ok(())).unwrap_err(), "fixture.parent_cancelled");
}
#[test]
fn control_factory_cannot_allocate_before_budget_admission() {
    let called = std::cell::Cell::new(false);
    let result = RuntimeBudget::with_control_factory(
        RuntimeLimits {
            allocation_bytes: 0,
            ..RuntimeLimits::defaults()
        },
        Instant::now() + Duration::from_secs(10),
        100,
        || {
            called.set(true);
            ExternalControl(Arc::new(AtomicBool::new(false)))
        },
    );
    assert_eq!(result.err().unwrap(), "memory.runtime_budget:allocations");
    assert!(!called.get());
}
#[test]
fn unicode_image_directory_has_one_precharged_owned_buffer() {
    let image: Vec<u16> = "C:\\中文目录\\客户端.exe".encode_utf16().collect();
    let directory_units = image.iter().rposition(|u| *u == 92).unwrap() + 1;
    let mut b = budget(RuntimeLimits {
        allocation_bytes: (directory_units * 3) as u64,
        ..RuntimeLimits::defaults()
    });
    let path = image_directory_from_wide(&image, &mut b).unwrap();
    assert_eq!(path, std::path::Path::new("C:\\中文目录\\"));
    assert_eq!(b.usage().allocation_bytes, (directory_units * 3) as u64);
    assert!(path.capacity() <= directory_units * 3);
}
#[test]
fn unicode_directory_budget_or_encoding_failure_publishes_nothing() {
    let image: Vec<u16> = "C:\\中文目录\\客户端.exe".encode_utf16().collect();
    let directory_units = image.iter().rposition(|u| *u == 92).unwrap() + 1;
    let mut b = budget(RuntimeLimits {
        allocation_bytes: (directory_units * 3 - 1) as u64,
        ..RuntimeLimits::defaults()
    });
    let mut published = None;
    let result = image_directory_from_wide(&image, &mut b).map(|path| published = Some(path));
    assert_eq!(result.unwrap_err(), "memory.runtime_budget:allocations");
    assert!(published.is_none());
    assert_eq!(b.usage().allocation_bytes, 0);
    assert_eq!(
        image_directory_from_wide(
            &[67, 58, 92, 0xd800, 92, 120],
            &mut budget(RuntimeLimits::defaults())
        )
        .unwrap_err(),
        "memory.runtime_image_encoding"
    );
}
