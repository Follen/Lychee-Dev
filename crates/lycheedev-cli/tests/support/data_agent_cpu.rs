//! Test-only CPU observation of an already identified member of the owned Job.
//! This does not inject work or change the production worker's scheduling.
use lycheedev_windows::ProcessIdentity;
use std::{ffi::c_void, ptr::NonNull};

#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
impl FileTime {
    fn ticks(&self) -> u64 {
        u64::from(self.high) << 32 | u64::from(self.low)
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn GetProcessTimes(
        process: *mut c_void,
        created: *mut FileTime,
        exited: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

pub(crate) struct WorkerCpu {
    handle: Option<NonNull<c_void>>,
    identity: ProcessIdentity,
}
impl WorkerCpu {
    pub(crate) fn open(identity: ProcessIdentity) -> Self {
        // SAFETY: Query/synchronize access only; this fixture owns the returned
        // noninheritable handle. The caller identified this exact owned member.
        let handle = unsafe { OpenProcess(0x1000 | 0x0010_0000, 0, identity.pid) };
        let observer = Self {
            handle: Some(NonNull::new(handle).unwrap_or_else(|| {
                panic!("open CPU observer: {}", std::io::Error::last_os_error())
            })),
            identity,
        };
        observer.user_ticks();
        observer
    }

    pub(crate) fn user_ticks(&self) -> u64 {
        let handle = self.handle.unwrap().as_ptr();
        let (mut created, mut exited, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        // SAFETY: The unique handle remains owned throughout this synchronous
        // call. All four FILETIME pointers name initialized, writable buffers.
        let ok =
            unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
        assert_ne!(
            ok,
            0,
            "CPU observation: {}",
            std::io::Error::last_os_error()
        );
        assert_eq!(
            created.ticks(),
            self.identity.created,
            "worker identity changed"
        );
        // SAFETY: Zero-time observation of the same still-owned process handle.
        let wait = unsafe { WaitForSingleObject(handle, 0) };
        assert_eq!(
            wait, 0x102,
            "worker must still be running at CPU observation"
        );
        user.ticks()
    }

    pub(crate) fn close(mut self) {
        let handle = self.handle.take().unwrap();
        // SAFETY: Taking the unique owner prevents any second close or access.
        let ok = unsafe { CloseHandle(handle.as_ptr()) };
        assert_ne!(
            ok,
            0,
            "close CPU observer: {}",
            std::io::Error::last_os_error()
        );
    }
}
impl Drop for WorkerCpu {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: Panic cleanup releases only this fixture's unique handle.
            let _ = unsafe { CloseHandle(handle.as_ptr()) };
        }
    }
}
