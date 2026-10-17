//! Opt-in OFFLINE diagnostic. The real fixed-Go Lua peer computes BODY bytes.
//! Synthetic readiness/input clocks are never used to measure elapsed time.
//! No production profiling hook, arbitrary code, game, network or native input.
#![forbid(unsafe_code)]
#[path = "../../../tests/support/formal_lua_fixture.rs"]
mod formal_lua_fixture;
#[path = "../../../tests/support/formal_lua_peer.rs"]
mod formal_lua_peer;

use super::{present, present_owned};
use crate::{
    go_mailbox_v1::{
        Identity, InputObservation, Kind, MAX_PAYLOAD, checksum,
        driver::Driver,
        durable_journal::DurableChannelJournal,
        journal::{StateJournal, read_log_bytes, snapshot},
        observations::{
            Backend, ChannelError, ChannelResult, InputAction, InputDispatch, ObservationDispatch,
            ObservationQuery,
        },
        state::{CleanupMethod, Operation, OperationPolicy, OperationStage, SlotEnvelope, State},
    },
    mailbox::{decode, encode},
};
use formal_lua_fixture::{
    Case, FixtureClock, Inputs, PERFORMANCE_HISTORY_EVENTS, PERFORMANCE_REPEATS,
    PERFORMANCE_RESULT_STRING_BYTES, Tokens, performance_code, read, write_new,
};
use formal_lua_peer::{Peer, PeerOwner};
use lycheedev_core::{control::OperationControl, public_json::write_serializable_line, storage};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    time::{Duration, Instant},
};

