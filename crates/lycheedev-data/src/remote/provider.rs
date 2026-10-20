//! Source-neutral query admission and lexical local/CDN file owners.
//! Numeric download width alone is never admission.
mod local;
use super::{
    admission::{AdmittedWorkspace, OpenWorkspace},
    cache::Store,
    fragments,
    session::{Options, Session},
};
use crate::{
    local_query::{File, FileBytes},
    resource::{Budget, Cost},
};
use lycheedev_core::{Invocation, Result, selection::DataPin, workspace::data_resources::Policy};
use std::sync::Arc;

fn resolve_pin(
    invocation: &Invocation,
    budget: &Arc<Budget>,
) -> Result<lycheedev_core::selection::PinnedSet> {
    lycheedev_core::selection::resolve_pin_request_controlled_with_check(
        lycheedev_core::selection::PinSelectionRequest {
            home: &invocation.home,
            cwd: &invocation.cwd,
            snapshot: invocation.flag("--snapshot"),
            project: invocation.flag("--project"),
        },
        &invocation.control,
        || budget.check(),
    )
}

/// No file-read method exists until this owner has acquired real workspace slots.
pub(crate) struct OpenedScope<'a> {
    workspace: OpenWorkspace<'a>,
    invocation: &'a Invocation,
    budget: &'a Arc<Budget>,
}
impl<'a> OpenedScope<'a> {
    pub fn open(invocation: &'a Invocation, budget: &'a Arc<Budget>) -> Result<Self> {
        let workspace =
            OpenWorkspace::open_metadata(&invocation.home, &invocation.control, Some(budget))?;
        Ok(Self {
            workspace,
            invocation,
            budget,
        })
    }
    pub fn admit(self) -> Result<Scope<'a>> {
        let admitted = self.workspace.acquire(Policy::Ordinary)?;
        Ok(Scope {
            admitted,
            invocation: self.invocation,
            budget: self.budget,
        })
    }
    /// Asset capture resolves the pin before admission but the Reader validates
    /// DataIdentity only after admission (fixed Go captureAsset / readFile).
    pub fn admit_asset(self) -> Result<(Scope<'a>, lycheedev_core::selection::PinnedSet)> {
        let result = (|| {
            self.workspace.check()?;
            let pin = resolve_pin(self.invocation, self.budget)?;
            if pin.data.is_none() {
                return Err(crate::fault(
                    "records.data_pin_required",
                    2,
                    "records.data_pin_required",
                ));
            }
            Ok(pin)
        })();
        match result {
            Ok(pin) => Ok((self.admit()?, pin)),
            Err(error) => self.workspace.finish(Err(error)),
        }
    }
    /// SQL and ordinary DB2 validate their snapshot before the resource wait.
    pub fn admit_pinned(
        self,
        missing: lycheedev_core::ToolkitError,
    ) -> Result<(Scope<'a>, lycheedev_core::selection::PinnedSet)> {
        let result = (|| {
            self.workspace.check()?;
            let pin = resolve_pin(self.invocation, self.budget)?;
            let data = pin.data.as_ref().ok_or(missing)?;
            fragments::data_identity(data)?;
            Ok(pin)
        })();
        match result {
            Ok(pin) => Ok((self.admit()?, pin)),
            Err(error) => self.workspace.finish(Err(error)),
        }
    }
}

