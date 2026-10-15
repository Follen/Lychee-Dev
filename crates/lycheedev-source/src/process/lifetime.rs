//! Root status, pipe results and joined threads are separate ownership facts.
//! No channel event, failed wait or termination request establishes completion.
use super::{Event, ProcessControl, Result, ToolkitError};
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
/// Reuse the exact reader cap/Event contract; only stderr can enter here.
pub(super) fn read_output_observed(
    pipe: impl Read,
    cap: usize,
    observation: super::StderrObservation,
) -> Event {
    read_output(
        1,
        super::stderr_observation::ObservedReader {
            reader: pipe,
            observation,
        },
        cap,
    )
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
    pub first_worker_error: Option<ToolkitError>,
    pub completion_error: Option<ToolkitError>,
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
    completion: &mut Option<(ProcessControl<'_>, Instant)>,
) {
    if state.status.is_none()
        && let Err(error) = state.observe_wait(owner.try_wait())
    {
        record_error(&mut cleanup.errors, format!("wait: {error}"));
    }
    // Root reap moved this pending-worker interval out of RUN. Preserve its
    // control-before-collect boundary, without cancelling ownership cleanup.
    if let Some((control, deadline)) = *completion {
        let result = control.check().and_then(|()| {
            if Instant::now() >= deadline {
                Err(control.expired())
            } else {
                Ok(())
            }
        });
        if let Err(error) = result {
            cleanup.completion_error.get_or_insert(error);
            *completion = None;
        }
    }
    // collect processes every finished worker even when another worker failed.
    if let Err(error) = workers.collect(state) {
        if completion.is_some() {
            cleanup
                .completion_error
                .get_or_insert_with(|| error.clone());
            *completion = None;
        }
        cleanup.first_worker_error.get_or_insert(error);
    }
    for error in &state.errors {
        record_error(&mut cleanup.errors, error.clone());
    }
    if workers.complete() {
        if let Some((control, _)) = *completion {
            // This is the old RUN completion boundary: select a final control
            // stop or successful-root stdin failure before later Job cleanup.
            let result = control.check_complete().and_then(|()| {
                if state.status == Some(0)
                    && let Some(error) = state.input_error.take()
                {
                    return Err(ToolkitError::new(
                        "luals.check_failed",
                        format!("Process stdin failed: {error}"),
                        5,
                    ));
                }
                Ok(())
            });
            if let Err(error) = result {
                cleanup.completion_error.get_or_insert(error);
            }
        }
        *completion = None;
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
    retire_inner(owner, workers, state, rx, deadline, None)
}

pub(super) fn retire_completing(
    owner: &mut impl ProcessOwner,
    workers: &mut WorkerSet,
    state: &mut ProcessState,
    rx: &mpsc::Receiver<()>,
    deadline: Instant,
    control: ProcessControl<'_>,
    execution_deadline: Instant,
) -> Retirement {
    retire_inner(
        owner,
        workers,
        state,
        rx,
        deadline,
        Some((control, execution_deadline)),
    )
}

fn retire_inner(
    owner: &mut impl ProcessOwner,
    workers: &mut WorkerSet,
    state: &mut ProcessState,
    rx: &mpsc::Receiver<()>,
    mut deadline: Instant,
    mut completion: Option<(ProcessControl<'_>, Instant)>,
) -> Retirement {
    let mut cleanup = Retirement::default();
    let mut error_grace_started = false;
    owner.close_parent_pipes();
    observe(owner, workers, state, &mut cleanup, &mut completion);
    if cleanup.completion_error.is_some() && !error_grace_started {
        // One transition from execution to error cleanup, never a renewed
        // execution budget or a per-iteration grace extension.
        deadline = Instant::now() + CLEANUP_GRACE;
        error_grace_started = true;
    }
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
        observe(owner, workers, state, &mut cleanup, &mut completion);
        if cleanup.completion_error.is_some() && !error_grace_started {
            // One transition from execution to error cleanup, never a renewed
            // execution budget or a per-iteration grace extension.
            deadline = Instant::now() + CLEANUP_GRACE;
            error_grace_started = true;
        }
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

#[cfg(test)]
mod completion_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Exited;
    impl ProcessOwner for Exited {
        fn close_parent_pipes(&mut self) {}
        fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
            Ok(Some(0))
        }
        fn terminate_job(&mut self) -> std::result::Result<bool, String> {
            Ok(true)
        }
        fn kill_root(&mut self) -> std::io::Result<()> {
            panic!("root already reaped")
        }
        fn job_empty(&mut self) -> std::result::Result<Option<bool>, String> {
            Ok(Some(true))
        }
    }
    fn wait_finished(workers: &WorkerSet, slot: usize) {
        let limit = Instant::now() + Duration::from_secs(5);
        while !workers.handles[slot].as_ref().unwrap().is_finished() {
            assert!(Instant::now() < limit);
            thread::yield_now();
        }
    }
    #[test]
    fn legacy_pending_completion_checks_stop_before_join_but_cleanup_finishes() {
        for cancel in [false, true] {
            let raw = AtomicBool::new(cancel);
            let (wake, rx) = mpsc::channel();
            let mut workers = WorkerSet::default();
            workers
                .spawn(1, wake, || {
                    Event::Output(0, b"tail".to_vec(), Ok(()), false)
                })
                .unwrap();
            wait_finished(&workers, 1);
            let mut state = ProcessState::default();
            state.status = Some(0);
            let deadline = if cancel {
                Instant::now() + Duration::from_secs(10)
            } else {
                Instant::now()
            };
            let cleanup = retire_completing(
                &mut Exited,
                &mut workers,
                &mut state,
                &rx,
                Instant::now() + Duration::from_secs(5),
                ProcessControl::Legacy(Some(&raw)),
                deadline,
            );
            assert!(cleanup.complete());
            assert_eq!(
                cleanup.completion_error.unwrap().code,
                if cancel {
                    "process.cancelled"
                } else {
                    "luals.check_failed"
                }
            );
            assert_eq!(state.outputs[0].as_deref(), Some(b"tail".as_slice()));
        }
    }
    #[test]
    fn observed_worker_failure_survives_later_control_while_second_worker_is_held() {
        for overflow in [false, true] {
            let control = super::super::OperationControl::default();
            let (wake, _rx) = mpsc::channel();
            let (release, gate) = mpsc::channel();
            let mut workers = WorkerSet::default();
            workers
                .spawn(1, wake.clone(), move || {
                    Event::Output(
                        0,
                        vec![1],
                        if overflow {
                            Ok(())
                        } else {
                            Err(std::io::Error::other("first reader error"))
                        },
                        overflow,
                    )
                })
                .unwrap();
            workers
                .spawn(2, wake, move || {
                    gate.recv().unwrap();
                    Event::Output(1, b"second-tail".to_vec(), Ok(()), false)
                })
                .unwrap();
            wait_finished(&workers, 1);
            let mut state = ProcessState::default();
            state.status = Some(0);
            let mut cleanup = Retirement::default();
            let mut completion = Some((ProcessControl::Complete(&control), control.deadline()));
            observe(
                &mut Exited,
                &mut workers,
                &mut state,
                &mut cleanup,
                &mut completion,
            );
            assert!(!workers.complete());
            assert!(completion.is_none());
            control.tracked_cancel_handle().unwrap().cancel();
            release.send(()).unwrap();
            wait_finished(&workers, 2);
            observe(
                &mut Exited,
                &mut workers,
                &mut state,
                &mut cleanup,
                &mut completion,
            );
            cleanup.job_empty = Some(true);
            let error = super::super::finish_retired_result(
                Ok(()),
                ProcessControl::Complete(&control),
                &mut state,
                &cleanup,
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                if overflow {
                    "luals.input_budget"
                } else {
                    "luals.check_failed"
                }
            );
            assert_eq!(state.outputs[1].as_deref(), Some(b"second-tail".as_slice()));
        }
    }
    #[test]
    fn joined_legacy_interval_does_not_start_a_new_cancellation_boundary() {
        let raw = AtomicBool::new(false);
        let (wake, _rx) = mpsc::channel();
        let mut workers = WorkerSet::default();
        workers
            .spawn(1, wake, || Event::Output(0, vec![], Ok(()), false))
            .unwrap();
        wait_finished(&workers, 1);
        let mut state = ProcessState::default();
        state.status = Some(0);
        let mut cleanup = Retirement::default();
        let mut completion = Some((
            ProcessControl::Legacy(Some(&raw)),
            Instant::now() + Duration::from_secs(10),
        ));
        observe(
            &mut Exited,
            &mut workers,
            &mut state,
            &mut cleanup,
            &mut completion,
        );
        assert!(completion.is_none());
        raw.store(true, Ordering::Release);
        observe(
            &mut Exited,
            &mut workers,
            &mut state,
            &mut cleanup,
            &mut completion,
        );
        assert!(cleanup.completion_error.is_none());
    }
}
