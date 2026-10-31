//! Public CON command adapter. Results remain typed through rendering.
#![forbid(unsafe_code)]
use crate::Invocation;
use lycheedev_core::{Result, ToolkitError, VERSION, control::OperationControl};
use lycheedev_live::mailbox::{
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
    DoctorAggregate(DoctorAggregate),
}
#[derive(Debug, Serialize)]
pub struct DoctorAggregate {
    healthy: bool,
    checks: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    live: Option<DoctorResult>,
}
#[derive(Debug, Default)]
pub struct Response {
    pub result: Option<LiveResult>,
    pub context: BTreeMap<String, Value>,
    pub operation_id: String,
    pub warnings: Vec<Value>,
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
        && (inv.command == "doctor" || inv.command.starts_with("live "))
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
        "--count" if integer().is_none_or(|n| !(1..=100).contains(&n)) => Some("invalid --count"),
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
        report: DoctorResult,
        inv: &Invocation,
        control: &OperationControl,
    ) -> (DoctorResult, ServiceOutcome);
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
        mut report: DoctorResult,
        inv: &Invocation,
        control: &OperationControl,
    ) -> (DoctorResult, ServiceOutcome) {
        use lycheedev_live::mailbox::connection::{
            doctor_delivery::ManagedDoctorDelivery, doctor_repair::DoctorFixOptions,
        };
        let mut delivery = match ManagedDoctorDelivery::new(VERSION, validation::CliAddonValidator)
        {
            Ok(delivery) => delivery,
            Err(error) => {
                report.add(
                    "live.repair",
                    "blocked",
                    error.to_string(),
                    "resolve the repair error, then run doctor --fix for the same target",
                );
                return (report, ServiceOutcome::Failed(error.into()));
            }
        };
        let bundled = bundled_doctor_release();
        let release = inv.flag("--release").map(|s| absolute(inv, Path::new(s)));
        let output = inv.flag("--output").map(|s| absolute(inv, Path::new(s)));
        self.doctor_fix_report(
            report,
            &DoctorFixOptions {
                session: flag(inv, "--session"),
                release: release.as_deref(),
                bundled_release: bundled.as_deref(),
                output: output.as_deref(),
                request: flag(inv, "--request"),
                hard_reset: inv.flag_bool("--hard-reset"),
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
    if inv.command == "doctor" {
        aggregate_doctor_with(
            inv,
            || crate::dispatch_doctor(inv),
            || dispatch_with(inv, open_native),
        )
    } else {
        dispatch_with(inv, open_native)
    }
}

fn route_admission(inv: &Invocation) -> Result<()> {
    let session = flag(inv, "--session");
    if !matches!(
        inv.command.as_str(),
        "live connect"
            | "live disconnect"
            | "live execute"
            | "live status"
            | "live session"
            | "live resume"
            | "live reload"
            | "live reload fallback"
            | "live bugs"
            | "doctor"
    ) {
        return Err(ToolkitError::arguments("unknown Live command"));
    }
    if matches!(
        inv.command.as_str(),
        "live status" | "live session" | "live resume"
    ) && !inv.positional.as_deref().unwrap_or("").starts_with("CON-")
    {
        return Err(ToolkitError::arguments(
            "Live recovery requires a CON connection ID",
        ));
    }
    if matches!(
        inv.command.as_str(),
        "live execute" | "live reload" | "live bugs"
    ) && !session.starts_with("CON-")
    {
        return Err(ToolkitError::arguments(
            "Native execution requires --session CON-...",
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
    if inv.command == "doctor"
        && let Err(error) = validate_doctor(inv)
    {
        return Response::failure(error);
    }
    let mut services = match factory(&project_path(inv)) {
        Ok(s) => s,
        Err(e) => return Response::failure(e),
    };
    let control = match wait_control(inv, if inv.command == "doctor" { 30 } else { 120 }) {
        Ok(c) => c,
        Err(e) => return Response::failure(super::command_fault(inv, e)),
    };
    if inv.command == "doctor" {
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
                let pin = lycheedev_core::selection::load_pinned_controlled(
                    &inv.home,
                    snapshot,
                    &inv.control,
                )
                .map_err(|error| {
                    // Only the selected input's ordinary admission errors
                    // become arguments. Keep resource/control failures and
                    // every structured cleanup cause from the pin owner.
                    if error.details.is_null()
                        && (error.code.starts_with("selection.")
                            || matches!(
                                error.code.as_str(),
                                "input.invalid" | "vault.record_missing"
                            ))
                    {
                        ToolkitError::arguments(error.message)
                    } else {
                        error
                    }
                })?;
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
                    .and_then(|_| {
                        lycheedev_live::probes::code(&crate::live_aux::context(inv), probe)
                    })
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
    let result = match outcome {
        ServiceOutcome::Completed(result) => Some(result),
        ServiceOutcome::Failed(error) => {
            response.error = Some(public_error(&error));
            None
        }
        ServiceOutcome::ReportedWithFailure { result, error } => {
            response.error = Some(public_error(&error));
            Some(result)
        }
    };
    if let Some(result) = result.filter(|r| !r.session.is_empty()) {
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
fn validate_doctor(inv: &Invocation) -> Result<()> {
    lycheedev_live::mailbox::connection::doctor_service::validate_doctor_options(
        &target(inv),
        flag(inv, "--session"),
        inv.flag_bool("--fix"),
        inv.flag_bool("--hard-reset"),
        flag(inv, "--release"),
        flag(inv, "--output"),
        flag(inv, "--request"),
    )
    .map_err(|error| ToolkitError::arguments(channel_message(&error)))
}
fn has_doctor_target(inv: &Invocation) -> bool {
    ["--installation", "--pid", "--session"]
        .iter()
        .any(|key| !flag(inv, key).is_empty())
}
/// Keep the optional live report typed through rendering. Workspace/provider
/// checks use Values, but a connection's RawReport must never pass through one.
fn aggregate_doctor_with(
    inv: &Invocation,
    inspect: impl FnOnce() -> Result<lycheedev_core::Outcome>,
    live: impl FnOnce() -> Response,
) -> Response {
    if let Err(error) = validate_doctor(inv) {
        return Response::failure(error);
    }
    let mut response = Response::default();
    let value = match inspect() {
        Ok(outcome) => {
            if let Some(context) = outcome.selection.as_object() {
                response.context.extend(
                    context
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
            }
            response.warnings = outcome.warnings;
            outcome.data
        }
        Err(mut error) => {
            let value = error.details["result"].clone();
            if let Some(context) = error.details["context"].as_object() {
                response.context.extend(
                    context
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
            }
            if let Some(details) = error.details.as_object_mut() {
                details.remove("result");
                details.remove("context");
            }
            response.error = Some(error);
            value
        }
    };
    let mut checks = value["checks"].as_array().cloned().unwrap_or_default();
    let can_continue = response
        .error
        .as_ref()
        .is_none_or(|error| error.code == "doctor.unhealthy");
    let mut report = None;
    if can_continue && (has_doctor_target(inv) || inv.flag_bool("--fix")) {
        let mut selected = live();
        response.context.extend(selected.context);
        response.operation_id = selected.operation_id;
        response.warnings.extend(selected.warnings);
        if let Some(LiveResult::Doctor(live)) = selected.result.take() {
            checks.extend(project_doctor_checks(&live));
            report = Some(live);
        } else if let Some(error) = &selected.error {
            checks.push(
                json!({"id":"live.project","ok":false,"status":"error","detail":error.message}),
            );
        }
        if let Some(mut error) = selected.error {
            if error.code == "doctor.check_failed" {
                error.code = "doctor.unhealthy".into();
                error.message = "doctor has error-status health checks".into();
            }
            response.error = Some(error);
        }
    }
    let healthy = response.error.is_none() && checks.iter().all(|check| check["status"] != "error");
    if !healthy && response.error.is_none() {
        response.error = Some(ToolkitError::new(
            "doctor.unhealthy",
            "doctor has error-status health checks",
            3,
        ));
    }
    response.result = Some(LiveResult::DoctorAggregate(DoctorAggregate {
        healthy,
        checks,
        live: report,
    }));
    merge_context(&mut response);
    response
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
    response.error = doctor_error(&report);
    if inv.flag_bool("--fix") && response.error.is_none() {
        let (repaired, outcome) = services.fix(report, inv, control);
        report = repaired;
        // Successful repair may itself be followed by a failed diagnostic.
        // Its typed causes are the authoritative projection for that report.
        response.error = doctor_error(&report);
        match outcome {
            ServiceOutcome::Completed(_) => {}
            ServiceOutcome::Failed(error) => {
                if response.error.is_none() {
                    response.error = Some(public_error(&error));
                }
            }
            ServiceOutcome::ReportedWithFailure { error, .. } => {
                if response.error.is_none() {
                    response.error = Some(public_error(&error));
                }
            }
        }
    }
    if let Some(connection) = &report.connection {
        response.operation_id.clone_from(&connection.session);
    }
    if response.error.is_none() {
        response.error = doctor_error(&report);
    }
    if response.error.is_none() && !report.healthy {
        response.error = Some(unhealthy());
    }
    response.result = Some(LiveResult::Doctor(report));
    merge_context(&mut response);
    response
}

fn doctor_error(report: &DoctorResult) -> Option<ToolkitError> {
    let failures = report.repair_blocking_errors();
    if failures.is_empty() {
        return None;
    }
    let joined = ChannelError::Joined {
        causes: failures.clone(),
    };
    let mut primary = public_error(&joined.into());
    fn project(error: &ChannelError, causes: &mut Vec<ToolkitError>) {
        match error {
            ChannelError::Joined { causes: children } => {
                for child in children {
                    project(child, causes);
                }
            }
            ChannelError::Toolkit {
                code,
                message,
                exit_code,
                details,
                ..
            } => {
                causes.push(
                    ToolkitError::new(code, message, *exit_code).with_details(details.clone()),
                );
            }
            other => causes.push(public_error(&other.clone().into())),
        }
    }
    let mut causes = Vec::new();
    for error in &failures {
        project(error, &mut causes);
    }
    if causes.len() > 1 {
        let original = primary.clone();
        primary.details = json!({"primary":original,"causes":causes});
    }
    Some(primary)
}

fn project_doctor_checks(report: &DoctorResult) -> Vec<Value> {
    report
        .checks
        .iter()
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
