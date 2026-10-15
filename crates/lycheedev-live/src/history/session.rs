//! Fixed Go 3.2.0 historical SESSION verifier, with no live/input capability.
//!
//! Oracle: 3057a9666ab078270b63c7f06de748069b0d1f4b,
//! internal/live/history_session.go and internal/desktop/receiver_bindings.go.
//! ReadStore must authenticate pin/capture manifests and capture bytes. This
//! module verifies their historical relationship; it never discovers a window.
use super::{
    ReadStore,
    bootstrap::{ReceiverBindings, Signal},
    decode_record, fault as failure,
    signal::{SignalExpectation, parse_signal, validate_receiver_bindings},
};
use crate::go_mailbox_v1::admission::{
    ConnectionTarget, RecordSeed, WireRecord, ignored, scalar, signed_integer,
};
use lycheedev_core::{Result, control::OperationControl, selection::PinnedSet, storage};
use serde::{Serialize, de::MapAccess};

const SESSION_SCHEMA: &str = "lycheedev.session.v1";
const EVIDENCE_SCHEMA: &str = "lycheedev.window-session.v1";
pub(super) const SESSION_CAPTURE_LIMIT: usize = 16_384;

#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionRecord {
    pub schema: String,
    pub id: String,
    pub snapshot: String,
    #[serde(rename = "captureId")]
    pub capture_id: String,
    pub region: SessionRegion,
    // Go does not omit a zero struct even with an omitempty tag.
    pub bindings: ReceiverBindings,
}

/// Go image.Rectangle fields are capitalized and retain signed int64 values on
/// the fixed Windows amd64 target. No rectangle geometry validation is added.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionRegion {
    #[serde(rename = "Min")]
    pub min: SessionPoint,
    #[serde(rename = "Max")]
    pub max: SessionPoint,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionPoint {
    #[serde(rename = "X")]
    pub x: i64,
    #[serde(rename = "Y")]
    pub y: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordedSession {
    pub record: SessionRecord,
    pub target: ConnectionTarget,
    pub ready: Signal,
}

/// Only the historical identity projection. An old ready receipt and binding
/// never become a native connection, owner claim, or permission to send keys.
#[derive(Debug, Clone, Serialize)]
pub struct HistoricalConnection {
    pub id: String,
    pub snapshot: String,
    pub target: ConnectionTarget,
    pub character: String,
    pub realm: String,
    #[serde(rename = "captureId")]
    pub capture_id: String,
}
impl RecordedSession {
    pub fn connection(&self) -> HistoricalConnection {
        HistoricalConnection {
            id: self.record.id.clone(),
            snapshot: self.record.snapshot.clone(),
            target: self.target.clone(),
            character: self.ready.character.clone(),
            realm: self.ready.realm.clone(),
            capture_id: self.record.capture_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
struct WindowSessionEvidence {
    schema: String,
    target: ConnectionTarget,
    ready: Signal,
}

impl WireRecord for SessionRecord {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "id" => scalar(&mut self.id, map),
            "snapshot" => scalar(&mut self.snapshot, map),
            "captureid" => scalar(&mut self.capture_id, map),
            "region" => map.next_value_seed(RecordSeed(&mut self.region)),
            "bindings" => map.next_value_seed(RecordSeed(&mut self.bindings)),
            _ => ignored(map),
        }
    }
}
impl WireRecord for SessionRegion {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "min" => map.next_value_seed(RecordSeed(&mut self.min)),
            "max" => map.next_value_seed(RecordSeed(&mut self.max)),
            _ => ignored(map),
        }
    }
}
impl WireRecord for SessionPoint {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "x" => signed_integer(&mut self.x, map),
            "y" => signed_integer(&mut self.y, map),
            _ => ignored(map),
        }
    }
}
impl WireRecord for WindowSessionEvidence {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "target" => map.next_value_seed(RecordSeed(&mut self.target)),
            "ready" => map.next_value_seed(RecordSeed(&mut self.ready)),
            _ => ignored(map),
        }
    }
}

fn record_id(record: &SessionRecord) -> Result<String> {
    let mut unsigned = record.clone();
    unsigned.id.clear();
    Ok(format!(
        "SESSION-{}",
        storage::sha256(&storage::identity_json(&unsigned)?)
    ))
}

