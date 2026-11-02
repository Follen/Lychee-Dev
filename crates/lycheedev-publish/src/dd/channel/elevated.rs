//! One fixed same-user helper, one nonce pipe, original deadline including UAC.
use super::super::{
    desktop::ChannelAnchor,
    worker::{check, encode, operation, read_frame_bounded, wire_deadline},
};
use super::{Choices, fault};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, metadata::owned_write::finish_owned, storage,
};
use lycheedev_windows::{
    BrokerJob, Process, ProcessIdentity,
    broker::{BrokerListener, BrokerScope, BrokerStream},
    process::{
        ProcessLifetime,
        elevation::{ChannelChild, SameUserPeer},
        observe_lifetime,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};
const WIRE: &str = "lycheedev.publish.dd-channel.v1";
const LIMIT: usize = 1 << 20;
const MAXIMUM: Duration = Duration::from_secs(55);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    wire: String,
    nonce: String,
    process: ProcessIdentity,
    digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    wire: String,
    nonce: String,
    deadline_ms: u64,
    anchor: ChannelAnchor,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    wire: String,
    nonce: String,
    choices: Result<Choices>,
}
fn invalid() -> ToolkitError {
    fault("invalid private DD channel helper identity or response")
}
fn platform(_: String) -> ToolkitError {
    fault("the one-shot DD channel helper process or pipe failed")
}
fn own() -> Result<ProcessIdentity> {
    let process = Process::open(std::process::id()).map_err(platform)?;
    let identity = process.identity().clone();
    process.close().map_err(platform)?;
    Ok(identity)
}
fn send<T: Serialize>(stream: &mut BrokerStream, value: &T) -> Result<()> {
    let bytes = encode(value)?;
    if bytes.len() > LIMIT + 4 {
        return Err(invalid());
    }
    stream.write_all(&bytes).map_err(|_| invalid())
}
trait Child {
    fn identity(&self) -> &ProcessIdentity;
    fn verify(&self, executable: &Path) -> Result<()>;
    fn wait(&self, control: &OperationControl) -> Result<()>;
    fn retire(&mut self, deadline: Instant) -> Result<()>;
    fn close(self) -> Result<()>;
}
impl Child for ChannelChild {
    fn identity(&self) -> &ProcessIdentity {
        self.identity()
    }
    fn verify(&self, executable: &Path) -> Result<()> {
        self.verify(executable).map_err(platform)
    }
    fn wait(&self, control: &OperationControl) -> Result<()> {
        self.wait_exit(control.deadline(), || {
            control
                .check()
                .map_err(|_| "DD channel operation stopped".into())
        })
        .map_err(|e| control.check().err().unwrap_or_else(|| platform(e)))
        .and_then(|code| {
            if code == 0 {
                Ok(())
            } else {
                Err(fault("DD channel helper exited with a cleanup failure"))
            }
        })
    }
    fn retire(&mut self, deadline: Instant) -> Result<()> {
        ChannelChild::retire(self, deadline).map_err(platform)
    }
    fn close(self) -> Result<()> {
        self.close().map_err(platform)
    }
}
pub(super) fn call(
    home: &Path,
    anchor: &ChannelAnchor,
    control: &OperationControl,
) -> Result<Choices> {
    call_with(home, anchor, control, |home, parent, nonce, deadline| {
        ChannelChild::launch(home, parent, nonce, deadline).map_err(|error| {
            control.check().err().unwrap_or_else(|| {
                if error == lycheedev_windows::process::elevation::CANCELLED {
                    ToolkitError::new(
                        "publish.dd_channel_cancelled",
                        "DD channel permission request was cancelled",
                        7,
                    )
                } else {
                    platform(error)
                }
            })
        })
    })
}
fn call_with<C: Child>(
    home: &Path,
    anchor: &ChannelAnchor,
    control: &OperationControl,
    launch: impl FnOnce(&Path, &ProcessIdentity, &str, u64) -> Result<C>,
) -> Result<Choices> {
    let local = control.ordered_child(Instant::now() + MAXIMUM)?;
    let control = &local;
    control.check()?;
    let parent = own()?;
    let executable = std::env::current_exe()?.canonicalize()?;
    let (file, digest) = storage::locked_sha256(&executable, 512 << 20, control)?;
    let result = (|| {
        let nonce = lycheedev_windows::random::token().map_err(platform)?;
        let scope = BrokerScope::for_dd_channel(home, &nonce).map_err(platform)?;
        let mut listener = BrokerListener::bind(&scope).map_err(platform)?;
        let result = (|| {
            let deadline_ms = wire_deadline(control)?;
            // The deadline is fixed before ShellExecuteEx/UAC. The enclosing
            // native worker remains subject to its caller's owned Job watchdog.
            let mut child = launch(home, &parent, &nonce, deadline_ms)?;
            let result = (|| {
                control.check()?;
                child.verify(&executable)?;
                let mut stream = listener
                    .accept_with_control(control.deadline(), check(control))
                    .map_err(|e| control.check().err().unwrap_or_else(|| platform(e)))?;
                let result = (|| {
                    if stream.peer_pid().map_err(platform)? != child.identity().pid {
                        return Err(invalid());
                    }
                    let hello: Hello = read_frame_bounded(&mut stream, LIMIT)?;
                    if hello.wire != WIRE
                        || hello.nonce != nonce
                        || hello.process != *child.identity()
                        || hello.digest != digest
                    {
                        return Err(invalid());
                    }
                    child.verify(&executable)?;
                    send(
                        &mut stream,
                        &Request {
                            wire: WIRE.into(),
                            nonce: nonce.clone(),
                            deadline_ms,
                            anchor: anchor.clone(),
                        },
                    )?;
                    let reply: Reply = read_frame_bounded(&mut stream, LIMIT)?;
                    if reply.wire != WIRE || reply.nonce != nonce {
                        return Err(invalid());
                    }
                    control.check()?;
                    let choices = reply.choices?;
                    choices.validate()?;
                    Ok(choices)
                })();
                finish_owned(result, stream.close().map_err(platform))
            })();
            let (cleanup, pending) = if result.is_ok() {
                match child.wait(control) {
                    Ok(()) => (Ok(()), false),
                    Err(error) => {
                        let retired = child.retire(Instant::now() + Duration::from_secs(5));
                        let pending = retired.is_err();
                        (finish_owned(Err(error), retired), pending)
                    }
                }
            } else {
                let retired = child.retire(Instant::now() + Duration::from_secs(5));
                let pending = retired.is_err();
                (retired, pending)
            };
            let id = child.identity().clone();
            let cleanup = finish_owned(cleanup, child.close());
            match (result, cleanup) {
                (Ok(choices), Err(error)) => Err(error.with_details(serde_json::json!({"verified_channels":choices,"helper":id,"cleanup_pending":pending}))),
                (result, cleanup) => finish_owned(result, cleanup),
            }
        })();
        finish_owned(result, listener.close().map_err(platform))
    })();
    storage::finish_file(file, result, control)
}
struct Binding {
    home: PathBuf,
    parent: ProcessIdentity,
    nonce: String,
    deadline_ms: u64,
}
impl Binding {
    fn parse(args: &[String]) -> Result<Self> {
        if args.len() != 6 || args[0] != "--publish-dd-channel-helper" {
            return Err(invalid());
        }
        let home = PathBuf::from(&args[1]);
        let parent = ProcessIdentity {
            pid: args[2].parse().map_err(|_| invalid())?,
            created: args[3].parse().map_err(|_| invalid())?,
        };
        let nonce = args[4].clone();
        let deadline_ms = args[5].parse().map_err(|_| invalid())?;
        if !home.is_absolute()
            || parent.pid == 0
            || parent.created == 0
            || nonce.len() != 32
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
        Ok(Self {
            home,
            parent,
            nonce,
            deadline_ms,
        })
    }
}
// UAC cannot assign a Job before the helper's first instruction. Before any
// credential work, this timer binds the original parent and absolute deadline;
// parent loss/deadline exits only this auxiliary process, closing its own Job.
struct Watch {
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Watch {
    fn start(parent: ProcessIdentity, deadline: Instant) -> Self {
        let (stop, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            loop {
                if Instant::now() >= deadline
                    || !matches!(
                        observe_lifetime(&parent, || Ok(())),
                        Ok(ProcessLifetime::Running)
                    )
                {
                    std::process::exit(124);
                }
                match receiver.recv_timeout(
                    Duration::from_millis(50)
                        .min(deadline.saturating_duration_since(Instant::now())),
                ) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub(super) fn entry(args: &[String]) -> Result<()> {
    serve(Binding::parse(args)?, |anchor, control, parent| {
        super::gui::read(anchor, control, parent)
    })
}
fn serve(
    binding: Binding,
    read: impl FnOnce(
        &ChannelAnchor,
        &OperationControl,
        &mut dyn FnMut() -> Result<()>,
    ) -> Result<Choices>,
) -> Result<()> {
    let control = operation(binding.deadline_ms, MAXIMUM.as_millis() as u64)?;
    let _watch = Watch::start(binding.parent.clone(), control.deadline());
    let executable = std::env::current_exe()?.canonicalize()?;
    let parent = SameUserPeer::open(&binding.parent, &executable).map_err(platform)?;
    let result = (|| {
        let (file, digest) = storage::locked_sha256(&executable, 512 << 20, &control)?;
        let result = (|| {
            let scope =
                BrokerScope::for_dd_channel(&binding.home, &binding.nonce).map_err(platform)?;
            let mut stream =
                BrokerStream::connect_with_control(&scope, control.deadline(), check(&control))
                    .map_err(platform)?;
            let result = (|| {
                if stream.peer_pid().map_err(platform)? != binding.parent.pid {
                    return Err(invalid());
                }
                parent.verify().map_err(platform)?;
                send(
                    &mut stream,
                    &Hello {
                        wire: WIRE.into(),
                        nonce: binding.nonce.clone(),
                        process: own()?,
                        digest,
                    },
                )?;
                let request: Request = read_frame_bounded(&mut stream, LIMIT)?;
                if request.wire != WIRE
                    || request.nonce != binding.nonce
                    || request.deadline_ms != binding.deadline_ms
                {
                    return Err(invalid());
                }
                control.check()?;
                let job = BrokerJob::own_current_process_inherited().map_err(platform)?;
                let result = (|| {
                    let mut verify = || {
                        control.check()?;
                        parent.verify().map_err(platform)
                    };
                    let choices =
                        read(&request.anchor, &control, &mut verify).and_then(|choices| {
                            choices.validate()?;
                            Ok(choices)
                        });
                    verify()?;
                    send(
                        &mut stream,
                        &Reply {
                            wire: WIRE.into(),
                            nonce: binding.nonce,
                            choices,
                        },
                    )
                })();
                finish_owned(result, job.close().map_err(platform))
            })();
            finish_owned(result, stream.close().map_err(platform))
        })();
        storage::finish_file(file, result, &control)
    })();
    finish_owned(result, parent.close().map_err(platform))
}

#[cfg(test)]
mod tests;
