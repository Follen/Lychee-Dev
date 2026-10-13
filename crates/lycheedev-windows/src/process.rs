use crate::Result;
mod runtime_module;
pub use runtime_module::{
    RuntimeBudget, RuntimeControl, RuntimeCost, RuntimeLimits, RuntimeModule,
    RuntimeModuleIdentity, RuntimeReadKind, RuntimeUsage,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    marker::PhantomData,
    os::windows::fs::OpenOptionsExt,
    rc::Rc,
    time::Instant,
};
use windows::{
    Win32::{
        Foundation::*,
        System::{
            Diagnostics::{Debug::ReadProcessMemory, ToolHelp::*},
            Memory::*,
            ProcessStatus::*,
            SystemInformation::{GetNativeSystemInfo, SYSTEM_INFO},
            Threading::*,
        },
        UI::WindowsAndMessaging::*,
    },
    core::PCWSTR,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub created: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProcessMemory {
    pub identity: ProcessIdentity,
    pub private_commit: u64,
    pub working_set: u64,
    pub peak_working_set: u64,
    pub peak_private_commit: u64,
    pub scope: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WindowIdentity {
    pub process: ProcessIdentity,
    pub hwnd: usize,
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub struct Process {
    handle: Handle,
    identity: ProcessIdentity,
}
impl Process {
    pub fn memory(&self) -> Result<ProcessMemory> {
        self.verify()?;
        let mut memory = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        unsafe {
            GetProcessMemoryInfo(
                self.handle.0,
                &mut memory as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                std::mem::size_of_val(&memory) as u32,
            )
            .map_err(|e| e.to_string())?;
        }
        self.verify()?;
        Ok(ProcessMemory{identity:self.identity.clone(),private_commit:memory.PrivateUsage as u64,working_set:memory.WorkingSetSize as u64,peak_working_set:memory.PeakWorkingSetSize as u64,peak_private_commit:memory.PeakPagefileUsage as u64,
            scope:"held process handle; GetProcessMemoryInfo PROCESS_MEMORY_COUNTERS_EX; PrivateUsage commit distinct from WorkingSetSize RSS".into()})
    }
    pub fn parent_pid(&self) -> Result<u32> {
        self.verify()?;
        unsafe {
            let snapshot =
                Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| e.to_string())?);
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            Process32FirstW(snapshot.0, &mut entry).map_err(|e| e.to_string())?;
            for _ in 0..65536 {
                if entry.th32ProcessID == self.identity.pid {
                    self.verify()?;
                    if entry.th32ParentProcessID == 0 {
                        return Err("process.parent_missing".into());
                    }
                    return Ok(entry.th32ParentProcessID);
                }
                if Process32NextW(snapshot.0, &mut entry).is_err() {
                    return Err("process.parent_missing".into());
                }
            }
            Err("process.inventory_budget".into())
        }
    }
    pub fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }
    /// Await this exact already-opened OS process object. Never reopen a PID or terminate it.
    /// Atomic cancellation has no kernel event, so wait slices have a bounded 50ms wake cost.
    pub fn wait_exit(
        &self,
        deadline: std::time::Instant,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<u32> {
        if unsafe { identity(self.handle.0, self.identity.pid)? } != self.identity {
            return Err("process.identity_changed".into());
        }
        loop {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Err("command.cancelled: process exit wait".into());
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err("command.deadline: process exit wait".into());
            }
            let state =
                unsafe { WaitForSingleObject(self.handle.0, left.as_millis().clamp(1, 50) as u32) };
            if state == WAIT_OBJECT_0 {
                let mut exit = 0;
                unsafe {
                    GetExitCodeProcess(self.handle.0, &mut exit).map_err(|e| e.to_string())?;
                }
                return Ok(exit);
            }
            if state != WAIT_TIMEOUT {
                return Err("process.wait_failed".into());
            }
        }
    }
    pub fn open(pid: u32) -> Result<Self> {
        unsafe {
            let handle = Handle(
                OpenProcess(
                    PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
                .map_err(|e| e.to_string())?,
            );
            let identity = identity(handle.0, pid)?;
            Ok(Self { handle, identity })
        }
    }
    pub fn verify(&self) -> Result<()> {
        unsafe {
            if WaitForSingleObject(self.handle.0, 0) != WAIT_TIMEOUT {
                return Err("process.exited".into());
            }
            if identity(self.handle.0, self.identity.pid)? != self.identity {
                return Err("process.identity_changed".into());
            }
            Ok(())
        }
    }
    pub fn image_path(&self) -> Result<std::path::PathBuf> {
        unsafe {
            self.verify()?;
            let mut text = vec![0u16; 32768];
            let mut len = text.len() as u32;
            QueryFullProcessImageNameW(
                self.handle.0,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(text.as_mut_ptr()),
                &mut len,
            )
            .map_err(|e| e.to_string())?;
            Ok(std::path::PathBuf::from(String::from_utf16_lossy(
                &text[..len as usize],
            )))
        }
    }
    pub fn read_private(&self, address: u64, bytes: &mut [u8]) -> Result<()> {
        self.read_kind(address, bytes, false, None)
    }
    /// Only an OS-inventoried main module can issue a sealed read capability.
    pub fn main_module(&self) -> Result<ModulePlan> {
        unsafe {
            self.verify()?;
            let mut modules = vec![HMODULE::default(); 4096];
            let mut needed = 0;
            EnumProcessModulesEx(
                self.handle.0,
                modules.as_mut_ptr(),
                (modules.len() * std::mem::size_of::<HMODULE>()) as u32,
                &mut needed,
                LIST_MODULES_ALL,
            )
            .map_err(|e| e.to_string())?;
            if needed == 0 || needed as usize > modules.len() * std::mem::size_of::<HMODULE>() {
                return Err("memory.module_inventory_budget".into());
            }
            let main = modules[0];
            let mut info = MODULEINFO::default();
            GetModuleInformation(
                self.handle.0,
                main,
                &mut info,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(|e| e.to_string())?;
            let mut file = OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(self.image_path()?)
                .map_err(|e| e.to_string())?;
            let size = file.metadata().map_err(|e| e.to_string())?.len();
            if size > 512 << 20 {
                return Err("memory.module_file_budget".into());
            }
            let mut disk = Vec::with_capacity(size as usize);
            file.read_to_end(&mut disk).map_err(|e| e.to_string())?;
            let hash = hex_hash(&disk);
            self.module_plan(
                info.lpBaseOfDll as u64,
                info.SizeOfImage as u64,
                file,
                disk,
                hash,
            )
        }
    }
    fn module_plan(
        &self,
        base: u64,
        size: u64,
        file: File,
        disk: Vec<u8>,
        hash: String,
    ) -> Result<ModulePlan> {
        if size == 0 || size > 512 << 20 {
            return Err("memory.module_size".into());
        }
        let end = base.checked_add(size).ok_or("memory.overflow")?;
        let mut at = base;
        let mut ranges = Vec::new();
        while at < end {
            let m = self.query(at)?;
            let next = (m.BaseAddress as u64)
                .checked_add(m.RegionSize as u64)
                .ok_or("memory.overflow")?;
            if next <= at {
                return Err("memory.mapping_geometry".into());
            }
            if m.State == MEM_COMMIT
                && (m.Type == MEM_IMAGE || m.Type == MEM_MAPPED)
                && readable(m.Protect)
            {
                ranges.push(Mapping {
                    start: at,
                    end: next.min(end),
                    allocation: m.AllocationBase as u64,
                    kind: m.Type.0,
                    protection: m.Protect.0,
                });
            }
            at = next.min(end);
            if ranges.len() > 131072 {
                return Err("memory.mapping_budget".into());
            }
        }
        self.verify()?;
        Ok(ModulePlan {
            process: self.identity.clone(),
            base,
            size,
            ranges,
            _file: file,
            disk,
            hash,
        })
    }
    pub fn read_module(&self, plan: &ModulePlan, address: u64, bytes: &mut [u8]) -> Result<()> {
        if plan.process != self.identity
            || address < plan.base
            || address
                .checked_add(bytes.len() as u64)
                .is_none_or(|end| end > plan.base + plan.size)
        {
            return Err("memory.module_capability".into());
        }
        self.read_kind(address, bytes, true, Some(plan))
    }
    fn query(&self, address: u64) -> Result<MEMORY_BASIC_INFORMATION> {
        unsafe {
            let mut m = MEMORY_BASIC_INFORMATION::default();
            if VirtualQueryEx(
                self.handle.0,
                Some(address as *const _),
                &mut m,
                std::mem::size_of_val(&m),
            ) == 0
            {
                return Err("memory.query_failed".into());
            }
            Ok(m)
        }
    }
    fn read_kind(
        &self,
        address: u64,
        bytes: &mut [u8],
        module: bool,
        plan: Option<&ModulePlan>,
    ) -> Result<()> {
        if address == 0 || bytes.is_empty() || address.checked_add(bytes.len() as u64).is_none() {
            return Err("memory.address".into());
        }
        self.verify()?;
        let mut offset = 0;
        while offset < bytes.len() {
            let at = address + offset as u64;
            let before = self.query(at)?;
            if before.State != MEM_COMMIT
                || !readable(before.Protect)
                || (!module && before.Type != MEM_PRIVATE)
            {
                return Err("memory.mapping_denied".into());
            }
            let mut end = before.BaseAddress as u64 + before.RegionSize as u64;
            if let Some(p) = plan {
                let issued = p
                    .ranges
                    .get(p.ranges.partition_point(|r| r.end <= at))
                    .filter(|r| r.start <= at && at < r.end)
                    .ok_or("memory.module_hole")?;
                if issued.allocation != before.AllocationBase as u64
                    || issued.kind != before.Type.0
                    || issued.protection != before.Protect.0
                {
                    return Err("memory.mapping_changed".into());
                }
                end = end.min(issued.end);
            }
            let n = (end - at).min((bytes.len() - offset) as u64) as usize;
            if n == 0 {
                return Err("memory.mapping_geometry".into());
            }
            let mut read = 0;
            unsafe {
                ReadProcessMemory(
                    self.handle.0,
                    at as *const _,
                    bytes[offset..].as_mut_ptr() as *mut _,
                    n,
                    Some(&mut read),
                )
                .map_err(|e| e.to_string())?;
            }
            if read != n {
                return Err("memory.short_read".into());
            }
            let after = self.query(at)?;
            if before.AllocationBase != after.AllocationBase
                || before.State != after.State
                || before.Type != after.Type
                || before.Protect != after.Protect
                || (after.BaseAddress as u64 + after.RegionSize as u64) < at + n as u64
            {
                return Err("memory.mapping_changed".into());
            }
            offset += n;
        }
        self.verify()
    }
}
fn readable(p: PAGE_PROTECTION_FLAGS) -> bool {
    p.0 & PAGE_GUARD.0 == 0 && matches!(p.0 & 0xff, 0x02 | 0x04 | 0x08 | 0x20 | 0x40 | 0x80)
}

/// Runtime-record discovery uses this separate, bounded API. Existing module
/// reads and legacy exact private reads retain their original behavior.
pub const PRIVATE_PREFIX_MAX_BYTES: usize = 16 << 20;

#[derive(Clone, Copy, Debug)]
pub struct PrivateReadLimits {
    /// Reserved upper bound for target/bounds/window API calls, including one
    /// possible GetLastError per operation and CloseHandle for an opened handle.
    /// Clock reads are not target-I/O operations. Unused reservations stay charged.
    pub os_call_units: u64,
    pub query_calls: u64,
    pub read_calls: u64,
    pub requested_bytes: u64,
}
impl Default for PrivateReadLimits {
    fn default() -> Self {
        Self {
            os_call_units: 4_000_000,
            query_calls: 400_000,
            read_calls: 200_000,
            requested_bytes: 64 << 30,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrivateReadUsage {
    pub os_call_units: u64,
    /// Admitted map queries, including a reservation cancelled before issue.
    pub query_calls: u64,
    /// Admitted physical reads, including a reservation cancelled before issue.
    pub read_calls: u64,
    /// Reserved physical RPM bytes; failed/partial/deadline attempts stay charged.
    pub requested_bytes: u64,
}

/// Mutable ownership keeps all calls, including mapping rechecks, in one ledger.
/// Limits never reset within this value. No timer can preempt a synchronous
/// Windows syscall; deadlines are checked immediately before/after each call.
pub struct PrivateReadBudget {
    limits: PrivateReadLimits,
    usage: PrivateReadUsage,
}
#[derive(Clone, Copy)]
enum PrivateCall {
    Other,
    Open,
    Query,
    Read(usize),
}
impl PrivateReadBudget {
    pub fn new(limits: PrivateReadLimits) -> Result<Self> {
        if limits.os_call_units > 8_000_000
            || limits.query_calls > 1_000_000
            || limits.read_calls > 1_000_000
            || limits.requested_bytes > 128 << 30
        {
            return Err("memory.private_budget_options".into());
        }
        Ok(Self {
            limits,
            usage: PrivateReadUsage::default(),
        })
    }
    pub fn usage(&self) -> PrivateReadUsage {
        self.usage
    }
    /// Admission before an adapter allocates an output staging buffer. This
    /// does not reserve/reset counters or promise that later guards can finish;
    /// every actual operation must still charge its own budget. Requiring room
    /// for the requested span is deliberately conservative for partial prefixes.
    pub fn preflight_private_read(&self, bytes: usize, deadline: Instant) -> Result<()> {
        private_deadline(deadline)?;
        if bytes > PRIVATE_PREFIX_MAX_BYTES {
            return Err("memory.private_read_size".into());
        }
        if bytes == 0 {
            return Ok(());
        }
        if self.usage.query_calls >= self.limits.query_calls {
            return Err("memory.private_query_budget".into());
        }
        if self.usage.read_calls >= self.limits.read_calls {
            return Err("memory.private_read_budget".into());
        }
        if bytes as u64 > self.limits.requested_bytes - self.usage.requested_bytes {
            return Err("memory.private_byte_budget".into());
        }
        if self.limits.os_call_units - self.usage.os_call_units < 2 {
            return Err("memory.private_os_call_budget".into());
        }
        Ok(())
    }
    fn charge(&mut self, call: PrivateCall, deadline: Instant) -> Result<()> {
        private_deadline(deadline)?;
        let units = if matches!(call, PrivateCall::Open) {
            3
        } else {
            2
        };
        if units > self.limits.os_call_units - self.usage.os_call_units {
            return Err("memory.private_os_call_budget".into());
        }
        if matches!(call, PrivateCall::Query) && self.usage.query_calls >= self.limits.query_calls {
            return Err("memory.private_query_budget".into());
        }
        if let PrivateCall::Read(bytes) = call {
            if self.usage.read_calls >= self.limits.read_calls {
                return Err("memory.private_read_budget".into());
            }
            if bytes as u64 > self.limits.requested_bytes - self.usage.requested_bytes {
                return Err("memory.private_byte_budget".into());
            }
            self.usage.read_calls += 1;
            self.usage.requested_bytes += bytes as u64;
        }
        if matches!(call, PrivateCall::Query) {
            self.usage.query_calls += 1;
        }
        self.usage.os_call_units += units;
        Ok(())
    }
}

fn private_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        Err("memory.private_deadline".into())
    } else {
        Ok(())
    }
}
fn private_finish<T>(result: Result<T>, deadline: Instant) -> Result<T> {
    let time = private_deadline(deadline);
    // Preserve an actual OS failure if the call also exhausted the deadline.
    let value = result?;
    time?;
    Ok(value)
}
fn private_os_error(operation: &str, error: windows::core::Error) -> String {
    // Numeric formatting retains the exact cause without a hidden FormatMessage call.
    format!("{operation}: hresult=0x{:08x}", error.code().0 as u32)
}

#[derive(Clone, Copy, Debug)]
pub struct AddressSpace {
    maximum_application_address: u64,
    page_bytes: u32,
}
impl AddressSpace {
    pub fn native(budget: &mut PrivateReadBudget, deadline: Instant) -> Result<Self> {
        budget.charge(PrivateCall::Other, deadline)?;
        let mut info = SYSTEM_INFO::default();
        unsafe {
            GetNativeSystemInfo(&mut info);
        }
        private_deadline(deadline)?;
        let maximum = info.lpMaximumApplicationAddress as u64;
        if std::mem::size_of::<usize>() != 8
            || maximum == 0
            || maximum == u64::MAX
            || !info.dwPageSize.is_power_of_two()
            || !(4096..=65536).contains(&info.dwPageSize)
        {
            return Err("memory.private_address_space".into());
        }
        Ok(Self {
            maximum_application_address: maximum,
            page_bytes: info.dwPageSize,
        })
    }
    pub fn maximum_application_address(&self) -> u64 {
        self.maximum_application_address
    }
    pub fn page_bytes(&self) -> u32 {
        self.page_bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryRegion {
    pub start: u64,
    pub end: u64,
    pub allocation: u64,
    pub allocation_protection: u32,
    pub protection: u32,
    pub state: u32,
    pub kind: u32,
}
impl MemoryRegion {
    pub fn private(&self) -> bool {
        self.kind == MEM_PRIVATE.0
    }
    pub fn committed(&self) -> bool {
        self.state == MEM_COMMIT.0
    }
    pub fn readable(&self) -> bool {
        readable(PAGE_PROTECTION_FLAGS(self.protection))
    }
    fn eligible(&self) -> bool {
        self.private() && self.committed() && self.readable()
    }
}

impl Process {
    /// Same read-only process rights/identity as open(), with per-call accounting.
    pub fn open_until(pid: u32, budget: &mut PrivateReadBudget, deadline: Instant) -> Result<Self> {
        if pid == 0 {
            return Err("process.identity_required".into());
        }
        budget.charge(PrivateCall::Open, deadline)?;
        let opened = unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .map(Handle)
        .map_err(|error| private_os_error("process.open", error));
        let handle = private_finish(opened, deadline)?;
        let identity = private_identity(handle.0, pid, budget, deadline)?;
        let process = Self { handle, identity };
        process.verify_until(budget, deadline)?;
        Ok(process)
    }
    pub fn verify_until(&self, budget: &mut PrivateReadBudget, deadline: Instant) -> Result<()> {
        budget.charge(PrivateCall::Other, deadline)?;
        let state = unsafe { WaitForSingleObject(self.handle.0, 0) };
        let checked = if state == WAIT_TIMEOUT {
            Ok(())
        } else if state == WAIT_FAILED {
            Err(private_os_error(
                "process.wait",
                windows::core::Error::from_thread(),
            ))
        } else {
            Err("process.exited".into())
        };
        private_finish(checked, deadline)?;
        if private_identity(self.handle.0, self.identity.pid, budget, deadline)? != self.identity {
            return Err("process.identity_changed".into());
        }
        Ok(())
    }
    /// Reuses the held process handle rather than reopening a PID. A HWND is an
    /// OS identity check, not proof of runtime continuity or protocol authority.
    pub fn verify_window_until(
        &self,
        window: &WindowIdentity,
        budget: &mut PrivateReadBudget,
        deadline: Instant,
    ) -> Result<()> {
        if window.process != self.identity || window.hwnd == 0 {
            return Err("window.process_reused".into());
        }
        self.verify_until(budget, deadline)?;
        let hwnd = HWND(window.hwnd as *mut _);
        budget.charge(PrivateCall::Other, deadline)?;
        let exists = unsafe { IsWindow(Some(hwnd)).as_bool() };
        private_finish(
            if exists {
                Ok(())
            } else {
                Err("window.closed".into())
            },
            deadline,
        )?;
        budget.charge(PrivateCall::Other, deadline)?;
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let owner = if thread == 0 {
            Err(private_os_error(
                "window.owner",
                windows::core::Error::from_thread(),
            ))
        } else if pid != self.identity.pid {
            Err("window.reused".into())
        } else {
            Ok(())
        };
        private_finish(owner, deadline)?;
        self.verify_until(budget, deadline)
    }
    /// Exactly one map query, with held-process checks before/after. None means
    /// ERROR_INVALID_PARAMETER *beyond* the OS maximum application address only.
    pub fn query_region_until(
        &self,
        address: u64,
        space: &AddressSpace,
        budget: &mut PrivateReadBudget,
        deadline: Instant,
    ) -> Result<Option<MemoryRegion>> {
        self.verify_until(budget, deadline)?;
        let region = self.private_region(address, space, budget, deadline)?;
        self.verify_until(budget, deadline)?;
        Ok(region)
    }
    fn private_region(
        &self,
        address: u64,
        space: &AddressSpace,
        budget: &mut PrivateReadBudget,
        deadline: Instant,
    ) -> Result<Option<MemoryRegion>> {
        budget.charge(PrivateCall::Query, deadline)?;
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let size = unsafe {
            VirtualQueryEx(
                self.handle.0,
                Some(address as *const _),
                &mut info,
                std::mem::size_of_val(&info),
            )
        };
        let result = if size == 0 {
            let error = unsafe { GetLastError() };
            private_query_failure(error, address, space.maximum_application_address)
        } else if size != std::mem::size_of_val(&info) {
            Err("memory.private_query_size".into())
        } else {
            let start = info.BaseAddress as u64;
            match start.checked_add(info.RegionSize as u64) {
                Some(end) if start <= address && end > address => Ok(Some(MemoryRegion {
                    start,
                    end,
                    allocation: info.AllocationBase as u64,
                    allocation_protection: info.AllocationProtect.0,
                    protection: info.Protect.0,
                    state: info.State.0,
                    kind: info.Type.0,
                })),
                _ => Err("memory.private_mapping_geometry".into()),
            }
        };
        private_finish(result, deadline)
    }
    /// Read the currently eligible prefix with at most ONE physical RPM call.
    /// Pre/post queries are capped at 2 * ceil((page offset + len) / page size),
    /// at most 8194 for the 16 MiB limit and the minimum supported 4096-byte page.
    /// They also debit the cumulative query/OS ledger. No hidden page retries.
    /// A post-read mapping change is fatal and releases no prefix. Concurrent
    /// unmap/remap ABA is not an atomic snapshot; full protocol checks remain required.
    pub fn read_private_prefix_until(
        &self,
        address: u64,
        bytes: &mut [u8],
        space: &AddressSpace,
        budget: &mut PrivateReadBudget,
        deadline: Instant,
    ) -> Result<usize> {
        self.verify_until(budget, deadline)?;
        if bytes.len() > PRIVATE_PREFIX_MAX_BYTES {
            return Err("memory.private_read_size".into());
        }
        let end = address
            .checked_add(bytes.len() as u64)
            .ok_or("memory.private_address_overflow")?;
        if bytes.is_empty() {
            return Ok(0);
        }
        let page = space.page_bytes as u64;
        let max_regions = ((address % page + bytes.len() as u64).div_ceil(page)) as usize;
        if max_regions > 4097 {
            return Err("memory.private_page_budget".into());
        }
        let mut regions = Vec::new();
        let mut at = address;
        let mut queries = 0;
        while at < end {
            if queries >= max_regions {
                return Err("memory.private_region_budget".into());
            }
            queries += 1;
            let Some(region) = self.private_region(at, space, budget, deadline)? else {
                break;
            };
            if !region.eligible() {
                break;
            }
            at = region.end.min(end);
            regions.push(region);
        }
        let wanted = (at - address) as usize;
        if wanted == 0 {
            self.verify_until(budget, deadline)?;
            return Ok(0);
        }
        budget.charge(PrivateCall::Read(wanted), deadline)?;
        // Do not mutate the caller's buffer until every post-read guard passed.
        // This scratch is bounded by PRIVATE_PREFIX_MAX_BYTES and released here.
        let mut observed = vec![0; wanted];
        let mut read = 0;
        private_deadline(deadline)?;
        let result = unsafe {
            ReadProcessMemory(
                self.handle.0,
                address as *const _,
                observed.as_mut_ptr() as *mut _,
                wanted,
                Some(&mut read),
            )
        };
        let read = private_finish(private_read_count(result, read, wanted), deadline)?;
        // Check every mapping that backs bytes we would return, including the
        // final short-prefix page. Never expose bytes rejected by a post guard.
        let mut at = address;
        for before in regions {
            if at >= address + read as u64 {
                break;
            }
            let after = self.private_region(at, space, budget, deadline)?;
            if after != Some(before) || !before.eligible() {
                return Err("memory.private_mapping_changed".into());
            }
            at = before.end;
        }
        self.verify_until(budget, deadline)?;
        bytes[..read].copy_from_slice(&observed[..read]);
        Ok(read)
    }
}

fn private_identity(
    handle: HANDLE,
    pid: u32,
    budget: &mut PrivateReadBudget,
    deadline: Instant,
) -> Result<ProcessIdentity> {
    budget.charge(PrivateCall::Other, deadline)?;
    let (mut c, mut e, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let result = unsafe { GetProcessTimes(handle, &mut c, &mut e, &mut k, &mut u) }
        .map_err(|error| private_os_error("process.times", error));
    private_finish(result, deadline)?;
    Ok(ProcessIdentity {
        pid,
        created: (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64,
    })
}
fn private_read_count(
    result: windows::core::Result<()>,
    read: usize,
    wanted: usize,
) -> Result<usize> {
    match result {
        Err(error) if error.code() != windows::core::HRESULT::from_win32(ERROR_PARTIAL_COPY.0) => {
            Err(private_os_error("memory.private_read", error))
        }
        Err(_) if read <= wanted => Ok(read),
        Ok(()) if read == wanted => Ok(read),
        _ => Err("memory.private_read_count".into()),
    }
}
fn private_query_failure(
    error: WIN32_ERROR,
    address: u64,
    maximum: u64,
) -> Result<Option<MemoryRegion>> {
    if error == ERROR_INVALID_PARAMETER && address > maximum {
        Ok(None)
    } else {
        Err(format!("memory.private_query: win32={}", error.0))
    }
}

#[cfg(test)]
mod runtime_private_tests {
    use super::*;
    fn os_error(error: WIN32_ERROR) -> windows::core::Error {
        windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(error.0))
    }
    #[test]
    fn staging_preflight_enforces_exact_size_cap_without_allocating_or_charging() {
        let budget = PrivateReadBudget::new(PrivateReadLimits::default()).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        budget
            .preflight_private_read(PRIVATE_PREFIX_MAX_BYTES, deadline)
            .unwrap();
        assert_eq!(
            budget
                .preflight_private_read(PRIVATE_PREFIX_MAX_BYTES + 1, deadline)
                .unwrap_err(),
            "memory.private_read_size"
        );
        assert_eq!(budget.usage(), PrivateReadUsage::default());
        // Each of the two staging layers uses this same 16 MiB cap; caller
        // buffers and allocation metadata are separate from these payload bytes.
        assert_eq!(PRIVATE_PREFIX_MAX_BYTES * 2, 32 << 20);
    }
    #[test]
    fn only_explicit_partial_copy_can_release_a_short_physical_prefix() {
        assert_eq!(
            private_read_count(Err(os_error(ERROR_PARTIAL_COPY)), 4, 8).unwrap(),
            4
        );
        assert_eq!(
            private_read_count(Err(os_error(ERROR_PARTIAL_COPY)), 0, 8).unwrap(),
            0
        );
        assert_eq!(
            private_read_count(Err(os_error(ERROR_ACCESS_DENIED)), 4, 8).unwrap_err(),
            "memory.private_read: hresult=0x80070005"
        );
        assert_eq!(
            private_read_count(Err(os_error(ERROR_INVALID_HANDLE)), 8, 8).unwrap_err(),
            "memory.private_read: hresult=0x80070006"
        );
        assert!(private_read_count(Err(os_error(ERROR_PARTIAL_COPY)), 9, 8).is_err());
        assert!(private_read_count(Ok(()), 4, 8).is_err());
        assert_eq!(private_read_count(Ok(()), 8, 8).unwrap(), 8);
    }
    #[test]
    fn query_errors_require_both_legal_end_address_and_exact_error() {
        assert_eq!(
            private_query_failure(ERROR_INVALID_PARAMETER, 101, 100).unwrap(),
            None
        );
        assert_eq!(
            private_query_failure(ERROR_INVALID_PARAMETER, 100, 100).unwrap_err(),
            "memory.private_query: win32=87"
        );
        assert_eq!(
            private_query_failure(ERROR_ACCESS_DENIED, 101, 100).unwrap_err(),
            "memory.private_query: win32=5"
        );
        assert_eq!(
            private_query_failure(ERROR_INVALID_HANDLE, 101, 100).unwrap_err(),
            "memory.private_query: win32=6"
        );
    }
}
unsafe fn identity(h: HANDLE, pid: u32) -> Result<ProcessIdentity> {
    unsafe {
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).map_err(|e| e.to_string())?;
        Ok(ProcessIdentity {
            pid,
            created: (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64,
        })
    }
}
#[derive(Clone, Debug)]
pub struct Mapping {
    pub start: u64,
    pub end: u64,
    pub allocation: u64,
    pub kind: u32,
    pub protection: u32,
}
pub struct ModulePlan {
    process: ProcessIdentity,
    base: u64,
    size: u64,
    ranges: Vec<Mapping>,
    _file: File,
    disk: Vec<u8>,
    hash: String,
}
impl ModulePlan {
    pub fn base(&self) -> u64 {
        self.base
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn disk(&self) -> &[u8] {
        &self.disk
    }
    pub fn hash(&self) -> &str {
        &self.hash
    }
}
fn hex_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl WindowIdentity {
    pub fn verify(&self) -> Result<()> {
        let p = Process::open(self.process.pid)?;
        if p.identity != self.process {
            return Err("window.process_reused".into());
        }
        unsafe {
            let mut pid = 0;
            let h = HWND(self.hwnd as *mut _);
            if !IsWindow(Some(h)).as_bool() {
                return Err("window.closed".into());
            }
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid != self.process.pid {
                return Err("window.reused".into());
            }
            Ok(())
        }
    }
}
/// Win32 mutex ownership is thread-local. !Send and !Sync prevent an incorrect release.
pub struct NamedLock {
    handle: Handle,
    _thread: PhantomData<Rc<()>>,
}
impl NamedLock {
    pub fn acquire(name: &str) -> Result<Self> {
        if !name.starts_with("Local\\LycheeDev.") {
            return Err("lock.name".into());
        }
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let handle = Handle(
                CreateMutexW(None, false, PCWSTR(wide.as_ptr())).map_err(|e| e.to_string())?,
            );
            match WaitForSingleObject(handle.0, 0) {
                WAIT_OBJECT_0 => Ok(Self {
                    handle,
                    _thread: PhantomData,
                }),
                WAIT_ABANDONED => {
                    let _ = ReleaseMutex(handle.0);
                    Err("lock.abandoned_requires_recovery".into())
                }
                WAIT_TIMEOUT => Err("lock.busy".into()),
                _ => Err("lock.failed".into()),
            }
        }
    }
}
impl Drop for NamedLock {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.handle.0);
        }
    }
}
