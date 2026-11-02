//! Private, caller-owned DD native process. The task owner alone holds its Job,
//! exact Child and pipe. A lost/expired exchange retires this worker, never
//! starts a replacement or repeats a submitted author request.
use super::session::TaskSession;
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use lycheedev_windows::{
    JobGuard, Process, ProcessIdentity,
    broker::{BrokerCheck, BrokerListener, BrokerScope, BrokerStream},
    guarded_spawn,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const WIRE: &str = "lycheedev.publish.dd-native.v1";
const START_ENV: &str = "LYCHEEDEV_DD_NATIVE_WORKER";
const FRAME_LIMIT: usize = 64 << 20;
const IDLE: Duration = Duration::from_secs(600);
const REAP: Duration = Duration::from_secs(5);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    wire: String,
    nonce: String,
    process: ProcessIdentity,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    deadline_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Call {
    Status,
    Get {
        path: String,
        query: Vec<(String, String)>,
    },
    Post {
        path: String,
        query: Vec<(String, String)>,
        body: Value,
        mutation: bool,
    },
    Channels,
    Upload {
        request: super::upload::Request,
    },
    BackupUpload {
        request: super::backup::upload::UploadRequest,
    },
    Stop,
}
impl Call {
    pub(super) fn mutates(&self) -> bool {
        matches!(
            self,
            Self::Post { mutation: true, .. } | Self::Upload { .. } | Self::BackupUpload { .. }
        )
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    wire: String,
    nonce: String,
    id: u64,
    deadline_ms: u64,
    call: Call,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    wire: String,
    nonce: String,
    id: u64,
    result: Result<Value>,
}

fn fault(message: &str) -> ToolkitError {
    ToolkitError::new("publish.dd_native_worker", message, 3)
}
fn protocol() -> ToolkitError {
    ToolkitError::new(
        "publish.dd_native_protocol",
        "invalid private DD worker frame or correlation",
        4,
    )
}
fn platform(_: String) -> ToolkitError {
    fault("private DD process or pipe operation failed")
}
pub(super) fn check(control: &OperationControl) -> BrokerCheck {
    let control = control.clone();
    Arc::new(move || control.check().map_err(|e| e.to_string()))
}
pub(super) fn now_ms() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| fault("system clock is unavailable"))?
            .as_millis(),
    )
    .map_err(|_| fault("system clock is outside the supported range"))
}
pub(super) fn wire_deadline(control: &OperationControl) -> Result<u64> {
    Ok(now_ms()?
        .saturating_add(u64::try_from(control.remaining()?.as_millis()).map_err(|_| protocol())?))
}
pub(super) fn operation(deadline_ms: u64, maximum_ms: u64) -> Result<OperationControl> {
    let observed = Instant::now();
    let remaining = deadline_ms.saturating_sub(now_ms()?);
    if remaining == 0 {
        return Err(ToolkitError::new(
            "command.deadline",
            "DD request reached its original deadline before admission",
            5,
        ));
    }
    if remaining > maximum_ms {
        return Err(protocol());
    }
    Ok(OperationControl::uncancelable(
        observed + Duration::from_millis(remaining),
    ))
}
pub(super) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.is_empty() || bytes.len() > FRAME_LIMIT {
        return Err(protocol());
    }
    let mut frame = Vec::with_capacity(bytes.len() + 4);
    frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    frame.extend(bytes);
    Ok(frame)
}
pub(super) fn read_frame<T: DeserializeOwned>(stream: &mut BrokerStream) -> Result<T> {
    read_frame_bounded(stream, FRAME_LIMIT)
}
pub(super) fn read_frame_bounded<T: DeserializeOwned>(
    stream: &mut BrokerStream,
    limit: usize,
) -> Result<T> {
    let mut header = [0; 4];
    stream
        .read_exact(&mut header)
        .map_err(|_| fault("private DD worker pipe ended"))?;
    let size = u32::from_le_bytes(header) as usize;
    if size == 0 || size > limit {
        return Err(protocol());
    }
    let mut bytes = vec![0; size];
    stream
        .read_exact(&mut bytes)
        .map_err(|_| fault("private DD worker frame was interrupted"))?;
    let value = crate::input::value(&bytes).map_err(|_| protocol())?;
    serde_json::from_value(value).map_err(|_| protocol())
}
pub(super) fn send<T: Serialize>(stream: &mut BrokerStream, value: &T) -> Result<()> {
    stream
        .write_all(&encode(value)?)
        .map_err(|_| fault("private DD worker frame could not be sent"))
}