fn nanos(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_nanos()).unwrap()
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Count {
    calls: u64,
    wall_nanos: u64,
}
#[derive(Default, Serialize)]
struct Meter(BTreeMap<&'static str, Count>);
impl Meter {
    fn add(&mut self, name: &'static str, elapsed: u64) {
        let value = self.0.entry(name).or_default();
        value.calls += 1;
        value.wall_nanos += elapsed;
    }
    fn total(&self) -> u64 {
        self.0.values().map(|value| value.wall_nanos).sum()
    }
}
#[derive(Default)]
struct Timeline(Vec<Value>);
impl Timeline {
    fn run<T>(&mut self, name: &'static str, action: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let result = action();
        self.0.push(json!({"name":name,"wallNanos":nanos(start)}));
        result
    }
    fn total(&self) -> u64 {
        self.0
            .iter()
            .map(|row| row["wallNanos"].as_u64().unwrap())
            .sum()
    }
    fn driver_total(&self) -> u64 {
        self.0
            .iter()
            .filter(|row| row["name"].as_str().unwrap().starts_with("driver_"))
            .map(|row| row["wallNanos"].as_u64().unwrap())
            .sum()
    }
}
struct MeasuredJournal {
    inner: DurableChannelJournal,
    meter: Meter,
}
impl StateJournal for MeasuredJournal {
    fn append(&mut self, c: &OperationControl, event: &str, state: &State) -> ChannelResult<()> {
        let start = Instant::now();
        let result = self.inner.append(c, event, state);
        self.meter.add("append", nanos(start));
        result
    }
    fn historical_request(
        &mut self,
        c: &OperationControl,
        request: &str,
    ) -> ChannelResult<Option<State>> {
        let start = Instant::now();
        let result = self.inner.historical_request(c, request);
        self.meter.add("historical_request", nanos(start));
        result
    }
    fn write_result(&mut self, c: &OperationControl, operation: &Operation) -> ChannelResult<()> {
        let start = Instant::now();
        let result = self.inner.write_result(c, operation);
        self.meter.add("write_result", nanos(start));
        result
    }
    fn checkpoint(
        &mut self,
        c: &OperationControl,
        state: &mut State,
        known_zero: bool,
    ) -> ChannelResult<()> {
        let start = Instant::now();
        let result = self.inner.checkpoint(c, state, known_zero);
        self.meter.add("checkpoint", nanos(start));
        result
    }
}
struct MeasuredPeer {
    inner: Peer,
    meter: Meter,
}
impl Backend for MeasuredPeer {
    fn observe(&mut self, c: &OperationControl, q: &ObservationQuery) -> ObservationDispatch {
        let start = Instant::now();
        let result = self.inner.observe(c, q);
        self.meter.add("observe", nanos(start));
        result
    }
    fn publish(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        let start = Instant::now();
        let result = self.inner.publish(c, e);
        self.meter.add("publish", nanos(start));
        result
    }
    fn observe_input(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        after: i64,
        capability: &str,
    ) -> ChannelResult<InputObservation> {
        let start = Instant::now();
        let result = self.inner.observe_input(c, e, after, capability);
        self.meter.add("observe_input", nanos(start));
        result
    }
    fn input(&mut self, c: &OperationControl, a: &InputAction) -> InputDispatch {
        let start = Instant::now();
        let result = self.inner.input(c, a);
        self.meter.add("input", nanos(start));
        result
    }
    fn runtime_candidate(
        &mut self,
        c: &OperationControl,
        i: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        let start = Instant::now();
        let result = self.inner.runtime_candidate(c, i);
        self.meter.add("runtime_candidate", nanos(start));
        result
    }
    fn supersede(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        i: &Identity,
    ) -> ChannelResult<()> {
        let start = Instant::now();
        let result = self.inner.supersede(c, e, i);
        self.meter.add("supersede", nanos(start));
        result
    }
    fn consumed(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        let start = Instant::now();
        let result = self.inner.consumed(c, e);
        self.meter.add("consumed", nanos(start));
        result
    }
}
type TestDriver = Driver<MeasuredPeer, MeasuredJournal, FixtureClock, Tokens>;
fn log(case: &Case) -> (Vec<u8>, usize) {
    let bytes = read(case.paths.log_path(), 64 << 20);
    let parsed = read_log_bytes(&bytes);
    assert!(parsed.failure.is_none(), "complete checksum/sequence chain");
    let count = parsed.events.len();
    (bytes, count)
}
fn wire(value: &impl Serialize) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_serializable_line(&mut bytes, value).unwrap();
    bytes
}
fn verify_result(case: &Case, state: &State, payload_bytes: usize) -> Value {
    let operation = state.operation.as_ref().unwrap();
    assert_eq!(operation.stage, OperationStage::Complete);
    assert_eq!(operation.cleanup_method, CleanupMethod::Released);
    assert_eq!(
        operation.report_bytes as usize,
        operation.result_bytes.0.len()
    );
    assert_eq!(
        operation.report_checksum,
        checksum(&operation.result_bytes.0)
    );
    assert!(operation.result_bytes.0.len() <= MAX_PAYLOAD);
    let report: Value = serde_json::from_slice(&operation.result_bytes.0).unwrap();
    assert_eq!(report["result"]["message"], "荔枝\n\u{0}123");
    assert_eq!(report["result"]["quoted"], "x\\y");
    let payload = report["result"]["payload"].as_str().unwrap().as_bytes();
    assert_eq!(payload.len(), payload_bytes);
    assert!(payload.iter().all(|byte| *byte == b'x'));
    let saved = read(
        &case
            .paths
            .result_directory()
            .join(format!("{}.json", operation.id)),
        2 << 20,
    );
    let saved: Operation = serde_json::from_slice(&saved).unwrap();
    assert_eq!(saved.id, operation.id);
    assert_eq!(saved.ticket, operation.ticket);
    assert_eq!(saved.request, operation.request);
    assert_eq!(saved.stage, OperationStage::ResultVerified);
    assert_eq!(saved.result_bytes, operation.result_bytes);
    assert_eq!(saved.report_checksum, operation.report_checksum);
    assert_eq!(read(&case.records.join("executions.txt"), 16), b"1");
    json!({"bodyPayloadBytes":operation.result_bytes.0.len(),"bodySHA256":storage::sha256(&operation.result_bytes.0),"requestedResultStringBytes":payload_bytes})
}
fn sample(inputs: &Inputs, payload_bytes: usize, history: usize, repeat: usize) -> Value {
    let start = Instant::now();
    let mut timeline = Timeline::default();
    let case = timeline.run("fixture_create", || {
        inputs.performance_case(payload_bytes, history, repeat)
    });
    // The original formal9 per-case execution bound is unchanged. It includes
    // real journal seeding, peer startup and all work; cleanup keeps its own grace.
    let control =
        OperationControl::new(Instant::now() + Duration::from_secs(20), Default::default());
    let mut driver: Option<TestDriver> = None;
    let mut pending_journal: Option<MeasuredJournal> = None;
    let mut owner: Option<PeerOwner> = None;
    let mut launch_intent = false;
    // Preserve the FIRST checked-close outcome: close consumes its lease even
    // on Err, so a second close must never turn that uncertainty into Ok.
    let mut journal_close_attempt: Option<ChannelResult<()>> = None;
    let mut measurements = None;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        timeline.run("journal_acquire", || {
            pending_journal = Some(MeasuredJournal {
                inner: case.open(&control),
                meter: Meter::default(),
            })
        });
        let journal = &mut pending_journal.as_mut().unwrap().inner;
        let mut state = timeline.run("history_seed", || {
            let state = journal.load(&control).unwrap();
            for _ in 1..history {
                journal.append(&control, "budget_observed", &state).unwrap();
            }
            state
        });
        let (before, before_events) = timeline.run("history_verify", || log(&case));
        assert_eq!(before_events, history);
        let loaded = timeline.run("journal_load_at_history", || {
            journal.load(&control).unwrap()
        });
        timeline.run("history_state_assert", || {
            assert_eq!(wire(&loaded), wire(&state))
        });
        timeline.run("journal_checkpoint_at_history", || {
            journal.checkpoint(&control, &mut state, false).unwrap()
        });
        let (after, after_events) = timeline.run("checkpoint_verify", || log(&case));
        assert_eq!(after_events, if history >= 256 { 1 } else { history });
        if history < 256 {
            assert_eq!(before, after);
            assert!(state.archive.is_empty());
        } else {
            assert!(!state.archive.is_empty());
            let archive = case
                .paths
                .history_directory()
                .join(format!("{}.jsonl", state.archive));
            assert_eq!(
                read(&archive, 64 << 20),
                before,
                "checkpoint must retain every original event byte"
            );
            assert_eq!(wire(&journal.load(&control).unwrap()), wire(&state));
        }
        timeline.run("journal_append_after_checkpoint", || {
            journal.append(&control, "budget_observed", &state).unwrap()
        });
        assert_eq!(log(&case).1, after_events + 1);
        launch_intent = true;
        let peer = timeline.run("peer_startup", || {
            Peer::start_performance(
                inputs,
                &case.records,
                case.paths.clone(),
                payload_bytes,
                &control,
            )
        });
        owner = Some(peer.owner());
        driver = Some(timeline.run("driver_construct", || {
            Driver::from_state(
                state,
                MeasuredPeer {
                    inner: peer,
                    meter: Meter::default(),
                },
                pending_journal.take().unwrap(),
                FixtureClock,
                Tokens(16),
            )
            .unwrap()
        }));
        let d = driver.as_mut().unwrap();
        timeline.run("driver_connect", || d.connect(&control).unwrap());
        let code = performance_code(payload_bytes);
        timeline.run("driver_prepare", || {
            d.prepare_request(
                &control,
                "offline-performance",
                &code,
                10,
                OperationPolicy::Observation,
            )
            .unwrap()
        });
        let original_request = d.state.operation.as_ref().unwrap().request.clone();
        let original_ticket = d.state.operation.as_ref().unwrap().ticket.clone();
        timeline.run("driver_run", || d.run(&control).unwrap());
        let body = timeline.run("terminal_correctness", || {
            verify_result(&case, &d.state, payload_bytes)
        });
        assert_eq!(
            d.state.operation.as_ref().unwrap().request,
            original_request
        );
        assert_eq!(d.state.operation.as_ref().unwrap().ticket, original_ticket);
        assert_eq!(d.backend.inner.release_checks, 1);
        let sends = d.backend.inner.sends.clone();
        assert_eq!(
            sends.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(),
            ["bind", "prepare", "commit", "confirm", "release"]
        );
        assert_eq!(
            sends
                .iter()
                .map(|e| e.nonce.as_str())
                .collect::<BTreeSet<_>>()
                .len(),
            sends.len()
        );
        timeline.run("driver_completed_retry", || d.run(&control).unwrap());
        assert_eq!(
            d.backend.inner.sends, sends,
            "completed retry must not replay input"
        );
        timeline.run("driver_disconnect", || d.disconnect(&control).unwrap());
        // Driver unbinds; ConnectionService owns final connection retirement.
        assert!(!d.state.closed && !d.state.bound && d.state.transaction.is_none());
        assert_eq!(d.backend.inner.sends.last().unwrap().action, "unbind");
        assert_eq!(d.backend.inner.sends.len(), 6);
        // Complete snapshot and production codec diagnostics use real terminal
        // data. They are disjoint extra phases, not charged again to Driver.
        let snap = timeline.run("state_snapshot", || snapshot(&d.state).unwrap());
        let copied: usize = snap.artifacts.values().map(Vec::len).sum();
        let capacity: usize = snap.artifacts.values().map(Vec::capacity).sum();
        assert_eq!(
            copied,
            code.as_bytes().len() + body["bodyPayloadBytes"].as_u64().unwrap() as usize
        );
        let state_bytes = timeline.run("state_serialize", || wire(&d.state));
        let state_roundtrip: State = timeline.run("state_deserialize", || {
            serde_json::from_slice(&state_bytes).unwrap()
        });
        timeline.run("state_roundtrip_assert", || {
            assert_eq!(wire(&state_roundtrip), state_bytes)
        });
        let mut paths: Vec<_> = fs::read_dir(&case.records)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("record-")
            })
            .collect();
        paths.sort();
        assert!(paths.len() <= 64);
        let records = timeline.run("codec_fixture_read", || {
            paths
                .iter()
                .map(|p| read(p, MAX_PAYLOAD + 120))
                .collect::<Vec<_>>()
        });
        let decoded = timeline.run("codec_decode", || {
            records
                .iter()
                .map(|bytes| decode(bytes).unwrap())
                .collect::<Vec<_>>()
        });
        let codec_copy: usize = decoded.iter().map(|record| record.payload.len()).sum();
        let codec_capacity: usize = decoded.iter().map(|record| record.payload.capacity()).sum();
        timeline.run("codec_correctness", || {
            let bodies: Vec<_> = decoded
                .iter()
                .filter(|r| r.kind == Kind::Body as u8)
                .collect();
            assert_eq!(bodies.len(), 1);
            assert_eq!(
                bodies[0].payload,
                d.state.operation.as_ref().unwrap().result_bytes.0
            );
            for (bytes, record) in records.iter().zip(&decoded) {
                assert_eq!(encode(record).unwrap(), *bytes);
            }
            let mut corrupt = records[0].clone();
            corrupt[0] ^= 1;
            assert!(decode(&corrupt).is_err());
        });
        let borrowed = timeline.run("terminal_projection_borrowed", || {
            present(&d.state, case.paths.log_path(), 1000, "", "")
        });
        let expected = timeline.run("terminal_serialize_borrowed", || wire(&borrowed));
        let report = &d.state.operation.as_ref().unwrap().result_bytes.0;
        let pointer = report.as_ptr();
        let report_capacity = report.capacity();
        timeline.run("journal_checked_close", || {
            let result = d.journal.inner.close();
            journal_close_attempt = Some(result.clone());
            result.unwrap();
        });
        let d = driver.take().unwrap();
        let journal_wall = d.journal.meter.total();
        let backend_wall = d.backend.meter.total();
        let exchange_wall = d.backend.inner.exchange_wall_nanos;
        assert_eq!(d.backend.inner.exchange_calls, 6);
        assert!(exchange_wall <= backend_wall);
        let meters = json!({"journal":d.journal.meter,"backend":d.backend.meter});
        let peer_records = json!({"readBytes":d.backend.inner.record_read_bytes, "decodeCalls":d.backend.inner.record_decode_calls, "decodeWallNanos":d.backend.inner.record_decode_wall_nanos, "payloadCopyBytes":d.backend.inner.record_payload_copy_bytes, "includedIn":"driverPartitionNanos.backendIncludingPeerExchange", "scope":"actual offline Peer record rereads/production codec, including subsequently rejected stale records; not native process-memory IO"});
        assert!(d.backend.inner.record_decode_wall_nanos <= backend_wall);
        let projected = timeline.run("terminal_projection_owned", || {
            present_owned(d.state, case.paths.log_path(), 1000, "", "")
        });
        let moved = &projected.report.as_ref().unwrap().0;
        assert_eq!(moved.as_ptr(), pointer);
        assert_eq!(moved.capacity(), report_capacity);
        let output = timeline.run("terminal_serialize_owned", || wire(&projected));
        timeline.run("projection_exact_assert", || assert_eq!(output, expected));
        let driver_wall = timeline.driver_total();
        assert!(journal_wall + backend_wall <= driver_wall);
        measurements = Some(
            json!({"body":body,"history":{"inputEvents":history,"inputLogBytes":before.len(),"afterCheckpointEvents":after_events,"afterCheckpointLogBytes":after.len(),"appendInputEvents":after_events},
            "driverPartitionNanos":{"journal":journal_wall,"backendIncludingPeerExchange":backend_wall,"driverAndMeasurementRemainder":driver_wall-journal_wall-backend_wall,"total":driver_wall},
            "nestedPeerExchange":{"wallNanos":exchange_wall,"calls":6,"includedIn":"driverPartitionNanos.backendIncludingPeerExchange","meaning":"offline Lua execution, pipes, scheduling and fixture file IO; not pure wait or game latency"},"calls":meters,"nestedPeerRecords":peer_records,
            "ownedBytes":{"snapshotArtifactCount":snap.artifacts.len(),"snapshotArtifactCopyBytes":copied,"snapshotArtifactCapacityBytes":capacity,"codecPayloadCopyBytes":codec_copy,"codecPayloadCapacityBytes":codec_capacity,"codecPayloadVectors":decoded.len(),"ownedProjectionReportCopyBytes":0,"ownedProjectionReportCapacityBytes":report_capacity,"borrowedProjectionReportCopyBytes":moved.len(),"stateSerializedBytes":state_bytes.len(),"terminalSerializedBytes":output.len(),"totalAllocatorCalls":null,"allocatorRequestedBytes":null,"peakRSS":null},
            "checks":{"exactReport":true,"wireRoundtrip":true,"checksumSequence":true,"checkpointBoundary":true,"persistBeforeRelease":true,"noReplay":true,"sixOrderedInputs":true,"ownedReportPointerCapacity":true,"ownedBorrowedExactWire":true,"malformedCodecRejected":true}}),
        );
    }));
    let peer_cleanup = timeline.run("peer_checked_cleanup", || match owner.as_mut() {
        Some(owner) => owner.close(outcome.is_ok()),
        None if !launch_intent => Ok(()),
        None => Err("peer launch did not return owner; retain fixture".into()),
    });
    let journal_cleanup = timeline.run("journal_failure_cleanup", || {
        if let Some(original) = &journal_close_attempt {
            original.clone()
        } else if let Some(d) = driver.as_mut() {
            d.journal.inner.close()
        } else if let Some(j) = pending_journal.as_mut() {
            j.inner.close()
        } else {
            // A failed consuming constructor may have released by Drop only.
            // Missing slots do not prove the OS handle was checked closed.
            Err(ChannelError::External {
                causes: vec!["journal owner unavailable for checked close; retain fixture".into()],
            })
        }
    });
    timeline.run("input_identity_recheck", || inputs.verify());
    let complete = peer_cleanup.is_ok() && journal_cleanup.is_ok();
    let wall = nanos(start);
    assert!(timeline.total() <= wall);
    let mut report = measurements.unwrap_or_else(|| json!({}));
    report["schema"] = json!("lycheedev.live-offline-sample.v1");
    report["repeat"] = json!(repeat);
    report["input"] = json!({"resultStringBytes":payload_bytes,"historyEvents":history});
    report["sampleWallNanos"] = json!(wall);
    report["unattributedWallNanos"] = json!(wall - timeline.total());
    report["scopes"] = json!(timeline.0);
    report["completion"] = json!({"bodyPanicked":outcome.is_err(),"peerCleanup":peer_cleanup,"journalCleanup":format!("{journal_cleanup:?}"),"verified":complete});
    write_new(
        &case.root.join("sample.json"),
        &serde_json::to_vec_pretty(&report).unwrap(),
    );
    assert!(
        complete,
        "explicit offline peer/journal cleanup failed; no further sample"
    );
    if let Err(panic) = outcome {
        resume_unwind(panic)
    }
    report
}

