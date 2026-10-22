//! Included as admission::activation_store. Reuses the existing admission,
//! execution and claim namespace; there is no second locking implementation.
#![forbid(unsafe_code)]
use super::*;
use crate::mailbox::{
    activation::{Activation, ActivationPhase, ActivationSelection},
    connection::TargetRequest,
};

#[derive(Debug)]
pub struct ActivationRecord {
    pub metadata: ConnectionMetadata,
    pub activation: Activation,
    pub paths: ChannelPaths,
}
impl ActivationRecord {
    pub fn activation_path(&self) -> PathBuf {
        activation_path(&self.paths)
    }
}
fn activation_path(paths: &ChannelPaths) -> PathBuf {
    paths
        .log_path()
        .with_file_name(format!("{}.activation.json", paths.connection_id()))
}
fn index_path(root: &Path, target: &ConnectionTarget, key: &str) -> PathBuf {
    let spelling = format!(
        "{}/{}/{}/{}",
        target.client.directory.to_lowercase(),
        target.window.process_id,
        target.window.process_started_at,
        key
    );
    root.join(".lycheedev/live/activations")
        .join(format!("{}.json", storage::sha256(spelling.as_bytes())))
}
fn read_activation(paths: &ChannelPaths, control: &OperationControl) -> ChannelResult<Activation> {
    let bytes = read_file(
        &activation_path(paths),
        16384,
        "live.channel_record_invalid",
        control,
    )?
    .ok_or_else(|| io_error(std::io::ErrorKind::NotFound.into()))?;
    let mut a = Activation::default();
    let mut decoder = serde_json::Deserializer::from_slice(&bytes);
    RecordSeed(&mut a)
        .deserialize(&mut decoder)
        .map_err(|_| error("live.channel_record_invalid"))?;
    decoder
        .end()
        .map_err(|_| error("live.channel_record_invalid"))?;
    a.validate()?;
    Ok(a)
}

impl WireRecord for ActivationSelection {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "installation" => scalar(&mut self.installation, map),
            "pid" => scalar(&mut self.pid, map),
            "character" => scalar(&mut self.character, map),
            "realm" => scalar(&mut self.realm, map),
            "build" => scalar(&mut self.build, map),
            "product" => scalar(&mut self.product, map),
            _ => ignored(map),
        }
    }
}
impl WireRecord for Activation {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "request" => scalar(&mut self.request, map),
            "phase" => scalar(&mut self.phase, map),
            "priorruntime" => scalar(&mut self.prior_runtime, map),
            "inputstep" => scalar(&mut self.input_step, map),
            "patchtransitions" => scalar(&mut self.patch_transitions, map),
            "selection" => map.next_value_seed(RecordSeed(&mut self.selection)),
            "budget" => {
                self.budget = map.next_value()?;
                Ok(())
            }
            "inputoutcome" => {
                self.outcome = map.next_value()?;
                Ok(())
            }
            _ => ignored(map),
        }
    }
}

