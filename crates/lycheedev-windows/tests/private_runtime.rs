//! Actual Windows current-process fixtures only. No game/module/file inspection.
use lycheedev_windows::{
    Process,
    process::{AddressSpace, PRIVATE_PREFIX_MAX_BYTES, PrivateReadBudget, PrivateReadLimits},
};
use std::{
    ffi::c_void,
    ptr::NonNull,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::Memory::*,
    },
    core::PCWSTR,
};

struct Allocation {
    pointer: NonNull<c_void>,
    bytes: usize,
}
impl Allocation {
    fn new(bytes: usize) -> Self {
        let pointer = NonNull::new(unsafe {
            VirtualAlloc(None, bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
        })
        .unwrap();
        let result = Self { pointer, bytes };
        unsafe {
            std::ptr::write_bytes(result.pointer.as_ptr(), 0x57, bytes);
        }
        result
    }
    fn address(&self) -> u64 {
        self.pointer.as_ptr() as u64
    }
    fn protect(&self, offset: usize, bytes: usize, protection: PAGE_PROTECTION_FLAGS) {
        assert!(offset + bytes <= self.bytes);
        let mut old = PAGE_PROTECTION_FLAGS::default();
        unsafe {
            VirtualProtect(
                self.pointer.as_ptr().byte_add(offset),
                bytes,
                protection,
                &mut old,
            )
            .unwrap();
        }
    }
}
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe {
            let _ = VirtualFree(self.pointer.as_ptr(), 0, MEM_RELEASE);
        }
    }
}
struct Mapped {
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}
impl Mapped {
    fn new(bytes: usize) -> Self {
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                bytes as u32,
                PCWSTR::null(),
            )
        }
        .unwrap();
        // Install handle ownership before mapping can fail.
        let mut result = Self {
            handle,
            view: MEMORY_MAPPED_VIEW_ADDRESS::default(),
        };
        result.view = unsafe { MapViewOfFile(result.handle, FILE_MAP_ALL_ACCESS, 0, 0, bytes) };
        assert!(!result.view.Value.is_null());
        result
    }
}
impl Drop for Mapped {
    fn drop(&mut self) {
        unsafe {
            if !self.view.Value.is_null() {
                let _ = UnmapViewOfFile(self.view);
            }
            let _ = CloseHandle(self.handle);
        }
    }
}
fn setup() -> (Process, AddressSpace, PrivateReadBudget, Instant) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut budget = PrivateReadBudget::new(PrivateReadLimits::default()).unwrap();
    let process = Process::open_until(std::process::id(), &mut budget, deadline).unwrap();
    let space = AddressSpace::native(&mut budget, deadline).unwrap();
    (process, space, budget, deadline)
}

#[test]
fn private_prefix_crosses_current_mappings_with_one_physical_read() {
    let (process, space, mut budget, deadline) = setup();
    let page = space.page_bytes() as usize;
    let allocation = Allocation::new(3 * page);
    allocation.protect(page, page, PAGE_READONLY);
    let before = budget.usage();
    let mut bytes = vec![0; 3 * page];
    let read = process
        .read_private_prefix_until(
            allocation.address(),
            &mut bytes,
            &space,
            &mut budget,
            deadline,
        )
        .unwrap();
    assert_eq!(read, bytes.len());
    assert!(bytes.iter().all(|byte| *byte == 0x57));
    let usage = budget.usage();
    assert_eq!(usage.read_calls - before.read_calls, 1);
    assert_eq!(
        usage.requested_bytes - before.requested_bytes,
        bytes.len() as u64
    );
    assert_eq!(usage.query_calls - before.query_calls, 6);
    assert!(usage.os_call_units - before.os_call_units <= 2 * (6 + 5));
}

#[test]
fn guard_and_noaccess_stop_prefix_without_touching_forbidden_pages() {
    let (process, space, mut budget, deadline) = setup();
    let page = space.page_bytes() as usize;
    for protection in [PAGE_READWRITE | PAGE_GUARD, PAGE_NOACCESS] {
        let allocation = Allocation::new(3 * page);
        allocation.protect(page, page, protection);
        let before = budget.usage();
        let mut bytes = vec![0xa5; 3 * page];
        let count = process
            .read_private_prefix_until(
                allocation.address(),
                &mut bytes,
                &space,
                &mut budget,
                deadline,
            )
            .unwrap();
        assert_eq!(count, page);
        assert!(bytes[..page].iter().all(|byte| *byte == 0x57));
        assert!(bytes[page..].iter().all(|byte| *byte == 0xa5));
        assert_eq!(budget.usage().read_calls - before.read_calls, 1);
        let read_before = budget.usage().read_calls;
        assert_eq!(
            process
                .read_private_prefix_until(
                    allocation.address() + page as u64,
                    &mut bytes[..page],
                    &space,
                    &mut budget,
                    deadline
                )
                .unwrap(),
            0
        );
        assert_eq!(budget.usage().read_calls, read_before);
        let guarded = process
            .query_region_until(
                allocation.address() + page as u64,
                &space,
                &mut budget,
                deadline,
            )
            .unwrap()
            .unwrap();
        assert_eq!(guarded.protection, protection.0);
        assert!(!guarded.readable());
    }
}

