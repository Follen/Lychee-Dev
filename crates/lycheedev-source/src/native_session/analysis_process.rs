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

// The optional journal is owned only by this killable analysis child. Each
// bounded sample is appended once through its CREATE_NEW handle, without fsync.
// Original coarse diagnostics retain their existing atomic publication path.
const PHASE_SAMPLE_LIMIT: usize = 128;
const PHASE_DETAIL_LIMIT: usize = 24;
const PHASE_FINAL_RESERVE: usize = 8;
const PHASE_LINE_LIMIT: usize = 2048;
const PHASE_BYTE_LIMIT: usize = 128 * 1024;
const PHASE_FILE_SLOTS: usize = 32;
#[derive(Default)]
struct PhaseSampling {
    callbacks: usize, records: usize, details: usize, omitted: usize, file_visits: usize,
}
fn final_phase(phase: &str) -> bool {
    matches!(phase,"query_assemble_reply"|"query_reply_assembled"|"retiring_analysis"|"analysis_retired")
}
fn file_phase(phase: &str) -> bool {
    matches!(phase,"query_relations_file_syntax"|"query_relations_reference_rows"|"query_relations_file_complete")
}
impl PhaseSampling {
    fn admit(&mut self, phase:&str, bytes:usize) -> bool {
        self.callbacks=self.callbacks.saturating_add(1);
        if phase=="query_relations_file_syntax" {self.file_visits=self.file_visits.saturating_add(1);}
        let final_stage=final_phase(phase);
        let record_limit=if final_stage{PHASE_SAMPLE_LIMIT}else{PHASE_SAMPLE_LIMIT-PHASE_FINAL_RESERVE};
        let byte_limit=if final_stage{PHASE_BYTE_LIMIT}else{PHASE_BYTE_LIMIT-PHASE_FINAL_RESERVE*PHASE_LINE_LIMIT};
        if phase.len()>64||self.records>=record_limit||bytes.saturating_add(PHASE_LINE_LIMIT)>byte_limit
            ||file_phase(phase)&&self.details>=PHASE_DETAIL_LIMIT {
            self.omitted=self.omitted.saturating_add(1);return false;
        }
        self.records+=1;if file_phase(phase){self.details+=1;}true
    }
}
struct PhaseJournal {
    file: Option<File>, name:String, state:&'static str, bytes:usize, sampling:PhaseSampling,
    previous_memory:Option<(u64,u64)>, previous_callback:Option<usize>,
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct PhaseEvent<'a> {
    schema:&'static str,worker_pid:u32,worker_created:u64,analysis_pid:u32,analysis_created:u64,
    request_deadline_unix_ms:u64,phase:&'a str,elapsed_ms:u128,
    sequence:usize,callback_ordinal:usize,file_visit:usize,previous_callback_ordinal:Option<usize>,
    omitted_before_this_sample:usize,detail_sampling_exhausted:bool,ordinary_sampling_exhausted:bool,
    process_memory:Option<&'a lycheedev_windows::process::ProcessMemory>,
    private_commit_delta:Option<i64>,working_set_delta:Option<i64>,
}
impl PhaseJournal {
    fn open(home:&std::path::Path,own:&ProcessIdentity,control:&OperationControl)->Self {
        // Fixed home-wide admission, including other sessions and restarts. Never
        // adopt, truncate or remove an occupied slot, even after its writer exits.
        let opened=(||->Result<Option<(File,String)>>{
            control.check()?;std::fs::create_dir_all(home.join("tmp"))?;control.check()?;
            let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1); // Readers may observe a prefix; no second writer/deleter.
            for offset in 0..PHASE_FILE_SLOTS {
                control.check()?;
                let slot=(own.pid as usize%PHASE_FILE_SLOTS+offset)%PHASE_FILE_SLOTS;
                let name=format!("source-native-phases-{slot:02}.jsonl");
                match options.open(home.join("tmp").join(&name)) {
                    Ok(file)=>{control.check()?;return Ok(Some((file,name)));}
                    Err(error) if error.kind()==std::io::ErrorKind::AlreadyExists=>continue,
                    Err(error)=>return Err(error.into()),
                }
            }
            Ok(None)
        })();
        let (file,name,state)=match opened {
            Ok(Some((file,name)))=>(Some(file),name,"opened"),
            Ok(None)=>(None,String::new(),"slots_exhausted"),
            Err(_)=>(None,String::new(),"unavailable"),
        };
        Self{file,name,state,bytes:0,sampling:PhaseSampling::default(),previous_memory:None,previous_callback:None}
    }
    fn admit(&mut self,phase:&str)->bool {
        // After disable/saturation the hot callbacks do no memory sampling,
        // serialization or filesystem work. Only bounded integer counters move.
        self.file.is_some()&&self.sampling.admit(phase,self.bytes)
    }
    fn metadata(&self)->Value {
        json!({"file":if self.name.is_empty(){None}else{Some(self.name.as_str())},"state":self.state,"recordsAttempted":self.sampling.records,"bytes":self.bytes,
            "recordLimit":PHASE_SAMPLE_LIMIT,"byteLimit":PHASE_BYTE_LIMIT,"lineLimit":PHASE_LINE_LIMIT,
            "detailLimit":PHASE_DETAIL_LIMIT,"finalReserve":PHASE_FINAL_RESERVE,
            "homeFileSlots":PHASE_FILE_SLOTS,"homeByteLimit":PHASE_FILE_SLOTS*PHASE_BYTE_LIMIT,
            "scope":"same analysis child; samples precede stages; deltas include observer work and skipped callbacks; fileVisit counts eligible file traversals; OS-buffered append without durability or completion claim"})
    }
    fn append(&mut self,phase:&str,elapsed_ms:u128,memory:Option<&lycheedev_windows::process::ProcessMemory>,
        owner:&ProcessIdentity,own:&ProcessIdentity,deadline:u64,control:&OperationControl) {
        let current=memory.map(|memory|(memory.private_commit,memory.working_set));
        let delta=|working_set:bool|self.previous_memory.zip(current).and_then(|(before,after)|{
            let (before,after)=if working_set{(before.1,after.1)}else{(before.0,after.0)};
            i64::try_from(i128::from(after)-i128::from(before)).ok()
        });
        let event=PhaseEvent{schema:"lycheedev.native-source-request-phase.v1",worker_pid:owner.pid,worker_created:owner.created,
            analysis_pid:own.pid,analysis_created:own.created,request_deadline_unix_ms:deadline,phase,elapsed_ms,
            sequence:self.sampling.records-1,callback_ordinal:self.sampling.callbacks,file_visit:self.sampling.file_visits,
            previous_callback_ordinal:self.previous_callback,omitted_before_this_sample:self.sampling.omitted,
            detail_sampling_exhausted:self.sampling.details>=PHASE_DETAIL_LIMIT,
            ordinary_sampling_exhausted:self.sampling.records>=PHASE_SAMPLE_LIMIT-PHASE_FINAL_RESERVE,
            process_memory:memory,private_commit_delta:delta(false),working_set_delta:delta(true)};
        let result=(||->Result<()>{
            control.check()?;
            // Fixed stack storage; no retained history or growable JSON buffer.
            let mut bytes=[0u8;PHASE_LINE_LIMIT];
            let mut cursor=std::io::Cursor::new(&mut bytes[..PHASE_LINE_LIMIT-1]);
            serde_json::to_writer(&mut cursor,&event)?;
            let length=cursor.position() as usize;bytes[length]=b'\n';let length=length+1;
            if self.bytes.saturating_add(length)>PHASE_BYTE_LIMIT{return Err(fault("Private phase journal byte limit"));}
            self.file.as_mut().expect("admitted phase journal").write_all(&bytes[..length])?;
            self.bytes+=length;control.check()
        })();
        self.previous_memory=current;self.previous_callback=Some(self.sampling.callbacks);
        if result.is_err(){self.state="write_stopped";self.file=None;}
    }
}

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
impl WorkerHello {
    fn authenticate(&self, expected: &ProcessIdentity, owner: &ProcessIdentity, token: &str) -> Result<()> {
        if self.wire != PRIVATE_WIRE || self.token != token || &self.identity != expected || &self.owner != owner {
            return Err(fault("Native analysis child identity differs"));
        }
        Ok(())
    }
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
    result: Result<(WorkerMeta, Option<RawResponse>)>,
    authenticated: bool,
}

