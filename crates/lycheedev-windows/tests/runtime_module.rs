//! Current-test-process fixtures only. No game, disk executable read or input.
use lycheedev_windows::{
    Process, WindowIdentity,
    process::{RuntimeBudget, RuntimeCost, RuntimeLimits, RuntimeModule, RuntimeReadKind},
};
use std::{
    ffi::c_void,
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{Foundation::HWND, System::Memory::*, UI::WindowsAndMessaging::*},
    core::w,
};

struct Window {
    hwnd: Option<HWND>,
    _thread: PhantomData<Rc<()>>,
}
impl Window {
    fn new() -> Self {
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Lychee runtime-module fixture"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .unwrap();
        Self {
            hwnd: Some(hwnd),
            _thread: PhantomData,
        }
    }
    fn identity(&self) -> WindowIdentity {
        WindowIdentity {
            process: Process::open(std::process::id())
                .unwrap()
                .identity()
                .clone(),
            hwnd: self.hwnd.unwrap().0 as usize,
        }
    }
    fn close(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            unsafe {
                DestroyWindow(hwnd).unwrap();
            }
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}
fn budget(limits: RuntimeLimits) -> (RuntimeBudget, Arc<AtomicBool>) {
    let cancel = Arc::new(AtomicBool::new(false));
    (
        RuntimeBudget::new(
            limits,
            Instant::now() + Duration::from_secs(10),
            cancel.clone(),
        )
        .unwrap(),
        cancel,
    )
}
#[test]
fn os_issued_module_reads_current_headers_and_accounts_exactly_one_rpm() {
    let window = Window::new();
    let identity = window.identity();
    let (mut budget, _) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(identity.clone(), &mut budget).unwrap();
    assert_eq!(module.identity().window(), &identity);
    assert_eq!(module.identity().architecture(), 0x8664);
    assert!(module.identity().base() > 0 && module.identity().size() > 0);
    let before = budget.usage();
    let mut header = [0xa5; 2];
    module
        .read_exact(0, &mut header, RuntimeReadKind::Metadata, &mut budget)
        .unwrap();
    assert_eq!(&header, b"MZ");
    let after = budget.usage();
    assert_eq!(after.read_calls - before.read_calls, 1);
    assert_eq!(after.requested_bytes - before.requested_bytes, 2);
    assert_eq!(after.query_calls - before.query_calls, 2);
    assert!(after.os_call_units > before.os_call_units);
}
#[test]
fn arbitrary_rvas_empty_and_oversize_reads_cannot_expand_the_capability() {
    let window = Window::new();
    let (mut budget, _) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let before = budget.usage();
    for rva in [module.identity().size(), u64::MAX] {
        let mut bytes = [0xa5; 2];
        assert_eq!(
            module
                .read_exact(rva, &mut bytes, RuntimeReadKind::Metadata, &mut budget)
                .unwrap_err(),
            "memory.runtime_read_range"
        );
        assert_eq!(bytes, [0xa5; 2]);
    }
    assert!(
        module
            .read_exact(0, &mut [], RuntimeReadKind::Metadata, &mut budget)
            .is_err()
    );
    let mut large = vec![0xa5; (1 << 20) + 1];
    assert!(
        module
            .read_exact(0, &mut large, RuntimeReadKind::Metadata, &mut budget)
            .is_err()
    );
    assert!(large.iter().all(|b| *b == 0xa5));
    assert_eq!(budget.usage(), before);
}
#[test]
fn byte_budget_preflight_precedes_allocations_and_any_new_os_work() {
    let window = Window::new();
    let (mut budget, _) = budget(RuntimeLimits {
        requested_bytes: 1,
        ..RuntimeLimits::defaults()
    });
    let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let before = budget.usage();
    let mut bytes = [0xa5; 2];
    assert_eq!(
        module
            .read_exact(0, &mut bytes, RuntimeReadKind::Metadata, &mut budget)
            .unwrap_err(),
        "memory.runtime_budget:bytes"
    );
    assert_eq!(bytes, [0xa5; 2]);
    assert_eq!(budget.usage(), before);
}
#[test]
fn cancellation_deadline_and_a_fresh_budget_do_not_read_or_publish_bytes() {
    let window = Window::new();
    let (mut first, cancel) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(window.identity(), &mut first).unwrap();
    let mut bytes = [0xa5; 2];
    let (mut replacement, _) = budget(RuntimeLimits::defaults());
    assert_eq!(
        module
            .read_exact(0, &mut bytes, RuntimeReadKind::Metadata, &mut replacement)
            .unwrap_err(),
        "memory.runtime_budget_replaced"
    );
    assert_eq!(replacement.usage(), RuntimeCost::default());
    let before = first.usage();
    cancel.store(true, Ordering::Release);
    assert_eq!(
        module
            .read_exact(0, &mut bytes, RuntimeReadKind::Metadata, &mut first)
            .unwrap_err(),
        "memory.runtime_cancelled"
    );
    assert_eq!(first.usage(), before);
    assert_eq!(bytes, [0xa5; 2]);
    let mut expired = RuntimeBudget::new(
        RuntimeLimits::defaults(),
        Instant::now(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(
        RuntimeModule::open(window.identity(), &mut expired)
            .err()
            .unwrap(),
        "memory.runtime_deadline"
    );
    assert_eq!(expired.usage(), RuntimeCost::default());
}
#[test]
fn closed_window_and_wrong_creation_identity_fail_before_rpm() {
    let mut window = Window::new();
    let (mut budget, _) = budget(RuntimeLimits::defaults());
    let identity = window.identity();
    let mut wrong = identity.clone();
    wrong.process.created += 1;
    assert_eq!(
        RuntimeModule::open(wrong, &mut budget).err().unwrap(),
        "process.identity_changed"
    );
    let module = RuntimeModule::open(identity, &mut budget).unwrap();
    let before = budget.usage();
    window.close();
    let mut bytes = [0xa5; 2];
    assert_eq!(
        module
            .read_exact(0, &mut bytes, RuntimeReadKind::Metadata, &mut budget)
            .unwrap_err(),
        "window.closed"
    );
    assert_eq!(budget.usage().read_calls, before.read_calls);
    assert_eq!(bytes, [0xa5; 2]);
}
#[inline(never)]
fn fixture_code(value: u64) -> u64 {
    std::hint::black_box(value).wrapping_add(17)
}
#[test]
fn executable_pages_do_not_grant_writable_root_slot_authority() {
    let window = Window::new();
    let (mut budget, _) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let address = fixture_code as *const () as u64;
    let rva = address.checked_sub(module.identity().base()).unwrap();
    let mut bytes = [0; 8];
    module
        .read_exact(rva, &mut bytes, RuntimeReadKind::Code, &mut budget)
        .unwrap();
    assert_eq!(bytes.as_slice(), unsafe {
        std::slice::from_raw_parts(address as *const u8, 8)
    });
    let before = budget.usage();
    let mut denied = [0xa5; 8];
    assert_eq!(
        module
            .read_exact(rva, &mut denied, RuntimeReadKind::WritableData, &mut budget)
            .unwrap_err(),
        "memory.runtime_mapping_role"
    );
    assert_eq!(budget.usage().read_calls, before.read_calls);
    assert_eq!(denied, [0xa5; 8]);
}

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
        unsafe {
            std::ptr::write_bytes(pointer.as_ptr(), 0x57, bytes);
        }
        Self { pointer, bytes }
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

#[test]
fn private_point_read_crosses_only_requested_pages_and_accounts_one_rpm() {
    let window = Window::new();
    let (mut budget, _) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let allocation = Allocation::new(3 * 4096);
    allocation.protect(4096, 4096, PAGE_READONLY);
    let before = budget.usage();
    let mut bytes = vec![0xa5; 2 * 4096 + 2];
    module
        .read_private_exact(allocation.address() + 4095, &mut bytes[..4098], &mut budget)
        .unwrap();
    assert!(bytes[..4098].iter().all(|b| *b == 0x57));
    assert!(bytes[4098..].iter().all(|b| *b == 0xa5));
    assert_eq!(budget.usage().read_calls - before.read_calls, 1);
    assert_eq!(
        budget.usage().requested_bytes - before.requested_bytes,
        4098
    );
    assert_eq!(budget.usage().query_calls - before.query_calls, 6);
    let mut header = [0; 2];
    module
        .read_exact(0, &mut header, RuntimeReadKind::Metadata, &mut budget)
        .unwrap();
    assert_eq!(budget.usage().read_calls - before.read_calls, 2);
}

#[test]
fn private_point_read_rejects_guard_noaccess_execute_and_module_bytes_without_prefix() {
    let window = Window::new();
    let (mut budget, _) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let allocation = Allocation::new(3 * 4096);
    for protection in [
        PAGE_READWRITE | PAGE_GUARD,
        PAGE_NOACCESS,
        PAGE_EXECUTE_READWRITE,
    ] {
        allocation.protect(4096, 4096, protection);
        let before = budget.usage().read_calls;
        let mut bytes = vec![0xa5; 8192];
        assert!(
            module
                .read_private_exact(allocation.address(), &mut bytes, &mut budget)
                .is_err()
        );
        assert_eq!(budget.usage().read_calls, before);
        assert!(bytes.iter().all(|b| *b == 0xa5));
    }
    let before = budget.usage();
    let mut bytes = [0xa5; 8];
    assert_eq!(
        module
            .read_private_exact(module.identity().base(), &mut bytes, &mut budget)
            .unwrap_err(),
        "memory.runtime_private_range"
    );
    assert_eq!(budget.usage(), before);
    assert_eq!(bytes, [0xa5; 8]);
}

#[test]
fn private_point_preflight_identity_budget_and_cancellation_never_publish_bytes() {
    let mut window = Window::new();
    let (mut first, cancel) = budget(RuntimeLimits {
        requested_bytes: 8,
        ..RuntimeLimits::defaults()
    });
    let module = RuntimeModule::open(window.identity(), &mut first).unwrap();
    let allocation = Allocation::new(4096);
    let (mut replacement, _) = budget(RuntimeLimits::defaults());
    let mut bytes = [0xa5; 9];
    let before = first.usage();
    assert_eq!(
        module
            .read_private_exact(allocation.address(), &mut bytes, &mut first)
            .unwrap_err(),
        "memory.runtime_budget:bytes"
    );
    assert_eq!(first.usage(), before);
    assert_eq!(
        module
            .read_private_exact(allocation.address(), &mut bytes, &mut replacement)
            .unwrap_err(),
        "memory.runtime_budget_replaced"
    );
    assert_eq!(replacement.usage(), RuntimeCost::default());
    cancel.store(true, Ordering::Release);
    assert_eq!(
        module
            .read_private_exact(allocation.address(), &mut bytes[..8], &mut first)
            .unwrap_err(),
        "memory.runtime_cancelled"
    );
    assert_eq!(first.usage(), before);
    cancel.store(false, Ordering::Release);
    window.close();
    assert_eq!(
        module
            .read_private_exact(allocation.address(), &mut bytes[..8], &mut first)
            .unwrap_err(),
        "window.closed"
    );
    assert_eq!(first.usage().read_calls, before.read_calls);
    assert_eq!(bytes, [0xa5; 9]);
}

#[test]
fn private_point_read_discards_staging_when_final_module_guard_exhausts_budget() {
    let window = Window::new();
    let (mut budget, _) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let allocation = Allocation::new(4096);
    // A one-region point read costs 50 units: 22 pre-guard + 2 query +
    // 2 RPM + 2 post-query + 22 final guard. Leave 48 so the final identity
    // check fails after a successful read, proving caller output is atomic.
    budget
        .charge(RuntimeCost {
            os_call_units: RuntimeLimits::defaults().os_call_units
                - budget.usage().os_call_units
                - 48,
            ..RuntimeCost::default()
        })
        .unwrap();
    let before = budget.usage();
    let mut bytes = [0xa5; 8];
    assert_eq!(
        module
            .read_private_exact(allocation.address(), &mut bytes, &mut budget)
            .unwrap_err(),
        "memory.runtime_budget:os_calls"
    );
    assert_eq!(budget.usage().read_calls - before.read_calls, 1);
    assert_eq!(budget.usage().requested_bytes - before.requested_bytes, 8);
    assert_eq!(bytes, [0xa5; 8]);
}

#[test]
fn image_directory_comes_from_selected_os_module_and_original_request() {
    let window = Window::new();
    let (mut request, _) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(window.identity(), &mut request).unwrap();
    let before = request.usage();
    let directory = module.image_directory(&mut request).unwrap();
    assert_eq!(
        directory,
        std::env::current_exe().unwrap().parent().unwrap()
    );
    assert_eq!(request.usage().read_calls, before.read_calls);
    assert!(request.usage().os_call_units > before.os_call_units);
    let (mut foreign, _) = budget(RuntimeLimits::defaults());
    assert_eq!(
        module.image_directory(&mut foreign).unwrap_err(),
        "memory.runtime_budget_replaced"
    );
    assert_eq!(foreign.usage(), RuntimeCost::default());
}

#[test]
fn os_issued_code_prefix_reads_only_selected_mapped_code_with_original_ledger() {
    let window = Window::new();
    let (mut budget, _) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let spans = module
        .code_readable_spans(0, module.identity().size(), &mut budget)
        .unwrap();
    assert!(!spans.is_empty());
    assert!(spans.windows(2).all(|pair| pair[0].1 < pair[1].0));
    let (start, end) = spans[0];
    assert!(end - start >= 16);
    let mut exact = [0; 16];
    module
        .read_exact(start, &mut exact, RuntimeReadKind::Code, &mut budget)
        .unwrap();
    let mut prefix = [0xa5; 16];
    let used = budget.usage();
    assert_eq!(
        module
            .read_code_prefix(start, &mut prefix, &mut budget)
            .unwrap(),
        (16, false)
    );
    assert_eq!(prefix, exact);
    assert_eq!(budget.usage().read_calls - used.read_calls, 1);
    assert_eq!(budget.usage().requested_bytes - used.requested_bytes, 16);
}
#[test]
fn os_read_diagnostics_capture_original_admission_without_extra_io_or_addresses() {
    let window = Window::new();
    let (mut budget, _) = budget(RuntimeLimits::defaults());
    let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let used = budget.usage();
    let mut bytes = [0xa5; 8];
    let failure = module
        .read_private_observed(0, &mut bytes, &mut budget)
        .unwrap_err();
    assert!(!failure.io_attempted);
    assert_eq!(failure.returned_bytes, 0);
    assert_eq!(failure.native_error_code, None);
    assert_eq!(failure.cause, "memory.runtime_private_range");
    assert_eq!(budget.usage(), used);
    assert_eq!(bytes, [0xa5; 8]);
}

#[test]
fn explicit_module_close_is_prepaid_and_runs_after_cancellation_or_exhaustion() {
    for cancelled in [false, true] {
        let window = Window::new();
        let limits = RuntimeLimits::defaults();
        let (mut budget, cancel) = budget(limits);
        let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
        budget
            .charge(RuntimeCost {
                read_calls: limits.read_calls - budget.usage().read_calls,
                ..RuntimeCost::default()
            })
            .unwrap();
        let usage = budget.usage();
        cancel.store(cancelled, Ordering::Release);
        let result = module.close(&budget);
        if cancelled {
            assert_eq!(result.unwrap_err(), "memory.runtime_cancelled");
        } else {
            result.unwrap();
        }
        assert_eq!(budget.usage(), usage); // no late debit can suppress cleanup
    }
}
