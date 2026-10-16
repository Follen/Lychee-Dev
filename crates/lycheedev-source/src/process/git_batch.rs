//! Fixed `git cat-file --batch` transport for demand-only pinned object reads.
//! The duplex worker owns both pipes, but only reads an object after a request.
//! Root, exact Job and workers retire through the ordinary process owner.
use super::{Event, OwnedProcess, ProcessState, Retirement, WorkerSet, lifetime};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, selection::valid_digest};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    time::Instant,
};

const STDERR_CAP: usize = 32 << 10;

struct Demand {
    object: String,
    kind: String,
    maximum_bytes: usize,
    reply: SyncSender<Result<Vec<u8>>>,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum SpawnFault {
    None,
    Attach,
    DuplexSpawn,
    CancelAfterAttach,
}

pub(crate) struct GitBatchProcess {
    child: Child,
    job: Option<lycheedev_windows::JobGuard>,
    workers: WorkerSet,
    state: ProcessState,
    progress: Receiver<()>,
    demand: Option<SyncSender<Demand>>,
    terminal: Receiver<Result<()>>,
    terminal_result: Option<Result<()>>,
    duplex_started: bool,
    // A clone shares cancellation ancestry and first-stop state. There is no
    // replacement scope, raw token export or new per-object execution budget.
    original_control: OperationControl,
    failure: Option<ToolkitError>,
    retirement: Option<Retirement>,
}

impl GitBatchProcess {
    #[cfg(test)]
    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn spawn(directory: &Path, control: &OperationControl) -> Result<Self> {
        control.check()?;
        Self::spawn_fixed(crate::git::batch_command(directory), control)
    }

    // Kept private: production callers cannot turn this into a general-purpose
    // interactive process API. Tests reuse the ordinary owned helper process.
    fn spawn_fixed(command: Command, control: &OperationControl) -> Result<Self> {
        #[cfg(test)]
        {
            Self::spawn_inner(command, control, SpawnFault::None)
        }
        #[cfg(not(test))]
        {
            Self::spawn_inner(command, control)
        }
    }

