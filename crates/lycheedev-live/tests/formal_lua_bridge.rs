//! Current Mailbox v1 bridge regression: the real Rust formal Driver, codec and
//! durable journal against the real addon Lua writer and the Lua protocol peer
//! under tests/protocol. Physical input, selected HWND, monotonic samples and
//! wall clock are fixtures. Also covers the memory-record and finite-number host
//! parity cases. These owned Lua peers do not replace real-client validation.
#![forbid(unsafe_code)]
#[path = "support/formal_lua_fixture.rs"]
mod formal_lua_fixture;
#[path = "support/formal_lua_peer.rs"]
mod formal_lua_peer;

use formal_lua_fixture::{
    BROKEN_CODE, CODE, Case, FixtureClock, Inputs, NUMERIC_CODE, Tokens, read,
};
use formal_lua_peer::{Peer, PeerOwner};
use lycheedev_core::control::OperationControl;
use lycheedev_live::mailbox::{
    Kind, decode_record,
    driver::Driver,
    durable_journal::DurableChannelJournal,
    observations::ChannelError,
    state::{CleanupMethod, Operation, OperationPolicy, OperationStage, State},
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

// One long-lived peer at a time bounds resources and prevents a failed owner
// from overlapping another case. The controller also uses --test-threads=1;
// a forgotten libtest flag cannot weaken this fixture lifetime requirement.
static SERIAL: Mutex<()> = Mutex::new(());
static UNRESOLVED_OWNER: AtomicBool = AtomicBool::new(false);
type TestDriver = Driver<Peer, DurableChannelJournal, FixtureClock, Tokens>;

fn has_code(error: &ChannelError, code: &str) -> bool {
    error.any_cause(&|cause| matches!(cause, ChannelError::Contract(c) if c.0 == code))
}
fn persisted(state: &State) -> serde_json::Value {
    serde_json::to_value(state).unwrap()
}
fn restart(slot: &mut Option<TestDriver>, case: &Case, control: &OperationControl) {
    // All fallible journal work occurs while the original Driver is retained.
    // The independent PeerOwner also survives the later constructor consuming
    // the Backend, even if that constructor rejects state unexpectedly.
    let old = slot.as_mut().expect("driver before restart");
    let expected = persisted(&old.state);
    old.journal
        .close()
        .expect("checked execution lease close before reopening");
    let journal = case.open(control);
    let state = journal.load(control).expect("real durable journal reload");
    assert_eq!(persisted(&state), expected, "restart altered durable state");
    let old = slot.take().unwrap();
    *slot = Some(Driver::from_state(state, old.backend, journal, old.clock, old.tokens).unwrap());
}

fn durable_result(case: &Case, operation: &Operation) -> Operation {
    let bytes = read(
        &case
            .paths
            .result_directory()
            .join(format!("{}.json", operation.id)),
        2 << 20,
    );
    let saved: Operation = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved.id, operation.id);
    assert_eq!(saved.ticket, operation.ticket);
    assert_eq!(saved.request, operation.request);
    assert_eq!(saved.result_bytes, operation.result_bytes);
    assert_eq!(saved.report_bytes, operation.report_bytes);
    assert_eq!(saved.report_checksum, operation.report_checksum);
    assert_eq!(saved.stage, OperationStage::ResultVerified);
    saved
}
fn memory_parity(case: &Case) {
    let bytes = read(&case.records.join("memory-record.bin"), 4096);
    let record = decode_record(&bytes).expect("decode actual Lua MemoryProtocol.Encode bytes");
    assert_eq!(record.payload, b"hello");
    assert_eq!(record.header.sequence, 17);
    assert_eq!(record.header.nonce, [0x11; 16]);
    assert_eq!(record.header.runtime, [0x22; 16]);
    assert_eq!(record.header.ticket, [0x33; 16]);
    assert_eq!(record.header.kind, Kind::Body);
    assert_eq!(record.header.state, 3);
    std::fs::write(
        case.root.join("host-parity.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema":"lycheedev.formal-lua-host-parity.v1", "passed":true,
            "case":"memory",
            "recordSHA256":lycheedev_core::storage::sha256(&bytes),
            "payload":"hello", "sequence":17, "nonce":"1".repeat(32),
            "runtime":"2".repeat(32), "ticket":"3".repeat(32), "kind":3, "state":3
        }))
        .unwrap(),
    )
    .unwrap();
}

