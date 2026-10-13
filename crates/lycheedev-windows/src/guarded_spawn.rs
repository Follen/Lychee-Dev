//! A child cannot execute before its exact held process is assigned to its Job.
//! Uses stable std Child/stdio ownership and bounded Windows thread discovery.
use crate::{JobGuard, ProcessIdentity};
use std::{
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_NO_MORE_FILES, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, GetProcessIdOfThread, GetProcessTimes, GetThreadId,
            OpenThread, ResumeThread, THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
            WaitForSingleObject,
        },
    },
};
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
#[derive(Debug, serde::Serialize)]
pub struct StartupFailure {
    pub stage: &'static str,
    pub cause: String,
    pub cleanup: StartupCleanup,
}
#[derive(Debug, serde::Serialize)]
pub struct StartupCleanup {
    #[serde(serialize_with = "serialize_identity")]
    pub identity: Option<ProcessIdentity>,
    pub terminate_requested: bool,
    pub child_waited: bool,
    pub job_empty: Option<bool>,
    pub error: Option<String>,
}
fn serialize_identity<S: serde::Serializer>(
    identity: &Option<ProcessIdentity>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    #[derive(serde::Serialize)]
    struct ExactIdentity {
        pid: u32,
        created: String,
    }
    identity
        .as_ref()
        .map(|value| ExactIdentity {
            pid: value.pid,
            created: value.created.to_string(),
        })
        .serialize(serializer)
}
impl StartupCleanup {
    fn no_child() -> Self {
        Self {
            identity: None,
            terminate_requested: false,
            child_waited: true,
            job_empty: None,
            error: None,
        }
    }
}
struct Custody {
    child: Option<Child>,
    job: Option<JobGuard>,
}
impl Custody {
    fn fail(mut self, cause: String) -> StartupFailure {
        StartupFailure {
            stage: "prepare",
            cause,
            cleanup: self.reap(),
        }
    }
    fn reap(&mut self) -> StartupCleanup {
        let Some(child) = self.child.as_mut() else {
            return StartupCleanup::no_child();
        };
        let identity = creation(child).ok().map(|created| ProcessIdentity {
            pid: child.id(),
            created,
        });
        let mut errors = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        child.stdin.take();
        if let Some(job) = &self.job
            && let Err(error) = job.terminate()
        {
            errors.push(error);
        }
        let terminate_requested = match child.kill() {
            Ok(()) => true,
            Err(error) => {
                if child.try_wait().ok().flatten().is_none() {
                    errors.push(error.to_string());
                }
                false
            }
        };
        let mut child_waited = false;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait = unsafe {
                WaitForSingleObject(
                    HANDLE(child.as_raw_handle()),
                    remaining.as_millis().clamp(1, 50) as u32,
                )
            };
            if wait == WAIT_OBJECT_0 {
                match child.wait() {
                    Ok(_) => child_waited = true,
                    Err(error) => errors.push(error.to_string()),
                }
                break;
            }
            if wait != WAIT_TIMEOUT {
                errors.push("startup.wait_failed".into());
                break;
            }
            if Instant::now() >= deadline {
                errors.push("startup.reap_deadline_unconfirmed".into());
                break;
            }
        }
        let mut job_empty = None;
        if let Some(job) = &self.job {
            loop {
                match job.memory_accounting() {
                    Ok(value) if value.active_processes == 0 => {
                        job_empty = Some(true);
                        break;
                    }
                    Ok(_) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Ok(_) => {
                        job_empty = Some(false);
                        errors.push("startup.job_drain_unconfirmed".into());
                        break;
                    }
                    Err(error) => {
                        errors.push(error);
                        break;
                    }
                }
            }
        }
        self.child.take();
        self.job.take();
        StartupCleanup {
            identity,
            terminate_requested,
            child_waited,
            job_empty,
            error: (!errors.is_empty()).then(|| errors.join("; ")),
        }
    }
}
impl Drop for Custody {
    fn drop(&mut self) {
        if self.child.is_some() {
            let _ = self.reap();
        }
    }
}
fn creation(child: &Child) -> Result<u64, String> {
    let (mut created, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        GetProcessTimes(
            HANDLE(child.as_raw_handle()),
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok((created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64)
}
fn check(deadline: Instant, cancel: &mut impl FnMut() -> bool) -> Result<(), String> {
    if cancel() {
        return Err("startup.cancelled".into());
    }
    if Instant::now() >= deadline {
        return Err("startup.deadline".into());
    }
    Ok(())
}
fn original_primary_thread(
    child: &Child,
    created: u64,
    deadline: Instant,
    cancel: &mut impl FnMut() -> bool,
) -> Result<Handle, String> {
    const MAX_THREADS: usize = 65536;
    let snapshot = Handle(unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).map_err(|error| error.to_string())?
    });
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    unsafe {
        Thread32First(snapshot.0, &mut entry).map_err(|error| error.to_string())?;
    }
    let (mut selected, mut entries) = (None, 0usize);
    loop {
        check(deadline, cancel)?;
        entries += 1;
        if entries > MAX_THREADS {
            return Err("startup.thread_inventory_budget".into());
        }
        if entry.th32OwnerProcessID == child.id() {
            if selected.is_some() {
                return Err("startup.primary_thread_ambiguous".into());
            }
            selected = Some(entry.th32ThreadID);
        }
        match unsafe { Thread32Next(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(error)
                if error.code() == windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0) =>
            {
                break;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let tid = selected.ok_or("startup.primary_thread_missing")?;
    let thread = Handle(unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
            false,
            tid,
        )
        .map_err(|error| error.to_string())?
    });
    let owner = unsafe { GetProcessIdOfThread(thread.0) };
    let actual_tid = unsafe { GetThreadId(thread.0) };
    if owner != child.id() || actual_tid != tid || creation(child)? != created {
        return Err("startup.thread_identity_changed".into());
    }
    if unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), 0) } != WAIT_TIMEOUT {
        return Err("startup.child_not_live".into());
    }
    Ok(thread)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrepareStage {
    CreatedSuspended,
    JobAssigned,
    BeforeResume,
}

