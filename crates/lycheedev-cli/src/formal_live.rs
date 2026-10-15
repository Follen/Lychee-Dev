//! Fixed Go 3.2.0 public CON command adapter. The default executable enters this
//! module, never the superseded v4 host. Results remain typed through rendering.
#![forbid(unsafe_code)]
use lycheedev_core::{Invocation, Result, ToolkitError, VERSION, control::OperationControl};
use lycheedev_live::go_mailbox_v1::{
    admission::{AdmissionError, TargetFilter},
    connection::{
        ConnectionService, ExecuteRequest, NativeEnvironment, ProjectResult, ServiceOutcome,
        SystemClock, SystemScheduler, SystemTokens, TargetRequest, WindowsProcessLifetime,
        classify_error,
    },
    doctor::DoctorResult,
    observations::ChannelError,
    state::{OperationPolicy, validate_request},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[path = "formal_live_output.rs"]
mod output;
pub use output::render;
#[path = "formal_live_validation.rs"]
mod validation;

pub const TRANSPORT: &str = "Lycheedev mailbox protocol v1";

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum LiveResult {
    Connection(ProjectResult),
    Doctor(DoctorResult),
    Bootstrap(lycheedev_live::history::BootstrapAttempt),
    HistoricalOperation(lycheedev_live::history::HistoricalOutcome),
    HistoricalSession(lycheedev_live::history::HistoricalConnection),
}
#[derive(Debug, Default)]
pub struct Response {
    pub result: Option<LiveResult>,
    pub context: BTreeMap<String, Value>,
    pub operation_id: String,
    pub warnings: Vec<String>,
    pub error: Option<ToolkitError>,
}
impl Response {
    pub(crate) fn failure(error: ToolkitError) -> Self {
        Self {
            error: Some(error),
            ..Self::default()
        }
    }
    pub fn exit_code(&self) -> i32 {
        self.error.as_ref().map_or(0, |e| e.exit_code)
    }
}

pub fn handles(inv: &Invocation) -> bool {
    !inv.flag_bool("--help")
        && inv.command.starts_with("live ")
        && !matches!(
            inv.command.as_str(),
            "live instances"
                | "live probe put"
                | "live probe list"
                | "live probe show"
                | "live probe remove"
        )
}
/// Called in the parser's original argv order, before installing a console
/// handler. It deliberately does not move Source/Data validation or defaults.
pub fn validate_option(key: &str, value: &str) -> Result<()> {
    let integer = || value.parse::<i64>().ok();
    let message = match key {
        "--budget-seconds" if integer().is_none_or(|n| !(1..=120).contains(&n)) => {
            Some("--budget-seconds must be between 1 and 120")
        }
        "--wait-seconds" if integer().is_none_or(|n| !(1..=600).contains(&n)) => {
            Some("--wait-seconds must be between 1 and 600")
        }
        "--pid"
            if value.is_empty()
                || !value.bytes().all(|b| b.is_ascii_digit())
                || value.parse::<u32>().ok().is_none_or(|n| n == 0) =>
        {
            Some("invalid --pid")
        }
        "--count" if integer().is_none_or(|n| !(1..=2000).contains(&n)) => Some("invalid --count"),
        "--policy" if !matches!(value, "opaque" | "observation") => {
            Some("--policy must be observation or opaque")
        }
        _ => None,
    };
    message.map_or(Ok(()), |m| Err(ToolkitError::arguments(m)))
}

pub enum Action<'a> {
    Connect(&'a TargetRequest),
    Resume(&'a str),
    Status(&'a str),
    Disconnect(&'a str),
    Execute(&'a str, ExecuteRequest<'a>),
    Activate(&'a TargetRequest, &'a str),
    Reload(&'a str, &'a str),
}
/// An invocation-level test seam. The production implementation below delegates
/// every action into the same durable service; it contains no second lifecycle.
pub trait Services {
    fn project_root(&self) -> &Path;
    fn invoke(&mut self, action: Action<'_>, control: &OperationControl) -> ServiceOutcome;
    fn doctor(
        &mut self,
        target: &TargetRequest,
        session: &str,
        control: &OperationControl,
    ) -> DoctorResult;
    fn fix(
        &mut self,
        report: &DoctorResult,
        inv: &Invocation,
        control: &OperationControl,
    ) -> ServiceOutcome;
}
type NativeService = ConnectionService<
    NativeEnvironment<WindowsProcessLifetime>,
    SystemClock,
    SystemTokens,
    SystemScheduler,
>;
impl Services for NativeService {
    fn project_root(&self) -> &Path {
        ConnectionService::project_root(self)
    }
    fn invoke(&mut self, action: Action<'_>, control: &OperationControl) -> ServiceOutcome {
        match action {
            Action::Connect(request) => self.connect_with_activation(request, control),
            Action::Resume(id) => self.resume_with_activation(id, control),
            Action::Status(id) => self.status_with_activation(id, control),
            Action::Disconnect(id) => self.disconnect_with_activation(id, control),
            Action::Execute(id, request) => self.execute(id, request, control),
            Action::Activate(target, key) => self.activate(target, key, control),
            Action::Reload(id, key) => self.reload(id, key, control),
        }
    }
    fn doctor(
        &mut self,
        target: &TargetRequest,
        session: &str,
        control: &OperationControl,
    ) -> DoctorResult {
        ConnectionService::doctor(self, target, session, control)
    }
    fn fix(
        &mut self,
        report: &DoctorResult,
        inv: &Invocation,
        control: &OperationControl,
    ) -> ServiceOutcome {
        use lycheedev_live::go_mailbox_v1::connection::{
            doctor_delivery::ManagedDoctorDelivery, doctor_repair::DoctorFixOptions,
        };
        let mut delivery = match ManagedDoctorDelivery::new(VERSION, validation::CliAddonValidator)
        {
            Ok(delivery) => delivery,
            Err(error) => {
                return ServiceOutcome {
                    result: None,
                    error: Some(error.into()),
                };
            }
        };
        let bundled = bundled_doctor_release();
        let release = inv.flag("--release").map(|s| absolute(inv, Path::new(s)));
        let output = inv.flag("--output").map(|s| absolute(inv, Path::new(s)));
        self.doctor_fix(
            report,
            &DoctorFixOptions {
                session: flag(inv, "--session"),
                release: release.as_deref(),
                bundled_release: bundled.as_deref(),
                output: output.as_deref(),
                request: flag(inv, "--request"),
            },
            &mut delivery,
            control,
        )
    }
}
fn absolute(inv: &Invocation, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.into()
    } else {
        inv.cwd.join(path)
    }
}
fn bundled_doctor_release() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let mut directory = executable.parent()?;
    for _ in 0..3 {
        if directory.join("release.json").is_file() {
            return Some(directory.to_owned());
        }
        directory = directory.parent()?;
    }
    None
}
fn flag<'a>(inv: &'a Invocation, key: &str) -> &'a str {
    inv.flag(key).unwrap_or("")
}
fn project_path(inv: &Invocation) -> PathBuf {
    let path = inv
        .flag("--project")
        .map(PathBuf::from)
        .unwrap_or_else(|| inv.cwd.clone());
    if path.is_absolute() {
        path
    } else {
        inv.cwd.join(path)
    }
}
fn target(inv: &Invocation) -> TargetRequest {
    TargetRequest {
        installation: flag(inv, "--installation").into(),
        pid: inv.flag("--pid").and_then(|s| s.parse().ok()).unwrap_or(0),
        filter: TargetFilter {
            character: flag(inv, "--character").into(),
            realm: flag(inv, "--realm").into(),
            ..Default::default()
        },
    }
}
fn wait_control(inv: &Invocation, default: u64) -> Result<OperationControl> {
    let seconds = inv
        .flag("--wait-seconds")
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|_| ToolkitError::arguments("--wait-seconds must be between 1 and 600"))?
        .unwrap_or(default);
    if !(1..=600).contains(&seconds) {
        return Err(ToolkitError::arguments(
            "--wait-seconds must be between 1 and 600",
        ));
    }
    inv.control
        .ordered_child(Instant::now() + Duration::from_secs(seconds))
}
fn open_native(project: &Path) -> Result<NativeService> {
    let environment = NativeEnvironment::new(project, VERSION, WindowsProcessLifetime)
        .map_err(|e| ToolkitError::arguments(e.to_string()))?;
    ConnectionService::open(
        project,
        environment,
        SystemClock,
        SystemTokens,
        SystemScheduler::default(),
    )
    .map_err(|e| ToolkitError::arguments(e.to_string()))
}
pub fn dispatch(inv: &Invocation) -> Response {
    dispatch_with(inv, open_native)
}