#[test]
fn mapped_sections_are_ineligible_even_when_readable() {
    let (process, space, mut budget, deadline) = setup();
    let mapped = Mapped::new(space.page_bytes() as usize);
    let address = mapped.view.Value as u64;
    let region = process
        .query_region_until(address, &space, &mut budget, deadline)
        .unwrap()
        .unwrap();
    assert!(region.committed() && region.readable() && !region.private());
    let before = budget.usage();
    let mut bytes = [0xa5; 16];
    assert_eq!(
        process
            .read_private_prefix_until(address, &mut bytes, &space, &mut budget, deadline)
            .unwrap(),
        0
    );
    assert_eq!(bytes, [0xa5; 16]);
    assert_eq!(budget.usage().read_calls, before.read_calls);
}

#[test]
fn native_address_space_end_is_explicit_and_enumeration_advances() {
    let (process, space, mut budget, deadline) = setup();
    let mut at = 0;
    // A bounded initial enumeration sample, not a full-process coverage claim.
    for _ in 0..16 {
        let region = process
            .query_region_until(at, &space, &mut budget, deadline)
            .unwrap()
            .unwrap();
        assert!(region.start <= at && region.end > at);
        at = region.end;
    }
    let before = budget.usage().query_calls;
    assert_eq!(
        process
            .query_region_until(
                space.maximum_application_address() + 1,
                &space,
                &mut budget,
                deadline
            )
            .unwrap(),
        None
    );
    assert_eq!(budget.usage().query_calls, before + 1);
}

#[test]
fn expiry_and_cumulative_os_budgets_cannot_issue_extra_queries_or_reads() {
    let (process, space, mut budget, deadline) = setup();
    let allocation = Allocation::new(space.page_bytes() as usize);
    let before = budget.usage();
    let mut bytes = [0xa5; 16];
    assert_eq!(
        process
            .read_private_prefix_until(
                allocation.address(),
                &mut bytes,
                &space,
                &mut budget,
                Instant::now()
            )
            .unwrap_err(),
        "memory.private_deadline"
    );
    assert_eq!(budget.usage(), before);
    assert_eq!(bytes, [0xa5; 16]);
    let mut budget = PrivateReadBudget::new(PrivateReadLimits {
        query_calls: 1,
        ..PrivateReadLimits::default()
    })
    .unwrap();
    process
        .query_region_until(allocation.address(), &space, &mut budget, deadline)
        .unwrap();
    assert_eq!(
        process
            .query_region_until(allocation.address(), &space, &mut budget, deadline)
            .unwrap_err(),
        "memory.private_query_budget"
    );
    assert_eq!(budget.usage().query_calls, 1);
    let mut budget = PrivateReadBudget::new(PrivateReadLimits {
        read_calls: 0,
        ..PrivateReadLimits::default()
    })
    .unwrap();
    assert_eq!(
        process
            .read_private_prefix_until(
                allocation.address(),
                &mut bytes,
                &space,
                &mut budget,
                deadline
            )
            .unwrap_err(),
        "memory.private_read_budget"
    );
    assert_eq!(budget.usage().read_calls, 0);
    assert_eq!(bytes, [0xa5; 16]);
}

#[test]
fn oversized_and_overflowed_reads_fail_before_physical_memory_io() {
    let (process, space, mut budget, deadline) = setup();
    let allocation = Allocation::new(space.page_bytes() as usize);
    let before = budget.usage().read_calls;
    assert_eq!(
        process
            .read_private_prefix_until(u64::MAX, &mut [0; 2], &space, &mut budget, deadline)
            .unwrap_err(),
        "memory.private_address_overflow"
    );
    assert_eq!(
        process
            .read_private_prefix_until(
                allocation.address(),
                &mut vec![0; PRIVATE_PREFIX_MAX_BYTES + 1],
                &space,
                &mut budget,
                deadline
            )
            .unwrap_err(),
        "memory.private_read_size"
    );
    assert_eq!(budget.usage().read_calls, before);
}

#[test]
fn post_read_query_budget_failure_keeps_caller_buffer_unchanged() {
    let (process, space, _, deadline) = setup();
    let allocation = Allocation::new(space.page_bytes() as usize);
    let mut budget = PrivateReadBudget::new(PrivateReadLimits {
        query_calls: 1,
        ..PrivateReadLimits::default()
    })
    .unwrap();
    let mut bytes = [0xa5; 16];
    assert_eq!(
        process
            .read_private_prefix_until(
                allocation.address(),
                &mut bytes,
                &space,
                &mut budget,
                deadline
            )
            .unwrap_err(),
        "memory.private_query_budget"
    );
    assert_eq!(budget.usage().read_calls, 1);
    assert_eq!(budget.usage().query_calls, 1);
    assert_eq!(bytes, [0xa5; 16]);
}
