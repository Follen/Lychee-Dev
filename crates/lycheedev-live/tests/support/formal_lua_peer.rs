//! Fixture transport: unchanged Lua computes every receipt and business result.
//! Only readiness/monotonic time and host-input failure injection are simulated.
#![forbid(unsafe_code)]
use super::formal_lua_fixture::{self as fixture, BROKEN_CODE, CODE, Inputs};
use lycheedev_core::{control::OperationControl, storage};
use lycheedev_live::{
    go_mailbox_v1::{
        Identity, InputBindings, InputDisposition, InputObservation, InputOutcome, MAX_PAYLOAD,
        SCHEMA, Selector, checksum,
        durable_journal::{self, ChannelPaths},
        observations::{
            Backend, ChannelError, ChannelResult, InputAction, InputDispatch, ObservationDispatch,
            ObservationQuery, RecordReader, check_control, observe_records,
        },
        slot_allocation::slot_payload,
        state::{InputKind, Operation, OperationStage, SlotEnvelope},
    },
    mailbox::{Record, decode},
};
use lycheedev_windows::{JobGuard, guarded_spawn::spawn_suspended_guarded};
use serde_json::json;
use std::{
    cell::RefCell,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    rc::Rc,
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

fn external(message: impl ToString) -> ChannelError {
    ChannelError::External {
        causes: vec![message.to_string()],
    }
}
type Worker = JoinHandle<Result<Vec<u8>, String>>;
struct OwnedPeer {
    child: Child,
    job: JobGuard,
    input: Option<SyncSender<Vec<u8>>>,
    writes: Receiver<Result<(), String>>,
    frames: Receiver<Result<Vec<u8>, String>>,
    workers: [Option<Worker>; 3],
    control: OperationControl,
    evidence: PathBuf,
    closed: bool,
    retired: bool,
}
impl OwnedPeer {
    fn start(
        inputs: &Inputs,
        directory: &Path,
        fault: &str,
        control: &OperationControl,
    ) -> Result<Self, String> {
        inputs.verify();
        let mut command = Command::new(&inputs.lua);
        command
            .arg(inputs.fixture.join("tests/protocol/slot_server.lua"))
            .arg(inputs.fixture.join("addon"))
            .arg(directory)
            .arg(if fault == "reload_bind" { "2" } else { "1" })
            .arg(fault)
            .current_dir(inputs.fixture.join("tests/protocol"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Lua startup configuration is never inherited from an arbitrary caller.
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy().to_ascii_uppercase();
            if name == "LUA" || name.starts_with("LUA_") {
                command.env_remove(key);
            }
        }
        command.env("LUA_PATH", "").env("LUA_CPATH", "");
        let (child, job) = spawn_suspended_guarded(command, 128 << 20, control.deadline(), || {
            control.check().is_err()
        })
        .map_err(|failure| serde_json::to_string(&failure).unwrap())?;
        let (input, requests) = mpsc::sync_channel::<Vec<u8>>(1);
        let (write_done, writes) = mpsc::channel();
        let (frame, frames) = mpsc::channel();
        let mut owner = Self {
            child,
            job,
            input: Some(input),
            writes,
            frames,
            workers: [None, None, None],
            control: control.clone(),
            evidence: directory.parent().unwrap().join("peer-lifetime.json"),
            closed: false,
            retired: false,
        };
        let start = (|| -> Result<(), String> {
            let mut stdin = owner.child.stdin.take().ok_or("stdin missing")?;
            owner.workers[0] = Some(
                thread::Builder::new()
                    .name("formal-lua-stdin".into())
                    .spawn(move || {
                        let mut count = 0;
                        while let Ok(line) = requests.recv() {
                            count += 1;
                            if count > 64 || line.len() > 4096 {
                                return Err("stdin frame budget".into());
                            }
                            let result = stdin
                                .write_all(&line)
                                .and_then(|()| stdin.flush())
                                .map_err(|e| e.to_string());
                            let failed = result.is_err();
                            let _ = write_done.send(result);
                            if failed {
                                return Err("stdin writer failed".into());
                            }
                        }
                        drop(stdin);
                        Ok(Vec::new())
                    })
                    .map_err(|e| e.to_string())?,
            );
            let stdout = owner.child.stdout.take().ok_or("stdout missing")?;
            owner.workers[1] = Some(
                thread::Builder::new()
                    .name("formal-lua-stdout".into())
                    .spawn(move || {
                        let mut reader = BufReader::new(stdout);
                        let mut log = Vec::new();
                        for _ in 0..64 {
                            let mut line = Vec::new();
                            let count = Read::by_ref(&mut reader)
                                .take(257)
                                .read_until(b'\n', &mut line)
                                .map_err(|e| e.to_string())?;
                            if count == 0 {
                                return Ok(log);
                            }
                            if line.len() > 256 || line.last() != Some(&b'\n') {
                                let _ =
                                    frame.send(Err("stdout frame limit/incomplete line".into()));
                                return Err("stdout frame limit/incomplete line".into());
                            }
                            log.extend(&line);
                            if frame.send(Ok(line)).is_err() {
                                return Err("stdout consumer disappeared".into());
                            }
                        }
                        let _ = frame.send(Err("stdout frame count".into()));
                        Err("stdout frame count".into())
                    })
                    .map_err(|e| e.to_string())?,
            );
            let mut stderr = owner.child.stderr.take().ok_or("stderr missing")?;
            owner.workers[2] = Some(
                thread::Builder::new()
                    .name("formal-lua-stderr".into())
                    .spawn(move || {
                        let mut bytes = Vec::new();
                        stderr
                            .by_ref()
                            .take(65537)
                            .read_to_end(&mut bytes)
                            .map_err(|e| e.to_string())?;
                        if bytes.len() > 65536 {
                            Err("stderr byte budget".into())
                        } else {
                            Ok(bytes)
                        }
                    })
                    .map_err(|e| e.to_string())?,
            );
            owner.expect(b"ready\n")
        })();
        if let Err(error) = start {
            let cleanup = owner.close(false);
            return Err(format!("{error}; cleanup={cleanup:?}"));
        }
        Ok(owner)
    }
    fn receive<T>(&self, channel: &Receiver<T>) -> Result<T, String> {
        loop {
            self.control.check().map_err(|e| e.to_string())?;
            let remaining = self
                .control
                .deadline()
                .saturating_duration_since(Instant::now());
            match channel.recv_timeout(remaining.min(Duration::from_millis(10))) {
                Ok(value) => return Ok(value),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("peer pipe worker stopped".into());
                }
            }
        }
    }
    fn expect(&self, expected: &[u8]) -> Result<(), String> {
        let line = self.receive(&self.frames)??;
        let normalized = if line.ends_with(b"\r\n") {
            [&line[..line.len() - 2], b"\n"].concat()
        } else {
            line
        };
        if normalized == expected {
            Ok(())
        } else {
            Err("unexpected Lua peer frame".into())
        }
    }
    fn write(&self, line: Vec<u8>) -> Result<(), String> {
        self.control.check().map_err(|e| e.to_string())?;
        if line.len() > 4096 || !line.ends_with(b"\n") {
            return Err("invalid input frame".into());
        }
        self.input
            .as_ref()
            .ok_or("peer input closed")?
            .try_send(line)
            .map_err(|e| e.to_string())?;
        self.receive(&self.writes)?
    }
    fn exchange(&self, line: Vec<u8>) -> Result<(), String> {
        self.write(line)?;
        self.expect(b"done\n")
    }
    /// Fixed cleanup grace, independent of the exhausted execution deadline.
    /// Close the input queue before terminating the exact Job. Only join workers
    /// after is_finished; a blocked pipe can never cause an unbounded join.
    fn close(&mut self, graceful: bool) -> Result<(), String> {
        if self.closed {
            return if self.retired {
                Ok(())
            } else {
                Err("prior retirement remains unverified".into())
            };
        }
        // One retirement attempt only. Failure cannot renew cleanup via Drop.
        self.closed = true;
        let mut errors = Vec::new();
        let mut forced = !graceful;
        if graceful && let Err(error) = self.write(b"quit\n".to_vec()) {
            errors.push(error);
            forced = true;
        }
        self.input.take();
        self.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        if forced && let Err(error) = self.job.terminate() {
            errors.push(error);
        }
        let mut status = None;
        let mut job_empty = false;
        let mut outputs: [Option<Vec<u8>>; 3] = [None, None, None];
        loop {
            if status.is_none() {
                match self.child.try_wait() {
                    Ok(value) => status = value,
                    Err(error) => {
                        errors.push(error.to_string());
                        forced = true;
                        let _ = self.job.terminate();
                    }
                }
            }
            for (index, slot) in self.workers.iter_mut().enumerate() {
                if slot.as_ref().is_some_and(JoinHandle::is_finished) {
                    match slot.take().unwrap().join() {
                        Ok(Ok(bytes)) => outputs[index] = Some(bytes),
                        Ok(Err(error)) => errors.push(error),
                        Err(_) => errors.push("pipe worker panic".into()),
                    }
                }
            }
            match self.job.memory_accounting() {
                Ok(value) => job_empty = value.active_processes == 0,
                Err(error) => {
                    if !errors.contains(&error) {
                        errors.push(error);
                    }
                    if !forced {
                        forced = true;
                        if let Err(error) = self.job.terminate() {
                            errors.push(error);
                        }
                    }
                }
            }
            if status.is_some() && job_empty && self.workers.iter().all(Option::is_none) {
                break;
            }
            // Graceful quit gets one second, never an extension of execution.
            if !forced && Instant::now() + Duration::from_secs(4) >= deadline {
                forced = true;
                if let Err(error) = self.job.terminate() {
                    errors.push(error);
                }
            }
            if Instant::now() >= deadline {
                errors.push("peer cleanup deadline/unjoined workers".into());
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        if status.is_none() || !job_empty || self.workers.iter().any(Option::is_some) {
            errors.push("peer ownership completion unverified; retain scratch".into());
        }
        if graceful && (forced || status.as_ref().is_none_or(|s| !s.success())) {
            errors.push("peer did not exit normally".into());
        }
        let proof = json!({"schema":"lycheedev.formal-lua-peer-lifetime.v1", "rootReaped":status.is_some(),
            "rootExit":status.and_then(|s|s.code()), "jobEmpty":job_empty, "workersJoined":self.workers.iter().all(Option::is_none),
            "forced":forced, "errors":errors, "innerHandleCloseStatus":"std Child/pipe and JobGuard Drop only; checked close unavailable",
            "scope":"exact suspended-spawn Child and Job; no game, external PID, or process-name termination"});
        // Lifetime and bounded raw pipe evidence remain in the owning scratch.
        fs::write(&self.evidence, serde_json::to_vec_pretty(&proof).unwrap())
            .map_err(|e| e.to_string())?;
        for (index, bytes) in outputs.into_iter().enumerate() {
            if let Some(bytes) = bytes {
                fs::write(
                    self.evidence.with_extension(format!("pipe-{index}.bin")),
                    bytes,
                )
                .map_err(|e| e.to_string())?;
            }
        }
        self.closed = true;
        if errors.is_empty() {
            self.retired = true;
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}
impl Drop for OwnedPeer {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.close(false);
        }
    }
}

/// Retains the exact existing owner independently of Driver construction/moves.
/// It adds no spawn, termination, pipe or ownership implementation of its own.
pub struct PeerOwner(Rc<RefCell<OwnedPeer>>);
impl PeerOwner {
    pub fn close(&mut self, graceful: bool) -> Result<(), String> {
        self.0.borrow_mut().close(graceful)
    }
}

pub struct Peer {
    process: Rc<RefCell<OwnedPeer>>,
    pub directory: PathBuf,
    paths: ChannelPaths,
    current: Option<SlotEnvelope>,
    fault: String,
    pub sends: Vec<SlotEnvelope>,
    pub input_samples: Vec<InputObservation>,
    millis: i64,
    pub release_checks: usize,
}
impl Peer {
    pub fn start(
        inputs: &Inputs,
        directory: &Path,
        paths: ChannelPaths,
        fault: &str,
        control: &OperationControl,
    ) -> Self {
        Self {
            process: Rc::new(RefCell::new(
                OwnedPeer::start(inputs, directory, fault, control).expect("owned Lua start"),
            )),
            directory: directory.into(),
            paths,
            current: None,
            fault: fault.into(),
            sends: Vec::new(),
            input_samples: Vec::new(),
            millis: 0,
            release_checks: 0,
        }
    }
    pub fn owner(&self) -> PeerOwner {
        PeerOwner(Rc::clone(&self.process))
    }
    fn send(&mut self, control: &OperationControl, action: &InputAction) -> ChannelResult<()> {
        check_control(control)?;
        let envelope = self
            .current
            .as_ref()
            .ok_or_else(|| external("no published envelope"))?;
        if action.kind != InputKind::Invoke || envelope != &action.envelope {
            return Err(external("unexpected fixture input"));
        }
        // Before every send, independently reload the real durable input intent.
        let state = durable_journal::load(&self.paths, control)?;
        let transaction = state
            .transaction
            .as_ref()
            .ok_or_else(|| external("missing durable transaction"))?;
        if &transaction.envelope != envelope
            || state
                .input
                .as_ref()
                .is_none_or(|input| input.exchange != envelope.nonce || input.outcome.is_some())
        {
            return Err(external("input before exact durable intent"));
        }
        if envelope.action == "release" {
            let operation = state
                .operation
                .as_ref()
                .ok_or_else(|| external("missing operation"))?;
            let bytes = fixture::read(
                &self
                    .paths
                    .result_directory()
                    .join(format!("{}.json", operation.id)),
                2 << 20,
            );
            let durable: Operation = serde_json::from_slice(&bytes).map_err(external)?;
            if operation.stage != OperationStage::ReleaseReady
                || durable.stage != OperationStage::ResultVerified
                || durable.id != operation.id
                || durable.request != operation.request
                || durable.ticket != operation.ticket
                || durable.result_bytes != operation.result_bytes
                || durable.report_checksum != checksum(&operation.result_bytes.0)
            {
                return Err(external("release before durable exact result"));
            }
            self.release_checks += 1;
        }
        let payload = self.directory.join("payload.lua");
        if fixture::read(&payload, 2 << 20) != slot_payload(envelope)? {
            return Err(external("payload changed before input"));
        }
        let path = payload
            .to_str()
            .ok_or_else(|| external("payload path encoding"))?
            .replace('\\', "/");
        if path.contains(['\r', '\n', '|']) {
            return Err(external("payload path framing"));
        }
        self.sends.push(envelope.clone());
        self.process
            .borrow()
            .exchange(format!("{}|{path}\n", envelope.index).into_bytes())
            .map_err(external)?;
        if self.fault == envelope.action {
            self.fault.clear();
            return Err(external("injected host death after input"));
        }
        Ok(())
    }
}
impl RecordReader for Peer {
    fn find(
        &mut self,
        control: &OperationControl,
        selector: &Selector,
        mut accept: Option<&mut dyn FnMut(&Record) -> bool>,
        eager: bool,
    ) -> ChannelResult<Vec<Record>> {
        check_control(control)?;
        let mut paths = Vec::new();
        for (index, entry) in fs::read_dir(&self.directory).map_err(external)?.enumerate() {
            if index >= 128 {
                return Err(external("peer directory entry budget"));
            }
            let entry = entry.map_err(external)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("record-") && name.ends_with(".bin") {
                paths.push(entry.path());
            } else if name != "payload.lua" && name != "executions.txt" {
                return Err(external("unexpected peer file"));
            }
        }
        if paths.len() > 64 {
            return Err(external("record count budget"));
        }
        paths.sort();
        let mut records = Vec::new();
        let mut total = 0usize;
        for path in paths {
            check_control(control)?;
            let bytes = fixture::read(&path, MAX_PAYLOAD + 120);
            total += bytes.len();
            if total > 16 << 20 {
                return Err(external("record read budget"));
            }
            // The actual production codec/correlator qualifies retained stale
            // records. No test code constructs a receipt or result payload.
            let Ok(record) = decode(&bytes) else {
                continue;
            };
            if !record.contract().is_ok_and(|r| selector.matches(&r.header)) {
                continue;
            }
            if accept.as_mut().is_some_and(|predicate| !predicate(&record)) {
                continue;
            }
            records.push(record);
            if eager {
                break;
            }
        }
        check_control(control)?;
        Ok(records)
    }
}
impl Backend for Peer {
    fn observe(
        &mut self,
        control: &OperationControl,
        query: &ObservationQuery,
    ) -> ObservationDispatch {
        observe_records(control, self, query).into()
    }
    fn publish(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<()> {
        check_control(control)?;
        if envelope.action == "prepare"
            && !matches!(envelope.code.as_bytes(), bytes if bytes == CODE.as_bytes() || bytes == BROKEN_CODE.as_bytes())
        {
            return Err(external("fixture source not in fixed Go test"));
        }
        if envelope.action != "prepare" && !envelope.code.is_empty() {
            return Err(external("unexpected executable source"));
        }
        let bytes = slot_payload(envelope)?;
        storage::atomic_write_controlled(&self.directory.join("payload.lua"), &bytes, control)
            .map_err(ChannelError::from)?;
        self.current = Some(envelope.clone());
        Ok(())
    }
    fn observe_input(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        after: i64,
        _: &str,
    ) -> ChannelResult<InputObservation> {
        check_control(control)?;
        self.millis = (self.millis + 100).max(after + 100);
        let sample = InputObservation {
            schema: SCHEMA.into(),
            bindings: Some(InputBindings {
                wake: "ALT-CTRL-F12".into(),
                reload: "ALT-CTRL-F11".into(),
                ..InputBindings::default()
            }),
            runtime: envelope.runtime.clone(),
            owner: envelope.owner.clone(),
            fence: envelope.fence,
            next_slot: envelope.index,
            guid: envelope.guid.clone(),
            build: envelope.build.clone(),
            sample_millis: self.millis,
            observed_at_millis: self.millis,
            input_blocked: Some(false),
            ..InputObservation::default()
        };
        self.input_samples.push(sample.clone());
        Ok(sample)
    }
    fn input(&mut self, control: &OperationControl, action: &InputAction) -> InputDispatch {
        let error = self.send(control, action).err();
        self.millis += 1;
        InputDispatch {
            outcome: InputOutcome {
                disposition: if error.is_some() {
                    InputDisposition::Uncertain
                } else {
                    InputDisposition::Submitted
                },
                messages_queued: 1,
                at_millis: self.millis,
                reason: String::new(),
                retryable: false,
            },
            error,
        }
    }
    fn runtime_candidate(
        &mut self,
        control: &OperationControl,
        _: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        check_control(control)?;
        Ok(None)
    }
    fn supersede(
        &mut self,
        _: &OperationControl,
        _: &SlotEnvelope,
        _: &Identity,
    ) -> ChannelResult<()> {
        Err(external("unexpected simulated retirement"))
    }
    fn consumed(&mut self, control: &OperationControl, _: &SlotEnvelope) -> ChannelResult<()> {
        check_control(control)
    }
}