/// Read-only terminal facts from the supervisor's existing process/Job handles.
/// A cleanup exit is explicitly distinct from an exit seen before termination.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AnalysisTerminal {
    worker_pid: u32,
    worker_created: u64,
    analysis_pid: Option<u32>,
    analysis_created: Option<u64>,
    child_hello_authenticated: bool,
    exit: Option<ChildExit>,
    exit_wait_error: Option<String>,
    job_memory: Option<JobObservation>,
    job_memory_error: Option<String>,
    terminate_requested: bool,
    job_terminate_accepted: bool,
    root_kill_accepted: bool,
    root_reaped: bool,
    pipe_worker_joined: bool,
    job_empty: bool,
    analysis_retired: bool,
    handle_close_errors: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChildExit {
    code: Option<u32>,
    observed_before_termination: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JobObservation {
    peak_job_memory_used: u64,
    peak_process_memory_used: u64,
    job_memory_limit: u64,
    active_processes: u32,
    limit_flags: u32,
}
impl AnalysisTerminal {
    pub(super) fn event(&self, hello: &Hello, request_deadline_unix_ms: u64) -> Option<Value> {
        if self.worker_pid != hello.pid || self.worker_created != hello.created {
            return None;
        }
        Some(json!({"schema":"lycheedev.native-source-analysis-terminal.v1",
            "authentication":"same_executable_authenticated_owner_response",
            "requestDeadlineUnixMs":request_deadline_unix_ms,"terminal":self,
            "exitScope":"exact guarded child handle; cleanup exit does not establish original failure cause",
            "jobScope":"exact held analysis Job; historical commit peaks, not current aggregate or RSS",
            "limitNotification":"unavailable_not_observed"}))
    }
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
    exit: Option<ChildExit>,
    exit_wait_error: Option<String>,
    job_observation: Option<JobObservation>,
    job_memory_error: Option<String>,
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
        match self.poll_exit() {
            Ok(false) => Ok(true),
            Ok(true) => {
                self.retiring = true;
                self.cached = None;
                Ok(false)
            }
            Err(error) => {
                self.retiring = true;
                self.cached = None;
                Err(error)
            }
        }
    }
    fn poll_exit(&mut self) -> Result<bool> {
        if self.reaped { return Ok(true); }
        let waited = self.child.as_mut().ok_or_else(|| fault("Native analysis process missing"))?.try_wait();
        match waited {
            Ok(Some(status)) => {
                self.exit = Some(ChildExit { code: status.code().map(|code| code as u32), observed_before_termination: !self.terminate_requested });
                self.reaped = true;
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(error) => { self.exit_wait_error = Some(error.to_string()); Err(error.into()) }
        }
    }
    fn terminal(&self, complete: bool) -> AnalysisTerminal {
        AnalysisTerminal {
            worker_pid:self.owner.pid, worker_created:self.owner.created,
            analysis_pid:self.identity.as_ref().map(|identity|identity.pid),
            analysis_created:self.identity.as_ref().map(|identity|identity.created),
            child_hello_authenticated:self.initialized, exit:self.exit.clone(),
            exit_wait_error:self.exit_wait_error.clone(), job_memory:self.job_observation.clone(),
            job_memory_error:self.job_memory_error.clone(), terminate_requested:self.terminate_requested,
            job_terminate_accepted:self.job_terminate_accepted, root_kill_accepted:self.root_kill_accepted,
            root_reaped:self.reaped, pipe_worker_joined:self.exchange.is_none(), job_empty:self.job_empty,
            analysis_retired:complete, handle_close_errors:self.close_errors.clone(),
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
                                exit: None, exit_wait_error: None, job_observation: None, job_memory_error: None,
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
            exit: None, exit_wait_error: None, job_observation: None, job_memory_error: None,
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
                let mut authenticated = initialized;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if !initialized {
                        let hello: WorkerHello = read_frame(&mut output, 4096)?;
                        hello.authenticate(&expected, &owner, &token)?;
                        authenticated = true;
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
                        Some(RawResponse::decode(read_raw_frame(&mut output, RESPONSE_BYTES)?)?)
                    } else {
                        None
                    };
                    Ok((meta, response))
                }))
                .unwrap_or_else(|_| Err(fault("Native analysis pipe operation panicked")));
                Exchange {
                    pipes: (input, output),
                    result,
                    authenticated,
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
    fn complete_exchange(&mut self) -> Option<Result<(WorkerMeta, Option<RawResponse>)>> {
        if !self.exchange.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        Some(match self.exchange.take().unwrap().join() {
            Ok(exchange) => {
                self.pipes = Some(exchange.pipes);
                self.initialized = exchange.authenticated;
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
        let mut errors = Vec::new();
        // EOF can win the race with the supervision loop. Observe the existing
        // child before requesting cleanup so an original status is not lost or
        // retrospectively mislabeled as a termination-induced status.
        if !self.reaped && self.child.is_some() {
            // A failed optional early read is retained in terminal evidence;
            // the existing retirement loop still owns public cleanup errors.
            let _ = self.poll_exit();
        }
        self.close_pipes();
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
            && self.child.is_some()
        {
            match self.poll_exit() {
                Ok(true) => {},
                _ => {
                    self.terminate_requested = true;
                    match self.child.as_mut().unwrap().kill() {
                        Ok(()) => self.root_kill_accepted = true,
                        Err(error) => errors.push(error.to_string()),
                    }
                }
            }
        }
        loop {
            if !self.reaped
                && self.child.is_some()
            {
                match self.poll_exit() {
                    Ok(_) => {},
                    Err(error) => {
                        if !errors.contains(&error.message) {
                            errors.push(error.message);
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
                        Ok(memory) => {
                            self.job_empty = memory.active_processes == 0;
                            self.job_observation = Some(JobObservation {
                                peak_job_memory_used:memory.peak_job_memory_used,
                                peak_process_memory_used:memory.peak_process_memory_used,
                                job_memory_limit:memory.job_memory_limit, active_processes:memory.active_processes,
                                limit_flags:memory.limit_flags,
                            });
                        },
                        Err(error) => {
                            self.job_memory_error = Some(error.clone());
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
    retire_slot_observed(slot, deadline).0
}
fn retire_slot_observed(slot: &mut Option<AnalysisProcess>, deadline: Instant) -> (Result<()>, Option<AnalysisTerminal>) {
    let mut terminal = None;
    if let Some(process) = slot {
        let result = process.retire(deadline);
        // Retirement is necessary work, but writing a diagnostic is not.
        // Never insert synchronous disk operations before the response or next
        // admission. Pending custody and its exact facts stay in this slot and
        // the existing error; confirmed success releases the checked handles.
        terminal = Some(process.terminal(result.is_ok()));
        if result.is_err() { return (result, terminal); }
    }
    *slot = None;
    (Ok(()), terminal)
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
    terminal: &mut Option<AnalysisTerminal>,
) -> Result<Option<OwnerReply>> {
    *terminal = None;
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
                return Ok(response);
            }
            if process.poll_exit()? {
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
            if response.as_ref().is_some_and(RawResponse::failed) {
                let (retired, observed)=retire_slot_observed(slot,retirement_deadline);
                return Ok(response.map(|response|OwnerReply::ChildError{response,retirement:retired.err(),terminal:observed}));
            }
            if slot.as_ref().is_some_and(|process| process.key().is_none()) {
                let (retired, observed) = retire_slot_observed(slot, retirement_deadline);
                if let Err(error) = retired { *terminal = observed; return Err(error); }
            }
            Ok(response.map(OwnerReply::Forward))
        }
        Err(error) => {
            let (retired, observed) = retire_slot_observed(slot, retirement_deadline);
            *terminal = observed;
            Err(with_retirement(error, retired))
        },
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
        let journal = std::cell::RefCell::new(PhaseJournal::open(&home,own.identity(),&control));
        let record = |phase: &str, error: Option<&ToolkitError>| {
            // All optional filesystem work stays in this killable child under
            // the original processing cutoff. The IPC owner never writes here.
            observe_if_active(&control, || {
                let coarse=!crate::native_research::allocation_phase(phase);
                let mut journal=journal.borrow_mut();
                let sampled=journal.admit(phase);
                if !coarse&&!sampled{return Ok(());}
                let elapsed=begun.elapsed().as_millis();let memory=own.memory().ok();
                if sampled{journal.append(phase,elapsed,memory.as_ref(),&owner,own.identity(),request_deadline_unix_ms,&control);}
                if coarse {
                    let value=json!({"schema":"lycheedev.native-source-request-diagnostic.v1","workerPid":owner.pid,
                        "workerCreated":owner.created,"analysisPid":own.identity().pid,"analysisCreated":own.identity().created,
                        "requestDeadlineUnixMs":request_deadline_unix_ms,"phase":phase,"elapsedMs":elapsed,
                        "processMemory":memory,"error":error,"phaseJournal":journal.metadata()});
                    storage::atomic_write_controlled(&diagnostic,&serde_json::to_vec(&value)?,&control)?;
                }
                Ok(())
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
                terminal: None,
            },
            Err(error) => Response {
                value: None,
                error: Some(error),
                terminal: None,
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