pub(super) fn read_session<S: ReadStore + ?Sized>(
    store: &S,
    id: &str,
    control: &OperationControl,
) -> Result<RecordedSession> {
    control.check()?;
    let document = store.document(&format!("session/{id}"), control)?;
    let mut record: SessionRecord = decode_record(&document.bytes, control)?;
    if record.id != id {
        return Err(failure("live.session_record_identity"));
    }
    if record.schema != SESSION_SCHEMA || record.id != record_id(&record)? {
        return Err(failure("live.session_record_integrity"));
    }
    // Authenticate the original bytes first. Defaults are presentation of a
    // historical record, not a mutation of the stored record/hash.
    if record.bindings.wake_binding.is_empty() {
        record.bindings = default_receiver_bindings();
    }
    validate_receiver_bindings(&record.bindings)?;
    control.check()?;
    let pin = store.pin(&record.snapshot, control)?;
    control.check()?;
    let capture = store.capture(&record.capture_id, SESSION_CAPTURE_LIMIT, control)?;
    control.check()?;
    if capture.bytes.len() > SESSION_CAPTURE_LIMIT {
        return Err(failure("vault.blob_limit"));
    }
    let payload: WindowSessionEvidence = decode_record(&capture.bytes, control)?;
    if payload.schema != EVIDENCE_SCHEMA || storage::identity_json(&payload)? != capture.bytes {
        return Err(failure("live.session_evidence_invalid"));
    }
    let signal = payload.ready;
    // The original Go re-marshals the typed receipt before ParseSignal. It does
    // not parse the untrusted nested raw JSON directly or fill missing identity.
    parse_signal(&storage::identity_json(&signal)?, control)?;
    let expected = SignalExpectation {
        kind: "ready".into(),
        release: signal.release.clone(),
        session_nonce: signal.session_nonce.clone(),
        character: signal.character.clone(),
        realm: signal.realm.clone(),
        product: signal.product.clone(),
        build: signal.build.clone(),
        require_input_ready: true,
        ..SignalExpectation::default()
    };
    session_expectation(&payload.target, &expected)?;
    signal.matches(&expected)?;
    if signal.guid.is_empty()
        || !signal.request_id.is_empty()
        || !signal.reload_nonce.is_empty()
        || signal.code_bytes != 0
        || !signal.code_adler32.is_empty()
        || signal.report_bytes != 0
        || !signal.report_adler32.is_empty()
        || signal.sequence > 9_007_199_254_740_991
    {
        return Err(failure("live.invalid_session_signal"));
    }
    snapshot_matches(&pin, &payload.target)?;
    let provenance = &capture.reference.provenance;
    if provenance.kind != "decoded-window-session"
        || provenance.locator
            != format!(
                "pid:{}/window:{}",
                payload.target.window.process_id, payload.target.window.handle
            )
        || provenance.snapshot != record.snapshot
        || !provenance.operation_id.is_empty()
        || !provenance.source_commit.is_empty()
        || !provenance.base_snapshot.is_empty()
        || !provenance.base_source_commit.is_empty()
        || provenance.data_build != signal.build
        || provenance.session != signal.session_nonce
        || !capture.reference.complete
        || capture.reference.truncated
        || capture.reference.media_type != "application/json"
    {
        return Err(failure("live.session_evidence_provenance"));
    }
    control.check()?;
    Ok(RecordedSession {
        record,
        target: payload.target,
        ready: signal,
    })
}

fn snapshot_matches(pin: &PinnedSet, target: &ConnectionTarget) -> Result<()> {
    let product = &target.client.product;
    let build = &target.client.full_build;
    // Historical sessions compare Source.Product exactly. SourceSupportsProduct
    // is deliberately NOT used here, even for a family-wide Forever source.
    if pin.source.as_ref().is_some_and(|s| &s.product != product)
        || pin
            .data
            .as_ref()
            .is_some_and(|d| &d.product != product || &d.full_build != build)
        || pin
            .changes
            .as_ref()
            .is_some_and(|c| &c.product != product || &c.full_build != build)
    {
        return Err(failure("live.session_snapshot_mismatch"));
    }
    Ok(())
}

fn binding_label(label: &str) -> bool {
    label.len() <= 128 && !label.chars().any(|c| c < '\u{20}' || c == '\u{7f}')
}
fn session_expectation(target: &ConnectionTarget, expected: &SignalExpectation) -> Result<()> {
    if !binding_label(&expected.character) || !binding_label(&expected.realm) {
        return Err(failure("live.invalid_binding_identity"));
    }
    if expected.kind != "ready"
        || expected.release.is_empty()
        || !expected.request_id.is_empty()
        || !expected.reload_nonce.is_empty()
        || !expected.cleanup_nonce.is_empty()
        || expected.product != target.client.product
        || expected.build != target.client.full_build
        || !expected.require_input_ready
        || (!expected.session_nonce.is_empty()
            && (expected.character.is_empty()
                || expected.realm.is_empty()
                || !lycheedev_core::selection::valid_digest(&expected.session_nonce, 32)))
    {
        return Err(failure("live.invalid_session_expectation"));
    }
    Ok(())
}

pub(super) fn default_receiver_bindings() -> ReceiverBindings {
    ReceiverBindings {
        wake_binding: "ALT-CTRL-]".into(),
        submit_binding: "ALT-CTRL-SHIFT-]".into(),
        close_binding: "ALT-CTRL-[".into(),
    }
}
#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