    fn spawn_inner(
        mut command: Command,
        control: &OperationControl,
        #[cfg(test)] fault: SpawnFault,
    ) -> Result<Self> {
        control.check()?;
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                ToolkitError::new("codebase.source_not_ready", error.to_string(), 3)
            })?;
        let (wake, progress) = mpsc::channel();
        let (demand, requests) = mpsc::sync_channel(1);
        let (terminal_sender, terminal) = mpsc::sync_channel(1);
        let mut process = Self {
            child,
            job: None,
            workers: WorkerSet::default(),
            state: ProcessState::default(),
            progress,
            demand: Some(demand),
            terminal,
            terminal_result: None,
            duplex_started: false,
            original_control: control.clone(),
            failure: None,
            retirement: None,
        };
        let result = (|| {
            #[cfg(test)]
            if fault == SpawnFault::Attach {
                return Err(ToolkitError::new(
                    "codebase.source_not_ready",
                    "fixture job attachment failed",
                    3,
                ));
            }
            process.job = Some(
                lycheedev_windows::JobGuard::attach(&process.child)
                    .map_err(|error| ToolkitError::new("codebase.source_not_ready", error, 3))?,
            );
            #[cfg(test)]
            if fault == SpawnFault::CancelAfterAttach {
                control.tracked_cancel_handle()?.cancel();
            }
            process.check(control)?;
            let input = process.child.stdin.take().expect("piped Git stdin");
            let output = process.child.stdout.take().expect("piped Git stdout");
            let work =
                move || duplex_event(input, BufReader::new(output), requests, terminal_sender);
            #[cfg(test)]
            if fault == SpawnFault::DuplexSpawn {
                // Builder::spawn failure drops this same owned closure. Inject
                // that boundary without exhausting real system thread limits.
                drop(work);
                return Err(map_process_error(ToolkitError::new(
                    "luals.unavailable",
                    "fixture duplex worker could not start",
                    3,
                )));
            }
            process
                .workers
                .spawn(0, wake.clone(), work)
                .map_err(map_process_error)?;
            process.duplex_started = true;
            let stderr = process.child.stderr.take().expect("piped Git stderr");
            process
                .workers
                .spawn(2, wake, move || {
                    lifetime::read_output(1, stderr, STDERR_CAP)
                })
                .map_err(map_process_error)?;
            process.check(control)
        })();
        if let Err(error) = result {
            return Err(process.fail(error));
        }
        Ok(process)
    }

    pub(crate) fn request(
        &mut self,
        object: &str,
        kind: &str,
        maximum_bytes: usize,
        control: &OperationControl,
    ) -> Result<Vec<u8>> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = self.request_inner(object, kind, maximum_bytes, control);
        result.map_err(|error| self.fail(error))
    }

    fn request_inner(
        &mut self,
        object: &str,
        kind: &str,
        maximum_bytes: usize,
        control: &OperationControl,
    ) -> Result<Vec<u8>> {
        self.check(control)?;
        validate_request(object, kind, maximum_bytes)?;
        let (reply, response) = mpsc::sync_channel(1);
        let mut pending = Demand {
            object: object.to_owned(),
            kind: kind.to_owned(),
            maximum_bytes,
            reply,
        };
        loop {
            self.check(control)?;
            let sender = self
                .demand
                .as_ref()
                .ok_or_else(|| failed("Git object reader is closed"))?;
            match sender.try_send(pending) {
                Ok(()) => break,
                Err(TrySendError::Full(request)) => pending = request,
                Err(TrySendError::Disconnected(_)) => {
                    self.observe()?;
                    self.check_terminal()?;
                    return Err(failed(
                        "Git object reader stopped before accepting a request",
                    ));
                }
            }
            // &mut self normally makes a full queue impossible. Still never
            // block in send: a broken protocol stays cancellable and bounded.
            self.observe()?;
            self.check_terminal()?;
            self.wait(control);
        }
        loop {
            self.check(control)?;
            // The reply itself wakes this wait, without an accumulating wakeup
            // queue or a fixed polling delay on every successful small object.
            match response.recv_timeout(self.wait_duration(control)) {
                Ok(result) => {
                    self.check(control)?;
                    return result;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.check(control)?;
                    self.observe()?;
                    self.check_terminal()?;
                    if self.workers.complete() {
                        return Err(failed("Git object reply channel disconnected"));
                    }
                    // A panic drops its reply before the JoinHandle becomes
                    // joinable. Wait for that real terminal evidence, not EOF.
                    self.wait(control);
                }
            }
            self.check(control)?;
            self.observe()?;
            self.check_terminal()?;
        }
    }

    pub(crate) fn finish<T>(mut self, result: Result<T>, control: &OperationControl) -> Result<T> {
        // A callback's already-selected error wins over tail errors or a stop
        // observed while retiring. No further object is requested here.
        let result = result.and_then(|value| {
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.finish_stream(control)?;
            Ok(value)
        });
        let cleanup = self.retire_once(result.is_err());
        match result {
            Err(error) => Err(with_cleanup(error, cleanup)),
            Ok(value) => {
                if let Some(error) = &cleanup.first_worker_error {
                    return Err(with_cleanup(map_process_error(error.clone()), cleanup));
                }
                if !cleanup.complete() || !cleanup.errors.is_empty() {
                    return Err(with_cleanup(
                        failed("Git process retirement is unconfirmed"),
                        cleanup,
                    ));
                }
                self.check(control).map_err(|error| {
                    with_cleanup(
                        error,
                        self.retirement.as_ref().expect("retired Git process"),
                    )
                })?;
                Ok(value)
            }
        }
    }

    fn finish_stream(&mut self, control: &OperationControl) -> Result<()> {
        self.demand.take();
        loop {
            self.check(control)?;
            self.observe()?;
            self.check_terminal()?;
            if self.workers.complete() && self.state.status.is_some() {
                if !matches!(self.terminal_result, Some(Ok(()))) {
                    return Err(failed("Git object worker omitted its terminal result"));
                }
                self.check(control)?;
                if self.state.status != Some(0) {
                    let stderr = self.state.outputs[1].as_deref().unwrap_or_default();
                    let message = String::from_utf8_lossy(stderr);
                    return Err(failed(&format!(
                        "Git object process exited with status {}: {}",
                        self.state.status.unwrap_or(-1),
                        message.trim()
                    )));
                }
                return Ok(());
            }
            self.wait(control);
        }
    }

    fn check(&self, control: &OperationControl) -> Result<()> {
        self.original_control.check()?;
        control.check()
    }

    fn wait_duration(&self, control: &OperationControl) -> std::time::Duration {
        self.original_control
            .deadline()
            .min(control.deadline())
            .saturating_duration_since(Instant::now())
            .min(lifetime::POLL_INTERVAL)
    }

    fn wait(&self, control: &OperationControl) {
        lifetime::wait_for_progress(&self.progress, self.wait_duration(control));
    }

    fn observe(&mut self) -> Result<()> {
        self.workers
            .collect(&mut self.state)
            .map_err(map_process_error)?;
        if self.state.status.is_none() {
            self.state
                .observe_wait(
                    self.child
                        .try_wait()
                        .map(|status| status.map(|status| status.code().unwrap_or(-1))),
                )
                .map_err(map_process_error)?;
        }
        self.receive_terminal();
        Ok(())
    }

    fn receive_terminal(&mut self) {
        if self.duplex_started && self.terminal_result.is_none() {
            match self.terminal.try_recv() {
                Ok(result) => self.terminal_result = Some(result),
                Err(TryRecvError::Disconnected) if self.workers.complete() => {
                    self.terminal_result = Some(Err(failed("Git object worker disconnected")));
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
            }
        }
    }

    fn check_terminal(&self) -> Result<()> {
        match &self.terminal_result {
            Some(Err(error)) => Err(error.clone()),
            _ => Ok(()),
        }
    }

    fn fail(&mut self, error: ToolkitError) -> ToolkitError {
        if let Some(first) = &self.failure {
            return first.clone();
        }
        let error = with_cleanup(error, self.retire_once(true));
        self.failure = Some(error.clone());
        error
    }

    fn retire_once(&mut self, failed: bool) -> &Retirement {
        if self.retirement.is_none() {
            self.demand.take();
            let deadline = Instant::now() + lifetime::CLEANUP_GRACE;
            let deadline = if failed {
                deadline
            } else {
                deadline.min(self.original_control.deadline())
            };
            let mut owned = OwnedProcess {
                child: &mut self.child,
                job: self.job.as_ref(),
            };
            let mut cleanup = lifetime::retire(
                &mut owned,
                &mut self.workers,
                &mut self.state,
                &self.progress,
                deadline,
            );
            // Slot 0 owns stdin AND stdout. An unfinished duplex worker must
            // never claim stdout completion merely because slot 1 is unused.
            cleanup.outputs_complete &= cleanup.stdin_complete;
            self.receive_terminal();
            if let Some(Err(error)) = &self.terminal_result {
                let message = format!("Git object worker: {error}");
                if !cleanup.errors.contains(&message) {
                    cleanup.errors.push(message);
                }
                cleanup
                    .first_worker_error
                    .get_or_insert_with(|| error.clone());
            }
            self.retirement = Some(cleanup);
        }
        self.retirement.as_ref().expect("Git process retired once")
    }
}

