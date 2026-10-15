//! Read-only fixed Go 3.2.0 bootstrap projections. These records never grant input authority.
use super::*;
use crate::go_mailbox_v1::admission::{
    ConnectionTarget, RecordSeed, WireRecord, ignored, scalar, signed_integer,
};
use serde::{
    Deserialize, Serialize,
    de::{DeserializeSeed, MapAccess},
};
fn is_zero<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BootstrapAttempt {
    #[serde(rename = "schema")]
    pub(super) schema: String,
    #[serde(rename = "id")]
    pub(super) id: String,
    #[serde(rename = "target")]
    pub(super) target: ConnectionTarget,
    #[serde(rename = "action")]
    pub(super) action: String,
    #[serde(rename = "requestId")]
    pub(super) request_id: String,
    #[serde(rename = "arg")]
    pub(super) arg: String,
    #[serde(rename = "phase")]
    pub(super) phase: String,
    #[serde(rename = "claimed", skip_serializing_if = "is_zero")]
    pub(super) claimed: bool,
    #[serde(rename = "receiver")]
    pub(super) receiver: ReceiverAttempt,
    #[serde(rename = "history", skip_serializing_if = "History::is_empty")]
    pub(super) history: History,
    #[serde(rename = "bindings")]
    pub(super) bindings: ReceiverBindings,
    #[serde(rename = "actorGuid", skip_serializing_if = "String::is_empty")]
    pub(super) actor_guid: String,
    #[serde(rename = "actorCharacter", skip_serializing_if = "String::is_empty")]
    pub(super) actor_character: String,
    #[serde(rename = "actorRealm", skip_serializing_if = "String::is_empty")]
    pub(super) actor_realm: String,
    #[serde(rename = "priorSessionNonce", skip_serializing_if = "String::is_empty")]
    pub(super) prior_session_nonce: String,
    #[serde(rename = "priorReadyNonce", skip_serializing_if = "String::is_empty")]
    pub(super) prior_ready_nonce: String,
    #[serde(rename = "priorReadyEpoch", skip_serializing_if = "is_zero")]
    pub(super) prior_ready_epoch: u64,
    #[serde(rename = "priorReadySequence", skip_serializing_if = "is_zero")]
    pub(super) prior_ready_sequence: u64,
    #[serde(rename = "business", skip_serializing_if = "Option::is_none")]
    pub(super) business: Option<Signal>,
    #[serde(rename = "createdAt")]
    pub(super) created_at: Timestamp,
    #[serde(rename = "updatedAt")]
    pub(super) updated_at: Timestamp,
}
impl WireRecord for BootstrapAttempt {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "id" => scalar(&mut self.id, map),
            "target" => map.next_value_seed(RecordSeed(&mut self.target)),
            "action" => scalar(&mut self.action, map),
            "requestid" => scalar(&mut self.request_id, map),
            "arg" => scalar(&mut self.arg, map),
            "phase" => scalar(&mut self.phase, map),
            "claimed" => scalar(&mut self.claimed, map),
            "receiver" => map.next_value_seed(RecordSeed(&mut self.receiver)),
            "history" => map.next_value_seed(HistorySeed(&mut self.history)),
            "bindings" => map.next_value_seed(RecordSeed(&mut self.bindings)),
            "actorguid" => scalar(&mut self.actor_guid, map),
            "actorcharacter" => scalar(&mut self.actor_character, map),
            "actorrealm" => scalar(&mut self.actor_realm, map),
            "priorsessionnonce" => scalar(&mut self.prior_session_nonce, map),
            "priorreadynonce" => scalar(&mut self.prior_ready_nonce, map),
            "priorreadyepoch" => scalar(&mut self.prior_ready_epoch, map),
            "priorreadysequence" => scalar(&mut self.prior_ready_sequence, map),
            "business" => map.next_value_seed(OptionalSignalSeed(&mut self.business)),
            "createdat" => scalar(&mut self.created_at, map),
            "updatedat" => scalar(&mut self.updated_at, map),
            _ => ignored(map),
        }
    }
}
impl<'de> Deserialize<'de> for BootstrapAttempt {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        RecordSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReceiverAttempt {
    #[serde(rename = "schema")]
    pub(super) schema: String,
    #[serde(rename = "operationId")]
    pub(super) operation_id: String,
    #[serde(rename = "action")]
    pub(super) action: String,
    #[serde(rename = "requestId")]
    pub(super) request_id: String,
    #[serde(rename = "arg")]
    pub(super) arg: String,
    #[serde(rename = "attempt")]
    pub(super) attempt: i64,
    #[serde(rename = "attemptId")]
    pub(super) attempt_id: String,
    #[serde(rename = "receiverNonce", skip_serializing_if = "String::is_empty")]
    pub(super) nonce: String,
    #[serde(rename = "runtimeEpoch", skip_serializing_if = "is_zero")]
    pub(super) epoch: u64,
    #[serde(rename = "reportScope", skip_serializing_if = "String::is_empty")]
    pub(super) report_scope: String,
    #[serde(rename = "phase")]
    pub(super) phase: String,
    #[serde(rename = "receiptKind", skip_serializing_if = "String::is_empty")]
    pub(super) receipt_kind: String,
    #[serde(rename = "sequence", skip_serializing_if = "is_zero")]
    pub(super) sequence: u64,
    #[serde(rename = "errorCode", skip_serializing_if = "String::is_empty")]
    pub(super) error_code: String,
    #[serde(rename = "updatedAt")]
    pub(super) updated_at: Timestamp,
}
impl WireRecord for ReceiverAttempt {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "operationid" => scalar(&mut self.operation_id, map),
            "action" => scalar(&mut self.action, map),
            "requestid" => scalar(&mut self.request_id, map),
            "arg" => scalar(&mut self.arg, map),
            "attempt" => signed_integer(&mut self.attempt, map),
            "attemptid" => scalar(&mut self.attempt_id, map),
            "receivernonce" => scalar(&mut self.nonce, map),
            "runtimeepoch" => scalar(&mut self.epoch, map),
            "reportscope" => scalar(&mut self.report_scope, map),
            "phase" => scalar(&mut self.phase, map),
            "receiptkind" => scalar(&mut self.receipt_kind, map),
            "sequence" => scalar(&mut self.sequence, map),
            "errorcode" => scalar(&mut self.error_code, map),
            "updatedat" => scalar(&mut self.updated_at, map),
            _ => ignored(map),
        }
    }
}
impl<'de> Deserialize<'de> for ReceiverAttempt {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        RecordSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Signal {
    #[serde(rename = "schema")]
    pub(super) schema: String,
    #[serde(rename = "release")]
    pub(super) release: String,
    #[serde(rename = "kind")]
    pub(super) kind: String,
    #[serde(rename = "sessionNonce")]
    pub(super) session_nonce: String,
    #[serde(rename = "requestId")]
    pub(super) request_id: String,
    #[serde(rename = "reloadNonce", skip_serializing_if = "String::is_empty")]
    pub(super) reload_nonce: String,
    #[serde(rename = "cleanupNonce", skip_serializing_if = "String::is_empty")]
    pub(super) cleanup_nonce: String,
    #[serde(rename = "probeNonce", skip_serializing_if = "String::is_empty")]
    pub(super) probe_nonce: String,
    #[serde(rename = "actorState", skip_serializing_if = "String::is_empty")]
    pub(super) actor_state: String,
    #[serde(rename = "inputReason", skip_serializing_if = "String::is_empty")]
    pub(super) input_reason: String,
    #[serde(rename = "character", skip_serializing_if = "String::is_empty")]
    pub(super) character: String,
    #[serde(rename = "realm", skip_serializing_if = "String::is_empty")]
    pub(super) realm: String,
    #[serde(rename = "guid", skip_serializing_if = "String::is_empty")]
    pub(super) guid: String,
    #[serde(rename = "product")]
    pub(super) product: String,
    #[serde(rename = "build")]
    pub(super) build: String,
    #[serde(rename = "sequence")]
    pub(super) sequence: u64,
    #[serde(rename = "runtimeEpoch", skip_serializing_if = "is_zero")]
    pub(super) runtime_epoch: u64,
    #[serde(rename = "inputReady")]
    pub(super) input_ready: bool,
    #[serde(rename = "codeBytes", skip_serializing_if = "is_zero")]
    pub(super) code_bytes: u32,
    #[serde(rename = "codeAdler32", skip_serializing_if = "String::is_empty")]
    pub(super) code_adler32: String,
    #[serde(rename = "reportBytes", skip_serializing_if = "is_zero")]
    pub(super) report_bytes: u32,
    #[serde(rename = "reportAdler32", skip_serializing_if = "String::is_empty")]
    pub(super) report_adler32: String,
    #[serde(rename = "receiverProtocol", skip_serializing_if = "String::is_empty")]
    pub(super) receiver_protocol: String,
    #[serde(rename = "receiverNonce", skip_serializing_if = "String::is_empty")]
    pub(super) receiver_nonce: String,
    #[serde(rename = "attemptId", skip_serializing_if = "String::is_empty")]
    pub(super) attempt_id: String,
    #[serde(rename = "bodyBytes", skip_serializing_if = "is_zero")]
    pub(super) body_bytes: u32,
    #[serde(rename = "bodyAdler32", skip_serializing_if = "String::is_empty")]
    pub(super) body_adler32: String,
    #[serde(rename = "commitNonce", skip_serializing_if = "String::is_empty")]
    pub(super) commit_nonce: String,
    #[serde(rename = "accepted", skip_serializing_if = "is_zero")]
    pub(super) accepted: bool,
    #[serde(rename = "errorCode", skip_serializing_if = "String::is_empty")]
    pub(super) error_code: String,
    #[serde(rename = "wakeBinding", skip_serializing_if = "String::is_empty")]
    pub(super) wake_binding: String,
    #[serde(rename = "submitBinding", skip_serializing_if = "String::is_empty")]
    pub(super) submit_binding: String,
    #[serde(rename = "closeBinding", skip_serializing_if = "String::is_empty")]
    pub(super) close_binding: String,
    #[serde(rename = "priorSessionSequence", skip_serializing_if = "is_zero")]
    pub(super) prior_session_sequence: u64,
    #[serde(rename = "priorSessionEpoch", skip_serializing_if = "is_zero")]
    pub(super) prior_session_epoch: u64,
    #[serde(rename = "reportScope", skip_serializing_if = "String::is_empty")]
    pub(super) report_scope: String,
    #[serde(rename = "workState", skip_serializing_if = "String::is_empty")]
    pub(super) work_state: String,
    #[serde(rename = "codeSHA256", skip_serializing_if = "String::is_empty")]
    pub(super) code_sha256: String,
    #[serde(rename = "receipt", skip_serializing_if = "String::is_empty")]
    pub(super) receipt: String,
    #[serde(rename = "resourcesReleased", skip_serializing_if = "is_zero")]
    pub(super) resources_released: bool,
}
impl WireRecord for Signal {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "release" => scalar(&mut self.release, map),
            "kind" => scalar(&mut self.kind, map),
            "sessionnonce" => scalar(&mut self.session_nonce, map),
            "requestid" => scalar(&mut self.request_id, map),
            "reloadnonce" => scalar(&mut self.reload_nonce, map),
            "cleanupnonce" => scalar(&mut self.cleanup_nonce, map),
            "probenonce" => scalar(&mut self.probe_nonce, map),
            "actorstate" => scalar(&mut self.actor_state, map),
            "inputreason" => scalar(&mut self.input_reason, map),
            "character" => scalar(&mut self.character, map),
            "realm" => scalar(&mut self.realm, map),
            "guid" => scalar(&mut self.guid, map),
            "product" => scalar(&mut self.product, map),
            "build" => scalar(&mut self.build, map),
            "sequence" => scalar(&mut self.sequence, map),
            "runtimeepoch" => scalar(&mut self.runtime_epoch, map),
            "inputready" => scalar(&mut self.input_ready, map),
            "codebytes" => scalar(&mut self.code_bytes, map),
            "codeadler32" => scalar(&mut self.code_adler32, map),
            "reportbytes" => scalar(&mut self.report_bytes, map),
            "reportadler32" => scalar(&mut self.report_adler32, map),
            "receiverprotocol" => scalar(&mut self.receiver_protocol, map),
            "receivernonce" => scalar(&mut self.receiver_nonce, map),
            "attemptid" => scalar(&mut self.attempt_id, map),
            "bodybytes" => scalar(&mut self.body_bytes, map),
            "bodyadler32" => scalar(&mut self.body_adler32, map),
            "commitnonce" => scalar(&mut self.commit_nonce, map),
            "accepted" => scalar(&mut self.accepted, map),
            "errorcode" => scalar(&mut self.error_code, map),
            "wakebinding" => scalar(&mut self.wake_binding, map),
            "submitbinding" => scalar(&mut self.submit_binding, map),
            "closebinding" => scalar(&mut self.close_binding, map),
            "priorsessionsequence" => scalar(&mut self.prior_session_sequence, map),
            "priorsessionepoch" => scalar(&mut self.prior_session_epoch, map),
            "reportscope" => scalar(&mut self.report_scope, map),
            "workstate" => scalar(&mut self.work_state, map),
            "codesha256" => scalar(&mut self.code_sha256, map),
            "receipt" => scalar(&mut self.receipt, map),
            "resourcesreleased" => scalar(&mut self.resources_released, map),
            _ => ignored(map),
        }
    }
}
impl<'de> Deserialize<'de> for Signal {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        RecordSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReceiverBindings {
    #[serde(rename = "wakeBinding")]
    pub(super) wake_binding: String,
    #[serde(rename = "submitBinding")]
    pub(super) submit_binding: String,
    #[serde(rename = "closeBinding")]
    pub(super) close_binding: String,
}
impl WireRecord for ReceiverBindings {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "wakebinding" => scalar(&mut self.wake_binding, map),
            "submitbinding" => scalar(&mut self.submit_binding, map),
            "closebinding" => scalar(&mut self.close_binding, map),
            _ => ignored(map),
        }
    }
}
impl<'de> Deserialize<'de> for ReceiverBindings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        RecordSeed(&mut value).deserialize(d)?;
        Ok(value)
    }
}

