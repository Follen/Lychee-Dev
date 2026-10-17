//! Opt-in Go AcquireLease semantics for coalescing identical blob publishers.
//! Immediate lock APIs and every other caller retain their previous behavior.
use super::*;
use std::time::{Duration, Instant};

impl ExclusiveLock {
    /// Source-cache-only repeated publication. Keep the exact resource hash,
    /// kernel lock, original deadline and checked close, while a real held
    /// strict Directory replaces per-file path-based directory preparation.
    pub(crate) fn resource_wait_in_directory(
        scope: &Directory,
        resource: &str,
        control: &crate::control::OperationControl,
    ) -> Result<Self> {
        struct Controlled<'a>(&'a crate::control::OperationControl);
        impl FileControl for Controlled<'_> {
            type Error = ToolkitError;
            fn charge(&mut self, _: FileCost) -> Result<()> {
                self.check()
            }
            fn check(&mut self) -> Result<()> {
                self.0.check()
            }
        }
        Self::acquire_prepared_with(
            &scope
                .path()
                .join(format!("{}.lock", sha256(resource.as_bytes()))),
            || control.check(),
            || scope.verify_controlled(&mut Controlled(control)),
            |file, check| {
                Self::wait_on_file(
                    file,
                    check,
                    Some(control.deadline()),
                    fs2::FileExt::try_lock_exclusive,
                    std::thread::sleep,
                )
            },
            lycheedev_windows::filesystem::close_file,
        )
    }
    /// Fixed-Go multi-file publishers share one original lock handle while
    /// waiting, with invocation-first supplemental checks on every attempt.
    pub fn resource_wait_controlled_with_check(
        scope: &Path,
        resource: &str,
        control: &crate::control::OperationControl,
        mut supplemental: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        Self::resource_wait_checked(scope, resource, Some(control.deadline()), || {
            control.check()?;
            supplemental()
        })
    }
    /// The original deadline only caps sleeps; `check` owns typed invocation and
    /// independent budget stops. One open File is retained across every retry.
    pub(crate) fn resource_wait_checked(
        scope: &Path,
        resource: &str,
        deadline: Option<Instant>,
        check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        Self::acquire_wait_with(
            &scope.join(format!("{}.lock", sha256(resource.as_bytes()))),
            deadline,
            check,
            fs2::FileExt::try_lock_exclusive,
            std::thread::sleep,
            lycheedev_windows::filesystem::close_file,
        )
    }
    fn acquire_wait_with(
        path: &Path,
        deadline: Option<Instant>,
        check: impl FnMut() -> Result<()>,
        lock: impl FnMut(&File) -> std::io::Result<()>,
        sleep: impl FnMut(Duration),
        close: impl FnOnce(File) -> std::io::Result<()>,
    ) -> Result<Self> {
        Self::acquire_checked_with(
            path,
            check,
            |file, check| Self::wait_on_file(file, check, deadline, lock, sleep),
            close,
        )
    }
    fn wait_on_file(
        file: &File,
        check: &mut dyn FnMut() -> Result<()>,
        deadline: Option<Instant>,
        mut lock: impl FnMut(&File) -> std::io::Result<()>,
        mut sleep: impl FnMut(Duration),
    ) -> Result<()> {
        loop {
            check()?;
            match lock(file) {
                Ok(()) => return check(),
                Err(error)
                    if error.raw_os_error().is_some()
                        && error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {}
                Err(error) => return Err(ToolkitError::io(error)),
            }
            #[cfg(test)]
            observe_contention();
            // Contention is not the request's failure. Re-observe the
            // original invocation/budget before and after this bounded wait.
            check()?;
            let interval = Duration::from_millis(25);
            let delay = deadline.map_or(interval, |until| {
                interval.min(until.saturating_duration_since(Instant::now()))
            });
            sleep(delay);
            check()?;
        }
    }
}

#[cfg(test)]
#[path = "blob_wait/tests.rs"]
mod tests;

#[cfg(test)]
thread_local! {
    static OBSERVER: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}
#[cfg(test)]
impl ExclusiveLock {
    pub(crate) fn set_blob_wait_observer(run: impl FnOnce() + 'static) {
        OBSERVER.with(|slot| {
            assert!(slot.borrow().is_none());
            *slot.borrow_mut() = Some(Box::new(run));
        });
    }
}
#[cfg(test)]
fn observe_contention() {
    let run = OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(run) = run {
        run();
    }
}