/// Keep the caller's configured stdio/arguments/environment. The narrow helper
/// owns NO_WINDOW|SUSPENDED flags and never adopts an external PID or falls back
/// to an unguarded launch. The original execution deadline is not renewed.
pub fn spawn_suspended_guarded(
    command: Command,
    limit: usize,
    deadline: Instant,
    cancel: impl FnMut() -> bool,
) -> Result<(Child, JobGuard), StartupFailure> {
    spawn_with_probe(command, limit, deadline, cancel, |_, _, _| Ok(()))
}
fn spawn_with_probe(
    mut command: Command,
    limit: usize,
    deadline: Instant,
    mut cancel: impl FnMut() -> bool,
    mut probe: impl FnMut(PrepareStage, &Child, Option<&JobGuard>) -> Result<(), String>,
) -> Result<(Child, JobGuard), StartupFailure> {
    if let Err(cause) = check(deadline, &mut cancel) {
        return Err(StartupFailure {
            stage: "control",
            cause,
            cleanup: StartupCleanup::no_child(),
        });
    }
    if !(1 << 20..=4usize << 30).contains(&limit) {
        return Err(StartupFailure {
            stage: "prepare",
            cause: "job.commit_budget_invalid".into(),
            cleanup: StartupCleanup::no_child(),
        });
    }
    command.creation_flags((CREATE_NO_WINDOW | CREATE_SUSPENDED).0);
    let child = command.spawn().map_err(|error| StartupFailure {
        stage: "spawn",
        cause: error.to_string(),
        cleanup: StartupCleanup::no_child(),
    })?;
    let mut custody = Custody {
        child: Some(child),
        job: None,
    };
    let prepare = (|| -> Result<Handle, String> {
        check(deadline, &mut cancel)?;
        let child = custody.child.as_ref().unwrap();
        let created = creation(child)?;
        probe(PrepareStage::CreatedSuspended, child, None)?;
        check(deadline, &mut cancel)?;
        custody.job = Some(JobGuard::attach_with_commit_limit(child, limit)?);
        let members = custody.job.as_ref().unwrap().membership()?;
        if members.member_pids != vec![child.id()] {
            return Err("startup.job_membership_changed".into());
        }
        probe(
            PrepareStage::JobAssigned,
            custody.child.as_ref().unwrap(),
            custody.job.as_ref(),
        )?;
        original_primary_thread(
            custody.child.as_ref().unwrap(),
            created,
            deadline,
            &mut cancel,
        )
    })();
    let thread = match prepare {
        Ok(thread) => thread,
        Err(cause) => return Err(custody.fail(cause)),
    };
    if let Err(cause) = probe(
        PrepareStage::BeforeResume,
        custody.child.as_ref().unwrap(),
        custody.job.as_ref(),
    ) {
        return Err(custody.fail(cause));
    }
    if let Err(cause) = check(deadline, &mut cancel) {
        return Err(custody.fail(cause));
    }
    let previous = unsafe { ResumeThread(thread.0) };
    if previous != 1 {
        let cause = if previous == u32::MAX {
            format!("startup.resume:{}", windows::core::Error::from_thread())
        } else {
            format!("startup.resume_count:{previous}")
        };
        return Err(custody.fail(cause));
    }
    drop(thread);
    Ok((custody.child.take().unwrap(), custody.job.take().unwrap()))
}

#[cfg(test)]
#[path = "guarded_spawn_tests.rs"]
mod tests;
