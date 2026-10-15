//! Read-only historical display proof verification. Oracle: Go v3.2.0,
//! 3057a9666ab078270b63c7f06de748069b0d1f4b, internal/live/history_display.go.
//! No capture, input, ACK, display mutation, or execution capability exists here.
//! Staged implementation and regression tests are NOT_RUN.
#![forbid(unsafe_code)]

use super::records::{ReportIntent, ReportObservation, WorkRecord, report_input};
use super::signal::{
    SignalExpectation, SignalIdentity, VerifiedReport, fill_signal_identity, parse_signal,
};
use super::store::{provenance, provenance_matches};
use super::{ReadStore, Timestamp, bootstrap::Signal, decode_record, fault};
use crate::go_mailbox_v1::admission::{RecordSeed, WireRecord, ignored, scalar, signed_integer};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, evidence::Provenance};
use serde::{Serialize, de::MapAccess};
use sha2::{Digest, Sha256};

const PROOF_LIMIT: usize = 4096;
const FRAME_LIMIT: usize = 32 << 20;

#[derive(Debug, Clone, Default, Serialize)]
pub struct HideReceiptResult {
    pub session: String,
    #[serde(
        rename = "bootstrapAttemptId",
        skip_serializing_if = "String::is_empty"
    )]
    pub bootstrap_attempt_id: String,
    pub cleared: bool,
    #[serde(rename = "messagesQueued")]
    pub messages_queued: i64,
    #[serde(rename = "submissionComplete")]
    pub submission_complete: bool,
    #[serde(rename = "observedAt")]
    pub(super) observed_at: Timestamp,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DisplayOutcome {
    pub state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub capture: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<HideReceiptResult>,
}

#[derive(Debug, Clone, Default)]
struct DisplayCompletion {
    schema: String,
    operation: String,
    binding: String,
    receipt: HideReceiptResult,
    checkpoint: String,
    challenge: String,
    frame: String,
    runtime_epoch: u64,
}

#[derive(Default)]
struct DisplayLink {
    capture: String,
}

impl WireRecord for DisplayLink {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "capture" => scalar(&mut self.capture, map),
            _ => ignored(map),
        }
    }
}
impl WireRecord for HideReceiptResult {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "session" => scalar(&mut self.session, map),
            "bootstrapattemptid" => scalar(&mut self.bootstrap_attempt_id, map),
            "cleared" => scalar(&mut self.cleared, map),
            "messagesqueued" => signed_integer(&mut self.messages_queued, map),
            "submissioncomplete" => scalar(&mut self.submission_complete, map),
            "observedat" => scalar(&mut self.observed_at, map),
            _ => ignored(map),
        }
    }
}
impl WireRecord for DisplayCompletion {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "operationid" => scalar(&mut self.operation, map),
            "binding" => scalar(&mut self.binding, map),
            "receipt" => map.next_value_seed(RecordSeed(&mut self.receipt)),
            "checkpoint" => scalar(&mut self.checkpoint, map),
            "challenge" => scalar(&mut self.challenge, map),
            "frame" => scalar(&mut self.frame, map),
            "runtimeepoch" => scalar(&mut self.runtime_epoch, map),
            _ => ignored(map),
        }
    }
}

fn display_provenance(record: &WorkRecord, binding: &str) -> Provenance {
    provenance(
        "decoded-display-clear",
        binding,
        &record.intent.snapshot,
        &record.operation_id,
        "",
        &record.intent.session,
    )
}
fn checkpoint_provenance(record: &WorkRecord, input: &ReportIntent, challenge: &str) -> Provenance {
    provenance(
        "decoded-game-checkpoint",
        challenge,
        &record.intent.snapshot,
        &record.operation_id,
        &input.expected.build,
        &input.expected.session_nonce,
    )
}

fn decode_display<T: WireRecord + Default>(raw: &[u8], control: &OperationControl) -> Result<T> {
    // Go maps malformed proof JSON to its display category; the shared wrapper
    // preserves the original typed cancellation and deadline failures.
    super::domain_decode(
        decode_record(raw, control),
        "live.invalid_display_completion",
        control,
    )
}

