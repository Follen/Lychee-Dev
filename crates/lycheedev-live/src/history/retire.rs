//! Explicit disk-only retirement of released-Go historical OP/BTP records.
use super::{records::*, retire_json::*, *};
use crate::go_mailbox_v1::{
    admission::{
        self, AdmissionError, ConnectionTarget, RecordSeed, WireRecord, historical, ignored, scalar,
    },
    durable_journal::WindowOwner,
    observations::ChannelError,
};
use lycheedev_core::{
    metadata::{
        HistoricalMutation, HistoricalWriter,
        historical_write::{finish_historical, historical_lease},
    },
    storage,
};
use serde::de::{DeserializeSeed, MapAccess};
use std::path::PathBuf;

fn transition() -> ToolkitError {
    ToolkitError::new(
        "journal.invalid_transition",
        "journal.invalid_transition",
        3,
    )
}
fn busy() -> ToolkitError {
    ToolkitError::new("journal.resource_busy", "journal.resource_busy", 3)
}
fn channel(error: ChannelError) -> ToolkitError {
    match error {
        ChannelError::Toolkit {
            code,
            message,
            exit_code,
            details,
            ..
        } => ToolkitError {
            code,
            message,
            exit_code,
            details,
        },
        ChannelError::Cancelled => ToolkitError::new("command.cancelled", "context canceled", 7),
        ChannelError::DeadlineExceeded => {
            ToolkitError::new("command.deadline", "context deadline exceeded", 5)
        }
        ChannelError::Contract(code) if code.0 == "journal.resource_busy" => busy(),
        ChannelError::Contract(code) if code.0 == "journal.invalid_transition" => transition(),
        ChannelError::Joined { causes } => {
            join_errors(causes.into_iter().map(channel)).expect("nonempty channel error tree")
        }
        other => fault(&other.to_string()),
    }
}
fn admission_error(error: AdmissionError) -> ToolkitError {
    match error {
        AdmissionError::Channel(error) => channel(error),
        AdmissionError::Joined(errors) => join_errors(errors.into_iter().map(admission_error))
            .expect("nonempty admission error tree"),
        other => ToolkitError::new("journal.resource_busy", other.to_string(), 3),
    }
}
fn join_errors(errors: impl IntoIterator<Item = ToolkitError>) -> Option<ToolkitError> {
    let mut result: Result<()> = Ok(());
    for error in errors {
        result = finish_historical(result, Err(error));
    }
    result.err()
}
fn now() -> Timestamp {
    parse_timestamp(&storage::utc_now()).expect("UTC clock produces a Go timestamp")
}
fn inspect_work(
    writer: &HistoricalWriter<'_>,
    id: &str,
    record: &mut WorkRecord,
    control: &OperationControl,
) -> Result<()> {
    *record = WorkRecord::default();
    let document = writer.document(&format!("work/{id}"))?;
    super::retire_decode::work(&document.bytes, record, control)?;
    if record.schema != "lycheedev.work.v1"
        || record.operation_id != id
        || record.generation != document.generation
    {
        return Err(fault("journal: corrupt work record"));
    }
    Ok(())
}
#[derive(Default)]
struct Installation {
    installation: String,
    load: Option<ProbeLoadIntent>,
    target: ConnectionTarget,
}
impl WireRecord for Installation {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "installation" => scalar(&mut self.installation, map),
            "load" => map.next_value_seed(PointerSeed(&mut self.load)),
            "target" => map.next_value_seed(RecordSeed(&mut self.target)),
            _ => ignored(map),
        }
    }
}
fn installation(record: &WorkRecord, control: &OperationControl) -> Result<PathBuf> {
    let input: Installation = decode_record(&record.intent.request.0, control)?;
    let mut directory = input.installation;
    if let Some(load) = input.load {
        directory = load.installation;
    }
    if !input.target.client.directory.is_empty() {
        directory = input.target.client.directory;
    }
    let path = Path::new(&directory);
    if !path.is_absolute() {
        return Err(fault("live.legacy_installation_missing"));
    }
    Ok(path.join("Interface").join("AddOns"))
}
fn owner(workspace: &str, record: &WorkRecord, control: &OperationControl) -> Result<WindowOwner> {
    Ok(WindowOwner {
        schema: "lycheedev.window-owner.v1".into(),
        workspace_id: workspace.into(),
        resource: record.intent.resource.clone(),
        operation_id: record.operation_id.clone(),
        intent_sha256: storage::sha256(&intent_bytes(&record.intent, true, control)?),
    })
}
fn ordinary_spelling(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).into()
    }
}
fn equal_fold(a: &str, b: &str) -> bool {
    // Both inputs are decoded Go JSON strings / canonical paths, so valid Rust
    // scalars encode losslessly to UTF-16. Reuse the pinned Go SimpleFold table.
    lycheedev_windows::mailbox_input::equal_fold_utf16(
        &a.encode_utf16().collect::<Vec<_>>(),
        &b.encode_utf16().collect::<Vec<_>>(),
    )
}

