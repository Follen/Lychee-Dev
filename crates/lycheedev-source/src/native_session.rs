//! Single-thread reusable native analysis. IPC and lifetime ownership reuse the
//! Windows Source mechanisms; the cached analysis never executes studied code.
#[path = "native_session/serialize.rs"]
pub(crate) mod serialize;
use crate::native_research::{QueryRequest, ResearchSession, WorkerRequest, ResearchReply};
use crate::request::SourceContext;
use lycheedev_core::{
    Outcome, Result, ToolkitError, control::OperationControl, storage, workspace,
};
use lycheedev_windows::{
    BrokerJob, Process,
    broker::{BrokerListener, BrokerScope, BrokerStream},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod analysis_process;
mod raw_response;
mod private_raw_value;
mod response_validation;
use raw_response::{OwnerReply,RawResponse};

/// Private CLI entry for the retained analysis child. Public IPC remains owned
/// by `server`; this entry only uses its exact parent's private stdio handles.
pub fn analysis_worker(args: &[String]) -> Result<()> { analysis_process::worker(args) }

const WIRE: &str = "lycheedev.native-source-session.v1";
const REQUEST_BYTES: usize = 768 << 20;
const RESPONSE_BYTES: usize = 32 << 20;
const REPLY_RESERVE: Duration = Duration::from_millis(500);
const OWNER_IDLE: Duration = Duration::from_secs(60);
const OWNER_LIFETIME: Duration = Duration::from_secs(600);
// Rust's bounded terminal reap after the fixed service cutoff. Two seconds is
// consistent with existing guarded-startup cleanup and Go's WaitDelay, but Go
// does not promise a two-second bound for its complete broker shutdown.
const OWNER_RETIRE_LIMIT: Duration = Duration::from_secs(2);
const REQUEST_LIMIT: Duration = Duration::from_secs(90);
const SEMANTIC_BATCH_LIMIT: usize = 256;

#[derive(Default)]
struct SemanticAdmission { batches:usize }
impl SemanticAdmission {
    fn admit(&mut self,requested:usize)->Result<bool> {
        if requested>2{return Err(fault("Native logical request exceeds two semantic batches"));}
        let total=self.batches.checked_add(requested).ok_or_else(||fault("Native semantic admission overflow"))?;
        if total>SEMANTIC_BATCH_LIMIT{return Ok(false);}
        self.batches=total;Ok(true)
    }
    fn exhausted(&self)->bool{self.batches>=SEMANTIC_BATCH_LIMIT}
}


fn request_wire_deadline(now_unix_ms: u64, remaining: Duration) -> u64 {
    now_unix_ms.saturating_add(remaining.min(REQUEST_LIMIT).as_millis() as u64)
}

/// Invalid envelopes are not admitted work and must neither retire the retained
/// analysis nor restart the owner's idle timer. Expired nonzero deadlines are
/// valid envelopes, matching Go's admission followed by caller cancellation.
fn admitted_request_remaining(deadline_unix_ms: u64, now_unix_ms: u64) -> Option<Duration> {
    if deadline_unix_ms == 0
        || deadline_unix_ms > now_unix_ms.saturating_add(REQUEST_LIMIT.as_millis() as u64)
    {
        return None;
    }
    Some(Duration::from_millis(deadline_unix_ms.saturating_sub(now_unix_ms)))
}

/// The idle interval starts after a request finishes. Active work suspends idle,
/// but neither work nor status may extend the fixed owner lifetime.
struct SessionLifetime {
    deadline: Instant,
    normal_expiry: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionEnd {
    Closed,
    IdleExpired,
    LifetimeExpired(Instant),
}
impl SessionLifetime {
    fn new(started: Instant, caller_deadline: Instant) -> Self {
        let owner_deadline = started + OWNER_LIFETIME;
        Self {
            deadline: caller_deadline.min(owner_deadline),
            normal_expiry: caller_deadline >= owner_deadline,
        }
    }
    /// Only the admission loop may classify the owner's fixed lifetime as a
    /// normal end. An active request's deadline and a shorter caller deadline
    /// remain errors; neither is converted by final cleanup.
    fn admission_end(&self, control: &OperationControl) -> Result<Option<SessionEnd>> {
        match control.check() {
            Ok(()) => Ok(None),
            Err(error) if self.normal_expiry && control.deadline() == self.deadline
                && error.code == "command.deadline" => Ok(Some(SessionEnd::LifetimeExpired(self.deadline))),
            Err(error) => Err(error),
        }
    }
    fn idle_deadline(&self, now: Instant) -> Instant {
        self.deadline.min(now + OWNER_IDLE)
    }
    fn handshake_deadline(&self, now: Instant, idle_deadline: Instant) -> Instant {
        self.deadline.min(idle_deadline).min(now + Duration::from_secs(5))
    }
    fn request_deadline(&self, requested: Instant) -> Instant {
        self.deadline.min(requested)
    }
}

/// Only normal owner expiry has a separate terminal reap window. Derive it
/// from the original service cutoff, never from the time cleanup starts; other
/// exits retain the request/idle cleanup deadline already owned by the caller.
fn terminal_retirement_deadline(end: &Result<SessionEnd>, original: Instant) -> Instant {
    match end {
        Ok(SessionEnd::LifetimeExpired(deadline)) => *deadline + OWNER_RETIRE_LIMIT,
        _ => original,
    }
}

/// Cleanup is still checked after normal expiry. Only a successful, explicitly
/// classified lifetime end may consume the expected owner-deadline observation;
/// cancellation and any primary/cleanup failure retain their original cause.
fn finish_session_end(
    result: Result<SessionEnd>,
    cleanup: Result<()>,
    control: &OperationControl,
) -> Result<SessionEnd> {
    let mut result = lycheedev_core::metadata::owned_write::finish_owned(result, cleanup);
    if let Ok(SessionEnd::LifetimeExpired(deadline)) = &result
        && Instant::now() >= *deadline + OWNER_RETIRE_LIMIT
    {
        // The same fixed bound covers disarming, lock release and the final
        // identity-file close too. A late checked close is not timely success.
        result = Err(fault("Native Source owner cleanup deadline exhausted")
            .with_details(json!({"cleanupDeadlineExceeded":true,
                "cleanupBudgetMs":OWNER_RETIRE_LIMIT.as_millis()})));
    }
    let stopped = match (result.as_ref(), control.check()) {
        (Ok(SessionEnd::LifetimeExpired(deadline)), Err(error))
            if control.deadline() == *deadline && error.code == "command.deadline" => Ok(()),
        (_, stopped) => stopped,
    };
    crate::persistence::finish_control(result, stopped)
}

fn session_status(cached_key: Option<&str>, owner_pid: u32) -> Value {
    json!({"state":if cached_key.is_some(){"ready"}else{"idle"},
        "backend":"native", "workerPid":if cached_key.is_some(){owner_pid}else{0},
        "ownerPid":owner_pid, "cachedSnapshot":cached_key})
}
/// The owner exists before it has an analysis worker. Bind readiness to the
/// owner identity, never to workerPid (which is zero in the valid idle state).
fn verify_status_owner(status: &Value, expected_owner: u32) -> Result<()> {
    let owner = u64::from(expected_owner);
    let coherent = match status["state"].as_str() {
        Some("idle" | "retiring") => status["workerPid"].as_u64() == Some(0)
            && status["cachedSnapshot"].is_null(),
        Some("ready") => status["workerPid"].as_u64() == Some(owner)
            && status["cachedSnapshot"].as_str().is_some(),
        _ => false,
    };
    if status["ownerPid"].as_u64() != Some(owner)
        || status["backend"] != "native" || !coherent
    {
        return Err(fault("Native Source status differs from the expected owner"));
    }
    Ok(())
}

/// Project the already-issued wire deadline onto the caller's monotonic clock.
/// Sample Instant before UTC; serialization, startup and queue waits consume the
/// same budget. The caller's original hard deadline is always the upper bound.
fn exchange_deadline(wire_deadline:u64,now_unix_ms:u64,observed:Instant,caller:Instant)->Result<Instant> {
    let remaining=admitted_request_remaining(wire_deadline,now_unix_ms)
        .ok_or_else(||fault("Invalid native Source request deadline"))?;
    Ok(caller.min(observed.checked_add(remaining).ok_or_else(||fault("Native request deadline range"))?))
}

fn reply_reserve(deadline: Instant) -> Duration {
    REPLY_RESERVE.min(deadline.saturating_duration_since(Instant::now()) / 4)
}

// Use one receive-time reserve throughout the request. The client's existing
// response cutoff uses half this reserve; retirement ends earlier, leaving a
// quarter for observer join/serialization/transport before that caller cutoff.
fn request_cutoffs(total: Instant, received: Instant) -> (Instant, Instant) {
    let reserve = REPLY_RESERVE.min(total.saturating_duration_since(received) / 4);
    (total.checked_sub(reserve).unwrap_or(received),
     total.checked_sub(reserve * 3 / 4).unwrap_or(received))
}

#[derive(Serialize, Deserialize)]
struct Hello {
    wire: String,
    executable: String,
    pid: u32,
    created: u64,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Status,
    Close,
    Query,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAction {
    Status,
    Close,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    wire: String,
    deadline_unix_ms: u64,
    action: Action,
    work: Option<WorkerRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cached: Option<CachedQuery>,
}
// Inspect only the bounded envelope in the IPC owner. Full typed Snapshot
// decoding and its potentially expensive destruction belong to the child.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestHead {
    wire: String,
    deadline_unix_ms: u64,
    action: Action,
    work: Option<serde::de::IgnoredAny>,
    cached: Option<CachedQuery>,
}
#[derive(Serialize, Deserialize)]
struct CachedQuery {
    key: String,
    query: QueryRequest,
}
#[derive(Serialize, Deserialize)]
struct Response {
    value: Option<Value>,
    error: Option<ToolkitError>,
    // Internal transport evidence only. Never inserted into the public result
    // or error; the authenticated CLI may emit it on its diagnostic stderr.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terminal: Option<analysis_process::AnalysisTerminal>,
}
// Only the analysis writer uses typed payloads. Existing Value decoding and
// owner raw-byte forwarding keep their original public/private wire contract.
#[derive(Serialize)]
struct ResponseWrite<T> {
    value: Option<T>,
    error: Option<ToolkitError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal: Option<analysis_process::AnalysisTerminal>,
}
struct Identity {
    path: PathBuf,
    digest: String,
    _file: File,
}
struct Cached {
    key: String,
    analysis: ResearchSession,
    admission: SemanticAdmission,
}
fn execute_cached_query(cached:&mut Option<Cached>,work:Option<WorkerRequest>,reuse:Option<CachedQuery>,control:&OperationControl,owner_pid:u32,record:impl Fn(&str,Option<&ToolkitError>))->Result<ResearchReply> {
    let result = (|| -> Result<ResearchReply> {
        control.check()?;
        let identity_only = reuse.is_some();
        let mut retained_snapshot=None;
        let (query, mut hit) = if let Some(reuse) = reuse {
            if cached.as_ref().is_none_or(|value| value.key != reuse.key) {
                // This handshake does not prepare or run a query.
                // The caller sends the full input after a miss.
                return Err(ToolkitError::new(
                    "source.native_snapshot_missing",
                    "Native Source snapshot is not cached",
                    3,
                ));
            }
            (reuse.query, true)
        } else {
            let WorkerRequest { snapshot, query } =
                work.ok_or_else(|| fault("Native query request missing"))?;
            // Only the independently hashed received snapshot can
            // establish a cache entry; a caller key never does.
            let key = snapshot.cache_key(&control)?;
            let hit = cached.as_ref().is_some_and(|value| value.key == key);
            if !hit {
                *cached = None;
                record("preparing", None);
                *cached = Some(Cached {
                    key,
                    admission:SemanticAdmission::default(),
                    analysis: ResearchSession::prepare_observed(
                        snapshot,
                        &control,
                        |phase| record(phase, None),
                    )?,
                });
            } else {
                retained_snapshot=Some((key,snapshot));
            }
            (query, hit)
        };
        record("planning",None);
        let mut planned=cached.as_mut().expect("prepared Source snapshot").analysis.plan_query(query.clone(),&control)?;
        let requested=planned.batch_count();
        if !cached.as_mut().expect("prepared Source snapshot").admission.admit(requested)? {
            // No semantic operation has run. Retire the old analysis
            // before asking for material or rebuilding exactly once.
            let spent=planned.planning_steps();drop(planned);drop(cached.take());
            let Some((key,snapshot))=retained_snapshot.take() else {
                return Err(ToolkitError::new("source.native_snapshot_missing","Native Source semantic batch quota retired the cached snapshot",3));
            };
            record("quota_repreparing",None);
            let mut replacement=ResearchSession::prepare_observed(snapshot,&control,|phase|record(phase,None))?;
            planned=replacement.plan_replacement_query(query,spent,&control)?;
            if planned.batch_count()!=requested{return Err(fault("Replacement semantic plan changed its batch count"));}
            let mut admission=SemanticAdmission::default();
            if !admission.admit(requested)?{return Err(fault("Fresh native analysis rejected bounded semantic plan"));}
            *cached=Some(Cached{key,analysis:replacement,admission});hit=false;
        }
        drop(retained_snapshot);
        let admitted=cached.as_ref().expect("admitted snapshot").admission.batches;
        record("querying", None);
        // Charge attempted work before execution. Partial or failed
        // completion cannot keep a hot worker alive indefinitely.
        let mut value=cached.as_mut().expect("admitted snapshot").analysis.execute_planned_observed(planned,&control,|phase|record(phase,None))?;
        value.fields_mut()["analysis"]["cacheHit"] = json!(hit);
        value.fields_mut()["nativeSession"] = json!({"workerPid":owner_pid,"cacheHit":hit,"snapshotTransfer":if identity_only{"identity_only"}else{"full"},"batches":admitted,"admittedSemanticBatches":requested,"analysisRetired":admitted>=SEMANTIC_BATCH_LIMIT});
        control.check()?;
        Ok(value)
    })();
    if result.is_err()||cached.as_ref().is_some_and(|value|value.admission.exhausted()){
        record("retiring_analysis",result.as_ref().err());
        *cached=None;
        record("analysis_retired",result.as_ref().err());
    }
    result
}

#[derive(Serialize, Deserialize)]
struct StartupIntent {
    identity: String,
    parent_pid: u32,
    parent_created: u64,
    expires_unix_ms: u64,
}

fn fault(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new("source.native_session", message, 5)
}
fn platform(message: String) -> ToolkitError {
    ToolkitError::new("source.native_session_unavailable", message, 3)
}
fn utc_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| fault(e.to_string()))?
        .as_millis() as u64)
}
fn identity(control: &OperationControl) -> Result<Identity> {
    let path = std::env::current_exe()?.canonicalize()?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(&path)?;
    let hashed = (|| {
        let mut hash = Sha256::new();
        let mut bytes = [0u8; 65536];
        loop {
            control.check()?;
            let n = file.read(&mut bytes)?;
            if n == 0 {
                break;
            }
            hash.update(&bytes[..n]);
        }
        control.check()?;
        Ok(format!("{:x}", hash.finalize()))
    })();
    match hashed {
        Ok(digest) => Ok(Identity {
            path,
            digest,
            _file: file,
        }),
        Err(error) => storage::finish_file(file, Err(error), control),
    }
}
fn write_frame(writer: &mut dyn Write, value: &impl Serialize, maximum: usize) -> Result<()> {
    let bytes = serialize::serialize(value, maximum).map_err(|_| {
        ToolkitError::new(
            "source.native_frame_limit",
            "Source frame exceeds budget",
            3,
        )
    })?;
    write_raw_frame(writer, &bytes)
}
fn write_raw_frame(writer: &mut dyn Write, bytes: &[u8]) -> Result<()> {
    let size = u32::try_from(bytes.len()).map_err(|_| fault("Native frame length overflow"))?;
    writer.write_all(&size.to_be_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}
fn read_frame<T: DeserializeOwned>(reader: &mut dyn Read, maximum: usize) -> Result<T> {
    Ok(serde_json::from_slice(&read_raw_frame(reader, maximum)?)?)
}
fn read_raw_frame(reader: &mut dyn Read, maximum: usize) -> Result<Vec<u8>> {
    let mut size = [0u8; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size > maximum {
        return Err(ToolkitError::new(
            "source.native_frame_limit",
            "Source frame exceeds budget",
            3,
        ));
    }
    let mut bytes = vec![0u8; size];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}
// Cleanup is an observation about the failed exchange, never its replacement.
// Keep unknown exit state distinct from successfully observed retirement.
fn peer_failure(
    mut error: ToolkitError,
    hello: &Hello,
    observed: Option<std::result::Result<u32, String>>,
    retirement: Option<std::result::Result<(), String>>,
) -> ToolkitError {
    let previous = std::mem::take(&mut error.details);
    let mut details = if previous.is_object() { previous } else { json!({}) };
    details["workerPid"] = json!(hello.pid);
    details["workerCreated"] = json!(hello.created);
    details["workerExitCode"] = json!(observed.as_ref().and_then(|result| result.as_ref().ok()).copied());
    details["workerExitWaitError"] = json!(observed.and_then(|result| result.err()));
    details["ownerRetired"] = json!(retirement.as_ref().is_some_and(|result| result.is_ok()));
    details["ownerRetirementAttempted"] = json!(retirement.is_some());
    details["ownerRetirementError"] = json!(retirement.and_then(|result| result.err()));
    error.with_details(details)
}
fn request(inv: &SourceContext, action: Action, work: Option<WorkerRequest>) -> Result<Request> {
    let remaining = inv.control.remaining()?;
    Ok(Request {
        wire: WIRE.into(),
        deadline_unix_ms: request_wire_deadline(utc_ms()?, remaining),
        action,
        work,
        cached: None,
    })
}
fn transport_check(control: &OperationControl) -> Arc<dyn Fn() -> lycheedev_windows::Result<()> + Send + Sync> {
    let control = control.clone();
    Arc::new(move || control.check().map_err(|error|error.to_string()))
}
fn exchange_transport_failure(
    error: ToolkitError,
    control: &OperationControl,
    deadline: Instant,
) -> ToolkitError {
    let stopped = control.check().err().or_else(|| {
        (Instant::now() >= deadline).then(|| ToolkitError::new(
            "command.deadline",
            "Source exchange exceeded its original caller or wire deadline",
            5,
        ))
    });
    match stopped {
        Some(mut stopped) => {
            stopped.details = json!({"originalError":error.code,"originalFailure":error});
            stopped
        }
        None => error,
    }
}

fn exchange(inv: &SourceContext, current: &Identity, request: &Request) -> Result<Option<Value>> {
    inv.control.check()?;
    let observed=Instant::now();
    let deadline=exchange_deadline(request.deadline_unix_ms,utc_ms()?,observed,inv.control.deadline())?;
    let scope = BrokerScope::for_native_source(&inv.home).map_err(platform)?;
    let mut stream =
        match BrokerStream::connect_with_control(&scope, deadline, transport_check(&inv.control)) {
            Ok(stream) => stream,
            Err(message) => {
                let error = exchange_transport_failure(platform(message), &inv.control, deadline);
                if error.code == "source.native_session_unavailable" && error.message.starts_with("broker.not_found:") {
                    return Ok(None);
                }
                return Err(error);
            }
        };
    let result = (|| {
        let hello: Hello = read_frame(&mut stream, 4096)
            .map_err(|error| exchange_transport_failure(error, &inv.control, deadline))?;
        let peer = Process::open(stream.peer_pid().map_err(platform)?).map_err(platform)?;
        let result = (|| {
            if hello.wire != WIRE
                || hello.executable != current.digest
                || peer.identity().pid != hello.pid
                || peer.identity().created != hello.created
                || !peer
                    .image_path()
                    .map_err(platform)?
                    .canonicalize()?
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&current.path.to_string_lossy())
            {
                return Err(fault("Source session belongs to a different CLI owner"));
            }
            let retirement = lycheedev_windows::broker::NativeSourcePeer::open(
                hello.pid,
                hello.created,
                &current.path,
            )
            .map_err(platform)?;
            let result = (|| {
                let closing = matches!(request.action, Action::Close);
                write_frame(&mut stream, request, REQUEST_BYTES)
                    .map_err(|error| exchange_transport_failure(error, &inv.control, deadline))?;
                // Leave retirement time inside the caller's original deadline. An observer
                // timeout is never a caller cancellation or permission to extend its budget.
                let response_deadline = deadline
                    .checked_sub(reply_reserve(deadline) / 2)
                    .unwrap_or_else(Instant::now);
                stream.set_deadline_with_control(response_deadline, transport_check(&inv.control));
                let response: Response = match read_frame(&mut stream, RESPONSE_BYTES) {
                    Ok(response) => response,
                    Err(error)
                        if Instant::now() >= response_deadline
                            || inv.control.check().is_err() =>
                    {
                        let retired = retirement.retire(inv.control.deadline());
                        let primary = inv.control.check().err().unwrap_or_else(|| ToolkitError::new(
                            "command.deadline",
                            "Source request exceeded its original caller or wire deadline",
                            5,
                        ));
                        return Err(peer_failure(
                            primary.with_details(json!({"originalError":error.code,"originalFailure":error})),
                            &hello, None, Some(retired),
                        ));
                    }
                    Err(error) => {
                        inv.control.check()?;
                        let wait = inv
                            .control
                            .deadline()
                            .min(Instant::now() + Duration::from_millis(50));
                        let observed = peer.wait_exit_with_control(wait, || inv.control.check().map_err(|error|error.to_string()));
                        let retired = if observed.is_err() {
                            Some(retirement.retire(inv.control.deadline()))
                        } else {
                            None
                        };
                        return Err(peer_failure(error, &hello, Some(observed), retired));
                    }
                };
                if std::env::var("LYCHEEDEV_SOURCE_PHASE_PROFILE").as_deref() == Ok("1") {
                    if let Some(event) = response.terminal.as_ref().and_then(|terminal|terminal.event(&hello,request.deadline_unix_ms)) {
                        // The IPC owner performs no filesystem or stderr writes. This is
                        // the existing diagnostic-enabled calling CLI, after receiving the
                        // authenticated response and before returning its original error.
                        let _ = writeln!(std::io::stderr().lock(), "{event}");
                    }
                }
                if let Some(error) = response.error {
                    return Err(error);
                }
                if closing {
                    peer.wait_exit_with_control(inv.control.deadline(), || inv.control.check().map_err(|error|error.to_string()))
                        .map_err(platform)?;
                }
                if matches!(request.action, Action::Status) {
                    let status = response.value.as_ref().ok_or_else(|| fault("Native Source status is missing"))?;
                    verify_status_owner(status, hello.pid)?;
                }
                Ok(response.value)
            })();
            lycheedev_core::metadata::owned_write::finish_owned(result, retirement.close().map_err(platform))
        })();
        lycheedev_core::metadata::owned_write::finish_owned(result, peer.close().map_err(platform))
    })();
    lycheedev_core::metadata::owned_write::finish_owned(result, stream.close().map_err(platform))
}

pub fn dispatch(inv: &SourceContext, action: SessionAction) -> Result<Outcome> {
    workspace::open_controlled(&inv.home, &inv.control)?;
    let wire_action = match action {
        SessionAction::Status => Action::Status,
        SessionAction::Close => Action::Close,
    };
    let current = identity(&inv.control)?;
    let result = (|| {
        Ok(Outcome::new(
            exchange(inv, &current, &request(inv, wire_action, None)?)?
                .unwrap_or_else(|| json!({"state":"closed","backend":"native"})),
        ))
    })();
    storage::finish_file(current._file, result, &inv.control)
}
// None authorizes submitting the full input after the cache-only handshake.
fn cached_exchange_result(result: Result<Option<Value>>) -> Result<Option<Value>> {
    match result {
        Err(error) if error.code == "source.native_snapshot_missing" && error.details.is_null() => Ok(None),
        result => result,
    }
}

pub fn analyze(inv: &SourceContext, work: WorkerRequest) -> Result<Value> {
    workspace::open_controlled(&inv.home, &inv.control)?;
    let current = identity(&inv.control)?;
    let result = (|| {
        let mut reuse = request(inv, Action::Query, None)?;
        reuse.cached = Some(CachedQuery {
            key: work.snapshot.cache_key(&inv.control)?,
            query: work.query.clone(),
        });
        if let Some(value) = cached_exchange_result(exchange(inv, &current, &reuse))? {
            return Ok(value);
        }
        let query = request(inv, Action::Query, Some(work))?;
        if let Some(value) = exchange(inv, &current, &query)? {
            return Ok(value);
        }
        let launch = storage::ExclusiveLock::resource_controlled(
            &inv.home.join("locks"),
            "source:native-session-launch",
            &inv.control,
        )?;
        let result = (|| {
            if let Some(value) = exchange(inv, &current, &query)? {
                return Ok(value);
            }
            let parent = Process::open(std::process::id()).map_err(platform)?;
            let parent_created = parent.identity().created;
            parent.close().map_err(platform)?;
            let token = lycheedev_windows::random::token().map_err(platform)?;
            let store_key = format!("source/native-session-start/{token}");
            let intent = StartupIntent {
                identity: startup_identity(inv, &current)?,
                parent_pid: std::process::id(),
                parent_created,
                expires_unix_ms: utc_ms()?.saturating_add(10_000),
            };
            let generation =
                crate::persistence::create_intent(&inv.home, &store_key, &intent, &inv.control)?;
            let args = vec![
                "--source-native-session-server".into(),
                "--home".into(),
                inv.home.canonicalize().map_err(ToolkitError::from).map_err(|error| crate::persistence::retained_intent(error, &store_key, generation))?.into_os_string(),
                "--parent-pid".into(),
                intent.parent_pid.to_string().into(),
                "--parent-created".into(),
                intent.parent_created.to_string().into(),
                "--start-token".into(),
                token.clone().into(),
            ];
            let spawned = lycheedev_windows::detached::spawn_hidden_no_inherit(
                &current.path,
                &args,
                &inv.cwd.canonicalize().map_err(ToolkitError::from).map_err(|error| crate::persistence::retained_intent(error, &store_key, generation))?,
                &[("LYCHEEDEV_NATIVE_SOURCE_START".into(), token.into())],
            );
            let mut child = match spawned {
                Ok(child) => child,
                Err(message) => {
                    return lycheedev_core::metadata::owned_write::finish_owned(
                        Err(platform(message)),
                        remove_startup_intent(inv, &store_key, generation, &intent),
                    );
                }
            };
            let ready = (|| -> Result<()> {
                loop {
                    inv.control.check()?;
                    if child.try_wait().map_err(platform)?.is_some() {
                        return Err(fault("Native Source owner exited during startup"));
                    }
                    if let Some(status) =
                        exchange(inv, &current, &request(inv, Action::Status, None)?)?
                    {
                        verify_status_owner(&status, child.id())?;
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            })();
            if let Err(error) = ready {
                let retired = child
                    .terminate_and_close(inv.control.deadline())
                    .map_err(platform);
                let failed = lycheedev_core::metadata::owned_write::finish_owned(
                    Err(error),
                    retired.map(|_| ()),
                );
                return lycheedev_core::metadata::owned_write::finish_owned(
                    failed,
                    remove_startup_intent(inv, &store_key, generation, &intent),
                );
            }
            child.handoff().map_err(platform)?;
            exchange(inv, &current, &query)?
                .ok_or_else(|| fault("Source owner disappeared after startup"))
        })();
        crate::persistence::finish_control(
            lycheedev_core::metadata::owned_write::finish_owned(result, launch.close()),
            inv.control.check(),
        )
    })();
    storage::finish_file(current._file, result, &inv.control)
}

fn remove_startup_intent(
    inv: &SourceContext,
    key: &str,
    generation: i64,
    expected: &StartupIntent,
) -> Result<()> {
    let result = crate::persistence::consume(
        &inv.home,
        key,
        Some(generation),
        &inv.control,
        |current: StartupIntent| {
            if current.identity != expected.identity
                || current.parent_pid != expected.parent_pid
                || current.parent_created != expected.parent_created
                || current.expires_unix_ms != expected.expires_unix_ms
            {
                return Err(fault("Native Source startup intent changed during launch"));
            }
            Ok(())
        },
    );
    match result {
        Err(error) if error.code == "vault.record_missing" && error.details.is_null() => Ok(()),
        Err(mut error) => {
            error.details = json!({"schema":"lycheedev.source-persistence.v1","key":key,"generation":generation,"mutation":"consume","state":"unconfirmed","cause":error.clone()});
            Err(error)
        }
        Ok(()) => Ok(()),
    }
}

fn startup_identity(inv: &SourceContext, current: &Identity) -> Result<String> {
    let scope = BrokerScope::for_native_source(&inv.home).map_err(platform)?;
    Ok(storage::sha256(&serde_json::to_vec(&(
        WIRE,
        scope.endpoint(),
        &current.path,
        &current.digest,
    ))?))
}

#[derive(Debug)]
pub struct SessionStartup {
    pub home: PathBuf,
    pub parent_pid: u32,
    pub parent_created: u64,
    pub start_token: String,
}

/// The single-use intent binds this launch to its actual parent process,
/// this executable and the selected same-logon workspace.
pub fn server(startup: SessionStartup) -> Result<()> {
    let SessionStartup {
        home,
        parent_pid: pid,
        parent_created: created,
        start_token: token,
    } = startup;
    if !lycheedev_core::selection::valid_digest(&token, 32)
        || std::env::var("LYCHEEDEV_NATIVE_SOURCE_START").ok().as_ref() != Some(&token)
    {
        return Err(fault("Native Source startup token differs"));
    }
    let control = OperationControl::uncancelable(Instant::now() + OWNER_LIFETIME);
    let cancellation = control.tracked_cancel_handle()?;
    let _console =
        lycheedev_windows::cancellation::ConsoleCancellation::install_callback(move || {
            cancellation.cancel()
        })
        .map_err(platform)?;
    let context = SourceContext {
        home: home.canonicalize()?,
        cwd: std::env::current_dir()?,
        control,
    };
    let current = identity(&context.control)?;
    let startup = (|| {
        let own = Process::open(std::process::id()).map_err(platform)?;
        let authenticated = (|| {
            let parent = Process::open(pid).map_err(platform)?;
            let result = (|| {
                if own.parent_pid().map_err(platform)? != pid
                    || parent.identity().created != created
                    || parent.image_path().map_err(platform)?.canonicalize()? != current.path
                {
                    return Err(fault("Native Source actual launcher differs"));
                }
                parent.verify().map_err(platform)
            })();
            lycheedev_core::metadata::owned_write::finish_owned(result, parent.close().map_err(platform))
        })();
        lycheedev_core::metadata::owned_write::finish_owned(authenticated, own.close().map_err(platform))?;
        let expected = startup_identity(&context, &current)?;
        let now = utc_ms()?;
        crate::persistence::consume::<StartupIntent, _>(
            &context.home,
            &format!("source/native-session-start/{token}"),
            None,
            &context.control,
            |intent| {
                if intent.identity != expected
                    || intent.parent_pid != pid
                    || intent.parent_created != created
                    || intent.expires_unix_ms <= now
                    || intent.expires_unix_ms > now.saturating_add(10_000)
                {
                    return Err(fault("Native Source startup intent differs or expired"));
                }
                Ok(())
            },
        )
    })();
    if let Err(error) = startup {
        return storage::finish_file(current._file, Err(error), &context.control);
    }
    // This is the same lifetime fixed before authentication/startup above, not
    // a fresh 600-second interval after startup finishes.
    let lifetime = SessionLifetime {
        deadline: context.control.deadline(),
        normal_expiry: true,
    };
    serve_owned(&context, current, lifetime)
}

pub fn serve(inv: &SourceContext) -> Result<()> {
    let current = identity(&inv.control)?;
    let lifetime = SessionLifetime::new(Instant::now(), inv.control.deadline());
    serve_owned(inv, current, lifetime)
}
fn serve_owned(inv: &SourceContext, current: Identity, lifetime: SessionLifetime) -> Result<()> {
    let result = serve_borrowed(inv, &current, &lifetime);
    finish_session_file(current._file, result, &inv.control)
}
fn finish_session_file(file: File, result: Result<SessionEnd>, control: &OperationControl) -> Result<()> {
    if matches!(&result, Ok(SessionEnd::LifetimeExpired(_))) {
        // A checked close is mandatory even though the expected lifetime has
        // ended. Do not manufacture a new control/deadline for this close.
        let closed = lycheedev_windows::filesystem::close_file(file).map_err(ToolkitError::io);
        finish_session_end(result, closed, control).map(|_| ())
    } else {
        storage::finish_file(file, result.map(|_| ()), control)
    }
}
fn finish_query_observation(
    result: Result<Option<OwnerReply>>,
    observed: Result<()>,
) -> Result<Option<OwnerReply>> {
    match (result, observed) {
        (Ok(Some(OwnerReply::ChildError { response, retirement, terminal })), Err(observation)) => {
            Ok(Some(OwnerReply::ChildError {
                response: response.with_observation(observation),
                retirement,
                terminal,
            }))
        }
        (result, observed) => lycheedev_core::metadata::owned_write::finish_owned(result, observed),
    }
}

fn write_query_observation(
    result: Result<Option<OwnerReply>>,
    joined: std::thread::Result<Result<()>>,
    terminal: Option<analysis_process::AnalysisTerminal>,
    publish: impl FnOnce(&OwnerReply) -> Result<()>,
) -> Result<Option<()>> {
    let write = |result: Result<Option<OwnerReply>>| {
        let response = match result {
            Ok(Some(response)) => response,
            // A rejected envelope did not touch the retained child cache and
            // still ends this connection without publishing a replacement.
            Ok(None) => return Ok(None),
            Err(error) => OwnerReply::Local(Response { value: None, error: Some(error), terminal }),
        };
        publish(&response).map(Some)
    };
    match joined {
        Ok(observed) => write(finish_query_observation(result, observed)),
        Err(_) => {
            let fatal = fault("Source request observer failed");
            let published = write(finish_query_observation(result, Err(fatal.clone()))).map(|_| ());
            // A reply attempt must not turn a panicked observer into a warm
            // owner. Retain the original fatal cause even if control/write
            // fails, then unwind through the existing server cleanup chain.
            lycheedev_core::metadata::owned_write::finish_owned(Err(fatal), published)
        }
    }
}

fn observe_request_disconnect(
    reader: &mut impl Read,
    completed: &AtomicBool,
    cancellation: &lycheedev_core::control::TrackedCancellation,
) -> Result<()> {
    let mut marker = [0u8; 1];
    match reader.read(&mut marker) {
        Ok(0) if !completed.load(Ordering::Acquire) => cancellation.cancel(),
        Ok(_) if marker[0] == 1 && !completed.load(Ordering::Acquire) => cancellation.cancel(),
        Ok(_) => {}
        // These exact scope stops do not establish a peer disconnect. A joined
        // checked-close error is a distinct failure and must reach the owner.
        Err(error) if matches!(error.to_string().as_str(), "broker.observation_stopped" | "broker.deadline") => {}
        Err(error) => {
            if !completed.load(Ordering::Acquire) { cancellation.cancel(); }
            return Err(error.into());
        }
    }
    Ok(())
}

fn clean_accept_deadline(message: &str, control: &OperationControl) -> bool {
    message == "broker.deadline" || control.check().err().is_some_and(|error| {
        error.code == "command.deadline" && message == error.to_string()
    })
}

fn serve_borrowed(inv: &SourceContext, current: &Identity, lifetime: &SessionLifetime) -> Result<SessionEnd> {
    let owner_control = inv.control.ordered_child(lifetime.deadline)?;
    workspace::open_controlled(&inv.home, &inv.control)?;
    let scope = BrokerScope::for_native_source(&inv.home).map_err(platform)?;
    let owner = storage::ExclusiveLock::resource_controlled(
        &inv.home.join("locks"),
        "source:native-session-owner",
        &owner_control,
    )?;
    let owned = (|| {
        let job = BrokerJob::own_current_process(2 << 30).map_err(platform)?;
        let own = match Process::open(std::process::id()) {
            Ok(own) => own,
            Err(error) => return lycheedev_core::metadata::owned_write::finish_owned(
                Err(platform(error)), job.close().map_err(platform),
            ),
        };
        let result = (|| {
            let mut listener = match BrokerListener::bind(&scope) {
                Ok(listener) => listener,
                Err(error) => return lycheedev_core::metadata::owned_write::finish_owned(
                    Err(platform(error)), job.close().map_err(platform),
                ),
            };
            let mut analysis: Option<analysis_process::AnalysisProcess> = None;
            let mut terminal_deadline = lifetime.deadline;
            let run = (|| -> Result<SessionEnd> {
                let mut idle_deadline = lifetime.idle_deadline(Instant::now());
                loop {
                    // Only an admitted request may restart idle. Failed handshakes and
                    // malformed envelopes retain this absolute idle boundary.
                    // Idle expiry stops admission at the original idle boundary.
                    // Idle cleanup retains the original owner boundary. Normal
                    // lifetime expiry alone has a separately bounded terminal reap.
                    terminal_deadline = lifetime.deadline;
                    if let Some(end) = lifetime.admission_end(&owner_control)? {
                        return Ok(end);
                    }
                    let mut stream = match listener.accept_with_control(idle_deadline, transport_check(&owner_control)) {
                        Ok(stream) => stream,
                        Err(message) if clean_accept_deadline(&message, &owner_control) => {
                            return Ok(lifetime.admission_end(&owner_control)?.unwrap_or(SessionEnd::IdleExpired));
                        }
                        Err(message) => return Err(platform(message)),
                    };
                    let connected = (|| -> Result<Option<SessionEnd>> {
                        if let Some(end) = lifetime.admission_end(&owner_control)? {
                            return Ok(Some(end));
                        }
                        stream.set_deadline_with_control(
                            lifetime.handshake_deadline(Instant::now(), idle_deadline),
                            transport_check(&owner_control),
                        );
                        if write_frame(
                            &mut stream,
                            &Hello {
                                wire: WIRE.into(),
                                executable: current.digest.clone(),
                                pid: own.identity().pid,
                                created: own.identity().created,
                            },
                            4096,
                        )
                        .is_err()
                        {
                            return Ok(None);
                        }
                        let raw = match read_raw_frame(&mut stream, REQUEST_BYTES) {
                            Ok(raw) => raw,
                            Err(_) => return Ok(None),
                        };
                        let observed = Instant::now();
                        let received_unix_ms = utc_ms()?;
                        let request: RequestHead = match serde_json::from_slice(&raw) {
                            Ok(request) => request,
                            Err(_) => return Ok(None),
                        };
                        // Status/close never enter the child, so retain their complete
                        // historical typed validation here, including unused payloads.
                        if !matches!(request.action, Action::Query)
                            && serde_json::from_slice::<Request>(&raw).is_err() { return Ok(None); }
                        if request.wire != WIRE {
                            return Ok(None);
                        }
                        // Decoding/validation consumes service time too. A connected
                        // peer cannot turn the terminal cleanup window into admission.
                        if let Some(end) = lifetime.admission_end(&owner_control)? {
                            return Ok(Some(end));
                        }
                        let Some(remaining) =
                            admitted_request_remaining(request.deadline_unix_ms, utc_ms()?)
                        else {
                            return Ok(None);
                        };
                        if remaining.is_zero() {
                            // Go dequeues an already-cancelled task without touching its
                            // worker, then resets idle. Its expired transport cannot reply.
                            // A skipped work member is not proof of a typed request.
                            // Preserve the old expired-envelope validation/no-admission
                            // rule; this path neither starts nor touches analysis.
                            if serde_json::from_slice::<Request>(&raw).is_ok() {
                                idle_deadline = lifetime.idle_deadline(Instant::now());
                            }
                            return Ok(None);
                        }
                        let received_remaining = Duration::from_millis(request.deadline_unix_ms.saturating_sub(received_unix_ms));
                        let total_deadline = observed.checked_add(received_remaining)
                            .ok_or_else(|| fault("Native request deadline range"))?;
                        let total_deadline = lifetime.request_deadline(total_deadline);
                        terminal_deadline = total_deadline;
                        let (processing_deadline, retirement_deadline) = request_cutoffs(total_deadline, observed);
                        let response_control =
                            owner_control.ordered_child(total_deadline)?;
                        stream.set_deadline_with_control(total_deadline, transport_check(&response_control));
                        match request.action {
                            Action::Status => {
                                analysis_process::refresh(&mut analysis, retirement_deadline)?;
                                write_frame(
                                &mut stream,
                                &Response {
                                    value: Some({
                                        let mut status = session_status(
                                            analysis.as_ref().and_then(|value| value.key()),
                                            own.identity().pid,
                                        );
                                        status["batches"] = json!(
                                            analysis.as_ref().map_or(0, |value| value.batches())
                                        );
                                        if analysis.as_ref().is_some_and(|value| value.retiring()) {
                                            status["state"] = json!("retiring");
                                        }
                                        status
                                    }),
                                    error: None,
                                    terminal: None,
                                },
                                RESPONSE_BYTES,
                            )?;
                            }
                            Action::Close => {
                                drop(raw);
                                let retired = analysis_process::retire_slot(&mut analysis, retirement_deadline);
                                if let Err(error) = retired {
                                    write_frame(&mut stream, &Response { value:None, error:Some(error), terminal:None }, RESPONSE_BYTES)?;
                                    idle_deadline = lifetime.idle_deadline(Instant::now());
                                    return Ok(None);
                                }
                                write_frame(
                                    &mut stream,
                                    &Response {
                                        value: Some(json!({"state":"closed","backend":"native"})),
                                        error: None,
                                        terminal: None,
                                    },
                                    RESPONSE_BYTES,
                                )?;
                                return Ok(Some(SessionEnd::Closed));
                            }
                            Action::Query => {
                                let control = owner_control.ordered_child(processing_deadline)?;
                                let observer_stop = Arc::new(AtomicBool::new(false));
                                let request_cancel = control.tracked_cancel_handle()?;
                                let mut observer = stream.try_clone().map_err(platform)?;
                                // The observation read shares the processing cutoff but
                                // has its own stop: a completed query releases the
                                // blocked read without claiming the request cancelled.
                                // A peer EOF cancels this request's local signal only.
                                let read_cancel = lycheedev_core::control::TrackedCancellation::new();
                                let observer_check: Arc<
                                    dyn Fn() -> lycheedev_windows::Result<()> + Send + Sync,
                                > = {
                                    let read_cancel = read_cancel.clone();
                                    Arc::new(move || {
                                        if read_cancel.is_cancelled() {
                                            Err("broker.observation_stopped".into())
                                        } else {
                                            Ok(())
                                        }
                                    })
                                };
                                // Observation is needed only while analysis can run.
                                // Its original processing cutoff also bounds the final
                                // join; a short request must not wait through its reply
                                // reserve for the broker's cancellation polling slice.
                                observer.set_deadline_with_control(processing_deadline, observer_check);
                                let observing = observer_stop.clone();
                                let handle = std::thread::spawn(move || {
                                    let observed = observe_request_disconnect(&mut observer, &observing, &request_cancel);
                                    lycheedev_core::metadata::owned_write::finish_owned(
                                        observed, observer.close().map_err(platform),
                                    )
                                });
                                let mut terminal = None;
                                let result = analysis_process::query(
                                    &mut analysis, raw, request.work.is_none().then_some(request.cached.as_ref()).flatten().map(|value|value.key.as_str()),
                                    current, &inv.home, own.identity(), &control, retirement_deadline,
                                    &mut terminal,
                                );
                                observer_stop.store(true, Ordering::Release);
                                read_cancel.cancel();
                                // Reply on the original transport budget, not the expired or
                                // cancelled processing token that produced the typed error.
                                if write_query_observation(result, handle.join(), terminal, |response| {
                                    response_control.check()?;
                                    response.write(&mut stream)
                                })?.is_none() {
                                    return Ok(None);
                                }
                                // No optional owner-side filesystem diagnostics here,
                                // including after the reply: a synchronous write/fsync
                                // would also block admission of the next short request.
                                // The killable analysis child retains phase diagnostics;
                                // only this authenticated reply establishes the outcome.
                            }
                        }
                        // Active work suspends idle, but cannot extend owner lifetime.
                        idle_deadline = lifetime.idle_deadline(Instant::now());
                        Ok(None)
                    })();
                    let connected = lycheedev_core::metadata::owned_write::finish_owned(
                        connected, stream.close().map_err(platform),
                    )?;
                    if let Some(end) = connected { return Ok(end); }
                }
            })();
            // Retire analysis before releasing the process ownership boundary on every
            // exit, including owner expiry, transport failure and cancellation.
            let retirement_deadline = terminal_retirement_deadline(&run, terminal_deadline);
            let retired = analysis_process::retire_slot(&mut analysis, retirement_deadline);
            let closed = if retired.is_ok() {
                job.close().map_err(platform)
            } else {
                // Retain the armed aggregate Job until OS process exit. Do not
                // disarm a tree whose actual retirement remains unconfirmed.
                std::mem::forget(job);
                Ok(())
            };
            let result = lycheedev_core::metadata::owned_write::finish_owned(
                lycheedev_core::metadata::owned_write::finish_owned(run, retired), closed);
            lycheedev_core::metadata::owned_write::finish_owned(result, listener.close().map_err(platform))
        })();
        lycheedev_core::metadata::owned_write::finish_owned(result, own.close().map_err(platform))
    })();
    finish_session_end(owned, owner.close(), &owner_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum HelloStop { Cancel, Deadline, Eof }

    // A real pipe accepts the public Status client but publishes no Hello.
    // Channels establish the cancellation point without a production delay hook.
    fn initial_hello_failure(stop: HelloStop) -> ToolkitError {
        let home = std::env::temp_dir().join(format!(
            "lycheedev-source-hello-{}-{}", std::process::id(),
            lycheedev_windows::random::token().unwrap(),
        ));
        workspace::initialize(&home).unwrap();
        let scope = BrokerScope::for_native_source(&home).unwrap();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        let (accepted_sender, accepted_receiver) = std::sync::mpsc::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || -> Result<()> {
            let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(15));
            let stream = match listener.accept_with_control(control.deadline(), transport_check(&control)) {
                Ok(stream) => stream,
                Err(error) => return lycheedev_core::metadata::owned_write::finish_owned(
                    Err(platform(error)), listener.close().map_err(platform),
                ),
            };
            accepted_sender.send(()).unwrap();
            if !matches!(stop, HelloStop::Eof) {
                release_receiver.recv_timeout(Duration::from_secs(15)).unwrap();
            }
            let closed = stream.close().map_err(platform);
            lycheedev_core::metadata::owned_write::finish_owned(closed, listener.close().map_err(platform))
        });
        let duration = if matches!(stop, HelloStop::Deadline) { 3 } else { 10 };
        let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(duration));
        let cancellation = control.tracked_cancel_handle().unwrap();
        let ctx = SourceContext { home: home.clone(), cwd: home.clone(), control };
        let client = std::thread::spawn(move || dispatch(&ctx, SessionAction::Status));
        let accepted = accepted_receiver.recv_timeout(Duration::from_secs(12));
        if accepted.is_ok() && matches!(stop, HelloStop::Cancel) { cancellation.cancel(); }
        let result = client.join();
        let _ = release_sender.send(());
        let server = server.join();
        std::fs::remove_dir_all(&home).unwrap();
        accepted.expect("the real client must reach initial Hello before the stop");
        server.unwrap().unwrap();
        result.unwrap().unwrap_err()
    }

    struct FailedObserverRead {
        marker: Option<u8>,
        cause: &'static str,
        calls: usize,
    }
    impl Read for FailedObserverRead {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if let Some(marker) = self.marker { bytes[0] = marker; }
            Err(std::io::Error::other(self.cause))
        }
    }

    #[test]
    fn observer_checked_read_failure_preserves_real_marker_and_error_result() {
        // The fixture models bytes followed by a checked-close error or an
        // independent I/O failure. Real Windows close injection stays in broker.
        for cause in ["broker.handle_close: fixture", "fixture independent observer read failure"] {
            for marker in [Some(1), None] {
                for completed in [false, true] {
                    let mut reader = FailedObserverRead { marker, cause, calls: 0 };
                    let control = OperationControl::default();
                    let cancellation = control.tracked_cancel_handle().unwrap();
                    let result = observe_request_disconnect(&mut reader, &AtomicBool::new(completed), &cancellation);
                    assert_eq!(reader.calls, 1);
                    let error = result.expect_err("read failure must reach the existing observer join result");
                    assert_eq!(error.code, "external.io");
                    assert_eq!(error.message, cause);
                    assert_eq!(cancellation.is_cancelled(), !completed, "{cause}, marker={marker:?}");
                }
            }
        }
    }

    #[test]
    fn observer_scope_stop_and_deadline_do_not_cancel_active_work() {
        for cause in ["broker.observation_stopped", "broker.deadline"] {
            let mut reader = FailedObserverRead { marker: None, cause, calls: 0 };
            let control = OperationControl::default();
            let cancellation = control.tracked_cancel_handle().unwrap();
            observe_request_disconnect(&mut reader, &AtomicBool::new(false), &cancellation).unwrap();
            assert_eq!(reader.calls, 1);
            assert!(!cancellation.is_cancelled(), "{cause}");
        }
    }

    #[test]
    fn observer_active_marker_and_eof_still_cancel_but_completed_scope_does_not() {
        for bytes in [vec![1], vec![]] {
            for completed in [false, true] {
                let control = OperationControl::default();
                let cancellation = control.tracked_cancel_handle().unwrap();
                observe_request_disconnect(&mut std::io::Cursor::new(bytes.clone()), &AtomicBool::new(completed), &cancellation).unwrap();
                assert_eq!(cancellation.is_cancelled(), !completed);
            }
        }
    }

    #[test]
    fn listener_clean_idle_deadline_does_not_swallow_checked_close_failure() {
        let control = OperationControl::default();
        assert!(clean_accept_deadline("broker.deadline", &control));
        assert!(!clean_accept_deadline("broker.deadline\nbroker.handle_close: fixture", &control));
        assert!(!clean_accept_deadline("fixture deadline text", &control));
    }

    #[test]
    fn listener_clean_owner_deadline_requires_the_exact_original_typed_error() {
        let control = OperationControl::uncancelable(Instant::now());
        let original = control.check().unwrap_err();
        assert_eq!(original.code, "command.deadline");
        assert!(clean_accept_deadline(&original.to_string(), &control));
        assert!(!clean_accept_deadline(&format!("{}\nbroker.handle_close: fixture", original), &control));
        let cancelled = OperationControl::default();
        cancelled.tracked_cancel_handle().unwrap().cancel();
        assert!(!clean_accept_deadline(&cancelled.check().unwrap_err().to_string(), &cancelled));
    }

    #[test]
    fn initial_hello_cancel_preserves_original_command_control() {
        let error = initial_hello_failure(HelloStop::Cancel);
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.exit_code, 7);
        let primary = if error.details["schema"] == lycheedev_core::storage::FILE_CLEANUP_SCHEMA {
            serde_json::from_value::<ToolkitError>(error.details["primary"].clone()).unwrap()
        } else { error };
        let original: ToolkitError = serde_json::from_value(primary.details["originalFailure"].clone()).unwrap();
        let expected = match original.code.as_str() {
            "source.native_session_unavailable" => ToolkitError::new(
                "source.native_session_unavailable", "command.cancelled: operation cancelled by the caller", 3,
            ),
            "external.io" => ToolkitError::new(
                "external.io", "command.cancelled: operation cancelled by the caller", 5,
            ),
            code => panic!("unexpected original transport stage: {code}"),
        };
        assert_eq!(primary.details["originalError"], expected.code);
        assert_eq!(serde_json::to_value(original).unwrap(), serde_json::to_value(expected).unwrap());
    }

    #[test]
    fn initial_hello_deadline_preserves_original_command_control() {
        let error = initial_hello_failure(HelloStop::Deadline);
        assert_eq!(error.code, "command.deadline");
        assert_eq!(error.exit_code, 5);
        assert!(serde_json::to_string(&error).unwrap().contains("external.io"));
    }

    #[test]
    fn initial_hello_unstopped_eof_keeps_independent_io_failure() {
        let error = initial_hello_failure(HelloStop::Eof);
        assert_eq!(error.code, "external.io");
        assert_eq!(error.exit_code, 5);
    }

    #[test]
    fn cached_miss_recovery_allows_plain_missing_snapshot() {
        let missing = ToolkitError::new(
            "source.native_snapshot_missing", "Native Source snapshot is not cached", 3,
        );
        assert!(missing.details.is_null());
        assert_eq!(cached_exchange_result(Err(missing)).unwrap(), None);
    }

    #[test]
    fn cached_miss_recovery_preserves_checked_close_failure() {
        let missing = ToolkitError::new(
            "source.native_snapshot_missing", "Native Source snapshot is not cached", 3,
        );
        let closed = lycheedev_core::metadata::owned_write::finish_owned::<Option<Value>>(
            Err(missing), Err(platform("fixture checked broker close failure".into())),
        ).unwrap_err();
        assert_eq!(closed.details["schema"], lycheedev_core::storage::FILE_CLEANUP_SCHEMA);
        assert_eq!(closed.details["primary"]["code"], "source.native_snapshot_missing");
        assert_eq!(closed.details["cleanup"]["message"], "fixture checked broker close failure");
        let expected = serde_json::to_value(&closed).unwrap();
        let preserved = cached_exchange_result(Err(closed))
            .expect_err("a checked-close failure must not authorize submitting full work");
        assert_eq!(serde_json::to_value(preserved).unwrap(), expected);
    }

    #[test]
    fn one_receive_time_reserve_orders_execution_retirement_and_client_reply() {
        let received=Instant::now();
        for duration in [Duration::from_millis(1),Duration::from_millis(1000),REQUEST_LIMIT] {
            let total=received+duration;
            let (processing,retirement)=request_cutoffs(total,received);
            let reserve=REPLY_RESERVE.min(duration/4);
            let caller_reply=total-reserve/2;
            assert!(received<=processing&&processing<=retirement);
            assert!(retirement<=caller_reply&&caller_reply<=total);
            assert_eq!(processing,total-reserve);
            assert_eq!(retirement,total-reserve*3/4);
        }
    }
    #[test]
    fn raw_frame_transfer_preserves_exact_bytes_and_existing_cap() {
        let bytes=br#" { "wire": "unchanged", "unicode": "\u4e2d", "n": 1e0 } "#;
        let mut framed=Vec::new();write_raw_frame(&mut framed,bytes).unwrap();
        assert_eq!(read_raw_frame(&mut std::io::Cursor::new(&framed),bytes.len()).unwrap(),bytes);
        assert_eq!(read_raw_frame(&mut std::io::Cursor::new(&framed),bytes.len()-1).unwrap_err().code,"source.native_frame_limit");
    }
    #[test]
    fn envelope_head_does_not_replace_full_typed_admission() {
        let malformed=json!({"wire":WIRE,"deadline_unix_ms":1,"action":"query","work":false});
        assert!(serde_json::from_value::<RequestHead>(malformed.clone()).is_ok());
        assert!(serde_json::from_value::<Request>(malformed).is_err());
        let unknown=json!({"wire":WIRE,"deadline_unix_ms":1,"action":"query","work":null,"extra":true});
        assert!(serde_json::from_value::<RequestHead>(unknown).is_err());
        let bad_cached=json!({"wire":WIRE,"deadline_unix_ms":1,"action":"query","work":null,"cached":{"key":"x","query":false}});
        assert!(serde_json::from_value::<RequestHead>(bad_cached).is_err());
    }
    #[test]
    fn unconfirmed_retirement_is_distinct_from_ready_and_idle() {
        let mut status=session_status(None,42);status["state"]=json!("retiring");
        verify_status_owner(&status,42).unwrap();
        assert_ne!(status["state"],"idle");assert_ne!(status["state"],"ready");
        status["cachedSnapshot"]=json!("stale");assert!(verify_status_owner(&status,42).is_err());
    }

    #[test]
    fn client_exchange_never_waits_past_the_issued_wire_budget() {
        let now=Instant::now();let caller=now+Duration::from_secs(180);
        assert_eq!(exchange_deadline(91_000,11_000,now,caller).unwrap(),now+Duration::from_secs(80));
        assert_eq!(exchange_deadline(91_000,11_000,now,now+Duration::from_secs(20)).unwrap(),now+Duration::from_secs(20));
        assert_eq!(exchange_deadline(91_000,91_001,now,caller).unwrap(),now);
        assert!(exchange_deadline(91_001,1_000,now,caller).is_err());
        assert!(exchange_deadline(0,1_000,now,caller).is_err());
    }

    #[test]
    fn unknown_request_field_is_rejected_by_production_frame_reader() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &json!({
            "wire":WIRE, "deadline_unix_ms":1_000, "action":"status",
            "work":null, "unknown_field":true,
        }), REQUEST_BYTES).unwrap();
        // Rejection occurs before admission and hence before the only admitted
        // request path that resets idle or touches cached analysis.
        let error = read_frame::<Request>(&mut std::io::Cursor::new(wire), REQUEST_BYTES)
            .err().expect("unknown envelope field must not be admitted");
        assert!(error.message.contains("unknown field"));
    }

    #[test]
    fn wire_request_budget_is_bounded_without_extending_shorter_callers() {
        assert_eq!(request_wire_deadline(1_000, Duration::from_secs(180)), 91_000);
        assert_eq!(request_wire_deadline(1_000, Duration::from_millis(250)), 1_250);
        assert_eq!(admitted_request_remaining(91_000, 1_000), Some(REQUEST_LIMIT));
        assert_eq!(admitted_request_remaining(91_001, 1_000), None);
        assert_eq!(admitted_request_remaining(u64::MAX, 1_000), None);
        assert_eq!(admitted_request_remaining(0, 1_000), None);
        assert_eq!(admitted_request_remaining(999, 1_000), Some(Duration::ZERO));
    }

    #[test]
    fn idle_restarts_after_work_but_owner_lifetime_does_not() {
        let start = Instant::now();
        let lifetime = SessionLifetime::new(start, start + Duration::from_secs(900));
        assert_eq!(
            lifetime.idle_deadline(start),
            start + Duration::from_secs(60)
        );
        // Connecting is not admission: a slow handshake at t=59 cannot renew
        // the owner's t=60 idle deadline.
        assert_eq!(
            lifetime.handshake_deadline(start + Duration::from_secs(59), lifetime.idle_deadline(start)),
            start + OWNER_IDLE
        );
        assert_eq!(lifetime.handshake_deadline(start, lifetime.idle_deadline(start)), start + Duration::from_secs(5));
        // A long query is allowed to outlive idle; completing it starts a fresh
        // idle interval. A status request has the same reset behavior.
        let finished = start + Duration::from_secs(120);
        assert_eq!(
            lifetime.idle_deadline(finished),
            start + Duration::from_secs(180)
        );
        assert_eq!(
            lifetime.idle_deadline(start + Duration::from_secs(590)),
            start + OWNER_LIFETIME
        );
        assert_eq!(
            lifetime.request_deadline(start + Duration::from_secs(900)),
            start + OWNER_LIFETIME
        );
        assert_eq!(
            lifetime.handshake_deadline(start + Duration::from_secs(599), start + OWNER_LIFETIME),
            start + OWNER_LIFETIME
        );
    }

    #[test]
    fn shorter_caller_and_request_deadlines_are_preserved() {
        let start = Instant::now();
        let lifetime = SessionLifetime::new(start, start + Duration::from_secs(30));
        assert_eq!(
            lifetime.idle_deadline(start),
            start + Duration::from_secs(30)
        );
        assert_eq!(
            lifetime.request_deadline(start + Duration::from_secs(5)),
            start + Duration::from_secs(5)
        );
        let owner = OperationControl::uncancelable(lifetime.deadline);
        let request = owner.ordered_child(start + Duration::from_secs(100)).unwrap();
        assert_eq!(request.deadline(), lifetime.deadline);
        let check = transport_check(&owner);
        owner.tracked_cancel_handle().unwrap().cancel();
        assert!(check().unwrap_err().contains("command.cancelled"));
        assert_eq!(request.check().unwrap_err().code, "command.cancelled");
    }

    #[test]
    fn fixed_lifetime_expiry_is_a_normal_admission_end_without_a_new_deadline() {
        let now = Instant::now();
        let live = SessionLifetime::new(now, now + Duration::from_secs(900));
        let live_control = OperationControl::uncancelable(live.deadline);
        assert_eq!(live.deadline, now + Duration::from_secs(600));
        assert_eq!(live.admission_end(&live_control).unwrap(), None);
        // Move only the test's origin into the past, not the production 600s
        // lifetime. The real IPC acceptance continues to wait the full 600s.
        let expired = SessionLifetime::new(now - OWNER_LIFETIME, now);
        let control = OperationControl::uncancelable(expired.deadline);
        let end = expired.admission_end(&control).unwrap().unwrap();
        assert_eq!(end, SessionEnd::LifetimeExpired(now));
        assert_eq!(finish_session_end(Ok(end), Ok(()), &control).unwrap(), end);
        assert_eq!(expired.deadline, now);
        assert_eq!(control.deadline(), now);
        assert_eq!(control.check().unwrap_err().code, "command.deadline");
    }

    #[test]
    fn lifetime_reap_window_never_shortens_service_or_renews_a_late_cleanup() {
        let start = Instant::now();
        let lifetime = SessionLifetime::new(start, start + Duration::from_secs(900));
        let service = start + Duration::from_secs(600);
        // Even the last nanosecond remains service time; no 500ms early cutoff.
        let last_request = service - Duration::from_nanos(1);
        assert_eq!(lifetime.request_deadline(last_request), last_request);
        assert_eq!(lifetime.handshake_deadline(last_request, service), service);
        assert_eq!(lifetime.request_deadline(service + OWNER_RETIRE_LIMIT), service);
        let end = Ok(SessionEnd::LifetimeExpired(service));
        let reap = terminal_retirement_deadline(&end, last_request);
        assert_eq!(reap, start + Duration::from_secs(602));
        assert_eq!(terminal_retirement_deadline(&end, reap + Duration::from_secs(60)), reap);
        let control = OperationControl::uncancelable(lifetime.deadline);
        assert_eq!(lifetime.admission_end(&control).unwrap(), None);
        assert_eq!(control.deadline(), service);
        // An already elapsed original cutoff ends admission, even though its
        // independently fixed reap window may still have time remaining.
        let expired_at = Instant::now();
        let expired = SessionLifetime::new(expired_at - OWNER_LIFETIME, expired_at);
        let stopped = OperationControl::uncancelable(expired_at);
        let end = expired.admission_end(&stopped).unwrap().unwrap();
        assert_eq!(end, SessionEnd::LifetimeExpired(expired_at));
        assert_eq!(terminal_retirement_deadline(&Ok(end), expired_at), expired_at + OWNER_RETIRE_LIMIT);
    }

    #[test]
    fn only_normal_lifetime_end_receives_the_fixed_terminal_reap_window() {
        let original = Instant::now();
        for end in [Ok(SessionEnd::Closed), Ok(SessionEnd::IdleExpired),
            Err(ToolkitError::new("command.cancelled", "original cancellation", 7)),
            Err(ToolkitError::new("command.deadline", "original request cutoff", 5)),
            Err(ToolkitError::new("external.io", "original close failure", 5))]
        {
            assert_eq!(terminal_retirement_deadline(&end, original), original);
        }
        // Delayed entry never gets a new now+2s window or a successful terminal.
        let service = original - Duration::from_secs(3);
        let end = SessionEnd::LifetimeExpired(service);
        let control = OperationControl::uncancelable(service);
        assert!(terminal_retirement_deadline(&Ok(end), original) < original);
        let failure = finish_session_end(Ok(end), Ok(()), &control).unwrap_err();
        assert_eq!(failure.code, "source.native_session");
        assert_eq!(failure.details["primary"]["details"]["cleanupDeadlineExceeded"], true);
        assert_eq!(failure.details["primary"]["details"]["cleanupBudgetMs"], 2000);
    }

    #[test]
    fn terminal_cleanup_preserves_primary_cancellation_and_close_errors() {
        let original = Instant::now();
        // The new timestamped stop model needs the live deadline after the
        // cancellation instant so cancellation is the first observed event.
        let cancelled = crate::native::stopped_control(false, original + Duration::from_secs(30));
        let primary = cancelled.check().unwrap_err();
        let cleanup = fault("Native analysis retirement is unconfirmed");
        let failure = finish_session_end(Err(primary), Err(cleanup), &cancelled).unwrap_err();
        assert_eq!(failure.code, "command.cancelled");
        assert_eq!(failure.exit_code, 7);
        assert_eq!(failure.details["primary"]["details"]["primary"]["code"], "command.cancelled");
        assert_eq!(failure.details["primary"]["details"]["cleanup"]["code"], "source.native_session");
        let close = ToolkitError::io(std::io::Error::other("original close failure"));
        let stopped = OperationControl::uncancelable(original);
        let failure = finish_session_end(Err(close), Ok(()), &stopped).unwrap_err();
        assert_eq!(failure.code, "external.io");
        assert_eq!(failure.details["primary"]["message"], "original close failure");
        assert_eq!(failure.details["cleanup"]["code"], "command.deadline");
    }

    #[test]
    fn shorter_caller_expiry_is_not_a_normal_owner_lifetime_end() {
        let now = Instant::now();
        let lifetime = SessionLifetime::new(now - Duration::from_secs(30), now);
        let control = OperationControl::uncancelable(now);
        assert!(!lifetime.normal_expiry);
        assert_eq!(lifetime.deadline, now);
        assert_eq!(lifetime.admission_end(&control).unwrap_err().code, "command.deadline");
        let owner = SessionLifetime::new(now, now + OWNER_LIFETIME);
        // Equal error text from an unrelated shorter request is insufficient.
        assert_eq!(owner.admission_end(&control).unwrap_err().code, "command.deadline");
        // A normal end cannot waive a different outer control's later expiry.
        let earlier_owner = now - Duration::from_secs(1);
        assert_eq!(finish_session_end(Ok(SessionEnd::LifetimeExpired(earlier_owner)), Ok(()), &control)
            .unwrap_err().code, "command.deadline");
    }

    #[test]
    fn normal_lifetime_end_never_hides_cancellation_or_unconfirmed_retirement() {
        let now = Instant::now();
        let lifetime = SessionLifetime::new(now - OWNER_LIFETIME, now);
        let control = OperationControl::uncancelable(now);
        let end = lifetime.admission_end(&control).unwrap().unwrap();
        let cleanup = fault("Native analysis retirement is unconfirmed")
            .with_details(json!({"analysisRetired":false,"rootReaped":false,"jobEmpty":false}));
        let failure = finish_session_end(Ok(end), Err(cleanup), &control).unwrap_err();
        assert_eq!(failure.code, "source.native_session");
        assert_eq!(failure.details["primary"]["details"]["analysisRetired"], false);
        assert_eq!(failure.details["primary"]["details"]["rootReaped"], false);
        assert_eq!(failure.details["cleanup"]["code"], "command.deadline");
        // A cancellation stamped before the fixed cutoff wins over the expected
        // lifetime end. The timestamped model keeps the first observed event;
        // a later cancellation cannot retroactively reorder an expired deadline.
        let cancelled = lycheedev_core::control::TrackedCancellation::new();
        cancelled.cancel();
        let stopped = OperationControl::tracked(now + Duration::from_secs(30), cancelled);
        assert_eq!(
            finish_session_end(Ok(end), Ok(()), &stopped)
                .unwrap_err()
                .code,
            "command.cancelled"
        );
        assert_eq!(
            lifetime.admission_end(&stopped).unwrap_err().code,
            "command.cancelled"
        );
    }

    #[test]
    fn active_request_and_non_lifetime_ends_keep_original_deadline_failure() {
        let control = OperationControl::uncancelable(Instant::now());
        let primary = control.check().unwrap_err();
        let expected = serde_json::to_value(&primary).unwrap();
        let failure = finish_session_end(Err(primary), Ok(()), &control).unwrap_err();
        assert_eq!(serde_json::to_value(&failure).unwrap(), expected);
        for end in [SessionEnd::Closed, SessionEnd::IdleExpired] {
            assert_eq!(finish_session_end(Ok(end), Ok(()), &control).unwrap_err().code, "command.deadline");
        }
        let original = ToolkitError::new("external.io", "failed to fill whole buffer", 5)
            .with_details(json!({"request":"original"}));
        let failure = finish_session_end(Err(original), Ok(()), &control).unwrap_err();
        assert_eq!(failure.code, "external.io");
        assert_eq!(failure.details["primary"]["details"]["request"], "original");
    }

    #[test]
    fn normal_expiry_identity_file_close_is_checked_without_renewing_control() {
        let control = OperationControl::uncancelable(Instant::now());
        let original_deadline = control.deadline();
        // Use an existing regular file and the real checked OS close boundary;
        // no worker, new process, fixture source, or replacement control.
        let file = File::open(std::env::current_exe().unwrap()).unwrap();
        finish_session_file(file, Ok(SessionEnd::LifetimeExpired(original_deadline)), &control).unwrap();
        assert_eq!(control.deadline(), original_deadline);
        assert_eq!(control.check().unwrap_err().code, "command.deadline");
        let close_error = ToolkitError::io(std::io::Error::other("checked close failed"));
        let failure = finish_session_end(Ok(SessionEnd::LifetimeExpired(original_deadline)), Err(close_error), &control).unwrap_err();
        assert_eq!(failure.code, "external.io");
        assert_eq!(failure.details["primary"]["message"], "checked close failed");
        let cancelled = crate::native::stopped_control(false, original_deadline + Duration::from_secs(30));
        let file = File::open(std::env::current_exe().unwrap()).unwrap();
        assert_eq!(finish_session_file(file, Ok(SessionEnd::LifetimeExpired(original_deadline)), &cancelled).unwrap_err().code, "command.cancelled");
    }

    #[test]
    fn startup_status_frame_accepts_idle_owner_and_rejects_worker_pid_substitution() {
        // Exercise the same frame codec and validator as exchange/startup. A
        // new owner has no prepared analysis until the first query is sent.
        let mut wire = Vec::new();
        write_frame(&mut wire, &Response {
            value: Some(session_status(None, 42)), error: None, terminal:None,
        }, RESPONSE_BYTES).unwrap();
        let response: Response = read_frame(&mut std::io::Cursor::new(wire), RESPONSE_BYTES).unwrap();
        let status = response.value.unwrap();
        assert_eq!(status["workerPid"], 0);
        verify_status_owner(&status, 42).unwrap();
        assert!(verify_status_owner(&status, 43).is_err());
        let mut substituted = session_status(Some("snapshot"), 42);
        substituted["ownerPid"] = json!(43);
        assert!(verify_status_owner(&substituted, 42).is_err());
        substituted.as_object_mut().unwrap().remove("ownerPid");
        assert!(verify_status_owner(&substituted, 42).is_err());
        let mut malformed = status;
        malformed["workerPid"] = json!(42);
        assert!(verify_status_owner(&malformed, 42).is_err());
        verify_status_owner(&session_status(Some("snapshot"), 42), 42).unwrap();
    }

    #[test]
    fn retained_owner_without_analysis_is_idle() {
        let idle = session_status(None, 42);
        assert_eq!(idle["state"], "idle");
        assert_eq!(idle["workerPid"], 0);
        assert_eq!(idle["ownerPid"], 42);
        assert!(idle["cachedSnapshot"].is_null());
        let ready = session_status(Some("authenticated-snapshot"), 42);
        assert_eq!(ready["state"], "ready");
        assert_eq!(ready["workerPid"], 42);
        assert_eq!(ready["cachedSnapshot"], "authenticated-snapshot");
    }

    #[test]
    fn truncated_response_keeps_primary_failure_when_retirement_is_denied() {
        // Exercise the production frame reader: the advertised body is absent.
        let mut truncated = std::io::Cursor::new(8u32.to_be_bytes());
        let primary = read_frame::<Response>(&mut truncated, RESPONSE_BYTES).err().expect("truncated frame");
        assert_eq!(primary.code, "external.io");
        let hello = Hello { wire: WIRE.into(), executable: "test".into(), pid: 42, created: 123 };
        let denied = "拒绝访问。 (0x80070005)";
        let error = peer_failure(primary.clone(), &hello, Some(Err("command.deadline: process exit wait".into())), Some(Err(denied.into())));
        assert_eq!(error.code, primary.code);
        assert_eq!(error.message, primary.message);
        assert_eq!(error.exit_code, primary.exit_code);
        assert_eq!(error.details["workerPid"], 42);
        assert!(error.details["workerExitCode"].is_null());
        assert_eq!(error.details["workerExitWaitError"], "command.deadline: process exit wait");
        assert_eq!(error.details["ownerRetirementError"], denied);
        assert_eq!(error.details["ownerRetirementAttempted"], true);
        assert_eq!(error.details["ownerRetired"], false);
        let exited = peer_failure(primary.clone(), &hello, Some(Ok(134)), None);
        assert_eq!(exited.details["workerExitCode"], 134);
        assert_eq!(exited.details["ownerRetirementAttempted"], false);
        assert_eq!(exited.details["ownerRetired"], false);
        let retired = peer_failure(primary, &hello, None, Some(Ok(())));
        assert!(retired.details["workerExitCode"].is_null());
        assert_eq!(retired.details["ownerRetired"], true);
        let deadline = ToolkitError::new("command.deadline", "original deadline", 5)
            .with_details(json!({"originalError":"external.io"}));
        let deadline = peer_failure(deadline, &hello, None, Some(Err(denied.into())));
        assert_eq!(deadline.code, "command.deadline");
        assert_eq!(deadline.details["originalError"], "external.io");
        assert_eq!(deadline.details["ownerRetirementError"], denied);
    }
}

