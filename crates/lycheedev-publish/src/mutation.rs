//! Durable state for mutable author requests. A pending request is read back;
//! it never licenses a second POST. Completed mutable values are read afresh.
use crate::provider::Provider;
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum State {
    IntentRecorded,
    NotSent,
    Acknowledged,
    Verified,
    Rejected,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    operation_id: String,
    provider: Provider,
    resource: String,
    target: u64,
    action: String,
    input_sha256: String,
    state: State,
    created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    evidence: Option<Value>,
}
pub(crate) struct Mutation {
    root: PathBuf,
    provider: Provider,
    resource: String,
    target: u64,
    action: String,
    input: String,
    current: Option<Record>,
    _lock: storage::ExclusiveLock,
}
impl Mutation {
    pub(crate) fn open(
        home: &Path,
        provider: Provider,
        resource: &str,
        target: u64,
        action: &str,
        input: &Value,
        control: &OperationControl,
    ) -> Result<Self> {
        control.check()?;
        if target == 0
            || resource.is_empty()
            || !resource
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'-')
        {
            return Err(ToolkitError::new(
                "publish.mutation_scope",
                "invalid author mutation scope",
                8,
            ));
        }
        let root = home
            .join("publish")
            .join(provider.id())
            .join("records")
            .join(resource)
            .join(target.to_string());
        let lock = storage::ExclusiveLock::resource_controlled(&root, "mutation", control)?;
        let input = storage::sha256(&storage::identity_json(input)?);
        let current = match fs::read(root.join("active.json")) {
            Ok(bytes) => {
                let active: Record = serde_json::from_slice(&bytes).map_err(|_| {
                    ToolkitError::invalid("invalid original author mutation record")
                })?;
                if active.schema != "lycheedev.publish.mutation.v1"
                    || active.provider != provider
                    || active.resource != resource
                    || active.target != target
                    || active.operation_id.len() != 64
                    || !active
                        .operation_id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(ToolkitError::invalid(
                        "author mutation record differs from its original target",
                    ));
                }
                Some(active)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let mutation = Self {
            root,
            provider,
            resource: resource.into(),
            target,
            action: action.into(),
            input,
            current,
            _lock: lock,
        };
        if mutation.pending()
            && mutation.current.as_ref().is_some_and(|record| {
                record.action != mutation.action || record.input_sha256 != mutation.input
            })
        {
            return Err(mutation
                .pending_error("retain the original author request while its result is pending"));
        }
        Ok(mutation)
    }
    pub(crate) fn pending(&self) -> bool {
        self.current.as_ref().is_some_and(|record| {
            matches!(record.state, State::IntentRecorded | State::Acknowledged)
        })
    }
    pub(crate) fn id(&self) -> Option<&str> {
        self.current
            .as_ref()
            .map(|record| record.operation_id.as_str())
    }
    pub(crate) fn prepare(&mut self, control: &OperationControl) -> Result<()> {
        self.prepare_record(None, control)
    }
    pub(crate) fn prepare_with_evidence(
        &mut self,
        evidence: Value,
        control: &OperationControl,
    ) -> Result<()> {
        self.prepare_record(Some(evidence), control)
    }
    fn prepare_record(
        &mut self,
        evidence: Option<Value>,
        control: &OperationControl,
    ) -> Result<()> {
        control.check()?;
        if self.pending() {
            return Err(
                self.pending_error("the original pending author write cannot be sent again")
            );
        }
        let random = lycheedev_windows::random::token().map_err(|_| {
            ToolkitError::new(
                "publish.operation_id",
                "cannot create author operation identity",
                3,
            )
        })?;
        self.current = Some(Record {
            schema: "lycheedev.publish.mutation.v1".into(),
            operation_id: storage::sha256(random.as_bytes()),
            provider: self.provider,
            resource: self.resource.clone(),
            target: self.target,
            action: self.action.clone(),
            input_sha256: self.input.clone(),
            state: State::IntentRecorded,
            created_at: storage::utc_now(),
            evidence,
        });
        self.save()
    }
    fn save(&self) -> Result<()> {
        let record = self.current.as_ref().ok_or_else(|| {
            ToolkitError::new(
                "publish.mutation_record",
                "author intent has not been recorded",
                8,
            )
        })?;
        storage::atomic_json(&self.root.join("active.json"), record)
    }
    pub(crate) fn acknowledged(&mut self) -> Result<()> {
        self.set_state(State::Acknowledged)
    }
    pub(crate) fn verified(&mut self) -> Result<()> {
        self.set_state(State::Verified)
    }
    pub(crate) fn evidence(&self) -> Option<&Value> {
        self.current
            .as_ref()
            .filter(|record| record.input_sha256 == self.input)
            .and_then(|record| record.evidence.as_ref())
    }
    pub(crate) fn verified_with(&mut self, evidence: Value) -> Result<()> {
        self.current
            .as_mut()
            .ok_or_else(|| {
                ToolkitError::new(
                    "publish.mutation_record",
                    "author intent has not been recorded",
                    8,
                )
            })?
            .evidence = Some(evidence);
        self.verified()
    }
    pub(crate) fn remember(&mut self, evidence: Value) -> Result<()> {
        self.current
            .as_mut()
            .ok_or_else(|| {
                ToolkitError::new(
                    "publish.mutation_record",
                    "author intent has not been recorded",
                    8,
                )
            })?
            .evidence = Some(evidence);
        self.save()
    }
    fn set_state(&mut self, state: State) -> Result<()> {
        let record = self.current.as_mut().ok_or_else(|| {
            ToolkitError::new(
                "publish.mutation_record",
                "author intent has not been recorded",
                8,
            )
        })?;
        record.state = state;
        self.save()
    }
    pub(crate) fn failure(&mut self, mut error: ToolkitError) -> ToolkitError {
        let known_rejection = definitive_rejection(&error);
        let not_sent = error.details["submission"] == "not_sent";
        let record_pending = if not_sent {
            self.set_state(State::NotSent).is_err()
        } else {
            known_rejection && self.set_state(State::Rejected).is_err()
        };
        error.details = json!({"operation_id":self.id(),"provider":self.provider,"resource":self.resource,"target":self.target,
            "submission":if not_sent {"not_sent"}else if known_rejection {"rejected"}else{"uncertain"},"replay_allowed":false,"receipt_pending":record_pending,"cause":error.details});
        error
    }
    pub(crate) fn pending_error(&self, message: &str) -> ToolkitError {
        ToolkitError::new("publish.mutation_pending",message,6).with_details(json!({"operation_id":self.id(),"provider":self.provider,"resource":self.resource,
            "target":self.target,"action":self.current.as_ref().map(|record|&record.action),"submission":"pending","replay_allowed":false}))
    }
    pub(crate) fn readback_error(&self, message: &str, cause: ToolkitError) -> ToolkitError {
        let mut error = self.pending_error(message);
        if matches!(
            cause.code.as_str(),
            "command.cancelled" | "command.deadline"
        ) {
            error.code = cause.code.clone();
            error.message = cause.message.clone();
            error.exit_code = cause.exit_code;
        }
        error.details["cause"] = json!(cause);
        error
    }
}

