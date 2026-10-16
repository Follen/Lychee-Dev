//! One persistent analysis child owned by the existing native-session owner.
//! Upstream full-batch analysis and AST destruction are synchronous. The owner
//! therefore retains the only termination capability outside that process.
use super::*;
use lycheedev_windows::{JobGuard, ProcessIdentity, guarded_spawn};
use std::{
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread::JoinHandle,
};

const PRIVATE_WIRE: &str = "lycheedev.native-source-analysis.v1";
const START_ENV: &str = "LYCHEEDEV_NATIVE_SOURCE_ANALYSIS_START";
const POLL: Duration = Duration::from_millis(5);

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheState {
    key: String,
    batches: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerHello {
    wire: String,
    token: String,
    identity: ProcessIdentity,
    owner: ProcessIdentity,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerCall {
    wire: String,
    processing_deadline_unix_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerMeta {
    wire: String,
    accepted: bool,
    cached: Option<CacheState>,
}
struct Exchange {
    pipes: (ChildStdin, ChildStdout),
    result: Result<(WorkerMeta, Option<Response>)>,
}

pub(super) struct AnalysisProcess {
    child: Option<Child>,
    job: Option<JobGuard>,
    identity: Option<ProcessIdentity>,
    token: String,
    owner: ProcessIdentity,
    pipes: Option<(ChildStdin, ChildStdout)>,
    exchange: Option<JoinHandle<Exchange>>,
    initialized: bool,
    cached: Option<CacheState>,
    retiring: bool,
    reaped: bool,
    job_empty: bool,
    terminate_requested: bool,
    job_terminate_accepted: bool,
    root_kill_accepted: bool,
    close_errors: Vec<String>,
}
impl AnalysisProcess {
    pub(super) fn key(&self) -> Option<&str> {
        self.cached.as_ref().map(|cache| cache.key.as_str())
    }
    pub(super) fn batches(&self) -> usize {
        self.cached.as_ref().map_or(0, |cache| cache.batches)
    }
    pub(super) fn retiring(&self) -> bool {
        self.retiring
    }
    fn running(&mut self) -> Result<bool> {
        if self.retiring {
            return Ok(false);
        }
        match self
            .child
            .as_mut()
            .ok_or_else(|| fault("Native analysis process missing"))?
            .try_wait()
        {
            Ok(None) => Ok(true),
            Ok(Some(_)) => {
                self.reaped = true;
                self.retiring = true;
                self.cached = None;
                Ok(false)
            }
            Err(error) => {
                self.retiring = true;
                self.cached = None;
                Err(error.into())
            }
        }
    }
    #[cfg(test)]
    fn observation(&self, complete: bool) -> Value {
        json!({"schema":"lycheedev.native-source-analysis-retirement.v1",
            "analysisPid":self.identity.as_ref().map(|value|value.pid),
            "analysisCreated":self.identity.as_ref().map(|value|value.created),
            "ownerPid":self.owner.pid,"ownerCreated":self.owner.created,
            "terminateRequested":self.terminate_requested,"jobTerminateAccepted":self.job_terminate_accepted,
            "rootKillAccepted":self.root_kill_accepted,"rootReaped":self.reaped,
            "pipeWorkerJoined":self.exchange.is_none(),"jobEmpty":self.job_empty,
            "exposedHandleCloseErrors":self.close_errors,"analysisRetired":complete,
            "handleCloseScope":"checked std-exposed process/stdio and analysis child Job; std internal primary-thread handle Drop is unobservable"})
    }

    fn start(
        current: &Identity,
        home: &std::path::Path,
        owner: &ProcessIdentity,
        control: &OperationControl,
        retirement_deadline: Instant,
    ) -> (Option<Self>, Result<()>) {
        let token = match lycheedev_windows::random::token().map_err(platform) {
            Ok(token) => token,
            Err(error) => return (None, Err(error)),
        };
        let mut command = Command::new(&current.path);
        command
            .arg("--source-native-session-worker")
            .arg("--home")
            .arg(home)
            .arg("--owner-pid")
            .arg(owner.pid.to_string())
            .arg("--owner-created")
            .arg(owner.created.to_string())
            .arg("--start-token")
            .arg(&token)
            .env(START_ENV, &token)
            .current_dir(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (child, job, started, startup_close_errors) =
            match guarded_spawn::spawn_suspended_guarded_until(
                command,
                2 << 30,
                control.deadline(),
                retirement_deadline,
                || control.check().is_err(),
            ) {
                Ok((child, job)) => (child, Some(job), Ok(()), Vec::new()),
                Err(mut failure) => {
                    let mut error = control
                        .check()
                        .err()
                        .unwrap_or_else(|| platform(failure.cause.clone()));
                    error.details = json!({"startup":&failure});
                    let startup_close_errors = failure.cleanup.handle_close_errors.clone();
                    let Some((child, job)) = failure.take_pending() else {
                        if !startup_close_errors.is_empty() {
                            let process = Self {
                                child: None,
                                job: None,
                                identity: failure.cleanup.identity,
                                token,
                                owner: owner.clone(),
                                pipes: None,
                                exchange: None,
                                initialized: false,
                                cached: None,
                                retiring: true,
                                reaped: failure.cleanup.child_waited,
                                job_empty: failure
                                    .cleanup
                                    .job_empty
                                    .unwrap_or(failure.cleanup.child_waited),
                                terminate_requested: failure.cleanup.terminate_requested,
                                job_terminate_accepted: false,
                                root_kill_accepted: false,
                                close_errors: startup_close_errors,
                            };
                            return (Some(process), Err(error));
                        }
                        return (None, Err(error));
                    };
                    (child, job, Err(error), startup_close_errors)
                }
            };
        let mut process = Self {
            child: Some(child),
            job,
            identity: None,
            token,
            owner: owner.clone(),
            pipes: None,
            exchange: None,
            initialized: false,
            cached: None,
            retiring: started.is_err(),
            reaped: false,
            job_empty: false,
            terminate_requested: false,
            job_terminate_accepted: false,
            root_kill_accepted: false,
            close_errors: startup_close_errors,
        };
        let result = started.and_then(|()| {
            process.identity = Some(
                guarded_spawn::child_identity(process.child.as_ref().unwrap()).map_err(platform)?,
            );
            let child = process.child.as_mut().unwrap();
            let input = child
                .stdin
                .take()
                .ok_or_else(|| fault("Native analysis stdin missing"))?;
            let Some(output) = child.stdout.take() else {
                child.stdin = Some(input);
                return Err(fault("Native analysis stdout missing"));
            };
            process.pipes = Some((input, output));
            Ok(())
        });
        (Some(process), result)
    }

    fn begin(&mut self, raw: Vec<u8>, deadline_unix_ms: u64) -> Result<()> {
        if self.retiring || self.exchange.is_some() {
            return Err(fault("Native analysis custody is not ready"));
        }
        let expected = self
            .identity
            .clone()
            .ok_or_else(|| fault("Native analysis identity missing"))?;
        let (input, output) = self
            .pipes
            .take()
            .ok_or_else(|| fault("Native analysis pipes missing"))?;
        let owner = self.owner.clone();
        let token = self.token.clone();
        let initialized = self.initialized;
        let transfer = Arc::new(std::sync::Mutex::new(Some((input, output, raw))));
        let thread_transfer = transfer.clone();
        let handle = std::thread::Builder::new()
            .name("source-native-analysis-io".into())
            .spawn(move || {
                let (mut input, mut output, raw) = thread_transfer
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                    .expect("single owned analysis transfer");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if !initialized {
                        let hello: WorkerHello = read_frame(&mut output, 4096)?;
                        if hello.wire != PRIVATE_WIRE
                            || hello.token != token
                            || hello.identity != expected
                            || hello.owner != owner
                        {
                            return Err(fault("Native analysis child identity differs"));
                        }
                    }
                    write_frame(
                        &mut input,
                        &WorkerCall {
                            wire: PRIVATE_WIRE.into(),
                            processing_deadline_unix_ms: deadline_unix_ms,
                        },
                        4096,
                    )?;
                    write_raw_frame(&mut input, &raw)?;
                    // The supervisor never decodes or retains a second Snapshot.
                    // Release the transferred raw input before waiting for analysis.
                    drop(raw);
                    let meta: WorkerMeta = read_frame(&mut output, 4096)?;
                    if meta.wire != PRIVATE_WIRE {
                        return Err(fault("Native analysis reply wire differs"));
                    }
                    let response = if meta.accepted {
                        Some(read_frame(&mut output, RESPONSE_BYTES)?)
                    } else {
                        None
                    };
                    Ok((meta, response))
                }))
                .unwrap_or_else(|_| Err(fault("Native analysis pipe operation panicked")));
                Exchange {
                    pipes: (input, output),
                    result,
                }
            });
        match handle {
            Ok(handle) => self.exchange = Some(handle),
            Err(error) => {
                let (input, output, raw) = transfer
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                    .expect("unstarted owned analysis transfer");
                self.pipes = Some((input, output));
                drop(raw);
                return Err(error.into());
            }
        }
        Ok(())
    }
    fn complete_exchange(&mut self) -> Option<Result<(WorkerMeta, Option<Response>)>> {
        if !self.exchange.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        Some(match self.exchange.take().unwrap().join() {
            Ok(exchange) => {
                self.pipes = Some(exchange.pipes);
                self.initialized = true;
                exchange.result
            }
            Err(_) => {
                self.close_errors
                    .push("pipe worker panicked before checked exposed-pipe close".into());
                Err(fault("Native analysis pipe worker panicked"))
            }
        })
    }
    fn close_pipes(&mut self) {
        if let Some((input, output)) = self.pipes.take() {
            for result in [
                guarded_spawn::close_owned_handle(input),
                guarded_spawn::close_owned_handle(output),
            ] {
                if let Err(error) = result {
                    self.close_errors.push(error);
                }
            }
        }
    }
    /// No fresh grace period. A termination request, root exit, empty Job, joined
    /// pipe thread and checked handle closes are distinct observations.
    fn retire(&mut self, deadline: Instant) -> Result<()> {
        self.retiring = true;
        self.cached = None;
        self.close_pipes();
        let mut errors = Vec::new();
        if !self.job_terminate_accepted
            && let Some(job) = &self.job
        {
            self.terminate_requested = true;
            match job.terminate() {
                Ok(()) => self.job_terminate_accepted = true,
                Err(error) => errors.push(error),
            }
        }
        if !self.job_terminate_accepted
            && !self.root_kill_accepted
            && let Some(child) = &mut self.child
        {
            match child.try_wait() {
                Ok(Some(_)) => self.reaped = true,
                _ => {
                    self.terminate_requested = true;
                    match child.kill() {
                        Ok(()) => self.root_kill_accepted = true,
                        Err(error) => errors.push(error.to_string()),
                    }
                }
            }
        }
        loop {
            if !self.reaped
                && let Some(child) = &mut self.child
            {
                match child.try_wait() {
                    Ok(Some(_)) => self.reaped = true,
                    Ok(None) => {}
                    Err(error) => {
                        if !errors.contains(&error.to_string()) {
                            errors.push(error.to_string());
                        }
                    }
                }
            }
            if self.complete_exchange().is_some() {
                self.close_pipes();
            }
            if !self.job_empty {
                match &self.job {
                    Some(job) => match job.memory_accounting() {
                        Ok(memory) => self.job_empty = memory.active_processes == 0,
                        Err(error) => {
                            if !errors.contains(&error) {
                                errors.push(error);
                            }
                        }
                    },
                    // The only Job-less custody is a failed suspended launch;
                    // it was never resumed and cannot have made descendants.
                    None => self.job_empty = self.reaped,
                }
            }
            if self.reaped && self.exchange.is_none() && self.job_empty {
                if let Some(child) = self.child.take() {
                    if let Err(error) = guarded_spawn::close_child(child) {
                        self.close_errors.push(error);
                    }
                }
                if let Some(job) = self.job.take() {
                    if let Err(error) = job.close() {
                        self.close_errors.push(error);
                    }
                }
                if self.close_errors.is_empty() {
                    return Ok(());
                }
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            std::thread::sleep(remaining.min(POLL));
        }
        let mut error = fault("Native analysis retirement is unconfirmed");
        error.details = json!({"analysisPid":self.identity.as_ref().map(|value|value.pid),
            "analysisCreated":self.identity.as_ref().map(|value|value.created),
            "terminateRequested":self.terminate_requested,"jobTerminateAccepted":self.job_terminate_accepted,
            "rootKillAccepted":self.root_kill_accepted,"rootReaped":self.reaped,
            "pipeWorkerJoined":self.exchange.is_none(),"jobEmpty":self.job_empty,
            "handleCloseErrors":self.close_errors,"cleanupErrors":errors,
            "cleanupDeadlineExceeded":Instant::now()>=deadline,"analysisRetired":false});
        Err(error)
    }
}

pub(super) fn retire_slot(slot: &mut Option<AnalysisProcess>, deadline: Instant) -> Result<()> {
    if let Some(process) = slot {
        let result = process.retire(deadline);
        // Retirement is necessary work, but writing a diagnostic is not.
        // Never insert synchronous disk operations before the response or next
        // admission. Pending custody and its exact facts stay in this slot and
        // the existing error; confirmed success releases the checked handles.
        result?;
    }
    *slot = None;
    Ok(())
}
pub(super) fn refresh(slot: &mut Option<AnalysisProcess>, deadline: Instant) -> Result<()> {
    if let Some(process) = slot
        && !process.running()?
    {
        // A pending close remains explicit status=retiring, never ready/idle.
        let _ = retire_slot(slot, deadline);
    }
    Ok(())
}
fn with_retirement(mut error: ToolkitError, retired: Result<()>) -> ToolkitError {
    // A clean retirement adds no fields to the existing public business/error
    // result. Uncertainty and its in-memory custody facts must still be returned
    // alongside the original primary error, never replace its cause.
    if let Err(retirement) = retired {
        let original = std::mem::take(&mut error.details);
        error.details = if original.is_object() {
            original
        } else {
            json!({"originalDetails":original})
        };
        error.details["analysisRetirementError"] = json!(retirement);
    }
    error
}

/// None means a malformed typed request, not admitted work. The owner closes
/// that connection without resetting idle or discarding a valid retained cache.
pub(super) fn query(
    slot: &mut Option<AnalysisProcess>,
    raw: Vec<u8>,
    identity_only_key: Option<&str>,
    current: &Identity,
    home: &std::path::Path,
    owner: &ProcessIdentity,
    control: &OperationControl,
    retirement_deadline: Instant,
) -> Result<Option<Response>> {
    let result = (|| {
        control.check()?;
        if slot.as_ref().is_some_and(AnalysisProcess::retiring) {
            return Err(fault("Native analysis retirement is still pending"));
        }
        if identity_only_key
            .is_some_and(|key| slot.as_ref().and_then(AnalysisProcess::key) != Some(key))
        {
            return Err(ToolkitError::new(
                "source.native_snapshot_missing",
                "Native Source snapshot is not cached",
                3,
            ));
        }
        if slot.is_none() {
            let (process, started) =
                AnalysisProcess::start(current, home, owner, control, retirement_deadline);
            *slot = process;
            started?;
        }
        control.check()?;
        let process = slot.as_mut().unwrap();
        // UTC precedes the monotonic remainder: sampling overhead shortens this
        // cutoff. The owner independently enforces its exact monotonic deadline.
        let now_unix_ms = utc_ms()?;
        let remaining = control.deadline().saturating_duration_since(Instant::now());
        let deadline_unix_ms = now_unix_ms.saturating_add(remaining.as_millis() as u64);
        process.begin(raw, deadline_unix_ms)?;
        loop {
            control.check()?;
            if let Some(reply) = process.complete_exchange() {
                let (meta, response) = reply?;
                control.check()?;
                process.cached = meta.cached;
                if let Some(response) = &response
                    && let Some(error) = &response.error
                {
                    return Err(error.clone());
                }
                return Ok(response);
            }
            if process.child.as_mut().unwrap().try_wait()?.is_some() {
                process.reaped = true;
                return Err(fault("Native analysis child exited before its response"));
            }
            std::thread::sleep(
                control
                    .deadline()
                    .saturating_duration_since(Instant::now())
                    .min(POLL),
            );
        }
    })();
    match result {
        Ok(response) => {
            if slot.as_ref().is_some_and(|process| process.key().is_none()) {
                retire_slot(slot, retirement_deadline)?;
            }
            Ok(response)
        }
        Err(error) => Err(with_retirement(
            error,
            retire_slot(slot, retirement_deadline),
        )),
    }
}

/// Private same-executable entry. The live parent identity and private inherited
/// pipes authenticate this child; it never binds a second public endpoint.
pub(super) fn worker(args: &[String]) -> Result<()> {
    let accepted = ["--home", "--owner-pid", "--owner-created", "--start-token"];
    if args.len() != 9 || args[0] != "--source-native-session-worker" {
        return Err(ToolkitError::arguments(
            "invalid private native analysis startup",
        ));
    }
    let mut flags = BTreeMap::new();
    for pair in args[1..].as_chunks::<2>().0 {
        if !accepted.contains(&pair[0].as_str())
            || pair[1].is_empty()
            || flags.insert(pair[0].clone(), pair[1].clone()).is_some()
        {
            return Err(ToolkitError::arguments(
                "invalid private native analysis arguments",
            ));
        }
    }
    let token = &flags["--start-token"];
    if !lycheedev_core::selection::valid_digest(token, 32)
        || std::env::var(START_ENV).ok().as_ref() != Some(token)
    {
        return Err(fault("Native analysis startup token differs"));
    }
    let owner = ProcessIdentity {
        pid: flags["--owner-pid"]
            .parse()
            .map_err(|_| fault("Invalid native analysis owner PID"))?,
        created: flags["--owner-created"]
            .parse()
            .map_err(|_| fault("Invalid native analysis owner creation"))?,
    };
    let home = PathBuf::from(&flags["--home"]);
    if !home.is_absolute() {
        return Err(fault("Native analysis home must be absolute"));
    }
    let own = Process::open(std::process::id()).map_err(platform)?;
    let parent = Process::open(owner.pid).map_err(platform)?;
    if own.parent_pid().map_err(platform)? != owner.pid
        || parent.identity() != &owner
        || parent.image_path().map_err(platform)?.canonicalize()?
            != std::env::current_exe()?.canonicalize()?
    {
        return Err(fault("Native analysis actual owner differs"));
    }
    parent.verify().map_err(platform)?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    write_frame(
        &mut output,
        &WorkerHello {
            wire: PRIVATE_WIRE.into(),
            token: token.clone(),
            identity: own.identity().clone(),
            owner: owner.clone(),
        },
        4096,
    )?;
    let mut cached: Option<Cached> = None;
    loop {
        let call: WorkerCall = read_frame(&mut input, 4096)?;
        if call.wire != PRIVATE_WIRE {
            return Err(fault("Native analysis request wire differs"));
        }
        let raw = read_raw_frame(&mut input, REQUEST_BYTES)?;
        let request: Request = match serde_json::from_slice(&raw) {
            Ok(request) => request,
            Err(_) => {
                drop(raw);
                write_frame(
                    &mut output,
                    &WorkerMeta {
                        wire: PRIVATE_WIRE.into(),
                        accepted: false,
                        cached: cache_state(&cached),
                    },
                    4096,
                )?;
                continue;
            }
        };
        drop(raw);
        if request.wire != WIRE || !matches!(request.action, Action::Query) {
            write_frame(
                &mut output,
                &WorkerMeta {
                    wire: PRIVATE_WIRE.into(),
                    accepted: false,
                    cached: cache_state(&cached),
                },
                4096,
            )?;
            continue;
        }
        let observed = Instant::now();
        let now = utc_ms()?;
        let cutoff = call
            .processing_deadline_unix_ms
            .min(request.deadline_unix_ms);
        let deadline = observed
            .checked_add(Duration::from_millis(cutoff.saturating_sub(now)))
            .ok_or_else(|| fault("Native analysis deadline range"))?;
        let control = OperationControl::uncancelable(deadline);
        let begun = Instant::now();
        let diagnostic = home.join("tmp/source-native-last-request.json");
        let request_deadline_unix_ms = request.deadline_unix_ms;
        let record = |phase: &str, error: Option<&ToolkitError>| {
            // A diagnostic can fail or be absent. Its synchronous filesystem
            // calls live only in this killable child, under the owner's original
            // processing cutoff; they can never stall the IPC owner itself.
            observe_if_active(&control, || {
                let value = json!({
                "schema":"lycheedev.native-source-request-diagnostic.v1", "workerPid":owner.pid,
                "workerCreated":owner.created,"analysisPid":own.identity().pid,
                "analysisCreated":own.identity().created,"requestDeadlineUnixMs":request_deadline_unix_ms,"phase":phase,
                "elapsedMs":begun.elapsed().as_millis(),"processMemory":own.memory().ok(),"error":error});
                storage::atomic_write_controlled(&diagnostic, &serde_json::to_vec(&value)?, &control)
            });
        };
        let result = execute_cached_query(
            &mut cached,
            request.work,
            request.cached,
            &control,
            owner.pid,
            record,
        );
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
        write_frame(
            &mut output,
            &WorkerMeta {
                wire: PRIVATE_WIRE.into(),
                accepted: true,
                cached: cache_state(&cached),
            },
            4096,
        )?;
        write_frame(&mut output, &response, RESPONSE_BYTES)?;
    }
}
// Optional phase observation is never started after its original control has
// stopped. A single filesystem syscall is still not interruptible; the owner
// bounds it by retaining the termination capability outside this process.
fn observe_if_active(control: &OperationControl, observe: impl FnOnce() -> Result<()>) {
    if control.check().is_ok() {
        let _ = observe();
    }
}

fn cache_state(cached: &Option<Cached>) -> Option<CacheState> {
    cached.as_ref().map(|cache| CacheState {
        key: cache.key.clone(),
        batches: cache.admission.batches,
    })
}

#[cfg(test)]
#[path = "analysis_process_tests.rs"]
mod tests;