pub(crate) struct Scope<'a> {
    admitted: AdmittedWorkspace<'a>,
    invocation: &'a Invocation,
    budget: &'a Arc<Budget>,
}
impl<'a> Scope<'a> {
    /// Lend the original admitted writer for one synchronous consumer scope.
    /// Sessions may borrow the admission while captures separately borrow this
    /// connection. No transaction spans a callback, codec or external output.
    /// Every ordinary result restores ownership before the existing checked
    /// resource -> metadata -> root teardown. Panic fallback drops the local
    /// connection before the still-held workspace owner.
    pub fn with_capture_metadata<T>(
        &mut self,
        work: impl FnOnce(&mut Self, &mut lycheedev_core::metadata::Metadata) -> Result<T>,
    ) -> Result<T> {
        let mut metadata = self.admitted.take_capture_metadata();
        let result = work(self, &mut metadata);
        self.admitted.restore_capture_metadata(metadata);
        result
    }
    pub fn check(&self) -> Result<()> {
        self.admitted.check()
    }
    pub fn check_pin(&self, pin: &DataPin) -> Result<()> {
        self.check()?;
        fragments::data_identity(pin).map(|_| ())
    }
    pub fn check_consumer(&self, invocation: &Invocation, budget: &Arc<Budget>) -> Result<()> {
        self.check()?;
        if !std::ptr::eq(self.invocation, invocation) || !Arc::ptr_eq(self.budget, budget) {
            return Err(crate::fault(
                "records.file_query",
                4,
                "Consumer differs from admitted invocation/budget",
            ));
        }
        Ok(())
    }
    pub fn resolve_pin(&self) -> Result<lycheedev_core::selection::PinnedSet> {
        self.check()?;
        resolve_pin(self.invocation, self.budget)
    }
    /// One lexical query reader owns preparation through scans and capture.
    /// Creating it is lazy; no file/keys/Encoding IO happens before a read.
    pub fn with_session<T>(
        &mut self,
        pin: &DataPin,
        work: impl FnOnce(&mut SessionScope<'_, 'a>) -> Result<T>,
    ) -> Result<T> {
        self.check_pin(pin)?;
        let mut reader = SessionScope {
            session: None,
            admitted: Some(&mut self.admitted),
            invocation: self.invocation,
            budget: self.budget,
            pin,
        };
        let result = work(&mut reader);
        let cleanup = reader.close();
        Store {
            home: self.admitted.home(),
            control: self.admitted.control(),
            budget: Some(self.budget),
        }
        .finish(result, cleanup)
    }
    /// Retain the original authenticated local observations through every
    /// consumer callback and capture. This borrows the existing admission;
    /// no fresh source snapshot or workspace lease is acquired at finalization.
    pub fn with_local_session<T>(
        &mut self,
        pin: &DataPin,
        work: impl FnOnce(&mut dyn FileScope) -> Result<T>,
    ) -> Result<T> {
        self.check_pin(pin)?;
        let mut reader = local::Session::new(self, pin)?;
        let result = work(&mut reader);
        reader.close_after(result)
    }
    pub fn read_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.with_session(pin, |reader| reader.read_batches(pin, ids, callback))
    }
    pub fn read_table_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.with_session(pin, |reader| reader.read_table_batches(pin, ids, callback))
    }
    /// The caller borrows the complete immutable body through capture/output.
    /// Session retains its descriptor and checks close before this method returns.
    pub fn with_verified_file<R>(
        &mut self,
        pin: &DataPin,
        id: u32,
        callback: &mut dyn FnMut(
            &mut lycheedev_core::evidence::VerifiedBlob,
            &serde_json::Value,
        ) -> Result<R>,
    ) -> Result<R> {
        self.with_session(pin, |reader| {
            reader
                .prepare_reader()?
                .with_verified(id, &mut |file| callback(&mut file.blob, &file.reading))
        })
    }
    pub fn finish<T>(self, result: Result<T>) -> Result<T> {
        self.admitted.finish(result)
    }
    #[cfg(test)]
    pub(crate) fn finish_after_metadata_close<T>(
        self,
        result: Result<T>,
        after_close: &mut dyn FnMut() -> Result<()>,
    ) -> Result<T> {
        self.admitted
            .finish_after_metadata_close(result, after_close)
    }
}

