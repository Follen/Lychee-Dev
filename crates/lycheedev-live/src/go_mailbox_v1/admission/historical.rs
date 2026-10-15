//! Only the fixed-Go historical OP/BTP disk-claim lifecycle. This capability
//! contains no runtime discovery, input, queue execution, or replay methods.
use super::*;

pub(crate) struct HistoricalLease {
    scope: PathBuf,
    owner: WindowOwner,
    execution: Option<Lease>,
}
impl HistoricalLease {
    pub(crate) fn check(&self, control: &OperationControl) -> AdmissionResult<&WindowOwner> {
        if self.execution.is_none() {
            return Err(error("journal.window_run_closed").into());
        }
        check_control(control)?;
        ordinary_path(&self.scope, true, false)?;
        let owner = read_marker(
            &marker(&self.scope, &self.owner.resource),
            Some(&self.owner.resource),
            control,
        )?
        .ok_or_else(|| io_error(std::io::ErrorKind::NotFound.into()))?;
        if owner != self.owner {
            return Err(occupied(owner, &self.owner.workspace_id));
        }
        Ok(&self.owner)
    }
    pub(crate) fn close(&mut self) -> ChannelResult<()> {
        self.execution.take().map_or(Ok(()), Lease::close)
    }
}
/// Missing OP claims may be reconstructed only by the caller proving the exact
/// durable, prepared+pending intent and its original canonical admission parent.
pub(crate) fn acquire_operation(
    parent: &Path,
    want: WindowOwner,
    control: &OperationControl,
    verify_missing: impl FnOnce(&Path) -> ChannelResult<()>,
) -> AdmissionResult<HistoricalLease> {
    if !want.operation_id.starts_with("OP-") {
        return Err(error("journal.invalid_transition").into());
    }
    let scope = AdmissionScope::acquire(parent, control)?;
    let result = (|| {
        let owner = match read_marker(
            &marker(&scope.path, &want.resource),
            Some(&want.resource),
            control,
        )? {
            Some(owner) => {
                if owner.workspace_id != want.workspace_id
                    || owner.operation_id != want.operation_id
                {
                    return Err(occupied(owner, &want.workspace_id));
                }
                if !owner.intent_sha256.is_empty() && owner.intent_sha256 != want.intent_sha256 {
                    return Err(error("journal.window_intent_changed").into());
                }
                owner
            }
            None => {
                verify_missing(scope.path.parent().expect("canonical owner scope parent"))?;
                owner_limit(&scope.path, control)?;
                publish_owner(&scope.path, &want, control)?;
                want
            }
        };
        let execution = execution(&scope.path, &owner.resource, control)?;
        let mut lease = HistoricalLease {
            scope: scope.path.clone(),
            owner,
            execution: Some(execution),
        };
        if let Err(error) = lease.check(control) {
            return finish(Err(error), lease.close());
        }
        Ok(lease)
    })();
    // A failed admission close must also close any acquired execution handle.
    match (result, scope.gate.close()) {
        (Ok(lease), Ok(())) => Ok(lease),
        (Ok(mut lease), Err(error)) => finish(Err(error.into()), lease.close()),
        (Err(error), cleanup) => finish(Err(error), cleanup),
    }
}
pub(crate) fn lock_bootstrap(
    parent: &Path,
    want: &WindowOwner,
    control: &OperationControl,
) -> AdmissionResult<HistoricalLease> {
    if !want.operation_id.starts_with("BTP-") {
        return Err(error("journal.invalid_transition").into());
    }
    let scope = AdmissionScope::acquire(parent, control)?;
    let result = (|| {
        let owner = read_marker(
            &marker(&scope.path, &want.resource),
            Some(&want.resource),
            control,
        )?;
        if owner.as_ref() != Some(want) {
            return Err(error("journal.resource_busy").into());
        }
        let execution = execution(&scope.path, &want.resource, control)?;
        Ok(HistoricalLease {
            scope: scope.path.clone(),
            owner: want.clone(),
            execution: Some(execution),
        })
    })();
    match (result, scope.gate.close()) {
        (Ok(lease), Ok(())) => Ok(lease),
        (Ok(mut lease), Err(error)) => finish(Err(error.into()), lease.close()),
        (Err(error), cleanup) => finish(Err(error), cleanup),
    }
}
/// Called only after the caller re-read a durable terminal record. Fixed Go OP
/// retirement matches workspace/id; BTP retirement matches the entire claim.
pub(crate) fn retire(
    parent: &Path,
    want: &WindowOwner,
    bootstrap: bool,
    control: &OperationControl,
) -> AdmissionResult<()> {
    if !(if bootstrap {
        want.operation_id.starts_with("BTP-")
    } else {
        want.operation_id.starts_with("OP-")
    }) {
        return Err(error("journal.invalid_transition").into());
    }
    let scope = AdmissionScope::acquire(parent, control)?;
    let result = (|| {
        let Some(owner) = read_marker(
            &marker(&scope.path, &want.resource),
            Some(&want.resource),
            control,
        )?
        else {
            return Ok(());
        };
        if if bootstrap {
            owner != *want
        } else {
            owner.workspace_id != want.workspace_id || owner.operation_id != want.operation_id
        } {
            return Err(occupied(owner, &want.workspace_id));
        }
        let execution = execution(&scope.path, &want.resource, control)?;
        let remove = (|| {
            check_control(control)?;
            fs::remove_file(marker(&scope.path, &want.resource)).map_err(io_error)?;
            Ok(())
        })();
        finish(remove, execution.close())
    })();
    finish(result, scope.gate.close())
}

fn execution(scope: &Path, resource: &str, control: &OperationControl) -> ChannelResult<Lease> {
    Lease::acquire(scope, &format!("execution:{resource}"), false, control).map_err(|error| {
        if matches!(&error, ChannelError::Contract(code) if code.0 == "journal.resource_busy") {
            lycheedev_core::ToolkitError::new(
                "journal.resource_busy",
                "journal.resource_busy: active window driver",
                3,
            )
            .into()
        } else {
            error
        }
    })
}