pub(super) fn read_display_proof(
    store: &impl ReadStore,
    record: &WorkRecord,
    binding: &str,
    control: &OperationControl,
) -> Result<DisplayOutcome> {
    control.check()?;
    let document = match store.document(&format!("display/{}", record.operation_id), control) {
        Ok(document) => document,
        Err(error) if error.code == "vault.record_missing" => return Ok(DisplayOutcome::default()),
        Err(error) => return Err(error),
    };
    let link: DisplayLink = decode_display(&document.bytes, control)?;
    if link.capture.is_empty() {
        return Err(fault("live.invalid_display_completion"));
    }
    let captured = store.capture(&link.capture, PROOF_LIMIT, control)?;
    let proof: DisplayCompletion = decode_display(&captured.bytes, control)?;
    let reference = captured.reference;
    if proof.schema != "lycheedev.display-completion.v1"
        || proof.operation != record.operation_id
        || proof.binding != binding
        || proof.receipt.session != binding
        || !proof.receipt.cleared
        || proof.receipt.observed_at.is_zero()
        || !reference.complete
        || reference.truncated
        || reference.media_type != "application/json"
        || !provenance_matches(&reference.provenance, &display_provenance(record, binding))
    {
        return Err(fault("live.invalid_display_completion"));
    }
    if record.intent.goal == "finished" {
        verify_finished_display(store, record, &proof, control)?;
    }
    control.check()?;
    Ok(DisplayOutcome {
        state: "cleared".into(),
        capture: reference.id,
        receipt: Some(proof.receipt),
    })
}

fn verify_finished_display(
    store: &impl ReadStore,
    record: &WorkRecord,
    proof: &DisplayCompletion,
    control: &OperationControl,
) -> Result<()> {
    let input = report_input(record, control)?;
    let captured = store.capture(&proof.checkpoint, PROOF_LIMIT, control)?;
    let reference = captured.reference;
    if !reference.complete
        || reference.truncated
        || reference.media_type != "application/json"
        || !provenance_matches(
            &reference.provenance,
            &checkpoint_provenance(record, &input, &proof.challenge),
        )
    {
        return Err(fault("live.finalization_provenance_mismatch"));
    }
    let signal = parse_signal(&captured.bytes, control)?;
    let mut expected = input.expected.clone();
    expected.kind = "checkpoint".into();
    expected.probe_nonce.clone_from(&proof.challenge);
    expected.after_sequence = 0;
    expected.runtime_epoch = proof.runtime_epoch;
    if proof.challenge.len() != 32
        || proof.runtime_epoch == 0
        || signal.work_state != "finished"
        || !signal.resources_released
    {
        return Err(fault("live.finalization_incomplete"));
    }
    validate_checkpoint(&signal, &input, &expected, control)?;
    let observed: ReportObservation = decode_record(&record.observation.0, control)?;
    let report = acknowledged_report(store, record, &input, &observed, control)?;
    // validate_checkpoint already established Some(load), before the report or
    // nested ACK was inspected. Avoid panicking even if that invariant changes.
    let load = input
        .load
        .as_ref()
        .ok_or_else(|| fault("live.checkpoint_identity_mismatch"))?;
    let ack = fill_signal_identity(
        parse_signal(signal.receipt.as_bytes(), control)?,
        &SignalIdentity {
            release: input.expected.release.clone(),
            product: input.expected.product.clone(),
            build: input.expected.build.clone(),
            character: input.expected.character.clone(),
            realm: input.expected.realm.clone(),
            guid: load.guid.clone(),
            session_nonce: input.expected.session_nonce.clone(),
            ..SignalIdentity::default()
        },
    );
    let mut expected = input.expected.clone();
    expected.kind = "acknowledged".into();
    expected.after_sequence = report.receipt.sequence;
    // Unlike acknowledged_report, fixed Go retains RequireInputReady here.
    ack.matches(&expected)?;
    if !same_report_digests(&ack, &report.receipt) {
        return Err(fault("live.finalization_report_mismatch"));
    }
    let captured = store.capture(&proof.frame, FRAME_LIMIT, control)?;
    let reference = captured.reference;
    let want = provenance(
        "wgc-finished-display",
        &proof.checkpoint,
        &record.intent.snapshot,
        &record.operation_id,
        "",
        &record.intent.session,
    );
    if !reference.complete
        || reference.truncated
        || reference.media_type != "image/png"
        || !provenance_matches(&reference.provenance, &want)
    {
        return Err(fault("live.finalization_frame_mismatch"));
    }
    png_decode_config(&captured.bytes, control)
}

