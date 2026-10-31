//! A child cannot execute before its exact held process is assigned to its Job.
//! Uses stable std Child/stdio ownership and bounded Windows thread discovery.
use crate::{JobGuard, ProcessIdentity};
use std::{
    os::windows::{
        io::{AsRawHandle, IntoRawHandle},
        process::CommandExt,
    },
    process::{Child, Command, ExitStatus},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_NO_MORE_ITEMS, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Diagnostics::ProcessSnapshotting::{
            HPSS, HPSSWALK, PSS_CAPTURE_THREADS, PSS_THREAD_ENTRY, PSS_WALK_THREADS,
            PssCaptureSnapshot, PssFreeSnapshot, PssWalkMarkerCreate, PssWalkMarkerFree,
            PssWalkSnapshot,
        },
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, GetCurrentProcess, GetProcessIdOfThread,
            GetProcessTimes, GetThreadId, OpenThread, ResumeThread,
            THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME, WaitForSingleObject,
        },
    },
};
struct Handle(HANDLE);
impl Handle {
    fn close(self) -> Result<(), String> {
        let raw = self.0;
        std::mem::forget(self);
        close_thread_handle(raw)
    }
}
fn close_thread_handle(raw: HANDLE) -> Result<(), String> {
    // SAFETY: Handle transfers its unique OpenThread owner exactly once; a
    // failed close is never retried. Drop is only the unwind fallback.
    let result = unsafe { CloseHandle(raw) }.map_err(|error| error.to_string());
    #[cfg(test)]
    let result = tests::observe_primary_thread_close(result);
    result
}