// Go pointer fields are set to nil by null; repeated non-null objects merge.
struct OptionalSignalSeed<'a>(&'a mut Option<Signal>);
impl<'de> DeserializeSeed<'de> for OptionalSignalSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct OptionalVisitor<'a>(&'a mut Option<Signal>);
        impl<'de> serde::de::Visitor<'de> for OptionalVisitor<'_> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a signal object or null")
            }
            fn visit_none<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                *self.0 = None;
                Ok(())
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                self.visit_none()
            }
            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                d: D,
            ) -> std::result::Result<(), D::Error> {
                RecordSeed(self.0.get_or_insert_with(Signal::default)).deserialize(d)
            }
        }
        d.deserialize_option(OptionalVisitor(self.0))
    }
}

// Go resets slice length but reuses its backing elements during repeated-key
// decoding. Keep hidden slots through shrink/regrow; null and [] clear them.
#[derive(Debug, Clone, Default)]
pub(super) struct History {
    storage: Vec<ReceiverAttempt>,
    length: usize,
}
impl History {
    fn is_empty(&self) -> bool {
        self.length == 0
    }
}
impl Serialize for History {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.storage[..self.length].serialize(s)
    }
}
struct HistorySeed<'a>(&'a mut History);
impl<'de> DeserializeSeed<'de> for HistorySeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct HistoryVisitor<'a>(&'a mut History);
        impl<'de> serde::de::Visitor<'de> for HistoryVisitor<'_> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an array or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                *self.0 = History::default();
                Ok(())
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<(), A::Error> {
                let mut length = 0;
                loop {
                    if length == self.0.storage.len() {
                        self.0.storage.push(ReceiverAttempt::default());
                    }
                    if seq
                        .next_element_seed(RecordSeed(&mut self.0.storage[length]))?
                        .is_none()
                    {
                        break;
                    }
                    length += 1;
                }
                self.0.length = length;
                if length == 0 {
                    *self.0 = History::default();
                }
                Ok(())
            }
        }
        d.deserialize_any(HistoryVisitor(self.0))
    }
}
