//! Private same-executable one-shot control. Session and batch retain their own
//! 90-second wire contracts. This is diagnostics/control, not a semantic shortcut.
use super::*;
use std::{
    io::Write,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ENV: &str = "LYCHEEDEV_NATIVE_ONESHOT_CONTROL";
const WIRE: &str = "lycheedev.native-one-shot-control.v3";
const PHASE_SCHEMA: &str = "lycheedev.native-one-shot-phase.v1";
const LIMIT_MS: u64 = 180_000;
const RECORD_LIMIT: usize = 128;
const BYTE_LIMIT: u64 = 64 * 1024;
const DEADLINE_MARKER: &[u8] =
    b"{\"schema\":\"lycheedev.native-one-shot-stop.v1\",\"reason\":\"deadline\"}";

// Reserved only for this same-executable private worker, never a public exit.
const DEADLINE_EXIT: i32 = 85;
pub(super) fn report_stop(mut error: ToolkitError) -> ToolkitError {
    if error.code == "command.deadline" {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr
            .write_all(DEADLINE_MARKER)
            .and_then(|_| stderr.write_all(b"\n"));
        error.exit_code = DEADLINE_EXIT;
    }
    error
}
pub(super) fn stopped_at_deadline(status: i32, stderr: &[u8]) -> bool {
    // A source/third-party error can print marker-looking text, but cannot select
    // this reserved exit. Require exactly one complete canonical marker too.
    status == DEADLINE_EXIT
        && stderr
            .split_inclusive(|byte| *byte == b'\n')
            .filter(|line| line.strip_suffix(b"\n") == Some(DEADLINE_MARKER))
            .count()
            == 1
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    schema: String,
    sent_unix_ms: u64,
    deadline_unix_ms: u64,
    phase_profile: bool,
}
fn fault() -> ToolkitError {
    ToolkitError::new(
        "source.native_worker_protocol",
        "Invalid private one-shot control",
        4,
    )
}
fn utc_ms() -> Result<u64> {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| fault())?
            .as_millis(),
    )
    .map_err(|_| fault())
}
fn remaining(envelope: &Envelope, now: u64) -> Result<Duration> {
    if envelope.schema != WIRE
        || envelope.sent_unix_ms == 0
        || envelope.deadline_unix_ms < envelope.sent_unix_ms
        || envelope.deadline_unix_ms - envelope.sent_unix_ms > LIMIT_MS
    {
        return Err(fault());
    }
    if now < envelope.sent_unix_ms {
        return Ok(Duration::ZERO);
    }
    Ok(Duration::from_millis(
        envelope.deadline_unix_ms.saturating_sub(now),
    ))
}
/// UTC is a transport clock, as in session IPC, never the parent's enforcement
/// clock. Sample it before remaining() and sample child Instant before UTC so
/// ordinary transport/launch/read delays cannot grant a fresh duration. Reject
/// backward clock movement; the parent's original monotonic hard stop remains
/// authoritative even if the system clock changes after these samples.
pub(super) fn receive_control() -> Result<OperationControl> {
    let started = Instant::now();
    let raw = std::env::var(ENV).map_err(|_| fault())?;
    if raw.len() > 4096 {
        return Err(fault());
    }
    let envelope: Envelope = serde_json::from_str(&raw).map_err(|_| fault())?;
    let left = remaining(&envelope, utc_ms()?)?;
    let control = OperationControl::uncancelable(started + left);
    control.check()?;
    Ok(control)
}
pub(super) fn configure(
    command: &mut Command,
    control: &OperationControl,
) -> Result<Option<crate::process::StderrObservation>> {
    let phase_profile =
        std::env::var_os("LYCHEEDEV_SOURCE_PHASE_PROFILE").is_some_and(|value| value == "1");
    let observation = phase_profile.then(crate::process::StderrObservation::new);
    let now = utc_ms()?;
    let budget = control.remaining()?.min(Duration::from_millis(LIMIT_MS));
    let reserve = Duration::from_millis(500).min(budget / 4);
    let ms = u64::try_from((budget - reserve).as_millis()).map_err(|_| fault())?;
    let envelope = Envelope {
        schema: WIRE.into(),
        sent_unix_ms: now,
        deadline_unix_ms: now.checked_add(ms).ok_or_else(fault)?,
        phase_profile,
    };
    command.env(ENV, serde_json::to_string(&envelope)?);
    Ok(observation)
}
fn allowed(phase: &str) -> bool {
    matches!(
        phase,
        "worker_started"
            | "request_decoded"
            | "prepare_started"
            | "api_definitions_verified"
            | "xml_prepared"
            | "host_prepared"
            | "source_admitted"
            | "emmy_indexed"
            | "source_identity_indexed"
            | "source_syntax_indexed"
            | "prepared"
            | "query_inputs_renewed"
            | "query_operations_planned"
            | "query_build_scope"
            | "query_scope_built"
            | "query_build_entry"
            | "query_execute_operations"
            | "query_operation_authenticate"
            | "query_references_project"
            | "query_relations_target_declarations"
            | "query_relations_target_values"
            | "query_relations_use_index"
            | "query_relations_use_sort"
            | "query_relations_source_rows"
            | "query_relations_file_syntax"
            | "query_relations_reference_rows"
            | "query_relations_file_complete"
            | "query_relations_xml_rows"
            | "query_relations_control_json"
            | "query_relations_assemble"
            | "query_references_digest"
            | "query_references_retain"
            | "query_definition_resolve"
            | "query_type_api"
            | "query_type_observations"
            | "query_operation_answer"
            | "query_execution_identity"
            | "query_operations_executed"
            | "query_scope_evidence"
            | "query_project"
            | "query_projected"
            | "query_assemble_reply"
            | "query_reply_assembled"
            | "queried"
            | "reply_serialized"
            | "reply_written"
            | "reply_values_released"
            | "worker_complete"
            | "worker_error"
    )
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    schema: String,
    phase: String,
    elapsed_ms: u64,
    phase_ms: u64,
    omitted: usize,
}
pub(super) struct Phases {
    enabled: bool,
    started: Instant,
    last: Instant,
    count: usize,
    bytes: u64,
    omitted: usize,
}
impl Phases {
    pub(super) fn from_environment() -> Self {
        let enabled = std::env::var(ENV)
            .ok()
            .filter(|raw| raw.len() <= 4096)
            .and_then(|raw| serde_json::from_str::<Envelope>(&raw).ok())
            .is_some_and(|wire| wire.schema == WIRE && wire.phase_profile);
        let started = Instant::now();
        Self {
            enabled,
            started,
            last: started,
            count: 0,
            bytes: 0,
            omitted: 0,
        }
    }
    pub(super) fn record(&mut self, phase: &str) {
        // Fine allocation stages belong to the retained child's private journal.
        // Do not consume this original coarse-only stderr profiling allowance.
        if !self.enabled || super::allocation_phase(phase) || !allowed(phase) {
            return;
        }
        let terminal = matches!(phase, "worker_complete" | "worker_error");
        if self.count >= RECORD_LIMIT - 1 && !terminal {
            self.omitted += 1;
            return;
        }
        if self.count >= RECORD_LIMIT {
            return;
        }
        let now = Instant::now();
        let event = Event {
            schema: PHASE_SCHEMA.into(),
            phase: phase.into(),
            elapsed_ms: now.duration_since(self.started).as_millis() as u64,
            phase_ms: now.duration_since(self.last).as_millis() as u64,
            omitted: self.omitted,
        };
        let Ok(mut bytes) = serde_json::to_vec(&event) else {
            return;
        };
        bytes.push(b'\n');
        if self.bytes + bytes.len() as u64 > BYTE_LIMIT {
            return;
        }
        self.bytes += bytes.len() as u64;
        self.count += 1;
        self.last = now;
        // Only the already-owned stderr pipe receives diagnostics. No child
        // path reopening or file-writing authority exists.
        let _ = std::io::stderr().write_all(&bytes);
    }
}
fn canonical_phase(line: &[u8]) -> Option<Vec<u8>> {
    let event: Event = serde_json::from_slice(line).ok()?;
    if event.schema != PHASE_SCHEMA || !allowed(&event.phase) {
        return None;
    }
    let mut bytes = serde_json::to_vec(&event).ok()?;
    if bytes != line {
        return None;
    }
    bytes.push(b'\n');
    Some(bytes)
}
fn phase_records(bytes: &[u8]) -> impl Iterator<Item = Vec<u8>> + '_ {
    bytes[..bytes.len().min(BYTE_LIMIT as usize)]
        .split_inclusive(|byte| *byte == b'\n')
        .filter_map(|chunk| chunk.strip_suffix(b"\n").and_then(canonical_phase))
        .take(RECORD_LIMIT)
}
pub(super) fn replay(observation: Option<crate::process::StderrObservation>) {
    let Some(observation) = observation else {
        return;
    };
    // Never wait for an unretired reader or acquire its mutex with lock().
    let crate::process::StderrSnapshot { bytes, state } = observation.snapshot();
    let mut records = 0;
    for safe in phase_records(&bytes) {
        let _ = std::io::stderr().write_all(&safe);
        records += 1;
    }
    // Fixed metadata honestly distinguishes a prefix from complete output;
    // observation failure never alters the primary process result/error.
    let state = json!({"schema":"lycheedev.native-one-shot-capture.v1","state":state,"bytes":bytes.len(),"replayedRecords":records});
    let _ = writeln!(std::io::stderr(), "{state}");
}

