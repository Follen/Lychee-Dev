use super::super::{lifetime::*, *};
use std::{collections::VecDeque, io::{self, Read, Write}, path::PathBuf, sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}}};

enum Wait {
    Pending,
    Exited(i32),
    Failed,
}
struct Owner {
    waits: VecDeque<Wait>,
    wait_count: usize,
    empty: std::result::Result<Option<bool>, String>,
    termination: std::result::Result<bool, String>,
    kill_fails: bool,
    parent_pipes_closed: bool,
    kills: usize,
    terminations: usize,
}
impl Owner {
    fn new(wait: Wait, empty: Option<bool>) -> Self {
        Self {
            waits: VecDeque::from([wait]),
            wait_count: 0,
            empty: Ok(empty),
            termination: Ok(true),
            kill_fails: false,
            parent_pipes_closed: false,
            kills: 0,
            terminations: 0,
        }
    }
}
impl ProcessOwner for Owner {
    fn close_parent_pipes(&mut self) {
        self.parent_pipes_closed = true;
    }
    fn try_wait(&mut self) -> io::Result<Option<i32>> {
        self.wait_count += 1;
        let value = if self.waits.len() > 1 {
            self.waits.pop_front().unwrap()
        } else {
            return match self.waits.front().unwrap() {
                Wait::Pending => Ok(None),
                Wait::Exited(code) => Ok(Some(*code)),
                Wait::Failed => Err(io::Error::other("injected wait failure")),
            };
        };
        match value {
            Wait::Pending => Ok(None),
            Wait::Exited(code) => Ok(Some(code)),
            Wait::Failed => Err(io::Error::other("injected wait failure")),
        }
    }
    fn terminate_job(&mut self) -> std::result::Result<bool, String> {
        self.terminations += 1;
        self.termination.clone()
    }
    fn kill_root(&mut self) -> io::Result<()> {
        self.kills += 1;
        if self.kill_fails {
            Err(io::Error::other("injected root kill failure"))
        } else {
            Ok(())
        }
    }
    fn job_empty(&mut self) -> std::result::Result<Option<bool>, String> {
        self.empty.clone()
    }
}
fn immediate_retirement(owner: &mut Owner) -> Retirement {
    let (_tx, rx) = mpsc::channel();
    retire(
        owner,
        &mut WorkerSet::default(),
        &mut ProcessState::default(),
        &rx,
        Instant::now(),
    )
}
fn collect_until_finished(workers: &mut WorkerSet, state: &mut ProcessState) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut error = None;
    loop {
        if let Err(current) = workers.collect(state) {
            error.get_or_insert(current);
        }
        if workers.complete() {
            return error.map_or(Ok(()), Err);
        }
        assert!(
            Instant::now() < deadline,
            "fixture worker did not finish; it was not joined while running"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn failed_wait_and_successful_terminate_never_prove_reap() {
    let mut owner = Owner::new(Wait::Failed, Some(false));
    let cleanup = immediate_retirement(&mut owner);
    assert!(!cleanup.reaped);
    assert!(!cleanup.complete());
    assert!(cleanup.terminated);
    assert!(cleanup.deadline_exceeded);
    assert_eq!(owner.wait_count, 2);
    assert_eq!(owner.terminations, 1);
    assert_eq!(owner.kills, 0);
    assert!(owner.parent_pipes_closed);
    assert!(
        cleanup
            .errors
            .iter()
            .any(|error| error.contains("injected wait failure"))
    );
}
#[test]
fn root_reap_is_separate_from_active_descendants() {
    let mut owner = Owner::new(Wait::Exited(0), Some(false));
    let cleanup = immediate_retirement(&mut owner);
    assert!(cleanup.reaped);
    assert!(cleanup.stdin_complete && cleanup.outputs_complete);
    assert_eq!(cleanup.job_empty, Some(false));
    assert!(!cleanup.complete());
    assert!(cleanup.deadline_exceeded);
    assert_eq!(owner.wait_count, 1, "do not wait twice on proven root exit");
}
#[test]
fn no_job_attach_failure_only_proves_original_root_and_stops_after_reap() {
    let mut owner = Owner::new(Wait::Pending, None);
    owner.waits.push_back(Wait::Exited(7));
    owner.termination = Ok(false);
    let cleanup = immediate_retirement(&mut owner);
    assert!(cleanup.reaped);
    assert!(cleanup.root_kill_requested);
    assert!(!cleanup.terminated);
    assert_eq!(cleanup.job_empty, None);
    assert!(!cleanup.complete());
    assert!(!cleanup.deadline_exceeded);
    assert_eq!(owner.kills, 1);
}
#[test]
fn failed_wait_job_query_termination_and_root_kill_all_survive() {
    let mut owner = Owner::new(Wait::Failed, None);
    owner.empty = Err("injected query failure".into());
    owner.termination = Err("injected job termination failure".into());
    owner.kill_fails = true;
    let cleanup = immediate_retirement(&mut owner);
    assert!(!cleanup.complete());
    assert!(!cleanup.reaped);
    assert!(!cleanup.terminated);
    assert!(!cleanup.root_kill_requested);
    for expected in [
        "wait failure",
        "query failure",
        "job termination failure",
        "root kill failure",
    ] {
        assert_eq!(
            cleanup
                .errors
                .iter()
                .filter(|error| error.contains(expected))
                .count(),
            1
        );
    }
}
#[test]
fn a_later_successful_wait_proves_reap_without_erasing_the_prior_failure() {
    let mut owner = Owner::new(Wait::Failed, Some(true));
    owner.waits.push_back(Wait::Exited(9));
    let cleanup = immediate_retirement(&mut owner);
    assert!(cleanup.complete());
    assert!(
        cleanup
            .errors
            .iter()
            .any(|error| error.contains("wait failure"))
    );
    let error = finish_result(Ok(()), ProcessControl(&OperationControl::default()), &cleanup).unwrap_err();
    assert_eq!(error.message, "External process retirement is unconfirmed");
    assert_eq!(error.details["reaped"], true);
    assert_eq!(
        owner.terminations, 0,
        "an observed empty Job needs no termination request"
    );
}
#[test]
fn primary_cancel_deadline_and_io_errors_survive_late_control_and_cleanup_faults() {
    for original in [
        cancelled(),
        deadline_exceeded(),
        ToolkitError::io(io::Error::other("original wait failure")),
    ] {
        let parent = OperationControl::default();
        parent.tracked_cancel_handle().unwrap().cancel();
        let mut owner = Owner::new(Wait::Failed, Some(false));
        owner.termination = Err("cleanup termination failure".into());
        let cleanup = immediate_retirement(&mut owner);
        let error = finish_result(
            Err(original.clone()),
            ProcessControl(&parent),
            &cleanup,
        )
        .unwrap_err();
        assert_eq!(error.code, original.code);
        assert_eq!(error.message, original.message);
        assert_eq!(error.exit_code, original.exit_code);
        assert_eq!(error.details, cleanup.details());
    }
}

struct BlockingDrop {
    reached: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    dropped: Arc<AtomicBool>,
}
impl Write for BlockingDrop {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Read for BlockingDrop {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}
impl Drop for BlockingDrop {
    fn drop(&mut self) {
        self.reached.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(5)).unwrap();
        self.dropped.store(true, Ordering::Release);
    }
}
#[test]
fn writer_and_reader_teardown_must_finish_before_they_are_joined_or_counted() {
    for index in 0..3 {
        let (reached, reached_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let pipe = BlockingDrop {
            reached,
            release: release_rx,
            dropped: dropped.clone(),
        };
        let (wake, rx) = mpsc::channel();
        let mut workers = WorkerSet::default();
        let mut state = ProcessState::default();
        workers
            .spawn(index, wake.clone(), move || {
                if index == 0 {
                    write_input(pipe, vec![b'x'])
                } else {
                    read_output(index - 1, pipe, 1)
                }
            })
            .unwrap();
        reached_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // An early/duplicate wakeup is deliberately injected while pipe Drop
        // is blocked; channel delivery must have no effect on terminal facts.
        wake.send(()).unwrap();
        workers.collect(&mut state).unwrap();
        assert!(!workers.complete());
        let mut owner = Owner::new(Wait::Exited(0), Some(true));
        let cleanup = retire(&mut owner, &mut workers, &mut state, &rx, Instant::now());
        assert!(!cleanup.complete());
        assert_eq!(cleanup.stdin_complete, index != 0);
        assert_eq!(cleanup.outputs_complete, index == 0);
        assert!(!dropped.load(Ordering::Acquire));
        release.send(()).unwrap();
        collect_until_finished(&mut workers, &mut state).unwrap();
        assert!(dropped.load(Ordering::Acquire));
        let cleanup = retire(&mut owner, &mut workers, &mut state, &rx, Instant::now());
        assert!(cleanup.complete());
    }
}
struct FaultyPipe {
    dropped: Arc<AtomicBool>,
    panic: bool,
}
impl Drop for FaultyPipe {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}
impl Read for FaultyPipe {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        assert!(!self.panic, "injected reader panic");
        Err(io::Error::other("injected reader error"))
    }
}
impl Write for FaultyPipe {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected writer error"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn reader_failure_is_not_an_output_cap_failure_and_both_pipes_are_dropped() {
    let dropped = Arc::new(AtomicBool::new(false));
    let mut state = ProcessState::default();
    let (wake, _rx) = mpsc::channel();
    let mut workers = WorkerSet::default();
    let pipe = FaultyPipe {
        dropped: dropped.clone(),
        panic: false,
    };
    workers
        .spawn(1, wake, move || read_output(0, pipe, 1024))
        .unwrap();
    let error = collect_until_finished(&mut workers, &mut state).unwrap_err();
    assert_eq!(error.code, "source.process_check_failed");
    assert!(error.message.contains("injected reader error"));
    assert!(dropped.load(Ordering::Acquire));
    dropped.store(false, Ordering::Release);
    let pipe = FaultyPipe {
        dropped: dropped.clone(),
        panic: false,
    };
    let event = write_input(pipe, vec![b'x']);
    assert!(dropped.load(Ordering::Acquire));
    assert!(
        matches!(event, Event::Input(Err(error)) if error.to_string() == "injected writer error")
    );
}
#[test]
fn worker_panic_is_joined_and_reported_even_if_root_and_job_are_terminal() {
    let dropped = Arc::new(AtomicBool::new(false));
    let pipe = FaultyPipe {
        dropped: dropped.clone(),
        panic: true,
    };
    let (wake, rx) = mpsc::channel();
    let mut workers = WorkerSet::default();
    let mut state = ProcessState::default();
    workers
        .spawn(2, wake, move || read_output(1, pipe, 1024))
        .unwrap();
    let error = collect_until_finished(&mut workers, &mut state).unwrap_err();
    assert_eq!(error.message, "Process pipe worker panicked");
    assert!(workers.complete());
    assert!(dropped.load(Ordering::Acquire));
    let mut owner = Owner::new(Wait::Exited(0), Some(true));
    let cleanup = retire(&mut owner, &mut workers, &mut state, &rx, Instant::now());
    assert!(cleanup.complete());
    assert_eq!(cleanup.errors.len(), 1);
    assert!(finish_result(Ok(()), ProcessControl(&OperationControl::default()), &cleanup).is_err());
}
#[test]
fn output_reader_retains_exact_cap_and_detects_the_next_byte() {
    for (bytes, overflow) in [(vec![b'a'; 4], false), (vec![b'a'; 5], true)] {
        let event = read_output(0, io::Cursor::new(bytes.clone()), 4);
        assert!(
            matches!(event, Event::Output(0, actual, Ok(()), actual_overflow) if actual == bytes && actual_overflow == overflow)
        );
    }
}
fn directory(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    (0..128)
        .find_map(|_| {
            let path = std::env::temp_dir().join(format!(
                "lychee-source-lifetime-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => Some(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => None,
                Err(error) => panic!("create fixture directory: {error}"),
            }
        })
        .expect("exclusive fixture directory")
}
#[test]
fn successful_echo_and_nonzero_exit_keep_output_and_join_the_stdin_writer() {
    let bytes = b"fixture-input".to_vec();
    let output = run_with_operation_control(
        super::helper("echo"),
        4096,
        Duration::from_secs(10),
        Some(bytes.clone()), &Default::default(),
    )
    .unwrap();
    assert_eq!(output.status, 0);
    assert_eq!(output.stderr, bytes);
    assert!(
        output
            .stdout
            .windows(b"source-process-echo".len())
            .any(|value| value == b"source-process-echo")
    );
    let output = run_with_operation_control(
        super::helper("nonzero"),
        4096,
        Duration::from_secs(10),
        Some(vec![b'x'; 4 << 20]), &Default::default(),
    )
    .unwrap();
    assert_eq!(output.status, 17);
    assert_eq!(output.stderr, b"source-process-nonzero");
}
#[test]
fn successful_child_with_failed_stdin_returns_joined_cleanup_evidence() {
    let error = run_with_operation_control(
        super::helper("no-read"),
        4096,
        Duration::from_secs(10),
        Some(vec![b'x'; 4 << 20]), &Default::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, "source.process_check_failed");
    assert!(error.message.starts_with("Process stdin failed:"));
    assert_eq!(error.details["ownedJobTerminal"], true);
    assert_eq!(error.details["stdinWriterComplete"], true);
    assert_eq!(error.details["outputReadersComplete"], true);
}
#[test]
fn success_retires_a_post_attachment_descendant_with_closed_output_pipes() {
    let root = directory("success-descendant");
    let ready = root.join("held");
    let mut command = super::helper("spawn-silent-descendant");
    command.env("LYCHEEDEV_SOURCE_TEST_READY", &ready);
    let output = run_with_operation_control(command, 4096, Duration::from_secs(20), Some(vec![b'G']), &Default::default()).unwrap();
    assert_eq!(output.status, 0);
    assert!(ready.is_file(), "descendant must have started");
    // The exact descendant held this file without FILE_SHARE_DELETE while
    // alive. Rename plus removal follows successful joined/empty-Job return.
    std::fs::rename(&ready, root.join("released")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn cancellation_drains_a_post_attachment_descendant_holding_output_pipes() {
    let root = directory("cancel-descendant");
    let ready = root.join("held");
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(20));
    let signal = control.tracked_cancel_handle().unwrap();
    let observed_ready = ready.clone();
    let notifier = std::thread::spawn(move || {
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
    let mut command = super::helper("spawn-pipe-descendant");
    command.env("LYCHEEDEV_SOURCE_TEST_READY", &ready);
    let result = run_with_operation_control(
        command,
        4096,
        Duration::from_secs(20),
        Some(vec![b'G']),
        &control,
    );
    assert!(
        notifier.join().unwrap(),
        "cancel only after descendant startup"
    );
    let error = result.unwrap_err();
    assert_eq!(error.code, "process.cancelled");
    assert_eq!(error.details["ownedJobTerminal"], true);
    assert_eq!(error.details["jobEmpty"], true);
    std::fs::rename(&ready, root.join("released")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

// Integration coverage stays with the real process Event and reader functions.
#[test]
fn original_and_observed_events_match_at_overflow_boundary() {
    let observation = StderrObservation::new();
    let original = read_output(1, io::Cursor::new(b"abcdef"), 3);
    let observed = read_output_observed(io::Cursor::new(b"abcdef"), 3, observation.clone());
    match (original, observed) {
        (Event::Output(ai, ab, ar, ao), Event::Output(bi, bb, br, bo)) => {
            assert_eq!((ai, &ab, ar.is_ok(), ao), (bi, &bb, br.is_ok(), bo));
            assert_eq!(observation.snapshot().bytes, ab);
        }
        _ => panic!("unexpected reader event"),
    }
}

#[test]
fn root_exit_retires_inherited_pipes_and_preserves_status_and_tail() {
    for (mode, status) in [
        ("spawn-inherited-descendant", 0),
        ("spawn-nonzero-descendant", 17),
    ] {
        let root = directory(mode);
        let ready = root.join("held");
        let mut command = super::helper(mode);
        command.env("LYCHEEDEV_SOURCE_TEST_READY", &ready);
        let output =
            run_with_operation_control(command, 4096, Duration::from_secs(20), Some(vec![b'G']), &Default::default()).unwrap();
        assert_eq!(output.status, status);
        if status == 17 {
            assert!(output.stderr.ends_with(b"nonzero-stderr-tail"));
        }
        assert!(
            output
                .stdout
                .windows(b"root-tail-before-exit".len())
                .any(|v| v == b"root-tail-before-exit")
        );
        assert!(ready.is_file());
        std::fs::rename(&ready, root.join("released")).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn late_worker_errors_keep_their_original_cause_after_root_exit() {
    for status in [0, 17] {
        for overflow in [false, true] {
            let (wake, rx) = mpsc::channel();
            let (release, gate) = mpsc::channel();
            let mut workers = WorkerSet::default();
            let mut state = ProcessState::default();
            state.status = Some(status);
            workers
                .spawn(1, wake, move || {
                    gate.recv().unwrap();
                    Event::Output(
                        0,
                        b"tail".to_vec(),
                        if overflow {
                            Ok(())
                        } else {
                            Err(io::Error::other("late reader failure"))
                        },
                        overflow,
                    )
                })
                .unwrap();
            assert!(!workers.complete());
            release.send(()).unwrap();
            let mut owner = Owner::new(Wait::Exited(status), Some(true));
            let cleanup = retire(
                &mut owner,
                &mut workers,
                &mut state,
                &rx,
                Instant::now() + Duration::from_secs(5),
            );
            assert!(cleanup.complete());
            assert_eq!(state.outputs[0].as_deref(), Some(b"tail".as_slice()));
            let error = super::finish_retired_result(
                Ok(()),
                ProcessControl(&OperationControl::default()),
                &mut state,
                &cleanup,
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                if overflow {
                    "source.process_input_budget"
                } else {
                    "source.process_check_failed"
                }
            );
            if !overflow {
                assert!(error.message.contains("late reader failure"));
            }
            let primary = ToolkitError::io(io::Error::other("original root wait failure"));
            let error = super::finish_retired_result(
                Err(primary.clone()),
                ProcessControl(&OperationControl::default()),
                &mut state,
                &cleanup,
            )
            .unwrap_err();
            assert_eq!((error.code, error.message), (primary.code, primary.message));
        }
    }
}

#[test]
fn retired_completion_preserves_control_stdin_and_nonzero_precedence() {
    let cleanup = Retirement {
        reaped: true,
        stdin_complete: true,
        outputs_complete: true,
        job_empty: Some(true),
        ..Retirement::default()
    };
    for status in [0, 17] {
        let mut state = ProcessState::default();
        state.status = Some(status);
        state.input_error = Some(io::Error::other("late stdin failure"));
        let result = super::finish_retired_result(
            Ok(()),
            ProcessControl(&OperationControl::default()),
            &mut state,
            &cleanup,
        );
        if status == 0 {
            assert!(result.unwrap_err().message.contains("late stdin failure"));
        } else {
            result.unwrap();
        }
    }
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    let mut state = ProcessState::default();
    state.status = Some(0);
    let error = super::finish_retired_result(
        Ok(()),
        ProcessControl(&control),
        &mut state,
        &cleanup,
    )
    .unwrap_err();
    assert_eq!(error.code, "process.cancelled");
    let incomplete = Retirement::default();
    assert!(
        super::finish_retired_result(
            Ok(()),
            ProcessControl(&OperationControl::default()),
            &mut state,
            &incomplete
        )
        .is_err()
    );
}
