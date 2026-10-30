//! Bounded external tool transport. The suspended root, its Job and joined pipe
//! workers retain one operation control through natural completion.
use crate::{Result, ToolkitError, control::OperationControl};
use std::{
    io::{Read, Write},
    os::windows::io::IntoRawHandle,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
pub struct Output {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
enum Event {
    Pipe(usize, Vec<u8>, std::io::Result<()>, bool, Option<String>),
    Input(std::io::Result<()>, Option<String>),
}
const CLEANUP_GRACE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(10);
struct Owner {
    child: Option<Child>,
    job: Option<lycheedev_windows::JobGuard>,
    workers: [Option<JoinHandle<Event>>; 3],
    status: Option<i32>,
    output: [Vec<u8>; 2],
    input_error: Option<std::io::Error>,
    job_empty: Option<bool>,
    close_errors: Vec<String>,
    cleanup_errors: Vec<String>,
    terminated: bool,
}
impl Owner {
    fn spawn<P: IntoRawHandle + Send + 'static>(
        &mut self,
        index: usize,
        pipe: P,
        wake: mpsc::Sender<()>,
        work: impl FnOnce(P) -> Event + Send + 'static,
    ) -> Result<()> {
        // Retain the original pipe if Builder cannot transfer it to a thread.
        let custody = Arc::new(Mutex::new(Some(pipe)));
        let transferred = custody.clone();
        let started = thread::Builder::new()
            .name(format!("command-pipe-{index}"))
            .spawn(move || {
                let pipe = transferred.lock().unwrap().take().unwrap();
                let result = work(pipe);
                let _ = wake.send(());
                result
            });
        match started {
            Ok(worker) => self.workers[index] = Some(worker),
            Err(error) => {
                if let Some(pipe) = custody.lock().unwrap().take()
                    && let Err(error) = lycheedev_windows::guarded_spawn::close_owned_handle(pipe)
                {
                    self.close_errors.push(error);
                }
                return Err(ToolkitError::new(
                    "command.child_worker",
                    error.to_string(),
                    5,
                ));
            }
        }
        Ok(())
    }
    fn collect(&mut self) -> Result<()> {
        let mut failure = None;
        for index in 0..self.workers.len() {
            if !self.workers[index]
                .as_ref()
                .is_some_and(JoinHandle::is_finished)
            {
                continue;
            }
            let result = match self.workers[index].take().unwrap().join() {
                Err(_) => Err(ToolkitError::new(
                    "command.child_worker",
                    "pipe worker panicked",
                    5,
                )),
                Ok(event) => {
                    let (result, close) = match event {
                        Event::Input(result, close) => {
                            self.input_error = result.err();
                            (Ok(()), close)
                        }
                        Event::Pipe(index, bytes, read, overflow, close) => {
                            self.output[index] = bytes;
                            let result = read.map_err(ToolkitError::io).and_then(|()| {
                                if overflow {
                                    Err(ToolkitError::new(
                                        "command.child_output_limit",
                                        "external output exceeded its byte cap",
                                        3,
                                    ))
                                } else {
                                    Ok(())
                                }
                            });
                            (result, close)
                        }
                    };
                    if let Some(error) = close {
                        self.close_errors.push(error.clone());
                        result.and(Err(ToolkitError::new("command.child_close", error, 5)))
                    } else {
                        result
                    }
                }
            };
            if let Err(error) = result {
                if failure.is_some() {
                    self.cleanup_errors.push(error.to_string());
                } else {
                    failure = Some(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
    fn observe(&mut self) -> Result<()> {
        let mut failure = None;
        if self.status.is_none() {
            match self.child.as_mut().unwrap().try_wait() {
                Ok(Some(status)) => self.status = Some(status.code().unwrap_or(-1)),
                Ok(None) => (),
                Err(error) => failure = Some(ToolkitError::io(error)),
            }
        }
        // A failed root wait must not skip joining finished readers or observing
        // the exact Job during cleanup.
        let workers = self.collect();
        let job = self
            .job
            .as_ref()
            .unwrap()
            .memory_accounting()
            .map_err(|error| ToolkitError::new("command.child_job", error, 5));
        self.job_empty = job.as_ref().ok().map(|value| value.active_processes == 0);
        for next in [workers, job.map(|_| ())] {
            if let Err(error) = next {
                if failure.is_some() {
                    self.cleanup_errors.push(error.to_string());
                } else {
                    failure = Some(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }
    fn terminal(&self) -> bool {
        self.status.is_some()
            && self.workers.iter().all(Option::is_none)
            && self.job_empty == Some(true)
    }
    fn close(&mut self) {
        if let Some(child) = self.child.take()
            && let Err(error) = lycheedev_windows::guarded_spawn::close_child(child)
        {
            self.close_errors.push(error);
        }
        if let Some(job) = self.job.take()
            && let Err(error) = job.close()
        {
            self.close_errors.push(error);
        }
    }
    fn failure(
        &mut self,
        primary: ToolkitError,
        retirement: Instant,
        control: &OperationControl,
    ) -> ToolkitError {
        if let Some(job) = &self.job {
            match job.terminate() {
                Ok(()) => self.terminated = true,
                Err(error) => self.cleanup_errors.push(error),
            }
        }
        if self.status.is_none()
            && let Some(child) = &mut self.child
            && let Err(error) = child.kill()
            && !matches!(child.try_wait(), Ok(Some(_)))
        {
            self.cleanup_errors.push(error.to_string());
        }
        loop {
            if let Err(error) = self.observe() {
                let error = error.to_string();
                if !self.cleanup_errors.contains(&error) {
                    self.cleanup_errors.push(error);
                }
            }
            if self.terminal() || Instant::now() >= retirement {
                break;
            }
            thread::sleep(POLL.min(retirement.saturating_duration_since(Instant::now())));
        }
        let terminal = self.terminal();
        self.close();
        self.with_cleanup(primary, terminal, control)
    }
    fn with_cleanup(
        &self,
        primary: ToolkitError,
        terminal: bool,
        control: &OperationControl,
    ) -> ToolkitError {
        let details = serde_json::json!({
            "primary":primary, "cause":primary.details,
            "terminated":self.terminated, "reaped":self.status.is_some(),
            "stdinWriterComplete":self.workers[0].is_none(),
            "outputReadersComplete":self.workers[1..].iter().all(Option::is_none),
            "jobEmpty":self.job_empty, "ownedJobTerminal":terminal,
            "cleanupDeadlineExceeded":!terminal,
            "handleCloseErrors":self.close_errors,"cleanupErrors":self.cleanup_errors,
            "stdinError":self.input_error.as_ref().map(ToString::to_string),
            "control":control.check().err(),
        });
        primary.with_details(details)
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        // Unwind fallback does not issue completion evidence.
        if let Some(job) = &self.job {
            let _ = job.terminate();
        }
        if let Some(child) = &mut self.child {
            let _ = child.kill();
        }
    }
}
fn read_pipe<P: Read + IntoRawHandle>(index: usize, mut pipe: P, cap: usize) -> Event {
    let mut bytes = Vec::new();
    let result = pipe
        .by_ref()
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map(|_| ());
    let close = lycheedev_windows::guarded_spawn::close_owned_handle(pipe).err();
    let overflow = bytes.len() > cap;
    Event::Pipe(index, bytes, result, overflow, close)
}
fn startup_primary_error(cause: &str, stopped: Option<ToolkitError>) -> ToolkitError {
    // Snapshot cleanup can append errors to the original startup stop. Keep
    // that stop as the primary; the caller also retains the full failure.
    match cause.split([';', '\n']).next().map(str::trim) {
        Some("startup.cancelled" | "startup.deadline") => {
            stopped.unwrap_or_else(|| ToolkitError::new("command.child_guard", cause, 5))
        }
        _ => ToolkitError::new("command.child_guard", cause, 5),
    }
}
pub fn run(
    mut command: Command,
    input: Option<Vec<u8>>,
    stdout_cap: usize,
    stderr_cap: usize,
    control: &OperationControl,
) -> Result<Output> {
    control.check()?;
    if stdout_cap > 64 << 20 || stderr_cap > 16 << 20 {
        return Err(ToolkitError::new(
            "command.child_output_limit",
            "external output admission exceeded",
            3,
        ));
    }
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let (child, job) = match lycheedev_windows::guarded_spawn::spawn_suspended_job_guarded(
        command,
        control.deadline(),
        || control.check().is_err(),
    ) {
        Ok(owned) => owned,
        Err(mut failure) => {
            let stopped = control.check().err();
            let error = startup_primary_error(&failure.cause, stopped.clone());
            // Startup already spent its one retirement window. Consume any
            // returned custody without granting another timeout or retrying a
            // close whose outcome was uncertain.
            if let Some((mut child, job)) = failure.take_pending() {
                match child.try_wait() {
                    Ok(Some(_)) => failure.cleanup.child_waited = true,
                    Ok(None) => (),
                    Err(error) => {
                        failure.cleanup.error = Some(format!(
                            "{}; final wait: {error}",
                            failure
                                .cleanup
                                .error
                                .as_deref()
                                .unwrap_or("startup retirement")
                        ))
                    }
                }
                if let Some(job) = &job {
                    match job.memory_accounting() {
                        Ok(accounting) => {
                            failure.cleanup.job_empty = Some(accounting.active_processes == 0)
                        }
                        Err(error) => {
                            failure.cleanup.error = Some(format!(
                                "{}; final Job observation: {error}",
                                failure
                                    .cleanup
                                    .error
                                    .as_deref()
                                    .unwrap_or("startup retirement")
                            ))
                        }
                    }
                }
                if let Err(error) = lycheedev_windows::guarded_spawn::close_child(child) {
                    failure.cleanup.handle_close_errors.push(error);
                }
                if let Some(job) = job
                    && let Err(error) = job.close()
                {
                    failure.cleanup.handle_close_errors.push(error);
                }
            }
            let details = serde_json::json!({
                "primary":error,"startup":failure,"control":stopped,
                "terminated":failure.cleanup.terminate_requested,"reaped":failure.cleanup.child_waited,
                "jobEmpty":failure.cleanup.job_empty,"stdinWriterComplete":true,"outputReadersComplete":true,
                "handleCloseErrors":failure.cleanup.handle_close_errors,
            });
            return Err(error.with_details(details));
        }
    };
    let mut owner = Owner {
        child: Some(child),
        job: Some(job),
        workers: [None, None, None],
        status: None,
        output: [Vec::new(), Vec::new()],
        input_error: None,
        job_empty: None,
        close_errors: Vec::new(),
        cleanup_errors: Vec::new(),
        terminated: false,
    };
    let (wake, receive) = mpsc::channel();
    let execution = (|| {
        let stdout = owner.child.as_mut().unwrap().stdout.take().unwrap();
        owner.spawn(1, stdout, wake.clone(), move |pipe| {
            read_pipe(0, pipe, stdout_cap)
        })?;
        let stderr = owner.child.as_mut().unwrap().stderr.take().unwrap();
        owner.spawn(2, stderr, wake.clone(), move |pipe| {
            read_pipe(1, pipe, stderr_cap)
        })?;
        if let Some(input) = input {
            let stdin = owner.child.as_mut().unwrap().stdin.take().unwrap();
            owner.spawn(0, stdin, wake.clone(), move |mut pipe| {
                let result = pipe.write_all(&input);
                let close = lycheedev_windows::guarded_spawn::close_owned_handle(pipe).err();
                Event::Input(result, close)
            })?;
        }
        drop(wake);
        loop {
            control.check()?;
            owner.observe()?;
            control.check()?;
            if owner.terminal() {
                break;
            }
            let slice = control.remaining()?.min(POLL);
            if matches!(
                receive.recv_timeout(slice),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ) {
                thread::sleep(slice);
            }
        }
        if owner.status == Some(0)
            && let Some(error) = owner.input_error.take()
        {
            return Err(ToolkitError::io(error));
        }
        Ok(())
    })();
    if let Err(error) = execution {
        return Err(owner.failure(error, Instant::now() + CLEANUP_GRACE, control));
    }
    owner.close();
    let stopped = control.check().err();
    if !owner.close_errors.is_empty() {
        let primary = stopped.unwrap_or_else(|| {
            ToolkitError::new(
                "command.child_close",
                "external process handle close failed",
                5,
            )
        });
        return Err(owner.with_cleanup(primary, true, control));
    }
    if let Some(error) = stopped {
        return Err(owner.with_cleanup(error, true, control));
    }
    Ok(Output {
        status: owner.status.unwrap(),
        stdout: std::mem::take(&mut owner.output[0]),
        stderr: std::mem::take(&mut owner.output[1]),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_stop_retains_its_code_when_snapshot_cleanup_also_fails() {
        let cancelled = OperationControl::default();
        cancelled.tracked_cancel_handle().unwrap().cancel();
        let expired = OperationControl::uncancelable(Instant::now());
        for (startup, control) in [
            ("startup.cancelled", cancelled),
            ("startup.deadline", expired),
        ] {
            let stop = control.check().unwrap_err();
            for suffix in [
                "",
                "; startup.thread_snapshot_free:5",
                "\r\nstartup.thread_snapshot_marker_free:6",
            ] {
                let cause = format!("{startup}{suffix}");
                let primary = startup_primary_error(&cause, Some(stop.clone()));
                assert_eq!(primary.code, stop.code);
                assert_eq!(primary.exit_code, stop.exit_code);
                let fallback = startup_primary_error(&cause, None);
                assert_eq!(fallback.message, cause);
            }
            let unrelated = startup_primary_error("startup.thread_snapshot_free:5", Some(stop));
            assert_eq!(unrelated.code, "command.child_guard");
            assert_eq!(unrelated.message, "startup.thread_snapshot_free:5");
        }
    }
    #[test]
    fn helper() {
        match std::env::var("LYCHEEDEV_CORE_PROCESS_FIXTURE").as_deref() {
            Ok("block") => std::thread::sleep(Duration::from_secs(20)),
            Ok("output") => std::io::stdout().write_all(&vec![b'x'; 65536]).unwrap(),
            Ok("silent-descendant-root") => silent_descendant_root(),
            Ok("silent-descendant") => silent_descendant(),
            _ => {}
        }
    }
    fn command(mode: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "process::tests::helper", "--nocapture"])
            .env("LYCHEEDEV_CORE_PROCESS_FIXTURE", mode);
        cmd
    }
    #[test]
    fn deadline_and_output_overflow_reap_owned_child_and_blocked_stdin() {
        let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(100));
        let fault = run(
            command("block"),
            Some(vec![b'x'; 4 << 20]),
            1024,
            1024,
            &control,
        )
        .err()
        .unwrap();
        assert_eq!(fault.code, "command.deadline");
        assert_eq!(fault.exit_code, 5);
        assert_eq!(fault.details["terminated"], true);
        assert_eq!(fault.details["reaped"], true);
        assert_eq!(fault.details["stdinWriterComplete"], true);
        assert_eq!(fault.details["outputReadersComplete"], true);
        let fault = run(command("output"), None, 1024, 1024, &Default::default())
            .err()
            .unwrap();
        assert_eq!(fault.code, "command.child_output_limit");
        assert_eq!(fault.details["reaped"], true);
    }

    fn fixture_directory() -> std::path::PathBuf {
        std::env::var_os("LYCHEEDEV_CORE_DESCENDANT_FIXTURE")
            .expect("owned descendant fixture directory")
            .into()
    }
    fn silent_descendant() {
        let directory = fixture_directory();
        std::fs::write(directory.join("descendant-ready"), b"ready").unwrap();
        // Cooperative cleanup belongs to this exact fixture. The finite self
        // stop also covers a controller panic before it receives the identity.
        let deadline = Instant::now() + Duration::from_secs(15);
        while !directory.join("release").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn silent_descendant_root() {
        let directory = fixture_directory();
        // run starts its stdin writer only after assigning the root to its Job.
        // Thus the descendant cannot race ahead of that attachment.
        std::io::stdin().read_exact(&mut [0u8; 1]).unwrap();
        let mut descendant = command("silent-descendant")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let identity = lycheedev_windows::guarded_spawn::child_identity(&descendant).unwrap();
        let ready_deadline = Instant::now() + Duration::from_secs(5);
        while !directory.join("descendant-ready").exists() {
            if Instant::now() >= ready_deadline {
                descendant.kill().unwrap();
                descendant.wait().unwrap();
                lycheedev_windows::guarded_spawn::close_child(descendant).unwrap();
                panic!("descendant did not start");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let root = lycheedev_windows::Process::open(std::process::id()).unwrap();
        crate::storage::atomic_json(&directory.join("root-identity.json"), root.identity())
            .unwrap();
        drop(root);
        crate::storage::atomic_json(&directory.join("identity.json"), &identity).unwrap();
        // The controller verifies the exact creation identity before allowing
        // root exit, so a fast normal return cannot hide an unstarted fixture.
        let ack_deadline = Instant::now() + Duration::from_secs(5);
        while !directory.join("root-may-exit").exists() {
            if Instant::now() >= ack_deadline {
                descendant.kill().unwrap();
                descendant.wait().unwrap();
                lycheedev_windows::guarded_spawn::close_child(descendant).unwrap();
                panic!("controller did not acknowledge descendant identity");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        lycheedev_windows::guarded_spawn::close_child(descendant).unwrap();
        std::fs::write(directory.join("root-returning"), b"root returning").unwrap();
    }

    struct OwnedDescendantCase {
        directory: std::path::PathBuf,
        control: OperationControl,
        worker: Option<std::thread::JoinHandle<Result<Output>>>,
    }
    impl OwnedDescendantCase {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "lycheedev-core-owned-descendant-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            ));
            std::fs::create_dir(&directory).unwrap();
            let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
            let operation = control.clone();
            let mut root = command("silent-descendant-root");
            root.env("LYCHEEDEV_CORE_DESCENDANT_FIXTURE", &directory);
            let worker =
                std::thread::spawn(move || run(root, Some(vec![b'G']), 4096, 4096, &operation));
            Self {
                directory,
                control,
                worker: Some(worker),
            }
        }
        fn identity(&self) -> lycheedev_windows::ProcessIdentity {
            loop {
                match crate::storage::read_json(&self.directory.join("identity.json")) {
                    Ok(identity) => return identity,
                    Err(error) => {
                        self.control
                            .check()
                            .unwrap_or_else(|_| panic!("descendant startup: {error}"));
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
        }
        fn cleanup(&mut self) -> std::result::Result<(), String> {
            // Always release both fixture gates before joining an unfinished
            // run. This cleanup does not grant the operation a new deadline.
            let _ = std::fs::write(self.directory.join("release"), b"release");
            let _ = std::fs::write(self.directory.join("root-may-exit"), b"exit");
            if let Some(worker) = self.worker.take() {
                self.control.tracked_cancel_handle().unwrap().cancel();
                let _ = worker.join().map_err(|_| "owned run worker panicked")?;
            }
            let identity: lycheedev_windows::ProcessIdentity =
                match crate::storage::read_json(&self.directory.join("identity.json")) {
                    Ok(identity) => identity,
                    Err(_) => return Ok(()),
                };
            let retirement = Instant::now() + Duration::from_secs(16);
            loop {
                use lycheedev_windows::process::ProcessLifetime;
                match lycheedev_windows::process::observe_lifetime(&identity, || Ok(()))? {
                    ProcessLifetime::Absent
                    | ProcessLifetime::Exited
                    | ProcessLifetime::PidReused => return Ok(()),
                    ProcessLifetime::Running if Instant::now() < retirement => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    ProcessLifetime::Running => {
                        return Err("original fixture descendant was not reaped".into());
                    }
                }
            }
        }
        fn observe_exited_root_and_live_descendant(
            &self,
            descendant: &lycheedev_windows::ProcessIdentity,
        ) {
            use lycheedev_windows::process::{ProcessLifetime, observe_lifetime};
            let root: lycheedev_windows::ProcessIdentity =
                crate::storage::read_json(&self.directory.join("root-identity.json")).unwrap();
            loop {
                self.control
                    .check()
                    .expect("root must exit before the original deadline");
                if self.directory.join("root-returning").exists()
                    && observe_lifetime(&root, || Ok(())).unwrap() != ProcessLifetime::Running
                {
                    assert_eq!(
                        observe_lifetime(descendant, || Ok(())).unwrap(),
                        ProcessLifetime::Running
                    );
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    impl Drop for OwnedDescendantCase {
        fn drop(&mut self) {
            if self.cleanup().is_ok() {
                let _ = std::fs::remove_dir_all(&self.directory);
            }
        }
    }

    #[test]
    fn silent_descendant_keeps_original_job_until_deadline_and_checked_reap() {
        let mut owned = OwnedDescendantCase::new();
        let identity = owned.identity();
        assert_eq!(
            lycheedev_windows::process::observe_lifetime(&identity, || Ok(())).unwrap(),
            lycheedev_windows::process::ProcessLifetime::Running,
        );
        std::fs::write(owned.directory.join("root-may-exit"), b"exit").unwrap();
        owned.observe_exited_root_and_live_descendant(&identity);
        let result = owned
            .worker
            .take()
            .unwrap()
            .join()
            .expect("owned run worker");
        // Reap using the original PID/creation record before asserting the red
        // result, so a failed business assertion cannot strand the fixture.
        owned.cleanup().expect("owned descendant cleanup");
        let error = result
            .err()
            .expect("root exit0 must not complete a live descendant Job");
        assert_eq!(error.code, "command.deadline");
        assert_eq!(error.exit_code, 5);
        assert_eq!(error.details["reaped"], true);
        assert_eq!(error.details["jobEmpty"], true);
        assert_eq!(error.details["handleCloseErrors"], serde_json::json!([]));
    }
    #[test]
    fn silent_descendant_natural_exit_returns_root_result_after_checked_reap() {
        let mut owned = OwnedDescendantCase::new();
        let identity = owned.identity();
        std::fs::write(owned.directory.join("root-may-exit"), b"exit").unwrap();
        owned.observe_exited_root_and_live_descendant(&identity);
        assert!(!owned.worker.as_ref().unwrap().is_finished());
        std::fs::write(owned.directory.join("release"), b"release").unwrap();
        let result = owned
            .worker
            .take()
            .unwrap()
            .join()
            .expect("owned run worker");
        owned.cleanup().expect("owned descendant cleanup");
        assert_eq!(result.unwrap().status, 0);
        owned
            .control
            .check()
            .expect("natural completion retains the original deadline");
    }
}