fn route_admission(inv: &Invocation) -> Result<()> {
    let session = flag(inv, "--session");
    // Fixed Go entry.go rejects retired recovery before opening the workspace.
    // Unknown outcomes are inspect/abandon-only; never construct native services.
    if inv.command == "live resume" && !inv.positional.as_deref().unwrap_or("").starts_with("CON-")
    {
        if inv.positional.as_deref().unwrap_or("").is_empty() {
            return Err(ToolkitError::arguments(
                "missing selection file or record ID; use describe",
            ));
        }
        return Err(ToolkitError::arguments(
            "legacy OP/BTP execution is retired; use live status or live abandon; Native recovery requires CON",
        ));
    }

    if matches!(
        inv.command.as_str(),
        "live execute" | "live reload" | "live bugs"
    ) && !session.starts_with("CON-")
    {
        return Err(ToolkitError::arguments(
            "Native execution requires --session CON-...; historical OP/BTP execution is retired",
        ));
    }
    if session.starts_with("CON-") || inv.positional.as_deref().unwrap_or("").starts_with("CON-") {
        if !matches!(
            inv.command.as_str(),
            "live connect"
                | "live disconnect"
                | "live execute"
                | "live status"
                | "live session"
                | "live resume"
                | "live reload"
                | "live bugs"
                | "live doctor"
        ) {
            return Err(ToolkitError::arguments(
                "CON connections support execute, bugs, status, session, resume, reload and disconnect; cleanup is automatic",
            ));
        }
    }
    if matches!(
        inv.command.as_str(),
        "live status" | "live session" | "live cancel" | "live abandon"
    ) && inv.positional.as_deref().unwrap_or("").is_empty()
    {
        return Err(ToolkitError::arguments(
            "missing selection file or record ID; use describe",
        ));
    }
    if !matches!(
        inv.command.as_str(),
        "live connect"
            | "live disconnect"
            | "live execute"
            | "live status"
            | "live cancel"
            | "live abandon"
            | "live session"
            | "live resume"
            | "live reload"
            | "live reload fallback"
            | "live bugs"
            | "live doctor"
    ) {
        return Err(ToolkitError::new(
            "live.migration_incomplete",
            format!(
                "formal Live adapter for {} is not integrated; no input was sent",
                inv.command
            ),
            3,
        ));
    }
    Ok(())
}