#[cfg(test)]
mod semantic_admission_tests {
    use super::*;
    fn material()->crate::native_research::SnapshotRequest {
        crate::native_research::SnapshotRequest{encoding_warnings:vec![],files:BTreeMap::from([("a.lua".into(),format!("function Caller()\n{}end\n",(0..128).map(|n|format!(" missing{n}()\n")).collect::<String>()))]),load_documents:BTreeMap::new(),environment_documents:BTreeMap::new(),source:json!({"repository":"quota-fixture","product":"retail"}),
            luac:crate::native::LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},api:vec![],environment_manifest:None,load_plans:vec![],input_gaps:vec![]}
    }
    fn query(direction:&str)->QueryRequest {
        QueryRequest{symbol:crate::index::Symbol{id:String::new(),kind:"declaration".into(),name:"Caller".into(),target:String::new(),category:"function".into(),confidence:String::new(),path:"a.lua".into(),line:1,end_line:130,signature:String::new(),scope:String::new(),ordinal:0},direction:direction.into()}
    }
    #[test]
    fn research_rule_cache_key_miss_accepts_unchanged_full_input_then_reuses() {
        let control=OperationControl::default();let mut snapshot=material();
        snapshot.files.insert("a.lua".into(),"function Caller() return 1 end\n".into());
        let old_key=storage::sha256(&serde_json::to_vec(&snapshot).unwrap());
        let key=snapshot.cache_key(&control).unwrap();
        assert_eq!(key,storage::sha256(&serde_json::to_vec(&(crate::native_research::RESEARCH_RULES,&snapshot)).unwrap()));
        assert_ne!(key,old_key);
        assert_ne!(key,storage::sha256(&serde_json::to_vec(&("source-native-research-v7-xml-script-ancestry",&snapshot)).unwrap()));
        let mut cached=None;
        let first=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot:snapshot.clone(),query:query("incoming")}),None,&control,42,|_,_|{}).unwrap().into_value();
        assert_eq!(first["nativeSession"]["cacheHit"],false);
        assert_eq!(cached.as_ref().unwrap().key,key);
        let phases=std::cell::RefCell::new(Vec::new());
        let miss=execute_cached_query(&mut cached,None,Some(CachedQuery{key:old_key,query:query("incoming")}),&control,42,|phase,_|phases.borrow_mut().push(phase.to_owned())).unwrap_err();
        assert_eq!(miss.code,"source.native_snapshot_missing");assert!(!phases.borrow().iter().any(|phase|phase=="querying"));
        let restored=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot,query:query("incoming")}),None,&control,42,|_,_|{}).unwrap().into_value();
        assert_eq!(restored["nativeSession"]["cacheHit"],false);
        let reused=execute_cached_query(&mut cached,None,Some(CachedQuery{key,query:query("incoming")}),&control,42,|_,_|{}).unwrap().into_value();
        assert_eq!(reused["nativeSession"]["cacheHit"],true);assert_eq!(reused["nativeSession"]["snapshotTransfer"],"identity_only");
        assert_eq!(restored["inputIdentity"],reused["inputIdentity"]);assert_eq!(restored["relations"],reused["relations"]);
    }
    #[test]
    fn cached_partial_queries_reach_quota_retire_analysis_and_allow_full_resend() {
        let control=OperationControl::default();let snapshot=material();let key=snapshot.cache_key(&control).unwrap();let mut cached=None;
        let value=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot:snapshot.clone(),query:query("incoming")}),None,&control,42,|_,_|{}).unwrap().into_value();
        assert_eq!(value["semanticExecution"]["receipt"]["completed_batches"],0);assert_eq!(value["nativeSession"]["batches"],1);
        cached.as_mut().unwrap().admission.batches=254;
        for total in [255,256]{let value=execute_cached_query(&mut cached,None,Some(CachedQuery{key:key.clone(),query:query("incoming")}),&control,42,|_,_|{}).unwrap().into_value();assert_eq!(value["nativeSession"]["batches"],total);}
        assert!(cached.is_none());assert_eq!(session_status(None,42)["ownerPid"],42);assert_eq!(session_status(None,42)["workerPid"],0);
        assert_eq!(execute_cached_query(&mut cached,None,Some(CachedQuery{key,query:query("incoming")}),&control,42,|_,_|{}).unwrap_err().code,"source.native_snapshot_missing");
        let fresh=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot,query:query("incoming")}),None,&control,42,|_,_|{}).unwrap().into_value();assert_eq!(fresh["nativeSession"]["batches"],1);assert_eq!(fresh["nativeSession"]["cacheHit"],false);
    }
    #[test]
    fn overflow_retires_before_execution_and_full_input_rebuilds_once() {
        let control=OperationControl::default();let snapshot=material();let key=snapshot.cache_key(&control).unwrap();let mut cached=None;
        execute_cached_query(&mut cached,Some(WorkerRequest{snapshot:snapshot.clone(),query:query("incoming")}),None,&control,42,|_,_|{}).unwrap().into_value();
        cached.as_mut().unwrap().admission.batches=255;
        let phases=std::cell::RefCell::new(Vec::new());
        let error=execute_cached_query(&mut cached,None,Some(CachedQuery{key,query:query("both")}),&control,42,|phase,_|phases.borrow_mut().push(phase.to_owned())).unwrap_err();
        assert_eq!(error.code,"source.native_snapshot_missing");assert!(cached.is_none());assert!(!phases.borrow().iter().any(|phase|phase=="querying"));
        execute_cached_query(&mut cached,Some(WorkerRequest{snapshot:snapshot.clone(),query:query("incoming")}),None,&control,42,|_,_|{}).unwrap().into_value();cached.as_mut().unwrap().admission.batches=255;phases.borrow_mut().clear();
        let value=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot,query:query("both")}),None,&control,42,|phase,_|phases.borrow_mut().push(phase.to_owned())).unwrap().into_value();
        assert_eq!(value["nativeSession"]["batches"],2);assert_eq!(value["nativeSession"]["cacheHit"],false);assert_eq!(phases.borrow().iter().filter(|phase|*phase=="quota_repreparing").count(),1);assert_eq!(phases.borrow().iter().filter(|phase|*phase=="querying").count(),1);
        assert_eq!(cached.as_ref().unwrap().analysis.request_control().deadline(),control.deadline());
    }
    #[test]
    fn execution_failure_after_admission_retires_cached_analysis() {
        let control=OperationControl::default();let mut cached=None;
        let cancel=control.tracked_cancel_handle().unwrap();let error=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot:material(),query:query("incoming")}),None,&control,42,|phase,_|{if phase=="querying"{cancel.cancel();}}).unwrap_err();
        assert_eq!(error.code,"command.cancelled");assert!(cached.is_none());
    }
    #[test]
    fn attempted_batches_retire_at_256_and_overflow_is_not_admitted() {
        let mut quota=SemanticAdmission::default();
        for _ in 0..127{assert!(quota.admit(2).unwrap());}
        assert_eq!(quota.batches,254);assert!(quota.admit(2).unwrap());assert!(quota.exhausted());
        assert!(!quota.admit(1).unwrap());assert_eq!(quota.batches,256);
        let mut quota=SemanticAdmission{batches:255};assert!(!quota.admit(2).unwrap());assert_eq!(quota.batches,255);
        assert!(quota.admit(1).unwrap());assert!(quota.exhausted());
    }
    #[test]
    fn partial_completion_does_not_refund_admission_and_zero_plan_is_not_one() {
        let mut quota=SemanticAdmission::default();assert!(quota.admit(0).unwrap());assert_eq!(quota.batches,0);
        for _ in 0..256 {assert!(quota.admit(1).unwrap());/* completed batches may be zero */}
        assert!(quota.exhausted());assert!(quota.admit(3).is_err());
        let retired=SemanticAdmission::default();assert_eq!(retired.batches,0);
    }
}
