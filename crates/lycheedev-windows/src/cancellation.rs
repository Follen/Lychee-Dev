//! Console signals may notify a caller-owned cancellation state without
//! exposing its writable token or depending on the Core crate.
use crate::Result;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex, Weak},
};
use windows::{
    Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler},
    core::BOOL,
};

type Callback = dyn Fn() + Send + Sync + 'static;
enum Slot {
    Empty,
    Installing(Weak<Callback>),
    Active(Weak<Callback>),
    Removing,
    // Do not register another copy if removal of the OS handler failed.
    RemovalFailed,
}
struct Registry(Mutex<Slot>);
impl Registry {
    const fn new() -> Self {
        Self(Mutex::new(Slot::Empty))
    }
    fn install(
        &self,
        callback: &Arc<Callback>,
        register: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        {
            let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
            match &*slot {
                Slot::Empty => *slot = Slot::Installing(Arc::downgrade(callback)),
                Slot::RemovalFailed => return Err("console.handler_removal_failed".into()),
                _ => return Err("console.handler_busy".into()),
            }
        }
        // No registry lock crosses FFI. A signal arriving after registration
        // but before Active can already find the Installing callback.
        let result = register();
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        *slot = if result.is_ok() {
            Slot::Active(Arc::downgrade(callback))
        } else {
            Slot::Empty
        };
        result
    }
    fn remove(
        &self,
        callback: &Arc<Callback>,
        unregister: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        {
            let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
            match &*slot {
                Slot::Active(active) if Weak::ptr_eq(active, &Arc::downgrade(callback)) => {
                    *slot = Slot::Removing;
                }
                _ => return Err("console.handler_registration_changed".into()),
            }
        }
        // Detach first. In-flight dispatch owns an Arc to the old callback and
        // may finish after removal; it cannot borrow the guard or a new slot.
        let result = unregister();
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = if result.is_ok() {
            Slot::Empty
        } else {
            Slot::RemovalFailed
        };
        result
    }
    fn callback(&self) -> Option<Arc<Callback>> {
        let slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        match &*slot {
            Slot::Installing(callback) | Slot::Active(callback) => callback.upgrade(),
            _ => None,
        }
    }
}
static ACTIVE: Registry = Registry::new();

fn dispatch(registry: &Registry, kind: u32) -> bool {
    if ![CTRL_C_EVENT, CTRL_BREAK_EVENT].contains(&kind) {
        return false;
    }
    // Both invocation and the last Arc's destruction happen outside the
    // registry lock and inside the unwind boundary. A callback panic does not
    // claim successful cancellation or unwind into the Windows caller.
    match catch_unwind(AssertUnwindSafe(|| {
        if let Some(callback) = registry.callback() {
            callback();
            true
        } else {
            false
        }
    })) {
        Ok(handled) => handled,
        Err(payload) => match catch_unwind(AssertUnwindSafe(|| drop(payload))) {
            Ok(()) => false,
            // A malicious panic payload may itself panic on Drop. Abort
            // rather than unwinding across FFI or leaking one per signal.
            Err(_payload) => std::process::abort(),
        },
    }
}
// SAFETY: Windows invokes this fixed ABI entry on an OS-owned thread. The
// process-static registry contains only synchronized weak references, and
// dispatch takes a strong reference before releasing its lock. No borrowed
// caller state or raw pointer crosses the entry, and panics cannot unwind out.
unsafe extern "system" fn handler(kind: u32) -> BOOL {
    BOOL(i32::from(dispatch(&ACTIVE, kind)))
}
fn set_handler(add: bool) -> Result<()> {
    // SAFETY: `handler` is a process-lifetime function with the required ABI.
    // Its state is synchronized and remains valid even if unregister fails or
    // a previously dispatched call finishes after the registration is dropped.
    unsafe { SetConsoleCtrlHandler(Some(handler), add).map_err(|e| e.to_string()) }
}