pub struct Worker {
    child: Option<Child>,
    job: Option<JobGuard>,
    stream: Option<BrokerStream>,
    process: Option<ProcessIdentity>,
    nonce: String,
    next: u64,
    identity: Value,
}
impl Worker {
    pub fn spawn(home: &Path, executable: &Path, control: &OperationControl) -> Result<Self> {
        Self::spawn_recorded(home, executable, control, |_| Ok(()))
    }
    pub(super) fn spawn_recorded(
        home: &Path,
        executable: &Path,
        control: &OperationControl,
        record: impl FnMut(&ProcessIdentity) -> Result<()>,
    ) -> Result<Self> {
        Self::spawn_observed(
            home,
            control,
            |nonce, parent| {
                let mut command = Command::new(executable);
                command.args([
                    "--publish-dd-native-worker",
                    &home.to_string_lossy(),
                    &parent.pid.to_string(),
                    &parent.created.to_string(),
                    nonce,
                ]);
                command.env(START_ENV, nonce);
                command
            },
            record,
        )
    }
    #[cfg(test)]
    fn spawn_with(
        home: &Path,
        control: &OperationControl,
        build: impl FnOnce(&str, &ProcessIdentity) -> Command,
    ) -> Result<Self> {
        Self::spawn_observed(home, control, build, |_| Ok(()))
    }
    fn spawn_observed(
        home: &Path,
        control: &OperationControl,
        build: impl FnOnce(&str, &ProcessIdentity) -> Command,
        mut record: impl FnMut(&ProcessIdentity) -> Result<()>,
    ) -> Result<Self> {
        let startup = control.ordered_child(Instant::now() + Duration::from_secs(90))?;
        let control = &startup;
        control.check()?;
        let parent = Process::open(std::process::id()).map_err(platform)?;
        let identity = parent.identity().clone();
        parent.close().map_err(platform)?;
        let nonce = lycheedev_windows::random::token().map_err(platform)?;
        let scope = BrokerScope::for_dd_worker(home, &nonce).map_err(platform)?;
        let mut command = build(&nonce, &identity);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .current_dir(home);
        let mut worker = Self {
            child: None,
            job: None,
            stream: None,
            process: None,
            nonce,
            next: 1,
            identity: Value::Null,
        };
        match guarded_spawn::spawn_suspended_job_guarded_until(
            command,
            control.deadline(),
            control.deadline() + REAP,
            || control.check().is_err(),
        ) {
            Ok((child, job)) => {
                worker.child = Some(child);
                worker.job = Some(job);
                match guarded_spawn::child_identity(worker.child.as_ref().unwrap()) {
                    Ok(identity) => worker.process = Some(identity),
                    Err(message) => {
                        let cleanup = worker.retire(Instant::now() + REAP, true);
                        return Err(cleanup_error(platform(message), cleanup));
                    }
                }
            }
            Err(mut failure) => {
                let primary = control
                    .check()
                    .err()
                    .unwrap_or_else(|| fault("DD native worker could not be started"));
                let details = serde_json::to_value(&failure)?;
                if let Some((child, job)) = failure.take_pending() {
                    worker.process = guarded_spawn::child_identity(&child).ok();
                    worker.child = Some(child);
                    worker.job = job;
                }
                let cleanup = worker.retire(Instant::now() + REAP, true);
                return Err(cleanup_error(
                    primary.with_details(json!({"startup":details})),
                    cleanup,
                ));
            }
        }
        let started = (|| {
            record(worker.process.as_ref().unwrap())?;
            loop {
                control.check()?;
                if worker.child.as_mut().unwrap().try_wait()?.is_some() {
                    return Err(fault("DD native worker exited before its ready response"));
                }
                match BrokerStream::connect_with_control(&scope, control.deadline(), check(control))
                {
                    Ok(stream) => {
                        worker.stream = Some(stream);
                        break;
                    }
                    Err(message)
                        if message.starts_with("broker.not_found:")
                            || message.starts_with("broker.busy:") =>
                    {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(message) => return Err(platform(message)),
                }
            }
            let stream = worker.stream.as_mut().unwrap();
            if Some(stream.peer_pid().map_err(platform)?) != worker.process.as_ref().map(|p| p.pid)
            {
                return Err(protocol());
            }
            let hello: Hello = read_frame(stream)?;
            if hello.wire != WIRE
                || hello.nonce != worker.nonce
                || Some(&hello.process) != worker.process.as_ref()
            {
                return Err(protocol());
            }
            send(
                stream,
                &Open {
                    deadline_ms: wire_deadline(control)?,
                },
            )?;
            let reply: Response = read_frame(stream)?;
            if reply.wire != WIRE || reply.nonce != worker.nonce || reply.id != 0 {
                return Err(protocol());
            }
            worker.identity = reply.result?;
            control.check()
        })();
        if let Err(error) = started {
            let cleanup = worker.retire(Instant::now() + REAP, true);
            return Err(cleanup_error(
                control.check().err().unwrap_or(error),
                cleanup,
            ));
        }
        Ok(worker)
    }
    pub fn identity(&self) -> &Value {
        &self.identity
    }
    pub fn process(&self) -> Option<&ProcessIdentity> {
        self.process.as_ref()
    }
    pub(super) fn call(&mut self, call: Call, control: &OperationControl) -> Result<Value> {
        let mutation = call.mutates();
        control
            .check()
            .map_err(|e| super::submission(e, mutation, "not_sent"))?;
        if self.stream.is_none() {
            return Err(super::submission(
                fault("DD native worker is retired; inspect the original task session"),
                mutation,
                "not_sent",
            ));
        }
        let id = self.next;
        self.next = self.next.checked_add(1).ok_or_else(protocol)?;
        let request = Request {
            wire: WIRE.into(),
            nonce: self.nonce.clone(),
            id,
            deadline_ms: wire_deadline(control)?,
            call,
        };
        let bytes = encode(&request).map_err(|e| super::submission(e, mutation, "not_sent"))?;
        let exchanged = (|| {
            let stream = self.stream.as_mut().unwrap();
            stream.set_deadline_with_control(control.deadline(), check(control));
            control.check()?;
            stream
                .write_all(&bytes)
                .map_err(|_| fault("DD native request transport ended"))?;
            let reply: Response = read_frame(stream)?;
            if reply.wire != WIRE || reply.nonce != self.nonce || reply.id != id {
                return Err(protocol());
            }
            control.check()?;
            Ok(reply.result)
        })();
        match exchanged {
            Ok(result) => result,
            Err(error) => {
                let cleanup = self.retire(Instant::now() + REAP, true);
                Err(super::submission(
                    cleanup_error(control.check().err().unwrap_or(error), cleanup),
                    mutation,
                    "uncertain",
                ))
            }
        }
    }
    pub fn status(&mut self, control: &OperationControl) -> Result<Value> {
        self.call(Call::Status, control)
    }
    pub(super) fn is_retired(&self) -> bool {
        self.stream.is_none()
    }
    pub fn get(
        &mut self,
        path: &str,
        query: &[(&str, String)],
        control: &OperationControl,
    ) -> Result<Value> {
        self.call(
            Call::Get {
                path: path.into(),
                query: query
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            },
            control,
        )
    }
    pub fn post(
        &mut self,
        path: &str,
        query: &[(&str, String)],
        body: &Value,
        mutation: bool,
        control: &OperationControl,
    ) -> Result<Value> {
        self.call(
            Call::Post {
                path: path.into(),
                query: query
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
                body: body.clone(),
                mutation,
            },
            control,
        )
    }
    pub fn close(mut self, control: &OperationControl) -> Result<Value> {
        let result = if self.stream.is_some() {
            self.call(Call::Stop, control)
        } else {
            Ok(json!({"already_retired":true}))
        };
        let cleanup = self.retire(control.deadline(), result.is_err());
        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), cleanup) => Err(cleanup_error(error, cleanup)),
            (Ok(_), Err(error)) => Err(error),
        }
    }
    fn retire(&mut self, deadline: Instant, force: bool) -> Result<()> {
        let mut errors = Vec::new();
        if let Some(stream) = self.stream.take() {
            if stream.close().is_err() {
                errors.push("pipe close");
            }
        }
        if force {
            if let Some(job) = &self.job {
                if job.terminate().is_err() {
                    errors.push("Job termination");
                }
            }
        }
        let mut exited = self.child.is_none();
        while let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    exited = true;
                    break;
                }
                Ok(None) => {}
                Err(_) => {
                    errors.push("process wait");
                    break;
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !exited {
            return Err(fault("DD native worker retirement is pending")
                .with_details(json!({"worker":self.process,"cleanup_pending":true})));
        }
        let mut empty = self.job.is_none();
        while let Some(job) = &self.job {
            match job.memory_accounting() {
                Ok(info) if info.active_processes == 0 => {
                    empty = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => {
                    errors.push("Job query");
                    break;
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !empty {
            return Err(fault("DD native descendants are still being retired")
                .with_details(json!({"worker":self.process,"cleanup_pending":true})));
        }
        if let Some(child) = self.child.take() {
            if guarded_spawn::close_child(child).is_err() {
                errors.push("process close");
            }
        }
        if let Some(job) = self.job.take() {
            if job.close().is_err() {
                errors.push("Job close");
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(fault("DD native worker cleanup reported an error")
                .with_details(json!({"errors":errors,"worker":self.process})))
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            let _ = job.terminate();
        }
        if let Some(child) = &mut self.child {
            let _ = child.kill();
        }
    }
}
fn cleanup_error(error: ToolkitError, cleanup: Result<()>) -> ToolkitError {
    match cleanup {
        Ok(()) => error,
        Err(cleanup) => {
            ToolkitError::new(error.code.clone(), error.message.clone(), error.exit_code)
                .with_details(json!({"primary":error,"cleanup":cleanup}))
        }
    }
}

struct Binding {
    home: PathBuf,
    nonce: String,
    parent: ProcessIdentity,
}
impl Binding {
    fn parse(args: &[String]) -> Result<Self> {
        if args.len() != 5 || args[0] != "--publish-dd-native-worker" {
            return Err(protocol());
        }
        let nonce = args[4].clone();
        if std::env::var(START_ENV).ok().as_deref() != Some(nonce.as_str()) {
            return Err(protocol());
        }
        let home = PathBuf::from(&args[1]);
        if !home.is_absolute() {
            return Err(protocol());
        }
        let parent = ProcessIdentity {
            pid: args[2].parse().map_err(|_| protocol())?,
            created: args[3].parse().map_err(|_| protocol())?,
        };
        if parent.pid == 0 || parent.created == 0 {
            return Err(protocol());
        }
        Ok(Self {
            home,
            nonce,
            parent,
        })
    }
}

trait Session {
    fn identity(&self) -> Value;
    fn execute(&mut self, home: &Path, call: &Call, control: &OperationControl) -> Result<Value>;
    fn close(self) -> Result<()>;
}
impl Session for TaskSession {
    fn identity(&self) -> Value {
        self.identity()
    }
    fn execute(&mut self, home: &Path, call: &Call, control: &OperationControl) -> Result<Value> {
        if let Call::Status = call {
            return Ok(self.identity());
        }
        self.begin_operation(control)
            .map_err(|error| super::submission(error, call.mutates(), "not_sent"))?;
        match call {
            Call::Get { path, query } => self.get(
                path,
                &query
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.clone()))
                    .collect::<Vec<_>>(),
            ),
            Call::Post {
                path,
                query,
                body,
                mutation,
            } => self.post(
                path,
                &query
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.clone()))
                    .collect::<Vec<_>>(),
                body,
                *mutation,
            ),
            Call::Channels => self.channels(control),
            Call::Upload { request } => self.upload(home, request, control),
            Call::BackupUpload { request } => self.backup_upload(home, request, control),
            _ => Err(protocol()),
        }
    }
    fn close(self) -> Result<()> {
        self.close()
    }
}

