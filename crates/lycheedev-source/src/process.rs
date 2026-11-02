//! Bounded, owned external process execution. The Windows job guards descendants
//! after attachment; spawn-to-attach race remains documented until suspended spawn.
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use std::{
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
#[derive(Debug)]
pub struct Output {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
mod git_batch;
mod lifetime;
mod stderr_observation;
pub(crate) use git_batch::GitBatchProcess;
use lifetime::{ProcessState, Retirement, WorkerSet};
pub(crate) use stderr_observation::{Snapshot as StderrSnapshot, StderrObservation};

enum Event {
    Output(usize, Vec<u8>, std::io::Result<()>, bool),
    Input(std::io::Result<()>),
}
#[derive(Clone, Copy)]
struct ProcessControl<'a>(&'a OperationControl);
fn cancelled() -> ToolkitError {
    ToolkitError::new("process.cancelled", "External process cancelled", 7)
}
fn deadline_exceeded() -> ToolkitError {
    ToolkitError::new(
        "source.process_check_failed",
        "External process execution deadline exceeded",
        5,
    )
}
impl ProcessControl<'_> {
    fn check(self) -> Result<()> {
        self.0.check().map_err(|error| match error.code.as_str() {
            "command.cancelled" => cancelled(), "command.deadline" => deadline_exceeded(), _ => error,
        })
    }
    fn deadline(self, requested: Instant) -> Instant { requested.min(self.0.deadline()) }
    fn expired(self) -> ToolkitError { self.check().err().unwrap_or_else(deadline_exceeded) }
}
fn complete_process_control(
    control: ProcessControl<'_>,
    cleanup: impl FnOnce() -> serde_json::Value,
) -> Result<()> {
    control.check().map_err(|mut error| {
        error.details = cleanup();
        error
    })
}
pub fn run_with_operation_control(
    cmd: Command,
    cap: usize,
    timeout: Duration,
    input: Option<Vec<u8>>,
    control: &OperationControl,
) -> Result<Output> {
    run_with_operation_control_observed(cmd, cap, timeout, input, control, None)
}
/// Internal diagnostic-only stderr prefix; legacy callers keep None.
pub(crate) fn run_with_operation_control_observed(
    cmd: Command,
    cap: usize,
    timeout: Duration,
    input: Option<Vec<u8>>,
    control: &OperationControl,
    observation: Option<StderrObservation>,
) -> Result<Output> {
    let scope = process_scope(control, Instant::now() + timeout)?;
    run_with_limits_inner(
        cmd,
        cap,
        cap,
        timeout,
        input,
        ProcessControl(&scope),
        observation,
    )
}
fn process_scope(control: &OperationControl, deadline: Instant) -> Result<OperationControl> {
    control.check()?;
    control.ordered_child(deadline)
}
pub(crate) fn run_with_limits(
    cmd: Command,
    stdout_cap: usize,
    stderr_cap: usize,
    timeout: Duration,
    input: Option<Vec<u8>>,
    control: &OperationControl,
) -> Result<Output> {
    run_with_limits_inner(
        cmd,
        stdout_cap,
        stderr_cap,
        timeout,
        input,
        ProcessControl(&process_scope(control, Instant::now() + timeout)?),
        None,
    )
}
fn run_with_limits_inner(
    mut cmd: Command,
    stdout_cap: usize,
    stderr_cap: usize,
    timeout: Duration,
    input: Option<Vec<u8>>,
    control: ProcessControl<'_>,
    observation: Option<StderrObservation>,
) -> Result<Output> {
    let deadline = control.deadline(Instant::now() + timeout);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    // A complete scope can already be expired (notably timeout=0). Do not
    // spawn before checking that narrowed scope; legacy admission is unchanged.
    control.check()?;
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .spawn()
        .map_err(|e| ToolkitError::new("source.process_unavailable", e.to_string(), 3))?;
    let (wake, rx) = mpsc::channel();
    let mut workers = WorkerSet::default();
    let mut state = ProcessState::default();
    let job = match lycheedev_windows::JobGuard::attach(&child) {
        Ok(job) => job,
        Err(error) => {
            let mut owned = OwnedProcess {
                child: &mut child,
                job: None,
            };
            let cleanup = lifetime::retire(
                &mut owned,
                &mut workers,
                &mut state,
                &rx,
                Instant::now() + lifetime::CLEANUP_GRACE,
            );
            return Err(
                ToolkitError::new("source.process_unavailable", error, 3).with_details(cleanup.details())
            );
        }
    };
    // Keep the Child in this scope. Only a successful try_wait establishes its
    // terminal status; a failed waiter event can no longer masquerade as reap.
    let result = (|| {
        if let Some(input) = input {
            let pipe = child.stdin.take().expect("piped child stdin");
            workers.spawn(0, wake.clone(), move || lifetime::write_input(pipe, input))?;
        }
        let out = child.stdout.take().expect("piped child stdout");
        workers.spawn(1, wake.clone(), move || {
            lifetime::read_output(0, out, stdout_cap)
        })?;
        let err = child.stderr.take().expect("piped child stderr");
        workers.spawn(2, wake, move || match observation {
            Some(observation) => lifetime::read_output_observed(err, stderr_cap, observation),
            None => lifetime::read_output(1, err, stderr_cap),
        })?;
        loop {
            control.check()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(control.expired());
            }
            workers.collect(&mut state)?;
            if state.status.is_none() {
                state.observe_wait(
                    child
                        .try_wait()
                        .map(|status| status.map(|s| s.code().unwrap_or(-1))),
                )?;
            }
            // A reaped root can leave descendants holding inherited pipe handles.
            // Retire the exact Job before waiting for their EOF; retirement
            // still drains and joins every started worker within the old budget.
            if state.status.is_some() {
                break;
            }
            // Worker messages are wakeups only. A wakeup/EOF is not terminal
            // evidence: collect joins only handles which report is_finished.
            lifetime::wait_for_progress(&rx, remaining.min(lifetime::POLL_INTERVAL));
        }
        Ok(())
    })();
    let result = result.and_then(|()| {
        // Preserve the existing final control-before-stdin-error ordering.
        // With pending readers, defer stdin failure until their cap/IO results
        // are known: those errors preceded stdin validation in the old loop.
        // Stops observed here use the same error grace as stops in the loop.
        control.check()?;
        if workers.complete()
            && state.status == Some(0)
            && let Some(error) = state.input_error.take()
        {
            return Err(ToolkitError::new(
                "source.process_check_failed",
                format!("Process stdin failed: {error}"),
                5,
            ));
        }
        Ok(())
    });
    // All ordinary post-spawn exits take the same bounded retirement path,
    // including worker startup, read/write, wait and output-cap failures.
    // Error retirement retains the existing separate five-second grace. A
    // successful operation does not acquire a new execution budget.
    let cleanup_deadline = if result.is_err() {
        Instant::now() + lifetime::CLEANUP_GRACE
    } else {
        deadline.min(Instant::now() + lifetime::CLEANUP_GRACE)
    };
    let mut owned = OwnedProcess {
        child: &mut child,
        job: Some(&job),
    };
    let cleanup = if result.is_ok() && !workers.complete() {
        lifetime::retire_completing(
            &mut owned,
            &mut workers,
            &mut state,
            &rx,
            cleanup_deadline,
            control,
            deadline,
        )
    } else {
        lifetime::retire(&mut owned, &mut workers, &mut state, &rx, cleanup_deadline)
    };
    finish_retired_result(result, control, &mut state, &cleanup)?;
    Ok(Output {
        status: state
            .status
            .expect("successful retirement reaped the child"),
        stdout: state.outputs[0].take().expect("joined stdout reader"),
        stderr: state.outputs[1].take().expect("joined stderr reader"),
    })
}
fn finish_retired_result(
    result: Result<()>,
    control: ProcessControl<'_>,
    state: &mut ProcessState,
    cleanup: &Retirement,
) -> Result<()> {
    let result = result.and_then(|()| {
        if let Some(error) = &cleanup.completion_error {
            return Err(error.clone());
        }
        control.check()?;
        if let Some(error) = &cleanup.first_worker_error {
            return Err(error.clone());
        }
        if state.status == Some(0)
            && let Some(error) = state.input_error.take()
        {
            return Err(ToolkitError::new(
                "source.process_check_failed",
                format!("Process stdin failed: {error}"),
                5,
            ));
        }
        Ok(())
    });
    finish_result(result, control, cleanup)
}
fn finish_result(
    result: Result<()>,
    control: ProcessControl<'_>,
    cleanup: &Retirement,
) -> Result<()> {
    // Once a primary failure has been selected, later cancellation, timeout or
    // cleanup failures cannot replace it. Keep independent cleanup evidence.
    result.map_err(|mut error| {
        error.details = cleanup.details();
        error
    })?;
    complete_process_control(control, || cleanup.details())?;
    if !cleanup.complete() || !cleanup.errors.is_empty() {
        return Err(ToolkitError::new(
            "source.process_check_failed",
            "External process retirement is unconfirmed",
            5,
        )
        .with_details(cleanup.details()));
    }
    Ok(())
}
struct OwnedProcess<'a> {
    child: &'a mut Child,
    job: Option<&'a lycheedev_windows::JobGuard>,
}
impl lifetime::ProcessOwner for OwnedProcess<'_> {
    fn close_parent_pipes(&mut self) {
        self.child.stdin.take();
        self.child.stdout.take();
        self.child.stderr.take();
    }
    fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
        self.child
            .try_wait()
            .map(|status| status.map(|s| s.code().unwrap_or(-1)))
    }
    fn terminate_job(&mut self) -> std::result::Result<bool, String> {
        self.job
            .map(|job| job.terminate().map(|()| true))
            .unwrap_or(Ok(false))
    }
    fn kill_root(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }
    fn job_empty(&mut self) -> std::result::Result<Option<bool>, String> {
        self.job
            .map(|job| {
                job.memory_accounting()
                    .map(|value| value.active_processes == 0)
            })
            .transpose()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Read, Write}, sync::atomic::Ordering};
    #[test]
    #[ignore = "subprocess helper; invoked by the process and Git batch lifecycle regressions"]
    fn helper_process() {
        let mode = std::env::var("LYCHEEDEV_SOURCE_TEST_PROCESS")
            .expect("required Source process helper mode");
        match mode.as_str() {
            "output" => {
                std::io::stdout().write_all(&vec![b'x'; 65536]).unwrap();
            }
            "block" => std::thread::sleep(Duration::from_secs(20)),
            "ready-block" => {
                let ready = std::env::var_os("LYCHEEDEV_SOURCE_TEST_READY").unwrap();
                std::fs::write(ready, b"ready").unwrap();
                std::thread::sleep(Duration::from_secs(30));
            }
            "echo" => {
                let mut input = Vec::new();
                std::io::stdin().read_to_end(&mut input).unwrap();
                std::io::stderr().write_all(&input).unwrap();
                std::io::stdout().write_all(b"source-process-echo").unwrap();
            }
            "nonzero" => {
                std::io::stderr()
                    .write_all(b"source-process-nonzero")
                    .unwrap();
                std::process::exit(17);
            }
            "no-read" => {}
            "descendant-block" => {
                use std::os::windows::fs::OpenOptionsExt;
                let ready = std::env::var_os("LYCHEEDEV_SOURCE_TEST_READY").unwrap();
                let mut held = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .share_mode(0)
                    .open(ready)
                    .unwrap();
                held.write_all(b"ready").unwrap();
                std::thread::sleep(Duration::from_secs(30));
                drop(held);
            }
            "spawn-pipe-descendant"
            | "spawn-silent-descendant"
            | "spawn-inherited-descendant"
            | "spawn-nonzero-descendant" => {
                // The production stdin worker starts only after Job attach.
                // Requiring its byte removes the attach race from this fixture.
                let mut gate = [0];
                std::io::stdin().read_exact(&mut gate).unwrap();
                assert_eq!(gate, [b'G']);
                let ready = std::path::PathBuf::from(
                    std::env::var_os("LYCHEEDEV_SOURCE_TEST_READY").unwrap(),
                );
                let mut command = helper("descendant-block");
                command.stdin(Stdio::null());
                // NUL redirects stdio; Windows may still inherit other pipe
                // handles. Neither this mode nor root exit proves reader EOF.
                if mode == "spawn-silent-descendant" {
                    command.stdout(Stdio::null()).stderr(Stdio::null());
                }
                // The runner's already-attached Job owns this descendant even
                // after this helper root exits and drops its std Child handle.
                let _descendant = command.spawn().unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while !ready.is_file() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert!(ready.is_file(), "post-attachment descendant started");
                std::io::stdout()
                    .write_all(b"root-tail-before-exit")
                    .unwrap();
                if mode == "spawn-pipe-descendant" {
                    // This fixture tests cancellation, not a race between root
                    // success and the notifier. Only cancellation retires it.
                    std::thread::sleep(Duration::from_secs(30));
                }
                if mode == "spawn-nonzero-descendant" {
                    std::io::stderr().write_all(b"nonzero-stderr-tail").unwrap();
                    std::process::exit(17);
                }
            }
            _ => panic!("unknown helper mode"),
        }
    }
    fn helper(mode: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "process::tests::helper_process", "--ignored", "--nocapture"])
            .env("LYCHEEDEV_SOURCE_TEST_PROCESS", mode);
        cmd
    }
    #[test]
    fn timeout_output_budget_and_cancel_reap_blocked_writer() {
        let timed = run_with_operation_control(
            helper("block"),
            1024,
            Duration::from_millis(100),
            Some(vec![b'x'; 4 << 20]), &Default::default(),
        )
        .unwrap_err();
        assert_eq!(timed.details["reaped"], true, "{:?}", timed.details);
        assert_eq!(timed.details["stdinWriterComplete"], true);
        assert_eq!(timed.details["outputReadersComplete"], true);
        assert_eq!(timed.details["jobEmpty"], true);
        let over = run_with_operation_control(helper("output"), 1024, Duration::from_secs(10), None, &Default::default()).unwrap_err();
        assert_eq!(over.code, "source.process_input_budget");
        assert_eq!(over.details["reaped"], true);
        assert_eq!(over.details["ownedJobTerminal"], true);
        let control = OperationControl::default();
        let signal = control.tracked_cancel_handle().unwrap();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            signal.cancel();
        });
        let aborted = run_with_operation_control(
            helper("block"),
            1024,
            Duration::from_secs(10),
            Some(vec![b'x'; 4 << 20]),
            &control,
        )
        .unwrap_err();
        thread.join().unwrap();
        assert_eq!(aborted.exit_code, 7);
        assert_eq!(aborted.details["reaped"], true);
        assert_eq!(aborted.details["stdinWriterComplete"], true);
        assert_eq!(aborted.details["outputReadersComplete"], true);
        assert_eq!(aborted.details["ownedJobTerminal"], true);
    }

    #[test]
    fn complete_control_maps_first_stop_preserves_first_stop() {
        use lycheedev_core::control::{StopCause, TrackedCancellation};
        let signal = TrackedCancellation::new();
        let expired = OperationControl::tracked(Instant::now(), signal.clone());
        signal.cancel();
        let error = ProcessControl(&expired).check().unwrap_err();
        assert_eq!(error.code, "source.process_check_failed");
        assert_eq!(error.exit_code, 5);
        assert_eq!(
            error.message,
            "External process execution deadline exceeded"
        );
        assert_eq!(
            expired.stop_cause().unwrap(),
            Some(StopCause::DeadlineExceeded)
        );

        let parent = OperationControl::default();
        let child = parent.ordered_child(parent.deadline()).unwrap();
        let local = child.tracked_cancel_handle().unwrap();
        let complete = ProcessControl(&child);
        complete.check().unwrap();
        assert_eq!(
            complete.deadline(parent.deadline() + Duration::from_secs(1)),
            child.deadline()
        );
        parent.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(complete.check().unwrap_err().code, "process.cancelled");
        assert_eq!(complete.expired().code, "process.cancelled");
        assert!(!local.is_cancelled());
        assert_eq!(child.stop_cause().unwrap(), Some(StopCause::Cancelled));
        assert_eq!(parent.stop_cause().unwrap(), Some(StopCause::Cancelled));


    }

    #[test]
    fn process_scope_bounds_its_timeout_and_keeps_cancellation_ancestry() {
        use lycheedev_core::control::StopCause;
        let parent = OperationControl::default();
        let requested = Instant::now();
        let scope = process_scope(&parent, requested).unwrap();
        assert_eq!(scope.deadline(), requested);
        parent.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(
            scope.stop_cause().unwrap(),
            Some(StopCause::DeadlineExceeded)
        );
        assert_eq!(parent.stop_cause().unwrap(), Some(StopCause::Cancelled));
        assert_eq!(
            ProcessControl(&scope).check().unwrap_err().code,
            "source.process_check_failed"
        );


    }

    #[test]
    fn complete_control_rejects_cancelled_operation_before_spawning() {
        let control = OperationControl::default();
        control.tracked_cancel_handle().unwrap().cancel();
        let error = run_with_operation_control(
            Command::new("lycheedev-fixture-must-not-spawn"),
            1024,
            Duration::from_secs(1),
            None,
            &control,
        )
        .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.exit_code, 7);
    }

    #[test]
    fn complete_control_zero_timeout_precedes_missing_program_without_spawning() {
        let missing = std::env::current_exe().unwrap().join("not-a-directory.exe");
        let parent = OperationControl::default();
        let error =
            run_with_operation_control(Command::new(&missing), 1024, Duration::ZERO, None, &parent)
                .unwrap_err();
        assert_eq!(error.code, "source.process_check_failed");
        assert_eq!(
            error.message,
            "External process execution deadline exceeded"
        );
        assert_eq!(error.exit_code, 5);
        assert!(error.details.is_null(), "no process was spawned or reaped");
        parent.check().unwrap();

    }

    #[test]
    fn complete_control_checks_cancel_after_final_event_and_keeps_cleanup_evidence() {
        use lycheedev_core::control::StopCause;
        use std::cell::Cell;
        let parent = OperationControl::default();
        let scope = process_scope(&parent, parent.deadline()).unwrap();
        let control = ProcessControl(&scope);
        control.check().unwrap();
        let (tx, rx) = mpsc::channel();
        tx.send(Event::Input(Ok(()))).unwrap();
        parent.tracked_cancel_handle().unwrap().cancel();
        assert!(matches!(rx.recv().unwrap(), Event::Input(Ok(()))));
        let evidence = serde_json::json!({"terminated":true,"reaped":true,"stdinWriterComplete":true,"outputReadersComplete":true});
        let cleanup_calls = Cell::new(0);
        // Exercise the exact completion helper used after the production loop,
        // with cancellation between its last pre-receive check and final event.
        let error = complete_process_control(control, || {
            cleanup_calls.set(cleanup_calls.get() + 1);
            evidence.clone()
        })
        .unwrap_err();
        assert_eq!(error.code, "process.cancelled");
        assert_eq!(error.exit_code, 7);
        assert_eq!(error.details, evidence);
        assert_eq!(cleanup_calls.get(), 1);
        assert_eq!(scope.stop_cause().unwrap(), Some(StopCause::Cancelled));

    }

    #[test]
    fn complete_control_detects_short_scope_deadline_at_completion_before_late_cancel() {
        use lycheedev_core::control::StopCause;
        for late_cancel in [false, true] {
            let parent = OperationControl::default();
            let scope = process_scope(&parent, Instant::now()).unwrap();
            parent.check().unwrap(); // A check of the original invocation misses this timeout.
            if late_cancel {
                parent.tracked_cancel_handle().unwrap().cancel();
            }
            let evidence = serde_json::json!({"reaped":true,"retainedCompletionEvidence":42});
            let error =
                complete_process_control(ProcessControl(&scope), || evidence.clone())
                    .unwrap_err();
            assert_eq!(error.code, "source.process_check_failed");
            assert_eq!(error.exit_code, 5);
            assert_eq!(error.details, evidence);
            assert_eq!(
                scope.stop_cause().unwrap(),
                Some(StopCause::DeadlineExceeded)
            );
        }
    }

    #[test]
    fn complete_control_parent_cancel_reaps_started_child_and_blocked_writer() {
        use lycheedev_core::control::StopCause;
        use std::{path::PathBuf, sync::atomic::AtomicU64};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory: PathBuf = (0..128)
            .find_map(|_| {
                let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "lychee-source-parent-cancel-{}-{sequence}",
                    std::process::id()
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => Some(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => panic!("fixture directory: {error}"),
                }
            })
            .expect("exclusive fixture directory");
        let ready = directory.join("ready");
        let parent = OperationControl::uncancelable(Instant::now() + Duration::from_secs(30));
        let child = parent
            .ordered_child(Instant::now() + Duration::from_secs(20))
            .unwrap();
        let local = child.tracked_cancel_handle().unwrap();
        let signal = parent.tracked_cancel_handle().unwrap();
        let observed_ready = ready.clone();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if observed_ready.is_file() {
                    signal.cancel();
                    return true;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            false
        });
        let mut command = helper("ready-block");
        command.env("LYCHEEDEV_SOURCE_TEST_READY", &ready);
        let result = run_with_operation_control(
            command,
            1024,
            Duration::from_secs(60),
            Some(vec![b'x'; 4 << 20]),
            &child,
        );
        let signalled_after_start = worker.join().unwrap();
        let error = result.unwrap_err();
        assert!(
            signalled_after_start,
            "helper must actually start before parent cancellation"
        );
        assert_eq!(error.code, "process.cancelled");
        assert_eq!(error.exit_code, 7);
        assert_eq!(error.details["terminated"], true);
        assert_eq!(error.details["reaped"], true);
        assert_eq!(error.details["stdinWriterComplete"], true);
        assert_eq!(error.details["outputReadersComplete"], true);
        assert_eq!(error.details["ownedJobTerminal"], true);
        assert!(!local.is_cancelled());
        assert_eq!(parent.stop_cause().unwrap(), Some(StopCause::Cancelled));
        assert_eq!(child.stop_cause().unwrap(), Some(StopCause::Cancelled));
        // Retain scratch on failure; delete only after all owned cleanup facts.
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[path = "lifetime_tests.rs"]
    mod lifetime_tests;
}