/// Owns one callback registration, with no writable cancellation token.
/// Dropping detaches the callback; a call that already took its Arc snapshot
/// may still finish. Signal routing linearizes at that snapshot, not at the
/// OS event's delivery time, which Windows does not expose to the callback.
/// Captures must therefore be owned (`'static`) and safe on OS handler threads.
pub struct ConsoleCancellationRegistration {
    callback: Option<Arc<Callback>>,
}
impl ConsoleCancellationRegistration {
    /// Explicitly remove the handler and report an OS removal failure. Drop
    /// makes the same attempt once; failure blocks later registrations.
    pub fn uninstall(mut self) -> Result<()> {
        self.remove()
    }
    fn remove(&mut self) -> Result<()> {
        match self.callback.take() {
            Some(callback) => ACTIVE.remove(&callback, || set_handler(false)),
            None => Ok(()),
        }
    }
}
impl Drop for ConsoleCancellationRegistration {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

pub struct ConsoleCancellation;
impl ConsoleCancellation {
    /// Invoke `cancel` synchronously for Ctrl+C/Break, without a worker thread
    /// or polling delay. The callback must be short, bounded and avoid I/O or
    /// waiting for another handler. Concurrent signals may invoke it together.
    /// A panic is unhandled, allowing Windows to continue its handler chain.
    pub fn install_callback(
        cancel: impl Fn() + Send + Sync + 'static,
    ) -> Result<ConsoleCancellationRegistration> {
        let callback: Arc<Callback> = Arc::new(cancel);
        ACTIVE.install(&callback, || set_handler(true))?;
        Ok(ConsoleCancellationRegistration {
            callback: Some(callback),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn signal_during_registration_runs_outside_lock_and_busy_is_explicit() {
        let registry = Arc::new(Registry::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let target = calls.clone();
        let observed_registry = registry.clone();
        let callback: Arc<Callback> = Arc::new(move || {
            assert!(observed_registry.0.try_lock().is_ok());
            target.fetch_add(1, Ordering::SeqCst);
        });
        registry
            .install(&callback, || {
                assert!(dispatch(&registry, CTRL_C_EVENT));
                assert_eq!(
                    registry.install(&callback, || panic!("must not register twice")),
                    Err("console.handler_busy".into())
                );
                Ok(())
            })
            .unwrap();
        assert!(dispatch(&registry, CTRL_BREAK_EVENT));
        assert!(!dispatch(&registry, 2)); // CTRL_CLOSE_EVENT remains unhandled.
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        registry.remove(&callback, || Ok(())).unwrap();
        assert!(!dispatch(&registry, CTRL_C_EVENT));
    }

    #[test]
    fn failed_install_releases_slot_but_failed_remove_blocks_duplicate_handler() {
        let registry = Registry::new();
        let callback: Arc<Callback> = Arc::new(|| {});
        assert_eq!(
            registry.install(&callback, || Err("fixture.install".into())),
            Err("fixture.install".into())
        );
        assert!(!dispatch(&registry, CTRL_C_EVENT));
        registry.install(&callback, || Ok(())).unwrap();
        let other: Arc<Callback> = Arc::new(|| {});
        assert_eq!(
            registry.remove(&other, || panic!("must not remove another registration")),
            Err("console.handler_registration_changed".into())
        );
        assert_eq!(
            registry.remove(&callback, || {
                assert!(!dispatch(&registry, CTRL_C_EVENT));
                assert_eq!(
                    registry.install(&other, || panic!("must not register during removal")),
                    Err("console.handler_busy".into())
                );
                Err("fixture.remove".into())
            }),
            Err("fixture.remove".into())
        );
        assert!(!dispatch(&registry, CTRL_C_EVENT));
        assert_eq!(
            registry.install(&other, || panic!("must not register after failed removal")),
            Err("console.handler_removal_failed".into())
        );
    }

    #[test]
    fn callback_panic_is_unhandled_without_poisoning_registration() {
        let registry = Registry::new();
        let callback: Arc<Callback> = Arc::new(|| panic!("fixture callback panic"));
        registry.install(&callback, || Ok(())).unwrap();
        assert!(!dispatch(&registry, CTRL_C_EVENT));
        registry.remove(&callback, || Ok(())).unwrap();
        let callback: Arc<Callback> = Arc::new(|| {});
        registry.install(&callback, || Ok(())).unwrap();
        assert!(dispatch(&registry, CTRL_C_EVENT));
        registry.remove(&callback, || Ok(())).unwrap();
    }

    #[test]
    fn contended_registry_does_not_drop_a_signal() {
        use std::{sync::mpsc, time::Duration};
        let registry = Arc::new(Registry::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = cancelled.clone();
        let callback: Arc<Callback> = Arc::new(move || signal.store(true, Ordering::Release));
        registry.install(&callback, || Ok(())).unwrap();
        let held = registry.0.lock().unwrap();
        let (started, start) = mpsc::sync_channel(1);
        let (finished, finish) = mpsc::sync_channel(1);
        let worker_registry = registry.clone();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            finished
                .send(dispatch(&worker_registry, CTRL_C_EVENT))
                .unwrap();
        });
        start.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            finish.recv_timeout(Duration::from_millis(10)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        drop(held);
        assert!(finish.recv_timeout(Duration::from_secs(5)).unwrap());
        worker.join().unwrap();
        assert!(cancelled.load(Ordering::Acquire));
        registry.remove(&callback, || Ok(())).unwrap();
    }

    #[test]
    fn in_flight_callback_keeps_only_old_state_alive_after_removal() {
        use std::sync::mpsc;
        use std::time::Duration;
        let registry = Arc::new(Registry::new());
        let old_calls = Arc::new(AtomicUsize::new(0));
        let weak_old = Arc::downgrade(&old_calls);
        let new_calls = Arc::new(AtomicUsize::new(0));
        let (entered, entry) = mpsc::sync_channel(1);
        let (release, exit) = mpsc::sync_channel(1);
        let exit = Mutex::new(exit);
        let callback: Arc<Callback> = Arc::new(move || {
            entered.send(()).unwrap();
            exit.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            old_calls.fetch_add(1, Ordering::SeqCst);
        });
        registry.install(&callback, || Ok(())).unwrap();
        let worker_registry = registry.clone();
        let worker = std::thread::spawn(move || dispatch(&worker_registry, CTRL_C_EVENT));
        entry.recv_timeout(Duration::from_secs(5)).unwrap();
        registry.remove(&callback, || Ok(())).unwrap();
        drop(callback);
        assert!(weak_old.upgrade().is_some());
        let target = new_calls.clone();
        let replacement: Arc<Callback> = Arc::new(move || {
            target.fetch_add(1, Ordering::SeqCst);
        });
        registry.install(&replacement, || Ok(())).unwrap();
        release.send(()).unwrap();
        assert!(worker.join().unwrap());
        assert!(weak_old.upgrade().is_none());
        assert_eq!(new_calls.load(Ordering::SeqCst), 0);
        assert!(dispatch(&registry, CTRL_BREAK_EVENT));
        assert_eq!(new_calls.load(Ordering::SeqCst), 1);
        registry.remove(&replacement, || Ok(())).unwrap();
    }

    #[test]
    fn real_registration_preserves_callback_ownership() {
        let observed = Arc::new(AtomicUsize::new(0));
        let target = observed.clone();
        let registration = ConsoleCancellation::install_callback(move || {
            target.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        assert!(dispatch(&ACTIVE, CTRL_BREAK_EVENT));
        assert_eq!(observed.load(Ordering::SeqCst), 1);
        registration.uninstall().unwrap();
        assert!(!dispatch(&ACTIVE, CTRL_C_EVENT));
        assert_eq!(Arc::strong_count(&observed), 1);
    }
}
