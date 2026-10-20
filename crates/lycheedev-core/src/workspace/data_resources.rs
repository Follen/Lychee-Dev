//! Kernel-owned workspace Data admission. Metadata preparation remains lazy.
use super::{configuration, data_disk};
use crate::{
    Result, ToolkitError,
    control::OperationControl,
    storage::{self, ExclusiveLock},
};
use std::path::PathBuf;
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

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
    concurrent_queries: usize,
    home: PathBuf,
    data_cache_max_bytes: u64,
    control: OperationControl,
}
impl DataResources {
    /// Admission capacity, never a racy count of current holders.
    pub fn concurrent_queries(&self) -> usize {
        self.concurrent_queries
    }
    pub fn download_workers(&self) -> usize {
        self.download_workers
    }
    /// The configured target for total Data disk accounting. The first owner
    /// below covers only the explicitly versioned Data-derived namespace.
    pub fn data_cache_max_bytes(&self) -> u64 {
        self.data_cache_max_bytes
    }
    pub fn data_cache_paths(&self) -> data_disk::DataCachePaths {
        data_disk::DataCachePaths::new(&self.home)
    }
    /// Borrow admission through the reservation so query leases cannot close
    /// before the Data disk owner. Legacy Data writes do not call this yet.
    pub fn reserve_derived_cache(
        &self,
        peak: data_disk::AdditionalPeak,
    ) -> Result<data_disk::DerivedReservation<'_>> {
        data_disk::DerivedReservation::acquire(
            self,
            &self.home,
            self.data_cache_max_bytes,
            peak,
            &self.control,
            None,
        )
    }
    /// Keep an independent query budget/deadline observable inside lock waits,
    /// inventory, every bounded write/copy/hash and checked final cleanup.
    pub fn reserve_derived_cache_with_check<'a>(
        &'a self,
        peak: data_disk::AdditionalPeak,
        check: &'a dyn Fn() -> Result<()>,
    ) -> Result<data_disk::DerivedReservation<'a>> {
        data_disk::DerivedReservation::acquire(
            self,
            &self.home,
            self.data_cache_max_bytes,
            peak,
            &self.control,
            Some(check),
        )
    }
    /// Capacity-triggered typed retirement and write admission under one quota
    /// owner. The caller must retain/serialize returned deletion facts even if
    /// later generation production fails. Unknown manifests fail closed.
    /// `retain_receipt` transfers only the receipt allocation into the caller's
    /// cumulative output budget, on success or partial-retirement error; temporary
    /// snapshot allocations remain refundable.
    pub fn reserve_derived_cache_with_retirement<'a, A>(
        &'a self,
        peak: data_disk::AdditionalPeak,
        check: &'a dyn Fn() -> Result<()>,
        reserve: impl FnMut(usize) -> Result<A>,
        reference: impl FnMut(&str, &[u8]) -> Result<data_disk::DerivedObject>,
        retain_receipt: impl FnMut(A),
    ) -> Result<(
        data_disk::DerivedReservation<'a>,
        Option<data_disk::DerivedRetirementReport<A>>,
    )> {
        data_disk::retirement::admit(
            self,
            &self.home,
            self.data_cache_max_bytes,
            &self.control,
            peak,
            check,
            reserve,
            reference,
            retain_receipt,
        )
    }
    /// Borrow this same query owner for zero-growth derived reads. Existing
    /// over-budget bytes remain visible; the returned capability exposes no
    /// publication, staging or additional-write reservation methods.
    pub fn read_derived_cache_with_check<'a>(
        &'a self,
        check: &'a dyn Fn() -> Result<()>,
    ) -> Result<data_disk::DerivedReadLease<'a>> {
        data_disk::DerivedReadLease::acquire(
            self,
            &self.home,
            self.data_cache_max_bytes,
            &self.control,
            check,
        )
    }
    /// Short zero-growth admission for an owned cursor reader. The check is
    /// borrowed and cloned from the SAME Arc, so detach cannot replace it. The
    /// containing query must close all readers before finishing this admission;
    /// this API neither clones nor reacquires query capacity.
    pub fn read_derived_cache_owned_with_check<'a>(
        &'a self,
        check: &'a data_disk::DerivedReadCheck,
    ) -> Result<data_disk::OwnedDerivedReadLease<'a>> {
        data_disk::OwnedDerivedReadLease::acquire(
            self,
            &self.home,
            self.data_cache_max_bytes,
            &self.control,
            check,
        )
    }
    /// Explicit typed-cache retirement, also usable by a future admission policy.
    /// The domain validates EVERY generation before any deletion; this owner
    /// supplies the unchanged query lifetime and original operation controls.
    /// `reserve` returns the caller's temporary allocation guard; the receipt's
    /// guard travels with the report through its final serialization.
    pub fn retire_derived_cache_with_check<A>(
        &self,
        options: data_disk::DerivedRetirementOptions,
        check: &dyn Fn() -> Result<()>,
        reserve: impl FnMut(usize) -> Result<A>,
        reference: impl FnMut(&str, &[u8]) -> Result<data_disk::DerivedObject>,
    ) -> Result<data_disk::DerivedRetirementReport<A>> {
        data_disk::retirement::retire(
            self,
            &self.home,
            self.data_cache_max_bytes,
            &self.control,
            options,
            check,
            reserve,
            reference,
        )
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
pub(super) fn finish<T>(
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
    let config = configuration::read(home, control, &mut check)?;
    let workers = config.download_workers;
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
        home: home.to_path_buf(),
        data_cache_max_bytes: config.data_cache_max_bytes,
        download_workers: width,
        concurrent_queries: if policy == Policy::Measurement {
            1
        } else {
            slots
        },
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
            concurrent_queries: 1,
            home: PathBuf::new(),
            data_cache_max_bytes: super::super::DEFAULT_DATA_CACHE_MAX_BYTES,
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

/// Minimal safe bridge to the existing Windows FFI crate. Query owners decide
/// when to observe memory; this function does not acquire or infer admission.
#[cfg(windows)]
pub fn available_query_memory() -> Result<u64> {
    lycheedev_windows::memory::available_memory()
        .map_err(|error| ToolkitError::new("windows.memory_status", error, 3))
}
