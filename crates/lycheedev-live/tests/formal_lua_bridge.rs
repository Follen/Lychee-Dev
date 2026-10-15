//! Fixed Go 3.2.2 TestSlotDriverLuaRecoveryWithoutBusinessReplay, with the real
//! Rust formal Driver/codec/durable journal and unchanged authenticated Lua.
//! Physical input, selected HWND, monotonic samples and wall clock are fixtures.
//! This does not replace real-client or the two remaining host bridge tests.
#![forbid(unsafe_code)]
#[path = "support/formal_lua_fixture.rs"]
mod formal_lua_fixture;
#[path = "support/formal_lua_peer.rs"]
mod formal_lua_peer;

use formal_lua_fixture::{BROKEN_CODE, CODE, Case, FixtureClock, Inputs, Tokens, read};
use formal_lua_peer::{Peer, PeerOwner};
use lycheedev_core::control::OperationControl;
use lycheedev_live::go_mailbox_v1::{
    driver::Driver,
    durable_journal::DurableChannelJournal,
    observations::ChannelError,
    state::{CleanupMethod, Operation, OperationPolicy, OperationStage, State},
};
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
    let control =
        OperationControl::new(Instant::now() + Duration::from_secs(20), Default::default());
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
        exercise(&mut driver, &case, fault, &control);
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
