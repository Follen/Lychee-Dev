//! Historical operation DTOs. Parsing these records never grants input authority.
use super::bootstrap::ReceiverBindings;
use super::signal::SignalExpectation;
use super::{Timestamp, decode_record, fault};
use crate::go_mailbox_v1::admission::{
    ConnectionTarget, RecordSeed, WireRecord, ignored, scalar, signed_integer,
};
use lycheedev_core::{Result, control::OperationControl};
use serde::{
    Deserialize,
    de::{DeserializeSeed, MapAccess},
};

#[derive(Debug, Clone, Default)]
pub(super) struct WorkIntent {
    pub kind: String,
    pub resource: String,
    pub snapshot: String,
    pub session: String,
    pub request: RawField,
    pub request_key: String,
    pub request_digest: String,
    pub goal: String,
    pub admission: Option<WindowAdmission>,
}
impl WireRecord for WorkIntent {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "kind" => scalar(&mut self.kind, map),
            "resource" => scalar(&mut self.resource, map),
            "snapshot" => scalar(&mut self.snapshot, map),
            "session" => scalar(&mut self.session, map),
            "request" => {
                self.request = map.next_value()?;
                Ok(())
            }
            "requestkey" => scalar(&mut self.request_key, map),
            "requestdigest" => scalar(&mut self.request_digest, map),
            "goal" => scalar(&mut self.goal, map),
            "admission" => map.next_value_seed(PointerSeed(&mut self.admission)),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct WindowAdmission {
    pub parent: String,
    pub workspace_id: String,
}
impl WireRecord for WindowAdmission {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "parent" => scalar(&mut self.parent, map),
            "workspaceid" => scalar(&mut self.workspace_id, map),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct WorkRecord {
    pub schema: String,
    pub operation_id: String,
    pub generation: i64,
    pub intent: WorkIntent,
    pub stage: String,
    pub status: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub observation: RawField,
}
impl WireRecord for WorkRecord {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "operationid" => scalar(&mut self.operation_id, map),
            "generation" => signed_integer(&mut self.generation, map),
            "intent" => map.next_value_seed(RecordSeed(&mut self.intent)),
            "stage" => scalar(&mut self.stage, map),
            "status" => scalar(&mut self.status, map),
            "createdat" => scalar(&mut self.created_at, map),
            "updatedat" => scalar(&mut self.updated_at, map),
            "observation" => {
                self.observation = map.next_value()?;
                Ok(())
            }
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct ReportIntent {
    pub schema: String,
    pub revision: String,
    pub budget_seconds: i64,
    pub expected: SignalExpectation,
    pub code: GoBytes,
    pub load: Option<ProbeLoadIntent>,
    pub binding: String,
    pub declared_budget: bool,
}
impl WireRecord for ReportIntent {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "revision" => scalar(&mut self.revision, map),
            "budgetseconds" => signed_integer(&mut self.budget_seconds, map),
            "expected" => map.next_value_seed(RecordSeed(&mut self.expected)),
            "code" => map.next_value_seed(BytesSeed(&mut self.code)),
            "load" => map.next_value_seed(PointerSeed(&mut self.load)),
            "binding" => scalar(&mut self.binding, map),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct ProbeLoadIntent {
    pub installation: String,
    pub account: String,
    pub report_scope: String,
    pub guid: String,
    pub reload_nonce: String,
}
impl WireRecord for ProbeLoadIntent {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "installation" => scalar(&mut self.installation, map),
            "account" => scalar(&mut self.account, map),
            "reportscope" => scalar(&mut self.report_scope, map),
            "guid" => scalar(&mut self.guid, map),
            "reloadnonce" => scalar(&mut self.reload_nonce, map),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct ReportObservation {
    pub bootstrap_ready_id: String,
    pub bootstrap_capture: String,
    pub bootstrap_input: Option<InputReceipt>,
    pub schema: String,
    pub source_path: String,
    pub source_sha256: String,
    pub body_id: String,
    pub receipt_id: String,
    pub acknowledgement_id: String,
    pub removal_id: String,
    pub removal_sha256: String,
    pub queue_retirement: RawField,
    pub cleanup_nonce: String,
    pub cleared_id: String,
    pub reloaded_capture: String,
    pub loaded_capture: String,
    pub load_ready_capture: String,
    pub reported_capture: String,
    pub flush_ready_capture: String,
    pub load_input: Option<InputReceipt>,
    pub dispatch_input: Option<InputReceipt>,
    pub flush_input: Option<InputReceipt>,
    pub ack_ready_capture: String,
    pub ack_input: Option<InputReceipt>,
    pub cleanup_ready_id: String,
    pub cleanup_input: Option<InputReceipt>,
}
impl WireRecord for ReportObservation {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "bootstrapreadyid" => scalar(&mut self.bootstrap_ready_id, map),
            "bootstrapcapture" => scalar(&mut self.bootstrap_capture, map),
            "bootstrapinput" => map.next_value_seed(PointerSeed(&mut self.bootstrap_input)),
            "schema" => scalar(&mut self.schema, map),
            "sourcepath" => scalar(&mut self.source_path, map),
            "sourcesha256" => scalar(&mut self.source_sha256, map),
            "bodyid" => scalar(&mut self.body_id, map),
            "receiptid" => scalar(&mut self.receipt_id, map),
            "acknowledgementid" => scalar(&mut self.acknowledgement_id, map),
            "removalid" => scalar(&mut self.removal_id, map),
            "removalsha256" => scalar(&mut self.removal_sha256, map),
            "queueretirement" => {
                self.queue_retirement = map.next_value()?;
                Ok(())
            }
            "cleanupnonce" => scalar(&mut self.cleanup_nonce, map),
            "clearedid" => scalar(&mut self.cleared_id, map),
            "reloadedcapture" => scalar(&mut self.reloaded_capture, map),
            "loadedcapture" => scalar(&mut self.loaded_capture, map),
            "loadreadycapture" => scalar(&mut self.load_ready_capture, map),
            "reportedcapture" => scalar(&mut self.reported_capture, map),
            "flushreadycapture" => scalar(&mut self.flush_ready_capture, map),
            "loadinput" => map.next_value_seed(PointerSeed(&mut self.load_input)),
            "dispatchinput" => map.next_value_seed(PointerSeed(&mut self.dispatch_input)),
            "flushinput" => map.next_value_seed(PointerSeed(&mut self.flush_input)),
            "ackreadycapture" => scalar(&mut self.ack_ready_capture, map),
            "ackinput" => map.next_value_seed(PointerSeed(&mut self.ack_input)),
            "cleanupreadyid" => scalar(&mut self.cleanup_ready_id, map),
            "cleanupinput" => map.next_value_seed(PointerSeed(&mut self.cleanup_input)),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct ProbeLoadObservation {
    pub bootstrap_ready_id: String,
    pub bootstrap_capture: String,
    pub bootstrap_input: Option<InputReceipt>,
    pub schema: String,
    pub revision: RawField,
    pub loaded_capture: String,
    pub load_ready_capture: String,
    pub load_input: Option<InputReceipt>,
    pub reported_capture: String,
    pub report_error_capture: String,
    pub dispatch_input: Option<InputReceipt>,
    pub execution_requested_at: Timestamp,
    pub flush_input: Option<InputReceipt>,
    pub flush_ready_capture: String,
    pub reloaded_capture: String,
}
impl WireRecord for ProbeLoadObservation {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "bootstrapreadyid" => scalar(&mut self.bootstrap_ready_id, map),
            "bootstrapcapture" => scalar(&mut self.bootstrap_capture, map),
            "bootstrapinput" => map.next_value_seed(PointerSeed(&mut self.bootstrap_input)),
            "schema" => scalar(&mut self.schema, map),
            "revision" => {
                self.revision = map.next_value()?;
                Ok(())
            }
            "loadedcapture" => scalar(&mut self.loaded_capture, map),
            "loadreadycapture" => scalar(&mut self.load_ready_capture, map),
            "loadinput" => map.next_value_seed(PointerSeed(&mut self.load_input)),
            "reportedcapture" => scalar(&mut self.reported_capture, map),
            "reporterrorcapture" => scalar(&mut self.report_error_capture, map),
            "dispatchinput" => map.next_value_seed(PointerSeed(&mut self.dispatch_input)),
            "executionrequestedat" => scalar(&mut self.execution_requested_at, map),
            "flushinput" => map.next_value_seed(PointerSeed(&mut self.flush_input)),
            "flushreadycapture" => scalar(&mut self.flush_ready_capture, map),
            "reloadedcapture" => scalar(&mut self.reloaded_capture, map),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct FixedReloadIntent {
    pub schema: String,
    pub target: ConnectionTarget,
    pub release: String,
    pub commit: String,
    pub identity_nonce: String,
    pub old_epoch: u64,
    pub old_report_scope: String,
    pub old_guid: String,
    pub old_character: String,
    pub old_realm: String,
    pub bindings: ReceiverBindings,
    pub known_bindings: bool,
}
impl WireRecord for FixedReloadIntent {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "target" => map.next_value_seed(RecordSeed(&mut self.target)),
            "release" => scalar(&mut self.release, map),
            "commit" => scalar(&mut self.commit, map),
            "identitynonce" => scalar(&mut self.identity_nonce, map),
            "oldepoch" => scalar(&mut self.old_epoch, map),
            "oldreportscope" => scalar(&mut self.old_report_scope, map),
            "oldguid" => scalar(&mut self.old_guid, map),
            "oldcharacter" => scalar(&mut self.old_character, map),
            "oldrealm" => scalar(&mut self.old_realm, map),
            "bindings" => map.next_value_seed(RecordSeed(&mut self.bindings)),
            "knownbindings" => scalar(&mut self.known_bindings, map),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct FixedReloadObservation {
    pub schema: String,
    pub dismiss_requested: bool,
    pub close_queued: bool,
    pub release_at: Timestamp,
    pub receiver_released: bool,
    pub next_step: i64,
    pub input_complete: bool,
    pub input: Option<InputReceipt>,
    pub identity_capture: String,
}
impl WireRecord for FixedReloadObservation {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "dismissrequested" => scalar(&mut self.dismiss_requested, map),
            "closequeued" => scalar(&mut self.close_queued, map),
            "releaseat" => scalar(&mut self.release_at, map),
            "receiverreleased" => scalar(&mut self.receiver_released, map),
            "nextstep" => signed_integer(&mut self.next_step, map),
            "inputcomplete" => scalar(&mut self.input_complete, map),
            "input" => map.next_value_seed(PointerSeed(&mut self.input)),
            "identitycapture" => scalar(&mut self.identity_capture, map),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct ReloadIntent {
    pub schema: String,
    pub binding: String,
    pub installation: String,
    pub guid: String,
    pub reload_nonce: String,
    pub expected: SignalExpectation,
    pub from_release: String,
    pub managed_commit: String,
}
impl WireRecord for ReloadIntent {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "binding" => scalar(&mut self.binding, map),
            "installation" => scalar(&mut self.installation, map),
            "guid" => scalar(&mut self.guid, map),
            "reloadnonce" => scalar(&mut self.reload_nonce, map),
            "expected" => map.next_value_seed(RecordSeed(&mut self.expected)),
            "fromrelease" => scalar(&mut self.from_release, map),
            "managedcommit" => scalar(&mut self.managed_commit, map),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct ReloadObservation {
    pub schema: String,
    pub input: Option<InputReceipt>,
    pub ready_capture: String,
}
impl WireRecord for ReloadObservation {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "input" => map.next_value_seed(PointerSeed(&mut self.input)),
            "readycapture" => scalar(&mut self.ready_capture, map),
            _ => ignored(map),
        }
    }
}
#[derive(Debug, Clone, Default)]
pub(super) struct InputReceipt {
    pub messages_queued: i64,
    pub submission_complete: bool,
}
impl WireRecord for InputReceipt {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "messagesqueued" => signed_integer(&mut self.messages_queued, map),
            "submissioncomplete" => scalar(&mut self.submission_complete, map),
            _ => ignored(map),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct RawField(pub Vec<u8>);
impl<'de> Deserialize<'de> for RawField {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Ok(Self(
            Box::<serde_json::value::RawValue>::deserialize(d)?
                .get()
                .as_bytes()
                .to_vec(),
        ))
    }
}
/// Go []byte accepts base64 strings, null, and arrays with reused byte slots.
#[derive(Debug, Clone, Default)]
pub(super) struct GoBytes {
    storage: Vec<u8>,
    length: usize,
}
impl GoBytes {
    pub fn bytes(&self) -> &[u8] {
        &self.storage[..self.length]
    }
}
struct BytesSeed<'a>(&'a mut GoBytes);
impl<'de> DeserializeSeed<'de> for BytesSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct BytesVisitor<'a>(&'a mut GoBytes);
        impl<'de> serde::de::Visitor<'de> for BytesVisitor<'_> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("base64 string, byte array, or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                *self.0 = GoBytes::default();
                Ok(())
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> std::result::Result<(), E> {
                let bytes = super::signal::decode_base64(s.as_bytes()).map_err(E::custom)?;
                self.0.length = bytes.len();
                self.0.storage = bytes;
                Ok(())
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<(), A::Error> {
                let mut length = 0;
                while let Some(byte) = seq.next_element::<Option<u8>>()? {
                    if length == self.0.storage.len() {
                        self.0.storage.push(0);
                    }
                    if let Some(byte) = byte {
                        self.0.storage[length] = byte;
                    }
                    length += 1;
                }
                self.0.length = length;
                if length == 0 {
                    self.0.storage.clear();
                }
                Ok(())
            }
        }
        d.deserialize_any(BytesVisitor(self.0))
    }
}
pub(super) struct PointerSeed<'a, T>(pub(super) &'a mut Option<T>);
impl<'de, T: WireRecord + Default> DeserializeSeed<'de> for PointerSeed<'_, T> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        struct PointerVisitor<'a, T>(&'a mut Option<T>);
        impl<'de, T: WireRecord + Default> serde::de::Visitor<'de> for PointerVisitor<'_, T> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("object or null")
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
                RecordSeed(self.0.get_or_insert_with(T::default)).deserialize(d)
            }
        }
        d.deserialize_option(PointerVisitor(self.0))
    }
}
pub(super) fn report_input(
    record: &WorkRecord,
    control: &OperationControl,
) -> Result<ReportIntent> {
    let mut input: ReportIntent = decode_record(&record.intent.request.0, control)?;
    if input.schema != "lycheedev.report-intent.v1"
        || record.intent.snapshot.is_empty()
        || record.intent.session.is_empty()
        || input.expected.session_nonce != record.intent.session
    {
        return Err(fault("live.invalid_report_intent"));
    }
    input.declared_budget = input.budget_seconds != 0;
    if input
        .load
        .as_ref()
        .is_some_and(|l| !l.report_scope.is_empty() && l.report_scope != "character-v1")
    {
        return Err(fault("bridge.invalid_report_scope"));
    }
    if input.budget_seconds == 0 {
        input.budget_seconds = 120;
    }
    if !(1..=120).contains(&input.budget_seconds) {
        return Err(fault("live.execution_budget_invalid"));
    }
    if !record.observation.0.is_empty() {
        let bootstrap: BootstrapObservation = decode_record(&record.observation.0, control)?;
        if !bootstrap.bootstrap_capture.is_empty() {
            input.expected.after_sequence = 0;
        }
    }
    Ok(input)
}

#[derive(Default)]
struct BootstrapObservation {
    bootstrap_ready_id: String,
    bootstrap_capture: String,
    bootstrap_input: Option<InputReceipt>,
}
impl WireRecord for BootstrapObservation {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "bootstrapreadyid" => scalar(&mut self.bootstrap_ready_id, map),
            "bootstrapcapture" => scalar(&mut self.bootstrap_capture, map),
            "bootstrapinput" => map.next_value_seed(PointerSeed(&mut self.bootstrap_input)),
            _ => ignored(map),
        }
    }
}
