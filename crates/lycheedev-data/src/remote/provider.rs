//! Actual default CDN query owner. Numeric download width alone is never admission.
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
    invocation.control.check()?;
    budget.check()?;
    if let Some(id) = invocation.flag("--snapshot") {
        lycheedev_core::selection::load_pinned_controlled_with_check(
            &invocation.home,
            id,
            &invocation.control,
            || budget.check(),
        )
    } else {
        // Legacy automatic project discovery remains an explicit gap.
        let pin = lycheedev_core::selection::resolve_pinned(invocation)?;
        invocation.control.check()?;
        budget.check()?;
        Ok(pin)
    }
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
    fn file_options(&self) -> Result<Options> {
        self.check()?;
        let variant = self.invocation.flag("--content-variant").unwrap_or("");
        if !["", "standard", "low-violence"].contains(&variant) {
            return Err(crate::fault("records.file_query", 4, "records.file_query"));
        }
        // Fixed Go validates this physical-source conflict only when a file is
        // read. Definition-only queries do not prepare CDN or require keys.
        if self
            .invocation
            .flag("--installation")
            .is_some_and(|path| !path.is_empty())
        {
            return Err(super::limit());
        }
        Ok(Options {
            offline: self.invocation.flag_bool("--offline"),
            key_file: self.invocation.flag("--key-file").map(str::to_owned),
            variant: variant.to_owned(),
            metadata_bytes: 512 << 20,
            content_bytes: self
                .invocation
                .usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?,
        })
    }
    pub fn read_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
        let options = self.file_options()?;
        let budget = self.budget;
        let mut session = Session::borrowed(&mut self.admitted, pin, options, budget)?;
        let result = session.read_batches(ids, &mut |files| {
            // The public wrapper Vec is distinct from the Session Vec. Admit its
            // bounded descriptors without copying authenticated body/provenance.
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
        });
        let cleanup = session.close();
        Store {
            home: self.admitted.home(),
            control: self.admitted.control(),
            budget: Some(self.budget),
        }
        .finish(result, cleanup)
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
        let options = self.file_options()?;
        let mut session = Session::borrowed(&mut self.admitted, pin, options, self.budget)?;
        let result = session.with_verified(id, &mut |file| callback(&mut file.blob, &file.reading));
        let cleanup = session.close();
        Store {
            home: self.admitted.home(),
            control: self.admitted.control(),
            budget: Some(self.budget),
        }
        .finish(result, cleanup)
    }
    pub fn finish<T>(self, result: Result<T>) -> Result<T> {
        self.admitted.finish(result)
    }
}
