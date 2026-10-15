//! Actual default CDN query owner. Numeric download width alone is never admission.
use super::{
    admission::AdmittedWorkspace,
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

pub(crate) struct Scope<'a> {
    admitted: AdmittedWorkspace<'a>,
    invocation: &'a Invocation,
    budget: &'a Arc<Budget>,
}
impl<'a> Scope<'a> {
    pub fn acquire(
        invocation: &'a Invocation,
        pin: &DataPin,
        budget: &'a Arc<Budget>,
    ) -> Result<Self> {
        invocation.control.check()?;
        budget.check()?;
        fragments::data_identity(pin)?;
        let admitted = AdmittedWorkspace::acquire(
            &invocation.home,
            Policy::Ordinary,
            &invocation.control,
            Some(budget),
        )?;
        Ok(Self {
            admitted,
            invocation,
            budget,
        })
    }
    pub fn check(&self) -> Result<()> {
        self.admitted.check()
    }
    pub fn read_batches(
        &mut self,
        pin: &DataPin,
        ids: &[u32],
        callback: &mut dyn FnMut(Vec<File>) -> Result<Vec<u32>>,
    ) -> Result<()> {
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
        let options = Options {
            offline: self.invocation.flag_bool("--offline"),
            key_file: self.invocation.flag("--key-file").map(str::to_owned),
            variant: variant.to_owned(),
            metadata_bytes: 512 << 20,
            content_bytes: self
                .invocation
                .usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?,
        };
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
    pub fn finish<T>(self, result: Result<T>) -> Result<T> {
        self.admitted.finish(result)
    }
}