/// Wait on the already-owned process handle for one finite caller-owned slice.
/// The caller retains its deadline, cancellation and retirement policy.
pub fn wait_child_timeout(
    child: &mut Child,
    timeout: Duration,
) -> std::io::Result<Option<ExitStatus>> {
    // SAFETY: Child holds the process handle throughout this mutable borrow.
    // Keep the timeout finite, including when the caller supplied a large value.
    let state = unsafe {
        WaitForSingleObject(
            HANDLE(child.as_raw_handle()),
            timeout.as_millis().min(u32::MAX as u128 - 1) as u32,
        )
    };
    if state == WAIT_OBJECT_0 {
        child.try_wait()
    } else if state == WAIT_TIMEOUT {
        Ok(None)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = close_thread_handle(self.0);
    }
}
#[derive(Debug, serde::Serialize)]
pub struct StartupFailure {
    pub stage: &'static str,
    pub cause: String,
    pub cleanup: StartupCleanup,
    // Retain exact custody when the caller's absolute retirement deadline ends
    // before reap. Callers must not mistake the cleanup record for completion.
    #[serde(skip)]
    pending: Option<PendingStartup>,
}
struct PendingStartup {
    child: Option<Child>,
    job: Option<JobGuard>,
}
impl std::fmt::Debug for PendingStartup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingStartup")
            .field("pid", &self.child.as_ref().map(Child::id))
            .finish()
    }
}
impl Drop for PendingStartup {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            let _ = job.terminate();
        }
        if let Some(child) = &mut self.child {
            let _ = child.kill();
        }
        // Unwind/legacy fallback only. No completion is claimed by Drop.
    }
}
impl StartupFailure {
    pub fn take_pending(&mut self) -> Option<(Child, Option<JobGuard>)> {
        let mut pending = self.pending.take()?;
        Some((
            pending.child.take().expect("pending owned child"),
            pending.job.take(),
        ))
    }
}
#[derive(Debug, serde::Serialize)]
pub struct StartupCleanup {
    #[serde(serialize_with = "serialize_identity")]
    pub identity: Option<ProcessIdentity>,
    pub terminate_requested: bool,
    pub child_waited: bool,
    pub job_empty: Option<bool>,
    pub error: Option<String>,
    pub handle_close_errors: Vec<String>,
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
            handle_close_errors: Vec::new(),
        }
    }
}
struct Custody {
    child: Option<Child>,
    job: Option<JobGuard>,
    retirement_deadline: Option<Instant>,
}
impl Custody {
    fn fail_with_closes(self, cause: String, closes: Vec<String>) -> StartupFailure {
        let mut failure = self.fail(cause);
        for close in closes {
            let error = format!("startup.primary_thread_close: {close}");
            failure.cleanup.error = Some(match failure.cleanup.error.take() {
                Some(primary) => format!("{primary}; {error}"),
                None => error,
            });
            failure.cleanup.handle_close_errors.push(close);
        }
        failure
    }
    fn fail(mut self, cause: String) -> StartupFailure {
        let cleanup = self.reap();
        let pending = self.child.take().map(|child| PendingStartup {
            child: Some(child),
            job: self.job.take(),
        });
        StartupFailure {
            stage: "prepare",
            cause,
            cleanup,
            pending,
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
        let mut handle_close_errors = Vec::new();
        let deadline = self
            .retirement_deadline
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(2));
        if let Some(pipe) = child.stdin.take() {
            if let Err(error) = close_owned_handle(pipe) {
                handle_close_errors.push(error.clone());
                errors.push(error);
            }
        }
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
                    remaining.as_millis().min(50) as u32,
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
        if child_waited && (job_empty == Some(true) || self.job.is_none()) {
            if let Some(child) = self.child.take() {
                if let Err(error) = close_child(child) {
                    handle_close_errors.push(error.clone());
                    errors.push(error);
                }
            }
            if let Some(job) = self.job.take() {
                if let Err(error) = job.close() {
                    handle_close_errors.push(error.clone());
                    errors.push(error);
                }
            }
        }
        StartupCleanup {
            identity,
            terminate_requested,
            child_waited,
            job_empty,
            error: (!errors.is_empty()).then(|| errors.join("; ")),
            handle_close_errors,
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
// Both PSS resources belong to this calling process. Normal/error returns
// explicitly check both frees; Drop only protects unwinding before that point.
struct ThreadSnapshot {
    snapshot: Option<HPSS>,
    marker: Option<HPSSWALK>,
}
impl ThreadSnapshot {
    fn capture(process: HANDLE) -> Result<Self, String> {
        let mut snapshot = HPSS::default();
        // SAFETY: the caller holds this process handle. Capture only its thread
        // list, without VA cloning, memory reads or thread contexts.
        let status =
            unsafe { PssCaptureSnapshot(process, PSS_CAPTURE_THREADS, None, &mut snapshot) };
        if status != 0 {
            return Err(format!("startup.thread_snapshot_capture:{status}"));
        }
        Ok(Self {
            snapshot: Some(snapshot),
            marker: None,
        })
    }
    fn unique_thread(
        &mut self,
        pid: u32,
        deadline: Instant,
        cancel: &mut impl FnMut() -> bool,
    ) -> Result<u32, String> {
        const MAX_THREADS: usize = 65536;
        check(deadline, cancel)?;
        let mut marker = HPSSWALK::default();
        // SAFETY: PSS creates one local walk marker with its default allocator.
        let status = unsafe { PssWalkMarkerCreate(None, &mut marker) };
        if status != 0 {
            return Err(format!("startup.thread_snapshot_marker:{status}"));
        }
        self.marker = Some(marker);
        let (mut selected, mut entries) = (None, 0usize);
        loop {
            check(deadline, cancel)?;
            let mut entry = std::mem::MaybeUninit::<PSS_THREAD_ENTRY>::uninit();
            // SAFETY: the generated repr(C) type supplies native alignment and
            // size. Initialize padding too, before exposing its entire storage
            // as PssWalkSnapshot's byte buffer. Read fields only on success.
            let status = unsafe {
                let pointer = entry.as_mut_ptr().cast::<u8>();
                std::ptr::write_bytes(pointer, 0, std::mem::size_of::<PSS_THREAD_ENTRY>());
                let bytes = std::slice::from_raw_parts_mut(
                    pointer,
                    std::mem::size_of::<PSS_THREAD_ENTRY>(),
                );
                PssWalkSnapshot(
                    self.snapshot.expect("owned thread snapshot"),
                    PSS_WALK_THREADS,
                    marker,
                    Some(bytes),
                )
            };
            if status == ERROR_NO_MORE_ITEMS.0 {
                break;
            }
            if status != 0 {
                return Err(format!("startup.thread_snapshot_walk:{status}"));
            }
            // SAFETY: a successful walk filled the initialized native entry.
            let entry = unsafe { entry.assume_init() };
            entries += 1;
            if entries > MAX_THREADS {
                return Err("startup.thread_inventory_budget".into());
            }
            if entry.ProcessId != pid || entry.ThreadId == 0 {
                return Err("startup.thread_identity_changed".into());
            }
            if selected.replace(entry.ThreadId).is_some() {
                return Err("startup.primary_thread_ambiguous".into());
            }
        }
        selected.ok_or_else(|| "startup.primary_thread_missing".into())
    }
    fn finish(mut self, result: Result<u32, String>) -> Result<u32, String> {
        let mut errors = Vec::new();
        if let Some(marker) = self.marker.take() {
            // SAFETY: take transfers the sole local marker for one free attempt.
            let status = unsafe { PssWalkMarkerFree(marker) };
            if status != 0 {
                errors.push(format!("startup.thread_snapshot_marker_free:{status}"));
            }
        }
        if let Some(snapshot) = self.snapshot.take() {
            // SAFETY: capture placed this snapshot in the current process,
            // even though the captured target was the held child process.
            let status = unsafe { PssFreeSnapshot(GetCurrentProcess(), snapshot) };
            if status != 0 {
                errors.push(format!("startup.thread_snapshot_free:{status}"));
            }
        }
        if errors.is_empty() {
            return result;
        }
        match result {
            Ok(_) => Err(errors.join("; ")),
            Err(primary) => Err(format!("{primary}; {}", errors.join("; "))),
        }
    }
}
impl Drop for ThreadSnapshot {
    fn drop(&mut self) {
        // Unwind fallback only; explicit finish consumes both handles before
        // checking statuses, so even a failed free is never retried by Drop.
        unsafe {
            if let Some(marker) = self.marker.take() {
                let _ = PssWalkMarkerFree(marker);
            }
            if let Some(snapshot) = self.snapshot.take() {
                let _ = PssFreeSnapshot(GetCurrentProcess(), snapshot);
            }
        }
    }
}
fn original_primary_thread(
    child: &Child,
    created: u64,
    deadline: Instant,
    cancel: &mut impl FnMut() -> bool,
    close_errors: &mut Vec<String>,
) -> Result<Handle, String> {
    check(deadline, cancel)?;
    let mut snapshot = ThreadSnapshot::capture(HANDLE(child.as_raw_handle()))?;
    let selected = snapshot.unique_thread(child.id(), deadline, cancel);
    let tid = snapshot.finish(selected)?;
    check(deadline, cancel)?;
    let thread = Handle(unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
            false,
            tid,
        )
        .map_err(|error| error.to_string())?
    });
    let checked = (|| {
        let owner = unsafe { GetProcessIdOfThread(thread.0) };
        let actual_tid = unsafe { GetThreadId(thread.0) };
        if owner != child.id() || actual_tid != tid || creation(child)? != created {
            return Err("startup.thread_identity_changed".into());
        }
        if unsafe { WaitForSingleObject(HANDLE(child.as_raw_handle()), 0) } != WAIT_TIMEOUT {
            return Err("startup.child_not_live".into());
        }
        Ok(())
    })();
    if let Err(error) = checked {
        if let Err(close) = thread.close() {
            close_errors.push(close);
        }
        return Err(error);
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
/// Keep inherited limits without adding a child commit ceiling. The suspended
/// root still joins its exact Job before its first instruction executes.
pub fn spawn_suspended_job_guarded(
    command: Command,
    deadline: Instant,
    cancel: impl FnMut() -> bool,
) -> Result<(Child, JobGuard), StartupFailure> {
    spawn_optional_limit_with_probe_until(command, None, deadline, None, cancel, |_, _, _| Ok(()))
}
/// Same suspended/assigned launch, but all failure retirement uses this existing
/// absolute boundary. An unfinished child is returned as owned pending custody.
pub fn spawn_suspended_guarded_until(
    command: Command,
    limit: usize,
    deadline: Instant,
    retirement_deadline: Instant,
    cancel: impl FnMut() -> bool,
) -> Result<(Child, JobGuard), StartupFailure> {
    spawn_with_probe_until(
        command,
        limit,
        deadline,
        Some(retirement_deadline),
        cancel,
        |_, _, _| Ok(()),
    )
}

/// Read identity from the original held Child, never by reopening a PID.
pub fn child_identity(child: &Child) -> Result<ProcessIdentity, String> {
    Ok(ProcessIdentity {
        pid: child.id(),
        created: creation(child)?,
    })
}
/// Consume an owned process/pipe handle exactly once. Failed close is not retried.
pub fn close_owned_handle(owner: impl IntoRawHandle) -> Result<(), String> {
    let handle = HANDLE(owner.into_raw_handle());
    // SAFETY: IntoRawHandle transfers the unique owner. No borrow is retained;
    // this is its sole CloseHandle attempt, including when the OS reports error.
    unsafe { CloseHandle(handle) }.map_err(|error| error.to_string())
}
pub fn close_child(mut child: Child) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Some(pipe) = child.stdin.take() {
        if let Err(e) = close_owned_handle(pipe) {
            errors.push(e);
        }
    }
    if let Some(pipe) = child.stdout.take() {
        if let Err(e) = close_owned_handle(pipe) {
            errors.push(e);
        }
    }
    if let Some(pipe) = child.stderr.take() {
        if let Err(e) = close_owned_handle(pipe) {
            errors.push(e);
        }
    }
    if let Err(e) = close_owned_handle(child) {
        errors.push(e);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
fn spawn_with_probe(
    command: Command,
    limit: usize,
    deadline: Instant,
    cancel: impl FnMut() -> bool,
    probe: impl FnMut(PrepareStage, &Child, Option<&JobGuard>) -> Result<(), String>,
) -> Result<(Child, JobGuard), StartupFailure> {
    spawn_with_probe_until(command, limit, deadline, None, cancel, probe)
}
fn spawn_with_probe_until(
    command: Command,
    limit: usize,
    deadline: Instant,
    retirement_deadline: Option<Instant>,
    cancel: impl FnMut() -> bool,
    probe: impl FnMut(PrepareStage, &Child, Option<&JobGuard>) -> Result<(), String>,
) -> Result<(Child, JobGuard), StartupFailure> {
    spawn_optional_limit_with_probe_until(
        command,
        Some(limit),
        deadline,
        retirement_deadline,
        cancel,
        probe,
    )
}
fn spawn_optional_limit_with_probe_until(
    mut command: Command,
    limit: Option<usize>,
    deadline: Instant,
    retirement_deadline: Option<Instant>,
    mut cancel: impl FnMut() -> bool,
    mut probe: impl FnMut(PrepareStage, &Child, Option<&JobGuard>) -> Result<(), String>,
) -> Result<(Child, JobGuard), StartupFailure> {
    if let Err(cause) = check(deadline, &mut cancel) {
        return Err(StartupFailure {
            stage: "control",
            cause,
            cleanup: StartupCleanup::no_child(),
            pending: None,
        });
    }
    if limit.is_some_and(|limit| !(1 << 20..=4usize << 30).contains(&limit)) {
        return Err(StartupFailure {
            stage: "prepare",
            cause: "job.commit_budget_invalid".into(),
            cleanup: StartupCleanup::no_child(),
            pending: None,
        });
    }
    command.creation_flags((CREATE_NO_WINDOW | CREATE_SUSPENDED).0);
    let child = command.spawn().map_err(|error| StartupFailure {
        stage: "spawn",
        cause: error.to_string(),
        cleanup: StartupCleanup::no_child(),
        pending: None,
    })?;
    let mut custody = Custody {
        child: Some(child),
        job: None,
        retirement_deadline,
    };
    let mut thread_close_errors = Vec::new();
    let prepare = (|| -> Result<Handle, String> {
        check(deadline, &mut cancel)?;
        let child = custody.child.as_ref().unwrap();
        let created = creation(child)?;
        probe(PrepareStage::CreatedSuspended, child, None)?;
        check(deadline, &mut cancel)?;
        custody.job = Some(match limit {
            Some(limit) => JobGuard::attach_with_commit_limit(child, limit)?,
            None => JobGuard::attach(child)?,
        });
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
            &mut thread_close_errors,
        )
    })();
    let thread = match prepare {
        Ok(thread) => thread,
        Err(cause) => return Err(custody.fail_with_closes(cause, thread_close_errors)),
    };
    if let Err(cause) = probe(
        PrepareStage::BeforeResume,
        custody.child.as_ref().unwrap(),
        custody.job.as_ref(),
    ) {
        return Err(custody.fail_with_closes(cause, thread.close().err().into_iter().collect()));
    }
    if let Err(cause) = check(deadline, &mut cancel) {
        return Err(custody.fail_with_closes(cause, thread.close().err().into_iter().collect()));
    }
    let previous = unsafe { ResumeThread(thread.0) };
    if previous != 1 {
        let cause = if previous == u32::MAX {
            format!("startup.resume:{}", windows::core::Error::from_thread())
        } else {
            format!("startup.resume_count:{previous}")
        };
        return Err(custody.fail_with_closes(cause, thread.close().err().into_iter().collect()));
    }
    if let Err(close) = thread.close() {
        let mut failure = custody.fail_with_closes(
            format!("startup.primary_thread_close: {close}"),
            vec![close],
        );
        failure.stage = "thread_close";
        return Err(failure);
    }
    Ok((custody.child.take().unwrap(), custody.job.take().unwrap()))
}

#[cfg(test)]
#[path = "guarded_spawn_tests.rs"]
mod tests;

#[cfg(test)]
mod thread_snapshot_tests {
    use super::*;
    use windows::Win32::System::Threading::GetCurrentProcessId;

    #[test]
    fn target_snapshot_refuses_multiple_threads() {
        // Keep a second real thread alive through capture: the caller cannot
        // pass unique-primary admission just because the snapshot is targeted.
        let (stop, wait) = std::sync::mpsc::sync_channel(0);
        let parked = std::thread::spawn(move || wait.recv().unwrap());
        let result = (|| {
            // SAFETY: pseudo handles and IDs refer only to this test process.
            let mut snapshot = ThreadSnapshot::capture(unsafe { GetCurrentProcess() })?;
            let selected = snapshot.unique_thread(
                unsafe { GetCurrentProcessId() },
                Instant::now() + Duration::from_secs(5),
                &mut || false,
            );
            snapshot.finish(selected)
        })();
        stop.send(()).unwrap();
        parked.join().unwrap();
        assert_eq!(result.unwrap_err(), "startup.primary_thread_ambiguous");
    }

    #[test]
    fn target_snapshot_preserves_cancellation_and_deadline() {
        // SAFETY: capture/read only this test process's thread identities.
        let mut snapshot = ThreadSnapshot::capture(unsafe { GetCurrentProcess() }).unwrap();
        let mut checks = 0;
        let selected = snapshot.unique_thread(
            unsafe { GetCurrentProcessId() },
            Instant::now() + Duration::from_secs(5),
            &mut || {
                checks += 1;
                // Enter the walk and read its first item before cancellation.
                checks == 3
            },
        );
        assert_eq!(snapshot.finish(selected).unwrap_err(), "startup.cancelled");

        let mut snapshot = ThreadSnapshot::capture(unsafe { GetCurrentProcess() }).unwrap();
        let selected = snapshot.unique_thread(
            unsafe { GetCurrentProcessId() },
            Instant::now(),
            &mut || false,
        );
        assert_eq!(snapshot.finish(selected).unwrap_err(), "startup.deadline");
    }
}
