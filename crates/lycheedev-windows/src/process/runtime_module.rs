//! OS-issued main-module reads for runtime instruction recipes. No file is
//! opened, read or hashed. This capability is separate from the historical
//! disk-backed ModulePlan and from private-heap discovery.
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
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
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
            || limits.work_units > 8 << 30
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
/// are private; only RuntimeModule can issue one. No executable digest is used.
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
            os_call_units: 3,
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
        let handle = budget.finish(opened)?;
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
        let module = Self {
            process,
            identity: RuntimeModuleIdentity {
                serial: scope_id()?,
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
        module.verify(budget)?;
        Ok(module)
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
            let current = query(&self.process, at, self.page_bytes, budget)?;
            let next = check_segment(
                &self.segments,
                current,
                at,
                end,
                self.identity.base,
                self.identity.base + self.identity.size,
                kind,
            )?;
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
        let result = unsafe {
            ReadProcessMemory(
                self.process.handle.0,
                start as *const _,
                staged.as_mut_ptr() as *mut _,
                staged.len(),
                Some(&mut count),
            )
        }
        .map_err(|e| os_error("memory.runtime_read", e));
        // Preserve the actual RPM error even if it also overran control.
        budget.finish(result)?;
        if count != into.len() {
            return Err("memory.runtime_short_read".into());
        }
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
            let current = query(&self.process, at, self.page_bytes, budget)?;
            let next = private_piece(
                current,
                at,
                end,
                self.address_minimum,
                self.address_maximum,
                allocation,
            )?;
            allocation = Some(current.allocation);
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
        let result = unsafe {
            ReadProcessMemory(
                self.process.handle.0,
                address as *const _,
                staged.as_mut_ptr() as *mut _,
                staged.len(),
                Some(&mut count),
            )
        }
        .map_err(|e| os_error("memory.runtime_private_read", e));
        budget.finish(result)?;
        if count != into.len() {
            return Err("memory.runtime_private_short_read".into());
        }
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
    budget.charge(RuntimeCost::os())?;
    let equal = unsafe {
        CompareStringOrdinal(
            &image[..length as usize],
            &module[..module_length as usize],
            true,
        )
    };
    budget.finish(if equal == CSTR_EQUAL {
        Ok(())
    } else if equal.0 == 0 {
        Err(os_error(
            "process.module_name_compare",
            windows::core::Error::from_thread(),
        ))
    } else {
        Err("memory.runtime_main_module_identity".into())
    })?;
    image.truncate(length as usize);
    Ok(image)
}
fn query(process: &Process, at: u64, page: u64, budget: &mut RuntimeBudget) -> Result<Region> {
    budget.charge(RuntimeCost {
        query_calls: 1,
        ..RuntimeCost::os()
    })?;
    let mut info = MEMORY_BASIC_INFORMATION::default();
    let count = unsafe {
        VirtualQueryEx(
            process.handle.0,
            Some(at as *const _),
            &mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    let result = if count == 0 {
        Err(os_error(
            "memory.runtime_query",
            windows::core::Error::from_thread(),
        ))
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
