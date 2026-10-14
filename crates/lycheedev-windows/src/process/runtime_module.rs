//! OS-issued main-module reads for runtime instruction recipes. Opening this
//! capability performs no disk read. The explicit verify_image_layout issuer
//! additionally locks/hashes its EXE under the same request ledger. Both remain
//! separate from historical ModulePlan and private-heap discovery.
use super::{Handle, Process, ProcessIdentity, WindowIdentity, readable};
use crate::Result;
use std::{
    mem::size_of,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use windows::Win32::{
    Foundation::*,
    System::{
        Diagnostics::Debug::ReadProcessMemory,
        Memory::*,
        ProcessStatus::*,
        SystemInformation::{
            GetNativeSystemInfo, IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64,
            IMAGE_FILE_MACHINE_UNKNOWN, SYSTEM_INFO,
        },
        Threading::*,
    },
    UI::WindowsAndMessaging::*,
};

#[path = "runtime_image.rs"]
mod runtime_image;
pub use runtime_image::{RuntimeImageGuard, RuntimeImageLayout, RuntimeImageSection};

const MAX_MODULE: u64 = 512 << 20;
const MAX_READ: usize = 1 << 20;
const MAX_MODULES: usize = 4096;
static NEXT_SCOPE: AtomicU64 = AtomicU64::new(1);

fn scope_id() -> Result<u64> {
    NEXT_SCOPE
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| "memory.runtime_scope_exhausted".into())
}

/// Cumulative admissions. OS units conservatively include one possible
/// GetLastError per API; opening a handle also reserves its eventual CloseHandle.
/// Allocation bytes count requested capacities cumulatively, not live RSS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeCost {
    pub os_call_units: u64,
    pub query_calls: u64,
    pub read_calls: u64,
    pub requested_bytes: u64,
    pub allocation_bytes: u64,
    pub work_units: u64,
}
pub type RuntimeUsage = RuntimeCost;
pub type RuntimeLimits = RuntimeCost;
/// Retained request control, checked throughout OS and CPU work. Implementors
/// can observe a complete cancellation ancestry without a dependency on Core.
/// Checks must be bounded and perform no I/O or allocation on success.
pub trait RuntimeControl: Send + Sync {
    fn check(&self) -> Result<()>;
}
impl RuntimeCost {
    pub fn defaults() -> Self {
        Self {
            os_call_units: 2_000_000,
            query_calls: 400_000,
            read_calls: 8192,
            requested_bytes: 256 << 20,
            allocation_bytes: 512 << 20,
            work_units: 1 << 30,
        }
    }
    pub fn allocation(bytes: usize) -> Self {
        Self {
            allocation_bytes: bytes as u64,
            ..Self::default()
        }
    }
    pub fn work(units: u64) -> Self {
        Self {
            work_units: units,
            ..Self::default()
        }
    }
    fn os() -> Self {
        Self {
            os_call_units: 2,
            ..Self::default()
        }
    }
    fn read(bytes: usize) -> Self {
        Self {
            os_call_units: 2,
            read_calls: 1,
            requested_bytes: bytes as u64,
            ..Self::default()
        }
    }
}

