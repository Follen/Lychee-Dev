//! Opt-in same-logon semantic broker. CLI composition authenticates its sealed
//! release before entering this module; IPC never accepts executable paths.
use crate::{
    SourceTools, environment, git, index, lsp_frame,
    luals::{Analysis, ResultAdmission, WorkspaceSession},
};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError,
    control::OperationControl,
    metadata::Metadata,
    selection::SourcePin,
    storage::{self, ExclusiveLock},
    workspace,
};
use lycheedev_windows::{
    BrokerJob, ChangeWatch, Process,
    broker::{BrokerListener, BrokerScope, BrokerStream},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const WIRE: &str = "lycheedev.luals-broker.rust.v1";
const REQUEST: usize = 64 << 10;
const RESPONSE: usize = 8 << 20;
const REPLY_RESERVE: Duration = Duration::from_millis(500);
static RESOURCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Debug)]
pub struct BrokerIdentity {
    pub exe_path: PathBuf,
    pub exe_sha256: String,
    pub release_root: PathBuf,
    pub release_manifest_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Hello {
    wire: String,
    identity: String,
    pid: u32,
    started: u64,
    executable: String,
    release: String,
}
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Action {
    Status,
    Retire,
    Analyze,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    wire: String,
    identity: String,
    action: Action,
    deadline_unix_ms: u64,
    #[serde(default)]
    pin: Option<SourcePin>,
    #[serde(default)]
    environment: Option<SourcePin>,
    #[serde(default)]
    batches: Vec<Vec<Value>>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Response {
    state: String,
    worker_pid: Option<u32>,
    batches: usize,
    reused: bool,
    #[serde(default)]
    analysis: Option<Analysis>,
    #[serde(default)]
    error: Option<ToolkitError>,
}
impl Response {
    fn status(worker: &mut Option<Worker>) -> Self {
        if worker.as_mut().is_some_and(|w| !w.session.usable()) {
            worker.take();
        }
        Self {
            state: if worker.is_some() { "ready" } else { "idle" }.into(),
            worker_pid: worker.as_ref().and_then(|w| w.session.pid()),
            batches: worker.as_ref().map_or(0, |w| w.session.batches()),
            reused: false,
            analysis: None,
            error: None,
        }
    }
}
struct Worker {
    key: String,
    session: WorkspaceSession,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartupIntent {
    identity: String,
    parent_pid: u32,
    parent_created: u64,
    expires_unix_ms: u64,
}
/// Consume the exact durable, single-use startup intent only after checking the
/// actual OS parent, its live creation identity and the same executable bytes.
pub fn consume_startup(
    inv: &Invocation,
    expected: &BrokerIdentity,
    parent_pid: u32,
    parent_created: u64,
    token: &str,
) -> Result<()> {
    if !lycheedev_core::selection::valid_digest(token, 32)
        || std::env::var("LYCHEEDEV_BROKER_START").ok().as_deref() != Some(token)
    {
        return Err(fault("Broker startup token differs"));
    }
    let process = Process::open(std::process::id()).map_err(native)?;
    if process.parent_pid().map_err(native)? != parent_pid {
        return Err(fault("Broker actual OS parent differs"));
    }
    let parent = Process::open(parent_pid).map_err(native)?;
    if parent.identity().created != parent_created
        || !same_path(
            &canonical(&parent.image_path().map_err(native)?)?,
            &canonical(&expected.exe_path)?,
        )
    {
        return Err(fault("Broker launcher creation/executable differs"));
    }
    let (held_parent, digest) = locked_hash(&expected.exe_path, 128 << 20, &inv.control)?;
    let result = (|| {
        if digest != expected.exe_sha256 {
            return Err(fault("Broker launcher content differs"));
        }
        parent.verify().map_err(native)?;
        let (key, held_identity) = identity(&inv.home, expected, &inv.control)?;
        let result = (|| {
            let store_key = format!("source/broker-start/{token}");
            let mut store = Metadata::open_controlled(&inv.home, true, &inv.control)?;
            let checked = (|| {
                let intent: StartupIntent = store.read_controlled(&store_key, &inv.control)?;
                let now = utc_ms()?;
                if intent.identity != key
                    || intent.parent_pid != parent_pid
                    || intent.parent_created != parent_created
                    || intent.expires_unix_ms <= now
                    || intent.expires_unix_ms > now + 10_000
                {
                    return Err(fault("Broker durable startup intent differs or expired"));
                }
                let generation = store
                    .generation_controlled(&store_key, &inv.control)?
                    .ok_or_else(|| fault("Broker startup intent disappeared"))?;
                store.consume_generation_controlled(
                    &store_key,
                    generation,
                    &inv.control,
                    |current: StartupIntent| {
                        if current.identity != intent.identity
                            || current.parent_pid != intent.parent_pid
                            || current.parent_created != intent.parent_created
                            || current.expires_unix_ms != intent.expires_unix_ms
                        {
                            return Err(fault("Broker startup intent changed"));
                        }
                        Ok(())
                    },
                )
            })();
            let consumed = checked.is_ok();
            crate::persistence::finish_control(
                lycheedev_core::metadata::historical_write::finish_historical(checked, store.close()), inv.control.check()).map_err(|mut error| {
                    if consumed {
                        error.details = json!({"schema":"lycheedev.source-persistence.v1","key":store_key,"mutation":"consume","state":"committed","cause":error.clone()});
                    }
                    error
                })
        })();
        crate::persistence::finish_files(held_identity, result, &inv.control)
    })();
    storage::finish_file(held_parent, result, &inv.control)
}
struct DisconnectObserver {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl DisconnectObserver {
    fn start(connection: &BrokerStream, control: &OperationControl) -> Result<Self> {
        Self::start_with_builder(
            connection,
            control,
            std::thread::Builder::new().name("source-disconnect".into()),
        )
    }
    fn start_with_builder(
        connection: &BrokerStream,
        control: &OperationControl,
        builder: std::thread::Builder,
    ) -> Result<Self> {
        let mut reader = connection.try_clone().map_err(native)?;
        let stop = Arc::new(AtomicBool::new(false));
        reader.set_deadline(control.deadline(), stop.clone());
        let stopped = stop.clone();
        let cancel = control.cancel_handle();
        let thread = builder
            .spawn(move || {
                let mut extra = [0u8; 1];
                // EOF or additional peer input is an observed client action.
                // This observer's own deadline/stop error is not a disconnect.
                if reader.read(&mut extra).is_ok() && !stopped.load(Ordering::Acquire) {
                    cancel.store(true, Ordering::Release);
                }
            })
            .map_err(|e| {
                let mut error = ToolkitError::new(
                    "luals.input_budget",
                    "Unable to admit the bounded disconnect observer thread",
                    3,
                );
                error.details =
                    json!({"resource":"disconnectObserverThread","osCode":e.raw_os_error()});
                error
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for DisconnectObserver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn fault(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new("luals.unavailable", message, 3)
}
fn native(reason: String) -> ToolkitError {
    fault(reason)
}
fn write_frame(writer: &mut dyn Write, value: &impl Serialize, maximum: usize) -> Result<()> {
    let bytes = lsp_frame::serialize(value, maximum).map_err(|_| {
        ToolkitError::new(
            "luals.input_budget",
            "Broker frame output exceeds allowance",
            3,
        )
    })?;
    if bytes.is_empty() {
        return Err(ToolkitError::invalid("Empty broker frame"));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
fn read_frame<T: DeserializeOwned>(reader: &mut dyn Read, maximum: usize) -> Result<T> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > maximum {
        return Err(ToolkitError::new(
            "luals.input_budget",
            "Broker frame input exceeds allowance",
            3,
        ));
    }
    let mut raw = vec![0; length];
    reader.read_exact(&mut raw)?;
    Ok(serde_json::from_slice(&raw)?)
}
fn canonical(path: &Path) -> Result<PathBuf> {
    Ok(fs::canonicalize(path)?)
}
fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .eq_ignore_ascii_case(b.to_string_lossy().trim_start_matches("\\\\?\\"))
}
fn locked_hash(
    path: &Path,
    maximum: u64,
    control: &OperationControl,
) -> Result<(fs::File, String)> {
    use sha2::{Digest, Sha256};
    let mut options = fs::OpenOptions::new();
    options.read(true);
    use std::os::windows::fs::OpenOptionsExt;
    options.share_mode(1);
    control.check()?;
    let mut file = options.open(path)?;
    let hashed = (|| {
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > maximum {
            return Err(fault("Broker identity file exceeds budget"));
        }
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        let mut bytes = 0u64;
        loop {
            control.check()?;
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            bytes += read as u64;
            if bytes > maximum {
                return Err(fault("Broker identity grew"));
            }
            hash.update(&buffer[..read]);
        }
        if bytes != metadata.len() {
            return Err(fault("Broker identity length changed"));
        }
        control.check()?;
        Ok(format!("{:x}", hash.finalize()))
    })();
    match hashed {
        Ok(digest) => Ok((file, digest)),
        Err(error) => storage::finish_file(file, Err(error), control),
    }
}
fn identity(
    home: &Path,
    expected: &BrokerIdentity,
    control: &OperationControl,
) -> Result<(String, Vec<fs::File>)> {
    if !lycheedev_core::selection::valid_digest(&expected.exe_sha256, 64)
        || !lycheedev_core::selection::valid_digest(&expected.release_manifest_sha256, 64)
    {
        return Err(fault("Invalid verified broker identity"));
    }
    let current = canonical(&std::env::current_exe()?)?;
    if !same_path(&current, &canonical(&expected.exe_path)?) {
        return Err(fault("Broker entry must use the running CLI executable"));
    }
    let release = canonical(&expected.release_root)?;
    storage::guard_path(&release, &release.join("release.json"))?;
    let (exe, exe_hash) = locked_hash(&current, 128 << 20, control)?;
    let (manifest, manifest_hash) =
        match locked_hash(&release.join("release.json"), 1 << 20, control) {
            Ok(value) => value,
            Err(error) => return storage::finish_file(exe, Err(error), control),
        };
    let verified = (|| {
        if exe_hash != expected.exe_sha256 || manifest_hash != expected.release_manifest_sha256 {
            return Err(fault("Verified broker release/executable identity changed"));
        }
        let key = storage::sha256(
            format!(
                "{WIRE}\0{}\0{}\0{exe_hash}\0{manifest_hash}",
                canonical(home)?.to_string_lossy().to_lowercase(),
                current.to_string_lossy().to_lowercase()
            )
            .as_bytes(),
        );
        Ok(key)
    })();
    match verified {
        Ok(key) => Ok((key, vec![exe, manifest])),
        Err(error) => crate::persistence::finish_files(vec![exe, manifest], Err(error), control),
    }
}
fn peer(
    stream: &BrokerStream,
    expected: &BrokerIdentity,
    pid: u32,
    started: Option<u64>,
    control: &OperationControl,
) -> Result<fs::File> {
    if stream.peer_pid().map_err(native)? != pid {
        return Err(fault("Broker pipe process mismatch"));
    }
    let process = Process::open(pid).map_err(native)?;
    if started.is_some_and(|s| process.identity().created != s) {
        return Err(fault("Broker peer process creation identity differs"));
    }
    let image = process.image_path().map_err(native)?;
    if !same_path(&canonical(&image)?, &canonical(&expected.exe_path)?) {
        return Err(fault("Broker peer executable path differs"));
    }
    let (file, digest) = locked_hash(&image, 128 << 20, control)?;
    let verified = (|| {
        if digest != expected.exe_sha256 {
            return Err(fault("Broker peer executable content differs"));
        }
        process.verify().map_err(native)?;
        control.check()
    })();
    match verified {
        Ok(()) => Ok(file),
        Err(error) => storage::finish_file(file, Err(error), control),
    }
}
fn utc_ms() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| fault("System clock predates epoch"))?
            .as_millis(),
    )
    .map_err(|_| fault("Clock range"))
}
fn request_control(request: &Request, parent: &OperationControl) -> Result<OperationControl> {
    let now = utc_ms()?;
    let remaining = request
        .deadline_unix_ms
        .checked_sub(now)
        .filter(|n| *n > 0 && *n <= 90_000)
        .ok_or_else(|| {
            ToolkitError::invalid("Broker request deadline must be current and at most90s")
        })?;
    Ok(parent.child(
        Instant::now() + Duration::from_millis(remaining),
        Arc::new(AtomicBool::new(false)),
    ))
}
fn processing_control(response: &OperationControl) -> OperationControl {
    let reserve = REPLY_RESERVE.min(
        response
            .deadline()
            .saturating_duration_since(Instant::now())
            / 4,
    );
    response.child(
        response.deadline() - reserve,
        Arc::new(AtomicBool::new(false)),
    )
}
fn analyze_request(
    inv: &Invocation,
    tools: &SourceTools<'_>,
    request: &Request,
    worker: &mut Option<Worker>,
    control: &OperationControl,
) -> Result<Response> {
    if request.batches.is_empty()
        || request.batches.len() > 2
        || request.batches.iter().any(|b| b.len() > 128)
    {
        return Err(ToolkitError::new(
            "luals.input_budget",
            "Broker batch admission",
            3,
        ));
    }
    let pin = request
        .pin
        .as_ref()
        .ok_or_else(|| ToolkitError::invalid("Broker source pin required"))?;
    git::validate_pin(pin)?;
    let client = request.environment.as_ref().unwrap_or(pin);
    if client.repository != "wow-ui-source" || client.product != pin.product {
        return Err(ToolkitError::new(
            "environment.client_pin_unavailable",
            "Broker requires the selected product's exact ClientPin",
            3,
        ));
    }
    git::validate_pin(client)?;
    let key = storage::sha256(&storage::identity_json(&(
        pin,
        client,
        index::fingerprint(pin),
    ))?);
    if worker.as_mut().is_some_and(|w| {
        w.key != key || !w.session.usable() || w.session.batches() + request.batches.len() > 256
    }) {
        worker.take();
    }
    let reused = worker.is_some();
    if !reused {
        let runtime = tools
            .luals
            .ok_or_else(|| fault("Verified LuaLS runtime required"))?;
        let owner = Process::open(std::process::id()).map_err(native)?;
        let (prepared, before, documents, definitions_bytes, definitions_sha256) = {
            let indexed = index::build_with_control(&inv.home, pin, control)?;
            let environment = environment::from_pin(&inv.home, client, control)?;
            let prepared = runtime.prepare_source_workspace(
                &inv.home,
                pin,
                &indexed,
                environment.definitions.as_bytes(),
                control,
            )?;
            let before = owner.memory().map_err(native)?;
            let documents = indexed.documents.len();
            let bytes = environment.definitions.len();
            let sha = storage::sha256(environment.definitions.as_bytes());
            (prepared, before, documents, bytes, sha)
        }; // Index/environment/raw declarations actually drop BEFORE LuaLS spawn.
        let after = owner.memory().map_err(native)?;
        let admitted = after.private_commit <= 32 << 20;
        let _ = storage::atomic_json(
            &inv.home.join("tmp/source-broker-resource-last.json"),
            &json!({
                "schema":"lycheedev.source-broker-resource.v1","phase":"preparedBeforeLuaLSSpawn",
                "owner":{"pid":owner.identity().pid,"created":owner.identity().created.to_string()},
                "sequence":RESOURCE_SEQUENCE.fetch_add(1, Ordering::Relaxed)+1,"unixMs":utc_ms()?,
                "sourcePin":pin,"documents":documents,"definitionsBytes":definitions_bytes,"definitionsSHA256":definitions_sha256,
                "beforeDerivedDrop":before,"afterDerivedDrop":after,"controlBaselineLimitBytes":32u64<<20,"admitted":admitted,
                "scope":"held owner process PrivateUsage after real derived-object scope Drop, before any LuaLS process; not RSS or current aggregate Job telemetry"
            }),
        );
        if !admitted {
            return Err(ToolkitError::new(
                "luals.input_budget",
                "Broker control-process baseline exceeds admission",
                3,
            ));
        }
        let session = runtime.open_prepared_source_session(prepared, control)?;
        *worker = Some(Worker { key, session });
    }
    let retained = worker.as_mut().unwrap();
    let mut combined = Analysis {
        state: "complete".into(),
        results: Vec::new(),
        runtime: tools.luals.unwrap().identity(),
        position_encoding: "utf-16".into(),
        coverage: Value::Null,
    };
    let mut admission = ResultAdmission::new();
    for batch in &request.batches {
        control.check()?;
        let mut part = retained
            .session
            .analyze_typed(batch, control, &mut admission)?;
        if part.state != "complete" {
            combined.state = "partial".into();
        }
        combined.coverage = part.coverage;
        combined
            .results
            .try_reserve_exact(part.results.len())
            .map_err(|_| {
                ToolkitError::new(
                    "luals.input_budget",
                    "Combined semantic results allocation budget",
                    3,
                )
            })?;
        combined.results.append(&mut part.results);
    }
    Ok(Response {
        state: combined.state.clone(),
        worker_pid: retained.session.pid(),
        batches: retained.session.batches(),
        reused,
        analysis: Some(combined),
        error: None,
    })
}
/// The hidden CLI caller must already authenticate the launcher PID/creation,
/// one-time startup token, executable and sealed release. This function owns
/// only this broker and its workers; it never kills or takes over a stale PID.
pub fn serve(inv: &Invocation, tools: SourceTools<'_>, expected: BrokerIdentity) -> Result<()> {
    workspace::open_controlled(&inv.home, &inv.control)?;
    inv.control.check()?;
    let (key, held_identity) = identity(&inv.home, &expected, &inv.control)?;
    let result = (|| {
        let scope = BrokerScope::for_home(&inv.home).map_err(native)?;
        let _aggregate = lycheedev_windows::NamedLock::acquire(&format!(
            "Local\\LycheeDev.SourceBroker.{}",
            scope.logon_hash()
        ))
        .map_err(native)?;
        let owner = ExclusiveLock::resource_controlled(
            &inv.home.join("locks"),
            "source:v1:broker-owner",
            &inv.control,
        )?;
        let owned = (|| {
            let mut custody = BrokerJob::own_current_process(2 << 30).map_err(native)?;
            // This bounded, replaceable diagnostic is never an authorization source.
            // Retain actual OS creation identity so an absent endpoint is not mistaken
            // for a known process exit, or a recycled PID for the owned server.
            let own = Process::open(std::process::id())
                .map_err(native)?
                .identity()
                .clone();
            let diagnostic = inv.home.join("tmp/source-broker-last-run.json");
            let begun = Instant::now();
            let record = |phase: &str, result: Option<&Result<()>>| {
                let error = result.and_then(|r| r.as_ref().err()).map(|e| {
                    let message = e.message.chars().take(512).collect::<String>();
                    json!({"code":e.code,"exitCode":e.exit_code,"message":message})
                });
                let value = json!({"schema":"lycheedev.source-broker-lifecycle.v1","pid":own.pid,"created":own.created.to_string(),"phase":phase,"terminal":result.is_some(),"error":error,"unixMs":utc_ms().ok(),"elapsedMs":begun.elapsed().as_millis()});
                let _ = storage::atomic_json(&diagnostic, &value);
            };
            record("starting", None);
            let mut phase = "starting";
            let result = (|| -> Result<()> {
                let release_changed = Arc::new(AtomicBool::new(false));
                let _release_watch =
                    ChangeWatch::start(&expected.release_root, true, release_changed.clone())
                        .map_err(native)?;
                let mut listener = BrokerListener::bind(&scope).map_err(native)?;
                let process = Process::open(std::process::id()).map_err(native)?;
                let hello = Hello {
                    wire: WIRE.into(),
                    identity: key.clone(),
                    pid: std::process::id(),
                    started: process.identity().created,
                    executable: expected.exe_sha256.clone(),
                    release: canonical(&expected.release_root)?.to_string_lossy().into(),
                };
                let life = inv
                    .control
                    .deadline()
                    .min(Instant::now() + Duration::from_secs(600));
                let mut worker = None;
                loop {
                    phase = "accept";
                    record(phase, None);
                    inv.control.check()?;
                    if release_changed.load(Ordering::Acquire) {
                        return Err(fault("Broker sealed release changed"));
                    }
                    let idle = life.min(Instant::now() + Duration::from_secs(60));
                    let mut connection = match listener.accept(idle, inv.control.token()) {
                        Ok(c) => c,
                        Err(e) if e == "broker.deadline" => break,
                        Err(e) => return Err(native(e)),
                    };
                    let handshake = OperationControl::new(
                        life.min(Instant::now() + Duration::from_secs(5)),
                        inv.control.cancel_handle(),
                    );
                    connection.set_deadline(handshake.deadline(), handshake.cancel_handle());
                    let client_pid = connection.peer_pid().map_err(native)?;
                    let held_peer = match peer(&connection, &expected, client_pid, None, &handshake)
                    {
                        Ok(p) => p,
                        Err(error) if !error.details.is_null() => return Err(error),
                        Err(_) => continue,
                    };
                    let handled = (|| -> Result<bool> {
                        if write_frame(&mut connection, &hello, 4096).is_err() {
                            return Ok(false);
                        }
                        let request: Request = match read_frame(&mut connection, REQUEST) {
                            Ok(r) => r,
                            Err(_) => return Ok(false),
                        };
                        if request.wire != WIRE || request.identity != key {
                            return Ok(false);
                        }
                        let life_control = OperationControl::new(life, inv.control.cancel_handle());
                        let response_control = match request_control(&request, &life_control) {
                            Ok(c) => c,
                            Err(_) => return Ok(false),
                        };
                        // Both stages stay inside the original request and owner lifetime.
                        // Business cancellation cannot prevent its typed error from replying.
                        let control = processing_control(&response_control);
                        connection.set_deadline(
                            response_control.deadline(),
                            response_control.cancel_handle(),
                        );
                        phase = "request";
                        let monitor = match DisconnectObserver::start(&connection, &control) {
                            Ok(monitor) => monitor,
                            Err(error) => {
                                // No observer was admitted. Reap the retained semantic worker
                                // before a bounded typed resource failure, then retire this
                                // owner instead of panicking or proceeding without monitoring.
                                worker.take();
                                let response = Response {
                                    state: "partial".into(),
                                    worker_pid: None,
                                    batches: 0,
                                    reused: false,
                                    analysis: None,
                                    error: Some(error.clone()),
                                };
                                write_frame(&mut connection, &response, RESPONSE)?;
                                return Err(error);
                            }
                        };
                        let response = match request.action {
                            Action::Status => Response::status(&mut worker),
                            Action::Retire => {
                                worker.take();
                                Response {
                                    state: "closed".into(),
                                    worker_pid: None,
                                    batches: 0,
                                    reused: false,
                                    analysis: None,
                                    error: None,
                                }
                            }
                            Action::Analyze => {
                                match analyze_request(inv, &tools, &request, &mut worker, &control)
                                {
                                    Ok(r) => r,
                                    Err(e) => {
                                        worker.take();
                                        Response {
                                            state: "partial".into(),
                                            worker_pid: None,
                                            batches: 0,
                                            reused: false,
                                            analysis: None,
                                            error: Some(e),
                                        }
                                    }
                                }
                            }
                        };
                        // Stop and drain the read observer before returning a normal reply;
                        // the successful client will then close its connection legitimately.
                        drop(monitor);
                        complete_response(
                            &mut connection,
                            &response,
                            &control,
                            &response_control,
                            &release_changed,
                            &mut worker,
                        );
                        if request.action == Action::Retire {
                            phase = "retired";
                            return Ok(true);
                        }
                        if worker.as_ref().is_some_and(|w| w.session.batches() >= 256) {
                            worker.take();
                        }
                        Ok(false)
                    })();
                    if storage::finish_file(held_peer, handled, &inv.control)? {
                        break;
                    }
                }
                drop(worker);
                drop(listener);
                Ok(())
            })();
            let result = lycheedev_core::metadata::historical_write::finish_historical(
                result,
                custody.disarm().map_err(native),
            );
            record(phase, Some(&result));
            result
        })();
        crate::persistence::finish_control(
            lycheedev_core::metadata::historical_write::finish_historical(owned, owner.close()),
            inv.control.check(),
        )
    })();
    crate::persistence::finish_files(held_identity, result, &inv.control)
}
// Reply only within the original transport deadline, after business has stopped.
fn complete_response(
    connection: &mut BrokerStream,
    response: &Response,
    control: &OperationControl,
    response_control: &OperationControl,
    release_changed: &AtomicBool,
    worker: &mut Option<Worker>,
) {
    if control.check().is_err() {
        worker.take();
    }
    connection.set_deadline(
        response_control.deadline(),
        response_control.cancel_handle(),
    );
    // A disconnected writer never leaves an analyzed result reusable.
    if release_changed.load(Ordering::Acquire)
        || response_control.check().is_err()
        || write_frame(connection, response, RESPONSE).is_err()
    {
        worker.take();
    }
}
fn transact(
    inv: &Invocation,
    expected: &BrokerIdentity,
    request: Request,
) -> Result<Option<Response>> {
    transact_with_owner(inv, expected, request, None)
}
fn transact_with_owner(
    inv: &Invocation,
    expected: &BrokerIdentity,
    request: Request,
    owner: Option<&lycheedev_windows::ProcessIdentity>,
) -> Result<Option<Response>> {
    let retiring = request.action == Action::Retire;
    let (key, held_identity) = identity(&inv.home, expected, &inv.control)?;
    let result = (|| {
        let scope = BrokerScope::for_home(&inv.home).map_err(native)?;
        let mut stream = match BrokerStream::connect(
            &scope,
            inv.control.deadline(),
            inv.control.cancel_handle(),
        ) {
            Ok(s) => s,
            Err(e) if e.starts_with("broker.not_found:") => return Ok(None),
            Err(e) => {
                inv.control.check()?;
                return Err(native(e));
            }
        };
        let exchange = (|| -> Result<Response> {
            let hello: Hello = read_frame(&mut stream, 4096)?;
            if owner.is_some_and(|p| p.pid != hello.pid || p.created != hello.started) {
                return Err(fault(
                    "Broker readiness came from a different process creation identity",
                ));
            }
            let held_peer = peer(
                &stream,
                expected,
                hello.pid,
                Some(hello.started),
                &inv.control,
            )?;
            let result = (|| {
                if hello.wire != WIRE
                    || hello.identity != key
                    || hello.executable != expected.exe_sha256
                    || !same_path(
                        Path::new(&hello.release),
                        &canonical(&expected.release_root)?,
                    )
                {
                    return Err(fault("Broker hello identity differs"));
                }
                // Keep the exact authenticated process handle before asking it to exit.
                // A Retire response acknowledges worker shutdown, but the server still
                // holds its owner lease until its function/CLI has actually terminated.
                let retiring_process = if retiring {
                    let process = Process::open(hello.pid).map_err(native)?;
                    if process.identity().created != hello.started {
                        return Err(fault("Broker retirement process creation identity differs"));
                    }
                    Some(process)
                } else {
                    None
                };
                write_frame(&mut stream, &request, REQUEST)?;
                let response: Response = read_frame(&mut stream, RESPONSE)?;
                if let Some(error) = response.error.clone() {
                    return Err(error);
                }
                if let Some(process) = retiring_process {
                    let exit = match process.wait_exit(inv.control.deadline(), inv.control.token())
                    {
                        Ok(exit) => exit,
                        Err(error) => {
                            inv.control.check()?;
                            return Err(native(error));
                        }
                    };
                    if exit != 0 {
                        return Err(fault(format!(
                            "Broker retirement exited unsuccessfully ({exit})"
                        )));
                    }
                }
                Ok(response)
            })();
            storage::finish_file(held_peer, result, &inv.control)
        })();
        match exchange {
            Ok(response) => Ok(Some(response)),
            Err(error) => crate::persistence::finish_control(Err(error), inv.control.check()),
        }
    })();
    crate::persistence::finish_files(held_identity, result, &inv.control)
}
fn request(
    inv: &Invocation,
    expected: &BrokerIdentity,
    action: Action,
    pin: Option<SourcePin>,
    environment: Option<SourcePin>,
    batches: Vec<Vec<Value>>,
) -> Result<Request> {
    let (key, held_identity) = identity(&inv.home, expected, &inv.control)?;
    let result = (|| {
        let remaining = inv.control.remaining()?.min(Duration::from_secs(90));
        Ok(Request {
            wire: WIRE.into(),
            identity: key,
            action,
            deadline_unix_ms: utc_ms()?
                .checked_add(remaining.as_millis() as u64)
                .ok_or_else(|| fault("Deadline range"))?,
            pin,
            environment,
            batches,
        })
    })();
    crate::persistence::finish_files(held_identity, result, &inv.control)
}
/// No implicit launch: absent endpoint is closed. An unknown owner stays owned.
pub fn dispatch(inv: &Invocation, expected: &BrokerIdentity) -> Result<Outcome> {
    workspace::open_controlled(&inv.home, &inv.control)?;
    let action = match inv.command.as_str() {
        "source session status" => Action::Status,
        "source session close" => Action::Retire,
        _ => return Err(ToolkitError::arguments("Invalid Source session command")),
    };
    let retiring = action == Action::Retire;
    let response = transact(
        inv,
        expected,
        request(inv, expected, action, None, None, vec![])?,
    )?;
    Ok(Outcome::new(match response {
        None => json!({"state":"closed"}),
        Some(_) if retiring => json!({"state":"closed"}),
        Some(r) => {
            json!({"state":r.state,"workerPid":r.worker_pid.unwrap_or(0),"batches":r.batches})
        }
    }))
}
pub fn analyze_existing(
    inv: &Invocation,
    expected: &BrokerIdentity,
    pin: SourcePin,
    environment: Option<SourcePin>,
    batches: Vec<Vec<Value>>,
) -> Result<Value> {
    let response = transact(
        inv,
        expected,
        request(
            inv,
            expected,
            Action::Analyze,
            Some(pin),
            environment,
            batches,
        )?,
    )?
    .ok_or_else(|| fault("Opt-in Source broker is absent; verified launch provider required"))?;
    Ok(serde_json::to_value(
        response
            .analysis
            .ok_or_else(|| fault("Broker analysis missing"))?,
    )?)
}

/// Opt-in persistent analysis. Only an absent, authenticated endpoint authorizes
/// launching this exact running CLI; foreign or busy owners are never replaced.
pub fn analyze(
    inv: &Invocation,
    expected: &BrokerIdentity,
    pin: SourcePin,
    environment: Option<SourcePin>,
    batches: Vec<Vec<Value>>,
) -> Result<Value> {
    let first = request(
        inv,
        expected,
        Action::Analyze,
        Some(pin.clone()),
        environment.clone(),
        batches.clone(),
    )?;
    if let Some(response) = transact(inv, expected, first)? {
        return Ok(serde_json::to_value(
            response
                .analysis
                .ok_or_else(|| fault("Broker analysis missing"))?,
        )?);
    }
    launch(inv, expected)?;
    analyze_existing(inv, expected, pin, environment, batches)
}
/// Create the durable single-use launcher intent while holding the launcher
/// lease and verified executable/release handles. The detached broker owns its
/// aggregate Job; failed startup terminates only our actual Child handle.
pub fn launch(inv: &Invocation, expected: &BrokerIdentity) -> Result<()> {
    use std::ffi::OsString;
    let begun = Instant::now();
    let mut phases = vec![];
    let mut attempt_count = 0usize;
    let mut child_identity = None;
    workspace::open_controlled(&inv.home, &inv.control)?;
    let (key, held_identity) = identity(&inv.home, expected, &inv.control)?;
    let result = (|| {
        phases.push(json!({"phase":"identity-verified","elapsedMs":begun.elapsed().as_millis()}));
        let launch = ExclusiveLock::resource_controlled(
            &inv.home.join("locks"),
            "source:v1:broker-launch",
            &inv.control,
        )?;
        let result = (|| {
            if transact(
                inv,
                expected,
                request(inv, expected, Action::Status, None, None, vec![])?,
            )?
            .is_some()
            {
                return Ok(());
            }
            let parent = Process::open(std::process::id()).map_err(native)?;
            let token = lycheedev_windows::random::token().map_err(native)?;
            let store_key = format!("source/broker-start/{token}");
            let intent = StartupIntent {
                identity: key,
                parent_pid: std::process::id(),
                parent_created: parent.identity().created,
                expires_unix_ms: utc_ms()?
                    .checked_add(10_000)
                    .ok_or_else(|| fault("Startup clock range"))?,
            };
            let generation =
                crate::persistence::create_intent(&inv.home, &store_key, &intent, &inv.control)?;
            let arguments: Vec<OsString> = vec![
                "--source-broker-server".into(),
                "--home".into(),
                canonical(&inv.home).map_err(|error| crate::persistence::retained_intent(error, &store_key, generation))?.into_os_string(),
                "--release".into(),
                canonical(&expected.release_root).map_err(|error| crate::persistence::retained_intent(error, &store_key, generation))?.into_os_string(),
                "--parent-pid".into(),
                intent.parent_pid.to_string().into(),
                "--parent-created".into(),
                intent.parent_created.to_string().into(),
                "--start-token".into(),
                token.clone().into(),
            ];
            // A retained broker communicates exclusively through its authenticated
            // named pipe. Broad std Command inheritance would retain the caller's
            // unrelated stdout handles even when all three configured streams are null.
            let spawned = lycheedev_windows::detached::spawn_hidden_no_inherit(
                &expected.exe_path,
                &arguments,
                &canonical(&inv.cwd).map_err(|error| crate::persistence::retained_intent(error, &store_key, generation))?,
                &[("LYCHEEDEV_BROKER_START".into(), token.clone().into())],
            )
            .map_err(native);
            let started = (|| -> Result<()> {
                let mut child = spawned?;
                child_identity =
                    Some(json!({"pid":child.id(),"created":child.identity().created.to_string()}));
                phases.push(json!({"phase":"spawned","elapsedMs":begun.elapsed().as_millis()}));
                let deadline = inv
                    .control
                    .deadline()
                    .min(Instant::now() + Duration::from_secs(5));
                let mut startup = inv.clone();
                // Native readiness waits observe the original caller token directly;
                // a fresh child token would not asynchronously inherit Console Ctrl+C.
                startup.control = OperationControl::new(deadline, inv.control.cancel_handle());
                let ready = (|| -> Result<()> {
                    loop {
                        startup.control.check()?;
                        if let Some(exit) = child.try_wait().map_err(native)? {
                            return Err(fault(format!(
                                "Owned Source broker exited before readiness ({exit})"
                            )));
                        }
                        attempt_count += 1;
                        let request =
                            request(&startup, expected, Action::Status, None, None, vec![])?;
                        if phases.len() < 24 {
                            phases.push(json!({"phase":"readiness-request-verified","attempt":attempt_count,"elapsedMs":begun.elapsed().as_millis()}));
                        }
                        let response = transact_with_owner(
                            &startup,
                            expected,
                            request,
                            Some(child.identity()),
                        )?;
                        if phases.len() < 24 {
                            phases.push(json!({"phase":"readiness-exchange","attempt":attempt_count,"available":response.is_some(),"elapsedMs":begun.elapsed().as_millis()}));
                        }
                        if response.is_some() {
                            return Ok(());
                        }
                        // This short bounded startup wait never extends the operation
                        // deadline and never probes or takes over an unknown owner.
                        std::thread::sleep(Duration::from_millis(20));
                    }
                })();
                if let Err(error) = ready {
                    let retired = child
                        .terminate_and_wait(Instant::now() + Duration::from_secs(2))
                        .map_err(native);
                    return crate::persistence::finish_control(
                        lycheedev_core::metadata::historical_write::finish_historical(
                            Err(error),
                            retired,
                        ),
                        inv.control.check(),
                    );
                }
                child.handoff().map_err(native)?;
                Ok(())
            })();
            let diagnostic = inv.home.join("tmp/source-broker-launch-last.json");
            let value = json!({"schema":"lycheedev.source-broker-launch-diagnostic.v1","parentPid":std::process::id(),"child":child_identity,"attempts":attempt_count,"phaseTruncated":phases.len()>=24,"phases":phases,"terminal":true,"complete":started.is_ok(),"elapsedMs":begun.elapsed().as_millis(),"error":started.as_ref().err().map(|e|json!({"code":e.code,"exitCode":e.exit_code}))});
            if storage::guard_path(&inv.home, &diagnostic).is_ok() {
                let _ = storage::atomic_json(&diagnostic, &value);
            }
            // Successful server consumes this key transactionally. On failed launch,
            // remove only our unchanged intent, never another launcher's metadata.
            let cleanup = remove_startup_intent(inv, &store_key, generation, &intent);
            lycheedev_core::metadata::historical_write::finish_historical(started, cleanup)
        })();
        crate::persistence::finish_control(
            lycheedev_core::metadata::historical_write::finish_historical(result, launch.close()),
            inv.control.check(),
        )
    })();
    crate::persistence::finish_files(held_identity, result, &inv.control)
}

fn remove_startup_intent(
    inv: &Invocation,
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
                return Err(fault("Startup intent changed during launch"));
            }
            Ok(())
        },
    );
    match result {
        Err(error)
            if matches!(
                error.code.as_str(),
                "vault.record_missing" | "vault.generation_conflict"
            ) && error.details.is_null() =>
        {
            Ok(())
        }
        Err(mut error) => {
            error.details = json!({"schema":"lycheedev.source-persistence.v1","key":key,"generation":generation,"mutation":"consume","state":"unconfirmed","cause":error.clone()}
            );
            Err(error)
        }
        Ok(()) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_admission_is_strict_before_input_or_output_allocation() {
        let mut input = std::io::Cursor::new((REQUEST as u32 + 1).to_le_bytes());
        assert_eq!(
            read_frame::<Request>(&mut input, REQUEST).unwrap_err().code,
            "luals.input_budget"
        );
        let value = json!({"text":"界".repeat(100)});
        let mut output = vec![];
        assert_eq!(
            write_frame(&mut output, &value, 16).unwrap_err().code,
            "luals.input_budget"
        );
        assert!(output.is_empty());
        let raw =
            br#"{"wire":"x","identity":"x","action":"status","deadlineUnixMs":1,"unknown":true}"#;
        let mut framed = (raw.len() as u32).to_le_bytes().to_vec();
        framed.extend(raw);
        assert!(read_frame::<Request>(&mut std::io::Cursor::new(framed), REQUEST).is_err());
    }
    #[test]
    fn request_deadline_does_not_extend_parent_or_accept_stale_time() {
        let parent = OperationControl::new(
            Instant::now() + Duration::from_millis(100),
            Arc::new(AtomicBool::new(false)),
        );
        let mut r = Request {
            wire: WIRE.into(),
            identity: String::new(),
            action: Action::Status,
            deadline_unix_ms: utc_ms().unwrap() + 90_000,
            pin: None,
            environment: None,
            batches: vec![],
        };
        let child = request_control(&r, &parent).unwrap();
        assert!(child.deadline() <= parent.deadline());
        let processing = processing_control(&child);
        assert!(processing.deadline() <= child.deadline());
        assert!(child.deadline().duration_since(processing.deadline()) <= REPLY_RESERVE);
        parent.cancel_handle().store(true, Ordering::Release);
        assert_eq!(child.check().unwrap_err().exit_code, 7);
        assert_eq!(processing.check().unwrap_err().exit_code, 7);
        r.deadline_unix_ms = 1;
        assert!(request_control(&r, &parent).is_err());
    }
}

#[cfg(test)]
mod observer_admission_tests {
    use super::*;
    #[test]
    fn actual_thread_os_admission_failure_preserves_pipe_and_caller_token() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../.tmp/rust-observer-admission-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let scope = BrokerScope::for_home(&root).unwrap();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        let control = OperationControl::default();
        let mut client =
            BrokerStream::connect(&scope, control.deadline(), control.cancel_handle()).unwrap();
        let mut server = listener
            .accept(control.deadline(), control.token())
            .unwrap();
        // Test-only invalid OS stack reservation forces CreateThread rejection;
        // production keeps the standard stack and unchanged2GiB aggregate job.
        let result = DisconnectObserver::start_with_builder(
            &server,
            &control,
            std::thread::Builder::new().stack_size(usize::MAX),
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("OS admitted an impossible stack reservation"),
        };
        assert_eq!(error.code, "luals.input_budget");
        assert_eq!(error.exit_code, 3);
        assert_eq!(error.details["resource"], "disconnectObserverThread");
        assert!(error.details["osCode"].is_number());
        assert!(!control.token().load(Ordering::Acquire));
        server.write_all(b"same-owned-pipe").unwrap();
        let mut body = [0; 15];
        client.read_exact(&mut body).unwrap();
        assert_eq!(&body, b"same-owned-pipe");
        drop(client);
        drop(server);
        drop(listener);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod request_boundary_tests {
    use super::*;

    static NEXT_PIPE: AtomicU64 = AtomicU64::new(0);

    struct NativePipe {
        client: Option<BrokerStream>,
        server: Option<BrokerStream>,
        listener: Option<BrokerListener>,
        root: PathBuf,
        caller: OperationControl,
    }

    impl NativePipe {
        fn open() -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
                "../../.tmp/rust-request-boundary-{}-{}",
                std::process::id(),
                NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let scope = BrokerScope::for_home(&root).unwrap();
            let caller = OperationControl::new(
                Instant::now() + Duration::from_secs(5),
                Arc::new(AtomicBool::new(false)),
            );
            let mut listener = BrokerListener::bind(&scope).unwrap();
            let client =
                BrokerStream::connect(&scope, caller.deadline(), caller.cancel_handle()).unwrap();
            let server = listener.accept(caller.deadline(), caller.token()).unwrap();
            Self {
                client: Some(client),
                server: Some(server),
                listener: Some(listener),
                root,
                caller,
            }
        }
    }

    impl Drop for NativePipe {
        fn drop(&mut self) {
            self.client.take();
            self.server.take();
            self.listener.take();
            let _ = fs::remove_dir(&self.root);
        }
    }

    #[test]
    fn observer_read_deadline_does_not_cancel_a_connected_request() {
        let mut pipe = NativePipe::open();
        let control = pipe.caller.child(
            Instant::now() + Duration::from_millis(40),
            Arc::new(AtomicBool::new(false)),
        );
        let started = Instant::now();
        let mut observer =
            DisconnectObserver::start(pipe.server.as_ref().unwrap(), &control).unwrap();
        // Join the actual native read completion without setting its stop token.
        // The peer remains connected, so only the observer's own deadline fires.
        observer.thread.take().unwrap().join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            !control.token().load(Ordering::Acquire),
            "observer read timeout forged caller cancellation"
        );
        assert_eq!(control.check().unwrap_err().code, "command.deadline");
        assert!(pipe.caller.check().is_ok());
        pipe.server.as_mut().unwrap().write_all(b"live").unwrap();
        let mut bytes = [0; 4];
        pipe.client.as_mut().unwrap().read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"live");
    }

    #[test]
    fn genuine_peer_disconnect_still_cancels_before_request_deadline() {
        let mut pipe = NativePipe::open();
        let control = pipe.caller.child(
            Instant::now() + Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        let mut observer =
            DisconnectObserver::start(pipe.server.as_ref().unwrap(), &control).unwrap();
        drop(pipe.client.take());
        observer.thread.take().unwrap().join().unwrap();
        assert!(
            Instant::now() < control.deadline(),
            "disconnect was only noticed at timeout"
        );
        assert_eq!(control.check().unwrap_err().code, "command.cancelled");
        assert!(pipe.caller.check().is_ok());
    }

    #[test]
    fn explicit_request_cancel_survives_observer_drain_without_cancelling_parent() {
        let mut pipe = NativePipe::open();
        let control = pipe.caller.child(
            Instant::now() + Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        let observer = DisconnectObserver::start(pipe.server.as_ref().unwrap(), &control).unwrap();
        control.cancel_handle().store(true, Ordering::Release);
        drop(observer);
        assert_eq!(control.check().unwrap_err().code, "command.cancelled");
        assert!(pipe.caller.check().is_ok());
        pipe.server.as_mut().unwrap().write_all(b"live").unwrap();
        let mut bytes = [0; 4];
        pipe.client.as_mut().unwrap().read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"live");
    }

    #[test]
    fn expired_request_returns_typed_deadline_without_reopening_business_budget() {
        let mut pipe = NativePipe::open();
        let control = pipe.caller.child(Instant::now(), Arc::new(AtomicBool::new(false)));
        let business_deadline = control.deadline();
        let response = Response {
            state: "partial".into(),
            worker_pid: None,
            batches: 0,
            reused: false,
            analysis: None,
            error: Some(control.check().unwrap_err()),
        };
        pipe.server
            .as_mut()
            .unwrap()
            .set_deadline(control.deadline(), control.cancel_handle());
        let started = Instant::now();
        // The production Analyze error branch already reaped/took the worker.
        // This test exercises its real response boundary, not a LuaLS cleanup proof.
        let mut worker = None;
        complete_response(
            pipe.server.as_mut().unwrap(),
            &response,
            &control,
            &pipe.caller,
            &AtomicBool::new(false),
            &mut worker,
        );
        // Model serve dropping this connection after its response attempt.
        drop(pipe.server.take());
        let reply = read_frame::<Response>(pipe.client.as_mut().unwrap(), RESPONSE)
            .expect("expired business request must return its typed error, not EOF");
        let error = reply.error.expect("deadline response must retain its error");
        assert_eq!(error.code, "command.deadline");
        assert_eq!(error.exit_code, 5);
        assert_eq!(reply.state, "partial");
        assert!(reply.analysis.is_none());
        assert_eq!(control.deadline(), business_deadline);
        assert_eq!(control.check().unwrap_err().code, "command.deadline");
        assert!(pipe.caller.check().is_ok());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "reply cleanup exceeded the bounded fixture interval"
        );
    }

    #[test]
    fn live_request_response_remains_readable_on_same_native_pipe() {
        let mut pipe = NativePipe::open();
        let control = pipe.caller.child(
            Instant::now() + Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        );
        pipe.server
            .as_mut()
            .unwrap()
            .set_deadline(control.deadline(), control.cancel_handle());
        let mut worker = None;
        let response = Response::status(&mut worker);
        complete_response(
            pipe.server.as_mut().unwrap(),
            &response,
            &control,
            &pipe.caller,
            &AtomicBool::new(false),
            &mut worker,
        );
        let reply = read_frame::<Response>(pipe.client.as_mut().unwrap(), RESPONSE).unwrap();
        assert_eq!(reply.state, "idle");
        assert!(reply.error.is_none());
        assert!(control.check().is_ok());
    }

    #[test]
    fn cancelled_business_returns_typed_error_inside_original_transport_deadline() {
        let mut pipe = NativePipe::open();
        let control = processing_control(&pipe.caller);
        let business_deadline = control.deadline();
        let total_deadline = pipe.caller.deadline();
        assert!(business_deadline < total_deadline);
        control.cancel_handle().store(true, Ordering::Release);
        let response = Response {
            state: "partial".into(),
            worker_pid: None,
            batches: 0,
            reused: false,
            analysis: None,
            error: Some(control.check().unwrap_err()),
        };
        pipe.server.as_mut().unwrap().set_deadline(control.deadline(), control.cancel_handle());
        let mut worker = None;
        let started = Instant::now();
        complete_response(pipe.server.as_mut().unwrap(), &response, &control, &pipe.caller, &AtomicBool::new(false), &mut worker);
        let reply = read_frame::<Response>(pipe.client.as_mut().unwrap(), RESPONSE).unwrap();
        let error = reply.error.unwrap();
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.exit_code, 7);
        assert_eq!(reply.state, "partial");
        assert!(reply.analysis.is_none());
        assert_eq!(control.deadline(), business_deadline);
        assert_eq!(pipe.caller.deadline(), total_deadline);
        assert_eq!(control.check().unwrap_err().code, "command.cancelled");
        assert!(pipe.caller.check().is_ok());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn expired_transport_does_not_reopen_for_an_error_reply() {
        let mut pipe = NativePipe::open();
        let response_control = pipe.caller.child(Instant::now(), Arc::new(AtomicBool::new(false)));
        let total_deadline = response_control.deadline();
        let control = processing_control(&response_control);
        let business_deadline = control.deadline();
        let response = Response {
            state: "partial".into(),
            worker_pid: None,
            batches: 0,
            reused: false,
            analysis: None,
            error: Some(control.check().unwrap_err()),
        };
        let mut worker = None;
        let started = Instant::now();
        complete_response(pipe.server.as_mut().unwrap(), &response, &control, &response_control, &AtomicBool::new(false), &mut worker);
        drop(pipe.server.take());
        let mut byte = [0u8; 1];
        assert_eq!(pipe.client.as_mut().unwrap().read(&mut byte).unwrap(), 0, "expired transport must not write any frame bytes");
        assert_eq!(response_control.deadline(), total_deadline);
        assert_eq!(control.deadline(), business_deadline);
        assert_eq!(response_control.check().unwrap_err().code, "command.deadline");
        assert_eq!(control.check().unwrap_err().code, "command.deadline");
        assert!(pipe.caller.check().is_ok());
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
