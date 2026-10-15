//! Included as connection::native::doctor_native. A diagnostic has a plain
//! NativeReader and no publication store, native sender, trace path or claim.
#![forbid(unsafe_code)]
use super::*;
use crate::go_mailbox_v1::{
    Selector,
    connection::doctor_service::DoctorEnvironment,
    doctor::{DoctorDeployment, DoctorRuntime, SlotAssessment},
    native_reader::NativeReader,
    observations::RecordReader,
};
use crate::mailbox::Record;

pub struct NativeDiagnostic {
    reader: Option<NativeReader>,
    target: ConnectionTarget,
}
impl RecordReader for NativeDiagnostic {
    fn find(
        &mut self,
        control: &OperationControl,
        selector: &Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        eager: bool,
    ) -> ChannelResult<Vec<Record>> {
        confirm(&self.target, control)?;
        let read = self
            .reader
            .as_mut()
            .ok_or_else(|| error("live.channel_reader_closed"))?
            .find(control, selector, accept, eager);
        let checked = confirm(&self.target, control).map(|_| ());
        match (read, checked) {
            (Ok(v), Ok(())) => Ok(v),
            (Err(a), Err(b)) => Err(ChannelError::Joined { causes: vec![a, b] }),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    }
}
impl DoctorRuntime for NativeDiagnostic {
    fn uptime_millis(&self) -> i64 {
        lycheedev_windows::mailbox_input::uptime_millis().min(i64::MAX as u64) as i64
    }
    fn checked_close(&mut self) -> ChannelResult<()> {
        self.reader.take().map_or(Ok(()), NativeReader::close)
    }
}
impl<L: ProcessLifetimeObserver> DoctorEnvironment for NativeEnvironment<L> {
    type Diagnostic = NativeDiagnostic;
    fn release(&self) -> &str {
        &self.release
    }
    fn confirm_target(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<()> {
        confirm(target, control).map(|_| ())
    }
    fn inspect_deployment(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<DoctorDeployment> {
        check_control(control)?;
        let status = installation::inspect_main_controlled(
            &recorded_addon_directory(target),
            "addon",
            control,
        )
        .map_err(durable_journal::storage_error)?;
        let field = |name| {
            status
                .get(name)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        let receipt = &status["receipt"];
        let version = receipt["version"].as_str().unwrap_or("").to_owned();
        let uses_slots = receipt["resources"].as_array().is_some_and(|resources| {
            resources
                .iter()
                .any(|r| r["path"] == "addon/Bridge/SlotRuntime.lua")
        });
        let slots = if !uses_slots {
            None
        } else {
            match lycheedev_delivery::slots::inspect_controlled(
                &target.addon_parent(),
                &version,
                control,
            ) {
                Ok(pool) => {
                    let files = pool["files"].as_array();
                    let count = files.map_or(0, Vec::len);
                    let pending = files.map_or(0, |files| {
                        files
                            .iter()
                            .filter(|f| f["pendingHash"].as_str().is_some_and(|s| !s.is_empty()))
                            .count()
                    });
                    Some(SlotAssessment {
                        state: if pending == 0 { "managed" } else { "pending" }.into(),
                        count: count as i32,
                        pending: pending as i32,
                        detail: String::new(),
                    })
                }
                Err(e) if matches!(e.code.as_str(), "command.cancelled" | "command.deadline") => {
                    return Err(e.into());
                }
                Err(e) => Some(SlotAssessment {
                    state: "incomplete".into(),
                    detail: e.to_string(),
                    ..Default::default()
                }),
            }
        };
        check_control(control)?;
        Ok(DoctorDeployment {
            state: field("state"),
            detail: field("detail"),
            version,
            commit: receipt["commit"].as_str().unwrap_or("").to_owned(),
            slots,
        })
    }
    fn open_diagnostic(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<Self::Diagnostic> {
        let window = confirm(target, control)?;
        let reader = NativeReader::open(window.identity().clone(), &self.release, control)
            .map_err(|e| {
                let mut causes = vec![external(e.cause)];
                if let Err(stopped) = check_control(control) {
                    causes.push(stopped);
                }
                ChannelError::Joined { causes }
            })?;
        if let Err(e) = confirm(target, control) {
            return reader.finish(Err(e));
        }
        Ok(NativeDiagnostic {
            reader: Some(reader),
            target: target.clone(),
        })
    }
}

impl<L: ProcessLifetimeObserver> crate::go_mailbox_v1::connection::doctor_repair::RepairEnvironment
    for NativeEnvironment<L>
{
    fn open_for_repair(
        &mut self,
        target: &ConnectionTarget,
        owner: &WindowOwner,
        runtime_release: &str,
        control: &OperationControl,
    ) -> ChannelResult<Self::Backend> {
        confirm(target, control)?;
        // CloseForRepair deliberately does not require clean current main
        // bytes. Only the authenticated original unbind receives the narrow
        // Delivery capability; ordinary business publication remains closed.
        let addon = lycheedev_delivery::deploy::destination(&recorded_addon_directory(target))
            .map_err(durable_journal::storage_error)?;
        check_control(control)?;
        let book = ConnectionBook::open(&self.project)?;
        let authority_target = target.clone();
        let authority_owner = owner.clone();
        let authority_release = runtime_release.to_owned();
        let window = bind(target)?;
        let target = target.clone();
        let owner = owner.clone();
        let scans = self
            .project
            .join(".lycheedev/live/scans")
            .join(lycheedev_windows::random::token().map_err(external)?);
        let inner = NativeBackend::open_for_repair(
            window,
            addon,
            runtime_release,
            Some(scans.clone()),
            move |control| {
                confirm(&target, control)?;
                let actual = inspect_window_owner(&target.addon_parent(), &owner.resource, control);
                if let Some(e) = actual.error {
                    return Err(e);
                }
                if owner.resource != target.resource() || actual.owner.as_ref() != Some(&owner) {
                    return Err(error("live.channel_ownership_changed"));
                }
                Ok(())
            },
            move |envelope, control| {
                super::super::doctor_repair::authenticated_repair_publication(
                    &book,
                    &authority_target,
                    &authority_owner,
                    &authority_release,
                    envelope,
                    control,
                )
            },
            control,
        )?;
        Ok(NativeConnectionBackend {
            inner,
            scans: scans.display().to_string(),
        })
    }
}
