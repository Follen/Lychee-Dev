use super::*;
use lycheedev_windows::{JobGuard, guarded_spawn};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread::{JoinHandle, Thread},
    time::{Duration, Instant},
};
const FRAME_LIMIT: usize = 1 << 20;
const POLL: Duration = Duration::from_millis(20);
const RETIRE: Duration = Duration::from_secs(5);
struct Input {
    bytes: Vec<u8>,
    id: u64,
    deadline: Instant,
    kind: Kind,
}
#[derive(Clone, Copy)]
enum Kind {
    Work,
    Cancel(u64),
    Close,
}
enum Event {
    Input(Input),
    Eof,
    Error(ToolkitError),
}
struct Owned {
    child: Option<Child>,
    job: Option<JobGuard>,
    writer: Option<JoinHandle<Result<()>>>,
    receipts: Option<JoinHandle<()>>,
    receipt_socket: Option<TcpStream>,
}
impl Drop for Owned {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            let _ = job.terminate();
        }
        if let Some(child) = &mut self.child {
            let _ = child.kill();
        }
    }
}
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
fn bounded_line(reader: &mut impl BufRead, maximum: usize) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(fault("Incomplete worker protocol frame"))
            };
        }
        let end = buffer.iter().position(|b| *b == b'\n');
        let count = end.map_or(buffer.len(), |n| n + 1);
        if line.len().checked_add(count).is_none_or(|n| n > maximum) {
            return Err(fault("Worker protocol frame exceeds its bound"));
        }
        line.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            return Ok(Some(line));
        }
    }
}
fn ingress(
    sender: mpsc::SyncSender<Event>,
    fatal: Arc<Mutex<Option<ToolkitError>>>,
    supervisor: &Thread,
) {
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let mut last = 0;
    loop {
        let event = match bounded_line(&mut reader, FRAME_LIMIT) {
            Ok(Some(bytes)) => {
                let request: lycheedev_data::agent_session::Request =
                    match serde_json::from_slice(&bytes) {
                        Ok(r) => r,
                        Err(e) => {
                            *lock(&fatal) = Some(fault(e.to_string()));
                            return;
                        }
                    };
                if request.protocol != lycheedev_data::agent_session::PROTOCOL
                    || request.id <= last
                    || request.timeout_ms == 0
                    || request.timeout_ms > 3_600_000
                {
                    *lock(&fatal) = Some(fault(
                        "Invalid Agent request identity, protocol or deadline; no request replayed",
                    ));
                    return;
                }
                last = request.id;
                let kind = match request.task {
                    lycheedev_data::agent_session::Task::Cancel { request_id } => {
                        Kind::Cancel(request_id)
                    }
                    lycheedev_data::agent_session::Task::Close {} => Kind::Close,
                    _ => Kind::Work,
                };
                Event::Input(Input {
                    bytes,
                    id: request.id,
                    deadline: Instant::now() + Duration::from_millis(request.timeout_ms),
                    kind,
                })
            }
            Ok(None) => Event::Eof,
            Err(error) => Event::Error(error),
        };
        let terminal = matches!(
            &event,
            Event::Eof
                | Event::Error(_)
                | Event::Input(Input {
                    kind: Kind::Close,
                    ..
                })
        );
        if sender.try_send(event).is_err() {
            *lock(&fatal) = Some(fault(
                "Agent supervisor ingress queue full; session terminated without replay",
            ));
            return;
        }
        supervisor.unpark();
        if terminal {
            return;
        }
    }
}
fn receive(
    stream: TcpStream,
    sender: mpsc::SyncSender<Result<(Receipt, Instant)>>,
    supervisor: &Thread,
) {
    let mut input = BufReader::new(stream);
    loop {
        let result = match bounded_line(&mut input, 1024) {
            Ok(Some(bytes)) => serde_json::from_slice(&bytes)
                .map(|receipt| (receipt, Instant::now()))
                .map_err(Into::into),
            Ok(None) => return,
            Err(error) => Err(error),
        };
        let failed = result.is_err();
        if sender.send(result).is_err() {
            return;
        }
        supervisor.unpark();
        if failed {
            return;
        }
    }
}
fn retire(owned: &mut Owned, deadline: Instant, terminate: bool) -> Result<()> {
    let mut errors = Vec::<String>::new();
    if let Some(socket) = owned.receipt_socket.take() {
        let _ = socket.shutdown(std::net::Shutdown::Both);
    }
    if terminate {
        let root_kill = match &owned.job {
            Some(job) => match job.terminate() {
                Ok(()) => false,
                Err(error) => {
                    errors.push(error);
                    true
                }
            },
            // Failed suspended startup may return custody before Job attach.
            // This exact child was never resumed; kill and reap it explicitly.
            None => true,
        };
        if root_kill && let Some(child) = &mut owned.child {
            if let Err(error) = child.kill() {
                if child.try_wait().ok().flatten().is_none() {
                    errors.push(error.to_string());
                }
            }
        }
    }
    let mut reaped = false;
    let mut empty = false;
    loop {
        if let Some(child) = &mut owned.child {
            match child.try_wait() {
                Ok(Some(_)) => reaped = true,
                Ok(None) => {}
                Err(error) => {
                    if !errors.contains(&error.to_string()) {
                        errors.push(error.to_string());
                    }
                }
            }
        } else {
            reaped = true;
        }
        if let Some(job) = &owned.job {
            match job.memory_accounting() {
                Ok(value) => empty = value.active_processes == 0,
                Err(error) => {
                    if !errors.contains(&error) {
                        errors.push(error);
                    }
                }
            }
        } else {
            empty = reaped;
        }
        let pipes = owned.writer.as_ref().is_none_or(JoinHandle::is_finished)
            && owned.receipts.as_ref().is_none_or(JoinHandle::is_finished);
        if reaped && empty && pipes {
            break;
        }
        if Instant::now() >= deadline {
            return Err(ToolkitError {
                code: "data.session.cleanup_pending".into(),
                exit_code: 5,
                message: "Owned Data worker retirement is unconfirmed".into(),
                details: serde_json::json!({"rootReaped":reaped,"jobEmpty":empty,"pipeThreadsFinished":pipes,"errors":errors}),
            });
        }
        std::thread::sleep(POLL);
    }
    if let Some(writer) = owned.writer.take() {
        match writer.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                if !terminate {
                    errors.push(error.to_string());
                }
            }
            Err(_) => errors.push("worker input thread panicked".into()),
        }
    }
    if let Some(reader) = owned.receipts.take() {
        if reader.join().is_err() {
            errors.push("worker receipt thread panicked".into());
        }
    }
    if let Some(child) = owned.child.take() {
        if let Err(error) = guarded_spawn::close_child(child) {
            errors.push(error);
        }
    }
    if let Some(job) = owned.job.take() {
        if let Err(error) = job.close() {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ToolkitError {
            code: "data.session.cleanup".into(),
            exit_code: 5,
            message: "Data worker cleanup reported errors".into(),
            details: serde_json::json!({"errors":errors}),
        })
    }
}
pub(super) fn serve(inv: Invocation) -> Result<()> {
    let (context, session) = crate::data_cli::admit_session(&inv)?;
    inv.control.check()?;
    let retained = session.retained_bytes;
    let token = lycheedev_windows::random::token().map_err(fault)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let owner = lycheedev_windows::process::Process::open(std::process::id()).map_err(fault)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--data-agent-worker")
        .arg(&token)
        .arg(listener.local_addr()?.port().to_string())
        .arg(owner.identity().pid.to_string())
        .arg(owner.identity().created.to_string())
        .env(START_ENV, &token)
        .current_dir(&inv.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let startup_deadline = (Instant::now() + Duration::from_secs(10)).min(inv.control.deadline());
    let (child, job) = match guarded_spawn::spawn_suspended_guarded_until(
        command,
        retained + (256 << 20),
        startup_deadline,
        startup_deadline + RETIRE,
        || inv.control.check().is_err(),
    ) {
        Ok(pair) => pair,
        Err(mut failure) => {
            let mut error = inv
                .control
                .check()
                .err()
                .unwrap_or_else(|| fault(failure.cause.clone()));
            error.details = serde_json::json!({"startup":&failure});
            if let Some((child, job)) = failure.take_pending() {
                let mut pending = Owned {
                    child: Some(child),
                    job,
                    writer: None,
                    receipts: None,
                    receipt_socket: None,
                };
                if let Err(cleanup) = retire(&mut pending, Instant::now() + RETIRE, true) {
                    error.details = serde_json::json!({"primary":error.clone(),"cleanup":cleanup});
                }
            }
            return Err(error);
        }
    };
    let mut owned = Owned {
        child: Some(child),
        job: Some(job),
        writer: None,
        receipts: None,
        receipt_socket: None,
    };
    let mut write_sender = None;
    let result = (|| {
        let identity =
            guarded_spawn::child_identity(owned.child.as_ref().unwrap()).map_err(fault)?;
        let mut input = owned
            .child
            .as_mut()
            .unwrap()
            .stdin
            .take()
            .ok_or_else(|| fault("Owned worker input missing"))?;
        let (sender, write_receiver) = mpsc::sync_channel::<Vec<u8>>(16);
        write_sender = Some(sender);
        owned.writer = Some(
            std::thread::Builder::new()
                .name("data-agent-child-input".into())
                .spawn(move || {
                    while let Ok(bytes) = write_receiver.recv() {
                        input.write_all(&bytes)?;
                        input.flush()?;
                    }
                    Ok(())
                })?,
        );
        let lifetime = inv
            .control
            .deadline()
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(3_600_000) as u64;
        let startup = Startup {
            home: inv.home.clone(),
            cwd: inv.cwd.clone(),
            source: context.source,
            session,
            lifetime_ms: lifetime,
        };
        let mut bytes = serde_json::to_vec(&startup)?;
        bytes.push(b'\n');
        if bytes.len() > 65536 {
            return Err(fault("Agent startup settings exceed bound"));
        }
        write_sender
            .as_ref()
            .unwrap()
            .try_send(bytes)
            .map_err(|_| fault("Worker startup input failed"))?;
        let stream = loop {
            inv.control.check()?;
            if Instant::now() >= startup_deadline {
                return Err(fault("Private worker ready deadline expired"));
            }
            if owned.child.as_mut().unwrap().try_wait()?.is_some() {
                return Err(fault(
                    "Worker exited before ready; no ready state established",
                ));
            }
            match listener.accept() {
                Ok((stream, address)) => {
                    if !address.ip().is_loopback() {
                        return Err(fault("Nonlocal receipt peer rejected"));
                    }
                    break stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(POLL)
                }
                Err(error) => return Err(error.into()),
            }
        };
        drop(listener);
        // Winsock inherits the listener's nonblocking mode on accept. Receipt
        // reads run in their own blocking thread; the supervisor independently
        // owns deadlines and shuts this socket down during retirement.
        stream.set_nonblocking(false)?;
        owned.receipt_socket = Some(stream.try_clone()?);
        let (receipt_sender, receipts) = mpsc::sync_channel(32);
        let receipt_supervisor = std::thread::current();
        owned.receipts = Some(
            std::thread::Builder::new()
                .name("data-agent-child-receipts".into())
                .spawn(move || {
                    receive(stream, receipt_sender, &receipt_supervisor);
                    // receive has dropped its sender before announcing EOF.
                    receipt_supervisor.unpark();
                })?,
        );
        let (ready, _) = loop {
            inv.control.check()?;
            if Instant::now() >= startup_deadline {
                return Err(fault("Authenticated ready receipt deadline expired"));
            }
            match receipts.recv_timeout(POLL) {
                Ok(value) => break value?,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(fault("Receipt channel closed before ready")),
            }
        };
        let valid = |r: &Receipt| {
            r.protocol == RECEIPT_PROTOCOL && r.token == token && r.pid == identity.pid
        };
        if !valid(&ready) || ready.event != "ready" || ready.id.is_some() {
            return Err(fault("Invalid authenticated worker ready receipt"));
        }
        let fatal = Arc::new(Mutex::new(None));
        let fatal_reader = fatal.clone();
        let (ingress_sender, ingress_receiver) = mpsc::sync_channel(16);
        let ingress_supervisor = std::thread::current();
        std::thread::Builder::new()
            .name("data-agent-user-input".into())
            .spawn(move || {
                ingress(ingress_sender, fatal_reader, &ingress_supervisor);
                ingress_supervisor.unpark();
            })?;
        let mut pending = BTreeMap::<u64, Instant>::new();
        let mut closing: Option<Input> = None;
        let mut eof = false;
        let mut closed_input = false;
        let mut receipt_closed = false;
        loop {
            inv.control.check()?;
            if let Some(error) = lock(&fatal).take() {
                return Err(error);
            }
            loop {
                let (receipt, received) = match receipts.try_recv() {
                    Ok(receipt) => receipt?,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        receipt_closed = true;
                        break;
                    }
                };
                if !valid(&receipt) || receipt.event != "completed" {
                    return Err(fault("Invalid private completion receipt"));
                }
                let id = receipt
                    .id
                    .ok_or_else(|| fault("Completion receipt lacks request ID"))?;
                let deadline = pending
                    .remove(&id)
                    .ok_or_else(|| fault("Unknown or duplicate completion receipt"))?;
                if received >= deadline {
                    return Err(ToolkitError { code: "command.deadline".into(), exit_code: 7,
                        message: "Completion arrived after its deadline; session invalidated without replay".into(),
                        details: serde_json::json!({"requestId":id,"sessionInvalidated":true,"replayed":false}) });
                }
            }
            if let Some(close) = &closing
                && Instant::now() >= close.deadline
            {
                return Err(ToolkitError { code: "command.deadline".into(), exit_code: 7,
                    message: "Agent close deadline expired while draining submitted work; session invalidated".into(),
                    details: serde_json::json!({"requestId":close.id,"sessionInvalidated":true,"replayed":false}) });
            }
            if let Some((&id, _)) = pending
                .iter()
                .find(|(_, deadline)| Instant::now() >= **deadline)
            {
                return Err(ToolkitError{code:"command.deadline".into(),exit_code:7,message:"Agent request deadline expired; owned worker terminated and all session handles invalidated".into(),details:serde_json::json!({"requestId":id,"sessionInvalidated":true,"replayed":false})});
            }
            let status = if closed_input && receipt_closed {
                let deadline = pending
                    .values()
                    .copied()
                    .min()
                    .unwrap_or(inv.control.deadline())
                    .min(inv.control.deadline());
                guarded_spawn::wait_child_timeout(
                    owned.child.as_mut().unwrap(),
                    POLL.min(deadline.saturating_duration_since(Instant::now())),
                )?
            } else {
                owned.child.as_mut().unwrap().try_wait()?
            };
            if let Some(status) = status {
                // Reaping can race the reader's final already-flushed receipt.
                // Drain the authenticated channel before classifying the exit.
                if !receipt_closed {
                    std::thread::park_timeout(POLL);
                    continue;
                }
                if status.success() && pending.is_empty() && closed_input {
                    return Ok(());
                }
                return Err(ToolkitError {
                    code: "data.session.worker_exited".into(),
                    exit_code: 5,
                    message:
                        "Agent worker exited before the session completed; no request replayed"
                            .into(),
                    details: serde_json::json!({"exitCode":status.code(),"sessionInvalidated":true,"outstandingRequestIds":pending.keys().collect::<Vec<_>>()}),
                });
            }
            if closed_input && receipt_closed {
                continue;
            }
            if pending.is_empty() && !closed_input && (eof || closing.is_some()) {
                if let Some(close) = closing.take() {
                    pending.insert(close.id, close.deadline);
                    write_sender
                        .as_ref()
                        .unwrap()
                        .try_send(close.bytes)
                        .map_err(|_| fault("Worker close queue full"))?;
                }
                write_sender.take();
                closed_input = true;
            }
            if !eof && closing.is_none() && !closed_input {
                match ingress_receiver.try_recv(){
                Ok(Event::Input(input))=>match input.kind{
                    Kind::Close=>closing=Some(input),
                    Kind::Cancel(target)if pending.contains_key(&target)=>return Err(ToolkitError{code:"command.cancelled".into(),exit_code:7,message:"Request cancellation terminates this bounded Agent worker; all session handles invalidated".into(),details:serde_json::json!({"requestId":target,"cancelRequestId":input.id,"sessionInvalidated":true,"replayed":false})}),
                    _=>{pending.insert(input.id,input.deadline);write_sender.as_ref().unwrap().try_send(input.bytes).map_err(|_|fault("Worker input queue full; no automatic replay"))?;}
                },Ok(Event::Eof)=>eof=true,Ok(Event::Error(error))=>return Err(error),Err(mpsc::TryRecvError::Empty)=>std::thread::park_timeout(POLL),Err(mpsc::TryRecvError::Disconnected)=>eof=true,
            }
            } else {
                std::thread::park_timeout(POLL);
            }
        }
    })();
    write_sender.take();
    let cleanup = retire(&mut owned, Instant::now() + RETIRE, result.is_err());
    let outcome = match (result, cleanup) {
        (Ok(()), result) => result,
        (Err(error), Ok(())) => Err(error),
        (Err(mut error), Err(cleanup)) => {
            error.details = serde_json::json!({"primary":error.clone(),"cleanup":cleanup});
            Err(error)
        }
    };
    outcome.map_err(|mut error| {
        // Native pipe/socket errors can precede observation of the worker exit.
        // Every terminal failure retires this session without replaying work.
        error.details["sessionInvalidated"] = true.into();
        error.details["replayed"] = false.into();
        error
    })
}
