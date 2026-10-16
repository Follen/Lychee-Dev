use std::{os::windows::io::AsRawHandle, process::Child};
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_MORE_DATA, HANDLE},
    System::{JobObjects::*, Threading::GetCurrentProcess},
};
pub struct JobGuard {
    handle: HANDLE,
}
/// Opt-in broker aggregate committed-memory ceiling, inherited by descendants.
/// Source must join Git/LuaLS workers before disarm; this is not an RSS claim.
pub struct BrokerJob {
    handle: HANDLE,
    limit: usize,
    armed: bool,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct JobMemoryAccounting {
    pub aggregate_commit: Option<u64>,
    pub aggregate_commit_status: String,
    pub peak_job_memory_used: u64,
    pub peak_process_memory_used: u64,
    pub job_memory_limit: u64,
    pub active_processes: u32,
    pub limit_flags: u32,
    pub scope: String,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct JobMembership {
    pub assigned_processes: u32,
    pub member_pids: Vec<u32>,
    pub scope: String,
}
impl BrokerJob {
    /// Public SDK telemetry only. Peak/limit are commit budget metrics; never sum RSS.
    pub fn memory_accounting(&self) -> crate::Result<JobMemoryAccounting> {
        memory_accounting(self.handle)
    }
}
fn memory_accounting(handle: HANDLE) -> crate::Result<JobMemoryAccounting> {
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            Some(handle),
            JobObjectExtendedLimitInformation,
            &mut limits as *mut _ as *mut _,
            std::mem::size_of_val(&limits) as u32,
            None,
        )
        .map_err(|e| e.to_string())?;
        QueryInformationJobObject(
            Some(handle),
            JobObjectBasicAccountingInformation,
            &mut accounting as *mut _ as *mut _,
            std::mem::size_of_val(&accounting) as u32,
            None,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(JobMemoryAccounting {aggregate_commit:None,aggregate_commit_status:"unsupportedPublicSdk: no current aggregate query; no RSS sum or historical notification substitution".into(),
            peak_job_memory_used:limits.PeakJobMemoryUsed as u64,peak_process_memory_used:limits.PeakProcessMemoryUsed as u64,job_memory_limit:limits.JobMemoryLimit as u64,active_processes:accounting.ActiveProcesses,
            limit_flags:limits.BasicLimitInformation.LimitFlags.0,scope:"QueryInformationJobObject ExtendedLimitInformation(9) and BasicAccountingInformation(1); no limit mutation".into()})
}
impl BrokerJob {
    pub fn own_current_process(max_commit_bytes: usize) -> crate::Result<Self> {
        if !(1 << 20..=4usize << 30).contains(&max_commit_bytes) {
            return Err("job.commit_budget_invalid".into());
        }
        unsafe {
            let handle = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
            let mut guard = Self {
                handle,
                limit: max_commit_bytes,
                armed: false,
            };
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_JOB_MEMORY;
            info.JobMemoryLimit = max_commit_bytes;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(|e| e.to_string())?;
            AssignProcessToJobObject(handle, GetCurrentProcess()).map_err(|e| e.to_string())?;
            guard.armed = true;
            Ok(guard)
        }
    }
    pub fn disarm(&mut self) -> crate::Result<()> {
        if !self.armed {
            return Ok(());
        }
        unsafe {
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY;
            info.JobMemoryLimit = self.limit;
            SetInformationJobObject(
                self.handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(|e| e.to_string())?;
        }
        self.armed = false;
        Ok(())
    }
}
impl Drop for BrokerJob {
    fn drop(&mut self) {
        // Never close an armed self-job after a failed disarm: that would kill the
        // current process while it is reporting the error. Explicit disarm exposes
        // the failure; retain the handle until process exit if it cannot be cleared.
        if self.disarm().is_ok() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}
impl JobGuard {
    /// Read the exact already-owned child Job. Peaks are historical commit
    /// counters; this observation neither changes limits nor grants authority.
    pub fn memory_accounting(&self) -> crate::Result<JobMemoryAccounting> {
        memory_accounting(self.handle)
    }
    /// Bounded PID membership snapshot, not process identity/authentication.
    /// Callers must independently hold and verify the corresponding processes.
    pub fn membership(&self) -> crate::Result<JobMembership> {
        const MAX: usize = 64;
        #[repr(C)]
        struct Members {
            assigned: u32,
            listed: u32,
            pids: [usize; MAX],
        }
        let mut members = Members {
            assigned: 0,
            listed: 0,
            pids: [0; MAX],
        };
        unsafe {
            QueryInformationJobObject(
                Some(self.handle),
                JobObjectBasicProcessIdList,
                &mut members as *mut _ as *mut _,
                std::mem::size_of_val(&members) as u32,
                None,
            )
            .map_err(|error| {
                if error.code() == windows::core::HRESULT::from_win32(ERROR_MORE_DATA.0) {
                    "job.member_budget: more than64 process IDs".into()
                } else {
                    format!("job.member_query: {error}")
                }
            })?;
        }
        if members.assigned as usize > MAX || members.listed as usize > MAX {
            return Err("job.member_budget: more than64 process IDs".into());
        }
        let mut pids = Vec::new();
        pids.try_reserve_exact(members.listed as usize)
            .map_err(|_| "job.member_allocation_budget")?;
        for &pid in &members.pids[..members.listed as usize] {
            pids.push(
                u32::try_from(pid)
                    .ok()
                    .filter(|pid| *pid != 0)
                    .ok_or("job.member_invalid_pid")?,
            );
        }
        Ok(JobMembership {
            assigned_processes: members.assigned,
            member_pids: pids,
            scope: "exact owned Job handle BasicProcessIdList(3), max64; PID snapshot only, no process identity or authorization".into(),
        })
    }
    /// Attaching an already running child cannot constrain descendants spawned before attach.
    pub fn attach(child: &Child) -> crate::Result<Self> {
        Self::attach_limited(child, None)
    }
    /// Bound this child job's aggregate committed memory inside any existing
    /// parent Job. This leaves control-process headroom without raising the
    /// enclosing aggregate ceiling; it is not a working-set/RSS limit.
    pub fn attach_with_commit_limit(child: &Child, bytes: usize) -> crate::Result<Self> {
        if !(1 << 20..=4usize << 30).contains(&bytes) {
            return Err("job.commit_budget_invalid".into());
        }
        Self::attach_limited(child, Some(bytes))
    }
    fn attach_limited(child: &Child, bytes: Option<usize>) -> crate::Result<Self> {
        unsafe {
            let handle = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
            let guard = Self { handle };
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Some(bytes) = bytes {
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
                info.JobMemoryLimit = bytes;
            }
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(|e| e.to_string())?;
            AssignProcessToJobObject(handle, HANDLE(child.as_raw_handle()))
                .map_err(|e| e.to_string())?;
            Ok(guard)
        }
    }
    /// Consume this unique Job handle after the caller observes terminal custody.
    /// Do not retry a failed close, which may already have consumed the OS handle.
    pub fn close(self) -> crate::Result<()> {
        let handle = self.handle;
        std::mem::forget(self);
        // SAFETY: self was the unique handle owner and its Drop is disarmed;
        // no references survive this consuming call and no retry is performed.
        unsafe { CloseHandle(handle) }.map_err(|error| error.to_string())
    }
    pub fn terminate(&self) -> crate::Result<()> {
        unsafe { TerminateJobObject(self.handle, 7).map_err(|e| e.to_string()) }
    }
}
impl Drop for JobGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}