/// The claim remains durable after checked-close. Only explicit terminal
/// retirement removes it; Drop releases the execution handle on unwind.
pub struct ActivationLease {
    lease: Option<Lease>,
    channel_seeded: bool,
    pub record: ActivationRecord,
}
impl ActivationLease {
    pub fn channel_seeded(&self) -> bool {
        self.channel_seeded
    }
    pub fn close(&mut self) -> ChannelResult<()> {
        self.lease.take().map_or(Ok(()), Lease::close)
    }
    pub fn save(&self, control: &OperationControl) -> ChannelResult<()> {
        if self.lease.is_none() {
            return Err(error("journal.invalid_transition"));
        }
        self.record.activation.validate()?;
        storage::atomic_write_controlled(
            &self.record.activation_path(),
            &json(&self.record.activation)?,
            control,
        )
        .map_err(storage_error)
    }
    /// The sole activation-to-channel handoff. Persist the initial channel log
    /// before marking runtime_selected. A crash after the log resumes that exact
    /// CON; it never creates a second owner or resends unrecorded activation.
    pub fn select_runtime(
        &mut self,
        state: &State,
        control: &OperationControl,
    ) -> ChannelResult<()> {
        if self.lease.is_none() || state.id != self.record.metadata.owner.operation_id {
            return Err(error("journal.invalid_transition"));
        }
        crate::mailbox::durable_journal::activation_history::seed_activation(
            &self.record.paths,
            state,
            control,
        )?;
        self.channel_seeded = true;
        self.record.activation.phase = ActivationPhase::RuntimeSelected;
        self.save(control)
    }
}
impl ConnectionBook {
    pub fn activation_status(
        &self,
        id: &str,
        control: &OperationControl,
    ) -> ChannelResult<ActivationRecord> {
        let metadata = self.metadata(id, control)?;
        let paths = self.paths(id)?;
        let activation = read_activation(&paths, control)?;
        Ok(ActivationRecord {
            metadata,
            activation,
            paths,
        })
    }
    pub fn activation_reference(
        &self,
        target: &ConnectionTarget,
        key: &str,
        control: &OperationControl,
    ) -> ChannelResult<Option<String>> {
        let Some(bytes) = read_file(
            &index_path(&self.root, target, key),
            4096,
            "live.channel_record_invalid",
            control,
        )?
        else {
            return Ok(None);
        };
        #[derive(serde::Deserialize)]
        struct Reference {
            id: String,
        }
        let reference: Reference =
            serde_json::from_slice(&bytes).map_err(|_| error("live.channel_record_invalid"))?;
        self.paths(&reference.id)?;
        Ok(Some(reference.id))
    }
    pub fn create_activation(
        &mut self,
        target: ConnectionTarget,
        request: &TargetRequest,
        key: &str,
        clock: &impl Clock,
        tokens: &mut impl TokenSource,
        control: &OperationControl,
    ) -> AdmissionResult<ActivationRecord> {
        let activation = Activation::prepared(request, key, clock.unix_millis())?;
        let token = tokens.token()?;
        parse_token(&token)?;
        let id = format!("CON-{token}");
        let paths = self.paths(&id)?;
        let resource = target.resource();
        let owner = WindowOwner {
            schema: OWNER_SCHEMA.into(),
            workspace_id: self.workspace.clone(),
            resource: resource.clone(),
            operation_id: id.clone(),
            intent_sha256: storage::sha256(format!("{resource}/{id}").as_bytes()),
        };
        let metadata = ConnectionMetadata {
            schema: TARGET_SCHEMA.into(),
            target,
            owner,
        };
        let scope = AdmissionScope::acquire(&metadata.target.addon_parent(), control)?;
        let result = (|| {
            if let Some(owner) =
                read_marker(&marker(&scope.path, &resource), Some(&resource), control)?
            {
                if owner.workspace_id == self.workspace {
                    if let Ok(existing) = self.activation_status(&owner.operation_id, control) {
                        if existing.activation.request == key
                            && existing.metadata.owner == owner
                            && existing
                                .metadata
                                .target
                                .same_connection_target(&metadata.target)
                        {
                            return Ok(existing);
                        }
                    }
                }
                check_control(control)?;
                return Err(occupied(owner, &self.workspace));
            }
            owner_limit(&scope.path, control)?;
            if fs::symlink_metadata(Self::target_path(&paths)).is_ok()
                || fs::symlink_metadata(paths.log_path()).is_ok()
                || fs::symlink_metadata(activation_path(&paths)).is_ok()
            {
                return Err(error("live.channel_connection_exists").into());
            }
            write_target_once(
                &Self::target_path(&paths),
                &json(&metadata)?,
                control,
                || check_control(control),
            )?;
            write_target_once(
                &activation_path(&paths),
                &json(&activation)?,
                control,
                || check_control(control),
            )?;
            // Fixed Go publishes ownership only after all persistence in its
            // activation callback succeeds, including the convenience index.
            storage::atomic_write_controlled(
                &index_path(&self.root, &metadata.target, key),
                &json(&serde_json::json!({"id": id}))?,
                control,
            )
            .map_err(storage_error)?;
            publish_owner(&scope.path, &metadata.owner, control)?;
            Ok(ActivationRecord {
                metadata,
                activation,
                paths,
            })
        })();
        finish(result, scope.gate.close())
    }
    pub fn acquire_activation(
        &self,
        id: &str,
        control: &OperationControl,
    ) -> AdmissionResult<ActivationLease> {
        let metadata = self.metadata(id, control)?;
        let scope = AdmissionScope::acquire(&metadata.target.addon_parent(), control)?;
        let acquired = (|| {
            let owner = read_marker(
                &marker(&scope.path, &metadata.owner.resource),
                Some(&metadata.owner.resource),
                control,
            )?;
            if owner.as_ref() != Some(&metadata.owner) {
                return Err(error("live.channel_ownership_changed").into());
            }
            if metadata.owner.resource != metadata.target.resource() {
                return Err(error("live.channel_ownership_changed").into());
            }
            let lease = Lease::acquire(
                &scope.path,
                &format!("execution:{}", metadata.owner.resource),
                false,
                control,
            )?;
            let read = self.activation_status(id, control);
            match read {
                Ok(record) => Ok(ActivationLease {
                    lease: Some(lease),
                    channel_seeded: false,
                    record,
                }),
                Err(e) => finish(Err(e.into()), lease.close()),
            }
        })();
        let closed = scope.gate.close();
        match (acquired, closed) {
            (Ok(lease), Ok(())) => Ok(lease),
            (Ok(mut lease), Err(e)) => finish(Err(e.into()), lease.close()),
            (Err(e), close) => finish(Err(e), close),
        }
    }
    pub fn retire_activation(
        &self,
        id: &str,
        control: &OperationControl,
    ) -> AdmissionResult<ActivationRecord> {
        let record = self.activation_status(id, control)?;
        if record.activation.phase != ActivationPhase::Cancelled {
            return Err(error("journal.invalid_transition").into());
        }
        let scope = AdmissionScope::acquire(&record.metadata.target.addon_parent(), control)?;
        let result = (|| {
            let want = &record.metadata.owner;
            let actual = read_marker(
                &marker(&scope.path, &want.resource),
                Some(&want.resource),
                control,
            )?;
            // Idempotent retirement never touches a new owner.
            if actual.as_ref() != Some(want) {
                return Ok(());
            }
            let lease = Lease::acquire(
                &scope.path,
                &format!("execution:{}", want.resource),
                false,
                control,
            )?;
            let removed = check_control(control).and_then(|_| {
                fs::remove_file(marker(&scope.path, &want.resource)).map_err(io_error)
            });
            finish(removed.map_err(Into::into), lease.close())
        })();
        finish(result, scope.gate.close())?;
        Ok(record)
    }
}