pub fn dispatch_with<S: Services>(
    inv: &Invocation,
    factory: impl FnOnce(&Path) -> Result<S>,
) -> Response {
    if let Err(e) = route_admission(inv) {
        return Response::failure(e);
    }
    if matches!(inv.command.as_str(), "live cancel" | "live abandon") {
        let id = inv
            .positional
            .as_deref()
            .expect("admitted historical positional");
        let mut response = Response {
            operation_id: id.into(),
            ..Response::default()
        };
        if inv.command == "live abandon" && id.starts_with("BTP-") {
            let retired = lycheedev_live::history::abandon_bootstrap(&inv.home, id, &inv.control);
            response.result = Some(LiveResult::Bootstrap(retired.attempt));
            response.error = retired.error.map(|error| historical_failure(inv, error));
        } else {
            let retired = lycheedev_live::history::retire_operation(
                &inv.home,
                id,
                inv.command == "live cancel",
                &inv.control,
            );
            if !retired.outcome.operation_id.is_empty() {
                response
                    .context
                    .insert("stage".into(), json!(retired.outcome.stage));
                if retired.outcome.status == "abandoned" {
                    response.warnings.push("Cleanup abandoned explicitly; available evidence retained; missing report results remain unknown. No game ACK or runtime unload was performed.".into());
                }
                response.result = Some(LiveResult::HistoricalOperation(retired.outcome));
            }
            response.error = retired.error.map(|error| historical_failure(inv, error));
        }
        return response;
    }
    if inv.command == "live doctor" {
        if let Err(e) =
            lycheedev_live::go_mailbox_v1::connection::doctor_service::validate_doctor_options(
                &target(inv),
                flag(inv, "--session"),
                inv.flag_bool("--fix"),
                flag(inv, "--release"),
                flag(inv, "--output"),
                flag(inv, "--request"),
            )
        {
            return Response::failure(ToolkitError::arguments(channel_message(&e)));
        }
    }
    if matches!(inv.command.as_str(), "live status" | "live session")
        && !inv.positional.as_deref().unwrap_or("").starts_with("CON-")
    {
        let id = inv
            .positional
            .as_deref()
            .expect("admitted historical positional");
        let mut response = Response::default();
        let result = if inv.command == "live session" {
            lycheedev_live::history::inspect_session(&inv.home, id, &inv.control)
                .map(LiveResult::HistoricalSession)
        } else {
            response.operation_id = id.into();
            if id.starts_with("BTP-") {
                lycheedev_live::history::inspect_bootstrap(&inv.home, id, &inv.control)
                    .map(LiveResult::Bootstrap)
            } else {
                {
                    let read =
                        lycheedev_live::history::inspect_operation(&inv.home, id, &inv.control);
                    response.result = Some(LiveResult::HistoricalOperation(read.outcome));
                    match read.error {
                        Some(error) => Err(error),
                        None => Ok(response.result.take().expect("historical outcome")),
                    }
                }
            }
        };
        match result {
            Ok(value) => {
                response.result = Some(value);
                if inv.command == "live session" {
                    response.context.insert("session".into(), json!(id));
                    response.warnings.push("Historical session evidence only; live window identity and readiness have not been refreshed.".into());
                }
            }
            Err(error) => {
                // Fixed Go entry.go:773–776 clears failed status projections.
                // The library retains partial evidence; the CLI does not emit it.
                if inv.command == "live status" {
                    response.result = None;
                }
                response.error = Some(historical_failure(inv, error));
            }
        }
        return response;
    }
    let mut services = match factory(&project_path(inv)) {
        Ok(s) => s,
        Err(e) => return Response::failure(e),
    };
    let control = match wait_control(
        inv,
        if inv.command == "live doctor" {
            30
        } else {
            120
        },
    ) {
        Ok(c) => c,
        Err(e) => return Response::failure(super::command_fault(inv, e)),
    };
    if inv.command == "live doctor" {
        return run_doctor(inv, &mut services, &control);
    }
    let outcome = match prepare_and_run(inv, &mut services, &control) {
        Ok(value) => value,
        Err(e) => return Response::failure(super::command_fault(inv, e)),
    };
    connection_response(services.project_root(), outcome)
}
fn prepare_and_run(
    inv: &Invocation,
    services: &mut impl Services,
    control: &OperationControl,
) -> Result<ServiceOutcome> {
    let session = flag(inv, "--session");
    let key = flag(inv, "--request");
    let mut request = target(inv);
    let code;
    let action = match inv.command.as_str() {
        "live bugs" => {
            let count = inv
                .flag("--count")
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(80);
            if !(1..=100).contains(&count) || validate_request(key).is_err() {
                return Err(ToolkitError::arguments(
                    "live bugs requires --request and --count <1-100>",
                ));
            }
            code = format!("local snapshot, reason = LycheeDevInternal.Diagnostics.SnapshotRecentErrors({count}, \"provider_storage\")\nreturn {{schema=\"lycheedev.bugs.v1\",status=snapshot and \"completed\" or \"unavailable\",complete=snapshot and snapshot.complete==true or false,snapshot=snapshot,error=reason}}").into_bytes();
            Action::Execute(
                session,
                ExecuteRequest {
                    request: key,
                    code: &code,
                    budget_seconds: 5,
                    policy: OperationPolicy::Observation,
                },
            )
        }
        "live reload fallback" => {
            if request.installation.is_empty()
                || request.pid == 0
                || validate_request(key).is_err()
                || !session.is_empty()
            {
                return Err(ToolkitError::arguments(
                    "native activation requires --installation --pid --request; resume a pending CON id without new input",
                ));
            }
            Action::Activate(&request, key)
        }
        "live connect" if !session.is_empty() => {
            if !request.installation.is_empty()
                || request.pid != 0
                || !request.filter.character.is_empty()
                || !request.filter.realm.is_empty()
                || !flag(inv, "--snapshot").is_empty()
            {
                return Err(ToolkitError::arguments(
                    "resume the retained CON session without new target selectors",
                ));
            }
            Action::Resume(session)
        }
        "live connect" => {
            if let Some(snapshot) = inv.flag("--snapshot") {
                let pin = lycheedev_core::selection::load_pinned(&inv.home, snapshot)
                    .map_err(|e| ToolkitError::arguments(e.message))?;
                let data = pin.data.as_ref().ok_or_else(|| {
                    ToolkitError::arguments("live snapshot requires a pinned data build")
                })?;
                request.filter.build = data.full_build.clone();
                request.filter.product = data.product.clone();
            }
            Action::Connect(&request)
        }
        "live execute" => {
            let file = flag(inv, "--file");
            let probe = flag(inv, "--probe");
            let budget = inv
                .flag("--budget-seconds")
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            if file.is_empty() == probe.is_empty()
                || !(1..=120).contains(&budget)
                || validate_request(key).is_err()
            {
                return Err(ToolkitError::arguments(
                    "live execute requires --session CON-... --request <key> --budget-seconds <1-120> and exactly one of --file or --probe",
                ));
            }
            let bytes = if !file.is_empty() {
                // The file helper converts only ordinary read admission failures.
                // Its final checked-close/control error must remain typed and whole.
                read_probe_file(inv, Path::new(file))?
            } else {
                lycheedev_core::workspace::open(&inv.home)
                    .and_then(|_| lycheedev_live::probes::code(inv, probe))
                    .map_err(probe_admission_error)?
            };
            // Preserve the file/probe's exact bytes through request identity.
            // The formal driver owns Go's JSON-string encoding boundary.
            code = bytes;
            let policy = match flag(inv, "--policy") {
                "" | "opaque" => OperationPolicy::Opaque,
                "observation" => OperationPolicy::Observation,
                _ => {
                    return Err(ToolkitError::arguments(
                        "--policy must be observation or opaque",
                    ));
                }
            };
            Action::Execute(
                session,
                ExecuteRequest {
                    request: key,
                    code: &code,
                    budget_seconds: budget,
                    policy,
                },
            )
        }
        "live status" | "live session" => Action::Status(inv.argument()?),
        "live resume" => Action::Resume(inv.argument()?),
        "live disconnect" => Action::Disconnect(inv.argument()?),
        "live reload" => Action::Reload(session, key),
        _ => unreachable!("route admitted before project open"),
    };
    Ok(services.invoke(action, control))
}
fn probe_admission_error(error: ToolkitError) -> ToolkitError {
    // Only ordinary pre-execution input errors have Go's explicit argument
    // classification. Never erase a stop, cleanup aggregate, or its cause tree.
    if matches!(
        error.code.as_str(),
        "command.cancelled" | "command.deadline" | "process.cancelled"
    ) || !error.details.is_null()
    {
        error
    } else {
        ToolkitError::arguments(error.message)
    }
}
fn read_probe_file(inv: &Invocation, path: &Path) -> Result<Vec<u8>> {
    read_probe_file_with(
        inv,
        path,
        |file| {
            let mut bytes = Vec::new();
            file.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        },
        |file, result| lycheedev_core::storage::finish_file(file, result, &inv.control),
    )
}
fn read_probe_file_with(
    inv: &Invocation,
    path: &Path,
    read: impl FnOnce(&mut File) -> Result<Vec<u8>>,
    finish: impl FnOnce(File, Result<Vec<u8>>) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    let path = absolute(inv, path);
    inv.control.check()?;
    let mut file = File::open(path)
        .map_err(ToolkitError::io)
        .map_err(probe_admission_error)?;
    let result = (|| {
        inv.control.check()?;
        let info = file.metadata().map_err(ToolkitError::io)?;
        if !info.is_file() || info.len() == 0 || info.len() > 256 * 1024 {
            return Err(ToolkitError::arguments(
                "probe must be a non-empty regular file of at most 256 KiB",
            ));
        }
        read(&mut file)
    })();
    // First consume the handle and preserve any combined original read, close
    // and control causes. Only a lone ordinary admission failure is converted
    // to Go's argument classification; cleanup-only failures stay typed too.
    let failed_before_close = result.is_err();
    match finish(file, result) {
        Err(error) if failed_before_close => Err(probe_admission_error(error)),
        result => result,
    }
}