impl Drop for GitBatchProcess {
    fn drop(&mut self) {
        // Covers abandoned readers and callback unwinding. Unconfirmed workers
        // retain exactly the ordinary process owner's bounded-cleanup model.
        self.retire_once(true);
    }
}

fn validate_request(object: &str, kind: &str, maximum_bytes: usize) -> Result<()> {
    if !valid_digest(object, 40) || !matches!(kind, "blob" | "tree" | "commit" | "tag") {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Invalid fixed Git object request",
            4,
        ));
    }
    if maximum_bytes > crate::git::MAX_SOURCE_BYTES {
        return Err(ToolkitError::new(
            "codebase.source_budget",
            "Git object request exceeds the source byte budget",
            3,
        ));
    }
    Ok(())
}

fn duplex(
    mut input: impl Write,
    mut output: impl BufRead,
    requests: Receiver<Demand>,
) -> Result<()> {
    while let Ok(request) = requests.recv() {
        let result = (|| {
            input
                .write_all(request.object.as_bytes())
                .map_err(io_failed)?;
            input.write_all(b"\n").map_err(io_failed)?;
            input.flush().map_err(io_failed)?;
            crate::git::read_object_frame(
                &mut output,
                &request.object,
                &request.kind,
                request.maximum_bytes,
            )
            .map_err(map_process_error)
        })();
        let failure = result.as_ref().err().cloned();
        match request.reply.try_send(result) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                return Err(failed("Git object reply slot was already occupied"));
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(
                    failure.unwrap_or_else(|| failed("Git object reply receiver disconnected"))
                );
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
    }
    // Successful finish first sends EOF to Git, then demands a real stdout EOF.
    // Any tail, broken read or inherited open pipe is handled under the same
    // original parent deadline; retirement never impersonates successful EOF.
    drop(input);
    let mut tail = [0];
    if output.read(&mut tail).map_err(io_failed)? != 0 {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Trailing Git batch output",
            4,
        ));
    }
    Ok(())
}

