use super::*;
use lycheedev_core::control::TrackedCancellation;
use lycheedev_windows::BrokerJob;
use std::{
    io::Read,
    sync::Arc,
    thread::{self, JoinHandle},
};

struct Watch {
    stop: TrackedCancellation,
    worker: JoinHandle<Result<()>>,
}
impl Watch {
    fn start(stream: &BrokerStream, control: &OperationControl) -> Result<Self> {
        let mut reader = stream.try_clone().map_err(platform)?;
        let stop = TrackedCancellation::new();
        let stopped = stop.clone();
        let cancel = control.tracked_cancel_handle()?;
        let monitor = OperationControl::tracked(control.deadline(), stop.clone());
        reader.set_deadline_with_control(monitor.deadline(), check(&monitor));
        let custody = Arc::new(std::sync::Mutex::new(Some(reader)));
        let transferred = custody.clone();
        match thread::Builder::new()
            .name("dd-task-caller".into())
            .spawn(move || {
                let mut reader = transferred.lock().unwrap().take().unwrap();
                let mut byte = [0];
                let _ = reader.read(&mut byte);
                if !stopped.is_cancelled() {
                    cancel.cancel();
                }
                reader.close().map_err(platform)
            }) {
            Ok(worker) => Ok(Self { stop, worker }),
            Err(error) => {
                let reader = custody.lock().unwrap().take().unwrap();
                finish_owned(Err(error.into()), reader.close().map_err(platform))
            }
        }
    }
    fn finish(self) -> Result<()> {
        self.stop.cancel();
        self.worker
            .join()
            .map_err(|_| fault("DD caller monitor panicked"))?
    }
}

