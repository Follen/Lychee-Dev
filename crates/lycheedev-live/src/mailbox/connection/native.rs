//! Production safe adapters. NativeReader/input/typed publication remain the
//! only memory and input implementation; no historical Transport is accepted.
use super::super::{
    InputObservation,
    admission::{ClientRecord, WindowRecord, inspect_window_owner},
    native_backend::NativeBackend,
    observations::{Backend, InputAction, InputDispatch, ObservationDispatch, ObservationQuery},
    state::RuntimeReplacementProof,
};
use super::*;
use lycheedev_core::{
    client,
    installation_selection::{self, InvocationMetadata},
};
use lycheedev_delivery::{
    installation,
    slot_publication::{PublicationError, PublicationLease, RecordedInstallationIdentity},
};
use lycheedev_windows::{ProcessIdentity, WindowIdentity, mailbox_input::MailboxWindow};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub struct SystemClock;
impl Clock for SystemClock {
    fn unix_millis(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(elapsed) => i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
            Err(before) => -i64::try_from(before.duration().as_millis()).unwrap_or(i64::MAX),
        }
    }
}
pub struct SystemTokens;
impl TokenSource for SystemTokens {
    fn token(&mut self) -> ChannelResult<String> {
        lycheedev_windows::random::token().map_err(external)
    }
}
pub struct SystemScheduler {
    started: Instant,
}
impl Default for SystemScheduler {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}
impl ContinuationScheduler for SystemScheduler {
    fn monotonic_millis(&self) -> u64 {
        self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }
    fn wait(&mut self, control: &OperationControl, duration: Duration) -> ChannelResult<()> {
        let until = Instant::now()
            .checked_add(duration)
            .ok_or_else(|| error("live.channel_budget_invalid"))?;
        loop {
            check_control(control)?;
            let now = Instant::now();
            if now >= until {
                return Ok(());
            }
            let remaining = control
                .remaining()
                .map_err(durable_journal::storage_error)?;
            std::thread::sleep((until - now).min(remaining).min(Duration::from_millis(25)));
        }
    }
}
/// Must be implemented by the checked-close Windows process-lifetime boundary.
/// The WindowsProcessLifetime implementation never turns an OS error into end
/// evidence. Alternative implementations are explicit test/application seams.
pub trait ProcessLifetimeObserver {
    fn observe(
        &mut self,
        original: &ProcessIdentity,
        control: &OperationControl,
    ) -> ChannelResult<ProcessEnd>;
}
/// Real Windows adapter; only a successful checked-close observation becomes
/// process-end evidence. Cancellation remains typed alongside OS/close causes.
pub struct WindowsProcessLifetime;
impl ProcessLifetimeObserver for WindowsProcessLifetime {
    fn observe(
        &mut self,
        original: &ProcessIdentity,
        control: &OperationControl,
    ) -> ChannelResult<ProcessEnd> {
        use lycheedev_windows::process::{ProcessLifetime, observe_lifetime};
        let observed = observe_lifetime(original, || control.check().map_err(|e| e.to_string()));
        let stopped = check_control(control);
        match (observed, stopped) {
            (Ok(lifetime), Ok(())) => Ok(match lifetime {
                ProcessLifetime::Running => ProcessEnd::None,
                ProcessLifetime::Absent => ProcessEnd::ProcessAbsent,
                ProcessLifetime::Exited => ProcessEnd::ProcessExited,
                ProcessLifetime::PidReused => ProcessEnd::PidReused,
            }),
            (Err(e), Err(stop)) => Err(ChannelError::Joined {
                causes: vec![external(e), stop],
            }),
            (Err(e), Ok(())) => Err(external(e)),
            (Ok(_), Err(stop)) => Err(stop),
        }
    }
}
pub struct NativeEnvironment<L> {
    project: PathBuf,
    release: String,
    lifetime: L,
}
impl<L: ProcessLifetimeObserver> NativeEnvironment<L> {
    pub fn new(project: &Path, release: &str, lifetime: L) -> ChannelResult<Self> {
        if release.is_empty() {
            return Err(error("live.channel_clean_current_addon_required"));
        }
        Ok(Self {
            project: fs::canonicalize(project).map_err(durable_journal::io_error)?,
            release: release.into(),
            lifetime,
        })
    }
}
fn external(value: impl Into<String>) -> ChannelError {
    ChannelError::External {
        causes: vec![value.into()],
    }
}
fn publication(error: PublicationError) -> ChannelError {
    match error {
        PublicationError::Failure(e) => durable_journal::storage_error(e),
        PublicationError::CloseFailures(es) => ChannelError::Joined {
            causes: es.into_iter().map(durable_journal::storage_error).collect(),
        },
        PublicationError::Reserved { .. } => {
            external("delivery.installation_conflict: delivery.slot_reserved")
        }
    }
}
fn process(target: &ConnectionTarget) -> ProcessIdentity {
    ProcessIdentity {
        pid: target.window.process_id,
        created: target.window.process_started_at,
    }
}
fn bind(target: &ConnectionTarget) -> ChannelResult<MailboxWindow> {
    let hwnd =
        usize::try_from(target.window.handle).map_err(|_| error("desktop.identity_changed"))?;
    MailboxWindow::bind(
        WindowIdentity {
            process: process(target),
            hwnd,
        },
        &target.window.executable,
        &target.window.class,
    )
    .map_err(external)
}
fn same_path(left: &Path, right: &Path) -> bool {
    lycheedev_windows::mailbox_input::equal_fold_utf16(
        &client::display_path(left)
            .encode_utf16()
            .collect::<Vec<_>>(),
        &client::display_path(right)
            .encode_utf16()
            .collect::<Vec<_>>(),
    )
}
fn game_executable(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        ["wow.exe", "wowclassic.exe", "wowb.exe"]
            .iter()
            .any(|n| name.to_string_lossy().eq_ignore_ascii_case(n))
    })
}
fn confirm(target: &ConnectionTarget, control: &OperationControl) -> ChannelResult<MailboxWindow> {
    check_control(control)?;
    let window = bind(target)?;
    let observed = client::inspect_with(
        Path::new(&target.client.directory),
        &mut InvocationMetadata(control),
    )
    .map_err(durable_journal::storage_error)?;
    let mut current = target.clone();
    current.client = ClientRecord::from(&observed);
    if !target.same_connection_target(&current) {
        return Err(error("live.channel_clean_current_addon_required"));
    }
    let executable = Path::new(&target.window.executable);
    if !executable.is_absolute() || !game_executable(executable) {
        return Err(error("live.window_executable_mismatch"));
    }
    let resolved = fs::canonicalize(executable).map_err(durable_journal::io_error)?;
    if !resolved
        .parent()
        .is_some_and(|parent| same_path(parent, Path::new(&observed.directory)))
    {
        return Err(error("live.window_installation_mismatch"));
    }
    check_control(control)?;
    // Rebind at the end, after filesystem work, to catch an identity change.
    let _ = window;
    bind(target)
}
// Fixed Go delivery.AddonDirectory is a pure projection of the saved client.
// Do not rediscover a potentially replaced installation on process-end cleanup.
pub(super) fn recorded_addon_directory(target: &ConnectionTarget) -> PathBuf {
    target.addon_parent().join("Lychee Dev")
}
fn deployment(
    target: &ConnectionTarget,
    release: &str,
    control: &OperationControl,
) -> ChannelResult<installation::VerifiedInstallation> {
    confirm(target, control)?;
    let verified = installation::verify_main_addon_controlled(
        &recorded_addon_directory(target),
        release,
        control,
    )
    .map_err(deployment_error)?;
    check_control(control)?;
    Ok(verified)
}
// Only a definite non-clean classification without cleanup/control causes
// becomes the capability blocker. IO, malformed receipts, and joined close
// failures retain their original typed code/exit/details.
pub(super) fn deployment_error(error: lycheedev_core::ToolkitError) -> ChannelError {
    if error.details.is_null()
        && matches!(
            error.code.as_str(),
            "delivery.installation_not_clean" | "delivery.invalid_payload"
        )
    {
        ChannelError::Joined {
            causes: vec![
                super::error("live.channel_clean_current_addon_required"),
                error.into(),
            ],
        }
    } else {
        error.into()
    }
}
// Shared by real resolution and filesystem regression fixtures. Window
// enumeration is separate; this uses the same observed report and error DTO.
pub(super) fn discover_installations(
    directory: &str,
    control: &OperationControl,
) -> ChannelResult<Option<Vec<client::ClientInstallation>>> {
    if directory.is_empty() {
        return Ok(None);
    }
    let report = installation_selection::discover_with(
        Path::new(directory),
        &mut InvocationMetadata(control),
    )
    .map_err(durable_journal::storage_error)?;
    installation_selection::require_discovered_candidates(&report)
        .map_err(durable_journal::storage_error)?;
    Ok(Some(report.candidates))
}
impl<L: ProcessLifetimeObserver> ConnectionEnvironment for NativeEnvironment<L> {
    type Backend = NativeConnectionBackend;
    fn resolve(
        &mut self,
        request: &TargetRequest,
        control: &OperationControl,
    ) -> ChannelResult<ConnectionTarget> {
        check_control(control)?;
        let windows = lycheedev_windows::inventory::passive_windows(|| {
            control.check().map_err(|e| e.to_string())
        });
        check_control(control)?;
        let windows = windows.map_err(external)?;
        let candidates = discover_installations(&request.installation, control)?;
        let mut selected = None;
        for window in windows {
            check_control(control)?;
            if request.pid != 0 && window.identity.process.pid != request.pid {
                continue;
            }
            let executable = Path::new(&window.executable);
            if !game_executable(executable) {
                if request.pid != 0 {
                    return Err(error("live.window_executable_mismatch"));
                }
                continue;
            }
            if window.identity.hwnd == 0
                || window.identity.process.created == 0
                || window.class.is_empty()
                || !executable.is_absolute()
            {
                return Err(error("live.invalid_window_identity"));
            }
            let executable = fs::canonicalize(executable).map_err(durable_journal::io_error)?;
            let parent = executable
                .parent()
                .ok_or_else(|| error("live.invalid_window_identity"))?;
            let client = if let Some(candidates) = &candidates {
                match candidates
                    .iter()
                    .find(|c| same_path(parent, Path::new(&c.directory)))
                {
                    Some(c) => c.clone(),
                    None if request.pid != 0 => {
                        return Err(error("live.window_installation_mismatch"));
                    }
                    None => continue,
                }
            } else {
                client::inspect_with(parent, &mut InvocationMetadata(control))
                    .map_err(durable_journal::storage_error)?
            };
            if selected.is_some() {
                return Err(error("live.window_ambiguous"));
            }
            selected = Some(ConnectionTarget {
                client: ClientRecord::from(&client),
                window: WindowRecord {
                    handle: window.identity.hwnd as u64,
                    process_id: window.identity.process.pid,
                    process_started_at: window.identity.process.created,
                    executable: window.executable,
                    class: window.class,
                    title: window.title,
                },
            });
        }
        let target = selected.ok_or_else(|| error("live.window_not_found"))?;
        confirm(&target, control)?;
        Ok(target)
    }
    fn open(
        &mut self,
        target: &ConnectionTarget,
        owner: Option<&WindowOwner>,
        control: &OperationControl,
    ) -> ChannelResult<Self::Backend> {
        let installation = deployment(target, &self.release, control)?;
        let window = bind(target)?;
        let target = target.clone();
        let owner = owner.cloned();
        let release = self.release.clone();
        let scan_id = lycheedev_windows::random::token().map_err(external)?;
        let scans = self
            .project
            .join(".lycheedev")
            .join("live")
            .join("scans")
            .join(scan_id);
        let inner = NativeBackend::open(
            window,
            &installation,
            &self.release,
            Some(scans.clone()),
            move |control| {
                deployment(&target, &release, control)?;
                if let Some(owner) = &owner {
                    if owner.resource != target.resource() {
                        return Err(error("live.channel_ownership_changed"));
                    }
                    let actual =
                        inspect_window_owner(&target.addon_parent(), &owner.resource, control);
                    if let Some(e) = actual.error {
                        return Err(e);
                    }
                    if actual.owner.as_ref() != Some(owner) {
                        return Err(error("live.channel_ownership_changed"));
                    }
                }
                Ok(())
            },
            control,
        )?;
        Ok(NativeConnectionBackend {
            inner,
            scans: scans.display().to_string(),
        })
    }
    fn process_end(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<ProcessEnd> {
        self.lifetime.observe(&process(target), control)
    }
    fn retire_process(
        &mut self,
        target: &ConnectionTarget,
        release: &str,
        envelopes: &[SlotEnvelope],
        control: &OperationControl,
    ) -> ChannelResult<()> {
        let identity = RecordedInstallationIdentity::from_authenticated_record(
            recorded_addon_directory(target),
        )
        .map_err(publication)?;
        let mut lease =
            PublicationLease::acquire_recorded(&identity, release, control).map_err(publication)?;
        let consumer = format!(
            "{}/{}",
            target.window.process_id, target.window.process_started_at
        );
        let retired = envelopes.iter().try_for_each(|envelope| {
            lease
                .retire_process(&consumer, envelope)
                .map_err(publication)
        });
        durable_journal::joined([retired, lease.close().map_err(publication)])
    }
}
pub struct NativeConnectionBackend {
    inner: NativeBackend,
    scans: String,
}
impl ConnectionBackend for NativeConnectionBackend {
    fn discover(
        &mut self,
        control: &OperationControl,
        character: &str,
        realm: &str,
    ) -> ChannelResult<Vec<Identity>> {
        self.inner.discover(control, character, realm)
    }
    fn checked_close(&mut self) -> ChannelResult<()> {
        self.inner.close()
    }
    fn scans(&self) -> &str {
        &self.scans
    }
}
impl Backend for NativeConnectionBackend {
    fn observe(&mut self, c: &OperationControl, q: &ObservationQuery) -> ObservationDispatch {
        self.inner.observe(c, q)
    }
    fn publish(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        self.inner.publish(c, e)
    }
    fn observe_input(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        a: i64,
        cap: &str,
    ) -> ChannelResult<InputObservation> {
        self.inner.observe_input(c, e, a, cap)
    }
    fn input(&mut self, c: &OperationControl, a: &InputAction) -> InputDispatch {
        self.inner.input(c, a)
    }
    fn runtime_candidate(
        &mut self,
        c: &OperationControl,
        i: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        self.inner.runtime_candidate(c, i)
    }
    fn supersede(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        i: &Identity,
    ) -> ChannelResult<()> {
        self.inner.supersede(c, e, i)
    }
    fn consumed(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        self.inner.consumed(c, e)
    }
}
impl RuntimeRetirementBackend for NativeConnectionBackend {
    fn observe_runtime_replacement(
        &mut self,
        c: &OperationControl,
        i: &Identity,
    ) -> ChannelResult<Option<RuntimeReplacementProof>> {
        self.inner.observe_runtime_replacement(c, i)
    }
    fn retire_replaced_runtime(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        p: &RuntimeReplacementProof,
    ) -> ChannelResult<()> {
        self.inner.retire_replaced_runtime(c, e, p)
    }
}

#[path = "../activation_native.rs"]
mod activation_native;

#[path = "../doctor_native.rs"]
mod doctor_native;