pub fn entry(args: &[String]) -> Result<()> {
    let binding = Binding::parse(args)?;
    serve(binding, TaskSession::open)
}
fn serve<S: Session>(
    binding: Binding,
    factory: impl FnOnce(&Path, &OperationControl) -> Result<S>,
) -> Result<()> {
    let scope = BrokerScope::for_dd_worker(&binding.home, &binding.nonce).map_err(platform)?;
    let mut listener = BrokerListener::bind(&scope).map_err(platform)?;
    let startup = OperationControl::uncancelable(Instant::now() + Duration::from_secs(90));
    let connected = listener
        .accept_with_control(startup.deadline(), check(&startup))
        .map_err(platform);
    let mut stream = match connected {
        Ok(stream) => stream,
        Err(error) => return Err(cleanup_error(error, listener.close().map_err(platform))),
    };
    let run = (|| {
        if stream.peer_pid().map_err(platform)? != binding.parent.pid {
            return Err(protocol());
        }
        let parent = Process::open(binding.parent.pid).map_err(platform)?;
        let verified = (|| {
            if parent.identity() != &binding.parent
                || parent.image_path().map_err(platform)?.canonicalize()?
                    != std::env::current_exe()?.canonicalize()?
            {
                return Err(protocol());
            }
            Ok(())
        })();
        let closed = parent.close().map_err(platform);
        verified?;
        closed?;
        let own = Process::open(std::process::id()).map_err(platform)?;
        let identity = own.identity().clone();
        own.close().map_err(platform)?;
        send(
            &mut stream,
            &Hello {
                wire: WIRE.into(),
                nonce: binding.nonce.clone(),
                process: identity,
            },
        )?;
        let open: Open = read_frame(&mut stream)?;
        let control = operation(open.deadline_ms, 90_000)?;
        stream.set_deadline_with_control(control.deadline(), check(&control));
        let mut session = match factory(&binding.home, &control) {
            Ok(session) => session,
            Err(error) => {
                send(
                    &mut stream,
                    &Response {
                        wire: WIRE.into(),
                        nonce: binding.nonce.clone(),
                        id: 0,
                        result: Err(error.clone()),
                    },
                )?;
                return Err(error);
            }
        };
        let opened = control.check().and_then(|()| {
            send(
                &mut stream,
                &Response {
                    wire: WIRE.into(),
                    nonce: binding.nonce.clone(),
                    id: 0,
                    result: Ok(session.identity()),
                },
            )
        });
        let outcome = (|| {
            opened?;
            let mut last = 0;
            loop {
                let idle = OperationControl::uncancelable(Instant::now() + IDLE);
                stream.set_deadline_with_control(idle.deadline(), check(&idle));
                let request: Request = match read_frame(&mut stream) {
                    Ok(value) => value,
                    Err(_) if Instant::now() >= idle.deadline() => break Ok(None),
                    Err(error) => break Err(error),
                };
                if request.wire != WIRE || request.nonce != binding.nonce || request.id <= last {
                    return Err(protocol());
                }
                last = request.id;
                let control = operation(request.deadline_ms, 900_000)?;
                stream.set_deadline_with_control(control.deadline(), check(&control));
                if matches!(request.call, Call::Stop) {
                    break Ok(Some(request.id));
                }
                let value = session.execute(&binding.home, &request.call, &control);
                send(
                    &mut stream,
                    &Response {
                        wire: WIRE.into(),
                        nonce: binding.nonce.clone(),
                        id: request.id,
                        result: value,
                    },
                )?;
            }
        })();
        let closed = session.close();
        match outcome {
            Ok(Some(id)) => {
                send(
                    &mut stream,
                    &Response {
                        wire: WIRE.into(),
                        nonce: binding.nonce.clone(),
                        id,
                        result: closed.map(|()| json!({"cleanup_complete":true})),
                    },
                )?;
                Ok(())
            }
            Ok(None) => closed,
            Err(error) => Err(cleanup_error(error, closed)),
        }
    })();
    let run = match run {
        Ok(()) => stream.close().map_err(platform),
        Err(error) => Err(cleanup_error(error, stream.close().map_err(platform))),
    };
    match run {
        Ok(()) => listener.close().map_err(platform),
        Err(error) => Err(cleanup_error(error, listener.close().map_err(platform))),
    }
}

#[cfg(test)]
mod tests;