fn numeric_bits(bytes: &[u8]) -> Vec<u64> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Report {
        ok: bool,
        resources_released: bool,
        result: Vec<Box<serde_json::value::RawValue>>,
    }
    let report: Report = serde_json::from_slice(bytes).expect("real Lua numeric report JSON");
    assert!(report.ok && report.resources_released);
    let mut expected = vec![0.0, -0.0];
    for value in [
        0.1,
        0.5,
        1.0,
        1.2345678901234567,
        100000000000000.125,
        9007199254740991.0,
        9007199254740992.0,
        9007199254740994.0,
        f64::from_bits(1.0_f64.to_bits() - 1),
        f64::from_bits(1.0_f64.to_bits() + 1),
        1e-200,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::MAX,
    ] {
        expected.push(value);
        expected.push(-value);
    }
    assert_eq!(report.result.len(), 30);
    assert_eq!(expected.len(), 30);
    // Preserve each JSON number token and use the standard correctly-rounded
    // Float64 parser, matching Go encoding/json's ParseFloat. serde_json's
    // optional fast f64 path is not the product under test here.
    let bits: Vec<_> = report
        .result
        .iter()
        .map(|number| {
            number
                .get()
                .parse::<f64>()
                .expect("finite JSON number")
                .to_bits()
        })
        .collect();
    let expected: Vec<_> = expected.iter().map(|number| number.to_bits()).collect();
    assert_eq!(
        bits, expected,
        "all thirty Float64bits, including signed zero and subnormals"
    );
    bits
}

