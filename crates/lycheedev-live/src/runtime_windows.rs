//! Read-only Windows adapter for runtime record discovery. No transport, input,
//! executable content, module plan or Lua layout is involved.
//! Only the explicit private-read deadline error is recoverable by the scanner;
//! process, window, OS and budget faults retain their fatal classification.
use crate::runtime_discovery::{Region, Scope, Source, SourceError, Span};
use lycheedev_windows::{
    Process, ProcessIdentity, WindowIdentity,
    process::{AddressSpace, PrivateReadBudget, PrivateReadLimits, PrivateReadUsage},
};
use std::time::Instant;

pub type Result<T> = std::result::Result<T, String>;
type SourceResult<T> = std::result::Result<T, SourceError>;

fn source_error(cause: String) -> SourceError {
    if cause == "memory.private_deadline" {
        SourceError::Deadline
    } else {
        SourceError::Fatal(cause)
    }
}

/// The caller supplies an independently selected installation/client/channel
/// scope. The OS adapter seals and compares it; it cannot authenticate those
/// protocol facts from process bytes. The record verifier still owns that proof.
/// In particular, equal build numbers never merge client/channel identities.
pub struct WindowsSource {
    process: Process,
    window: WindowIdentity,
    scope: Scope,
    space: AddressSpace,
    budget: PrivateReadBudget,
    deadline: Instant,
}
impl WindowsSource {
    pub fn open(scope: Scope, limits: PrivateReadLimits, deadline: Instant) -> Result<Self> {
        if scope.pid == 0
            || scope.created == 0
            || scope.window == 0
            || scope.protocol == 0
            || scope.installation == [0; 32]
            || scope.client_channel == [0; 32]
            || scope.runtime == Some([0; 16])
        {
            return Err("memory.runtime_scope_required".into());
        }
        let mut budget = PrivateReadBudget::new(limits)?;
        let process = Process::open_until(scope.pid, &mut budget, deadline)?;
        let identity = ProcessIdentity {
            pid: scope.pid,
            created: scope.created,
        };
        if process.identity() != &identity {
            return Err("memory.runtime_process_reused".into());
        }
        let window = WindowIdentity {
            process: identity,
            hwnd: usize::try_from(scope.window).map_err(|_| "memory.runtime_window")?,
        };
        process.verify_window_until(&window, &mut budget, deadline)?;
        let space = AddressSpace::native(&mut budget, deadline)?;
        Ok(Self {
            process,
            window,
            scope,
            space,
            budget,
            deadline,
        })
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn usage(&self) -> PrivateReadUsage {
        self.budget.usage()
    }
    pub fn address_space(&self) -> AddressSpace {
        self.space
    }
    fn effective_deadline(&self, received: Instant) -> Instant {
        self.deadline.min(received)
    }
    fn verify(&mut self, deadline: Instant) -> SourceResult<()> {
        self.process
            .verify_window_until(&self.window, &mut self.budget, deadline)
            .map_err(source_error)
    }
}
impl Source for WindowsSource {
    fn verify_identity(&mut self, scope: &Scope, deadline: Instant) -> SourceResult<()> {
        if scope != &self.scope {
            return Err(SourceError::Fatal("memory.runtime_scope_changed".into()));
        }
        let deadline = self.effective_deadline(deadline);
        self.verify(deadline)
    }
    fn query_region(&mut self, at: u64, deadline: Instant) -> SourceResult<Option<Region>> {
        let deadline = self.effective_deadline(deadline);
        self.verify(deadline)?;
        let region = self
            .process
            .query_region_until(at, &self.space, &mut self.budget, deadline)
            .map_err(source_error)?;
        self.verify(deadline)?;
        Ok(region.map(|region| Region {
            span: Span {
                start: region.start,
                end: region.end,
            },
            private: region.private(),
            committed: region.committed(),
            readable: region.readable(),
        }))
    }
    fn read_private(&mut self, at: u64, into: &mut [u8], deadline: Instant) -> SourceResult<usize> {
        let deadline = self.effective_deadline(deadline);
        self.budget
            .preflight_private_read(into.len(), deadline)
            .map_err(source_error)?;
        at.checked_add(into.len() as u64)
            .ok_or_else(|| SourceError::Fatal("memory.private_address_overflow".into()))?;
        self.verify(deadline)?;
        // Preserve the caller's bytes on *every* error, including the final
        // window check. Each layer's staging buffer is capped at 16 MiB; the
        // two read layers together retain at most 32 MiB of temporary bytes.
        let mut staged = vec![0; into.len()];
        let count = self
            .process
            .read_private_prefix_until(at, &mut staged, &self.space, &mut self.budget, deadline)
            .map_err(source_error)?;
        self.verify(deadline)?;
        into[..count].copy_from_slice(&staged[..count]);
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_private_deadline_is_classified_as_deadline() {
        assert_eq!(
            source_error("memory.private_deadline".into()),
            SourceError::Deadline
        );
        for cause in [
            "window.closed",
            "memory.runtime_process_reused",
            "memory.private_os_call_budget",
            "memory.private_read:fixture_os_failure",
            "memory.private_deadline:os_failure",
        ] {
            assert_eq!(source_error(cause.into()), SourceError::Fatal(cause.into()));
        }
    }
}