pub(crate) fn digest(value: &Value) -> Result<String> {
    Ok(storage::sha256(&storage::identity_json(value)?))
}
pub(crate) fn fingerprints(value: &Value) -> Result<Value> {
    value
        .as_object()
        .ok_or_else(|| ToolkitError::invalid("expected author readback fields must be an object"))?
        .iter()
        .map(|(k, v)| Ok((k.clone(), json!(digest(v)?))))
        .collect::<Result<serde_json::Map<_, _>>>()
        .map(Value::Object)
}
pub(crate) fn matches_fingerprints(expected: &Value, actual: &Value) -> Result<bool> {
    for (key, hash) in expected
        .as_object()
        .ok_or_else(|| ToolkitError::invalid("original author readback digest is invalid"))?
    {
        if actual.get(key).is_none() || json!(digest(&actual[key])?) != *hash {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn completed(mutation: &Mutation) -> Option<Value> {
    mutation
        .evidence()
        .and_then(|e| e.get("result"))
        .map(|result| {
            let mut result = result.clone();
            result["reused"] = json!(true);
            result
        })
}

pub(crate) fn definitive_rejection(error: &ToolkitError) -> bool {
    error.code == "publish.newbee_business"
        || error.code == "publish.ncc_business"
        || (error.code == "publish.modus_rejected" && error.details["submission"] == "rejected")
        || (error.code == "publish.dd_rejected" && error.details["submission"] == "rejected")
        || (matches!(
            error.code.as_str(),
            "publish.blackbox_rejected" | "publish.blackbox_login_required"
        ) && error.details["submission"] == "rejected")
        || (error.code == "publish.http_status"
            && error.details["http_status"].as_u64().is_some_and(|status| {
                matches!(status, 400 | 401 | 403 | 404 | 413 | 415 | 422 | 429)
            }))
}
