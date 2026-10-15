//! Historical OP read model. Availability never implies execution or cleanup.
use super::{
    ReadStore, decode_record, fault,
    store::{provenance, provenance_matches},
};
use super::{
    display::{self, DisplayOutcome},
    records::*,
    signal::{self, VerifiedReport},
};
use crate::go_mailbox_v1::{
    admission::{WireRecord, ignored, scalar, signed_integer},
    connection::RawReport,
};
use lycheedev_core::{Result, control::OperationControl};
use serde::{Serialize, de::MapAccess};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalOutcome {
    pub operation_id: String,
    pub snapshot: String,
    pub status: String,
    pub complete: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub goal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<NextAction>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub runtime_capture: String,
    pub report: ReportOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub business: Option<BusinessOutcome>,
    pub cleanup: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<DisplayOutcome>,
    #[serde(skip)]
    pub stage: String,
}
#[derive(Debug, Serialize)]
pub struct NextAction {
    pub command: String,
    pub args: Vec<String>,
    pub reason: String,
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportOutcome {
    pub state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error_code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error_capture: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub body_capture: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub receipt_capture: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<RawReport>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BusinessOutcome {
    pub state: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub accepted_budget_seconds: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assertions_passed: Option<bool>,
}
fn is_zero(value: &i64) -> bool {
    *value == 0
}
fn pending(record: &WorkRecord) -> HistoricalOutcome {
    let cleanup = if record.stage == "abandoned" && record.status == "abandoned" {
        "abandoned"
    } else if record.stage == "cleaned"
        && matches!(record.status.as_str(), "completed" | "cancelled")
    {
        "complete"
    } else {
        "pending"
    };
    HistoricalOutcome {
        operation_id: record.operation_id.clone(),
        snapshot: record.intent.snapshot.clone(),
        status: record.status.clone(),
        complete: false,
        goal: record.intent.goal.clone(),
        next_action: None,
        runtime_capture: String::new(),
        report: ReportOutcome {
            state: "unavailable".into(),
            ..Default::default()
        },
        business: None,
        cleanup: cleanup.into(),
        display: None,
        stage: record.stage.clone(),
    }
}
fn next(mut result: HistoricalOutcome) -> HistoricalOutcome {
    if result.goal == "finished"
        && !result.complete
        && result.cleanup != "abandoned"
        && result.status != "failed"
    {
        result.next_action = Some(NextAction {
            command: "lycheedev".into(),
            args: vec!["live".into(), "abandon".into(), result.operation_id.clone()],
            reason: "legacy_execution_retired_explicit_host_abandon_available".into(),
        });
    }
    result
}
/// Go returns its typed projection alongside errors, including the zero outcome
/// for workspace/journal failures and verified bytes for later display failures.
#[derive(Debug)]
pub struct HistoricalRead {
    pub outcome: HistoricalOutcome,
    pub error: Option<lycheedev_core::ToolkitError>,
}
#[cfg(test)]
pub(super) fn read_status(
    store: &impl ReadStore,
    id: &str,
    control: &OperationControl,
) -> Result<HistoricalOutcome> {
    let read = read_status_outcome(store, id, control);
    match read.error {
        Some(error) => Err(error),
        None => Ok(read.outcome),
    }
}
pub(super) fn read_status_outcome(
    store: &impl ReadStore,
    id: &str,
    control: &OperationControl,
) -> HistoricalRead {
    let mut result = HistoricalOutcome::default();
    let error = (|| -> Result<()> {
        control.check()?;
        let document = store.document(&format!("work/{id}"), control)?;
        let record: WorkRecord = decode_record(&document.bytes, control)?;
        if record.schema != "lycheedev.work.v1"
            || record.operation_id != id
            || record.generation != document.generation
        {
            return Err(fault("journal: corrupt work record"));
        }
        result = pending(&record);
        if record.intent.kind == "reload" {
            return reload(store, &record, &mut result, control);
        }
        let observed: ReportObservation = decode_record(&record.observation.0, control)?;
        if record.status == "failed" && record.stage == "dispatch_requested" {
            let failed: ProbeLoadObservation = decode_record(&record.observation.0, control)?;
            if failed.report_error_capture.is_empty() {
                return Ok(());
            }
            let capture = store.capture(&failed.report_error_capture, 4096, control)?;
            let input = report_input(&record, control)?;
            let reference = &capture.reference;
            if reference.provenance.kind != "decoded-game-report-error"
                || reference.provenance.operation_id != record.operation_id
                || reference.provenance.locator != input.expected.request_id
                || !reference.complete
                || reference.truncated
            {
                return Err(fault("live.report_error_evidence_mismatch"));
            }
            let signal = signal::parse_signal(&capture.bytes, control)?;
            if signal.kind != "report_error"
                || signal.request_id != input.expected.request_id
                || signal.error_code.is_empty()
                || signal.session_nonce != input.expected.session_nonce
            {
                return Err(fault("live.report_error_evidence_mismatch"));
            }
            result.report.error_code = signal.error_code;
            result.report.error_capture = reference.id.clone();
            return Ok(());
        }
        if observed.body_id.is_empty() || observed.receipt_id.is_empty() {
            return Ok(());
        }
        let input = report_input(&record, control)?;
        let report = verified_report(
            store,
            &input,
            &record,
            &observed.body_id,
            &observed.receipt_id,
            control,
        )?;
        result.report = ReportOutcome {
            state: "verified".into(),
            body_capture: observed.body_id,
            receipt_capture: observed.receipt_id,
            sha256: report.sha256,
            content: Some(RawReport(report.body)),
            ..Default::default()
        };
        result.business = business_outcome(
            &result
                .report
                .content
                .as_ref()
                .expect("verified report body")
                .0,
            &input,
            control,
        )?;
        result.complete = result.cleanup == "complete";
        if record.intent.goal == "finished" {
            result.display = Some(DisplayOutcome {
                state: "pending".into(),
                ..Default::default()
            });
            result.complete = false;
            let display = display::read_display_proof(store, &record, &input.binding, control)?;
            if display.state == "cleared" {
                result.display = Some(display);
                result.complete = result.cleanup == "complete";
            }
        }
        control.check()?;
        Ok(())
    })()
    .err();
    HistoricalRead {
        outcome: next(result),
        error,
    }
}
pub(super) fn verified_report(
    store: &impl ReadStore,
    input: &ReportIntent,
    record: &WorkRecord,
    body_id: &str,
    receipt_id: &str,
    control: &OperationControl,
) -> Result<VerifiedReport> {
    if record.operation_id.is_empty() || record.intent.snapshot.is_empty() {
        return Err(fault("evidence.report_context_required"));
    }
    let body = store.capture(body_id, 512 << 10, control)?;
    let receipt = store.capture(receipt_id, 4096, control)?;
    for (capture, kind) in [(&body, "game-report"), (&receipt, "game-receipt")] {
        let want = provenance(
            kind,
            &input.expected.request_id,
            &record.intent.snapshot,
            &record.operation_id,
            &input.expected.build,
            &input.expected.session_nonce,
        );
        if !provenance_matches(&capture.reference.provenance, &want)
            || !capture.reference.complete
            || capture.reference.truncated
            || capture.reference.media_type != "application/json"
        {
            return Err(fault("evidence.report_provenance_mismatch"));
        }
    }
    signal::verify_report(
        &receipt.bytes,
        &body.bytes,
        input.code.bytes(),
        &input.expected,
        control,
    )
}
#[derive(Default)]
struct BusinessPayload {
    probe_status: String,
    accepted_budget_seconds: i64,
    result: RawField,
}
impl WireRecord for BusinessPayload {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "probestatus" => scalar(&mut self.probe_status, map),
            "acceptedbudgetseconds" => signed_integer(&mut self.accepted_budget_seconds, map),
            "result" => {
                self.result = map.next_value()?;
                Ok(())
            }
            _ => ignored(map),
        }
    }
}
#[derive(Default)]
struct Assertion {
    passed: Option<bool>,
}
impl WireRecord for Assertion {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        if key == "passed" {
            self.passed = map.next_value()?;
            Ok(())
        } else {
            ignored(map)
        }
    }
}
fn business_outcome(
    body: &[u8],
    input: &ReportIntent,
    control: &OperationControl,
) -> Result<Option<BusinessOutcome>> {
    if input.revision == "builtin:bugs" {
        return Ok(None);
    }
    let report: BusinessPayload = decode_record(body, control)?;
    if !matches!(report.probe_status.as_str(), "completed" | "failed") {
        if !input.declared_budget {
            return Ok(None);
        }
        return Err(fault("live.business_status_invalid"));
    }
    if input.declared_budget && report.accepted_budget_seconds != input.budget_seconds {
        return Err(fault("live.execution_budget_mismatch"));
    }
    let mut result = BusinessOutcome {
        state: report.probe_status,
        accepted_budget_seconds: report.accepted_budget_seconds,
        assertions_passed: None,
    };
    if result.state == "completed" && !report.result.0.is_empty() {
        // Malformed/non-object assertion payloads do not invalidate the report.
        if let Ok(assertion) = decode_record::<Assertion>(&report.result.0, control) {
            result.assertions_passed = assertion.passed;
            if let Some(passed) = assertion.passed {
                result.state = if passed { "passed" } else { "assertion_failed" }.into();
            }
        }
        control.check()?;
    }
    Ok(Some(result))
}
#[derive(Default)]
struct SchemaOnly {
    schema: String,
}
impl WireRecord for SchemaOnly {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        if key == "schema" {
            scalar(&mut self.schema, map)
        } else {
            ignored(map)
        }
    }
}
fn reload(
    store: &impl ReadStore,
    record: &WorkRecord,
    result: &mut HistoricalOutcome,
    control: &OperationControl,
) -> Result<()> {
    if decode_record::<SchemaOnly>(&record.intent.request.0, control)
        .is_ok_and(|s| s.schema == "lycheedev.fixed-reload.v1")
    {
        let intent: FixedReloadIntent = super::domain_decode(
            decode_record(&record.intent.request.0, control),
            "live.fixed_reload_intent_invalid",
            control,
        )?;
        if intent.schema != "lycheedev.fixed-reload.v1"
            || intent.release.is_empty()
            || intent.identity_nonce.is_empty()
            || intent.target.client.directory.is_empty()
            || intent.target.window.process_id == 0
            || intent.target.window.process_started_at == 0
        {
            return Err(fault("live.fixed_reload_intent_invalid"));
        }
        let observed: FixedReloadObservation =
            if record.observation.0.is_empty() || record.observation.0 == b"null" {
                Default::default()
            } else {
                let o: FixedReloadObservation = super::domain_decode(
                    decode_record(&record.observation.0, control),
                    "live.fixed_reload_observation_invalid",
                    control,
                )?;
                if o.schema != "lycheedev.fixed-reload-observation.v1"
                    || !(0..=6).contains(&o.next_step)
                {
                    return Err(fault("live.fixed_reload_observation_invalid"));
                }
                o
            };
        if record.stage != "cleaned" || record.status != "completed" {
            return Ok(());
        }
        let capture = store.capture(&observed.identity_capture, 4096, control)?;
        let p = &capture.reference.provenance;
        if p.kind != "decoded-fixed-reload"
            || p.locator != intent.identity_nonce
            || p.operation_id != record.operation_id
            || !capture.reference.complete
            || capture.reference.truncated
        {
            return Err(fault("live.fixed_reload_evidence_mismatch"));
        }
        let signal = signal::parse_signal(&capture.bytes, control)?;
        if signal.kind != "identity"
            || signal.probe_nonce != intent.identity_nonce
            || signal.release != intent.release
            || signal.product != intent.target.client.product
            || signal.build != intent.target.client.full_build
        {
            return Err(fault("live.fixed_reload_evidence_mismatch"));
        }
        result.complete = true;
        result.runtime_capture = capture.reference.id;
        return Ok(());
    }
    control.check()?;
    let input: ReloadIntent = super::domain_decode(
        decode_record(&record.intent.request.0, control),
        "live.invalid_reload_intent",
        control,
    )?;
    if input.schema != "lycheedev.reload-intent.v1"
        || input.binding.is_empty()
        || !std::path::Path::new(&input.installation).is_absolute()
        || input.reload_nonce.len() != 32
        || hex::decode(&input.reload_nonce).is_err()
        || input.expected.request_id != format!("RELOAD-{}", input.reload_nonce)
        || input.expected.reload_nonce != input.reload_nonce
        || input.expected.kind != "ready"
    {
        return Err(fault("live.invalid_reload_intent"));
    }
    if record.stage != "cleaned" || record.status != "completed" {
        return Ok(());
    }
    let observed: ReloadObservation = super::domain_decode(
        decode_record(&record.observation.0, control),
        "live.invalid_reload_receipt",
        control,
    )?;
    if observed.schema != "lycheedev.reload-observation.v1" || observed.ready_capture.is_empty() {
        return Err(fault("live.invalid_reload_receipt"));
    }
    let capture = store.capture(&observed.ready_capture, 4096, control)?;
    let want = provenance(
        "decoded-standalone-reload",
        &input.reload_nonce,
        &record.intent.snapshot,
        &record.operation_id,
        &input.expected.build,
        &input.expected.session_nonce,
    );
    if !provenance_matches(&capture.reference.provenance, &want)
        || !capture.reference.complete
        || capture.reference.truncated
        || capture.reference.media_type != "application/json"
    {
        return Err(fault("live.invalid_reload_receipt"));
    }
    let signal = signal::parse_signal(&capture.bytes, control)?;
    signal.matches(&input.expected)?;
    if signal.guid != input.guid
        || signal.code_bytes != 0
        || signal.report_bytes != 0
        || !signal.cleanup_nonce.is_empty()
    {
        return Err(fault("live.invalid_reload_receipt"));
    }
    result.complete = true;
    result.runtime_capture = capture.reference.id;
    Ok(())
}
