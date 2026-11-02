//! One queued Windows file-lock request with caller-owned cancellation checks.
//! This module owns no watcher threads and never frees pending OVERLAPPED state.
use std::{
    cell::UnsafeCell,
    fs::{File, OpenOptions},
    io,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    time::Duration,
};
use windows::Win32::{
    Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_NOT_FOUND, ERROR_OPERATION_ABORTED, HANDLE,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Storage::FileSystem::{FILE_FLAG_OVERLAPPED, LOCKFILE_EXCLUSIVE_LOCK, LockFileEx},
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Threading::{CreateEventW, WaitForSingleObject},
    },
};

#[derive(Debug)]
pub struct NativeFailure {
    pub stage: &'static str,
    pub error: io::Error,
}
#[derive(Debug)]
pub enum Cause<E> {
    Check(E),
    Native(NativeFailure),
}
#[derive(Debug)]
pub struct Failure<E> {
    pub causes: Vec<Cause<E>>,
}
fn native(stage: &'static str, error: windows::core::Error) -> NativeFailure {
    NativeFailure {
        stage,
        error: io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32),
    }
}
fn io_failure<E>(stage: &'static str, error: io::Error) -> Failure<E> {
    Failure {
        causes: vec![Cause::Native(NativeFailure { stage, error })],
    }
}
struct Event(Option<HANDLE>);
impl Event {
    fn open() -> Result<Self, NativeFailure> {
        // SAFETY: unnamed manual-reset event, no inherited pointer or callback.
        // The returned handle has exactly one Event owner and is not exposed.
        unsafe { CreateEventW(None, true, false, None) }
            .map(|handle| Self(Some(handle)))
            .map_err(|e| native("create-event", e))
    }
    fn handle(&self) -> HANDLE {
        self.0.expect("live event owner")
    }
    fn close(&mut self) -> Result<(), NativeFailure> {
        if let Some(handle) = self.0.take() {
            // SAFETY: pending completion has already been joined by its owner.
            // This consumes the only event handle once, even on close failure.
            unsafe { CloseHandle(handle) }.map_err(|e| native("close-event", e))?;
        }
        Ok(())
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
struct Pending<'a> {
    file: &'a File,
    operation: Box<UnsafeCell<OVERLAPPED>>,
    event: Event,
    pending: bool,
}
impl<'a> Pending<'a> {
    fn new(file: &'a File) -> Result<Self, NativeFailure> {
        let event = Event::open()?;
        let operation = Box::new(UnsafeCell::new(OVERLAPPED {
            hEvent: event.handle(),
            ..Default::default()
        }));
        Ok(Self {
            file,
            operation,
            event,
            pending: false,
        })
    }
    fn handle(&self) -> HANDLE {
        HANDLE(self.file.as_raw_handle())
    }
    fn start(&mut self) -> Result<(), NativeFailure> {
        // Arm RAII before issuing I/O. No user callback runs until this owner
        // knows whether that exact request is pending or terminal.
        self.pending = true;
        // SAFETY: file remains borrowed/live, offset-zero OVERLAPPED is fully
        // initialized in a stable Box<UnsafeCell<_>>, with a private manual-reset
        // event. Only raw get() pointers reach the kernel: no Rust reference to
        // OVERLAPPED is formed while the kernel may mutate completion fields.
        // All exits, including callback unwind, join before reclaiming the Box.
        let result = unsafe {
            LockFileEx(
                self.handle(),
                LOCKFILE_EXCLUSIVE_LOCK,
                None,
                1,
                0,
                self.operation.get(),
            )
        };
        match result {
            Ok(()) => {
                self.pending = false;
                Ok(())
            }
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => Ok(()),
            Err(error) => {
                self.pending = false;
                Err(native("lock-file", error))
            }
        }
    }
    fn complete(&mut self) -> Result<(), NativeFailure> {
        let mut transferred = 0;
        // SAFETY: the original File/event/Box remain alive and are neither
        // reused nor shared with another request. UnsafeCell permits kernel writes;
        // get() creates only a raw pointer, never a reference to live I/O state.
        // Waiting establishes terminal
        // completion before this owner clears pending or releases any storage.
        let result = unsafe {
            GetOverlappedResult(self.handle(), self.operation.get(), &mut transferred, true)
        };
        self.pending = false;
        result.map_err(|e| native("complete-lock", e))
    }
    fn cancel_complete(&mut self) -> Vec<NativeFailure> {
        if !self.pending {
            return Vec::new();
        }
        let mut errors = Vec::new();
        // SAFETY: cancel only this owner's original pending request; it does
        // not grant permission to free or reuse its OVERLAPPED. UnsafeCell::get
        // forms no reference while concurrent kernel completion may mutate it.
        // Join follows before any storage is reclaimed.
        if let Err(error) = unsafe { CancelIoEx(self.handle(), Some(self.operation.get())) } {
            if error.code() != ERROR_NOT_FOUND.to_hresult() {
                errors.push(native("cancel-lock", error));
            }
        }
        if let Err(error) = self.complete() {
            if error.error.raw_os_error() != Some(ERROR_OPERATION_ABORTED.0 as i32) {
                errors.push(error);
            }
        }
        errors
    }
    fn wait<E>(
        &mut self,
        check: &mut impl FnMut() -> Result<Duration, E>,
    ) -> Result<(), Failure<E>> {
        while self.pending {
            let remaining = match check() {
                Ok(remaining) => remaining,
                Err(error) => {
                    let mut causes = vec![Cause::Check(error)];
                    causes.extend(self.cancel_complete().into_iter().map(Cause::Native));
                    return Err(Failure { causes });
                }
            };
            let interval = remaining.min(Duration::from_millis(25));
            let milliseconds = interval.as_nanos().div_ceil(1_000_000).max(1) as u32;
            // SAFETY: event is private and remains open through this wait and
            // subsequent completion. A timeout never reissues the lock request.
            let state = unsafe { WaitForSingleObject(self.event.handle(), milliseconds) };
            if state == WAIT_TIMEOUT {
                continue;
            }
            if state != WAIT_OBJECT_0 {
                let error = io::Error::last_os_error();
                let mut causes = vec![Cause::Native(NativeFailure {
                    stage: "wait-lock",
                    error,
                })];
                causes.extend(self.cancel_complete().into_iter().map(Cause::Native));
                return Err(Failure { causes });
            }
            self.complete().map_err(|error| Failure {
                causes: vec![Cause::Native(error)],
            })?;
        }
        // An immediate acquisition, or completion racing cancellation, still
        // obeys the original caller's stop ordering before ownership escapes.
        check().map(|_| ()).map_err(|error| Failure {
            causes: vec![Cause::Check(error)],
        })
    }
}
impl Drop for Pending<'_> {
    fn drop(&mut self) {
        // No caller callback here. Unwinding cannot leave an OS request pointing
        // into reclaimed storage. Completion precedes automatic Event/Box Drop.
        let _ = self.cancel_complete();
    }
}

