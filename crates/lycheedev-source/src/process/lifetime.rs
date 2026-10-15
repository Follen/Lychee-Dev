//! Root status, pipe results and joined threads are separate ownership facts.
//! No channel event, failed wait or termination request establishes completion.
use super::{Event, Result, ToolkitError};
use std::{
    io::{Read, Write},
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) const CLEANUP_GRACE: Duration = Duration::from_secs(5);
pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Default)]
pub(super) struct ProcessState {
    pub status: Option<i32>,
    pub outputs: [Option<Vec<u8>>; 2],
    pub input_error: Option<std::io::Error>,
    errors: Vec<String>,
}
impl ProcessState {
    pub fn observe_wait(&mut self, result: std::io::Result<Option<i32>>) -> Result<()> {
        if let Some(status) = result? {
            self.status = Some(status);
        }
        Ok(())
    }
    fn observe_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Input(result) => self.input_error = result.err(),
            Event::Output(index, bytes, result, overflow) => {
                self.outputs[index] = Some(bytes);
                if let Err(error) = result {
                    return Err(ToolkitError::new(
                        "luals.check_failed",
                        format!("Process output reader failed: {error}"),
                        5,
                    ));
                }
                if overflow {
                    return Err(ToolkitError::new(
                        "luals.input_budget",
                        "External process output cap exceeded",
                        3,
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct WorkerSet {
    // stdin, stdout, stderr. None means never started or actually joined, never
    // merely received an event. Failed Builder::spawn drops its owned closure.
    handles: [Option<JoinHandle<Event>>; 3],
}
impl WorkerSet {
    pub fn spawn(
        &mut self,
        index: usize,
        wake: mpsc::Sender<()>,
        work: impl FnOnce() -> Event + Send + 'static,
    ) -> Result<()> {
        debug_assert!(self.handles[index].is_none());
        self.handles[index] = Some(
            thread::Builder::new()
                .name(format!("source-process-{index}"))
                .spawn(move || {
                    let result = work();
                    let _ = wake.send(());
                    result
                })
                .map_err(|error| {
                    ToolkitError::new(
                        "luals.unavailable",
                        format!("Process pipe worker could not start: {error}"),
                        3,
                    )
                })?,
        );
        Ok(())
    }
    pub fn collect(&mut self, state: &mut ProcessState) -> Result<()> {
        let mut first_error = None;
        for slot in &mut self.handles {
            if slot.as_ref().is_some_and(JoinHandle::is_finished) {
                // Never join a still-running worker. is_finished is the std
                // nonblocking readiness check; joining then synchronizes its
                // completed body and owned pipe teardown. This is not a hard
                // realtime bound on OS scheduling or standard-library teardown.
                let result = match slot.take().expect("finished worker").join() {
                    Ok(event) => state.observe_event(event),
                    Err(_) => Err(ToolkitError::new(
                        "luals.check_failed",
                        "Process pipe worker panicked",
                        5,
                    )),
                };
                if let Err(error) = result {
                    record_error(&mut state.errors, error.to_string());
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
    pub fn complete(&self) -> bool {
        self.handles.iter().all(Option::is_none)
    }
    fn input_complete(&self) -> bool {
        self.handles[0].is_none()
    }
    fn output_complete(&self) -> bool {
        self.handles[1..].iter().all(Option::is_none)
    }
}

pub(super) fn write_input(mut pipe: impl Write, input: Vec<u8>) -> Event {
    let result = pipe.write_all(&input);
    drop(pipe);
    drop(input);
    Event::Input(result)
}
pub(super) fn read_output(index: usize, mut pipe: impl Read, cap: usize) -> Event {
    let mut bytes = Vec::new();
    let result = pipe
        .by_ref()
        .take((cap as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map(|_| ());
    drop(pipe);
    let overflow = bytes.len() > cap;
    Event::Output(index, bytes, result, overflow)
}
pub(super) fn wait_for_progress(rx: &mpsc::Receiver<()>, timeout: Duration) {
    if matches!(
        rx.recv_timeout(timeout),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ) {
        // All senders can disappear before a finished thread is joinable, or
        // before a child with closed stdio exits. Do not spin on disconnected.
        thread::sleep(timeout);
    }
}

// This boundary keeps failure/late-completion fixtures on the same retirement
// algorithm. Production always uses its original Child and exact attached Job.
pub(super) trait ProcessOwner {
    fn close_parent_pipes(&mut self);
    fn try_wait(&mut self) -> std::io::Result<Option<i32>>;
    fn terminate_job(&mut self) -> std::result::Result<bool, String>;
    fn kill_root(&mut self) -> std::io::Result<()>;
    fn job_empty(&mut self) -> std::result::Result<Option<bool>, String>;
}
#[derive(Default, Debug)]
pub(super) struct Retirement {
    pub terminated: bool,
    pub root_kill_requested: bool,
    pub reaped: bool,
    pub stdin_complete: bool,
    pub outputs_complete: bool,
    pub job_empty: Option<bool>,
    pub deadline_exceeded: bool,
    pub errors: Vec<String>,
}
impl Retirement {
    pub fn complete(&self) -> bool {
        self.reaped && self.stdin_complete && self.outputs_complete && self.job_empty == Some(true)
    }
    pub fn details(&self) -> serde_json::Value {
        serde_json::json!({
            "terminated": self.terminated,
            "rootKillRequested": self.root_kill_requested,
            "reaped": self.reaped,
            "stdinWriterComplete": self.stdin_complete,
            "outputReadersComplete": self.outputs_complete,
            "jobEmpty": self.job_empty,
            "ownedJobTerminal": self.complete(),
            "cleanupDeadlineExceeded": self.deadline_exceeded,
            "cleanupErrors": self.errors,
            "ownershipScope": "original Child, joined pipe workers and exact post-spawn attached Job; pre-attachment descendants unproven",
            "handleCloseStatus": "standard-library and JobGuard Drop only; checked close unavailable"
        })
    }
}
fn record_error(errors: &mut Vec<String>, error: String) {
    if !errors.contains(&error) {
        errors.push(error);
    }
}
fn observe(
    owner: &mut impl ProcessOwner,
    workers: &mut WorkerSet,
    state: &mut ProcessState,
    cleanup: &mut Retirement,
) {
    if state.status.is_none()
        && let Err(error) = state.observe_wait(owner.try_wait())
    {
        record_error(&mut cleanup.errors, format!("wait: {error}"));
    }
    // collect processes every finished worker even when another worker failed.
    let _ = workers.collect(state);
    for error in &state.errors {
        record_error(&mut cleanup.errors, error.clone());
    }
    cleanup.reaped = state.status.is_some();
    cleanup.stdin_complete = workers.input_complete();
    cleanup.outputs_complete = workers.output_complete();
}
pub(super) fn retire(
    owner: &mut impl ProcessOwner,
    workers: &mut WorkerSet,
    state: &mut ProcessState,
    rx: &mpsc::Receiver<()>,
    deadline: Instant,
) -> Retirement {
    let mut cleanup = Retirement::default();
    owner.close_parent_pipes();
    observe(owner, workers, state, &mut cleanup);
    let mut no_job = false;
    match owner.job_empty() {
        Ok(value) => {
            cleanup.job_empty = value;
            no_job = value.is_none();
        }
        Err(error) => record_error(&mut cleanup.errors, format!("job query: {error}")),
    }
    if cleanup.job_empty != Some(true) {
        match owner.terminate_job() {
            Ok(terminated) => cleanup.terminated = terminated,
            Err(error) => record_error(&mut cleanup.errors, format!("job terminate: {error}")),
        }
    }
    if !cleanup.reaped && !cleanup.terminated {
        match owner.kill_root() {
            Ok(()) => cleanup.root_kill_requested = true,
            Err(error) => record_error(&mut cleanup.errors, format!("root terminate: {error}")),
        }
    }
    loop {
        observe(owner, workers, state, &mut cleanup);
        if !no_job && cleanup.job_empty != Some(true) {
            match owner.job_empty() {
                Ok(value) => {
                    cleanup.job_empty = value;
                    no_job = value.is_none();
                }
                Err(error) => record_error(&mut cleanup.errors, format!("job query: {error}")),
            }
        }
        if cleanup.reaped && workers.complete() && (cleanup.job_empty == Some(true) || no_job) {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            cleanup.deadline_exceeded = true;
            break;
        }
        wait_for_progress(rx, remaining.min(POLL_INTERVAL));
    }
    cleanup
}
