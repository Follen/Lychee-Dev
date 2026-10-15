//! Disk-backed fixed-Go retirement regressions. All are NOT_RUN until an owned
//! runner records their real exit status; no native/input adapter is reachable.
use super::*;
use serde_json::{Value, json};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    home: PathBuf,
    parent: PathBuf,
    workspace: String,
    id: String,
    control: OperationControl,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "historical-retire-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let home = root.join("workspace");
        let workspace = lycheedev_core::workspace::initialize(&home).unwrap()["workspaceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let parent = root.join("game/Interface/AddOns");
        fs::create_dir_all(&parent).unwrap();
        let parent = fs::canonicalize(parent).unwrap();
        Self {
            root,
            home,
            parent,
            workspace,
            id: format!("OP-{}", "1".repeat(32)),
            control: OperationControl::default(),
        }
    }
    fn record(&self, kind: &str, stage: &str, status: &str) -> WorkRecord {
        let game = self.parent.parent().unwrap().parent().unwrap();
        let mut value = json!({"schema":"lycheedev.work.v1","operationId":self.id,"generation":1,"intent":{"kind":kind,"resource":"window/1/2/3","snapshot":"PIN-test","session":"session","goal":"finished","request":{"schema":"lycheedev.report-intent.v1","budgetSeconds":5,"expected":{"Kind":"reported","Release":"3.2.0","SessionNonce":"session","RequestID":"request","Character":"paladin","Realm":"realm","Product":"retail","Build":"12.1.0.69875"},"load":{"installation":game}},"admission":{"parent":ordinary_spelling(&self.parent),"workspaceId":self.workspace}},"stage":stage,"status":status,"observation":null});
        if kind == "reload" {
            value["intent"]["request"] = json!({"schema":"lycheedev.fixed-reload.v1","target":{"client":{"directory":game},"window":{"processId":1,"processStartedAt":"2"}},"release":"3.2.0","identityNonce":"nonce"});
        }
        decode_record(&serde_json::to_vec(&value).unwrap(), &self.control).unwrap()
    }
    fn seed(&self, record: &WorkRecord, claim: bool) {
        self.document(
            &format!("work/{}", self.id),
            &work_bytes(record, &self.control).unwrap(),
        );
        self.document(
            &format!("ownership/{}", record.intent.resource),
            &serde_json::to_vec(&json!({"operationId":self.id})).unwrap(),
        );
        if claim {
            self.claim(&owner(&self.workspace, record, &self.control).unwrap());
        }
    }
    fn document(&self, key: &str, raw: &[u8]) {
        Metadata::with_historical_write(&self.home, &self.control, |w| {
            w.commit(&[HistoricalMutation {
                key,
                expected_generation: 0,
                bytes: raw,
            }])
        })
        .unwrap();
    }
    fn claim(&self, owner: &WindowOwner) {
        let p = self.marker(&owner.resource);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(
            p,
            compact(&serde_json::to_vec(owner).unwrap(), &self.control).unwrap(),
        )
        .unwrap();
    }
    fn marker(&self, resource: &str) -> PathBuf {
        self.parent
            .join(".lycheedev-window-owners")
            .join(format!("{}.json", storage::sha256(resource.as_bytes())))
    }
    fn read(&self) -> WorkRecord {
        let mut record = WorkRecord::default();
        Metadata::with_historical_write(&self.home, &self.control, |w| {
            inspect_work(w, &self.id, &mut record, &self.control)
        })
        .unwrap();
        record
    }
    fn result(&self, cancel: bool) -> HistoricalRead {
        retire_operation(&self.home, &self.id, cancel, &self.control)
    }
    fn btp(&mut self, phase: &str, claimed: bool) -> BootstrapAttempt {
        self.id = format!("BTP-{}", "2".repeat(32));
        let value = json!({"schema":"lycheedev.bootstrap-receiver.v1","id":self.id,"target":{"client":{"directory":self.parent.parent().unwrap().parent().unwrap()},"window":{"processId":1,"processStartedAt":"2","handle":"3"}},"action":"identify","requestId":"request","arg":"arg","phase":phase,"claimed":claimed,"future":{"unknown":true},"receiver":{"phase":"uncertain"}});
        let raw = serde_json::to_vec(&value).unwrap();
        let attempt = decode_bootstrap(&raw, &self.id, &self.control).unwrap();
        self.document(&format!("bootstrap/attempt/{}", self.id), &raw);
        if claimed {
            self.claim(&bootstrap_owner(&self.workspace, &attempt, &self.control).unwrap());
        }
        attempt
    }
    fn btp_refs(&self, attempt: &BootstrapAttempt, window: &[u8]) {
        self.document(
            &format!(
                "bootstrap/current/{}",
                storage::sha256(
                    format!("{}\0{}", attempt.target.resource(), attempt.action).as_bytes()
                )
            ),
            &serde_json::to_vec(&json!({"id":self.id})).unwrap(),
        );
        self.document(
            &format!(
                "bootstrap/window/{}",
                storage::sha256(attempt.target.resource().as_bytes())
            ),
            window,
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn prepared_cancel_is_atomic_disk_only_and_repeated_terminal_is_read_only() {
    let f = Fixture::new();
    let before = f.record("probe", "prepared", "pending");
    f.seed(&before, true);
    let first = f.result(true);
    assert!(first.error.is_none(), "{:?}", first.error);
    assert_eq!(first.outcome.status, "cancelled");
    assert_eq!(first.outcome.cleanup, "complete");
    assert!(!first.outcome.complete);
    assert_eq!(first.outcome.report.state, "unavailable");
    let after = f.read();
    assert_eq!(after.generation, 2);
    assert_eq!(after.intent.request.0, before.intent.request.0);
    assert_eq!(after.observation.0, b"null");
    assert!(!f.marker(&before.intent.resource).exists());
    let owner = Metadata::read_document_raw_controlled(
        &f.home,
        &format!("ownership/{}", before.intent.resource),
        &f.control,
    )
    .unwrap();
    assert_eq!(owner.generation, 2);
    assert_eq!(owner.bytes, br#"{"operationId":""}"#);
    assert!(f.result(true).error.is_none());
    assert_eq!(f.read().generation, 2);
    assert!(!f.root.join(".lycheedev").exists());
}
#[test]
fn cancel_refuses_every_recorded_effect_status_and_non_null_observation() {
    for (stage, status, observation) in [
        ("loaded", "running", b"null".as_slice()),
        ("prepared", "running", b"null"),
        ("prepared", "pending", b"{}"),
        (
            "prepared",
            "pending",
            br#"{"dispatchInput":{"messagesQueued":0}}"#,
        ),
    ] {
        let f = Fixture::new();
        let mut record = f.record("probe", stage, status);
        record.observation.0 = observation.into();
        f.seed(&record, true);
        let read = f.result(true);
        assert_eq!(read.error.unwrap().code, "journal.invalid_transition");
        assert_eq!(f.read().generation, 1);
        assert!(f.marker(&record.intent.resource).exists());
    }
}
#[test]
fn every_fixed_go_abandon_transition_and_abandoning_resume_is_implemented() {
    for kind in ["probe", "faults", "reload"] {
        for stage in [
            "prepared",
            "load_requested",
            "loaded",
            "dispatch_requested",
            "reported",
            "flush_requested",
            "persisted",
            "verified",
            "ack_requested",
            "acknowledged",
            "reload_requested",
            "abandoning",
        ] {
            let allowed = stage == "abandoning" || allows_abandon(kind, stage);
            let f = Fixture::new();
            let record = f.record(
                kind,
                stage,
                if stage == "prepared" {
                    "pending"
                } else {
                    "running"
                },
            );
            f.seed(&record, true);
            let read = f.result(false);
            if allowed {
                assert!(read.error.is_none(), "{kind}/{stage}: {:?}", read.error);
                assert_eq!(read.outcome.cleanup, "abandoned");
                assert!(!read.outcome.complete);
                assert_eq!(
                    f.read().generation,
                    if stage == "abandoning" { 2 } else { 3 }
                );
                assert!(!f.marker(&record.intent.resource).exists());
            } else {
                assert_eq!(read.error.unwrap().code, "journal.invalid_transition");
                assert_eq!(f.read().generation, 1);
            }
        }
    }
}
#[test]
fn missing_claim_recovery_requires_original_prepared_admission() {
    for (stage, admission, accepted) in [
        ("prepared", true, true),
        ("prepared", false, false),
        ("load_requested", true, false),
    ] {
        let f = Fixture::new();
        let mut record = f.record(
            "probe",
            stage,
            if stage == "prepared" {
                "pending"
            } else {
                "running"
            },
        );
        if !admission {
            record.intent.admission = None;
        }
        f.seed(&record, false);
        let read = f.result(false);
        assert_eq!(read.error.is_none(), accepted, "{:?}", read.error);
        if !accepted {
            assert!(
                read.error
                    .unwrap()
                    .message
                    .starts_with("journal.window_admission_unavailable")
            );
            assert_eq!(f.read().generation, 1);
        }
    }
}
#[test]
fn prepared_recovery_rejects_changed_admission_parent_and_workspace() {
    for foreign_workspace in [false, true] {
        let f = Fixture::new();
        let mut record = f.record("probe", "prepared", "pending");
        let a = record.intent.admission.as_mut().unwrap();
        if foreign_workspace {
            a.workspace_id = "a".repeat(32);
        } else {
            a.parent.push_str("-other");
        }
        f.seed(&record, false);
        assert!(
            f.result(true)
                .error
                .unwrap()
                .message
                .contains("journal.window_admission_unavailable")
        );
        assert_eq!(f.read().generation, 1);
    }
}
#[test]
fn same_process_other_owner_and_foreign_workspace_and_intent_mismatch_are_preserved() {
    for field in ["workspace", "operation", "intent"] {
        let f = Fixture::new();
        let record = f.record("probe", "prepared", "pending");
        f.seed(&record, false);
        let mut claim = owner(&f.workspace, &record, &f.control).unwrap();
        match field {
            "workspace" => claim.workspace_id = "f".repeat(32),
            "operation" => claim.operation_id = format!("OP-{}", "f".repeat(32)),
            _ => claim.intent_sha256 = "f".repeat(64),
        }
        f.claim(&claim);
        let saved = fs::read(f.marker(&record.intent.resource)).unwrap();
        let read = f.result(true);
        let e = read.error.unwrap();
        assert_eq!(
            e.code,
            if field == "intent" {
                "command.failed"
            } else {
                "journal.resource_busy"
            }
        );
        assert_eq!(fs::read(f.marker(&record.intent.resource)).unwrap(), saved);
        assert_eq!(f.read().generation, 1);
    }
}
#[test]
fn active_execution_driver_blocks_retirement_without_removing_durable_claim() {
    let f = Fixture::new();
    let record = f.record("probe", "prepared", "pending");
    f.seed(&record, true);
    let want = owner(&f.workspace, &record, &f.control).unwrap();
    let mut held =
        historical::acquire_operation(&f.parent, want, &f.control, |_| panic!("claim exists"))
            .unwrap();
    let read = f.result(true);
    assert_eq!(read.error.as_ref().unwrap().code, "journal.resource_busy");
    assert!(read.error.unwrap().message.contains("active window driver"));
    assert_eq!(f.read().generation, 1);
    assert!(f.marker(&record.intent.resource).exists());
    held.close().unwrap();
    assert!(f.result(true).error.is_none());
}
#[test]
fn wrong_local_ownership_retains_abandoning_checkpoint_and_unknown_evidence() {
    let f = Fixture::new();
    let mut record = f.record("probe", "dispatch_requested", "unresolved");
    record.observation.0=br#"{"dispatchInput":{"messagesQueued":1,"submissionComplete":false},"unknownFutureField":"keep"}"#.to_vec();
    f.seed(&record, true);
    Metadata::with_historical_write(&f.home, &f.control, |w| {
        w.commit(&[HistoricalMutation {
            key: "ownership/window/1/2/3",
            expected_generation: 1,
            bytes: br#"{"operationId":"OP-other"}"#,
        }])
    })
    .unwrap();
    let read = f.result(false);
    assert_eq!(read.error.unwrap().code, "journal.resource_busy");
    assert_eq!(read.outcome.stage, "abandoning");
    assert_eq!(f.read().generation, 2);
    assert_eq!(f.read().observation.0, record.observation.0);
    assert!(f.marker(&record.intent.resource).exists());
}
#[test]
fn terminal_record_only_retires_exact_owner_and_never_rewrites_database() {
    for foreign in [false, true] {
        let f = Fixture::new();
        let record = f.record("probe", "abandoned", "abandoned");
        f.seed(&record, true);
        if foreign {
            let mut claim = owner(&f.workspace, &record, &f.control).unwrap();
            claim.operation_id = format!("BTP-{}", "f".repeat(32));
            f.claim(&claim);
        }
        let read = f.result(true);
        assert_eq!(read.error.is_none(), !foreign);
        assert_eq!(f.read().generation, 1);
        assert_eq!(f.marker(&record.intent.resource).exists(), foreign);
    }
}
#[test]
fn unsupported_work_kind_and_relative_installation_cannot_acquire_or_mutate() {
    for kind in ["other", "probe"] {
        let f = Fixture::new();
        let mut record = f.record(kind, "prepared", "pending");
        if kind == "probe" {
            record.intent.request.0 = br#"{"installation":"relative"}"#.to_vec();
        }
        f.seed(&record, true);
        let read = f.result(true);
        assert!(read.error.is_some());
        assert_eq!(f.read().generation, 1);
        assert!(f.marker(&record.intent.resource).exists());
    }
}
#[test]
fn installation_precedence_is_target_then_load_then_direct() {
    let f = Fixture::new();
    let mut record = f.record("probe", "prepared", "pending");
    let direct = f.root.join("direct");
    let load = f.root.join("load");
    let target = f.root.join("target");
    for (request, want) in [
        (json!({"installation":direct}), &direct),
        (
            json!({"installation":direct,"load":{"installation":load}}),
            &load,
        ),
        (
            json!({"installation":direct,"load":{"installation":load},"target":{"client":{"directory":target}}}),
            &target,
        ),
    ] {
        record.intent.request.0 = serde_json::to_vec(&request).unwrap();
        assert_eq!(
            installation(&record, &f.control).unwrap(),
            want.join("Interface/AddOns")
        );
    }
}
#[test]
fn missing_report_error_is_returned_with_durable_abandoned_projection() {
    let f = Fixture::new();
    let mut record = f.record("probe", "verified", "running");
    record.observation.0 =
        br#"{"bodyId":"CAP-missing-body","receiptId":"CAP-missing-receipt"}"#.to_vec();
    f.seed(&record, true);
    let read = f.result(false);
    assert!(read.error.is_some());
    assert_eq!(read.outcome.status, "abandoned");
    assert_eq!(read.outcome.cleanup, "abandoned");
    assert!(!read.outcome.complete);
    assert_eq!(f.read().observation.0, record.observation.0);
    assert!(!f.marker(&record.intent.resource).exists());
}
#[test]
fn raw_retirement_keeps_invalid_utf8_duplicates_and_numeric_spelling() {
    let f = Fixture::new();
    let mut record = f.record("probe", "dispatch_requested", "unresolved");
    record.observation.0 =
        b"{ \"future\":\"\xff<&\",\"wide\":18446744073709551615,\"wide\":1e+09 }".to_vec();
    f.seed(&record, true);
    let read = f.result(false);
    assert!(read.error.is_none(), "{:?}", read.error);
    assert_eq!(
        f.read().observation.0,
        b"{\"future\":\"\xff\\u003c\\u0026\",\"wide\":18446744073709551615,\"wide\":1e+09}"
    );
}
#[test]
fn raw_duplicate_intent_objects_merge_and_request_null_is_preserved() {
    let c = OperationControl::default();
    let raw=br#"{"intent":{"request":{"a":1},"kind":"probe"},"INTENT":null,"intent":{"resource":"x","request":null},"observation":{"a":1},"OBSERVATION":null}"#;
    let mut record: WorkRecord = decode_record(raw, &c).unwrap();
    preserve_raw(&mut record, raw, &c).unwrap();
    assert_eq!(record.intent.kind, "probe");
    assert_eq!(record.intent.request.0, b"null");
    assert_eq!(record.observation.0, b"null");
}
#[test]
fn owner_hash_omits_goal_but_keeps_original_request_and_admission() {
    let f = Fixture::new();
    let mut a = f.record("probe", "prepared", "pending");
    let first = owner(&f.workspace, &a, &f.control).unwrap();
    a.intent.goal = "changed".into();
    assert_eq!(first, owner(&f.workspace, &a, &f.control).unwrap());
    a.intent.request.0 = br#"{"installation":"different"}"#.to_vec();
    assert_ne!(first, owner(&f.workspace, &a, &f.control).unwrap());
}
#[test]
fn historical_parent_uses_pinned_simple_fold_without_full_case_expansion() {
    for (a, b) in [("S", "ſ"), ("K", "K"), ("Σ", "ς"), ("ß", "ẞ")] {
        assert!(equal_fold(a, b));
    }
    for (a, b) in [("İ", "i"), ("I", "ı"), ("ß", "SS"), ("é", "e\u{301}")] {
        assert!(!equal_fold(a, b));
    }
}
#[test]
fn bootstrap_abandon_claimed_and_unclaimed_preserves_unknown_fields_and_receiver() {
    for claimed in [false, true] {
        let mut f = Fixture::new();
        let attempt = f.btp("prepared", claimed);
        let window = serde_json::to_vec(&json!({"id":f.id})).unwrap();
        f.btp_refs(&attempt, &window);
        let read = abandon_bootstrap(&f.home, &f.id, &f.control);
        assert!(read.error.is_none(), "{:?}", read.error);
        assert_eq!(read.attempt.phase, "abandoned");
        assert_eq!(read.attempt.receiver.phase, "uncertain");
        assert_eq!(read.attempt.claimed, claimed);
        assert!(!f.marker(&attempt.target.resource()).exists());
        let doc = Metadata::read_document_raw_controlled(
            &f.home,
            &format!("bootstrap/attempt/{}", f.id),
            &f.control,
        )
        .unwrap();
        assert_eq!(doc.generation, 2);
        assert_eq!(
            serde_json::from_slice::<Value>(&doc.bytes).unwrap()["future"]["unknown"],
            true
        );
        let again = abandon_bootstrap(&f.home, &f.id, &f.control);
        assert!(again.error.is_none());
        assert_eq!(
            Metadata::read_document_raw_controlled(
                &f.home,
                &format!("bootstrap/attempt/{}", f.id),
                &f.control
            )
            .unwrap()
            .generation,
            2
        );
    }
}
#[test]
fn bootstrap_second_reference_reuses_prior_id_for_null_empty_and_null_id() {
    for window in [b"null".as_slice(), b"{}", br#"{"id":null}"#] {
        let mut f = Fixture::new();
        let attempt = f.btp("prepared", false);
        f.btp_refs(&attempt, window);
        let read = abandon_bootstrap(&f.home, &f.id, &f.control);
        assert!(read.error.is_none(), "{:?}", read.error);
        assert_eq!(read.attempt.phase, "abandoned");
    }
}
#[test]
fn bootstrap_confirmed_and_missing_claim_after_effect_are_refused() {
    for phase in ["confirmed", "submitted"] {
        let mut f = Fixture::new();
        let attempt = f.btp(phase, true);
        if phase == "submitted" {
            fs::remove_file(f.marker(&attempt.target.resource())).unwrap();
        }
        let read = abandon_bootstrap(&f.home, &f.id, &f.control);
        assert_eq!(
            read.error.unwrap().message,
            if phase == "confirmed" {
                "live.bootstrap_already_confirmed"
            } else {
                "live.bootstrap_claim_missing"
            }
        );
        assert_eq!(read.attempt.phase, phase);
    }
}
#[test]
fn bootstrap_reference_mismatch_and_missing_document_do_not_mutate_attempt() {
    for window in [None, Some(br#"{"id":"BTP-other"}"#.as_slice())] {
        let mut f = Fixture::new();
        let attempt = f.btp("prepared", false);
        if let Some(window) = window {
            f.btp_refs(&attempt, window);
        }
        let read = abandon_bootstrap(&f.home, &f.id, &f.control);
        assert!(read.error.is_some());
        assert_eq!(read.attempt.phase, "prepared");
        assert_eq!(
            Metadata::read_document_raw_controlled(
                &f.home,
                &format!("bootstrap/attempt/{}", f.id),
                &f.control
            )
            .unwrap()
            .generation,
            1
        );
    }
}
#[test]
fn bootstrap_foreign_exact_claim_and_active_driver_are_never_retired() {
    for active in [false, true] {
        let mut f = Fixture::new();
        let attempt = f.btp("prepared", true);
        f.btp_refs(&attempt, &serde_json::to_vec(&json!({"id":f.id})).unwrap());
        let want = bootstrap_owner(&f.workspace, &attempt, &f.control).unwrap();
        let mut held = None;
        if active {
            held = Some(historical::lock_bootstrap(&f.parent, &want, &f.control).unwrap());
        } else {
            let mut wrong = want.clone();
            wrong.intent_sha256 = "f".repeat(64);
            f.claim(&wrong);
        }
        let read = abandon_bootstrap(&f.home, &f.id, &f.control);
        assert_eq!(read.error.unwrap().code, "journal.resource_busy");
        assert_eq!(read.attempt.phase, "prepared");
        assert!(f.marker(&want.resource).exists());
        if let Some(held) = held.as_mut() {
            held.close().unwrap();
        }
    }
}
#[test]
fn bootstrap_raw_unknown_fields_keep_lexemes_invalid_bytes_and_map_last_key_wins() {
    let c = OperationControl::default();
    let raw =
        b"{\"x\":1,\"x\":1e+09,\"z\":\"\xff<\",\"Phase\":\"prepared\",\"phase\":\"submitted\"}";
    let time = parse_timestamp("2026-10-15T00:00:00Z").unwrap();
    let bytes = bootstrap_bytes(raw, &time, &c).unwrap();
    assert_eq!(bytes,b"{\"Phase\":\"prepared\",\"phase\":\"abandoned\",\"updatedAt\":\"2026-10-15T00:00:00Z\",\"x\":1e+09,\"z\":\"\xff\\u003c\"}");
}
#[test]
fn cancelled_retirement_preserves_original_typed_stop_and_all_durable_bytes() {
    let f = Fixture::new();
    let record = f.record("probe", "prepared", "pending");
    f.seed(&record, true);
    let before = fs::read(f.marker(&record.intent.resource)).unwrap();
    f.control.tracked_cancel_handle().unwrap().cancel();
    let read = f.result(true);
    assert_eq!(read.error.unwrap().code, "command.cancelled");
    assert_eq!(fs::read(f.marker(&record.intent.resource)).unwrap(), before);
}
#[test]
fn op_type_errors_return_partially_decoded_pending_result_without_mutation() {
    let f = Fixture::new();
    let raw = format!(
        r#"{{"schema":"lycheedev.work.v1","operationId":"{}","generation":1,"stage":4,"status":"pending","intent":{{"kind":"probe","snapshot":"PIN-kept"}},"observation":null}}"#,
        f.id
    );
    f.document(&format!("work/{}", f.id), raw.as_bytes());
    let read = f.result(true);
    assert!(read.error.is_some());
    assert_eq!(read.outcome.operation_id, f.id);
    assert_eq!(read.outcome.snapshot, "PIN-kept");
    assert_eq!(read.outcome.status, "pending");
    assert_eq!(
        Metadata::read_document_raw_controlled(&f.home, &format!("work/{}", f.id), &f.control)
            .unwrap()
            .bytes,
        raw.as_bytes()
    );
}
#[test]
fn bootstrap_type_error_keeps_fields_after_bad_type_and_custom_time_stops() {
    let c = OperationControl::default();
    let id = "BTP-test";
    let raw=br#"{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-test","phase":5,"action":"identify","target":{"window":{"processId":"bad","title":"kept"}},"receiver":{"attempt":1.5,"phase":"uncertain"}}"#;
    let mut attempt = BootstrapAttempt::default();
    assert!(super::super::retire_decode::attempt(raw, id, &mut attempt, &c).is_err());
    assert_eq!(attempt.id, id);
    assert_eq!(attempt.action, "identify");
    assert_eq!(attempt.target.window.title, "kept");
    assert_eq!(attempt.receiver.phase, "uncertain");
    let mut record = WorkRecord::default();
    let raw=br#"{"operationId":"OP-kept","createdAt":"bad","status":"must-not-appear","observation":{"after":true}}"#;
    assert!(super::super::retire_decode::work(raw, &mut record, &c).is_err());
    assert_eq!(record.operation_id, "OP-kept");
    assert!(record.status.is_empty());
    assert!(record.observation.0.is_empty());
}
#[test]
fn bootstrap_partial_history_reuses_backing_slots_around_type_errors() {
    let c = OperationControl::default();
    let raw=br#"{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-test","history":[{"action":"kept","attempt":1.5,"phase":"later"}],"history":[{"action":null,"arg":"new"}],"phase":"submitted"}"#;
    let mut attempt = BootstrapAttempt::default();
    assert!(super::super::retire_decode::attempt(raw, "BTP-test", &mut attempt, &c).is_err());
    let value = serde_json::to_value(attempt).unwrap();
    assert_eq!(value["history"][0]["action"], "kept");
    assert_eq!(value["history"][0]["phase"], "later");
    assert_eq!(value["history"][0]["arg"], "new");
    assert_eq!(value["phase"], "submitted");
}

#[test]
fn bootstrap_quoted_uint_fatal_errors_stop_outer_record_but_saved_errors_continue() {
    let c = OperationControl::default();
    for field in ["handle", "processStartedAt"] {
        for (token, fatal) in [
            (r#""+19""#, true),
            (r#""abc""#, true),
            (r#""\"bad""#, true),
            (r#""""#, false),
            (r#""no""#, false),
            (r#""-1""#, false),
            (r#""1e2""#, false),
            ("12", false),
            ("true", false),
            ("{}", false),
            (r#""\"valid\"""#, false),
            (r#""\"ok\" ""#, true),
            (r#""\"\\ud800\"""#, false),
            (r#""\"\\'\"""#, false),
        ] {
            let raw = format!(
                r#"{{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-test","target":{{"window":{{"{field}":{token},"title":"later"}}}},"phase":"submitted"}}"#
            );
            let mut attempt = BootstrapAttempt::default();
            assert!(
                super::super::retire_decode::attempt(raw.as_bytes(), "BTP-test", &mut attempt, &c)
                    .is_err(),
                "{field}: {token}"
            );
            assert_eq!(
                attempt.target.window.title,
                if fatal { "" } else { "later" },
                "{field}: {token}"
            );
            assert_eq!(
                attempt.phase,
                if fatal { "" } else { "submitted" },
                "{field}: {token}"
            );
        }
    }
}
#[test]
fn duplicate_time_failure_resets_only_quoted_time_and_stops_following_fields() {
    let c = OperationControl::default();
    for (token, zero) in [(r#""bad""#, true), ("42", false), ("{}", false)] {
        let raw = format!(
            r#"{{"createdAt":"2026-10-15T00:00:00Z","createdAt":{token},"phase":"later"}}"#
        );
        let mut attempt = BootstrapAttempt::default();
        assert!(
            super::super::retire_decode::attempt(raw.as_bytes(), "BTP-test", &mut attempt, &c)
                .is_err()
        );
        assert_eq!(attempt.created_at.zero, zero, "{token}");
        assert!(attempt.phase.is_empty());
    }
    let mut record = WorkRecord::default();
    let raw = br#"{"createdAt":"2026-10-15T00:00:00Z","createdAt":null,"status":"kept"}"#;
    assert!(super::super::retire_decode::work(raw, &mut record, &c).is_ok());
    assert!(!record.created_at.zero);
    assert_eq!(record.status, "kept");
}
