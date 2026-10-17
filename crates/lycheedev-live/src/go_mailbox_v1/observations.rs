//! Fixed-Go observation/backend contracts. No native adapter or input is opened.
//! Observation variants retain Go's receipt/error and pending/diagnostic pairs.
use super::state::{Blocker, InputKind, SlotEnvelope};
use super::{
    BodyQualification, ContractError, Identity, InputDiagnostic, InputObservation, InputOutcome,
    Kind, MAX_PAYLOAD, Receipt, ReceiptMatch, SCHEMA, Selector, ZERO_TOKEN, checksum,
    decode_publication_receipt, decode_receipt, parse_token,
};
use crate::mailbox::Record;
use lycheedev_core::control::OperationControl;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    Contract(ContractError),
    /// Additional pending causes retain clock/limit/scheduling distinctions.
    Pending {
        causes: Vec<&'static str>,
    },
    Blocked(Blocker),
    /// Publish proved the original slot belongs elsewhere before any write.
    SlotAvailable {
        /// Proposed destination; blocker.slot remains the occupied old slot.
        index: i32,
        blocker: Blocker,
    },
    Cancelled,
    DeadlineExceeded,
    /// Like Go errors.Join: preserve every independent typed cause and payload.
    Joined {
        causes: Vec<ChannelError>,
    },
    Rejected {
        reason: String,
    },
    /// Structured application failure. Preserve original exit/details through
    /// native/publication adapters; public channel mapping still takes priority.
    Toolkit {
        code: String,
        message: String,
        exit_code: i32,
        details: serde_json::Value,
        /// Recognized cleanup aggregates expose their actual typed leaves.
        /// The outer original error/details remain available to presentation.
        causes: Vec<ChannelError>,
    },
    /// Preserve independent cancellation, identity, transport and I/O causes.
    External {
        causes: Vec<String>,
    },
}
pub type ChannelResult<T> = std::result::Result<T, ChannelError>;
impl From<ContractError> for ChannelError {
    fn from(e: ContractError) -> Self {
        Self::Contract(e)
    }
}
impl From<lycheedev_core::ToolkitError> for ChannelError {
    fn from(error: lycheedev_core::ToolkitError) -> Self {
        Self::from_toolkit(error, 0)
    }
}
impl ChannelError {
    fn from_toolkit(error: lycheedev_core::ToolkitError, depth: usize) -> Self {
        if error.details.is_null() {
            match error.code.as_str() {
                "command.cancelled" => return Self::Cancelled,
                "command.deadline" => return Self::DeadlineExceeded,
                _ => (),
            }
        }
        // Only the core-owned, complete schema authorizes cause expansion.
        // Malformed, opaque or excessive-depth details remain a non-pure leaf;
        // they must not be mistaken for a suppressible optional timeout.
        let unpack = || -> Option<Vec<Self>> {
            let object = error.details.as_object()?;
            if depth >= 32
                || object.len() != 4
                || object.get("schema")?.as_str()? != lycheedev_core::storage::FILE_CLEANUP_SCHEMA
            {
                return None;
            }
            let mut causes = Vec::new();
            for key in ["primary", "cleanup", "control"] {
                let value = object.get(key)?;
                if !value.is_null() {
                    let nested: lycheedev_core::ToolkitError =
                        serde_json::from_value(value.clone()).ok()?;
                    causes.push(Self::from_toolkit(nested, depth + 1));
                }
            }
            (causes.len() >= 2).then_some(causes)
        };
        let causes = unpack().unwrap_or_default();
        Self::Toolkit {
            code: error.code,
            message: error.message,
            exit_code: error.exit_code,
            details: error.details,
            causes,
        }
    }
}
impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract(e) => std::fmt::Display::fmt(e, f),
            Self::Pending { causes } => {
                f.write_str("live.channel_pending")?;
                for c in causes {
                    write!(f, "\n{c}")?;
                }
                Ok(())
            }
            Self::Blocked(b) => write!(f, "live.channel_blocked: {}", b.kind),
            Self::SlotAvailable { blocker, .. } => {
                write!(f, "live.channel_blocked: {}", blocker.kind)
            }
            Self::Cancelled => f.write_str("command.cancelled"),
            Self::DeadlineExceeded => f.write_str("command.deadline"),
            Self::Joined { causes } => {
                for (index, cause) in causes.iter().enumerate() {
                    if index != 0 {
                        f.write_str("\n")?;
                    }
                    std::fmt::Display::fmt(cause, f)?;
                }
                Ok(())
            }
            Self::Rejected { reason } => write!(f, "live.channel_rejected: {reason}"),
            Self::Toolkit {
                code,
                message,
                causes,
                ..
            } => {
                let first = format!("{code}: {message}");
                f.write_str(&first)?;
                for cause in causes {
                    if cause.to_string() != first {
                        write!(f, "\n{cause}")?;
                    }
                }
                Ok(())
            }
            Self::External { causes } => f.write_str(&causes.join("\n")),
        }
    }
}
impl std::error::Error for ChannelError {}
impl ChannelError {
    // Fixed Go BlockedError.Unwrap, including the independent publication
    // cause. SlotAvailable wraps Blocked; its destination is not error text.
    fn blocker_cause(blocker: &Blocker) -> Self {
        let pending = Self::Pending { causes: vec![] };
        match blocker.kind.as_str() {
            "slot_reservation" | "slot_pool_full" | "publication_lock" => Self::Joined {
                causes: vec![
                    pending,
                    Self::Contract(ContractError("live.channel_shared_publication_pending")),
                ],
            },
            _ => pending,
        }
    }
    /// Search the error and all unwrapped causes, analogous to Go errors.Is/As.
    /// Matching a cause never implies other joined causes may be discarded.
    pub fn any_cause(&self, predicate: &impl Fn(&Self) -> bool) -> bool {
        if predicate(self) {
            return true;
        }
        match self {
            Self::Joined { causes } => causes.iter().any(|c| c.any_cause(predicate)),
            Self::Toolkit { code, causes, .. } => {
                let control = match code.as_str() {
                    "command.cancelled" => Some(Self::Cancelled),
                    "command.deadline" => Some(Self::DeadlineExceeded),
                    _ => None,
                };
                control.as_ref().is_some_and(predicate)
                    || causes.iter().any(|c| c.any_cause(predicate))
            }
            Self::SlotAvailable { blocker, .. } => {
                Self::Blocked(blocker.clone()).any_cause(predicate)
            }
            Self::Blocked(blocker) => Self::blocker_cause(blocker).any_cause(predicate),
            _ => false,
        }
    }
    /// Inspect terminal unwrapped causes, not wrapper labels. Empty joins are
    /// not a valid reason to suppress an error.
    pub fn all_causes(&self, predicate: &impl Fn(&Self) -> bool) -> bool {
        match self {
            Self::Joined { causes } => {
                !causes.is_empty() && causes.iter().all(|c| c.all_causes(predicate))
            }
            Self::SlotAvailable { blocker, .. } | Self::Blocked(blocker) => {
                Self::blocker_cause(blocker).all_causes(predicate)
            }
            Self::Toolkit { causes, .. } if !causes.is_empty() => {
                causes.iter().all(|c| c.all_causes(predicate))
            }
            leaf => predicate(leaf),
        }
    }
    pub fn is_pending(&self) -> bool {
        self.any_cause(&|c| matches!(c, Self::Pending { .. }))
    }
    pub fn is_blocked(&self) -> bool {
        self.any_cause(&|c| matches!(c, Self::Blocked(_)))
    }
    pub fn slot_available_blocker(&self) -> Option<&Blocker> {
        match self {
            Self::SlotAvailable { blocker, .. } => Some(blocker),
            Self::Joined { causes } => causes.iter().find_map(Self::slot_available_blocker),
            _ => None,
        }
    }
    /// Fixed Go runtime_retirement.go optionalObservationTimeout. Only pure
    /// pending/deadline leaves qualify; extra pending annotations are unknown
    /// causes, so they conservatively prevent suppression.
    pub fn optional_observation_timeout(&self) -> bool {
        self.all_causes(&|c| {
            matches!(c, Self::DeadlineExceeded)
                || matches!(c, Self::Pending { causes } if causes.is_empty())
        })
    }
}
pub fn check_control(control: &OperationControl) -> ChannelResult<()> {
    control.check().map_err(Into::into)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationKind {
    Receipt,
    Result,
}
#[derive(Debug, Clone)]
pub struct ObservationQuery {
    pub kind: ObservationKind,
    pub envelope: SlotEnvelope,
    pub identity: Identity,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    Receipt(Receipt),
    Reported {
        receipt: Receipt,
        payload: Vec<u8>,
    },
    /// A diagnostic describes an input callback, never receipt or execution.
    Pending {
        diagnostic: Option<InputDiagnostic>,
    },
    Rejected {
        receipt: Receipt,
    },
}
/// Observation facts and errors are independent, as in Go's (Observation,error).
/// A receipt or diagnostic does not override cancellation or transport failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationDispatch {
    pub observation: Option<Observation>,
    pub error: Option<ChannelError>,
}
impl From<ChannelResult<Observation>> for ObservationDispatch {
    fn from(result: ChannelResult<Observation>) -> Self {
        match result {
            Ok(observation) => Self {
                observation: Some(observation),
                error: None,
            },
            Err(error) => Self {
                observation: None,
                error: Some(error),
            },
        }
    }
}
#[derive(Debug, Clone)]
pub struct InputAction {
    pub kind: InputKind,
    pub envelope: SlotEnvelope,
    pub observation: Option<InputObservation>,
    pub capability: String,
    pub after_millis: i64,
}
#[derive(Debug, Clone)]
pub struct InputDispatch {
    pub outcome: InputOutcome,
    /// An error does not erase queued-message facts. Caller persists both the
    /// intent and observed outcome even when cancellation stops further work.
    pub error: Option<ChannelError>,
}
/// Drivers must persist intent before calling side-effecting methods. Implementors
/// must keep the selected target and current publication/physical-input gates.
/// These signatures alone do not confer user authorization or a native handle.
pub trait Backend {
    fn observe(
        &mut self,
        control: &OperationControl,
        query: &ObservationQuery,
    ) -> ObservationDispatch;
    fn publish(&mut self, control: &OperationControl, envelope: &SlotEnvelope)
    -> ChannelResult<()>;
    fn observe_input(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        after_millis: i64,
        capability: &str,
    ) -> ChannelResult<InputObservation>;
    fn input(&mut self, control: &OperationControl, action: &InputAction) -> InputDispatch;
    fn runtime_candidate(
        &mut self,
        control: &OperationControl,
        identity: &Identity,
    ) -> ChannelResult<Option<Identity>>;
    fn supersede(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        replacement: &Identity,
    ) -> ChannelResult<()>;
    fn consumed(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<()>;
}
/// A reader applies Accept only after path/identity guards, then checks control
/// again. Simulated readers are requalified below; no address becomes authority.
pub trait RecordReader {
    fn find(
        &mut self,
        control: &OperationControl,
        selector: &Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        eager: bool,
    ) -> ChannelResult<Vec<Record>>;
}
fn invalid(code: &'static str) -> ChannelError {
    ContractError(code).into()
}

/// Same shared validator as Go channel.ObserveRecords, for real and fake sources.
pub fn observe_records(
    control: &OperationControl,
    reader: &mut dyn RecordReader,
    q: &ObservationQuery,
) -> ChannelResult<Observation> {
    check_control(control)?;
    let mut expected = q.envelope.expected();
    if expected.schema.is_empty() {
        expected.schema = SCHEMA.into();
    }
    let nonce = parse_token(&expected.nonce).unwrap_or(ZERO_TOKEN);
    let runtime = parse_token(&expected.runtime).unwrap_or(ZERO_TOKEN);
    let ticket = parse_token(&expected.ticket).unwrap_or(ZERO_TOKEN);
    let mut selector = Selector {
        kind: Kind::Receipt,
        nonce,
        runtime,
        ticket,
        body: None,
    };
    let mut accept = |record: &Record| -> bool {
        record
            .contract()
            .ok()
            .and_then(|r| decode_publication_receipt(&r).ok())
            .is_some_and(|r| {
                r.identity.schema == expected.schema
                    && r.state == "reported"
                    && r.action == "prepare"
                    && r.nonce == expected.nonce
                    && r.ticket == expected.ticket
                    && r.identity.runtime == expected.runtime
                    && r.identity.owner == expected.owner
                    && r.identity.fence == expected.fence
                    && r.identity.guid == expected.guid
                    && r.identity.build == expected.build
                    && r.identity.product == q.identity.product
                    && r.identity.release == q.identity.release
            })
    };
    let records = match q.kind {
        ObservationKind::Receipt => {
            if expected.action == "confirm" {
                selector.kind = Kind::Confirmation;
            }
            reader.find(control, &selector, None, true)?
        }
        ObservationKind::Result => reader.find(control, &selector, Some(&mut accept), true)?,
    };
    check_control(control)?;
    let Some(record) = records.first() else {
        return Ok(Observation::Pending { diagnostic: None });
    };
    if q.kind == ObservationKind::Receipt {
        let borrowed = record
            .contract()
            .map_err(|_| invalid("live.channel_receipt_record_invalid"))?;
        let matched = decode_receipt(&borrowed, &expected)?;
        let (receipt, rejected) = match matched {
            ReceiptMatch::Accepted(r) => (r, false),
            ReceiptMatch::Rejected(r) => (r, true),
        };
        if receipt.identity.product != q.identity.product
            || receipt.identity.release != q.identity.release
        {
            return Err(invalid("live.channel_receipt_target_mismatch"));
        }
        return Ok(if rejected {
            Observation::Rejected { receipt }
        } else {
            Observation::Receipt(receipt)
        });
    }
    if !accept(record) {
        return Err(invalid("live.channel_receipt_mismatch"));
    }
    let borrowed = record
        .contract()
        .map_err(|_| invalid("live.channel_receipt_record_invalid"))?;
    let receipt = decode_publication_receipt(&borrowed)?;
    if receipt.report_bytes == 0 || receipt.report_bytes as usize > MAX_PAYLOAD {
        return Err(invalid("live.channel_report_length"));
    }
    let body_selector = Selector {
        kind: Kind::Body,
        nonce,
        runtime,
        ticket,
        body: Some(BodyQualification {
            length: receipt.report_bytes,
            checksum: receipt.report_checksum,
        }),
    };
    let body = reader.find(control, &body_selector, None, true)?;
    check_control(control)?;
    let Some(record) = body.into_iter().next() else {
        return Ok(Observation::Pending { diagnostic: None });
    };
    let valid = record
        .contract()
        .is_ok_and(|r| body_selector.matches(&r.header));
    if !valid
        || record.payload.len() != receipt.report_bytes as usize
        || checksum(&record.payload) != receipt.report_checksum
    {
        return Err(invalid("live.channel_report_record_invalid"));
    }
    if !lycheedev_core::raw_json::is_valid(&record.payload) {
        return Err(invalid("live.channel_report_json"));
    }
    Ok(Observation::Reported {
        receipt,
        payload: record.payload,
    })
}

#[cfg(test)]
#[path = "observations_tests.rs"]
mod tests;
