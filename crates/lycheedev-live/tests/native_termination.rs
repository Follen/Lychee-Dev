//! Formal host + durable files/claim termination, with no native capability opened.
//! Lua/input/optics are tested separately by native_e2e; these cases forbid those calls.
use lycheedev_core::{
    Invocation, Result, coordination::InstallationGuard, metadata::Metadata, storage, workspace,
};
use lycheedev_live::{
    host::{self, Services, Target},
    journal::DurableStore,
    lifecycle::{Action, Phase, Session, Store, Transport},
    managed::{ManagedIdentity, ManagedInstallation},
    slots::Instance,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
struct NoNative;
// Both tests deliberately use the same real PID/creation; the production lock is global.
static ADVANCE: std::sync::Mutex<()> = std::sync::Mutex::new(());
impl ManagedInstallation for NoNative {
    fn verify_clean(&self, _: &Path, _: &str, _: &str, _: u16) -> Result<ManagedIdentity> {
        panic!("host termination must not open managed capability")
    }
    fn publish_slot(&self, _: &ManagedIdentity, _: u16, _: Option<&str>, _: &[u8]) -> Result<()> {
        panic!("host termination must not publish")
    }
}
impl Services for NoNative {
    fn targets(&self, _: &Invocation) -> Result<Vec<Target>> {
        panic!("termination must not select a new target")
    }
    fn open(&self, _: &Target, _: Option<&Session>) -> Result<Box<dyn Transport>> {
        panic!("termination must not observe or send input")
    }
    fn managed(&self) -> &dyn ManagedInstallation {
        self
    }
}
fn invocation(home: &Path, command: &str, selector: &str) -> Invocation {
    Invocation {
        command: command.into(),
        positional: Some(selector.into()),
        flags: BTreeMap::new(),
        home: home.into(),
        cwd: home.into(),
        control: Default::default(),
    }
}
fn setup(phase: Phase) -> (PathBuf, PathBuf, Session) {
    let root = std::env::temp_dir().join(format!(
        "lycheedev-native-termination-{}",
        lycheedev_windows::random::token().unwrap()
    ));
    let home = root.join("home");
    let install = root.join("AddOns/Lychee Dev");
    fs::create_dir_all(&install).unwrap();
    workspace::initialize(&home).unwrap();
    let process = lycheedev_windows::Process::open(std::process::id()).unwrap();
    let mut session = Session::new(
        Instance {
            pid: process.identity().pid,
            created: process.identity().created,
            module_sha256: "f".repeat(64),
            runtime: "1".repeat(32),
        },
        "actor".into(),
        "2".repeat(32),
        lycheedev_windows::random::token().unwrap(),
    );
    let target = Target {
        window: lycheedev_windows::WindowIdentity {
            process: process.identity().clone(),
            hwnd: 0,
        },
        installation: install.clone(),
        actor: session.actor.clone(),
        build: "fixture".into(),
        module_sha256: session.instance.module_sha256.clone(),
        runtime: session.instance.runtime.clone(),
    };
    let mut store = DurableStore::create(&home, &install, &session).unwrap();
    storage::atomic_json(
        &home
            .join("live/v2")
            .join(format!("CON-{}.target.json", session.owner)),
        &target,
    )
    .unwrap();
    session.phase = Phase::Idle;
    session
        .prepare("a".repeat(32), "return 1".into(), 30, u64::MAX)
        .unwrap();
    session.phase = phase;
    if session.phase == Phase::ExecutePublished {
        session.pending_action = Some(Action::InputExecute);
        session.input_messages = Some(2);
    }
    store.checkpoint(&session).unwrap();
    drop(store);
    Metadata::open(&home, true)
        .unwrap()
        .put(
            &format!("native/request/{}", session.request.as_ref().unwrap().id),
            &serde_json::json!({"owner":session.owner}),
            None,
        )
        .unwrap();
    (home, install, session)
}
#[test]
fn cancel_and_abandon_preserve_evidence_release_exact_claim_and_never_open_native() {
    let _advance = ADVANCE.lock().unwrap();
    for (phase, command, terminal, cleanup, unknown) in [
        (
            Phase::Prepared,
            "live cancel",
            Phase::Cancelled,
            "cancelled",
            false,
        ),
        (
            Phase::ExecutePublished,
            "live abandon",
            Phase::Abandoned,
            "abandoned",
            true,
        ),
    ] {
        let (home, install, original) = setup(phase);
        let selector = format!("MEM-{}", original.request.as_ref().unwrap().id);
        let out = host::dispatch_with(&invocation(&home, command, &selector), &NoNative).unwrap();
        assert!(!out.complete);
        assert_eq!(out.data["cleanup"], cleanup);
        assert_eq!(out.data["executionUnknown"], unknown);
        assert_eq!(out.data["report"]["state"], "unavailable");
        assert_eq!(out.data["ownershipReleased"], true);
        assert_eq!(out.data["runtimeUnloadConfirmed"], false);
        InstallationGuard::acquire(&install)
            .unwrap()
            .require_idle()
            .unwrap();
        let (_, saved) = DurableStore::resume(&home, &install, &original.owner).unwrap();
        assert_eq!(saved.phase, terminal);
        assert_eq!(saved.pending_action, original.pending_action);
        assert_eq!(saved.input_messages, original.input_messages);
        assert_eq!(saved.request.as_ref().unwrap().code, "return 1");
        for retry in [command, "live resume", "live status"] {
            let repeated =
                host::dispatch_with(&invocation(&home, retry, &selector), &NoNative).unwrap();
            assert_eq!(repeated.data, out.data);
        }
    }
}
#[test]
fn zero_or_partial_publication_is_not_cancellable_and_prepared_is_not_abandonable() {
    let _advance = ADVANCE.lock().unwrap();
    let (home, install, mut session) = setup(Phase::Prepared);
    let selector = format!("MEM-{}", session.request.as_ref().unwrap().id);
    assert!(host::dispatch_with(&invocation(&home, "live abandon", &selector), &NoNative).is_err());
    let (mut store, _) = DurableStore::resume(&home, &install, &session.owner).unwrap();
    session.pending_action = Some(Action::PublishExecute);
    session.input_messages = Some(0);
    store.checkpoint(&session).unwrap();
    drop(store);
    let error =
        host::dispatch_with(&invocation(&home, "live cancel", &selector), &NoNative).unwrap_err();
    assert!(error.code.contains("cancel_requires"));
    assert!(
        InstallationGuard::acquire(&install)
            .unwrap()
            .require_idle()
            .is_err()
    );
}
#[test]
fn historical_settled_request_survives_later_abandoned_connection_without_native_calls() {
    let _advance = ADVANCE.lock().unwrap();
    let (home, install, mut session) = setup(Phase::Prepared);
    let a = format!("MEM-{}", session.request.as_ref().unwrap().id);
    let (mut store, _) = DurableStore::resume(&home, &install, &session.owner).unwrap();
    let request = session.request.as_mut().unwrap();
    request.result_digest = Some(
        store
            .persist_result(request, br#"{"ok":true,"result":{"a":19}}"#)
            .unwrap(),
    );
    session.phase = Phase::Settled;
    session.cleanup_complete = true;
    store.checkpoint(&session).unwrap();
    drop(store);
    let before = host::dispatch_with(&invocation(&home, "live status", &a), &NoNative).unwrap();
    let (mut store, _) = DurableStore::resume(&home, &install, &session.owner).unwrap();
    session
        .prepare("b".repeat(32), "return 2".into(), 15, u64::MAX)
        .unwrap();
    session.phase = Phase::ExecutePublished;
    session.pending_action = Some(Action::InputExecute);
    session.input_messages = Some(2);
    store.checkpoint(&session).unwrap();
    drop(store);
    let b = format!("MEM-{}", session.request.as_ref().unwrap().id);
    Metadata::open(&home, true)
        .unwrap()
        .put(
            &format!("native/request/{}", session.request.as_ref().unwrap().id),
            &serde_json::json!({"owner":session.owner}),
            None,
        )
        .unwrap();
    let abandoned = host::dispatch_with(&invocation(&home, "live abandon", &b), &NoNative).unwrap();
    assert_eq!(abandoned.data["operationId"], b);
    for command in ["live resume", "live ack", "live finish", "live status"] {
        let historical = host::dispatch_with(&invocation(&home, command, &a), &NoNative).unwrap();
        assert_eq!(historical.data, before.data);
        assert_eq!(historical.data["operationId"], a);
        assert_eq!(historical.data["report"]["result"]["result"]["a"], 19);
    }
}
#[test]
fn terminal_journal_survives_real_claim_deny_delete_and_resume_completes_retirement() {
    use std::os::windows::fs::OpenOptionsExt;
    let _advance = ADVANCE.lock().unwrap();
    let (home, install, original) = setup(Phase::ExecutePublished);
    let b = format!("MEM-{}", original.request.as_ref().unwrap().id);
    let claims = install.parent().unwrap().join(".lycheedev-claims.json");
    let deny_delete = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&claims)
        .unwrap();
    let error = host::dispatch_with(&invocation(&home, "live abandon", &b), &NoNative).unwrap_err();
    assert_eq!(error.code, "vault.publish", "{error:?}");
    assert!(
        error.message.contains("0x80070020") || error.message.contains("0x80070005"),
        "{error:?}"
    );
    let journal = home
        .join("live/v2")
        .join(format!("CON-{}.json", original.owner));
    let bytes = fs::read(&journal).unwrap();
    let saved: Session = storage::read_json(&journal).unwrap();
    assert_eq!(saved.phase, Phase::Abandoned);
    assert!(
        InstallationGuard::acquire(&install)
            .unwrap()
            .require_idle()
            .is_err()
    );
    let pending = host::dispatch_with(&invocation(&home, "live status", &b), &NoNative).unwrap();
    assert_eq!(pending.data["ownershipReleased"], false);
    drop(deny_delete);
    let resumed = host::dispatch_with(&invocation(&home, "live resume", &b), &NoNative).unwrap();
    assert_eq!(resumed.data["ownershipReleased"], true);
    assert!(!resumed.complete);
    assert_eq!(
        fs::read(&journal).unwrap(),
        bytes,
        "terminal resume must not rewrite the receipt"
    );
    InstallationGuard::acquire(&install)
        .unwrap()
        .require_idle()
        .unwrap();
    let modified = fs::metadata(&claims).unwrap().modified().unwrap();
    assert_eq!(
        host::dispatch_with(&invocation(&home, "live resume", &b), &NoNative)
            .unwrap()
            .data,
        resumed.data
    );
    assert_eq!(
        fs::metadata(&claims).unwrap().modified().unwrap(),
        modified,
        "successful repeat must not rewrite claims"
    );
}
#[test]
fn queued_capacity_journal_preserves_previous_terminal_metadata_and_freezes_next_report_scope() {
    let _advance = ADVANCE.lock().unwrap();
    let (home, install, mut session) = setup(Phase::Prepared);
    let old = format!("MEM-{}", session.request.as_ref().unwrap().id);
    let (mut store, _) = DurableStore::resume(&home, &install, &session.owner).unwrap();
    let request = session.request.as_mut().unwrap();
    request.result_digest = Some(
        store
            .persist_result(request, br#"{"ok":true,"result":28}"#)
            .unwrap(),
    );
    session.sequence = 28;
    session.phase = Phase::Settled;
    session.cleanup_complete = true;
    store.checkpoint(&session).unwrap();
    drop(store);
    let old_view = host::dispatch_with(&invocation(&home, "live status", &old), &NoNative).unwrap();
    let metadata = home.join("live/v2/results").join(format!(
        "{}.metadata.json",
        session.request.as_ref().unwrap().id
    ));
    let bytes = fs::read(&metadata).unwrap();
    let (mut store, _) = DurableStore::resume(&home, &install, &session.owner).unwrap();
    session
        .queue_capacity(
            "b".repeat(32),
            "return 29".into(),
            17,
            "e".repeat(32),
            i64::MAX,
        )
        .unwrap();
    store.checkpoint(&session).unwrap();
    drop(store);
    Metadata::open(&home, true)
        .unwrap()
        .put(
            &format!("native/request/{}", "b".repeat(32)),
            &serde_json::json!({"owner":session.owner}),
            None,
        )
        .unwrap();
    assert_eq!(
        fs::read(&metadata).unwrap(),
        bytes,
        "rotation context must not rewrite previous exactMEM receipt"
    );
    assert_eq!(
        host::dispatch_with(&invocation(&home, "live status", &old), &NoNative)
            .unwrap()
            .data,
        old_view.data
    );
    let next = host::dispatch_with(
        &invocation(&home, "live status", &format!("MEM-{}", "b".repeat(32))),
        &NoNative,
    )
    .unwrap();
    assert!(!next.complete);
    assert_eq!(next.data["report"]["state"], "unavailable");
    assert_eq!(next.data["cleanup"], "pending");
}