fn numeric_parity(slot: &mut Option<TestDriver>, case: &Case, control: &OperationControl) {
    let d = slot.as_mut().unwrap();
    d.connect(control).unwrap();
    d.prepare_request(
        control,
        "numeric-roundtrip",
        NUMERIC_CODE,
        10,
        OperationPolicy::Observation,
    )
    .unwrap();
    d.run(control).unwrap();
    let op = d.state.operation.as_ref().unwrap();
    assert_eq!(op.stage, OperationStage::Complete);
    assert_eq!(op.cleanup_method, CleanupMethod::Released);
    let original = op.result_bytes.0.clone();
    let bits = numeric_bits(&original);
    durable_result(case, op);
    assert_eq!(d.backend.release_checks, 1);
    let sends = d.backend.sends.clone();
    assert_eq!(
        sends
            .iter()
            .map(|row| row.action.as_str())
            .collect::<Vec<_>>(),
        ["bind", "prepare", "commit", "confirm", "release"]
    );
    restart(slot, case, control);
    let d = slot.as_mut().unwrap();
    let recovered = &d.state.operation.as_ref().unwrap().result_bytes.0;
    assert_eq!(recovered, &original, "recovered report bytes changed");
    assert_eq!(numeric_bits(recovered), bits);
    d.run(control).unwrap();
    assert_eq!(d.backend.sends, sends, "completed report retry sent input");
    assert_eq!(d.state.operation.as_ref().unwrap().result_bytes.0, original);
    d.disconnect(control).unwrap();
    assert!(!d.state.bound);
    assert!(d.state.transaction.is_none());
    assert_eq!(d.backend.sends.len(), sends.len() + 1);
    assert_eq!(d.backend.sends.last().unwrap().action, "unbind");
    assert_eq!(read(&case.records.join("executions.txt"), 16), b"1");
    std::fs::write(case.root.join("host-parity.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "schema":"lycheedev.formal-lua-host-parity.v1", "passed":true,
        "case":"numeric",
        "valueCount":bits.len(), "float64Bits":bits.iter().map(|bits| format!("{bits:016x}")).collect::<Vec<_>>(),
        "reportSHA256":lycheedev_core::storage::sha256(&original),
        "recoveredBytesEqual":true, "completedRetryInputCountUnchanged":true,
        "resourcesReleased":true, "disconnected":true, "executions":1
    })).unwrap()).unwrap();
}
fn exercise(slot: &mut Option<TestDriver>, case: &Case, fault: &str, control: &OperationControl) {
    let connected = slot.as_mut().unwrap().connect(control);
    if let Err(error) = connected {
        assert!(
            matches!(fault, "bind" | "reload_bind"),
            "unexpected connect failure: {error:?}"
        );
        let original = slot.as_ref().unwrap().state.transaction.clone().unwrap();
        let runtime = slot.as_ref().unwrap().state.identity.runtime.clone();
        let deadline = slot.as_ref().unwrap().state.connect_budget.clone();
        restart(slot, case, control);
        if fault == "reload_bind" {
            // Lua generation 2 ignored a generation-1 bind and retained no
            // receipt. Repeated resume must neither republish nor send input.
            for _ in 0..2 {
                let d = slot.as_mut().unwrap();
                let error = d
                    .connect(control)
                    .expect_err("foreign runtime stays pending");
                assert!(error.is_pending(), "{error:?}");
                assert_eq!(d.backend.sends.len(), 1);
                assert!(!d.state.bound);
                assert_eq!(d.state.identity.runtime, runtime);
                assert_eq!(d.state.connect_budget, deadline);
                assert_eq!(d.state.transaction, Some(original.clone()));
                assert!(d.state.transaction.as_ref().unwrap().receipt.is_none());
                restart(slot, case, control);
            }
            assert_eq!(read(&case.records.join("executions.txt"), 16), b"0");
            return;
        }
        slot.as_mut().unwrap().connect(control).unwrap();
    } else {
        assert!(
            !matches!(fault, "bind" | "reload_bind"),
            "fault was not exercised"
        );
    }
    let d = slot.as_mut().unwrap();
    d.prepare_request(
        control,
        "protocol-recovery",
        if fault == "compile_error" {
            BROKEN_CODE
        } else {
            CODE
        },
        10,
        OperationPolicy::Observation,
    )
    .unwrap();
    let original = d.state.operation.clone().unwrap();
    let result = d.run(control);
    if let Err(error) = result {
        if fault == "cleanup_error" {
            assert!(
                has_code(&error, "live.channel_cleanup_reload_required"),
                "{error:?}"
            );
            let operation = d.state.operation.as_ref().unwrap();
            assert_eq!(operation.stage, OperationStage::ReleaseReady);
            assert_eq!(operation.cleanup_method, CleanupMethod::ReloadRequired);
            durable_result(case, operation);
            let sends = d.backend.sends.len();
            restart(slot, case, control);
            let d = slot.as_mut().unwrap();
            let error = d.run(control).expect_err("cleanup must remain pending");
            assert!(
                has_code(&error, "live.channel_cleanup_reload_required"),
                "{error:?}"
            );
            assert_eq!(d.backend.sends.len(), sends, "uncertain cleanup repeated");
            assert_eq!(d.backend.release_checks, 1);
            assert_eq!(read(&case.records.join("executions.txt"), 16), b"1");
            let operation = d.state.operation.as_ref().unwrap();
            assert_eq!(operation.ticket, original.ticket);
            assert_eq!(operation.recovery_budget, original.recovery_budget);
            durable_result(case, operation);
            return;
        }
        assert!(
            matches!(fault, "prepare" | "commit" | "confirm" | "release"),
            "unexpected run failure: {error:?}"
        );
        let before = d.state.transaction.clone().unwrap();
        assert_eq!(before.envelope.action, fault, "wrong fault boundary");
        assert!(
            error.any_cause(&|cause| matches!(cause, ChannelError::External { causes }
            if causes.iter().any(|c| c == "injected host death after input")))
        );
        restart(slot, case, control);
        assert_eq!(slot.as_ref().unwrap().state.transaction, Some(before));
        slot.as_mut().unwrap().run(control).unwrap();
    } else {
        assert!(
            matches!(fault, "normal" | "bind" | "compile_error"),
            "fault was not exercised"
        );
    }
    let d = slot.as_mut().unwrap();
    let operation = d.state.operation.as_ref().unwrap();
    assert_eq!(operation.stage, OperationStage::Complete);
    assert_eq!(operation.cleanup_method, CleanupMethod::Released);
    assert_eq!(operation.request, original.request);
    assert_eq!(operation.id, original.id);
    assert_eq!(operation.ticket, original.ticket);
    assert_eq!(
        operation.recovery_budget, original.recovery_budget,
        "restart extended original deadline"
    );
    let text = std::str::from_utf8(&operation.result_bytes.0).unwrap();
    assert!(text.contains(if fault == "compile_error" {
        "compile_error"
    } else {
        "荔枝"
    }));
    // Unicode, newline, NUL followed by digits, and quoted delimiter survive
    // the shared Rust payload writer and unchanged Lua execution/report path.
    if fault != "compile_error" {
        let report: serde_json::Value = serde_json::from_slice(&operation.result_bytes.0).unwrap();
        assert_eq!(report["result"]["message"], "荔枝\n\u{0}123");
        assert_eq!(report["result"]["quoted"], "x\\y");
    }
    durable_result(case, operation);
    assert_eq!(d.backend.release_checks, 1);
    assert_eq!(
        read(&case.records.join("executions.txt"), 16),
        if fault == "compile_error" { b"0" } else { b"1" }
    );
    let sends = d.backend.sends.clone();
    assert_eq!(
        sends
            .iter()
            .map(|e| e.nonce.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        sends.len(),
        "exchange replayed"
    );
    assert_eq!(
        sends.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(),
        if fault == "compile_error" {
            vec!["bind", "prepare", "confirm", "release"]
        } else {
            vec!["bind", "prepare", "commit", "confirm", "release"]
        }
    );
    restart(slot, case, control);
    let d = slot.as_mut().unwrap();
    d.run(control).unwrap();
    assert_eq!(d.backend.sends, sends, "completed retry sent input");
    d.disconnect(control).unwrap();
    assert!(!d.state.bound);
    assert_eq!(d.backend.sends.last().unwrap().action, "unbind");
    assert_eq!(d.backend.sends.len(), sends.len() + 1);
}

fn run_case(fault: &str) {
    let _serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(
        !UNRESOLVED_OWNER.load(Ordering::SeqCst),
        "prior peer cleanup unverified; no further launch"
    );
    let inputs = Inputs::load();
    let case = inputs.case(fault);
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(20));
    let mut driver = None;
    let mut owner: Option<PeerOwner> = None;
    let mut launch_intent = false;
    // Setup and restart are inside the same panic boundary as business work.
    // In-flight is set BEFORE spawn and cleared only by explicit complete
    // retirement, never by Drop, a channel event, or a vanished Driver slot.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let journal = case.open(&control);
        let state = journal.load(&control).unwrap();
        launch_intent = true;
        UNRESOLVED_OWNER.store(true, Ordering::SeqCst);
        let peer = Peer::start(
            &inputs,
            &case.records,
            case.paths.clone(),
            if fault == "normal" { "" } else { fault },
            &control,
        );
        owner = Some(peer.owner());
        driver = Some(Driver::from_state(state, peer, journal, FixtureClock, Tokens(16)).unwrap());
        match fault {
            "memory" => memory_parity(&case),
            "numeric" => numeric_parity(&mut driver, &case, &control),
            _ => exercise(&mut driver, &case, fault, &control),
        }
    }));
    let cleanup = match owner.as_mut() {
        Some(owner) => owner.close(outcome.is_ok()),
        None if !launch_intent => Ok(()),
        None => Err("setup did not return an owned peer; startup cleanup proof is not sufficient to permit another case".into()),
    };
    let journal_close = driver.as_mut().map(|d| d.journal.close());
    let complete =
        cleanup.is_ok() && (!launch_intent || journal_close.as_ref().is_some_and(Result::is_ok));
    if complete {
        UNRESOLVED_OWNER.store(false, Ordering::SeqCst);
    }
    std::fs::write(case.root.join("case-outcome.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "launchIntent":launch_intent, "ownerRetained":owner.is_some(), "bodyPanicked":outcome.is_err(),
        "peerCleanup":cleanup, "journalClose":format!("{journal_close:?}"),
        "completionVerified":complete, "launchBlocked":UNRESOLVED_OWNER.load(Ordering::SeqCst)
    })).unwrap()).expect("retain cleanup outcome");
    inputs.verify();
    assert!(
        complete,
        "explicit fixture cleanup incomplete; Lua={cleanup:?}; journal={journal_close:?}; original assertion={}",
        outcome.is_err()
    );
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
}

macro_rules! case {
    ($test:ident, $fault:literal) => {
        #[test]
        fn $test() {
            run_case($fault);
        }
    };
}
case!(formal_lua_recovery_normal, "normal");
case!(formal_lua_recovery_after_bind, "bind");
case!(formal_lua_recovery_after_prepare, "prepare");
case!(formal_lua_recovery_after_commit, "commit");
case!(formal_lua_recovery_after_confirm, "confirm");
case!(formal_lua_recovery_after_release, "release");
case!(formal_lua_recovery_reload_bind, "reload_bind");
case!(formal_lua_recovery_compile_error, "compile_error");
case!(formal_lua_recovery_cleanup_error, "cleanup_error");
case!(formal_lua_memory_record_parity, "memory");
case!(
    formal_lua_numeric_report_preserves_finite_numbers,
    "numeric"
);
