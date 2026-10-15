//! Single-thread reusable native analysis. IPC and lifetime ownership reuse the
//! Windows Source mechanisms; the cached analysis never executes studied code.
use crate::native_research::{QueryRequest, ResearchSession, WorkerRequest};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError, control::OperationControl, metadata::Metadata,
    storage, workspace,
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

const WIRE: &str = "lycheedev.native-source-session.v1";
const REQUEST_BYTES: usize = 768 << 20;
const RESPONSE_BYTES: usize = 32 << 20;
const REPLY_RESERVE: Duration = Duration::from_millis(500);
const OWNER_IDLE: Duration = Duration::from_secs(60);
const OWNER_LIFETIME: Duration = Duration::from_secs(600);
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
}
impl SessionLifetime {
    fn new(started: Instant, caller_deadline: Instant) -> Self {
        Self {
            deadline: caller_deadline.min(started + OWNER_LIFETIME),
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
        Some("idle") => status["workerPid"].as_u64() == Some(0)
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
#[derive(Serialize, Deserialize)]
struct CachedQuery {
    key: String,
    query: QueryRequest,
}
#[derive(Serialize, Deserialize)]
struct Response {
    value: Option<Value>,
    error: Option<ToolkitError>,
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
fn execute_cached_query(cached:&mut Option<Cached>,work:Option<WorkerRequest>,reuse:Option<CachedQuery>,control:&OperationControl,owner_pid:u32,record:impl Fn(&str,Option<&ToolkitError>))->Result<Value> {
    let result = (|| -> Result<Value> {
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
        let executed=cached.as_mut().expect("admitted snapshot").analysis.execute_planned_observed(planned,&control,|phase|record(phase,None))?;
        let mut value=executed.value;
        value["analysis"]["cacheHit"] = json!(hit);
        value["nativeSession"] = json!({"workerPid":owner_pid,"cacheHit":hit,"snapshotTransfer":if identity_only{"identity_only"}else{"full"},"batches":admitted,"admittedSemanticBatches":requested,"analysisRetired":admitted>=SEMANTIC_BATCH_LIMIT});
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
    Ok(Identity {
        path,
        digest: format!("{:x}", hash.finalize()),
        _file: file,
    })
}
fn write_frame(writer: &mut dyn Write, value: &impl Serialize, maximum: usize) -> Result<()> {
    let bytes = crate::lsp_frame::serialize(value, maximum).map_err(|_| {
        ToolkitError::new(
            "source.native_frame_limit",
            "Source frame exceeds budget",
            3,
        )
    })?;
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
fn read_frame<T: DeserializeOwned>(reader: &mut dyn Read, maximum: usize) -> Result<T> {
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
    Ok(serde_json::from_slice(&bytes)?)
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
fn request(inv: &Invocation, action: Action, work: Option<WorkerRequest>) -> Result<Request> {
    let remaining = inv.control.remaining()?;
    Ok(Request {
        wire: WIRE.into(),
        deadline_unix_ms: request_wire_deadline(utc_ms()?, remaining),
        action,
        work,
        cached: None,
    })
}
fn exchange(inv: &Invocation, current: &Identity, request: &Request) -> Result<Option<Value>> {
    inv.control.check()?;
    let observed=Instant::now();
    let deadline=exchange_deadline(request.deadline_unix_ms,utc_ms()?,observed,inv.control.deadline())?;
    let scope = BrokerScope::for_native_source(&inv.home).map_err(platform)?;
    let mut stream =
        match BrokerStream::connect(&scope, deadline, inv.control.cancel_handle()) {
            Ok(stream) => stream,
            Err(message) if message.starts_with("broker.not_found:") => return Ok(None),
            Err(message) => return Err(platform(message)),
        };
    let hello: Hello = read_frame(&mut stream, 4096)?;
    let peer = Process::open(stream.peer_pid().map_err(platform)?).map_err(platform)?;
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
    let closing = matches!(request.action, Action::Close);
    write_frame(&mut stream, request, REQUEST_BYTES)?;
    // Leave retirement time inside the caller's original deadline. An observer
    // timeout is never a caller cancellation or permission to extend its budget.
    let response_deadline = deadline
        .checked_sub(reply_reserve(deadline) / 2)
        .unwrap_or_else(Instant::now);
    stream.set_deadline(response_deadline, inv.control.cancel_handle());
    let response: Response = match read_frame(&mut stream, RESPONSE_BYTES) {
        Ok(response) => response,
        Err(error)
            if Instant::now() >= response_deadline
                || inv.control.token().load(Ordering::Acquire) =>
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
            let observed = peer.wait_exit(wait, &AtomicBool::new(false));
            let retired = if observed.is_err() {
                Some(retirement.retire(inv.control.deadline()))
            } else {
                None
            };
            return Err(peer_failure(error, &hello, Some(observed), retired));
        }
    };
    if closing {
        peer.wait_exit(inv.control.deadline(), inv.control.token())
            .map_err(platform)?;
    }
    if let Some(error) = response.error {
        return Err(error);
    }
    if matches!(request.action, Action::Status) {
        let status = response.value.as_ref().ok_or_else(|| fault("Native Source status is missing"))?;
        verify_status_owner(status, hello.pid)?;
    }
    Ok(response.value)
}

pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    workspace::open(&inv.home)?;
    let action = match inv.command.as_str() {
        "source session status" => Action::Status,
        "source session close" => Action::Close,
        _ => {
            return Err(ToolkitError::arguments(
                "Invalid native Source session command",
            ));
        }
    };
    let current = identity(&inv.control)?;
    Ok(Outcome::new(
        exchange(inv, &current, &request(inv, action, None)?)?
            .unwrap_or_else(|| json!({"state":"closed","backend":"native"})),
    ))
}
pub fn analyze(inv: &Invocation, work: WorkerRequest) -> Result<Value> {
    workspace::open(&inv.home)?;
    let current = identity(&inv.control)?;
    let mut reuse = request(inv, Action::Query, None)?;
    reuse.cached = Some(CachedQuery {
        key: work.snapshot.cache_key(&inv.control)?,
        query: work.query.clone(),
    });
    match exchange(inv, &current, &reuse) {
        Ok(Some(value)) => return Ok(value),
        Ok(None) => {},
        Err(error) if error.code == "source.native_snapshot_missing" => {},
        Err(error) => return Err(error),
    }
    let query = request(inv, Action::Query, Some(work))?;
    if let Some(value) = exchange(inv, &current, &query)? {
        return Ok(value);
    }
    let _launch =
        storage::ExclusiveLock::resource(&inv.home.join("locks"), "source:native-session-launch")?;
    if let Some(value) = exchange(inv, &current, &query)? {
        return Ok(value);
    }
    let parent = Process::open(std::process::id()).map_err(platform)?;
    let token = lycheedev_windows::random::token().map_err(platform)?;
    let store_key = format!("source/native-session-start/{token}");
    let intent = StartupIntent {
        identity: startup_identity(inv, &current)?,
        parent_pid: std::process::id(),
        parent_created: parent.identity().created,
        expires_unix_ms: utc_ms()?.saturating_add(10_000),
    };
    Metadata::open(&inv.home, true)?.put(&store_key, &intent, None)?;
    let args = vec![
        "--source-native-session-server".into(),
        "--home".into(),
        inv.home.canonicalize()?.into_os_string(),
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
        &inv.cwd.canonicalize()?,
        &[("LYCHEEDEV_NATIVE_SOURCE_START".into(), token.into())],
    );
    let mut child = match spawned {
        Ok(child) => child,
        Err(message) => {
            Metadata::open(&inv.home, true)?.delete(&store_key)?;
            return Err(platform(message));
        }
    };
    let ready = (|| -> Result<()> {
        loop {
            inv.control.check()?;
            if child.try_wait().map_err(platform)?.is_some() {
                return Err(fault("Native Source owner exited during startup"));
            }
            if let Some(status) = exchange(inv, &current, &request(inv, Action::Status, None)?)? {
                verify_status_owner(&status, child.id())?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if let Err(error) = ready {
        let _ = child.terminate_and_wait(inv.control.deadline());
        Metadata::open(&inv.home, true)?.delete(&store_key)?;
        return Err(error);
    }
    child.handoff().map_err(platform)?;
    exchange(inv, &current, &query)?.ok_or_else(|| fault("Source owner disappeared after startup"))
}

fn startup_identity(inv: &Invocation, current: &Identity) -> Result<String> {
    let scope = BrokerScope::for_native_source(&inv.home).map_err(platform)?;
    Ok(storage::sha256(&serde_json::to_vec(&(
        WIRE,
        scope.endpoint(),
        &current.path,
        &current.digest,
    ))?))
}

/// Private CLI entry. The single-use intent binds this launch to its actual
/// parent process, this executable and the selected same-logon workspace.
pub fn server(args: &[String]) -> Result<()> {
    let accepted = [
        "--home",
        "--parent-pid",
        "--parent-created",
        "--start-token",
    ];
    if args.len() != 9 || args[0] != "--source-native-session-server" {
        return Err(ToolkitError::arguments(
            "private native Source startup requires exact owned arguments",
        ));
    }
    let mut flags = BTreeMap::new();
    for pair in args[1..].as_chunks::<2>().0 {
        if !accepted.contains(&pair[0].as_str())
            || pair[1].is_empty()
            || flags.insert(pair[0].clone(), pair[1].clone()).is_some()
        {
            return Err(ToolkitError::arguments(
                "invalid private native Source startup arguments",
            ));
        }
    }
    let token = &flags["--start-token"];
    if !lycheedev_core::selection::valid_digest(token, 32)
        || std::env::var("LYCHEEDEV_NATIVE_SOURCE_START").ok().as_ref() != Some(token)
    {
        return Err(fault("Native Source startup token differs"));
    }
    let pid = flags["--parent-pid"]
        .parse::<u32>()
        .map_err(|_| ToolkitError::arguments("invalid native Source parent PID"))?;
    let created = flags["--parent-created"]
        .parse::<u64>()
        .map_err(|_| ToolkitError::arguments("invalid native Source parent creation"))?;
    let home = PathBuf::from(&flags["--home"]);
    if !home.is_absolute() {
        return Err(ToolkitError::arguments(
            "native Source home must be absolute",
        ));
    }
    let console =
        lycheedev_windows::cancellation::ConsoleCancellation::install().map_err(platform)?;
    let inv = Invocation {
        command: String::new(),
        positional: None,
        flags: flags.clone(),
        home: home.canonicalize()?,
        cwd: std::env::current_dir()?,
        control: OperationControl::new(Instant::now() + OWNER_LIFETIME, console.token()),
    };
    let current = identity(&inv.control)?;
    let own = Process::open(std::process::id()).map_err(platform)?;
    let parent = Process::open(pid).map_err(platform)?;
    if own.parent_pid().map_err(platform)? != pid
        || parent.identity().created != created
        || parent.image_path().map_err(platform)?.canonicalize()? != current.path
    {
        return Err(fault("Native Source actual launcher differs"));
    }
    parent.verify().map_err(platform)?;
    let expected = startup_identity(&inv, &current)?;
    let now = utc_ms()?;
    Metadata::open(&inv.home, true)?.consume::<StartupIntent, _>(
        &format!("source/native-session-start/{token}"),
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
    )?;
    serve_owned(&inv, current)
}

pub fn serve(inv: &Invocation) -> Result<()> {
    serve_owned(inv, identity(&inv.control)?)
}
fn serve_owned(inv: &Invocation, current: Identity) -> Result<()> {
    let lifetime = SessionLifetime::new(Instant::now(), inv.control.deadline());
    let owner_control = inv
        .control
        .child(lifetime.deadline, inv.control.cancel_handle());
    workspace::open(&inv.home)?;
    let scope = BrokerScope::for_native_source(&inv.home).map_err(platform)?;
    let _owner =
        storage::ExclusiveLock::resource(&inv.home.join("locks"), "source:native-session-owner")?;
    let mut job = BrokerJob::own_current_process(2 << 30).map_err(platform)?;
    let own = Process::open(std::process::id()).map_err(platform)?;
    let mut listener = BrokerListener::bind(&scope).map_err(platform)?;
    let mut cached: Option<Cached> = None;
    let run = (|| -> Result<()> {
        let mut idle_deadline = lifetime.idle_deadline(Instant::now());
        loop {
            // Only an admitted request may restart idle. Failed handshakes and
            // malformed envelopes retain this absolute idle boundary.
            owner_control.check()?;
            let mut stream = match listener.accept(
                idle_deadline,
                owner_control.token(),
            ) {
                Ok(stream) => stream,
                Err(message) if message.contains("deadline") => return Ok(()),
                Err(message) => return Err(platform(message)),
            };
            stream.set_deadline(
                lifetime.handshake_deadline(Instant::now(), idle_deadline),
                owner_control.cancel_handle(),
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
            ).is_err() {
                continue;
            }
            let request: Request = match read_frame(&mut stream, REQUEST_BYTES) {
                Ok(request) => request,
                Err(_) => continue,
            };
            if request.wire != WIRE {
                continue;
            }
            let Some(remaining) = admitted_request_remaining(request.deadline_unix_ms, utc_ms()?) else {
                continue;
            };
            if remaining.is_zero() {
                // Go dequeues an already-cancelled task without touching its
                // worker, then resets idle. Its expired transport cannot reply.
                idle_deadline = lifetime.idle_deadline(Instant::now());
                continue;
            }
            let total_deadline = Instant::now()
                .checked_add(remaining)
                .ok_or_else(|| fault("Native request deadline range"))?;
            let total_deadline = lifetime.request_deadline(total_deadline);
            let response_control =
                owner_control.child(total_deadline, owner_control.cancel_handle());
            stream.set_deadline(total_deadline, response_control.cancel_handle());
            match request.action {
                Action::Status => write_frame(
                    &mut stream,
                    &Response {
                        value: Some({let mut status=session_status(cached.as_ref().map(|value|value.key.as_str()),own.identity().pid);
                            status["batches"]=json!(cached.as_ref().map_or(0,|value|value.admission.batches));status}),
                        error: None,
                    },
                    RESPONSE_BYTES,
                )?,
                Action::Close => {
                    drop(cached.take());
                    write_frame(
                        &mut stream,
                        &Response {
                            value: Some(json!({"state":"closed","backend":"native"})),
                            error: None,
                        },
                        RESPONSE_BYTES,
                    )?;
                    return Ok(());
                }
                Action::Query => {
                    let work = request.work;
                    let reuse = request.cached;
                    let cancel = Arc::new(AtomicBool::new(false));
                    let observer_stop = Arc::new(AtomicBool::new(false));
                    let read_cancel = Arc::new(AtomicBool::new(false));
                    let mut observer = stream.try_clone().map_err(platform)?;
                    observer.set_deadline(total_deadline, read_cancel.clone());
                    let observing = observer_stop.clone();
                    let request_cancel = cancel.clone();
                    let handle = std::thread::spawn(move || {
                        let mut marker = [0u8; 1];
                        match observer.read(&mut marker) {
                            Ok(0) if !observing.load(Ordering::Acquire) => {
                                request_cancel.store(true, Ordering::Release)
                            }
                            Ok(_) if marker[0] == 1 && !observing.load(Ordering::Acquire) => {
                                request_cancel.store(true, Ordering::Release)
                            }
                            // Timeout/stop of our observation is not a peer disconnect.
                            _ => {}
                        }
                    });
                    let deadline = total_deadline
                        .checked_sub(reply_reserve(total_deadline))
                        .unwrap_or_else(Instant::now);
                    let control = owner_control.child(deadline, cancel);
                    let diagnostic = inv.home.join("tmp/source-native-last-request.json");
                    let begun = Instant::now();
                    let record = |phase: &str, error: Option<&ToolkitError>| {
                        let _ = storage::atomic_json(
                            &diagnostic,
                            &json!({"schema":"lycheedev.native-source-request-diagnostic.v1","workerPid":own.identity().pid,"workerCreated":own.identity().created,"phase":phase,"elapsedMs":begun.elapsed().as_millis(),"memory":job.memory_accounting().ok(),"processMemory":own.memory().ok(),"error":error}),
                        );
                    };
                    record("processing", None);
                    let result=execute_cached_query(&mut cached,work,reuse,&control,own.identity().pid,&record);
                    observer_stop.store(true, Ordering::Release);
                    read_cancel.store(true, Ordering::Release);
                    if handle.join().is_err() {
                        return Err(fault("Source request observer failed"));
                    }
                    record("replying", result.as_ref().err());
                    let response = match result {
                        Ok(value) => Response {
                            value: Some(value),
                            error: None,
                        },
                        Err(error) => Response {
                            value: None,
                            error: Some(error),
                        },
                    };
                    // Reply on the original transport budget, not the expired or
                    // cancelled processing token that produced the typed error.
                    response_control.check()?;
                    write_frame(&mut stream, &response, RESPONSE_BYTES)?;
                }
            }
            // Active work suspends idle, but cannot extend owner lifetime.
            idle_deadline = lifetime.idle_deadline(Instant::now());
        }
    })();
    // Preserve the actual owner failure before destruction can consume the
    // remaining reply budget. This diagnostic does not grant extra time or
    // replace the authenticated transport outcome.
    let terminal=inv.home.join("tmp/source-native-last-terminal.json");
    let cleanup_started=Instant::now();
    let terminal_record=|phase:&str,cleanup_error:Option<&ToolkitError>| {
        let _=storage::atomic_json(&terminal,&json!({"schema":"lycheedev.native-source-terminal-diagnostic.v1",
            "workerPid":own.identity().pid,"workerCreated":own.identity().created,"phase":phase,
            "cleanupElapsedMs":cleanup_started.elapsed().as_millis(),"error":run.as_ref().err(),
            "cleanupError":cleanup_error,"processMemory":own.memory().ok()}));
    };
    terminal_record("retiring_analysis",None);
    // Retire analysis before releasing the process ownership boundary on every
    // exit, including owner expiry, transport failure and cancellation.
    drop(cached.take());
    let disarmed=job.disarm().map_err(platform);
    terminal_record("owner_terminal",disarmed.as_ref().err());
    disarmed?;
    run
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let cancelled = Arc::new(AtomicBool::new(false));
        let owner = OperationControl::new(lifetime.deadline, cancelled.clone());
        let request = owner.child(
            start + Duration::from_secs(100),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(request.deadline(), lifetime.deadline);
        // Blocking transport only observes this atomic, without invoking
        // OperationControl::check to propagate cancellation lazily.
        let transport = owner.child(lifetime.deadline, owner.cancel_handle());
        let transport_token = transport.cancel_handle();
        cancelled.store(true, Ordering::Release);
        assert!(transport_token.load(Ordering::Acquire));
        assert_eq!(request.check().unwrap_err().code, "command.cancelled");
    }

    #[test]
    fn startup_status_frame_accepts_idle_owner_and_rejects_worker_pid_substitution() {
        // Exercise the same frame codec and validator as exchange/startup. A
        // new owner has no prepared analysis until the first query is sent.
        let mut wire = Vec::new();
        write_frame(&mut wire, &Response {
            value: Some(session_status(None, 42)), error: None,
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
        crate::native_research::SnapshotRequest{files:BTreeMap::from([("a.lua".into(),format!("function Caller()\n{}end\n",(0..128).map(|n|format!(" missing{n}()\n")).collect::<String>()))]),load_documents:BTreeMap::new(),environment_documents:BTreeMap::new(),source:json!({"repository":"quota-fixture","product":"retail"}),
            luac:crate::native::LuacIdentity{version:"5.1.5".into(),platform:"windows-amd64".into(),executable_sha256:"a".repeat(64),executable_bytes:1},api:vec![],environment_manifest:None,load_plans:vec![],input_gaps:vec![]}
    }
    fn query(direction:&str)->QueryRequest {
        QueryRequest{symbol:crate::index::Symbol{id:String::new(),kind:"declaration".into(),name:"Caller".into(),target:String::new(),category:"function".into(),confidence:String::new(),path:"a.lua".into(),line:1,end_line:130,signature:String::new(),scope:String::new(),ordinal:0},direction:direction.into()}
    }
    #[test]
    fn cached_partial_queries_reach_quota_retire_analysis_and_allow_full_resend() {
        let control=OperationControl::default();let snapshot=material();let key=snapshot.cache_key(&control).unwrap();let mut cached=None;
        let value=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot:snapshot.clone(),query:query("incoming")}),None,&control,42,|_,_|{}).unwrap();
        assert_eq!(value["semanticExecution"]["receipt"]["completed_batches"],0);assert_eq!(value["nativeSession"]["batches"],1);
        cached.as_mut().unwrap().admission.batches=254;
        for total in [255,256]{let value=execute_cached_query(&mut cached,None,Some(CachedQuery{key:key.clone(),query:query("incoming")}),&control,42,|_,_|{}).unwrap();assert_eq!(value["nativeSession"]["batches"],total);}
        assert!(cached.is_none());assert_eq!(session_status(None,42)["ownerPid"],42);assert_eq!(session_status(None,42)["workerPid"],0);
        assert_eq!(execute_cached_query(&mut cached,None,Some(CachedQuery{key,query:query("incoming")}),&control,42,|_,_|{}).unwrap_err().code,"source.native_snapshot_missing");
        let fresh=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot,query:query("incoming")}),None,&control,42,|_,_|{}).unwrap();assert_eq!(fresh["nativeSession"]["batches"],1);assert_eq!(fresh["nativeSession"]["cacheHit"],false);
    }
    #[test]
    fn overflow_retires_before_execution_and_full_input_rebuilds_once() {
        let control=OperationControl::default();let snapshot=material();let key=snapshot.cache_key(&control).unwrap();let mut cached=None;
        execute_cached_query(&mut cached,Some(WorkerRequest{snapshot:snapshot.clone(),query:query("incoming")}),None,&control,42,|_,_|{}).unwrap();
        cached.as_mut().unwrap().admission.batches=255;
        let phases=std::cell::RefCell::new(Vec::new());
        let error=execute_cached_query(&mut cached,None,Some(CachedQuery{key,query:query("both")}),&control,42,|phase,_|phases.borrow_mut().push(phase.to_owned())).unwrap_err();
        assert_eq!(error.code,"source.native_snapshot_missing");assert!(cached.is_none());assert!(!phases.borrow().iter().any(|phase|phase=="querying"));
        execute_cached_query(&mut cached,Some(WorkerRequest{snapshot:snapshot.clone(),query:query("incoming")}),None,&control,42,|_,_|{}).unwrap();cached.as_mut().unwrap().admission.batches=255;phases.borrow_mut().clear();
        let value=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot,query:query("both")}),None,&control,42,|phase,_|phases.borrow_mut().push(phase.to_owned())).unwrap();
        assert_eq!(value["nativeSession"]["batches"],2);assert_eq!(value["nativeSession"]["cacheHit"],false);assert_eq!(phases.borrow().iter().filter(|phase|*phase=="quota_repreparing").count(),1);assert_eq!(phases.borrow().iter().filter(|phase|*phase=="querying").count(),1);
        assert_eq!(cached.as_ref().unwrap().analysis.request_control().deadline(),control.deadline());
    }
    #[test]
    fn execution_failure_after_admission_retires_cached_analysis() {
        let control=OperationControl::default();let mut cached=None;
        let cancel=control.cancel_handle();let error=execute_cached_query(&mut cached,Some(WorkerRequest{snapshot:material(),query:query("incoming")}),None,&control,42,|phase,_|{if phase=="querying"{cancel.store(true,Ordering::Release);}}).unwrap_err();
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