/// One request's immutable deadline/cancellation and cumulative ledger. A module
/// seals this ledger's identity, so replacing it with a fresh budget cannot reset
/// the authority of an already opened capability. Callers may debit additional
/// bounded recipe/path work through charge; no method refunds or resets usage.
pub struct RuntimeBudget {
    id: u64,
    limits: RuntimeLimits,
    used: RuntimeUsage,
    deadline: Instant,
    cancel: Option<Arc<AtomicBool>>,
    control: Option<Box<dyn RuntimeControl>>,
    scoped_controls: Vec<Box<dyn RuntimeControl>>,
}
impl RuntimeBudget {
    pub fn new(limits: RuntimeLimits, deadline: Instant, cancel: Arc<AtomicBool>) -> Result<Self> {
        Self::initialize(limits, deadline, Some(cancel))
    }
    pub fn with_control<C: RuntimeControl + 'static>(
        limits: RuntimeLimits,
        deadline: Instant,
        control: C,
    ) -> Result<Self> {
        control.check()?;
        Self::with_control_factory(limits, deadline, 0, || control)
    }
    /// Reserve owned control storage before invoking the factory. This lets a
    /// caller debit retained cancellation ancestry before cloning it.
    pub fn with_control_factory<C: RuntimeControl + 'static>(
        limits: RuntimeLimits,
        deadline: Instant,
        retained_bytes: usize,
        create: impl FnOnce() -> C,
    ) -> Result<Self> {
        let mut budget = Self::initialize(limits, deadline, None)?;
        let allocation = size_of::<C>()
            .checked_add(retained_bytes)
            .ok_or("memory.runtime_control_allocation")?;
        budget.charge(RuntimeCost::allocation(allocation))?;
        let control = create();
        control.check()?;
        budget.control = Some(Box::new(control));
        Ok(budget)
    }
    fn initialize(
        limits: RuntimeLimits,
        deadline: Instant,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Self> {
        if limits.os_call_units > 8_000_000
            || limits.query_calls > 1_000_000
            || limits.read_calls > 1_000_000
            || limits.requested_bytes > 128 << 30
            || limits.allocation_bytes > 2 << 30
            || limits.work_units > 32 << 30
        {
            return Err("memory.runtime_budget_options".into());
        }
        Ok(Self {
            id: scope_id()?,
            limits,
            used: RuntimeUsage::default(),
            deadline,
            cancel,
            control: None,
            scoped_controls: Vec::new(),
        })
    }
    pub fn usage(&self) -> RuntimeUsage {
        self.used
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn request_id(&self) -> u64 {
        self.id
    }
    pub fn check(&self) -> Result<()> {
        if let Some(control) = &self.control {
            control.check()?;
        }
        for control in &self.scoped_controls {
            control.check()?;
        }
        if self
            .cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire))
        {
            return Err("memory.runtime_cancelled".into());
        }
        if Instant::now() >= self.deadline {
            return Err("memory.runtime_deadline".into());
        }
        Ok(())
    }
    /// Apply a caller's narrower operation scope without replacing this
    /// capability's original identity, deadline, ancestors or cumulative ledger.
    /// The factory's retained storage is charged before it is cloned/boxed.
    /// A panic leaves the narrow scope installed (fail closed); normal return
    /// restores only the previous extra scope. The callback must check its own
    /// result boundary, as the module/reader methods already do.
    pub fn with_control_scope_factory<C: RuntimeControl + 'static, T>(
        &mut self,
        retained_bytes: usize,
        create: impl FnOnce() -> C,
        run: impl FnOnce(&mut Self) -> T,
    ) -> Result<T> {
        self.check()?;
        if self.scoped_controls.len() >= 64 {
            return Err("memory.runtime_control_depth".into());
        }
        let growth = if self.scoped_controls.len() == self.scoped_controls.capacity() {
            size_of::<Box<dyn RuntimeControl>>()
        } else {
            0
        };
        let allocation = size_of::<C>()
            .checked_add(retained_bytes)
            .and_then(|n| n.checked_add(growth))
            .ok_or("memory.runtime_control_allocation")?;
        self.charge(RuntimeCost::allocation(allocation))?;
        let control = create();
        control.check()?;
        if growth != 0 {
            self.scoped_controls.reserve_exact(1);
        }
        self.scoped_controls.push(Box::new(control));
        let result = run(self);
        self.scoped_controls.pop();
        Ok(result)
    }

    fn admitted(&self, cost: RuntimeCost) -> Result<RuntimeUsage> {
        self.check()?;
        let mut next = self.used;
        for (used, maximum, addition, field) in [
            (
                &mut next.os_call_units,
                self.limits.os_call_units,
                cost.os_call_units,
                "os_calls",
            ),
            (
                &mut next.query_calls,
                self.limits.query_calls,
                cost.query_calls,
                "queries",
            ),
            (
                &mut next.read_calls,
                self.limits.read_calls,
                cost.read_calls,
                "reads",
            ),
            (
                &mut next.requested_bytes,
                self.limits.requested_bytes,
                cost.requested_bytes,
                "bytes",
            ),
            (
                &mut next.allocation_bytes,
                self.limits.allocation_bytes,
                cost.allocation_bytes,
                "allocations",
            ),
            (
                &mut next.work_units,
                self.limits.work_units,
                cost.work_units,
                "work",
            ),
        ] {
            if addition > maximum - *used {
                return Err(format!("memory.runtime_budget:{field}"));
            }
            *used += addition;
        }
        Ok(next)
    }
    pub fn charge(&mut self, cost: RuntimeCost) -> Result<()> {
        self.used = self.admitted(cost)?;
        Ok(())
    }
    fn finish<T>(&self, result: Result<T>) -> Result<T> {
        // A failed OS call must never be relabeled cancellation/deadline.
        let value = result?;
        self.check()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeReadKind {
    /// Locating metadata; still confined to issued readable module mappings.
    Metadata,
    /// Readable and executable current pages within an executable PE section.
    Code,
    /// Readable, writable and non-executable current pages for the root slot.
    WritableData,
}

/// Descriptive identity is not a read capability. Its fields and issuance nonce
/// are private; only RuntimeModule can issue one. The optional trusted EXE
/// layout binds to this exact issuance; a digest alone cannot recreate it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeModuleIdentity {
    serial: u64,
    request: u64,
    window: WindowIdentity,
    base: u64,
    size: u64,
    architecture: u16,
}
impl RuntimeModuleIdentity {
    pub fn process(&self) -> &ProcessIdentity {
        &self.window.process
    }
    pub fn window(&self) -> &WindowIdentity {
        &self.window
    }
    pub fn base(&self) -> u64 {
        self.base
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn architecture(&self) -> u16 {
        self.architecture
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Region {
    start: u64,
    end: u64,
    allocation: u64,
    state: u32,
    kind: u32,
    protection: u32,
}
#[derive(Clone, Copy, Debug)]
struct Segment {
    start: u64,
    end: u64,
    region: Region,
    eligible: bool,
}

/// Facts retained from one existing module/private read, with no diagnostic IO.
#[derive(Debug)]
pub struct RuntimeReadFailure {
    pub cause: String,
    pub returned_bytes: usize,
    pub reason: &'static str,
    pub io_attempted: bool,
    pub native_error_code: Option<u32>,
    /// Original queried (Type, State, Protect), present only when ineligible.
    pub region: Option<(u32, u32, u32)>,
}
struct ReadAttempt {
    returned_bytes: usize,
    reason: &'static str,
    io_attempted: bool,
    native_error_code: Option<u32>,
    region: Option<(u32, u32, u32)>,
}
impl Default for ReadAttempt {
    fn default() -> Self {
        Self {
            returned_bytes: 0,
            reason: "read_admission",
            io_attempted: false,
            native_error_code: None,
            region: None,
        }
    }
}
impl ReadAttempt {
    fn fail(self, cause: String) -> RuntimeReadFailure {
        RuntimeReadFailure {
            cause,
            returned_bytes: self.returned_bytes,
            reason: self.reason,
            io_attempted: self.io_attempted,
            native_error_code: self.native_error_code,
            region: self.region,
        }
    }
    fn native_error(&mut self, result: &std::result::Result<(), windows::core::Error>) {
        if result.is_err() {
            self.reason = "native_read_failed";
        }
        self.native_error_code = result.as_ref().err().and_then(win32_error_code);
    }
    fn queried(&mut self, region: Region) {
        self.reason = "region_ineligible";
        self.region = Some((region.kind, region.state, region.protection));
    }
    fn eligible(&mut self) {
        self.reason = "read_admission";
        self.region = None;
    }
}

fn win32_error_code(error: &windows::core::Error) -> Option<u32> {
    let code = error.code().0 as u32;
    (code & 0xffff0000 == 0x80070000).then_some(code & 0xffff)
}
fn unread_code_gap(cause: &str) -> bool {
    matches!(
        cause,
        "memory.runtime_mapping_changed"
            | "memory.runtime_mapping_hole"
            | "memory.runtime_mapping_role"
    )
}
#[allow(clippy::too_many_arguments)]
fn verify_code_prefix(
    issued: &[Segment],
    before: &[Segment],
    start: u64,
    received: usize,
    base: u64,
    module_end: u64,
    max_queries: usize,
    mut query: impl FnMut(u64) -> Result<Region>,
) -> Result<()> {
    let end = start
        .checked_add(received as u64)
        .ok_or("memory.runtime_read_range")?;
    let mut at = start;
    let mut queries = 0;
    while at < end {
        if queries == max_queries {
            return Err("memory.runtime_mapping_budget".into());
        }
        let current = query(at)?;
        let original = check_segment(
            issued,
            current,
            at,
            end,
            base,
            module_end,
            RuntimeReadKind::Code,
        )?;
        let previous = check_segment(
            before,
            current,
            at,
            end,
            base,
            module_end,
            RuntimeReadKind::Code,
        )?;
        at = original.min(previous);
        queries += 1;
    }
    Ok(())
}

fn join_runtime_causes(mut first: String, next: Result<()>) -> String {
    if let Err(next) = next {
        for leaf in next.lines() {
            if !first.lines().any(|prior| prior == leaf) {
                first.push('\n');
                first.push_str(leaf);
            }
        }
    }
    first
}
// Disarm the owner's Drop before the checked close attempt. Failure never
// authorizes a second close: Windows may already have consumed/reused a handle.
fn consume_close<T>(
    owner: T,
    token: impl FnOnce(&T) -> HANDLE,
    close: impl FnOnce(HANDLE) -> Result<()>,
) -> Result<()> {
    let raw = token(&owner);
    std::mem::forget(owner);
    close(raw)
}
#[cfg(test)]
thread_local! { static CHECKED_CLOSE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
fn close_handle(handle: Handle, budget: &RuntimeBudget) -> Result<()> {
    let before = budget.check();
    let close = consume_close(
        handle,
        |handle| handle.0,
        |raw| {
            #[cfg(test)]
            CHECKED_CLOSE_CALLS.with(|calls| calls.set(calls.get() + 1));
            // SAFETY: raw belongs to the consumed unique read-only process owner.
            // Its Drop has been disarmed before this synchronous close, and no
            // reference to its Process or RuntimeModule remains usable afterward.
            unsafe { CloseHandle(raw) }
                .map_err(|error| os_error("memory.runtime_process_close", error))
        },
    );
    let after = budget.check();
    let mut result = close;
    for check in [before, after] {
        if let Err(next) = check {
            result = Err(match result {
                Ok(()) => next,
                Err(first) => join_runtime_causes(first, Err(next)),
            });
        }
    }
    result
}

/// Owns the held read-only process. The only constructor inventories the OS main
/// module; there is no constructor from caller-supplied base, size or mappings.
pub struct RuntimeModule {
    process: Process,
    identity: RuntimeModuleIdentity,
    page_bytes: u64,
    address_minimum: u64,
    address_maximum: u64,
    segments: Vec<Segment>,
}
impl RuntimeModule {
    pub fn open(window: WindowIdentity, budget: &mut RuntimeBudget) -> Result<Self> {
        if window.process.pid == 0 || window.process.created == 0 || window.hwnd == 0 {
            return Err("memory.runtime_identity_required".into());
        }
        budget.charge(RuntimeCost {
            // OpenProcess/GetLastError + reserved CloseHandle/GetLastError.
            os_call_units: 4,
            ..RuntimeCost::default()
        })?;
        let opened = unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
                false,
                window.process.pid,
            )
        }
        .map(Handle)
        .map_err(|e| os_error("process.open", e));
        let handle = match opened {
            Ok(handle) => handle,
            Err(cause) => return Err(join_runtime_causes(cause, budget.check())),
        };
        if let Err(cause) = budget.check() {
            return Err(join_runtime_causes(cause, close_handle(handle, budget)));
        }
        let process = Process {
            handle,
            identity: window.process.clone(),
        };
        Self::from_process(process, window, budget)
    }
    // The process was opened above under this ledger, including its RAII close.
    // No external unaccounted handle or caller-supplied module range can enter.
    fn from_process(
        process: Process,
        window: WindowIdentity,
        budget: &mut RuntimeBudget,
    ) -> Result<Self> {
        let mut held = Some(process);
        let prepared = (|| {
            let process = held.as_ref().expect("owned opening process");
            if process.identity != window.process || window.hwnd == 0 {
                return Err("memory.runtime_process_reused".into());
            }
            verify_window(&process, &window, budget)?;
            let architecture = architecture(&process, budget)?;
            budget.charge(RuntimeCost::os())?;
            let mut info = SYSTEM_INFO::default();
            unsafe {
                GetNativeSystemInfo(&mut info);
            }
            budget.finish(Ok(()))?;
            let page = info.dwPageSize as u64;
            if !(4096..=65536).contains(&page) || !page.is_power_of_two() {
                return Err("memory.runtime_page_size".into());
            }
            let (base, size) = inventory(&process, budget)?;
            if base == 0
                || size == 0
                || size > MAX_MODULE
                || base % page != 0
                || size % page != 0
                || base.checked_add(size).is_none()
                || base + size - 1 > info.lpMaximumApplicationAddress as u64
            {
                return Err("memory.runtime_module_range".into());
            }
            confirm_main_image(&process, base, budget)?;
            let capacity = (size / page) as usize;
            budget.charge(RuntimeCost::allocation(capacity * size_of::<Segment>()))?;
            let mut segments = Vec::with_capacity(capacity);
            let mut at = base;
            while at < base + size {
                if segments.len() == capacity {
                    return Err("memory.runtime_mapping_budget".into());
                }
                let region = query(&process, at, page, budget)?;
                let end = region.end.min(base + size);
                segments.push(Segment {
                    start: at,
                    end,
                    eligible: eligible(region, base, base + size),
                    region,
                });
                at = end;
            }
            let serial = scope_id()?;
            let module = Self {
                process: held.take().expect("process transferred once"),
                identity: RuntimeModuleIdentity {
                    serial,
                    request: budget.id,
                    window,
                    base,
                    size,
                    architecture,
                },
                page_bytes: page,
                address_minimum: info.lpMinimumApplicationAddress as u64,
                address_maximum: info.lpMaximumApplicationAddress as u64,
                segments,
            };
            if let Err(cause) = module.verify(budget) {
                return Err(join_runtime_causes(cause, module.close(budget)));
            }
            Ok(module)
        })();
        match prepared {
            Ok(module) => Ok(module),
            Err(cause) => Err(match held.take() {
                Some(process) => join_runtime_causes(cause, close_handle(process.handle, budget)),
                None => cause, // ownership already transferred and explicitly closed
            }),
        }
    }
    /// Consume the held process, attempting its pre-reserved CloseHandle exactly
    /// once even after cancellation/deadline/budget exhaustion. No verification
    /// IO or retry is performed during cleanup. Drop remains only panic fallback.
    pub fn close(self, budget: &RuntimeBudget) -> Result<()> {
        let before = self.check_budget(budget);
        let closed = close_handle(self.process.handle, budget);
        match before {
            Ok(()) => closed,
            Err(cause) => Err(join_runtime_causes(cause, closed)),
        }
    }
    pub fn identity(&self) -> &RuntimeModuleIdentity {
        &self.identity
    }
    /// Fresh OS-reported image directory for metadata scope. No executable is
    /// opened, read or hashed. Caller metadata reads must share this ledger.
    pub fn image_directory(&self, budget: &mut RuntimeBudget) -> Result<std::path::PathBuf> {
        self.verify(budget)?;
        let image = confirm_main_image(&self.process, self.identity.base, budget)?;
        let directory = image_directory_from_wide(&image, budget)?;
        self.verify(budget)?;
        Ok(directory)
    }
    pub fn verify(&self, budget: &mut RuntimeBudget) -> Result<()> {
        self.check_budget(budget)?;
        verify_process(&self.process, budget)?;
        if architecture(&self.process, budget)? != self.identity.architecture {
            return Err("memory.runtime_architecture_changed".into());
        }
        let (base, size) = inventory(&self.process, budget)?;
        if base != self.identity.base || size != self.identity.size {
            return Err("memory.runtime_module_changed".into());
        }
        verify_window(&self.process, &self.identity.window, budget)
    }
    fn check_budget(&self, budget: &RuntimeBudget) -> Result<()> {
        if self.identity.request != budget.id {
            return Err("memory.runtime_budget_replaced".into());
        }
        budget.check()
    }
    /// Exactly one RPM on success, with exact-length semantics and a staging
    /// buffer. All spanned current pieces must match the sealed plan before and
    /// after; the caller's buffer is unchanged on every error. No prefix salvage.
    /// OS calls are synchronous and cannot be forcibly interrupted at deadline.
    pub fn read_exact(
        &self,
        rva: u64,
        into: &mut [u8],
        kind: RuntimeReadKind,
        budget: &mut RuntimeBudget,
    ) -> Result<()> {
        self.read_exact_observed(rva, into, kind, budget)
            .map_err(|failure| failure.cause)
    }
    pub fn read_exact_observed(
        &self,
        rva: u64,
        into: &mut [u8],
        kind: RuntimeReadKind,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<(), RuntimeReadFailure> {
        let mut attempt = ReadAttempt::default();
        self.read_exact_inner(rva, into, kind, budget, &mut attempt)
            .map_err(|cause| attempt.fail(cause))
    }
    fn read_exact_inner(
        &self,
        rva: u64,
        into: &mut [u8],
        kind: RuntimeReadKind,
        budget: &mut RuntimeBudget,
        attempt: &mut ReadAttempt,
    ) -> Result<()> {
        self.check_budget(budget)?;
        if into.is_empty()
            || into.len() > MAX_READ
            || rva > self.identity.size
            || into.len() as u64 > self.identity.size - rva
        {
            return Err("memory.runtime_read_range".into());
        }
        let start = self.identity.base + rva;
        let end = start + into.len() as u64;
        let pages =
            (start % self.page_bytes + into.len() as u64).div_ceil(self.page_bytes) as usize;
        // Deny an already exhausted physical-read/byte/allocation budget before
        // staging or guard IO. Guards still debit each actual OS invocation.
        budget.admitted(RuntimeCost {
            allocation_bytes: (into.len() + pages * size_of::<Segment>()) as u64,
            query_calls: 2,
            ..RuntimeCost::read(into.len())
        })?;
        self.verify(budget)?;
        budget.charge(RuntimeCost::allocation(pages * size_of::<Segment>()))?;
        let mut before = Vec::with_capacity(pages);
        let mut at = start;
        while at < end {
            if before.len() == pages {
                return Err("memory.runtime_mapping_budget".into());
            }
            attempt.reason = "region_query_failed";
            let current =
                query_observed(&self.process, at, self.page_bytes, budget, Some(attempt))?;
            attempt.queried(current);
            let next = check_segment(
                &self.segments,
                current,
                at,
                end,
                self.identity.base,
                self.identity.base + self.identity.size,
                kind,
            )?;
            attempt.eligible();
            before.push(Segment {
                start: at,
                end: next,
                region: current,
                eligible: true,
            });
            at = next;
        }
        budget.charge(RuntimeCost::allocation(into.len()))?;
        let mut staged = vec![0; into.len()];
        budget.charge(RuntimeCost::read(into.len()))?;
        let mut count = 0;
        attempt.io_attempted = true;
        attempt.reason = "short_read";
        // SAFETY: the held read-only handle and all mapped spans were verified;
        // staged owns len writable bytes and outlives this synchronous RPM call.
        let raw = unsafe {
            ReadProcessMemory(
                self.process.handle.0,
                start as *const _,
                staged.as_mut_ptr() as *mut _,
                staged.len(),
                Some(&mut count),
            )
        };
        attempt.returned_bytes = count;
        attempt.native_error(&raw);
        let result = raw.map_err(|e| os_error("memory.runtime_read", e));
        // Preserve the actual RPM error even if it also overran control.
        budget.finish(result)?;
        if count != into.len() {
            return Err("memory.runtime_short_read".into());
        }
        attempt.reason = "post_read_guard";
        let mut at = start;
        let mut queries = 0;
        while at < end {
            if queries == pages {
                return Err("memory.runtime_mapping_budget".into());
            }
            let current = query(&self.process, at, self.page_bytes, budget)?;
            let issued = check_segment(
                &self.segments,
                current,
                at,
                end,
                self.identity.base,
                self.identity.base + self.identity.size,
                kind,
            )?;
            let observed = check_segment(
                &before,
                current,
                at,
                end,
                self.identity.base,
                self.identity.base + self.identity.size,
                kind,
            )?;
            at = issued.min(observed);
            queries += 1;
        }
        self.verify(budget)?;
        into.copy_from_slice(&staged);
        Ok(())
    }

    /// Ordered readable/executable spans from this OS-issued module plan. The
    /// recipe intersects them with its independently validated text section.
    /// No private-memory region is enumerated and no caller range expands it.
    pub fn code_readable_spans(
        &self,
        start: u64,
        end: u64,
        budget: &mut RuntimeBudget,
    ) -> Result<Vec<(u64, u64)>> {
        self.check_budget(budget)?;
        if start >= end || end > self.identity.size {
            return Err("memory.runtime_read_range".into());
        }
        self.verify(budget)?;
        budget.charge(RuntimeCost::allocation(
            self.segments.len() * size_of::<(u64, u64)>(),
        ))?;
        let mut spans: Vec<(u64, u64)> = Vec::with_capacity(self.segments.len());
        for segment in &self.segments {
            budget.charge(RuntimeCost::work(1))?;
            if !segment.eligible || !matches!(segment.region.protection & 0xff, 0x20 | 0x40 | 0x80)
            {
                continue;
            }
            let first = segment.start.saturating_sub(self.identity.base).max(start);
            let last = segment.end.saturating_sub(self.identity.base).min(end);
            if first >= last {
                continue;
            }
            if let Some(previous) = spans.last_mut() {
                if previous.1 == first {
                    previous.1 = last;
                    continue;
                }
            }
            spans.push((first, last));
        }
        self.verify(budget)?;
        Ok(spans)
    }

    /// One bounded code RPM, retaining an actual short prefix only after all
    /// original/current mapping and process/window guards pass. Ordinary RPM
    /// failures or unread-span eligibility changes are sparse observations;
    /// returned-prefix mapping, identity, control and budget failures remain
    /// fatal and expose no bytes. The bool
    /// reports a failed/short RPM independently of its usable prefix length.
    pub fn read_code_prefix(
        &self,
        rva: u64,
        into: &mut [u8],
        budget: &mut RuntimeBudget,
    ) -> Result<(usize, bool)> {
        self.check_budget(budget)?;
        if into.is_empty()
            || into.len() > MAX_READ
            || rva > self.identity.size
            || into.len() as u64 > self.identity.size - rva
        {
            return Err("memory.runtime_read_range".into());
        }
        let start = self.identity.base + rva;
        let end = start + into.len() as u64;
        let pages =
            (start % self.page_bytes + into.len() as u64).div_ceil(self.page_bytes) as usize;
        budget.admitted(RuntimeCost {
            allocation_bytes: (into.len() + pages * size_of::<Segment>()) as u64,
            query_calls: 2,
            work_units: into.len() as u64 * 2 + pages as u64 * 32,
            ..RuntimeCost::read(into.len())
        })?;
        budget.charge(RuntimeCost::work(into.len() as u64 * 2 + pages as u64 * 32))?;
        self.verify(budget)?;
        budget.charge(RuntimeCost::allocation(pages * size_of::<Segment>()))?;
        let mut before = Vec::with_capacity(pages);
        let mut at = start;
        while at < end {
            if before.len() == pages {
                return Err("memory.runtime_mapping_budget".into());
            }
            let current = query(&self.process, at, self.page_bytes, budget)?;
            let next = match check_segment(
                &self.segments,
                current,
                at,
                end,
                self.identity.base,
                self.identity.base + self.identity.size,
                RuntimeReadKind::Code,
            ) {
                Ok(next) => next,
                Err(cause) if unread_code_gap(&cause) => {
                    // Nothing was returned from this unread span. Its changed
                    // eligibility is a sparse gap, never permission to expose
                    // bytes. Identity/control failures remain fatal.
                    self.verify(budget)?;
                    budget.check()?;
                    return Ok((0, true));
                }
                Err(cause) => return Err(cause),
            };
            before.push(Segment {
                start: at,
                end: next,
                region: current,
                eligible: true,
            });
            at = next;
        }
        budget.charge(RuntimeCost::allocation(into.len()))?;
        let mut staged = vec![0; into.len()];
        budget.charge(RuntimeCost::read(into.len()))?;
        let mut count = 0;
        // SAFETY: the read-only process handle is held by self. start/end are
        // confined to its sealed mapped module, all current spans were checked,
        // and staged owns exactly len writable bytes until this synchronous call
        // finishes. No target pointer is dereferenced in this process.
        let result = unsafe {
            ReadProcessMemory(
                self.process.handle.0,
                start as *const _,
                staged.as_mut_ptr() as *mut _,
                staged.len(),
                Some(&mut count),
            )
        };
        let partial = result.as_ref().err().is_some_and(|error| {
            error.code() == windows::core::HRESULT::from_win32(ERROR_PARTIAL_COPY.0)
        });
        let received = qualified_code_prefix(count, into.len(), result.is_ok(), partial)?;
        let failed = result.is_err() || received != into.len();
        // Only acquired bytes need post-read eligibility; an unread suffix
        // may have changed protection and is handled by the sparse page pass.
        verify_code_prefix(
            &self.segments,
            &before,
            start,
            received,
            self.identity.base,
            self.identity.base + self.identity.size,
            pages,
            |at| query(&self.process, at, self.page_bytes, budget),
        )?;
        self.verify(budget)?;
        budget.check()?;
        into[..received].copy_from_slice(&staged[..received]);
        Ok((received, failed))
    }

    /// Read one already selected private-memory range, never enumerate the heap.
    /// The range is bounded to 1 MiB and one current private allocation. Every
    /// intersecting mapping must stay committed, readable and non-executable.
    /// The original module/window/process capability and request ledger remain
    /// mandatory. As with module reads, no bytes escape a failed post-guard.
    pub fn read_private_exact(
        &self,
        address: u64,
        into: &mut [u8],
        budget: &mut RuntimeBudget,
    ) -> Result<()> {
        self.read_private_observed(address, into, budget)
            .map_err(|failure| failure.cause)
    }
    pub fn read_private_observed(
        &self,
        address: u64,
        into: &mut [u8],
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<(), RuntimeReadFailure> {
        let mut attempt = ReadAttempt::default();
        self.read_private_inner(address, into, budget, &mut attempt)
            .map_err(|cause| attempt.fail(cause))
    }
    fn read_private_inner(
        &self,
        address: u64,
        into: &mut [u8],
        budget: &mut RuntimeBudget,
        attempt: &mut ReadAttempt,
    ) -> Result<()> {
        self.check_budget(budget)?;
        let (end, pages) = private_range(
            address,
            into.len(),
            self.address_minimum,
            self.address_maximum,
            self.identity.base,
            self.identity.size,
            self.page_bytes,
        )?;
        // Conservatively reserve zeroing/copying plus both bounded mapping
        // walks, including the binary search through observed pieces.
        let work = into.len() as u64 * 2 + pages as u64 * 32;
        budget.admitted(RuntimeCost {
            allocation_bytes: (into.len() + pages * size_of::<Segment>()) as u64,
            query_calls: 2,
            work_units: work,
            ..RuntimeCost::read(into.len())
        })?;
        budget.charge(RuntimeCost::work(work))?;
        self.verify(budget)?;
        budget.charge(RuntimeCost::allocation(pages * size_of::<Segment>()))?;
        let mut before = Vec::with_capacity(pages);
        let mut at = address;
        let mut allocation = None;
        while at < end {
            if before.len() == pages {
                return Err("memory.runtime_private_mapping_budget".into());
            }
            attempt.reason = "region_query_failed";
            let current =
                query_observed(&self.process, at, self.page_bytes, budget, Some(attempt))?;
            attempt.queried(current);
            let next = private_piece(
                current,
                at,
                end,
                self.address_minimum,
                self.address_maximum,
                allocation,
            )?;
            allocation = Some(current.allocation);
            attempt.eligible();
            before.push(Segment {
                start: at,
                end: next,
                region: current,
                eligible: true,
            });
            at = next;
        }
        budget.charge(RuntimeCost::allocation(into.len()))?;
        let mut staged = vec![0; into.len()];
        budget.charge(RuntimeCost::read(into.len()))?;
        let mut count = 0;
        attempt.io_attempted = true;
        attempt.reason = "short_read";
        // SAFETY: this selected range was verified in one private allocation;
        // staged owns len writable bytes and the read-only handle stays alive
        // throughout this synchronous call. The target pointer is never local.
        let raw = unsafe {
            ReadProcessMemory(
                self.process.handle.0,
                address as *const _,
                staged.as_mut_ptr() as *mut _,
                staged.len(),
                Some(&mut count),
            )
        };
        attempt.returned_bytes = count;
        attempt.native_error(&raw);
        let result = raw.map_err(|e| os_error("memory.runtime_private_read", e));
        budget.finish(result)?;
        if count != into.len() {
            return Err("memory.runtime_private_short_read".into());
        }
        attempt.reason = "post_read_guard";
        let mut at = address;
        let mut queries = 0;
        while at < end {
            if queries == pages {
                return Err("memory.runtime_private_mapping_budget".into());
            }
            let current = query(&self.process, at, self.page_bytes, budget)?;
            at = check_private_piece(
                &before,
                current,
                at,
                end,
                self.address_minimum,
                self.address_maximum,
            )?;
            queries += 1;
        }
        self.verify(budget)?;
        into.copy_from_slice(&staged);
        Ok(())
    }
}

fn qualified_code_prefix(
    count: usize,
    requested: usize,
    succeeded: bool,
    partial_copy: bool,
) -> Result<usize> {
    if count > requested {
        return Err("memory.runtime_invalid_read_count".into());
    }
    // An unrelated OS failure cannot authenticate its claimed output bytes.
    Ok(if succeeded || partial_copy { count } else { 0 })
}

fn image_directory_from_wide(
    image: &[u16],
    budget: &mut RuntimeBudget,
) -> Result<std::path::PathBuf> {
    if image.is_empty() || image.len() >= 32768 {
        return Err("memory.runtime_image_name".into());
    }
    budget.charge(RuntimeCost::work(image.len() as u64 * 8))?;
    let separator = image
        .iter()
        .rposition(|unit| [47, 92].contains(unit))
        .ok_or("memory.runtime_image_directory")?;
    let directory = &image[..=separator];
    // A UTF-16 unit needs at most three UTF-8 bytes. Allocate exactly this
    // requested capacity once; moving String into Windows OsString/PathBuf
    // reuses its UTF-8 buffer. Refuse ill-formed UTF-16 instead of lossy identity.
    budget.charge(RuntimeCost::allocation(directory.len() * 3))?;
    let mut text = String::with_capacity(directory.len() * 3);
    for (i, character) in std::char::decode_utf16(directory.iter().copied()).enumerate() {
        if i % 4096 == 0 {
            budget.check()?;
        }
        let character = character.map_err(|_| "memory.runtime_image_encoding")?;
        if character == '\0' {
            return Err("memory.runtime_image_encoding".into());
        }
        text.push(character);
    }
    let path = std::path::PathBuf::from(text);
    if !path.is_absolute() {
        return Err("memory.runtime_image_directory".into());
    }
    budget.check()?;
    Ok(path)
}

fn private_range(
    address: u64,
    bytes: usize,
    minimum: u64,
    maximum: u64,
    module_base: u64,
    module_size: u64,
    page: u64,
) -> Result<(u64, usize)> {
    let end = address
        .checked_add(bytes as u64)
        .ok_or("memory.runtime_private_range")?;
    if bytes == 0
        || bytes > MAX_READ
        || address < minimum
        || end - 1 > maximum
        || (address < module_base + module_size && module_base < end)
    {
        return Err("memory.runtime_private_range".into());
    }
    let pages = (address % page + bytes as u64).div_ceil(page) as usize;
    Ok((end, pages))
}

fn private_piece(
    current: Region,
    at: u64,
    end: u64,
    minimum: u64,
    maximum: u64,
    allocation: Option<u64>,
) -> Result<u64> {
    if current.start > at
        || current.end <= at
        || current.start < minimum
        || current.end - 1 > maximum
        || current.allocation < minimum
        || current.allocation > current.start
        || allocation.is_some_and(|a| current.allocation != a)
        || current.state != MEM_COMMIT.0
        || current.kind != MEM_PRIVATE.0
        || !readable(PAGE_PROTECTION_FLAGS(current.protection))
        || !matches!(current.protection & 0xff, 0x02 | 0x04 | 0x08)
    {
        return Err("memory.runtime_private_mapping".into());
    }
    Ok(end.min(current.end))
}

fn check_private_piece(
    before: &[Segment],
    current: Region,
    at: u64,
    end: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64> {
    let observed = before
        .get(before.partition_point(|s| s.end <= at))
        .filter(|s| s.start <= at)
        .ok_or("memory.runtime_private_mapping_changed")?;
    let next = private_piece(
        current,
        at,
        end,
        minimum,
        maximum,
        Some(observed.region.allocation),
    )?;
    if current.protection != observed.region.protection
        || current.state != observed.region.state
        || current.kind != observed.region.kind
    {
        return Err("memory.runtime_private_mapping_changed".into());
    }
    Ok(next.min(observed.end))
}

fn os_error(operation: &str, error: windows::core::Error) -> String {
    // Numeric formatting avoids an unbudgeted FormatMessage lookup.
    format!("{operation}: hresult=0x{:08x}", error.code().0 as u32)
}
fn verify_process(process: &Process, budget: &mut RuntimeBudget) -> Result<()> {
    budget.charge(RuntimeCost::os())?;
    let state = unsafe { WaitForSingleObject(process.handle.0, 0) };
    budget.finish(if state == WAIT_TIMEOUT {
        Ok(())
    } else if state == WAIT_FAILED {
        Err(os_error(
            "process.wait",
            windows::core::Error::from_thread(),
        ))
    } else {
        Err("process.exited".into())
    })?;
    budget.charge(RuntimeCost::os())?;
    let (mut c, mut e, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let result = unsafe { GetProcessTimes(process.handle.0, &mut c, &mut e, &mut k, &mut u) }
        .map_err(|e| os_error("process.times", e));
    budget.finish(result)?;
    let created = (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64;
    if created != process.identity.created {
        return Err("process.identity_changed".into());
    }
    Ok(())
}
fn verify_window(
    process: &Process,
    window: &WindowIdentity,
    budget: &mut RuntimeBudget,
) -> Result<()> {
    verify_process(process, budget)?;
    budget.charge(RuntimeCost::os())?;
    let hwnd = HWND(window.hwnd as *mut _);
    let exists = unsafe { IsWindow(Some(hwnd)).as_bool() };
    budget.finish(if exists {
        Ok(())
    } else {
        Err("window.closed".into())
    })?;
    budget.charge(RuntimeCost::os())?;
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let result = if thread == 0 {
        Err(os_error(
            "window.owner",
            windows::core::Error::from_thread(),
        ))
    } else if pid != process.identity.pid {
        Err("window.reused".into())
    } else {
        Ok(())
    };
    budget.finish(result)?;
    verify_process(process, budget)
}
fn architecture(process: &Process, budget: &mut RuntimeBudget) -> Result<u16> {
    budget.charge(RuntimeCost::os())?;
    let mut target = IMAGE_FILE_MACHINE::default();
    let mut native = IMAGE_FILE_MACHINE::default();
    let result = unsafe { IsWow64Process2(process.handle.0, &mut target, Some(&mut native)) }
        .map_err(|e| os_error("process.architecture", e));
    budget.finish(result)?;
    let machine = if target == IMAGE_FILE_MACHINE_UNKNOWN {
        native
    } else {
        target
    };
    if machine != IMAGE_FILE_MACHINE_AMD64 {
        return Err("memory.runtime_architecture".into());
    }
    Ok(machine.0)
}
fn inventory(process: &Process, budget: &mut RuntimeBudget) -> Result<(u64, u64)> {
    // EnumProcessModulesEx returns the first module even with this one-entry
    // buffer. needed is the whole inventory size and remains explicitly bounded.
    budget.charge(RuntimeCost::os())?;
    let mut first = HMODULE::default();
    let mut needed = 0;
    let result = unsafe {
        EnumProcessModulesEx(
            process.handle.0,
            &mut first,
            size_of::<HMODULE>() as u32,
            &mut needed,
            LIST_MODULES_ALL,
        )
    }
    .map_err(|e| os_error("memory.runtime_inventory", e));
    budget.finish(result)?;
    if needed == 0
        || needed as usize > MAX_MODULES * size_of::<HMODULE>()
        || !(needed as usize).is_multiple_of(size_of::<HMODULE>())
        || first.0.is_null()
    {
        return Err("memory.runtime_inventory_budget".into());
    }
    budget.charge(RuntimeCost::os())?;
    let mut info = MODULEINFO::default();
    let result = unsafe {
        GetModuleInformation(
            process.handle.0,
            first,
            &mut info,
            size_of::<MODULEINFO>() as u32,
        )
    }
    .map_err(|e| os_error("memory.runtime_inventory", e));
    budget.finish(result)?;
    if info.lpBaseOfDll as u64 != first.0 as u64 {
        return Err("memory.runtime_inventory_identity".into());
    }
    Ok((info.lpBaseOfDll as u64, info.SizeOfImage as u64))
}
/// Same fixed-Go SimpleFold routine as selected input identity. These are
/// counted OS UTF-16 paths (no terminator); invalid units become RuneError, with
/// no normalization/full fold. At most 32768 units per path and <=16 work units
/// per unit cover decode plus binary search in the pinned <2048-entry table.
fn equal_fold_image_path(left: &[u16], right: &[u16], budget: &mut RuntimeBudget) -> Result<bool> {
    if left.len() > 32768 || right.len() > 32768 {
        return Err("memory.runtime_image_name".into());
    }
    budget.charge(RuntimeCost::work((left.len() + right.len()) as u64 * 16))?;
    let equal = crate::mailbox_input::equal_fold_utf16(left, right);
    budget.check()?;
    Ok(equal)
}

fn confirm_main_image(
    process: &Process,
    base: u64,
    budget: &mut RuntimeBudget,
) -> Result<Vec<u16>> {
    // These are OS-reported names only. No path is opened or used as an RVA gate.
    const CHARACTERS: usize = 32768;
    budget.charge(RuntimeCost::allocation(2 * CHARACTERS * size_of::<u16>()))?;
    let mut image = vec![0u16; CHARACTERS];
    let mut module = vec![0u16; CHARACTERS];
    budget.charge(RuntimeCost::os())?;
    let mut length = CHARACTERS as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process.handle.0,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(image.as_mut_ptr()),
            &mut length,
        )
    }
    .map_err(|e| os_error("process.image_name", e));
    budget.finish(result)?;
    if length == 0 || length as usize >= CHARACTERS {
        return Err("memory.runtime_image_name".into());
    }
    budget.charge(RuntimeCost::os())?;
    let module_length = unsafe {
        GetModuleFileNameExW(
            Some(process.handle.0),
            Some(HMODULE(base as *mut _)),
            &mut module,
        )
    };
    let result = if module_length == 0 {
        Err(os_error(
            "process.module_name",
            windows::core::Error::from_thread(),
        ))
    } else if module_length as usize >= CHARACTERS {
        Err("memory.runtime_module_name".into())
    } else {
        Ok(())
    };
    budget.finish(result)?;
    let equal = equal_fold_image_path(
        &image[..length as usize],
        &module[..module_length as usize],
        budget,
    )?;
    budget.finish(if equal {
        Ok(())
    } else {
        Err("memory.runtime_main_module_identity".into())
    })?;
    image.truncate(length as usize);
    Ok(image)
}
fn query(process: &Process, at: u64, page: u64, budget: &mut RuntimeBudget) -> Result<Region> {
    query_observed(process, at, page, budget, None)
}
fn query_observed(
    process: &Process,
    at: u64,
    page: u64,
    budget: &mut RuntimeBudget,
    attempt: Option<&mut ReadAttempt>,
) -> Result<Region> {
    query_with(at, page, budget, attempt, |info| {
        // SAFETY: the process owns a held read-only handle; info is one writable
        // initialized structure alive for this synchronous call. at is only a
        // remote address, never a locally dereferenced pointer.
        let count = unsafe {
            VirtualQueryEx(
                process.handle.0,
                Some(at as *const _),
                info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        let error = (count == 0).then(windows::core::Error::from_thread);
        (count, error)
    })
}
fn query_with(
    at: u64,
    page: u64,
    budget: &mut RuntimeBudget,
    attempt: Option<&mut ReadAttempt>,
    call: impl FnOnce(&mut MEMORY_BASIC_INFORMATION) -> (usize, Option<windows::core::Error>),
) -> Result<Region> {
    budget.charge(RuntimeCost {
        query_calls: 1,
        ..RuntimeCost::os()
    })?;
    let mut info = MEMORY_BASIC_INFORMATION::default();
    let (count, error) = call(&mut info);
    let result = if count == 0 {
        if let Some(attempt) = attempt {
            attempt.reason = "region_query_failed";
            attempt.native_error_code = error.as_ref().and_then(win32_error_code);
        }
        Err(match error {
            Some(error) => os_error("memory.runtime_query", error),
            None => "memory.runtime_query_failed_without_error".into(),
        })
    } else if count != size_of::<MEMORY_BASIC_INFORMATION>() {
        Err("memory.runtime_query_size".into())
    } else {
        Ok(())
    };
    budget.finish(result)?;
    let start = info.BaseAddress as u64;
    let end = start
        .checked_add(info.RegionSize as u64)
        .ok_or("memory.runtime_mapping_geometry")?;
    if start > at
        || end <= at
        || !start.is_multiple_of(page)
        || !(info.RegionSize as u64).is_multiple_of(page)
    {
        return Err("memory.runtime_mapping_geometry".into());
    }
    Ok(Region {
        start,
        end,
        allocation: info.AllocationBase as u64,
        state: info.State.0,
        kind: info.Type.0,
        protection: info.Protect.0,
    })
}
fn eligible(region: Region, base: u64, end: u64) -> bool {
    region.state == MEM_COMMIT.0
        && (region.kind == MEM_IMAGE.0 || region.kind == MEM_MAPPED.0)
        && readable(PAGE_PROTECTION_FLAGS(region.protection))
        && region.allocation >= base
        && region.allocation < end
        && region.allocation <= region.start
}
fn check_segment(
    plan: &[Segment],
    current: Region,
    at: u64,
    end: u64,
    base: u64,
    module_end: u64,
    kind: RuntimeReadKind,
) -> Result<u64> {
    let issued = plan
        .get(plan.partition_point(|s| s.end <= at))
        .filter(|s| s.start <= at)
        .ok_or("memory.runtime_mapping_hole")?;
    if !issued.eligible
        || !eligible(current, base, module_end)
        || at < base
        || end > module_end
        || current.start > at
        || current.end <= at
        || current.allocation != issued.region.allocation
        || current.kind != issued.region.kind
        || current.state != issued.region.state
        || current.protection != issued.region.protection
    {
        return Err("memory.runtime_mapping_changed".into());
    }
    let protection = current.protection & 0xff;
    let permission = match kind {
        RuntimeReadKind::Metadata => true,
        RuntimeReadKind::Code => matches!(protection, 0x20 | 0x40 | 0x80),
        RuntimeReadKind::WritableData => matches!(protection, 0x04 | 0x08),
    };
    if !permission {
        return Err("memory.runtime_mapping_role".into());
    }
    Ok(end.min(issued.end).min(current.end))
}

#[cfg(test)]
mod tests;
