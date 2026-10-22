//! Formal selected-record adapter over the named Lua reader and mandatory
//! controlled EXE/hash/layout issuance. Read-only observations grant no input
//! authority; no arbitrary RootRule, RVA or layout override is exposed.
use super::observations::{ChannelError, ChannelResult, RecordReader, check_control};
use super::{ContractError, Selector};
use crate::{
    mailbox::lookup::Record,
    runtime_binding::{RuntimeBindingFailure, RuntimeBoundMemory, runtime_budget},
};
use lycheedev_core::control::OperationControl;
use lycheedev_windows::{
    WindowIdentity,
    process::{RuntimeBudget, RuntimeControl, RuntimeCost, RuntimeLimits},
};

struct LookupControl(OperationControl);
/// One bounded formal-open/read budget, including the mandatory complete EXE
/// hash (<=512MiB), two-pass <=128MiB text scan and subsequent guarded lookups.
/// Work is conservatively debited at full pattern length for each leading 0x48;
/// 32Gi units admits that worst-case charge while the original deadline and all
/// other OS/query/allocation limits continue to apply unchanged.
fn native_limits() -> RuntimeLimits {
    RuntimeLimits {
        requested_bytes: 1 << 30,
        read_calls: 32_768,
        work_units: 1 << 35,
        ..RuntimeLimits::defaults()
    }
}
impl RuntimeControl for LookupControl {
    fn check(&self) -> std::result::Result<(), String> {
        self.0.check().map_err(|e| match e.code.as_str() {
            "command.cancelled" => "memory.runtime_cancelled".into(),
            "command.deadline" => "memory.runtime_deadline".into(),
            _ => format!("{}: {}", e.code, e.message),
        })
    }
}

trait SelectedReader {
    fn lookup(
        &mut self,
        selector: &Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure>;
}
impl SelectedReader for RuntimeBoundMemory {
    fn lookup(
        &mut self,
        selector: &Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure> {
        self.lookup_formal(selector, accept, budget)
    }
}
struct ReadState<R> {
    reader: R,
    budget: RuntimeBudget,
    last_failure: Option<RuntimeBindingFailure>,
}
impl<R: SelectedReader> ReadState<R> {
    fn find(
        &mut self,
        control: &OperationControl,
        selector: &Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
    ) -> ChannelResult<Vec<Record>> {
        self.last_failure = None;
        check_control(control)?;
        let reader = &mut self.reader;
        let read = self.budget.with_control_scope_factory(
            control.clone_allocation_bytes(),
            || LookupControl(control.clone()),
            |budget| reader.lookup(selector, accept, budget),
        );
        let result = match read {
            Ok(Ok(record)) => {
                self.budget
                    .charge(RuntimeCost::allocation(std::mem::size_of::<Record>()))
                    .map_err(|cause| classify_reader_error(&cause))?;
                Ok(record.into_iter().collect())
            }
            Ok(Err(failure)) => {
                let error = classify_reader_error(&failure.cause);
                self.last_failure = Some(failure);
                Err(error)
            }
            Err(cause) => Err(classify_reader_error(&cause)),
        };
        // Do not replace an independent read/identity error with cancellation.
        match (result, check_control(control)) {
            (Err(first), Err(second)) => Err(ChannelError::Joined {
                causes: vec![first, second],
            }),
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(records), Ok(())) => Ok(records),
        }
    }
}

pub struct NativeReader {
    state: ReadState<RuntimeBoundMemory>,
}
impl NativeReader {
    pub fn open(
        window: WindowIdentity,
        release: &str,
        control: &OperationControl,
    ) -> std::result::Result<Self, RuntimeBindingFailure> {
        let mut budget =
            runtime_budget(control, native_limits()).map_err(|cause| RuntimeBindingFailure {
                cause,
                stage: "budget",
                root: Box::default(),
                usage: RuntimeCost::default(),
                first_read_failure: None,
            })?;
        let reader = RuntimeBoundMemory::open_formal(window, release, &mut budget)?;
        Ok(Self {
            state: ReadState {
                reader,
                budget,
                last_failure: None,
            },
        })
    }
    /// Consume and explicitly close the held process once. Reports cleanup and
    /// original control errors, not previously handled transient observations.
    pub fn close(self) -> ChannelResult<()> {
        let ReadState { reader, budget, .. } = self.state;
        reader
            .close(&budget)
            .map_err(|cause| classify_reader_error(&cause))
    }
    /// Preserve the caller's current operation/read error together with every
    /// cleanup error. A failed operation still always attempts checked close.
    pub fn finish<T>(self, result: ChannelResult<T>) -> ChannelResult<T> {
        finish_with_close(result, || self.close())
    }
    /// The original read's facts; inspecting these performs no process read.
    pub fn last_failure(&self) -> Option<&RuntimeBindingFailure> {
        self.state.last_failure.as_ref()
    }
    pub fn usage(&self) -> RuntimeCost {
        self.state.budget.usage()
    }
    pub fn root_evidence(&self) -> &crate::runtime_root::RootEvidence {
        self.state.reader.root_evidence()
    }
    pub fn latest_root_evidence(&self) -> &crate::runtime_root::RootEvidence {
        self.state.reader.latest_root_evidence()
    }
}
impl RecordReader for NativeReader {
    fn find(
        &mut self,
        control: &OperationControl,
        selector: &Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        _eager: bool,
    ) -> ChannelResult<Vec<Record>> {
        // Like Go Native.Find, eager does not add a scan or alter the named path.
        self.state.find(control, selector, accept)
    }
}

fn finish_with_close<T>(
    result: ChannelResult<T>,
    close: impl FnOnce() -> ChannelResult<()>,
) -> ChannelResult<T> {
    match (result, close()) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(first), Err(second)) => Err(ChannelError::Joined {
            causes: vec![first, second],
        }),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

/// Preserve joined causes. Only the exact three public-path conditions acquire
/// the typed classifications consumed by continuation; ordinary protocol, root,
/// layout and identity failures must not become retryable pending observations.
fn classify_reader_error(cause: &str) -> ChannelError {
    let causes: Vec<_> = cause
        .lines()
        .map(|part| match part {
            "memory.runtime_cancelled" | "command.cancelled" => ChannelError::Cancelled,
            "memory.runtime_deadline" | "command.deadline" => ChannelError::DeadlineExceeded,
            "mailbox.path_changed" => {
                ContractError("memory.mailbox_unavailable: path_changed").into()
            }
            "mailbox.lua_state_null" => {
                ContractError("memory.mailbox_unavailable: lua_state_null").into()
            }
            "mailbox.string_layout" => {
                ContractError("memory.mailbox_unavailable: string_layout").into()
            }
            _ => ChannelError::External {
                causes: vec![part.into()],
            },
        })
        .collect();
    if causes.len() == 1 {
        causes.into_iter().next().unwrap()
    } else if causes.is_empty() {
        ChannelError::External {
            causes: vec!["live.channel_reader_error_missing".into()],
        }
    } else {
        ChannelError::Joined { causes }
    }
}

#[cfg(test)]
#[path = "native_reader_tests.rs"]
mod tests;