fn validate_checkpoint(
    signal: &Signal,
    input: &ReportIntent,
    expected: &SignalExpectation,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    signal.matches(expected)?;
    let Some(load) = &input.load else {
        return Err(fault("live.checkpoint_identity_mismatch"));
    };
    if signal.guid != load.guid {
        return Err(fault("live.checkpoint_identity_mismatch"));
    }
    let mut hasher = Sha256::new();
    for chunk in input.code.bytes().chunks(65536) {
        control.check()?;
        hasher.update(chunk);
    }
    if signal.code_sha256 != hex::encode(hasher.finalize()) {
        return Err(fault("live.checkpoint_identity_mismatch"));
    }
    control.check()
}

fn acknowledged_report(
    store: &impl ReadStore,
    record: &WorkRecord,
    input: &ReportIntent,
    observed: &ReportObservation,
    control: &OperationControl,
) -> Result<VerifiedReport> {
    control.check()?;
    if observed.schema != "lycheedev.report-observation.v1"
        || observed.source_path.is_empty()
        || observed.source_sha256.is_empty()
        || observed.acknowledgement_id.is_empty()
    {
        return Err(fault("live.removal_evidence_missing"));
    }
    let report = super::status::verified_report(
        store,
        input,
        record,
        &observed.body_id,
        &observed.receipt_id,
        control,
    )?;
    let captured = store.capture(&observed.acknowledgement_id, PROOF_LIMIT, control)?;
    let reference = captured.reference;
    let want = provenance(
        "decoded-game-acknowledgement",
        &input.expected.request_id,
        &record.intent.snapshot,
        &record.operation_id,
        &input.expected.build,
        &input.expected.session_nonce,
    );
    if !provenance_matches(&reference.provenance, &want)
        || !reference.complete
        || reference.truncated
        || reference.media_type != "application/json"
    {
        return Err(fault("live.acknowledgement_provenance_mismatch"));
    }
    // The durable ACK is not identity-filled in fixed Go. Only the checkpoint's
    // nested ACK has that behavior; preserve this meaningful distinction.
    let ack = parse_signal(&captured.bytes, control)?;
    let mut expected = input.expected.clone();
    expected.kind = "acknowledged".into();
    expected.after_sequence = report.receipt.sequence;
    expected.require_input_ready = false;
    ack.matches(&expected)?;
    if !same_report_digests(&ack, &report.receipt) {
        return Err(fault("live.acknowledgement_payload_mismatch"));
    }
    control.check()?;
    Ok(report)
}

fn same_report_digests(ack: &Signal, receipt: &Signal) -> bool {
    ack.code_bytes == receipt.code_bytes
        && ack.code_adler32 == receipt.code_adler32
        && ack.report_bytes == receipt.report_bytes
        && ack.report_adler32 == receipt.report_adler32
}