/// `path` names only the fixed workspace lease file selected by Core. The
/// callback returns the original remaining deadline or its original typed stop.
/// Caller keeps the returned File until consuming checked close releases lock.
pub fn acquire<E>(
    path: &Path,
    mut check: impl FnMut() -> Result<Duration, E>,
) -> Result<File, Failure<E>> {
    check().map_err(|error| Failure {
        causes: vec![Cause::Check(error)],
    })?;
    // Match Go's create/seed-close before its overlapped OPEN_EXISTING handle.
    let seed = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(3)
        .open(path)
        .map_err(|error| io_failure("open-seed", error))?;
    let seed_close = crate::filesystem::close_file(seed);
    let stopped = check().err();
    if seed_close.is_err() || stopped.is_some() {
        let mut causes = Vec::new();
        if let Err(error) = seed_close {
            causes.push(Cause::Native(NativeFailure {
                stage: "close-seed",
                error,
            }));
        }
        if let Some(error) = stopped {
            causes.push(Cause::Check(error));
        }
        return Err(Failure { causes });
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(3)
        .custom_flags(FILE_FLAG_OVERLAPPED.0)
        .open(path)
        .map_err(|error| io_failure("open-overlapped", error))?;
    let result = (|| {
        let mut owner = Pending::new(&file).map_err(|error| Failure {
            causes: vec![Cause::Native(error)],
        })?;
        let mut result = match check() {
            Err(error) => Err(Failure {
                causes: vec![Cause::Check(error)],
            }),
            Ok(_) => owner
                .start()
                .map_err(|error| Failure {
                    causes: vec![Cause::Native(error)],
                })
                .and_then(|_| owner.wait(&mut check)),
        };
        // Fixed Go checks ctx after LockFileEx/completion even when that call
        // failed. Keep native failures as supplemental, never erase the stop.
        if let Err(error) = check() {
            match &mut result {
                Err(failure)
                    if failure
                        .causes
                        .iter()
                        .any(|cause| matches!(cause, Cause::Check(_))) =>
                {
                    failure.causes.push(Cause::Check(error))
                }
                Err(failure) => failure.causes.insert(0, Cause::Check(error)),
                Ok(()) => {
                    result = Err(Failure {
                        causes: vec![Cause::Check(error)],
                    })
                }
            }
        }
        if let Err(error) = owner.event.close() {
            match &mut result {
                Err(failure) => failure.causes.push(Cause::Native(error)),
                Ok(()) => {
                    result = Err(Failure {
                        causes: vec![Cause::Native(error)],
                    })
                }
            }
        }
        if let Err(error) = check() {
            match &mut result {
                Err(failure) => failure.causes.push(Cause::Check(error)),
                Ok(()) => {
                    result = Err(Failure {
                        causes: vec![Cause::Check(error)],
                    })
                }
            }
        }
        result
    })();
    match result {
        Ok(()) => Ok(file),
        Err(mut failure) => {
            if let Err(error) = crate::filesystem::close_file(file) {
                failure.causes.push(Cause::Native(NativeFailure {
                    stage: "close-failed-lock",
                    error,
                }));
            }
            if let Err(error) = check() {
                failure.causes.push(Cause::Check(error));
            }
            Err(failure)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::atomic::{AtomicU64, Ordering},
        time::Instant,
    };
    fn path() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/data-lease-fixtures");
        std::fs::create_dir_all(&root).unwrap();
        root.join(format!(
            "{}-{}-{}.lock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    fn holder(path: &Path) -> File {
        let deadline = Instant::now() + Duration::from_secs(5);
        acquire(path, || {
            deadline
                .checked_duration_since(Instant::now())
                .ok_or("fixture deadline")
        })
        .unwrap()
    }
    #[test]
    fn pending_cancel_joins_original_operation_before_file_and_event_drop() {
        let path = path();
        let held = holder(&path);
        let mut checks = 0;
        let failure = acquire(&path, || {
            checks += 1;
            if checks >= 5 {
                Err("original-stop")
            } else {
                Ok(Duration::from_millis(25))
            }
        })
        .unwrap_err();
        assert!(checks >= 5);
        assert!(matches!(
            failure.causes.first(),
            Some(Cause::Check("original-stop"))
        ));
        crate::filesystem::close_file(held).unwrap();
        crate::filesystem::close_file(holder(&path)).unwrap();
    }
    #[test]
    fn pending_callback_unwind_cannot_leave_a_live_request_or_ghost_owner() {
        let path = path();
        let held = holder(&path);
        let mut checks = 0;
        let result = catch_unwind(AssertUnwindSafe(|| {
            acquire(&path, || {
                checks += 1;
                if checks == 4 {
                    panic!("injected callback panic after request queued");
                }
                Ok::<_, ()>(Duration::from_secs(1))
            })
        }));
        assert!(result.is_err());
        assert_eq!(checks, 4);
        crate::filesystem::close_file(held).unwrap();
        crate::filesystem::close_file(holder(&path)).unwrap();
    }
    #[test]
    fn completion_racing_stop_never_returns_an_admitted_file() {
        for _ in 0..16 {
            let path = path();
            let mut held = Some(holder(&path));
            let mut checks = 0;
            let result = acquire(&path, || {
                checks += 1;
                if checks == 4 {
                    crate::filesystem::close_file(held.take().unwrap()).unwrap();
                    return Err("racing-stop");
                }
                Ok(Duration::from_secs(1))
            });
            assert!(matches!(
                result.unwrap_err().causes.first(),
                Some(Cause::Check("racing-stop"))
            ));
            crate::filesystem::close_file(holder(&path)).unwrap();
        }
    }
}