#[cfg(test)]
mod tests {
    use super::*;
    fn envelope() -> Envelope {
        Envelope {
            schema: WIRE.into(),
            sent_unix_ms: 1000,
            deadline_unix_ms: 11_000,
            phase_profile: false,
        }
    }
    #[test]
    fn transport_time_is_debited_and_expiry_is_not_reset() {
        assert_eq!(
            remaining(&envelope(), 4500).unwrap(),
            Duration::from_millis(6500)
        );
        assert_eq!(remaining(&envelope(), 11_001).unwrap(), Duration::ZERO);
        assert_eq!(remaining(&envelope(), 999).unwrap(), Duration::ZERO);
    }
    #[test]
    fn malformed_or_expanded_control_is_rejected() {
        let mut wire = envelope();
        wire.deadline_unix_ms = 181_001;
        assert!(remaining(&wire, 1000).is_err());
        wire.deadline_unix_ms = 999;
        assert!(remaining(&wire, 1000).is_err());
        wire = envelope();
        wire.schema = "other".into();
        assert!(remaining(&wire, 1000).is_err());
    }
    #[test]
    fn only_fixed_metadata_can_be_replayed() {
        assert!(allowed("emmy_indexed"));
        assert!(!allowed("source.lua"));
        assert!(serde_json::from_str::<Event>(r#"{"schema":"x","phase":"emmy_indexed","elapsed_ms":1,"phase_ms":1,"omitted":0,"source":"private"}"#).is_err());
    }
    #[test]
    fn allocation_boundaries_are_canonical_and_keep_existing_caps() {
        for phase in [
            "query_operation_authenticate",
            "query_references_project",
            "query_relations_target_declarations",
            "query_relations_target_values",
            "query_relations_use_index",
            "query_relations_use_sort",
            "query_relations_source_rows",
            "query_relations_file_syntax",
            "query_relations_reference_rows",
            "query_relations_file_complete",
            "query_relations_xml_rows",
            "query_relations_control_json",
            "query_relations_assemble",
            "query_references_digest",
            "query_references_retain",
            "query_definition_resolve",
            "query_type_api",
            "query_type_observations",
            "query_operation_answer",
            "query_execution_identity",
            "query_operations_executed",
            "query_scope_evidence",
        ] {
            assert!(allowed(phase));assert!(super::super::allocation_phase(phase));
            let now=Instant::now();
            let mut coarse=Phases{enabled:true,started:now,last:now,count:0,bytes:0,omitted:0};
            coarse.record(phase);assert_eq!((coarse.count,coarse.bytes,coarse.omitted),(0,0,0));
            let event=Event{schema:PHASE_SCHEMA.into(),phase:phase.into(),elapsed_ms:u64::MAX,phase_ms:u64::MAX,omitted:usize::MAX};
            let bytes=serde_json::to_vec(&event).unwrap();
            assert!((bytes.len()+1)*RECORD_LIMIT<=BYTE_LIMIT as usize);
            let mut line=bytes.clone();line.push(b'\n');
            assert_eq!(canonical_phase(&bytes),Some(line));
        }
        assert!(!allowed("query_references_success"));
    }
    #[test]
    fn reply_boundaries_are_canonical_observations_inside_existing_caps() {
        for phase in ["reply_serialized","reply_written","reply_values_released"] {
            assert!(allowed(phase));assert!(super::super::allocation_phase(phase));
            let now=Instant::now();
            let mut coarse=Phases{enabled:true,started:now,last:now,count:0,bytes:0,omitted:0};
            coarse.record(phase);assert_eq!((coarse.count,coarse.bytes,coarse.omitted),(0,0,0));
            let event=Event{schema:PHASE_SCHEMA.into(),phase:phase.into(),elapsed_ms:u64::MAX,phase_ms:u64::MAX,omitted:usize::MAX};
            let bytes=serde_json::to_vec(&event).unwrap();
            assert!((bytes.len()+1)*RECORD_LIMIT<=BYTE_LIMIT as usize);
            let mut line=bytes.clone();line.push(b'\n');
            assert_eq!(canonical_phase(&bytes),Some(line.clone()));
            assert_eq!(phase_records(&line).collect::<Vec<_>>(),vec![line]);
        }
        assert!(!allowed("reply_acknowledged"));
    }
    #[test]
    fn private_control_accepts_unknown_fields_without_changing_request_schema() {
        let value = json!({"schema":WIRE,"sent_unix_ms":1000,"deadline_unix_ms":2000,"phase_profile":false,"future":true});
        let wire: Envelope = serde_json::from_value(value).unwrap();
        assert_eq!(remaining(&wire, 1500).unwrap(), Duration::from_millis(500));
    }
    #[test]
    fn deadline_mapping_requires_exact_marker_and_reserved_exit() {
        let mut complete = DEADLINE_MARKER.to_vec();
        complete.push(b'\n');
        assert!(stopped_at_deadline(DEADLINE_EXIT, &complete));
        for status in [0, 4, 5, 7, 84] {
            assert!(!stopped_at_deadline(status, &complete));
        }
        assert!(!stopped_at_deadline(DEADLINE_EXIT, b"command.deadline\n"));
        assert!(!stopped_at_deadline(DEADLINE_EXIT, DEADLINE_MARKER));
        let mut duplicate = complete.clone();
        duplicate.extend_from_slice(&complete);
        assert!(!stopped_at_deadline(DEADLINE_EXIT, &duplicate));
        let mut prefixed = b"source says ".to_vec();
        prefixed.extend_from_slice(&complete);
        assert!(!stopped_at_deadline(DEADLINE_EXIT, &prefixed));
        let mut truncated = complete.clone();
        truncated.remove(3);
        assert!(!stopped_at_deadline(DEADLINE_EXIT, &truncated));
    }
    #[test]
    fn maximum_metadata_remains_inside_the_fixed_byte_cap() {
        for phase in [
            "source_identity_indexed",
            "query_operations_planned",
            "worker_error",
        ] {
            let event = Event {
                schema: PHASE_SCHEMA.into(),
                phase: phase.into(),
                elapsed_ms: u64::MAX,
                phase_ms: u64::MAX,
                omitted: usize::MAX,
            };
            assert!(
                (serde_json::to_vec(&event).unwrap().len() + 1) * RECORD_LIMIT
                    <= BYTE_LIMIT as usize
            );
        }
    }
    #[test]
    fn replay_accepts_only_canonical_whitelisted_metadata() {
        let event = Event {
            schema: PHASE_SCHEMA.into(),
            phase: "emmy_indexed".into(),
            elapsed_ms: 1,
            phase_ms: 1,
            omitted: 0,
        };
        let encoded = serde_json::to_vec(&event).unwrap();
        assert!(canonical_phase(&encoded).is_some());
        let mut extra = encoded.clone();
        extra.insert(1, b' ');
        assert!(canonical_phase(&extra).is_none());
        let mut value = serde_json::to_value(&event).unwrap();
        value["source"] = json!("private");
        assert!(canonical_phase(&serde_json::to_vec(&value).unwrap()).is_none());
        assert!(canonical_phase(b"third party diagnostic").is_none());
    }
    #[test]
    fn replay_drops_partial_tail_and_obeys_record_bound() {
        let event = Event {
            schema: PHASE_SCHEMA.into(),
            phase: "emmy_indexed".into(),
            elapsed_ms: 1,
            phase_ms: 1,
            omitted: 0,
        };
        let encoded = serde_json::to_vec(&event).unwrap();
        assert_eq!(phase_records(&encoded).count(), 0);
        let mut complete = encoded.clone();
        complete.push(b'\n');
        let mut partial = complete.clone();
        partial.extend_from_slice(&encoded);
        assert_eq!(phase_records(&partial).count(), 1);
        assert_eq!(
            phase_records(&complete.repeat(RECORD_LIMIT + 1)).count(),
            RECORD_LIMIT
        );
        assert_eq!(BYTE_LIMIT, 64 * 1024);
    }
}