fn channel_message(error: &ChannelError) -> String {
    match error {
        ChannelError::Toolkit { message, .. } => message.clone(),
        _ => error.to_string(),
    }
}
fn has_channel(error: &AdmissionError, predicate: &impl Fn(&ChannelError) -> bool) -> bool {
    match error {
        AdmissionError::Channel(e) => e.any_cause(predicate),
        AdmissionError::Joined(errors) => errors.iter().any(|e| has_channel(e, predicate)),
        AdmissionError::Occupied { .. } => false,
    }
}
fn public_error(error: &AdmissionError) -> ToolkitError {
    let failure = classify_error(error);
    let precedence = failure.as_ref().is_some_and(|f| {
        matches!(
            f.code.as_str(),
            "live.channel_trace_flush" | "live.channel_wait_cancelled" | "live.channel_pending"
        )
    });
    if !precedence && has_channel(error, &|e| matches!(e, ChannelError::DeadlineExceeded)) {
        return ToolkitError::new("live.deadline_pending", error.to_string(), 6)
            .with_details(failure.map_or(Value::Null, |f| f.details));
    }
    if let Some(failure) = failure {
        let mut e = ToolkitError::new(failure.code, failure.message, failure.exit_code)
            .with_details(failure.details);
        if e.code.starts_with("live.channel_")
            || e.code.starts_with("selection.installation_")
            || e.code == "doctor.check_failed"
        {
            if e.details.is_null() {
                e.details = json!({});
            }
            if e.details.is_object() {
                e.details["stage"] = json!("query");
            }
        }
        return e;
    }
    let (code, exit) = if error.is_busy() {
        ("journal.resource_busy", 3)
    } else {
        ("command.failed", 5)
    };
    ToolkitError::new(code, error.to_string(), exit)
}