#[test]
#[ignore = "opt-in owned offline performance runner; not ordinary lib tests"]
fn offline_driver_journal_performance() {
    let start = Instant::now();
    let inputs = Inputs::load();
    let authentication = nanos(start);
    let mut samples = Vec::new();
    // Interleave axes by repeat without relabeling cache state. Every sample has
    // an independent filesystem fixture and fresh owned Lua process.
    for repeat in 0..PERFORMANCE_REPEATS {
        for &history in &PERFORMANCE_HISTORY_EVENTS {
            for &bytes in &PERFORMANCE_RESULT_STRING_BYTES {
                samples.push(sample(&inputs, bytes, history, repeat));
            }
        }
    }
    assert_eq!(samples.len(), 27);
    let sample_wall: u64 = samples
        .iter()
        .map(|s| s["sampleWallNanos"].as_u64().unwrap())
        .sum();
    let wall = nanos(start);
    assert!(authentication + sample_wall <= wall);
    write_new(&inputs.cases.join("performance.json"),&serde_json::to_vec_pretty(&json!({
        "schema":"lycheedev.live-offline-performance.v1","oracle":formal_lua_fixture::ORACLE,"state":"passed","samples":samples,
        "totalWallNanos":wall,"authenticationWallNanos":authentication,"sampleWallNanos":sample_wall,"betweenSamplesAndEvidenceWallNanos":wall-authentication-sample_wall,
        "scope":"Rust-only offline Driver/codec/durable journal/owned terminal diagnostic; fixed-Go323 Lua fixture. No comparable fixed-Go host timing, no ratio, no cold/warm or 20-repeat acceptance, no game latency. Fixture clocks do not measure elapsed time.",
        "timing":"std::time::Instant; disjoint sample scopes plus honest remainder; Driver journal/backend buckets replace their parent interval, nested exchange must not be added again. Setup, history seeding, peer startup, full checks and cleanup retained. Final evidence serialization and root process lifetime are included only in controller elapsed.",
        "memory":"Only directly observed output Vec capacities and code-provable bulk-copy bytes; no allocator hook, total allocation count, requested-byte total or process peak claim."
    })).unwrap());
}
