//! Included as connection::doctor_repair. Ordered repair, not an implicit
//! request replay. The caller supplies the reviewed Delivery mutation adapter.
#![forbid(unsafe_code)]
use super::activation_service::ActivationBackend;
use super::doctor_service::DoctorEnvironment;
use super::*;
use crate::mailbox::doctor::{DoctorDeployment, DoctorResult};
use std::path::{Path, PathBuf};

pub trait RepairEnvironment: DoctorEnvironment {
    /// Original runtime version and exact existing CON claim stay authoritative.
    /// A repair adapter must not downgrade validation for arbitrary business.
    fn open_for_repair(
        &mut self,
        target: &ConnectionTarget,
        owner: &WindowOwner,
        runtime_release: &str,
        control: &OperationControl,
    ) -> ChannelResult<Self::Backend>;
}
/// Adapter contract for the existing Delivery install/upgrade operation. Its
/// implementation must hold formal installation admission and the existing
/// deployment exclusion, reject a preexisting archive, retain all errors and
/// use the caller's control. It cannot resume any Live business transaction.
/// No permissive/no-op production implementation is provided.
pub trait RepairDelivery {
    fn apply(
        &mut self,
        target: &ConnectionTarget,
        deployment: &DoctorDeployment,
        release: &Path,
        archive: Option<&Path>,
        control: &OperationControl,
    ) -> ChannelResult<()>;
}
/// Reads authority from the original durable CON while its caller owns the
/// execution lease. This does not recover a torn tail or create a close intent.
/// Each publication/verification calls it again under the original control.
pub(crate) fn authenticated_repair_publication(
    book: &ConnectionBook,
    target: &ConnectionTarget,
    owner: &WindowOwner,
    original_release: &str,
    envelope: &super::super::state::SlotEnvelope,
    control: &OperationControl,
) -> ChannelResult<lycheedev_delivery::slot_publication::RepairPublication> {
    use lycheedev_delivery::slot_publication::{RecordedInstallationIdentity, RepairPublication};
    check_control(control)?;
    let record = book.status(&owner.operation_id, control)?;
    let state = &record.state;
    state.validate()?;
    let claim = super::super::admission::inspect_window_owner(
        &target.addon_parent(),
        &owner.resource,
        control,
    );
    if let Some(error) = claim.error {
        return Err(error);
    }
    if claim.owner.as_ref() != Some(owner)
        || owner.resource != target.resource()
        || record.metadata.owner != *owner
        || !record.metadata.target.same_connection_target(target)
        || state.id != owner.operation_id
        || state.identity.release != original_release
        || !state.closing
        || state.closed
        || state.close_budget.is_none()
        || !repair_action_matches_operation(state, envelope)
        || state
            .transaction
            .as_ref()
            .is_none_or(|tx| tx.envelope != *envelope)
    {
        return Err(error("live.channel_repair_publication_invalid"));
    }
    if envelope.action == "release" {
        let operation = state.operation.as_ref().expect("matched release operation");
        let path = record
            .paths
            .result_directory()
            .join(format!("{}.json", operation.id));
        let bytes = durable_journal::read_file(
            &path,
            // A journal snapshot carries metadata, while the standalone artifact
            // embeds up to 6x JSON-escaped 256 KiB code and base64 512 KiB
            // result. Three event bounds cover all three with headroom.
            3 * super::super::journal::MAX_EVENT_BYTES,
            "live.channel_repair_publication_invalid",
            control,
        )?
        .ok_or_else(|| error("live.channel_repair_publication_invalid"))?;
        let artifact: super::super::state::Operation = serde_json::from_slice(&bytes)
            .map_err(|_| error("live.channel_repair_publication_invalid"))?;
        if !repair_budget_progression(
            artifact.recovery_budget.as_ref(),
            operation.recovery_budget.as_ref(),
        ) {
            return Err(error("live.channel_repair_publication_invalid"));
        }
        let mut expected = operation.clone();
        expected.recovery_budget = artifact.recovery_budget.clone();
        expected.stage = super::super::state::OperationStage::ResultVerified;
        if artifact != expected {
            return Err(error("live.channel_repair_publication_invalid"));
        }
    }
    let addon = lycheedev_delivery::deploy::destination(&super::native::recorded_addon_directory(
        &record.metadata.target,
    ))
    .map_err(durable_journal::storage_error)?;
    check_control(control)?;
    let identity = RecordedInstallationIdentity::from_authenticated_record(addon)
        .map_err(repair_publication_error)?;
    let consumer = format!(
        "{}/{}",
        target.window.process_id, target.window.process_started_at
    );
    let constructor = if envelope.action == "unbind" {
        RepairPublication::from_authenticated_record
    } else {
        RepairPublication::from_authenticated_finalization
    };
    let capability = constructor(&identity, original_release, &consumer, envelope)
        .map_err(repair_publication_error)?;
    check_control(control)?;
    Ok(capability)
}
// State is read from the durable journal and validated before this check.
// Confirm has a durable candidate, not a durable verified result yet. Release
// requires the stage written only after write_result + result_durable succeed.
fn repair_action_matches_operation(
    state: &super::super::state::State,
    envelope: &super::super::state::SlotEnvelope,
) -> bool {
    use super::super::state::{CleanupMethod, OperationStage, ProcessEnd, RecoveryPhase};
    if envelope.action == "unbind" {
        return true;
    }
    let Some(op) = &state.operation else {
        return false;
    };
    if !state.bound
        || state.process_end != ProcessEnd::None
        || state.runtime_end.is_some()
        || state
            .recovery
            .as_ref()
            .is_some_and(|r| r.phase != RecoveryPhase::Complete)
        || envelope.fence != state.identity.fence
        || envelope.ticket != op.ticket
        || op.cleanup_method != CleanupMethod::None
    {
        return false;
    }
    match envelope.action.as_str() {
        "confirm" => op.stage == OperationStage::ConfirmReady,
        "release" => {
            op.stage == OperationStage::ReleaseReady
                && envelope.report_bytes == op.report_bytes
                && envelope.report_checksum == op.report_checksum
        }
        _ => false,
    }
}
// Recovery observations can advance after the immutable result artifact was
// written. Ignore only that projection, never reset its origin or deadline.
fn repair_budget_progression(
    before: Option<&super::super::state::DurableBudget>,
    after: Option<&super::super::state::DurableBudget>,
) -> bool {
    match (before, after) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.validate().is_ok()
                && b.validate().is_ok()
                && a.started_at_ms == b.started_at_ms
                && a.deadline_ms == b.deadline_ms
                && a.last_observed_ms <= b.last_observed_ms
                && (!a.exhausted || b.exhausted)
                && (!a.clock_rollback || b.clock_rollback)
        }
        _ => false,
    }
}
fn repair_publication_error(
    cause: lycheedev_delivery::slot_publication::PublicationError,
) -> ChannelError {
    use lycheedev_delivery::slot_publication::PublicationError;
    match cause {
        PublicationError::Failure(error) => error.into(),
        PublicationError::CloseFailures(errors) => ChannelError::Joined {
            causes: errors.into_iter().map(ChannelError::from).collect(),
        },
        PublicationError::Reserved { reservation } => {
            let mut error = lycheedev_core::ToolkitError::new(
                "delivery.installation_conflict",
                "delivery.slot_reserved",
                4,
            );
            error.details = serde_json::json!({"reservation": reservation});
            error.into()
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct DoctorFixOptions<'a> {
    pub session: &'a str,
    pub release: Option<&'a Path>,
    pub bundled_release: Option<&'a Path>,
    pub output: Option<&'a Path>,
    pub request: &'a str,
}
/// Exact Go VolumeName policy, independent of the host running fixture tests.
/// It intentionally does not infer volume identity from directory contents.
fn volume(path: &Path) -> String {
    let spelling = path.to_string_lossy().replace('/', "\\");
    let s = spelling
        .strip_prefix("\\\\?\\UNC\\")
        .map(|s| format!("\\\\{s}"))
        .unwrap_or_else(|| {
            spelling
                .strip_prefix("\\\\?\\")
                .unwrap_or(&spelling)
                .to_owned()
        });
    if let Some(unc) = s.strip_prefix("\\\\") {
        return unc
            .split('\\')
            .take(2)
            .collect::<Vec<_>>()
            .join("\\")
            .to_lowercase();
    }
    if s.as_bytes().get(1) == Some(&b':') {
        return s[..2].to_ascii_lowercase();
    }
    String::new()
}
pub fn default_repair_archive(project: &Path, installation: &Path, stamp: &str) -> PathBuf {
    if volume(project) == volume(installation) {
        project.join(".lycheedev/live/repairs").join(stamp)
    } else {
        installation
            .join("Interface/.lycheedev-repairs")
            .join(stamp)
    }
}
impl<E: RepairEnvironment, C: Clock, T: TokenSource, S: ContinuationScheduler>
    ConnectionService<E, C, T, S>
where
    E::Backend: ActivationBackend,
{
    pub fn doctor_fix(
        &mut self,
        report: &DoctorResult,
        options: &DoctorFixOptions<'_>,
        delivery: &mut impl RepairDelivery,
        control: &OperationControl,
    ) -> ServiceOutcome {
        let (Some(target), Some(deployment)) = (&report.target, &report.deployment) else {
            return ServiceOutcome::failure(ChannelError::from(lycheedev_core::ToolkitError::new(
                "doctor.check_failed",
                "doctor.check_failed",
                3,
            )));
        };
        let needs_install = deployment.needs_install(self.environment.release());
        if let Some(owner) = &report.owner {
            if owner.workspace_id != self.book.workspace_id() {
                return ServiceOutcome::failure(AdmissionError::Occupied {
                    owner: owner.clone(),
                    foreign: true,
                });
            }
            if !options.session.is_empty() && options.session != owner.operation_id {
                return ServiceOutcome::failure(ChannelError::External {
                    causes: vec![
                        "live.channel_owner_mismatch: repair must use the original CON".into(),
                    ],
                });
            }
            if !needs_install && options.release.is_none() {
                return self.resume_with_activation(&owner.operation_id, control);
            }
            // Close intent is persisted before any ordinary continuation. A
            // pending opaque/unknown operation is never executed as pre-repair.
            let closed = self.close_for_repair(&owner.operation_id, control);
            if !matches!(&closed, ServiceOutcome::Completed(_)) {
                return closed;
            }
        }
        if needs_install || options.release.is_some() {
            let Some(release) = options.release.or(options.bundled_release) else {
                return ServiceOutcome::failure(ChannelError::External { causes: vec!["live.channel_release_required: supply --release with the current distribution".into()] });
            };
            let stamp = chrono::DateTime::from_timestamp_millis(self.clock.unix_millis())
                .map(|now| now.format("%Y%m%dT%H%M%S.%9f").to_string());
            let Some(stamp) = stamp else {
                return ServiceOutcome::failure(error("live.channel_budget_invalid"));
            };
            let default_archive;
            let archive = if deployment.state == "absent" {
                None
            } else if let Some(output) = options.output {
                Some(output)
            } else {
                default_archive = default_repair_archive(
                    self.book.root(),
                    Path::new(&target.client.directory),
                    &stamp,
                );
                Some(default_archive.as_path())
            };
            // Only the generated default's parent is created. An explicit
            // output remains byte-for-byte the caller's requested destination.
            if deployment.state != "absent" && options.output.is_none() {
                let prepare = check_control(control).and_then(|_| {
                    let parent = archive
                        .and_then(Path::parent)
                        .ok_or_else(|| error("live.channel_project_invalid"))?;
                    std::fs::create_dir_all(parent).map_err(durable_journal::io_error)?;
                    check_control(control)
                });
                if let Err(e) = prepare {
                    return ServiceOutcome::failure(e);
                }
            }
            if let Err(e) = delivery.apply(target, deployment, release, archive, control) {
                return ServiceOutcome::failure(e);
            }
            let key = if options.request.is_empty() {
                format!("doctor-{stamp}")
            } else {
                options.request.to_owned()
            };
            return self.activate(
                &TargetRequest {
                    installation: target.client.directory.clone(),
                    pid: target.window.process_id,
                    ..Default::default()
                },
                &key,
                control,
            );
        }
        if !options.session.is_empty() {
            self.resume_with_activation(options.session, control)
        } else {
            self.connect_with_activation(
                &TargetRequest {
                    installation: target.client.directory.clone(),
                    pid: target.window.process_id,
                    ..Default::default()
                },
                control,
            )
        }
    }
    pub fn close_for_repair(&mut self, id: &str, control: &OperationControl) -> ServiceOutcome {
        let before = match self.read_recover(id, control) {
            Ok(v) => v,
            Err(AdmissionError::Channel(e)) if e.all_causes(&|e| matches!(e, ChannelError::Contract(c) if c.0 == "live.channel_journal_missing")) => return self.disconnect_activation(id, control),
            Err(e) => return ServiceOutcome::failure(e),
        };
        if before.state.closed {
            return self.retire(before, control);
        }
        match self
            .environment
            .process_end(&before.metadata.target, control)
        {
            Ok(ProcessEnd::None) => (),
            Ok(_) => return self.disconnect_ended(id, control),
            Err(e) => {
                return ServiceOutcome::with_result(self.project(&before, "", ""), Err(e.into()));
            }
        }
        let (record, mut journal) = match self.book.acquire(id, control) {
            Ok(v) => v,
            Err(e) => return self.acquired(&before, e),
        };
        if record.state.closed {
            let mut result = ServiceOutcome::with_result(
                self.project(&record, "", ""),
                Err(contract("live.channel_closed")),
            );
            result = result.join(channel(journal.close()));
            return result;
        }
        let mut state = record.state.clone();
        let validation = state.validate().map_err(ChannelError::from).and_then(|_| {
            super::super::recovery::synchronize_automatic_budget(&mut state).map(|_| ())
        });
        if let Err(e) = validation {
            let mut result =
                ServiceOutcome::with_result(self.project(&record, "", ""), Err(e.into()));
            result = result.join(channel(journal.close()));
            return result;
        }
        let backend = match self.environment.open_for_repair(
            &record.metadata.target,
            &record.metadata.owner,
            &state.identity.release,
            control,
        ) {
            Ok(v) => v,
            Err(e) => {
                let mut result =
                    ServiceOutcome::with_result(self.project(&record, "", ""), Err(e.into()));
                result = result.join(channel(journal.close()));
                return result;
            }
        };
        let mut driver = Driver::from_state(
            state,
            backend,
            journal,
            BorrowClock(&self.clock),
            BorrowTokens(&mut self.tokens),
        )
        .expect("validated before resource transfer");
        let target = RetirementTarget {
            process_id: record.metadata.target.window.process_id,
            process_started_at: record.metadata.target.window.process_started_at,
        };
        let result = driver
            .request_close(control)
            .and_then(|_| driver.continue_or_retire(control, target, true, &mut self.scheduler));
        let retire = result.is_ok() && driver.state.closed;
        let projected = present_owned(
            driver.state,
            record.paths.log_path(),
            self.clock.unix_millis(),
            &driver.waiting,
            driver.backend.scans(),
        );
        let mut outcome = ServiceOutcome::with_result(projected, channel(result));
        outcome = outcome.join(channel(driver.backend.checked_close()));
        let closed = driver.journal.close();
        let released = closed.is_ok();
        outcome = outcome.join(channel(closed));
        if retire && released {
            outcome = outcome.join(self.book.retire_closed(id, control).map(|_| ()));
        }
        outcome
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repair_default_archive_uses_target_volume_and_explicit_paths_are_not_rewritten() {
        assert_eq!(
            default_repair_archive(
                Path::new("C:\\project"),
                Path::new("D:\\wow\\_retail_"),
                "stamp"
            ),
            Path::new("D:\\wow\\_retail_").join("Interface/.lycheedev-repairs/stamp")
        );
        assert_eq!(
            default_repair_archive(
                Path::new("C:\\project"),
                Path::new("c:\\wow\\_retail_"),
                "stamp"
            ),
            Path::new("C:\\project").join(".lycheedev/live/repairs/stamp")
        );
        assert_eq!(
            volume(Path::new("\\\\?\\UNC\\server\\share\\x")),
            volume(Path::new("\\\\server\\share\\y"))
        );
    }
}
