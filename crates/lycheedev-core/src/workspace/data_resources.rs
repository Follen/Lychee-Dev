//! Kernel-owned workspace Data admission. Metadata preparation remains lazy.
use crate::{
    Result, ToolkitError,
    control::OperationControl,
    storage::{self, ExclusiveLock},
};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
mod config;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Ordinary,
    Measurement,
}
enum Lease {
    Immediate(ExclusiveLock),
    #[cfg(windows)]
    Queued(fs::File),
    #[cfg(test)]
    InjectedClose {
        name: &'static str,
        calls: Arc<std::sync::Mutex<Vec<&'static str>>>,
        error: ToolkitError,
    },
}
impl Lease {
    fn close(self) -> Result<()> {
        match self {
            Self::Immediate(lease) => lease.close(),
            #[cfg(test)]
            Self::InjectedClose { name, calls, error } => {
                calls.lock().unwrap().push(name);
                Err(error)
            }
            #[cfg(windows)]
            Self::Queued(file) => {
                lycheedev_windows::filesystem::close_file(file).map_err(ToolkitError::io)
            }
        }
    }
}
/// This owner, not its numeric width, is admission. A Data read session retains
/// it through all query work and checked teardown; do not acquire it twice.
pub struct DataResources {
    leases: Vec<Lease>,
    download_workers: usize,
    control: OperationControl,
}
impl DataResources {
    pub fn download_workers(&self) -> usize {
        self.download_workers
    }
    fn close_all(&mut self) -> Vec<ToolkitError> {
        let mut errors = Vec::new();
        while let Some(lease) = self.leases.pop() {
            if let Err(error) = lease.close() {
                errors.push(error);
            }
        }
        errors
    }
    /// Consume only this owner's reverse-order handle cleanup. The outer owner
    /// may then close its Metadata/root before observing its original scopes.
    /// No stop is sampled here and no cleanup event is deduplicated.
    pub fn close_cleanup(mut self) -> Vec<ToolkitError> {
        self.close_all()
    }
    pub fn close(self) -> Result<()> {
        self.close_with_check(|| Ok(()))
    }
    pub fn close_with_check(mut self, mut check: impl FnMut() -> Result<()>) -> Result<()> {
        let cleanup = self.close_all();
        finish(Ok(()), cleanup, self.control.check(), check())
    }
}
impl Drop for DataResources {
    fn drop(&mut self) {
        // Fallback only; consuming close has already emptied the owner vector.
        // Every handle is consumed once, never retried by numeric identity.
        let _ = self.close_all();
    }
}
fn same(a: &ToolkitError, b: &ToolkitError) -> bool {
    a.code == b.code
        && a.message == b.message
        && a.exit_code == b.exit_code
        && a.details == b.details
}
fn combined(errors: impl IntoIterator<Item = ToolkitError>) -> Option<ToolkitError> {
    // Every operation/cleanup event survives, even equal errors from separate
    // handles. Only repeated observations through check() are deduplicated.
    let all: Vec<_> = errors.into_iter().collect();
    let mut first = all.first()?.clone();
    if all.len() > 1 {
        first.details =
            serde_json::json!({"schema":"lycheedev.data-admission-errors.v1","causes":all});
    }
    Some(first)
}
fn append_check(causes: &mut Vec<ToolkitError>, error: ToolkitError) {
    if !causes.iter().any(|previous| same(previous, &error)) {
        causes.push(error);
    }
}
fn finish<T>(
    result: Result<T>,
    cleanup: impl IntoIterator<Item = ToolkitError>,
    control: Result<()>,
    check: Result<()>,
) -> Result<T> {
    let (value, error) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    let mut causes: Vec<_> = error.into_iter().chain(cleanup).collect();
    for observed in control.err().into_iter().chain(check.err()) {
        append_check(&mut causes, observed);
    }
    match combined(causes) {
        Some(error) => Err(error),
        None => Ok(value.expect("successful admission operation retains its result")),
    }
}
fn pure_busy(error: &ToolkitError) -> bool {
    error.code == "workspace.busy" && error.details.is_null()
}
fn check_scope(control: &OperationControl, check: &mut dyn FnMut() -> Result<()>) -> Result<()> {
    control.check()?;
    check()
}
fn bounded(control: &OperationControl) -> Result<OperationControl> {
    control.check()?;
    let deadline = control
        .deadline()
        .min(Instant::now() + Duration::from_secs(30));
    match control.ordered_child(deadline) {
        Ok(child) => Ok(child),
        Err(error)
            if error.code == "control.cancellation_order_unavailable"
                && error.details.is_null() =>
        {
            Ok(control.child(deadline, Arc::new(AtomicBool::new(false))))
        }
        Err(error) => Err(error),
    }
}
fn immediate(
    scope: &Path,
    name: &str,
    control: &OperationControl,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<Lease> {
    ExclusiveLock::resource_checked(scope, name, || check_scope(control, check))
        .map(Lease::Immediate)
}
fn wait(
    scope: &Path,
    name: &str,
    control: &OperationControl,
    check: &mut dyn FnMut() -> Result<()>,
) -> Result<Lease> {
    check_scope(control, check)?;
    #[cfg(windows)]
    {
        use lycheedev_windows::data_lease::{Cause, Failure, NativeFailure};
        fn native(error: NativeFailure) -> ToolkitError {
            let os_error = error.error.raw_os_error();
            ToolkitError::io(error.error)
                .with_details(serde_json::json!({"stage":error.stage,"osError":os_error}))
        }
        fs::create_dir_all(scope)?;
        let path = scope.join(format!("{}.lock", storage::sha256(name.as_bytes())));
        match lycheedev_windows::data_lease::acquire(&path, || {
            check_scope(control, check)?;
            control.remaining()
        }) {
            Ok(file) => Ok(Lease::Queued(file)),
            Err(Failure { causes }) => {
                let mut errors = Vec::new();
                for cause in causes {
                    match cause {
                        Cause::Check(error) => append_check(&mut errors, error),
                        Cause::Native(error) => errors.push(native(error)),
                    }
                }
                Err(combined(errors).expect("failed kernel acquisition has a cause"))
            }
        }
    }
    #[cfg(not(windows))]
    {
        // Go's non-Windows development path has the existing 25ms try-lock
        // loop. It is not evidence for Windows queued-kernel behavior.
        loop {
            check_scope(control, check)?;
            match immediate(scope, name, control, check) {
                Err(error) if pure_busy(&error) => {
                    std::thread::sleep(control.remaining()?.min(Duration::from_millis(25)))
                }
                result => return result,
            }
        }
    }
}
pub fn acquire(home: &Path, policy: Policy, control: &OperationControl) -> Result<DataResources> {
    acquire_with_check(home, policy, control, || Ok(()))
}
pub fn acquire_with_check(
    home: &Path,
    policy: Policy,
    control: &OperationControl,
    mut check: impl FnMut() -> Result<()>,
) -> Result<DataResources> {
    let workers = config::read(home, control, &mut check)?;
    let bounded = bounded(control)?;
    check_scope(&bounded, &mut check)?;
    let scope = home.join("locks/data-resources-v1");
    let gate = wait(&scope, "admission", &bounded, &mut check)?;
    let slots = if workers == 1 { 1 } else { 2 };
    let width = if workers == 0 {
        8
    } else {
        (workers / slots).clamp(1, 8)
    };
    let mut owner = DataResources {
        leases: vec![gate],
        download_workers: width,
        control: control.clone(),
    };
    let result = (|| {
        if policy == Policy::Measurement {
            for slot in 0..2 {
                owner
                    .leases
                    .push(wait(&scope, &format!("slot:{slot}"), &bounded, &mut check)?);
            }
            return Ok(());
        }
        let mut selected = None;
        for slot in 0..slots {
            match immediate(&scope, &format!("slot:{slot}"), &bounded, &mut check) {
                Ok(lease) => {
                    selected = Some(lease);
                    break;
                }
                Err(error) if pure_busy(&error) => (),
                Err(error) => return Err(error),
            }
        }
        // Hold admission while queuing specifically for slot zero. Do not let
        // later ordinary callers overtake a queued measurement by polling both.
        let lease = match selected {
            Some(lease) => lease,
            None => wait(&scope, "slot:0", &bounded, &mut check)?,
        };
        owner.leases.push(lease);
        owner.leases.remove(0).close()?;
        Ok(())
    })();
    let result = finish(result, [], bounded.check(), check());
    match result {
        Ok(()) => Ok(owner),
        Err(error) => {
            let cleanup = owner.close_all();
            finish(Err(error), cleanup, control.check(), check())
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    #[test]
    fn cleanup_only_preserves_each_equal_close_event_in_reverse_order_without_stop() {
        let control = OperationControl::default();
        control
            .cancel_handle()
            .store(true, std::sync::atomic::Ordering::Release);
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let error = ToolkitError::new("test.close", "injected close failure", 5);
        let owner = DataResources {
            leases: ["gate", "slot:0", "slot:1"]
                .into_iter()
                .map(|name| Lease::InjectedClose {
                    name,
                    calls: calls.clone(),
                    error: error.clone(),
                })
                .collect(),
            download_workers: 1,
            control,
        };
        let errors = owner.close_cleanup();
        assert_eq!(*calls.lock().unwrap(), ["slot:1", "slot:0", "gate"]);
        assert_eq!(errors.len(), 3);
        for actual in errors {
            assert!(same(&actual, &error));
        }
    }
}