/// Only these source-neutral operations are lent to DB2/navigation consumers.
/// It does not own or release workspace leases.
pub(crate) trait FileScope {
    fn reserve_derived_read<'reservation>(
        &'reservation self,
        check: &'reservation dyn Fn() -> Result<()>,
    ) -> Result<lycheedev_core::workspace::data_disk::DerivedReadLease<'reservation>>;
    fn reserve_derived_read_owned<'reservation>(
        &'reservation self,
        check: &'reservation lycheedev_core::workspace::data_disk::DerivedReadCheck,
    ) -> Result<lycheedev_core::workspace::data_disk::OwnedDerivedReadLease<'reservation>>;
    fn reserve_derived_cache<'reservation>(
        &'reservation self,
        peak: lycheedev_core::workspace::data_disk::AdditionalPeak,
        check: &'reservation dyn Fn() -> Result<()>,
        budget: &'reservation crate::resource::Budget,
    ) -> Result<crate::generation::retirement::Admission<'reservation>>;
    fn check(&self) -> Result<()>;
    /// Reauthenticate the original source observations before publication.
    /// CDN has no local mutable source; its existing session check suffices.
    fn verify_source(&self) -> Result<()> {
        self.check()
    }
    fn check_consumer(&self, invocation: &Invocation, budget: &Arc<Budget>) -> Result<()>;
    fn check_pin(&self, pin: &DataPin) -> Result<()>;
    fn resolve_pin(&self) -> Result<lycheedev_core::selection::PinnedSet>;
    fn read_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()>;
    fn read_table_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()>;
}
impl FileScope for Scope<'_> {
    fn reserve_derived_read<'reservation>(
        &'reservation self,
        check: &'reservation dyn Fn() -> Result<()>,
    ) -> Result<lycheedev_core::workspace::data_disk::DerivedReadLease<'reservation>> {
        self.check()?;
        self.admitted.reserve_derived_read(check)
    }
    fn reserve_derived_read_owned<'reservation>(
        &'reservation self,
        check: &'reservation lycheedev_core::workspace::data_disk::DerivedReadCheck,
    ) -> Result<lycheedev_core::workspace::data_disk::OwnedDerivedReadLease<'reservation>> {
        self.check()?;
        self.admitted.reserve_derived_read_owned(check)
    }
    fn reserve_derived_cache<'reservation>(
        &'reservation self,
        peak: lycheedev_core::workspace::data_disk::AdditionalPeak,
        check: &'reservation dyn Fn() -> Result<()>,
        budget: &'reservation crate::resource::Budget,
    ) -> Result<crate::generation::retirement::Admission<'reservation>> {
        self.check()?;
        self.admitted.reserve_derived_cache(peak, check, budget)
    }
    fn check(&self) -> Result<()> {
        Scope::check(self)
    }
    fn check_consumer(&self, i: &Invocation, b: &Arc<Budget>) -> Result<()> {
        Scope::check_consumer(self, i, b)
    }
    fn check_pin(&self, p: &DataPin) -> Result<()> {
        Scope::check_pin(self, p)
    }
    fn resolve_pin(&self) -> Result<lycheedev_core::selection::PinnedSet> {
        Scope::resolve_pin(self)
    }
    fn read_batches(
        &mut self,
        p: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        Scope::read_batches(self, p, ids, callback)
    }
    fn read_table_batches(
        &mut self,
        p: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        Scope::read_table_batches(self, p, ids, callback)
    }
}
pub(crate) struct SessionScope<'a, 'scope> {
    session: Option<Session<'a, 'scope>>,
    admitted: Option<&'a mut AdmittedWorkspace<'scope>>,
    invocation: &'scope Invocation,
    budget: &'scope Arc<Budget>,
    pin: &'a DataPin,
}
impl<'a, 'scope> SessionScope<'a, 'scope> {
    fn prepare_reader(&mut self) -> Result<&mut Session<'a, 'scope>> {
        self.check()?;
        if self.session.is_none() {
            let options = file_options(self.invocation)?;
            let admitted = self.admitted.take().expect("unopened reader admission");
            self.session = Some(Session::borrowed(admitted, self.pin, options, self.budget)?);
        }
        Ok(self.session.as_mut().expect("prepared reader"))
    }
    fn close(self) -> Result<()> {
        match self.session {
            Some(session) => session.close(),
            None => match self.admitted {
                Some(admitted) => admitted.check(),
                None => {
                    self.invocation.control.check()?;
                    self.budget.check()
                }
            },
        }
    }
}
impl FileScope for SessionScope<'_, '_> {
    fn reserve_derived_read<'reservation>(
        &'reservation self,
        check: &'reservation dyn Fn() -> Result<()>,
    ) -> Result<lycheedev_core::workspace::data_disk::DerivedReadLease<'reservation>> {
        self.check()?;
        match &self.session {
            Some(session) => session.reserve_derived_read(check),
            None => self
                .admitted
                .as_ref()
                .ok_or_else(|| {
                    crate::fault(
                        "records.file_query",
                        4,
                        "Reader initialization previously failed",
                    )
                })?
                .reserve_derived_read(check),
        }
    }
    fn reserve_derived_read_owned<'reservation>(
        &'reservation self,
        check: &'reservation lycheedev_core::workspace::data_disk::DerivedReadCheck,
    ) -> Result<lycheedev_core::workspace::data_disk::OwnedDerivedReadLease<'reservation>> {
        self.check()?;
        match &self.session {
            Some(session) => session.reserve_derived_read_owned(check),
            None => self
                .admitted
                .as_ref()
                .ok_or_else(|| {
                    crate::fault(
                        "records.file_query",
                        4,
                        "Reader initialization previously failed",
                    )
                })?
                .reserve_derived_read_owned(check),
        }
    }
    fn reserve_derived_cache<'reservation>(
        &'reservation self,
        peak: lycheedev_core::workspace::data_disk::AdditionalPeak,
        check: &'reservation dyn Fn() -> Result<()>,
        budget: &'reservation crate::resource::Budget,
    ) -> Result<crate::generation::retirement::Admission<'reservation>> {
        self.check()?;
        match &self.session {
            Some(session) => session.reserve_derived_cache(peak, check, budget),
            None => self
                .admitted
                .as_ref()
                .ok_or_else(|| {
                    crate::fault(
                        "records.file_query",
                        4,
                        "Reader initialization previously failed",
                    )
                })?
                .reserve_derived_cache(peak, check, budget),
        }
    }
    fn check(&self) -> Result<()> {
        match &self.session {
            Some(session) => session.check(),
            None => match self.admitted.as_ref() {
                Some(admitted) => admitted.check(),
                None => Err(crate::fault(
                    "records.file_query",
                    4,
                    "Reader initialization previously failed",
                )),
            },
        }
    }
    fn check_consumer(&self, invocation: &Invocation, budget: &Arc<Budget>) -> Result<()> {
        self.check()?;
        if !std::ptr::eq(self.invocation, invocation) || !Arc::ptr_eq(self.budget, budget) {
            return Err(crate::fault(
                "records.file_query",
                4,
                "Consumer differs from admitted invocation/budget",
            ));
        }
        Ok(())
    }
    fn check_pin(&self, pin: &DataPin) -> Result<()> {
        self.check()?;
        if pin != self.pin {
            return Err(crate::fault(
                "records.file_query",
                4,
                "Prepared session pin differs",
            ));
        }
        Ok(())
    }
    fn resolve_pin(&self) -> Result<lycheedev_core::selection::PinnedSet> {
        self.check()?;
        resolve_pin(self.invocation, self.budget)
    }
    fn read_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.check_pin(pin)?;
        let budget = self.budget;
        self.prepare_reader()?.read_batches(ids, &mut |files| {
            budget
                .safety_only()
                .charge(Cost::metadata(files.len() * size_of::<File>() + 256))?;
            callback(
                files
                    .into_iter()
                    .map(|file| {
                        let (body, reading) = file.into_parts();
                        File {
                            bytes: FileBytes::remote(body),
                            reading,
                        }
                    })
                    .collect(),
            )
        })
    }
    fn read_table_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        self.check_pin(pin)?;
        let budget = self.budget;
        self.prepare_reader()?
            .read_table_batches(ids, &mut |files| {
                budget
                    .safety_only()
                    .charge(Cost::metadata(files.len() * size_of::<File>() + 256))?;
                callback(
                    files
                        .into_iter()
                        .map(|file| {
                            let (body, reading) = file.into_parts();
                            File {
                                bytes: FileBytes::remote(body),
                                reading,
                            }
                        })
                        .collect(),
                )
            })
    }
}

fn file_options(invocation: &Invocation) -> Result<Options> {
    invocation.control.check()?;
    let variant = invocation.flag("--content-variant").unwrap_or("");
    if !["", "standard", "low-violence"].contains(&variant) {
        return Err(crate::fault("records.file_query", 4, "records.file_query"));
    }
    // Fixed Go validates this physical-source conflict only when a file is
    // read. Definition-only queries do not prepare CDN or require keys.
    if invocation
        .flag("--installation")
        .is_some_and(|path| !path.is_empty())
    {
        return Err(super::limit());
    }
    Ok(Options {
        offline: invocation.flag_bool("--offline"),
        key_file: invocation.flag("--key-file").map(str::to_owned),
        variant: variant.to_owned(),
        metadata_bytes: 512 << 20,
        content_bytes: invocation.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?,
    })
}
