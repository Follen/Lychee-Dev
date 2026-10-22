//! Included as connection::doctor_service. Both public doctor entrypoints use
//! this same read-only service. Repair is an explicit, separately typed call.
#![forbid(unsafe_code)]
use super::activation_service::{ActivationBackend, ActivationEnvironment};
use super::*;
use crate::mailbox::{
    admission::inspect_window_owner,
    doctor::{DoctorDeployment, DoctorResult, DoctorRuntime, TOP_LEVEL_LIMIT, inspect_runtime},
};
use std::time::Instant;

pub trait DoctorEnvironment: ActivationEnvironment {
    type Diagnostic: DoctorRuntime;
    fn release(&self) -> &str;
    fn confirm_target(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<()>;
    fn inspect_deployment(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<DoctorDeployment>;
    fn open_diagnostic(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<Self::Diagnostic>;
}
/// Existing CLI options, with no new flag or command. Validate before any IO.
pub fn validate_doctor_options(
    request: &TargetRequest,
    session: &str,
    fix: bool,
    release: &str,
    output: &str,
    key: &str,
) -> ChannelResult<()> {
    let message = if !session.is_empty() && (!request.installation.is_empty() || request.pid != 0) {
        Some("use --session with its original --project, or select --installation/--pid")
    } else if !fix && (!release.is_empty() || !output.is_empty() || !key.is_empty()) {
        Some("--release, --output and --request require --fix")
    } else {
        None
    };
    match message {
        Some(m) => Err(lycheedev_core::ToolkitError::arguments(m).into()),
        None => Ok(()),
    }
}
impl<E: DoctorEnvironment, C: Clock, T: TokenSource, S: ContinuationScheduler>
    ConnectionService<E, C, T, S>
where
    E::Backend: ActivationBackend,
{
    /// Read-only; no execution/admission lease, claim, trace, slot or key write.
    pub fn doctor(
        &mut self,
        request: &TargetRequest,
        session: &str,
        control: &OperationControl,
    ) -> DoctorResult {
        let mut report = DoctorResult::default();
        let target = if !session.is_empty() {
            match self.book.metadata(session, control) {
                Ok(metadata) => {
                    if let ServiceOutcome::Completed(result) =
                        self.status_with_activation(session, control)
                    {
                        report.connection = Some(result);
                    }
                    match self.environment.confirm_target(&metadata.target, control) {
                        Ok(()) => metadata.target,
                        Err(e) => {
                            report.failure(
                                "live.target",
                                "blocked",
                                e,
                                "select one running WoW PID with live instances",
                            );
                            return report;
                        }
                    }
                }
                Err(e) => {
                    report.failure(
                        "live.target",
                        "error",
                        e,
                        "use the original project and CON id",
                    );
                    return report;
                }
            }
        } else {
            match self.environment.resolve(request, control) {
                Ok(target) => target,
                Err(e) => {
                    report.failure(
                        "live.target",
                        "blocked",
                        e,
                        "select one running WoW PID with live instances",
                    );
                    return report;
                }
            }
        };
        report.target = Some(target.clone());
        report.add(
            "live.target",
            "ok",
            format!(
                "PID {}, {}",
                target.window.process_id, target.client.full_build
            ),
            "",
        );
        let release = self.environment.release().to_owned();
        match self.environment.inspect_deployment(&target, control) {
            Ok(deployment) => report.inspect_deployment(deployment, &release),
            Err(e) => report.failure(
                "live.installation",
                "error",
                e,
                "inspect addon status and the selected installation",
            ),
        }
        let inspection = inspect_window_owner(&target.addon_parent(), &target.resource(), control);
        if let Some(e) = inspection.error {
            report.failure(
                "live.ownership",
                "error",
                e,
                "inspect the retained connection journal",
            );
            return report;
        }
        if let Some(owner) = inspection.owner {
            if owner.workspace_id != self.book.workspace_id() {
                report.add(
                    "live.ownership",
                    "blocked",
                    "selected PID belongs to another project",
                    "resume or disconnect from its owning project",
                );
            } else {
                report.add("live.ownership", "ok", &owner.operation_id, "");
                match self.status_with_activation(&owner.operation_id, control) {
                    ServiceOutcome::Completed(result) => report.connection = Some(result),
                    ServiceOutcome::Failed(error)
                    | ServiceOutcome::ReportedWithFailure { error, .. } => report.add(
                        "live.connection",
                        "error",
                        error.to_string(),
                        "restore the original project's recovery files",
                    ),
                }
            }
            report.owner = Some(owner);
        } else {
            report.add("live.ownership", "ok", "unclaimed", "");
        }
        let mut runtime = match self.environment.open_diagnostic(&target, control) {
            Ok(runtime) => runtime,
            Err(e) => {
                report.failure(
                    "live.memory",
                    "error",
                    e,
                    "verify the running build and its runtime locator",
                );
                return report;
            }
        };
        inspect_runtime(
            &mut report,
            &mut runtime,
            &target,
            &release,
            &request.filter.character,
            &request.filter.realm,
            control,
            &mut self.scheduler,
        );
        report
    }
    /// Top-level `doctor` performs active reads only for an explicit selection,
    /// always inside a child of the original control capped at ten seconds.
    pub fn top_level_live_doctor(
        &mut self,
        request: &TargetRequest,
        session: &str,
        control: &OperationControl,
    ) -> Option<DoctorResult> {
        if request.installation.is_empty() && request.pid == 0 && session.is_empty() {
            return None;
        }
        let bounded = match control.ordered_child(Instant::now() + TOP_LEVEL_LIMIT) {
            Ok(c) => c,
            Err(e) => {
                let mut r = DoctorResult::default();
                r.failure(
                    "live.project",
                    "error",
                    e.into(),
                    "retry diagnosis for the selected client",
                );
                return Some(r);
            }
        };
        let mut report = self.doctor(request, session, &bounded);
        // The top-level vault representation calls blocked checks errors.
        for check in &mut report.checks {
            if check.state == "blocked" {
                check.state = "error".into();
            }
        }
        Some(report)
    }
}
