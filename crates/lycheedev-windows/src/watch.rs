//! Event-driven invalidation for immutable input trees. Any change or lost
//! notification retires the session. No timer or readiness polling is installed.
use crate::Result;
use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
use windows::{
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{IO::*, Threading::*},
    },
    core::PCWSTR,
};
struct Handle(HANDLE);
impl Handle {
    fn raw(&self) -> HANDLE {
        self.0
    }
}
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub struct ChangeWatch {
    stop: Arc<Handle>,
    notification: Arc<Handle>,
    changed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl ChangeWatch {
    pub fn start(path: &Path, recursive: bool, cancel: Arc<AtomicBool>) -> Result<Self> {
        if cancel.load(Ordering::Acquire) {
            return Err("watch.cancelled".into());
        }
        let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err("watch.directory_invalid".into());
        }
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let directory = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED | FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
            .map(Handle)
            .map_err(|e| e.to_string())?
        };
        let stop = Arc::new(unsafe {
            CreateEventW(None, true, false, None)
                .map(Handle)
                .map_err(|e| e.to_string())?
        });
        let notification = Arc::new(unsafe {
            CreateEventW(None, true, false, None)
                .map(Handle)
                .map_err(|e| e.to_string())?
        });
        let changed = Arc::new(AtomicBool::new(false));
        let owned_stop = stop.clone();
        let owned_notification = notification.clone();
        let owned_changed = changed.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("immutable-input-watch".into())
            .spawn(move || {
                let mut buffer = vec![0u8; 65536];
                let mut overlapped = Box::new(OVERLAPPED::default());
                overlapped.hEvent = owned_notification.0;
                let filter = FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_ATTRIBUTES
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_CREATION
                    | FILE_NOTIFY_CHANGE_SECURITY;
                let arm = unsafe {
                    ReadDirectoryChangesW(
                        directory.raw(),
                        buffer.as_mut_ptr() as *mut _,
                        buffer.len() as u32,
                        recursive,
                        filter,
                        None,
                        Some(&mut *overlapped),
                        None,
                    )
                };
                if let Err(error) = arm {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
                let _ = ready_tx.send(Ok(()));
                let status = unsafe {
                    WaitForMultipleObjects(&[owned_stop.0, owned_notification.0], false, INFINITE)
                };
                if status == WAIT_OBJECT_0 {
                    if unsafe { WaitForSingleObject(owned_notification.0, 0) } == WAIT_OBJECT_0 {
                        owned_changed.store(true, Ordering::Release);
                        cancel.store(true, Ordering::Release);
                    }
                    unsafe {
                        let _ = CancelIoEx(directory.raw(), Some(&*overlapped));
                        let mut n = 0;
                        let _ = GetOverlappedResult(directory.raw(), &*overlapped, &mut n, true);
                    }
                    return;
                }
                // Completion, overflow, and unexpected wait failures all invalidate.
                owned_changed.store(true, Ordering::Release);
                cancel.store(true, Ordering::Release);
                unsafe {
                    let _ = CancelIoEx(directory.raw(), Some(&*overlapped));
                    let mut n = 0;
                    let _ = GetOverlappedResult(directory.raw(), &*overlapped, &mut n, true);
                }
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                stop,
                notification,
                changed,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                Err(error.to_string())
            }
        }
    }
    pub fn changed(&self) -> bool {
        self.changed.load(Ordering::Acquire)
            || unsafe { WaitForSingleObject(self.notification.0, 0) == WAIT_OBJECT_0 }
    }
}
impl Drop for ChangeWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = SetEvent(self.stop.0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_insert_delete_invalidates_and_idle_drop_joins() {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-watch-{}",
            crate::random::token().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let watch = ChangeWatch::start(&root, true, cancel.clone()).unwrap();
        assert!(!watch.changed());
        let child = root.join("transient.lua");
        std::fs::write(&child, "return 1").unwrap();
        std::fs::remove_file(&child).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !watch.changed() && std::time::Instant::now() < deadline {
            std::thread::yield_now()
        }
        assert!(watch.changed());
        drop(watch);
        assert!(cancel.load(Ordering::Acquire));
        let idle = ChangeWatch::start(&root, false, Arc::new(AtomicBool::new(false))).unwrap();
        drop(idle);
    }
}