fn connection_response(project: &Path, outcome: ServiceOutcome) -> Response {
    let mut response = Response::default();
    response.context.insert(
        "project".into(),
        json!(lycheedev_core::client::display_path(project)),
    );
    response
        .context
        .insert("transport".into(), json!(TRANSPORT));
    if let Some(result) = outcome.result.filter(|r| !r.session.is_empty()) {
        response.operation_id = result.session.clone();
        response
            .context
            .insert("session".into(), json!(result.session));
        response.context.insert("stage".into(), json!(result.stage));
        if !result.operation.is_empty() {
            response
                .context
                .insert("operation".into(), json!(result.operation));
        }
        response.result = Some(LiveResult::Connection(result));
    }
    response.error = outcome.error.as_ref().map(public_error);
    merge_context(&mut response);
    response
}
fn merge_context(response: &mut Response) {
    if let Some(context) = response
        .error
        .as_ref()
        .and_then(|e| e.details.get("__commandContext"))
        .and_then(Value::as_object)
    {
        response
            .context
            .extend(context.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
}
fn unhealthy() -> ToolkitError {
    ToolkitError::new("doctor.check_failed", "doctor.check_failed", 3)
        .with_details(json!({"stage":"query"}))
}
fn run_doctor(
    inv: &Invocation,
    services: &mut impl Services,
    control: &OperationControl,
) -> Response {
    let target = target(inv);
    let mut report = services.doctor(&target, flag(inv, "--session"), control);
    let mut response = Response::default();
    response.context.insert(
        "project".into(),
        json!(lycheedev_core::client::display_path(
            services.project_root()
        )),
    );
    response.context.insert("protocol".into(), json!(TRANSPORT));
    if inv.flag_bool("--fix") {
        let fixed = services.fix(&report, inv, control);
        if let Some(result) = &fixed.result {
            response.operation_id = result.session.clone();
        }
        if let Some(error) = fixed.error {
            report.healthy = false;
            let next = if response.operation_id.is_empty() {
                "resolve the repair error, then run live doctor --fix for the same target".into()
            } else {
                format!(
                    "live resume {} --project <original-project>",
                    response.operation_id
                )
            };
            report.add("live.repair", "blocked", error.to_string(), &next);
            response.error = Some(public_error(&error));
        } else {
            report = services.doctor(&target, &response.operation_id, control);
        }
        if let Some(result) = fixed.result.filter(|r| !r.session.is_empty()) {
            report.connection = Some(result);
        }
    }
    if response.error.is_none() && !report.healthy {
        response.error = Some(unhealthy());
    }
    response.result = Some(LiveResult::Doctor(report));
    merge_context(&mut response);
    response
}

/// Compose the same read-only checks into ordinary doctor. No selected target
/// means no project open, native capability probe, or claim read at all.
pub fn doctor_checks(inv: &Invocation) -> Vec<Value> {
    if flag(inv, "--installation").is_empty()
        && flag(inv, "--pid").is_empty()
        && flag(inv, "--session").is_empty()
    {
        return vec![];
    }
    let mut service = match open_native(&project_path(inv)) {
        Ok(s) => s,
        Err(e) => {
            return vec![
                json!({"id":"live.project","status":"error","ok":false,"detail":e.message}),
            ];
        }
    };
    let control = match inv
        .control
        .ordered_child(Instant::now() + Duration::from_secs(10))
    {
        Ok(c) => c,
        Err(e) => {
            return vec![
                json!({"id":"live.project","status":"error","ok":false,"detail":e.message}),
            ];
        }
    };
    let report = service.doctor(&target(inv), flag(inv, "--session"), &control);
    project_doctor_checks(report)
}
fn project_doctor_checks(report: DoctorResult) -> Vec<Value> {
    report
        .checks
        .into_iter()
        .map(|c| {
            let status = if c.state == "blocked" {
                "error"
            } else {
                &c.state
            };
            let mut value = json!({"id":c.id,"status":status,"ok":status=="ok","detail":c.detail});
            if !c.next_step.is_empty() {
                value["nextStep"] = json!(c.next_step);
            }
            value
        })
        .collect()
}

#[cfg(test)]
#[path = "formal_live_tests.rs"]
mod tests;

fn historical_failure(inv: &Invocation, mut error: ToolkitError) -> ToolkitError {
    // Go errors.Is searches every errors.Join leaf, in entry.go priority order.
    // Only the core-owned complete aggregate schema permits traversal.
    fn has(error: &ToolkitError, wanted: &str, depth: usize) -> bool {
        if error.code == wanted {
            return true;
        }
        if depth >= 32
            || error.details.as_object().is_none_or(|o| o.len() != 4)
            || error.details["schema"] != lycheedev_core::storage::FILE_CLEANUP_SCHEMA
        {
            return false;
        }
        ["primary", "cleanup", "control"].into_iter().any(|key| {
            serde_json::from_value::<ToolkitError>(error.details[key].clone())
                .is_ok_and(|nested| has(&nested, wanted, depth + 1))
        })
    }
    let selected = [
        ("journal.resource_busy", 3),
        ("journal.invalid_transition", 3),
        ("command.cancelled", 7),
        ("command.deadline", 6),
        ("live.deadline_pending", 6),
        ("vault.legacy_detected", 3),
        ("vault.unsupported_workspace", 4),
        ("vault.workspace_schema_newer", 4),
        ("vault.blob_integrity", 4),
        ("vault.generation_conflict", 3),
        ("vault.record_missing", 2),
    ]
    .into_iter()
    .find(|(code, _)| has(&error, code, 0));
    if let Some((code, exit)) = selected {
        error.code = match code {
            "vault.workspace_schema_newer" => "vault.unsupported_workspace",
            "command.deadline" => "live.deadline_pending",
            other => other,
        }
        .into();
        error.exit_code = exit;
        // A joined failure keeps its complete message, including the first cause.
        if code == "vault.record_missing" && error.details.is_null() {
            error.message = "vault.record_missing".into();
        }
    } else {
        error.code = "command.failed".into();
        error.exit_code = 5;
    }
    super::command_fault(inv, error)
}
