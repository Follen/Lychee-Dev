use super::*;
use lycheedev_windows::{
    broker::NativeSourcePeer,
    detached::{self, DetachedChild},
};
use std::ffi::OsString;

/// An explicit task selector, not a cached authentication capability.
pub struct Client {
    home: PathBuf,
    executable: PathBuf,
    session: String,
}
impl Client {
    pub fn attach(home: &Path, executable: &Path, session: &str) -> Result<Self> {
        validate_session_id(session)?;
        Ok(Self {
            home: home.to_owned(),
            executable: executable.to_owned(),
            session: session.into(),
        })
    }
    pub fn start(home: &Path, executable: &Path, control: &OperationControl) -> Result<Value> {
        Self::start_with(home, executable, control, |record, home| {
            detached::spawn_hidden_no_inherit(
                &record.executable,
                &[
                    OsString::from("--publish-dd-task-owner"),
                    home.as_os_str().to_owned(),
                    OsString::from(&record.id),
                ],
                home,
                &[(START_ENV.into(), OsString::from(&record.id))],
            )
            .map_err(platform)
        })
    }
    pub(super) fn start_with(
        home: &Path,
        executable: &Path,
        control: &OperationControl,
        launch: impl FnOnce(&Record, &Path) -> Result<DetachedChild>,
    ) -> Result<Value> {
        let control = control.ordered_child(Instant::now() + Duration::from_secs(90))?;
        let store = Store::open(home, &control, true)?;
        let launch_gate =
            storage::ExclusiveLock::resource_controlled(&store.root, "launch", &control)?;
        let (file, digest) = pin(executable, &control)?;
        let started = (|| {
            if let Some(record) = store.active(&control)? {
                if !store.released(&record)? {
                    let client = Self::attach(home, executable, &record.id)?;
                    let mut status=client.request_with(&store,&digest,Action::Status,&control)?.ok_or_else(||ToolkitError::new("publish.dd_task_pending","the original DD task owner is still present; inspect its session before starting another",6).with_details(view(&record,true)))?;
                    if status["phase"] != "ready" {
                        return Err(ToolkitError::new(
                            "publish.dd_task_pending",
                            "DD task startup or cleanup is still in progress",
                            6,
                        )
                        .with_details(status));
                    }
                    status["reused"] = json!(true);
                    return Ok(status);
                }
                if startup_pending(&record)? {
                    return Err(pending(&record));
                }
            }
            let id = format!("DDT-{}", token()?);
            let record = Record {
                schema: SCHEMA.into(),
                id: id.clone(),
                digest: digest.clone(),
                executable: executable.canonicalize()?,
                parent: current()?,
                start_deadline_ms: wire_deadline(&control)?,
                owner: None,
                worker: None,
                phase: Phase::Starting,
                identity: Value::Null,
                last: None,
                error: None,
            };
            store.save(&record, &control)?;
            store.select(&id, &control)?;
            let child = match launch(&record, home) {
                Ok(child) => child,
                Err(error) => {
                    let mut failed = record;
                    failed.phase = Phase::Failed;
                    failed.error = Some(error.clone());
                    let cleanup = OperationControl::uncancelable(Instant::now() + REAP);
                    return finish_owned(Err(error), store.save(&failed, &cleanup));
                }
            };
            let client = Self::attach(home, executable, &id)?;
            let activated = (|| {
                let mut launched = record;
                launched.owner = Some(child.identity().clone());
                store.save(&launched, &control)?;
                loop {
                    control.check()?;
                    if child.try_wait().map_err(platform)?.is_some() {
                        let latest = store.record(&id, &control)?;
                        return Err(latest
                            .error
                            .unwrap_or_else(|| fault("DD task owner exited during startup")));
                    }
                    match client.request_with(&store, &digest, Action::Activate, &control) {
                        Ok(Some(mut value)) => {
                            if value["owner"]["pid"].as_u64() != Some(child.id() as u64)
                                || value["owner"]["created"].as_u64()
                                    != Some(child.identity().created)
                            {
                                return Err(invalid());
                            }
                            value["reused"] = json!(false);
                            return Ok(value);
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                        Err(error) => return Err(error),
                    }
                }
            })();
            match activated {
                Ok(value) => {
                    child.handoff().map_err(platform)?;
                    Ok(value)
                }
                Err(error) => {
                    let deadline = Instant::now() + REAP;
                    let retired = child
                        .terminate_and_close(deadline)
                        .map(|_| ())
                        .map_err(platform);
                    let cleanup = OperationControl::uncancelable(deadline);
                    if retired.is_ok() {
                        if let Ok(mut latest) = store.record(&id, &cleanup) {
                            latest.phase = if !store.released(&latest)? {
                                Phase::CleanupPending
                            } else if latest.phase == Phase::Failed {
                                Phase::Failed
                            } else {
                                Phase::Interrupted
                            };
                            latest.error = Some(error.clone());
                            return finish_owned(Err(error), store.save(&latest, &cleanup));
                        }
                    }
                    finish_owned(Err(error), retired)
                }
            }
        })();
        let result = storage::finish_file(file, started, &control);
        finish_owned(result, launch_gate.close())
    }
    pub fn status(
        home: &Path,
        executable: &Path,
        session: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        control.check()?;
        if let Some(session) = session {
            validate_session_id(session)?;
        }
        if !home.exists() {
            return Ok(json!({"running":false,"session_id":session}));
        }
        let store = Store::open(home, control, false)?;
        let record = match session {
            Some(id) => Some(store.record(id, control)?),
            None => store.active(control)?,
        };
        let Some(record) = record else {
            return Ok(json!({"running":false}));
        };
        if startup_pending(&record)? {
            let mut value = view(&record, false);
            value["startup_pending"] = json!(true);
            value["cleanup_complete"] = json!(false);
            return Ok(value);
        }
        let (file, digest) = pin(executable, control)?;
        let result = (|| {
            if store.released(&record)? {
                let mut value = view(&record, false);
                value["cleanup_complete"] = json!(true);
                if record.phase == Phase::Starting || record.phase == Phase::Ready {
                    value["phase"] = json!("interrupted");
                }
                if record.phase == Phase::CleanupPending {
                    value["phase"] = json!("resources_released");
                }
                if record
                    .last
                    .as_ref()
                    .is_some_and(|r| r.outcome == "accepted")
                {
                    value["last_operation"]["outcome"] = json!("uncertain");
                }
                return Ok(value);
            }
            let client = Self::attach(home, executable, &record.id)?;
            match client.request_with(&store, &digest, Action::Status, control)? {
                Some(value) => Ok(value),
                None => {
                    let mut value = view(&record, true);
                    if record.phase == Phase::Starting && record.start_deadline_ms > now_ms()? {
                        value["startup_pending"] = json!(true);
                    } else {
                        value["phase"] = json!("owner_unavailable");
                    }
                    value["cleanup_complete"] = json!(false);
                    Ok(value)
                }
            }
        })();
        storage::finish_file(file, result, control)
    }
    pub fn stop(&self, control: &OperationControl) -> Result<Value> {
        let store = Store::open(&self.home, control, false)?;
        let record = store.record(&self.session, control)?;
        if startup_pending(&record)? {
            return Err(pending(&record));
        }
        if store.released(&record)? {
            let mut value = view(&record, false);
            value["cleanup_complete"] = json!(true);
            return Ok(value);
        }
        let result = self.request(Action::Stop, control)?;
        while !store.released(&record)? {
            control.check()?;
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(result)
    }
    pub fn get(
        &self,
        path: &str,
        query: &[(&str, String)],
        control: &OperationControl,
    ) -> Result<Value> {
        self.request(
            Action::Execute {
                call: Call::Get {
                    path: path.into(),
                    query: query
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.clone()))
                        .collect(),
                },
            },
            control,
        )
    }
    pub fn post(
        &self,
        path: &str,
        query: &[(&str, String)],
        body: &Value,
        mutation: bool,
        control: &OperationControl,
    ) -> Result<Value> {
        self.request(
            Action::Execute {
                call: Call::Post {
                    path: path.into(),
                    query: query
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.clone()))
                        .collect(),
                    body: body.clone(),
                    mutation,
                },
            },
            control,
        )
    }
    pub(in crate::dd) fn author_identity(&self, control: &OperationControl) -> Result<Value> {
        self.request(Action::Execute { call: Call::Status }, control)
    }
    pub fn channels(&self, control: &OperationControl) -> Result<Value> {
        self.request(
            Action::Execute {
                call: Call::Channels,
            },
            control,
        )
    }
    pub(in crate::dd) fn upload(
        &self,
        request: &super::super::upload::Request,
        control: &OperationControl,
    ) -> Result<Value> {
        self.request(
            Action::Execute {
                call: Call::Upload {
                    request: request.clone(),
                },
            },
            control,
        )
    }
    pub(in crate::dd) fn backup_upload(
        &self,
        request: &super::super::backup::upload::UploadRequest,
        control: &OperationControl,
    ) -> Result<Value> {
        self.request(
            Action::Execute {
                call: Call::BackupUpload {
                    request: request.clone(),
                },
            },
            control,
        )
    }
    fn request(&self, action: Action, control: &OperationControl) -> Result<Value> {
        let mutation = action.mutation();
        let not_sent = |error| super::super::submission(error, mutation, "not_sent");
        let store = Store::open(&self.home, control, false).map_err(not_sent)?;
        let (file, digest) = pin(&self.executable, control).map_err(not_sent)?;
        let result = self
            .request_with(&store, &digest, action, control)
            .and_then(|value| {
                value.ok_or_else(|| {
                    not_sent(fault(
                        "the original DD task owner is unavailable; inspect its status",
                    ))
                })
            });
        storage::finish_file(file, result, control)
    }
    fn request_with(
        &self,
        store: &Store,
        digest: &str,
        action: Action,
        control: &OperationControl,
    ) -> Result<Option<Value>> {
        let mutation = action.mutation();
        let not_sent = |error| super::super::submission(error, mutation, "not_sent");
        control.check().map_err(not_sent)?;
        let record = store.record(&self.session, control).map_err(not_sent)?;
        if record.owner.is_none() {
            return Ok(None);
        }
        if record.digest != digest {
            return Err(not_sent(fault(
                "this DD task belongs to another CLI build; use its original executable to close it",
            )));
        }
        let mut stream = match BrokerStream::connect_with_control(
            &store.scope,
            control.deadline(),
            check(control),
        ) {
            Ok(stream) => stream,
            Err(message) if message.starts_with("broker.not_found:") => return Ok(None),
            Err(message) => return Err(not_sent(platform(message))),
        };
        let mut retirement: Option<NativeSourcePeer> = None;
        let mut sent = false;
        let result = (|| {
            let hello: Hello = read_frame(&mut stream)?;
            if hello.wire != WIRE
                || hello.session != self.session
                || hello.digest != digest
                || stream.peer_pid().map_err(platform)? != hello.owner.pid
                || record.owner.as_ref() != Some(&hello.owner)
            {
                return Err(invalid());
            }
            retirement = Some(
                NativeSourcePeer::open(hello.owner.pid, hello.owner.created, &self.executable)
                    .map_err(platform)?,
            );
            let id = token()?;
            let request = Request {
                wire: WIRE.into(),
                session: self.session.clone(),
                id: id.clone(),
                deadline_ms: wire_deadline(control)?,
                action,
            };
            let bytes = worker::encode(&request)?;
            control.check()?;
            sent = true;
            use std::io::Write;
            stream
                .write_all(&bytes)
                .map_err(|_| fault("DD task request transport ended"))?;
            let reply: Reply = read_frame(&mut stream)?;
            if reply.wire != WIRE || reply.session != self.session || reply.id != id {
                return Err(invalid());
            }
            control.check()?;
            Ok(reply.result)
        })();
        let result = match result {
            Ok(value) => value.map(Some),
            Err(error) => {
                let primary = super::super::submission(
                    control.check().err().unwrap_or(error),
                    mutation,
                    if sent { "uncertain" } else { "not_sent" },
                );
                if sent {
                    if let Some(peer) = &retirement {
                        finish_owned(
                            Err(primary),
                            peer.retire(Instant::now() + REAP).map_err(platform),
                        )
                    } else {
                        Err(primary)
                    }
                } else {
                    Err(primary)
                }
            }
        };
        let result = if let Some(peer) = retirement {
            finish_owned(result, peer.close().map_err(platform))
        } else {
            result
        };
        finish_owned(result, stream.close().map_err(platform))
    }
}