fn check_run(
    run: &historical::HistoricalLease,
    writer: &HistoricalWriter<'_>,
    id: &str,
    record: &mut WorkRecord,
    control: &OperationControl,
) -> Result<()> {
    let want = run.check(control).map_err(admission_error)?;
    inspect_work(writer, id, record, control)?;
    if record.intent.resource != want.resource {
        return Err(fault("journal.window_resource_mismatch"));
    }
    if !want.intent_sha256.is_empty()
        && want.intent_sha256 != storage::sha256(&intent_bytes(&record.intent, true, control)?)
    {
        return Err(fault("journal.window_intent_changed"));
    }
    if record.stage == "cleaned"
        || matches!(
            record.status.as_str(),
            "completed" | "cancelled" | "abandoned"
        )
    {
        return Err(transition());
    }
    Ok(())
}
#[derive(Default)]
struct Ownership {
    operation_id: String,
}
impl WireRecord for Ownership {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "operationid" => scalar(&mut self.operation_id, map),
            _ => ignored(map),
        }
    }
}
fn allows_abandon(kind: &str, stage: &str) -> bool {
    match kind {
        "probe" | "faults" => matches!(
            stage,
            "prepared"
                | "load_requested"
                | "loaded"
                | "dispatch_requested"
                | "reported"
                | "flush_requested"
                | "persisted"
                | "verified"
                | "ack_requested"
                | "acknowledged"
        ),
        "reload" => matches!(stage, "prepared" | "reload_requested"),
        _ => false,
    }
}
fn advance(
    writer: &HistoricalWriter<'_>,
    id: &str,
    record: &mut WorkRecord,
    to: &str,
    status: &str,
    control: &OperationControl,
) -> Result<()> {
    let expected_generation = record.generation;
    let expected_stage = record.stage.clone();
    let mut stored = WorkRecord::default();
    inspect_work(writer, id, &mut stored, control)?;
    if stored.generation != expected_generation || stored.stage != expected_stage {
        return Err(ToolkitError::new(
            "vault.generation_conflict",
            "vault.generation_conflict",
            3,
        ));
    }
    if matches!(
        stored.status.as_str(),
        "completed" | "cancelled" | "abandoned"
    ) {
        return Err(transition());
    }
    let allowed = match to {
        "cleaned" => stored.stage == "prepared" && status == "cancelled",
        "abandoning" => allows_abandon(&stored.intent.kind, &stored.stage) && status == "running",
        "abandoned" => {
            matches!(stored.intent.kind.as_str(), "probe" | "faults" | "reload")
                && stored.stage == "abandoning"
                && status == "abandoned"
        }
        _ => false,
    };
    if !allowed {
        return Err(transition());
    }
    let observation = if record.observation.0.is_empty() {
        b"null".to_vec()
    } else {
        record.observation.0.clone()
    };
    if !lycheedev_core::raw_json::is_valid(&observation) {
        return Err(fault("journal: invalid observation"));
    }
    stored.stage = to.into();
    stored.status = status.into();
    stored.observation.0 = observation;
    stored.generation = stored
        .generation
        .checked_add(1)
        .ok_or_else(|| fault("vault: invalid mutation"))?;
    stored.updated_at = now();
    let key = format!("work/{id}");
    let bytes = work_bytes(&stored, control)?;
    let mut changes = vec![HistoricalMutation {
        key: &key,
        expected_generation,
        bytes: &bytes,
    }];
    let owner_key = format!("ownership/{}", stored.intent.resource);
    let mut owner_doc = None;
    if matches!(to, "cleaned" | "abandoned") {
        owner_doc = Some(writer.document(&owner_key)?);
        let doc = owner_doc.as_ref().expect("read terminal owner");
        let current: Ownership = decode_record(&doc.bytes, control)?;
        if current.operation_id != stored.operation_id {
            return Err(busy());
        }
        changes.push(HistoricalMutation {
            key: &owner_key,
            expected_generation: doc.generation,
            bytes: br#"{"operationId":""}"#,
        });
    }
    writer.commit(&changes)?;
    inspect_work(writer, id, record, control)
}
fn retire_operation_inner(
    home: &Path,
    id: &str,
    cancel_only: bool,
    record: &mut WorkRecord,
    control: &OperationControl,
) -> Result<()> {
    Metadata::with_historical_write(home, control, |writer| {
        let operation = historical_lease(home, &format!("operation:{id}"), control)?;
        let result = (|| {
            inspect_work(writer, id, record, control)?;
            if !matches!(record.intent.kind.as_str(), "probe" | "faults" | "reload") {
                return Err(transition());
            }
            let parent = installation(record, control)?;
            let workspace = lycheedev_core::workspace::open(home)?;
            let workspace = workspace["workspaceId"]
                .as_str()
                .ok_or_else(|| fault("vault.unsupported_workspace"))?;
            let want = owner(workspace, record, control)?;
            if matches!(record.stage.as_str(), "abandoned" | "cleaned") {
                return retire_terminal(writer, id, record, &parent, &want, control);
            }
            let mut run =
                historical::acquire_operation(&parent, want.clone(), control, |canonical_parent| {
                    let admission = record.intent.admission.as_ref();
                    if record.stage != "prepared"
                        || record.status != "pending"
                        || admission.is_none_or(|a| {
                            a.workspace_id != workspace
                                || !equal_fold(&a.parent, &ordinary_spelling(canonical_parent))
                        })
                    {
                        return Err(fault("journal.window_admission_unavailable").into());
                    }
                    let mut stored = WorkRecord::default();
                    inspect_work(writer, id, &mut stored, control)?;
                    if stored.generation != record.generation
                        || intent_bytes(&stored.intent, true, control)?
                            != intent_bytes(&record.intent, true, control)?
                    {
                        return Err(ToolkitError::new(
                            "vault.generation_conflict",
                            "vault.generation_conflict",
                            3,
                        )
                        .into());
                    }
                    Ok(())
                })
                .map_err(admission_error)?;
            let result = (|| {
                check_run(&run, writer, id, record, control)?;
                let unstarted = record.stage == "prepared"
                    && record.status == "pending"
                    && (record.observation.0.is_empty() || record.observation.0 == b"null");
                if cancel_only && !unstarted {
                    return Err(transition());
                }
                if cancel_only {
                    advance(writer, id, record, "cleaned", "cancelled", control)?;
                } else {
                    if record.stage != "abandoning" {
                        advance(writer, id, record, "abandoning", "running", control)?;
                    }
                    advance(writer, id, record, "abandoned", "abandoned", control)?;
                }
                Ok(())
            })();
            finish_historical(result, run.close().map_err(channel))?;
            retire_terminal(writer, id, record, &parent, &want, control)
        })();
        finish_historical(result, operation.close())
    })
}
fn retire_terminal(
    writer: &HistoricalWriter<'_>,
    id: &str,
    record: &mut WorkRecord,
    parent: &Path,
    want: &WindowOwner,
    control: &OperationControl,
) -> Result<()> {
    inspect_work(writer, id, record, control)?;
    if !(record.stage == "cleaned" && matches!(record.status.as_str(), "completed" | "cancelled")
        || record.stage == "abandoned" && record.status == "abandoned")
    {
        return Err(transition());
    }
    // The re-read durable terminal record determines the exact resource to retire.
    let want = WindowOwner {
        resource: record.intent.resource.clone(),
        ..want.clone()
    };
    historical::retire(parent, &want, false, control).map_err(admission_error)
}
pub fn retire_operation(
    home: &Path,
    id: &str,
    cancel_only: bool,
    control: &OperationControl,
) -> HistoricalRead {
    let mut record = WorkRecord::default();
    let error = retire_operation_inner(home, id, cancel_only, &mut record, control).err();
    if record.operation_id.is_empty() {
        return HistoricalRead {
            outcome: HistoricalOutcome::default(),
            error,
        };
    }
    let mut read = super::inspect_operation(home, id, control);
    if read.outcome.operation_id.is_empty() {
        read.outcome = super::status::pending(&record);
    }
    read.error = join_errors(error.into_iter().chain(read.error));
    read
}