// Go1.27.1 src/image/png/reader.go DecodeConfig and its config-only chunk path.
// Source SHA256: f6e97084ce561f92ae7658b34f16d797aa46c65325b2ae9cd931d0f2434993c1.
// This is deliberately not an image decoder: Go stops after IHDR for non-
// palette images, or after tRNS / the IDAT header for paletted ones. Requiring
// IDAT data, IEND, decompression or pixel allocation changes historical parity.
fn png_decode_config(raw: &[u8], control: &OperationControl) -> Result<()> {
    fn invalid(message: &str) -> ToolkitError {
        fault(&format!("png: invalid format: {message}"))
    }
    fn unsupported(message: &str) -> ToolkitError {
        fault(&format!("png: unsupported feature: {message}"))
    }
    fn take<'a>(raw: &'a [u8], at: &mut usize, count: usize) -> Result<&'a [u8]> {
        let end = at
            .checked_add(count)
            .ok_or_else(|| fault("unexpected EOF"))?;
        let bytes = raw.get(*at..end).ok_or_else(|| fault("unexpected EOF"))?;
        *at = end;
        Ok(bytes)
    }
    control.check()?;
    let mut at = 0usize;
    if take(raw, &mut at, 8)? != b"\x89PNG\r\n\x1a\n" {
        return Err(invalid("not a PNG file"));
    }
    // stage 0=start, 1=IHDR, 2=PLTE. tRNS or IDAT completes palette config.
    let mut stage = 0u8;
    let mut depth = 0u8;
    let mut palette = false;
    loop {
        control.check()?;
        let header = take(raw, &mut at, 8)?;
        let length = u32::from_be_bytes(header[..4].try_into().expect("four-byte PNG length"));
        let kind = &header[4..];
        let data;
        let complete;
        match kind {
            b"IHDR" => {
                if stage != 0 {
                    return Err(invalid("chunk out of order"));
                }
                stage = 1;
                if length != 13 {
                    return Err(invalid("bad IHDR length"));
                }
                data = take(raw, &mut at, 13)?;
                if data[10] != 0 {
                    return Err(unsupported("compression method"));
                }
                if data[11] != 0 {
                    return Err(unsupported("filter method"));
                }
                if data[12] > 1 {
                    return Err(invalid("invalid interlace method"));
                }
                let width = i32::from_be_bytes(data[..4].try_into().expect("PNG width"));
                let height = i32::from_be_bytes(data[4..8].try_into().expect("PNG height"));
                if width <= 0 || height <= 0 {
                    return Err(invalid("non-positive dimension"));
                }
                // The fixed release is Windows amd64: Go int is signed 64-bit.
                if i64::from(width) * i64::from(height) > i64::MAX / 8 {
                    return Err(unsupported("dimension overflow"));
                }
                depth = data[8];
                let color = data[9];
                if !match depth {
                    1 | 2 | 4 => matches!(color, 0 | 3),
                    8 => matches!(color, 0 | 2 | 3 | 4 | 6),
                    16 => matches!(color, 0 | 2 | 4 | 6),
                    _ => false,
                } {
                    return Err(unsupported(&format!(
                        "bit depth {depth}, color type {color}"
                    )));
                }
                palette = color == 3;
                complete = !palette;
            }
            b"PLTE" => {
                if stage != 1 {
                    return Err(invalid("chunk out of order"));
                }
                stage = 2;
                let entries = length / 3;
                if length % 3 != 0 || entries == 0 || entries > 256 || entries > 1u32 << depth {
                    return Err(invalid("bad PLTE length"));
                }
                data = take(raw, &mut at, length as usize)?;
                complete = false;
            }
            b"tRNS" => {
                // No non-palette IHDR reaches this point: it already returned.
                if !palette || stage != 2 {
                    return Err(invalid("chunk out of order"));
                }
                if length > 256 {
                    return Err(invalid("bad tRNS length"));
                }
                data = take(raw, &mut at, length as usize)?;
                complete = true;
            }
            b"IDAT" => {
                if stage != 2 || !palette {
                    return Err(invalid("chunk out of order"));
                }
                // Go config mode intentionally does not read data or CRC here,
                // even when the declared IDAT length exceeds the PNG limit.
                return control.check();
            }
            b"IEND" => return Err(invalid("chunk out of order")),
            _ => {
                // Unknown chunks are ignored, including critical ones, and may
                // precede IHDR. Match Go rather than tightening PNG admission.
                if length > 0x7fff_ffff {
                    return Err(invalid(&format!("Bad chunk length: {length}")));
                }
                data = take(raw, &mut at, length as usize)?;
                complete = false;
            }
        }
        let checksum = take(raw, &mut at, 4)?;
        if u32::from_be_bytes(checksum.try_into().expect("four-byte PNG CRC"))
            != png_crc(kind, data, control)?
        {
            return Err(invalid("invalid checksum"));
        }
        if complete {
            return control.check();
        }
    }
}

fn png_crc(kind: &[u8], bytes: &[u8], control: &OperationControl) -> Result<u32> {
    const fn table() -> [u32; 256] {
        let mut out = [0; 256];
        let mut index = 0;
        while index < 256 {
            let mut crc = index as u32;
            let mut bit = 0;
            while bit < 8 {
                crc = if crc & 1 != 0 {
                    0xedb8_8320 ^ (crc >> 1)
                } else {
                    crc >> 1
                };
                bit += 1;
            }
            out[index] = crc;
            index += 1;
        }
        out
    }
    const TABLE: [u32; 256] = table();
    let mut crc = u32::MAX;
    for part in [kind, bytes] {
        for chunk in part.chunks(65536) {
            control.check()?;
            for byte in chunk {
                crc = TABLE[((crc ^ u32::from(*byte)) & 255) as usize] ^ (crc >> 8);
            }
        }
    }
    Ok(!crc)
}

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;