pub fn entry(args: &[String]) -> Result<()> {
    if args.len() != 3
        || args[0] != "--publish-dd-task-owner"
        || !session_id(&args[2])
        || std::env::var(START_ENV).ok().as_deref() != Some(args[2].as_str())
    {
        return Err(invalid());
    }
    let home = PathBuf::from(&args[1]);
    if !home.is_absolute() {
        return Err(invalid());
    }
    serve(&home, &args[2], IDLE, |store, record, control| {
        Worker::spawn_recorded(
            &store.home,
            &record.executable.clone(),
            control,
            |identity| {
                record.worker = Some(identity.clone());
                store.save(record, control)
            },
        )
    })
}
pub(super) fn serve<B: Backend>(
    home: &Path,
    id: &str,
    idle_span: Duration,
    factory: impl FnOnce(&Store, &mut Record, &OperationControl) -> Result<B>,
) -> Result<()> {
    let opening = OperationControl::uncancelable(Instant::now() + Duration::from_secs(90));
    let store = Store::open(home, &opening, true)?;
    let mut record = store.record(id, &opening)?;
    if record.phase != Phase::Starting || record.start_deadline_ms <= now_ms()? {
        return Err(invalid());
    }
    let startup = operation(record.start_deadline_ms, 90_000)?;
    let (file, digest) = pin(&std::env::current_exe()?, &startup)?;
    let admitted = (|| {
        if digest != record.digest
            || std::env::current_exe()?.canonicalize()? != record.executable.canonicalize()?
        {
            return Err(invalid());
        }
        let parent = Process::open(record.parent.pid).map_err(platform)?;
        let verified = (|| {
            if parent.identity() != &record.parent
                || parent.image_path().map_err(platform)?.canonicalize()?
                    != record.executable.canonicalize()?
            {
                return Err(invalid());
            }
            Ok(())
        })();
        finish_owned(verified, parent.close().map_err(platform))
    })();
    if let Err(error) = admitted {
        return storage::finish_file(file, Err(error), &startup);
    }
    let lease = match storage::ExclusiveLock::resource_controlled(&store.root, "owner", &startup) {
        Ok(lease) => lease,
        Err(error) => return storage::finish_file(file, Err(error), &startup),
    };
    let job = match BrokerJob::own_current_process_inherited() {
        Ok(job) => job,
        Err(error) => {
            return finish_owned(
                storage::finish_file(file, Err(platform(error)), &startup),
                lease.close(),
            );
        }
    };
    record.owner = Some(current()?);
    let mut listener = match BrokerListener::bind(&store.scope) {
        Ok(listener) => listener,
        Err(error) => {
            return finish_owned(
                finish_owned(
                    storage::finish_file(file, Err(platform(error)), &startup),
                    lease.close(),
                ),
                job.close().map_err(platform),
            );
        }
    };
    let mut backend: Option<B> = None;
    let mut factory = Some(factory);
    let mut pending: Option<(BrokerStream, String, OperationControl, Result<Value>)> = None;
    let result = (|| {
        store.save(&record, &startup)?;
        let mut idle = Instant::now() + idle_span;
        loop {
            let cutoff = if backend.is_none() && record.phase == Phase::Starting {
                operation(record.start_deadline_ms, 90_000)?.deadline()
            } else {
                idle
            };
            if Instant::now() >= cutoff {
                break Ok(());
            }
            let observe = (Instant::now() + Duration::from_secs(5)).min(cutoff);
            let accept = OperationControl::uncancelable(cutoff);
            let mut stream = match listener.accept_with_control(observe, check(&accept)) {
                Ok(stream) => stream,
                Err(message) if message == "broker.deadline" && Instant::now() >= observe => {
                    if Instant::now() >= cutoff {
                        break Ok(());
                    }
                    if let Some(backend) = &mut backend {
                        let probe =
                            OperationControl::uncancelable((Instant::now() + REAP).min(cutoff));
                        let status = backend.execute(Call::Status, &probe)?;
                        if status.pointer("/cc/active") == Some(&Value::Bool(false)) {
                            return Err(fault("DD connection retired while the task was idle"));
                        }
                    }
                    continue;
                }
                Err(message) => return Err(platform(message)),
            };
            let handshake = OperationControl::uncancelable((Instant::now() + REAP).min(cutoff));
            stream.set_deadline_with_control(handshake.deadline(), check(&handshake));
            let mut stop = false;
            let connected = (|| {
                let peer = Process::open(stream.peer_pid().map_err(platform)?).map_err(platform)?;
                let peer_id = peer.identity().clone();
                let verified = (|| {
                    if peer.image_path().map_err(platform)?.canonicalize()?
                        != record.executable.canonicalize()?
                    {
                        return Err(invalid());
                    }
                    Ok(())
                })();
                finish_owned(verified, peer.close().map_err(platform))?;
                send(
                    &mut stream,
                    &Hello {
                        wire: WIRE.into(),
                        session: record.id.clone(),
                        owner: record.owner.clone().unwrap(),
                        digest: record.digest.clone(),
                    },
                )?;
                let request: Request = read_frame(&mut stream)?;
                if request.wire != WIRE
                    || request.session != record.id
                    || !hex(&request.id, 32)
                    || record
                        .last
                        .as_ref()
                        .is_some_and(|last| last.id == request.id)
                {
                    return Err(invalid());
                }
                let control = operation(request.deadline_ms, 900_000)?;
                if matches!(request.action, Action::Activate) && peer_id != record.parent {
                    return Err(invalid());
                }
                stream.set_deadline_with_control(control.deadline(), check(&control));
                let watch = Watch::start(&stream, &control)?;
                let reply = (|| match request.action {
                    Action::Activate => {
                        if backend.is_none() {
                            if record.phase != Phase::Starting {
                                return Err(fault("DD task cannot repeat its original activation"));
                            }
                            let startup_deadline = operation(record.start_deadline_ms, 90_000)?
                                .deadline()
                                .min(control.deadline());
                            let activation = control.ordered_child(startup_deadline)?;
                            let started = factory.take().ok_or_else(invalid)?(
                                &store,
                                &mut record,
                                &activation,
                            );
                            match started {
                                Ok(value) => {
                                    backend = Some(value);
                                    activation.check()?;
                                    record.identity = backend.as_ref().unwrap().identity();
                                    record.phase = Phase::Ready;
                                    store.save(&record, &control)?;
                                }
                                Err(error) => {
                                    record.error = Some(error.clone());
                                    record.phase = Phase::Failed;
                                    stop = true;
                                    return Err(error);
                                }
                            }
                        }
                        Ok(view(&record, true))
                    }
                    Action::Status => {
                        if let Some(backend) = &mut backend {
                            record.identity = backend.execute(Call::Status, &control)?;
                            if record.identity.pointer("/cc/active") == Some(&Value::Bool(false)) {
                                record.phase = Phase::Interrupted;
                                stop = true;
                                return Err(fault(
                                    "the original DD connection is retired; no replacement login was attempted",
                                ));
                            }
                        }
                        Ok(view(&record, true))
                    }
                    Action::Stop => {
                        stop = true;
                        Ok(json!({"stopping":true}))
                    }
                    Action::Execute { call } => {
                        if matches!(call, Call::Stop) {
                            return Err(invalid());
                        }
                        let mutation = call.mutates();
                        let backend = backend
                            .as_mut()
                            .ok_or_else(|| fault("DD task has no ready session"))?;
                        record.last = Some(Last {
                            id: request.id.clone(),
                            input_sha256: storage::sha256(&storage::identity_json(
                                &serde_json::to_value(&call)?,
                            )?),
                            deadline_ms: request.deadline_ms,
                            mutation,
                            outcome: "accepted".into(),
                        });
                        store
                            .save(&record, &control)
                            .map_err(|e| super::super::submission(e, mutation, "not_sent"))?;
                        let result = backend.execute(call, &control);
                        record.last.as_mut().unwrap().outcome = match &result {
                            Ok(_) => "completed",
                            Err(error) if mutation => match error.details["submission"].as_str() {
                                Some("not_sent") => "not_sent",
                                Some("rejected") => "rejected",
                                _ => "uncertain",
                            },
                            Err(_) => "failed",
                        }
                        .into();
                        if backend.retired() {
                            record.phase = Phase::Interrupted;
                            stop = true;
                        }
                        let save = OperationControl::uncancelable(Instant::now() + REAP);
                        finish_owned(result, store.save(&record, &save))
                    }
                })();
                let reply = finish_owned(reply, watch.finish());
                if stop {
                    pending = Some((
                        stream.try_clone().map_err(platform)?,
                        request.id,
                        control,
                        reply,
                    ));
                    return Ok(());
                }
                send(
                    &mut stream,
                    &Reply {
                        wire: WIRE.into(),
                        session: record.id.clone(),
                        id: request.id,
                        result: reply,
                    },
                )?;
                idle = Instant::now() + idle_span;
                Ok(())
            })();
            let closed = stream.close().map_err(platform);
            if stop {
                finish_owned(connected, closed)?;
                break Ok(());
            }
            // Invalid/incomplete peers do not reset idle or start authentication.
            if let Err(error) = closed {
                return Err(error);
            }
            if let Err(error) = connected {
                if record.phase != Phase::Starting && backend.as_ref().is_some_and(Backend::retired)
                {
                    return Err(error);
                }
            }
        }
    })();
    let cleanup = OperationControl::uncancelable(Instant::now() + REAP);
    let closed = backend.take().map_or(Ok(()), |backend| {
        backend.close(pending.as_ref().map(|p| &p.2).unwrap_or(&cleanup))
    });
    let primary = result;
    let mut released = finish_owned(closed, listener.close().map_err(platform));
    released = finish_owned(released, lease.close());
    released = storage::finish_file(file, released, &cleanup);
    released = finish_owned(released, job.close().map_err(platform));
    if released.is_err() {
        record.phase = Phase::CleanupPending;
    } else if primary.is_err() {
        record.phase = if record.phase == Phase::Starting {
            Phase::Failed
        } else {
            Phase::Interrupted
        };
    } else if record.phase != Phase::Failed && record.phase != Phase::Interrupted {
        record.phase = Phase::ResourcesReleased;
    }
    let mut outcome = finish_owned(primary, released);
    if let Err(error) = &outcome {
        record.error = Some(error.clone());
    }
    let saved = store.save(&record, &cleanup);
    outcome = finish_owned(outcome, saved);
    if let Some((mut stream, id, control, reply)) = pending {
        stream.set_deadline_with_control(control.deadline(), check(&control));
        let reply = match reply {
            Err(error) => Err(error),
            Ok(_) if outcome.is_ok() => {
                let mut value = view(&record, false);
                value["cleanup_complete"] = json!(true);
                Ok(value)
            }
            Ok(_) => Err(outcome.clone().unwrap_err()),
        };
        outcome = finish_owned(
            outcome,
            send(
                &mut stream,
                &Reply {
                    wire: WIRE.into(),
                    session: record.id.clone(),
                    id,
                    result: reply,
                },
            ),
        );
        outcome = finish_owned(outcome, stream.close().map_err(platform));
    }
    outcome
}