#[derive(Debug)]
pub struct BootstrapRetirement {
    pub attempt: BootstrapAttempt,
    pub error: Option<ToolkitError>,
}
fn bootstrap_owner(
    workspace: &str,
    attempt: &BootstrapAttempt,
    control: &OperationControl,
) -> Result<WindowOwner> {
    #[derive(Serialize)]
    struct Identity<'a> {
        #[serde(rename = "ID")]
        id: &'a str,
        #[serde(rename = "Target")]
        target: &'a ConnectionTarget,
        #[serde(rename = "Action")]
        action: &'a str,
        #[serde(rename = "Request")]
        request: &'a str,
        #[serde(rename = "Arg")]
        arg: &'a str,
    }
    let raw = serde_json::to_vec(&Identity {
        id: &attempt.id,
        target: &attempt.target,
        action: &attempt.action,
        request: &attempt.request_id,
        arg: &attempt.arg,
    })?;
    Ok(WindowOwner {
        schema: "lycheedev.window-owner.v1".into(),
        workspace_id: workspace.into(),
        resource: attempt.target.resource(),
        operation_id: attempt.id.clone(),
        intent_sha256: storage::sha256(&compact(&raw, control)?),
    })
}
#[derive(Default)]
struct Current {
    id: String,
}
impl WireRecord for Current {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "id" => scalar(&mut self.id, map),
            _ => ignored(map),
        }
    }
}
fn merge_current(raw: &[u8], current: &mut Current, control: &OperationControl) -> Result<()> {
    control.check()?;
    if !lycheedev_core::raw_json::is_valid(raw) {
        return Err(fault("invalid historical JSON"));
    }
    let normalized = lycheedev_core::evidence::normalize_go_json_strings(raw, control)?;
    let mut decoder = serde_json::Deserializer::from_slice(&normalized);
    RecordSeed(current).deserialize(&mut decoder)?;
    decoder.end()?;
    control.check()
}
pub fn abandon_bootstrap(home: &Path, id: &str, control: &OperationControl) -> BootstrapRetirement {
    let mut prior = BootstrapAttempt::default();
    let inspected =
        Metadata::read_raw_controlled(home, &format!("bootstrap/attempt/{id}"), control)
            .map_err(decode_error)
            .and_then(|raw| super::retire_decode::attempt(&raw, id, &mut prior, control));
    if let Err(error) = inspected {
        return BootstrapRetirement {
            attempt: prior,
            error: Some(error),
        };
    }
    let mut attempt = prior.clone();
    let mut driver = None;
    let result = (|| {
        if prior.claimed && prior.phase != "abandoned" {
            let workspace = lycheedev_core::workspace::open(home)?;
            let workspace = workspace["workspaceId"]
                .as_str()
                .ok_or_else(|| fault("vault.unsupported_workspace"))?;
            let inspect = admission::inspect_window_owner(
                &prior.target.addon_parent(),
                &prior.target.resource(),
                control,
            );
            if let Some(error) = inspect.error {
                return Err(channel(error));
            }
            if inspect.occupied {
                driver = Some(
                    historical::lock_bootstrap(
                        &prior.target.addon_parent(),
                        &bootstrap_owner(workspace, &prior, control)?,
                        control,
                    )
                    .map_err(admission_error)?,
                );
            } else if prior.phase != "prepared" {
                return Err(fault("live.bootstrap_claim_missing"));
            }
        }
        attempt = BootstrapAttempt::default();
        Metadata::with_historical_write(home, control, |writer| {
            let key = format!("bootstrap/attempt/{id}");
            let doc = writer.document(&key)?;
            attempt = BootstrapAttempt::default();
            super::retire_decode::attempt(&doc.bytes, id, &mut attempt, control)?;
            if attempt.phase == "abandoned" {
                return Ok(());
            }
            if attempt.phase == "confirmed" {
                return Err(fault("live.bootstrap_already_confirmed"));
            }
            let resource = attempt.target.resource();
            let current_key = format!(
                "bootstrap/current/{}",
                storage::sha256(format!("{resource}\0{}", attempt.action).as_bytes())
            );
            let window_key = format!("bootstrap/window/{}", storage::sha256(resource.as_bytes()));
            let current_doc = writer.document(&current_key)?;
            let mut current: Current = domain_decode(
                decode_record(&current_doc.bytes, control),
                "live.bootstrap_current_mismatch",
                control,
            )?;
            if current.id != id {
                return Err(fault("live.bootstrap_current_mismatch"));
            }
            let window_doc = writer.document(&window_key)?;
            domain_decode(
                merge_current(&window_doc.bytes, &mut current, control),
                "live.bootstrap_window_mismatch",
                control,
            )?;
            if current.id != id {
                return Err(fault("live.bootstrap_window_mismatch"));
            }
            attempt.phase = "abandoned".into();
            attempt.updated_at = now();
            let bytes = bootstrap_bytes(&doc.bytes, &attempt.updated_at, control)?;
            writer.commit(&[HistoricalMutation {
                key: &key,
                expected_generation: doc.generation,
                bytes: &bytes,
            }])
        })
    })();
    let close = driver
        .as_mut()
        .map_or(Ok(()), |driver| driver.close().map_err(channel));
    let result = finish_historical(result, close).and_then(|()| {
        if !attempt.claimed {
            return Ok(());
        }
        if !matches!(attempt.phase.as_str(), "confirmed" | "abandoned") {
            return Err(transition());
        }
        let workspace = lycheedev_core::workspace::open(home)?;
        let workspace = workspace["workspaceId"]
            .as_str()
            .ok_or_else(|| fault("vault.unsupported_workspace"))?;
        historical::retire(
            &attempt.target.addon_parent(),
            &bootstrap_owner(workspace, &attempt, control)?,
            true,
            control,
        )
        .map_err(admission_error)
    });
    BootstrapRetirement {
        attempt,
        error: result.err(),
    }
}
#[cfg(test)]
mod tests;