fn duplex_event(
    input: impl Write,
    output: impl BufRead,
    requests: Receiver<Demand>,
    terminal: SyncSender<Result<()>>,
) -> Event {
    let result = duplex(input, output, requests);
    // The single terminal slot is never shared with object bodies; an abandoned
    // request cannot block ownership completion. Panics propagate to WorkerSet's
    // joined panic result and drop all three channel/pipe owners on this stack.
    let _ = terminal.try_send(result);
    Event::Output(0, Vec::new(), Ok(()), false)
}

fn failed(message: &str) -> ToolkitError {
    ToolkitError::new("codebase.git_failed", message, 5)
}

fn io_failed(error: std::io::Error) -> ToolkitError {
    failed(&format!("Git object pipe failed: {error}"))
}

fn map_process_error(mut error: ToolkitError) -> ToolkitError {
    error.code = match error.code.as_str() {
        "luals.unavailable" => "codebase.source_not_ready",
        "luals.input_budget" => "codebase.source_budget",
        "process.cancelled" => "command.cancelled",
        "luals.check_failed" | "external.io" => "codebase.git_failed",
        _ => return error,
    }
    .into();
    error
}

fn with_cleanup(mut error: ToolkitError, cleanup: &Retirement) -> ToolkitError {
    let details = cleanup.details();
    if details.as_object().is_some_and(|fields| {
        fields
            .iter()
            .all(|(key, value)| error.details.get(key) == Some(value))
    }) {
        return error;
    }
    let original = std::mem::replace(&mut error.details, details);
    if !original.is_null() {
        error.details["primaryErrorDetails"] = original;
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha1::{Digest, Sha1};
    use std::{
        io::{Cursor, Read},
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        thread,
        time::Duration,
    };

    fn object(body: &[u8]) -> (String, Vec<u8>) {
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", body.len()));
        hash.update(body);
        let hash = format!("{:x}", hash.finalize());
        let mut frame = format!("{hash} blob {}\n", body.len()).into_bytes();
        frame.extend_from_slice(body);
        frame.push(b'\n');
        (hash, frame)
    }

    fn demand(sender: &SyncSender<Demand>, object: &str) -> Receiver<Result<Vec<u8>>> {
        let (reply, response) = mpsc::sync_channel(1);
        sender
            .try_send(Demand {
                object: object.to_owned(),
                kind: "blob".into(),
                maximum_bytes: 1024,
                reply,
            })
            .unwrap_or_else(|_| panic!("single demand slot must be available"));
        response
    }

    fn response(receiver: Receiver<Result<Vec<u8>>>) -> Result<Vec<u8>> {
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("bounded worker response")
    }

    struct Writer {
        bytes: Arc<Mutex<Vec<u8>>>,
        closed: Arc<AtomicBool>,
        broken: bool,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.broken {
                return Err(std::io::ErrorKind::BrokenPipe.into());
            }
            self.bytes.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Drop for Writer {
        fn drop(&mut self) {
            self.closed.store(true, Ordering::Release);
        }
    }
    struct Reader {
        bytes: Cursor<Vec<u8>>,
        consumed: Arc<AtomicUsize>,
        input_closed: Arc<AtomicBool>,
    }
    impl Read for Reader {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let n = self.bytes.read(bytes)?;
            self.consumed.fetch_add(n, Ordering::AcqRel);
            if n == 0 {
                assert!(
                    self.input_closed.load(Ordering::Acquire),
                    "stdin closes before EOF check"
                );
            }
            Ok(n)
        }
    }
    impl BufRead for Reader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            self.bytes.fill_buf()
        }
        fn consume(&mut self, n: usize) {
            self.bytes.consume(n);
            self.consumed.fetch_add(n, Ordering::AcqRel);
        }
    }

    // Emit the exact bounded prefix, then fail instead of returning EOF. An
    // empty prefix exercises header fill_buf; a partial body exercises Read.
    struct BrokenReader(Cursor<Vec<u8>>);
    impl Read for BrokenReader {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if self.0.position() as usize == self.0.get_ref().len() {
                return Err(std::io::Error::other("fixture stdout I/O error"));
            }
            self.0.read(bytes)
        }
    }
    impl BufRead for BrokenReader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            if self.0.position() as usize == self.0.get_ref().len() {
                return Err(std::io::Error::other("fixture stdout I/O error"));
            }
            self.0.fill_buf()
        }
        fn consume(&mut self, count: usize) {
            self.0.consume(count);
        }
    }

    #[test]
    fn header_and_body_stdout_io_failures_keep_git_transport_classification() {
        let (hash, _) = object(b"data");
        for (phase, prefix) in [
            ("header", Vec::new()),
            ("body", format!("{hash} blob 4\nd").into_bytes()),
        ] {
            let (sender, requests) = mpsc::sync_channel(1);
            let reply = demand(&sender, &hash);
            let terminal =
                duplex(Vec::new(), BrokenReader(Cursor::new(prefix)), requests).unwrap_err();
            let error = response(reply).unwrap_err();
            assert_eq!(error.code, "codebase.git_failed", "{phase}");
            assert_eq!(error.exit_code, 5, "{phase}");
            assert!(
                error.message.contains("fixture stdout I/O error"),
                "{phase}"
            );
            assert_eq!(terminal.code, error.code, "{phase}");
            assert_eq!(terminal.message, error.message, "{phase}");
        }
    }

    #[test]
    fn transport_mapping_preserves_frame_integrity_and_budget_rejections() {
        let (hash, _) = object(b"data");
        for (frame, code, exit_code) in [
            (
                b"malformed header\n".to_vec(),
                "codebase.source_integrity",
                4,
            ),
            (
                format!("{hash} blob 1025\n").into_bytes(),
                "codebase.source_budget",
                3,
            ),
        ] {
            let (sender, requests) = mpsc::sync_channel(1);
            let reply = demand(&sender, &hash);
            let terminal = duplex(Vec::new(), Cursor::new(frame), requests).unwrap_err();
            let error = response(reply).unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(error.exit_code, exit_code);
            assert_eq!(terminal.code, code);
            assert_eq!(terminal.message, error.message);
        }
    }

    #[test]
    fn fixed_request_admission_rejects_revision_expressions_and_excess_cap() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        for invalid in [
            "HEAD",
            "--batch",
            "",
            "0123456789abcdef0123456789abcdef01234567\n",
        ] {
            assert_eq!(
                validate_request(invalid, "blob", 1).unwrap_err().code,
                "codebase.source_integrity"
            );
        }
        assert!(validate_request(hash, "blob", crate::git::MAX_SOURCE_BYTES).is_ok());
        assert!(validate_request(hash, "blob", 0).is_ok());
        assert_eq!(
            validate_request(hash, "unknown", 1).unwrap_err().code,
            "codebase.source_integrity"
        );
        assert_eq!(
            validate_request(hash, "blob", crate::git::MAX_SOURCE_BYTES + 1)
                .unwrap_err()
                .code,
            "codebase.source_budget"
        );
    }

    #[test]
    fn duplex_only_reads_demanded_object_and_closes_stdin_before_eof() {
        let (hash, frame) = object(b"first");
        let frame_length = frame.len();
        let written = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let consumed = Arc::new(AtomicUsize::new(0));
        let (sender, requests) = mpsc::sync_channel(1);
        let input = Writer {
            bytes: written.clone(),
            closed: closed.clone(),
            broken: false,
        };
        let output = Reader {
            bytes: Cursor::new(frame),
            consumed: consumed.clone(),
            input_closed: closed.clone(),
        };
        let worker = thread::spawn(move || duplex(input, output, requests));
        assert!(written.lock().unwrap().is_empty());
        assert_eq!(consumed.load(Ordering::Acquire), 0);
        assert_eq!(response(demand(&sender, &hash)).unwrap(), b"first");
        assert_eq!(consumed.load(Ordering::Acquire), frame_length);
        assert_eq!(
            written.lock().unwrap().as_slice(),
            format!("{hash}\n").as_bytes()
        );
        assert!(
            !closed.load(Ordering::Acquire),
            "worker waits for the next demand"
        );
        drop(sender);
        worker.join().unwrap().unwrap();
        assert!(closed.load(Ordering::Acquire));
    }

    #[test]
    fn later_corrupt_object_does_not_fail_the_earlier_callback_boundary() {
        let (hash, mut frame) = object(b"earlier");
        let boundary = frame.len();
        frame.extend_from_slice(b"bad next header\n");
        let consumed = Arc::new(AtomicUsize::new(0));
        let (sender, requests) = mpsc::sync_channel(1);
        let output = Reader {
            bytes: Cursor::new(frame),
            consumed: consumed.clone(),
            input_closed: Arc::new(AtomicBool::new(false)),
        };
        let worker = thread::spawn(move || duplex(Vec::new(), output, requests));
        assert_eq!(response(demand(&sender, &hash)).unwrap(), b"earlier");
        assert_eq!(
            consumed.load(Ordering::Acquire),
            boundary,
            "later header untouched before callback"
        );
        let error = response(demand(&sender, &hash)).unwrap_err();
        assert_eq!(error.code, "codebase.source_integrity");
        let terminal = worker.join().unwrap().unwrap_err();
        assert_eq!(terminal.message, error.message);
    }

    #[test]
    fn duplex_checks_tail_only_after_demand_input_closes() {
        let (hash, mut frame) = object(b"earlier");
        frame.extend_from_slice(b"unexpected tail");
        let (sender, requests) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || duplex(Vec::new(), Cursor::new(frame), requests));
        assert_eq!(response(demand(&sender, &hash)).unwrap(), b"earlier");
        drop(sender);
        let error = worker.join().unwrap().unwrap_err();
        assert_eq!(error.code, "codebase.source_integrity");
        assert_eq!(error.message, "Trailing Git batch output");
    }

    #[test]
    fn broken_input_and_abandoned_reply_are_terminal_and_never_block() {
        let (hash, frame) = object(b"data");
        let (sender, requests) = mpsc::sync_channel(1);
        let closed = Arc::new(AtomicBool::new(false));
        let input = Writer {
            bytes: Arc::new(Mutex::new(Vec::new())),
            closed: closed.clone(),
            broken: true,
        };
        let worker = thread::spawn(move || duplex(input, Cursor::new(frame), requests));
        let error = response(demand(&sender, &hash)).unwrap_err();
        assert_eq!(error.code, "codebase.git_failed");
        assert_eq!(worker.join().unwrap().unwrap_err().message, error.message);
        assert!(closed.load(Ordering::Acquire));

        let (hash, frame) = object(b"data");
        let (sender, requests) = mpsc::sync_channel(1);
        drop(demand(&sender, &hash));
        let error = duplex(Vec::new(), Cursor::new(frame), requests).unwrap_err();
        assert_eq!(error.message, "Git object reply receiver disconnected");
    }

    #[test]
    fn occupied_reply_slot_is_a_terminal_protocol_error_not_a_blocking_send() {
        let (hash, frame) = object(b"data");
        let (sender, requests) = mpsc::sync_channel(1);
        let (reply, response) = mpsc::sync_channel(1);
        reply.try_send(Ok(Vec::new())).unwrap();
        sender
            .try_send(Demand {
                object: hash,
                kind: "blob".into(),
                maximum_bytes: 1024,
                reply,
            })
            .unwrap_or_else(|_| panic!("one demand slot"));
        let error = duplex(Vec::new(), Cursor::new(frame), requests).unwrap_err();
        assert_eq!(error.message, "Git object reply slot was already occupied");
        assert!(response.try_recv().unwrap().unwrap().is_empty());
    }

    struct PanicReader;
    impl Read for PanicReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("fixture reader panic")
        }
    }
    impl BufRead for PanicReader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            panic!("fixture reader panic")
        }
        fn consume(&mut self, _: usize) {
            panic!("fixture reader panic")
        }
    }

    #[test]
    fn duplex_panic_is_joined_by_existing_worker_owner_and_closes_channels() {
        let (hash, _) = object(b"data");
        let (sender, requests) = mpsc::sync_channel(1);
        let response = demand(&sender, &hash);
        let (terminal_sender, terminal) = mpsc::sync_channel(1);
        let (wake, progress) = mpsc::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let input = Writer {
            bytes: Arc::new(Mutex::new(Vec::new())),
            closed: closed.clone(),
            broken: false,
        };
        let mut workers = WorkerSet::default();
        workers
            .spawn(0, wake, move || {
                duplex_event(input, PanicReader, requests, terminal_sender)
            })
            .unwrap();
        let mut state = ProcessState::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        let error = loop {
            if let Err(error) = workers.collect(&mut state) {
                break map_process_error(error);
            }
            assert!(
                Instant::now() < deadline,
                "panicked worker must become joinable"
            );
            lifetime::wait_for_progress(&progress, lifetime::POLL_INTERVAL);
        };
        assert_eq!(error.code, "codebase.git_failed");
        assert!(error.message.contains("panicked"));
        assert!(workers.complete());
        assert!(closed.load(Ordering::Acquire));
        assert!(matches!(
            response.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
        assert!(matches!(
            terminal.try_recv(),
            Err(TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn callback_unwind_drops_and_retires_the_original_child() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = (0..128)
            .find_map(|_| {
                let path = std::env::temp_dir().join(format!(
                    "lychee-git-duplex-drop-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => Some(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => panic!("exclusive helper directory: {error}"),
                }
            })
            .expect("exclusive helper directory");
        let ready = directory.join("held-until-child-exits");
        let mut command = blocked_helper();
        command
            .env("LYCHEEDEV_SOURCE_TEST_PROCESS", "descendant-block")
            .env("LYCHEEDEV_SOURCE_TEST_READY", &ready);
        let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(10));
        let process = GitBatchProcess::spawn_fixed(command, &control).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.is_file() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            ready.is_file(),
            "owned child acquired its exclusive file handle"
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _reader = process;
            panic!("fixture callback unwind");
        }));
        assert!(result.is_err());
        // The helper opens with share_mode(0), so this proves its held file was
        // released by the same Drop retirement. Failed cleanup retains scratch.
        std::fs::remove_file(&ready).expect("Drop retired the owned child handle");
        std::fs::remove_dir(directory).unwrap();
    }

    fn blocked_helper() -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "process::tests::helper_process", "--nocapture"])
            .env("LYCHEEDEV_SOURCE_TEST_PROCESS", "block");
        command
    }

    fn assert_retired(error: &ToolkitError) {
        for field in [
            "reaped",
            "stdinWriterComplete",
            "outputReadersComplete",
            "jobEmpty",
            "ownedJobTerminal",
        ] {
            assert_eq!(error.details[field], true, "{field}: {:?}", error.details);
        }
    }

    #[test]
    fn already_cancelled_control_prevents_process_spawn() {
        let control = OperationControl::default();
        control.tracked_cancel_handle().unwrap().cancel();
        let error = GitBatchProcess::spawn_fixed(
            Command::new("must-not-start-cancelled-git-fixture"),
            &control,
        )
        .err()
        .expect("cancelled before executable lookup");
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.exit_code, 7);
        assert!(error.details.is_null(), "no Child was created or retired");
    }

    #[test]
    fn cancellation_after_spawn_and_attach_retires_before_any_worker_starts() {
        use lycheedev_core::control::StopCause;
        let control = OperationControl::default();
        let error =
            GitBatchProcess::spawn_inner(blocked_helper(), &control, SpawnFault::CancelAfterAttach)
                .err()
                .expect("post-spawn cancellation is observed");
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(control.stop_cause().unwrap(), Some(StopCause::Cancelled));
        assert_retired(&error);
        assert_eq!(error.details["cleanupErrors"], serde_json::json!([]));
    }

    #[test]
    fn attach_failure_reaps_root_and_keeps_unattached_job_scope_unconfirmed() {
        let control = OperationControl::default();
        let error = GitBatchProcess::spawn_inner(blocked_helper(), &control, SpawnFault::Attach)
            .err()
            .expect("injected attachment failure");
        assert_eq!(error.code, "codebase.source_not_ready");
        assert_eq!(error.message, "fixture job attachment failed");
        // A root can exit when its parent closes stdio before kill_root. Reap
        // is the required fact; the fixture does not require that race's order.
        assert_eq!(error.details["terminated"], false);
        assert_eq!(error.details["reaped"], true);
        assert_eq!(error.details["stdinWriterComplete"], true);
        assert_eq!(error.details["outputReadersComplete"], true);
        assert!(error.details["jobEmpty"].is_null());
        assert_eq!(error.details["ownedJobTerminal"], false);
        assert_eq!(error.details["cleanupErrors"], serde_json::json!([]));
    }

    #[test]
    fn duplex_spawn_failure_drops_captured_pipes_and_retires_attached_owner() {
        let control = OperationControl::default();
        let error =
            GitBatchProcess::spawn_inner(blocked_helper(), &control, SpawnFault::DuplexSpawn)
                .err()
                .expect("injected duplex spawn failure");
        assert_eq!(error.code, "codebase.source_not_ready");
        assert_eq!(error.message, "fixture duplex worker could not start");
        assert_retired(&error);
        assert_eq!(error.details["cleanupErrors"], serde_json::json!([]));
    }

    #[test]
    fn cancellation_retires_an_idle_duplex_without_exporting_parent_token() {
        use lycheedev_core::control::StopCause;
        let parent = OperationControl::default();
        let child = parent.ordered_child(parent.deadline()).unwrap();
        let local = child.tracked_cancel_handle().unwrap();
        let mut process = GitBatchProcess::spawn_fixed(blocked_helper(), &child).unwrap();
        parent.tracked_cancel_handle().unwrap().cancel();
        let (hash, _) = object(b"data");
        let error = process.request(&hash, "blob", 1024, &child).unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        assert_retired(&error);
        assert!(!local.is_cancelled());
        assert_eq!(parent.stop_cause().unwrap(), Some(StopCause::Cancelled));
        assert_eq!(child.stop_cause().unwrap(), Some(StopCause::Cancelled));
        let final_error = process
            .finish::<()>(Err(error.clone()), &child)
            .unwrap_err();
        assert_eq!(
            final_error.details, error.details,
            "cleanup is not recursively wrapped"
        );
    }

    #[test]
    fn callback_error_and_its_details_survive_stop_and_late_cleanup_failure() {
        let control = OperationControl::default();
        let process = GitBatchProcess::spawn_fixed(blocked_helper(), &control).unwrap();
        control.tracked_cancel_handle().unwrap().cancel();
        let details = serde_json::json!({"path":"earlier.lua","boundary":7});
        let error = ToolkitError::new("fixture.callback", "earlier callback failure", 4)
            .with_details(details.clone());
        let error = process.finish::<()>(Err(error), &control).unwrap_err();
        assert_eq!(error.code, "fixture.callback");
        assert_eq!(error.details["primaryErrorDetails"], details);
        assert_retired(&error);
    }

    #[test]
    fn expired_control_prevents_spawn_and_new_control_cannot_renew_old_deadline() {
        let expired = OperationControl::uncancelable(Instant::now());
        let error =
            GitBatchProcess::spawn_fixed(Command::new("must-not-start-source-fixture"), &expired)
                .err()
                .expect("expired control prevents spawn");
        assert_eq!(error.code, "command.deadline");
        assert!(error.details.is_null());

        let original = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
        let mut process = GitBatchProcess::spawn_fixed(blocked_helper(), &original).unwrap();
        // Expire only the retained immutable control in this private fixture;
        // a later caller scope still cannot renew it or erase its stop cause.
        process.original_control = expired;
        let (hash, _) = object(b"data");
        let error = process
            .request(&hash, "blob", 1024, &OperationControl::default())
            .unwrap_err();
        assert_eq!(error.code, "command.deadline");
        assert_retired(&error);
    }
}
