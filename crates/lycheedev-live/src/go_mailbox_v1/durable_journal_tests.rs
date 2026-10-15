//! Offline filesystem fixtures. These do not open a game, HWND or native input.
//! Fixture trees are retained for the controlled runner's evidence policy.
use super::super::state::{
    Blocker, CHANNEL_SCHEMA, CleanupMethod, InputAttempt, OperationPolicy, ResultBytes,
};
use super::super::{Identity, SCHEMA};
use super::*;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy)]
pub(super) enum Action {
    Fail,
    Cancel,
    Crash,
}
pub(super) struct Fault {
    pub(super) point: FaultPoint,
    action: Action,
}
impl Fault {
    pub(super) fn trigger(self, control: &OperationControl) -> ChannelResult<()> {
        match self.action {
            Action::Fail => Err(error("fixture.journal_io_failure")),
            Action::Cancel => {
                control.tracked_cancel_handle().unwrap().cancel();
                check_control(control)
            }
            Action::Crash => std::process::exit(91),
        }
    }
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    addon: PathBuf,
    paths: ChannelPaths,
    owner: WindowOwner,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-channel-journal-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self::at(root)
    }
    fn at(root: PathBuf) -> Self {
        let addon = root.join("addon-parent");
        fs::create_dir_all(&addon).unwrap();
        let paths = ChannelPaths::new(&root, &state().id).unwrap();
        let owner = WindowOwner {
            schema: OWNER_SCHEMA.into(),
            workspace_id: paths.workspace_id().unwrap(),
            resource: "window/17/18/19".into(),
            operation_id: paths.connection_id.clone(),
            intent_sha256: "a".repeat(64),
        };
        Self {
            root,
            addon,
            paths,
            owner,
        }
    }
    fn publish_fixture_claim(&self) {
        let scope = self.scope();
        fs::create_dir_all(&scope).unwrap();
        fs::write(self.marker(), json(&self.owner).unwrap()).unwrap();
    }
    fn scope(&self) -> PathBuf {
        fs::canonicalize(&self.addon)
            .unwrap()
            .join(".lycheedev-window-owners")
    }
    fn marker(&self) -> PathBuf {
        self.scope().join(format!(
            "{}.json",
            storage::sha256(self.owner.resource.as_bytes())
        ))
    }
    fn acquire(&self, control: &OperationControl) -> ChannelResult<DurableChannelJournal> {
        DurableChannelJournal::acquire(self.paths.clone(), &self.addon, self.owner.clone(), control)
    }
    fn open(&self) -> DurableChannelJournal {
        // Model the fixed Go admission order: durable initial log, then claim.
        // This is fixture preparation, not the production new-claim API.
        if !self.paths.log.exists() {
            self.seed([state()]);
        }
        self.publish_fixture_claim();
        self.acquire(&OperationControl::default()).unwrap()
    }
    fn seed(&self, states: impl IntoIterator<Item = State>) {
        let mut events = Vec::new();
        let mut log = Vec::new();
        for state in states {
            state.validate().unwrap();
            let line = prepare_event(&events, "fixture", &state).unwrap();
            events.push(serde_json::from_slice::<MemoryEvent>(&line).unwrap());
            log.extend(line);
        }
        fs::create_dir_all(self.paths.log.parent().unwrap()).unwrap();
        fs::write(&self.paths.log, log).unwrap();
    }
}
fn state() -> State {
    State {
        schema: CHANNEL_SCHEMA.into(),
        id: format!("CON-{}", "1".repeat(32)),
        owner: "1".repeat(32),
        identity: Identity {
            schema: SCHEMA.into(),
            runtime: "2".repeat(32),
            owner: String::new(),
            fence: 0,
            next_slot: 1,
            slots: 500,
            inventory: None,
            input_state: SCHEMA.into(),
            character: "c".into(),
            realm: "r".into(),
            guid: "g".into(),
            build: "b".into(),
            product: "retail".into(),
            release: "3.2.0".into(),
        },
        ..State::default()
    }
}
fn with_operation(request: &str, completed: bool) -> State {
    let mut state = state();
    let mut op = Operation::prepared(
        format!("LMO-{}", "3".repeat(32)),
        "4".repeat(32),
        "return '<&>'".into(),
        30,
        OperationPolicy::Opaque,
    );
    op.request = request.into();
    op.stage = if completed {
        OperationStage::Complete
    } else {
        OperationStage::ResultVerified
    };
    op.cleanup_method = if completed {
        CleanupMethod::Released
    } else {
        CleanupMethod::None
    };
    op.prepared_nonce = "5".repeat(32);
    op.challenge = "6".repeat(32);
    op.result_bytes = ResultBytes(b" { \"z\": 1.00, \"a\": \"<&>\" } \n".to_vec());
    op.report_bytes = op.result_bytes.0.len() as u32;
    op.report_checksum = checksum(&op.result_bytes.0);
    state.operation = Some(op);
    state.validate().unwrap();
    state
}
fn assert_code<T>(result: ChannelResult<T>, code: &str) {
    let error = match result {
        Ok(_) => panic!("expected {code}"),
        Err(error) => error,
    };
    assert!(
        error.any_cause(&|e| matches!(e, ChannelError::Contract(c) if c.0 == code)),
        "{error:?}"
    );
}
fn inject(journal: &mut DurableChannelJournal, point: FaultPoint, action: Action) {
    journal.fault = Some(Fault { point, action });
}

#[test]
fn go_mailbox_v1_durable_paths_and_readonly_missing_do_not_create_state() {
    let f = Fixture::new();
    assert_eq!(
        f.paths.log,
        fs::canonicalize(&f.root)
            .unwrap()
            .join(".lycheedev/live/connections")
            .join(format!("{}.jsonl", state().id))
    );
    assert_eq!(
        f.paths.artifacts,
        f.paths.log.parent().unwrap().join("artifacts")
    );
    assert_eq!(
        f.paths.results,
        f.paths
            .log
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("results")
    );
    let control = OperationControl::default();
    assert_code(load(&f.paths, &control), "live.channel_journal_missing");
    assert!(
        historical_request(&f.paths, "request", &control)
            .unwrap()
            .is_none()
    );
    assert!(!f.root.join(".lycheedev").exists());
    assert!(!f.scope().exists());
    assert!(f.acquire(&control).is_err());
    assert!(!f.scope().exists());
    assert_code(
        ChannelPaths::new(&f.root, "../outside"),
        "live.channel_connection_id_required",
    );
}

#[test]
fn go_mailbox_v1_durable_lease_is_exclusive_and_retains_claim_and_lock_files() {
    let f = Fixture::new();
    let mut first = f.open();
    let control = OperationControl::default();
    assert_code(f.acquire(&control), "journal.resource_busy");
    let admission = f
        .scope()
        .join(format!("{}.lock", storage::sha256(b"window-admission")));
    let execution = f.scope().join(format!(
        "{}.lock",
        storage::sha256(b"execution:window/17/18/19")
    ));
    assert!(admission.is_file() && execution.is_file());
    first.append(&control, "connect_intent", &state()).unwrap();
    assert_eq!(load(&f.paths, &control).unwrap(), state());
    first.close().unwrap();
    first.close().unwrap();
    assert_code(first.load(&control), "journal.window_run_closed");
    let mut next = f.acquire(&control).unwrap();
    assert_eq!(next.load(&control).unwrap(), state());
    next.close().unwrap();
    assert_eq!(fs::read(f.marker()).unwrap(), json(&f.owner).unwrap());
    assert!(admission.is_file() && execution.is_file());
}

#[test]
fn go_mailbox_v1_durable_admission_wait_is_cancel_and_deadline_bounded() {
    let f = Fixture::new();
    f.seed([state()]);
    f.publish_fixture_claim();
    let held = Lease::acquire(
        &f.scope(),
        "window-admission",
        false,
        &OperationControl::default(),
    )
    .unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(45));
    let before = Instant::now();
    assert!(matches!(
        f.acquire(&control),
        Err(ChannelError::DeadlineExceeded)
    ));
    assert!(before.elapsed() < Duration::from_secs(2));
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        cancel.cancel();
    });
    assert!(matches!(f.acquire(&control), Err(ChannelError::Cancelled)));
    worker.join().unwrap();
    held.close().unwrap();
    let mut journal = f.acquire(&OperationControl::default()).unwrap();
    journal.close().unwrap();
}

#[test]
fn go_mailbox_v1_durable_every_claim_field_is_authenticated_before_writes() {
    for field in [
        "schema",
        "workspaceId",
        "resource",
        "operationId",
        "intentSha256",
    ] {
        let f = Fixture::new();
        let mut journal = f.open();
        let control = OperationControl::default();
        journal.append(&control, "initial", &state()).unwrap();
        let original = fs::read(&f.paths.log).unwrap();
        let mut owner = serde_json::to_value(&f.owner).unwrap();
        owner[field] = serde_json::json!("changed");
        // Serialize the typed record to retain field order; changed values are
        // authenticated, not rejected just because key ordering was different.
        let changed: WindowOwner = serde_json::from_value(owner).unwrap();
        fs::write(f.marker(), json(&changed).unwrap()).unwrap();
        assert!(
            journal
                .append(&control, "must_not_append", &state())
                .is_err(),
            "{field}"
        );
        assert_eq!(fs::read(&f.paths.log).unwrap(), original);
    }
    let f = Fixture::new();
    f.seed([state()]);
    f.publish_fixture_claim();
    let mut bytes = json(&f.owner).unwrap();
    bytes.push(b'\n');
    fs::write(f.marker(), bytes).unwrap();
    assert_code(
        f.acquire(&OperationControl::default()),
        "journal.invalid_window_owner",
    );
}

#[test]
fn go_mailbox_v1_durable_artifacts_precede_log_and_result_keeps_original_bytes() {
    let f = Fixture::new();
    let mut journal = f.open();
    let control = OperationControl::default();
    let s = with_operation("first request", false);
    journal.append(&control, "result_verified", &s).unwrap();
    let disk = read_segment(&f.paths.log, &control).unwrap();
    let snap: State = serde_json::from_str(disk.events.last().unwrap().data.get()).unwrap();
    assert!(snap.operation.as_ref().unwrap().code.is_empty());
    assert!(snap.operation.as_ref().unwrap().result_bytes.0.is_empty());
    for digest in snap.artifacts.values() {
        assert!(f.paths.artifact(digest).unwrap().is_file());
    }
    assert_eq!(journal.load(&control).unwrap(), s);
    journal
        .write_result(&control, s.operation.as_ref().unwrap())
        .unwrap();
    let op = s.operation.as_ref().unwrap();
    let bytes = fs::read(f.paths.results.join(format!("{}.json", op.id))).unwrap();
    assert_eq!(bytes, json(op).unwrap());
    let persisted: Operation = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(persisted.result_bytes, op.result_bytes);
    assert_eq!(persisted.report_checksum, checksum(&op.result_bytes.0));
    assert!(!String::from_utf8(bytes).unwrap().contains("\"result\":"));
}

#[test]
fn go_mailbox_v1_durable_artifact_failure_and_cancel_leave_log_unchanged() {
    for action in [Action::Fail, Action::Cancel] {
        let f = Fixture::new();
        let mut journal = f.open();
        let control = OperationControl::default();
        journal.append(&control, "first", &state()).unwrap();
        let original = fs::read(&f.paths.log).unwrap();
        inject(&mut journal, FaultPoint::ArtifactDurable, action);
        assert!(
            journal
                .append(&control, "result", &with_operation("r", true))
                .is_err()
        );
        assert_eq!(fs::read(&f.paths.log).unwrap(), original);
        assert!(fs::read_dir(&f.paths.artifacts).unwrap().next().is_some());
        assert_code(
            journal.append(&OperationControl::default(), "retry", &state()),
            "live.channel_journal_reload_required",
        );
    }
}

#[test]
fn go_mailbox_v1_durable_partial_or_uncertain_append_never_permits_another_write() {
    for point in [
        FaultPoint::BeforeAppend,
        FaultPoint::AppendPartial,
        FaultPoint::AppendWritten,
        FaultPoint::AppendSynced,
        FaultPoint::AppendClosed,
    ] {
        let f = Fixture::new();
        let mut journal = f.open();
        let control = OperationControl::default();
        journal.append(&control, "first", &state()).unwrap();
        let original = fs::read(&f.paths.log).unwrap();
        inject(&mut journal, point, Action::Fail);
        assert!(journal.append(&control, "second", &state()).is_err());
        let retained = fs::read(&f.paths.log).unwrap();
        assert!(retained.starts_with(&original));
        assert_code(
            journal.append(&control, "must_not_follow", &state()),
            "live.channel_journal_reload_required",
        );
        assert_eq!(fs::read(&f.paths.log).unwrap(), retained);
        journal.close().unwrap();
        if point == FaultPoint::AppendPartial {
            assert_code(f.acquire(&control), "journal.memory_torn_tail");
            assert_eq!(fs::read(&f.paths.log).unwrap(), retained);
        } else {
            let mut resumed = f.acquire(&control).unwrap();
            assert_eq!(resumed.load(&control).unwrap(), state());
            let count = read_log_bytes(&original).events.len()
                + usize::from(point != FaultPoint::BeforeAppend);
            assert_eq!(
                read_segment(&f.paths.log, &control).unwrap().events.len(),
                count
            );
            resumed.close().unwrap();
        }
    }
}

#[test]
fn go_mailbox_v1_durable_corruption_and_missing_artifact_fail_closed_without_repair() {
    let f = Fixture::new();
    let mut journal = f.open();
    let control = OperationControl::default();
    journal
        .append(&control, "result", &with_operation("r", true))
        .unwrap();
    journal.close().unwrap();
    let original = fs::read(&f.paths.log).unwrap();
    for suffix in [b"{".as_slice(), b"{}\n".as_slice()] {
        let mut damaged = original.clone();
        damaged.extend_from_slice(suffix);
        fs::write(&f.paths.log, &damaged).unwrap();
        assert!(f.acquire(&control).is_err());
        assert!(load(&f.paths, &control).is_err());
        assert!(historical_request(&f.paths, "absent", &control).is_err());
        assert_eq!(fs::read(&f.paths.log).unwrap(), damaged);
    }
    fs::write(&f.paths.log, &original).unwrap();
    let segment = read_segment(&f.paths.log, &control).unwrap();
    let snap: State = serde_json::from_str(segment.events.last().unwrap().data.get()).unwrap();
    let artifact = f
        .paths
        .artifact(snap.artifacts.get("result").unwrap())
        .unwrap();
    // Retain original bytes under a fixture-only name rather than deleting them.
    fs::rename(&artifact, artifact.with_extension("preserved")).unwrap();
    assert!(matches!(
        load(&f.paths, &control),
        Err(ChannelError::External { .. })
    ));
    fs::write(&artifact, b"corrupt").unwrap();
    assert_code(load(&f.paths, &control), "live.channel_artifact_corrupt");
    assert_eq!(fs::read(&f.paths.log).unwrap(), original);
}

#[test]
fn go_mailbox_v1_durable_append_count_size_and_line_limits() {
    let control = OperationControl::default();
    let f = Fixture::new();
    f.seed(std::iter::repeat_n(state(), MAX_EVENTS));
    let original = fs::read(&f.paths.log).unwrap();
    let mut journal = f.open();
    assert_code(
        journal.append(&control, "over_limit", &state()),
        "journal.memory_log_limit",
    );
    assert_eq!(fs::read(&f.paths.log).unwrap(), original);
    let f = Fixture::new();
    fs::create_dir_all(f.paths.log.parent().unwrap()).unwrap();
    File::create(&f.paths.log)
        .unwrap()
        .set_len((MAX_LOG_BYTES + 1) as u64)
        .unwrap();
    assert_code(load(&f.paths, &control), "journal.memory_log_limit");
    for extra in [0, 1, 2] {
        let f = Fixture::new();
        let mut journal = f.open();
        let original = fs::read(&f.paths.log).unwrap();
        let previous = read_log_bytes(&original).events;
        let base = prepare_event(&previous, "", &state()).unwrap().len();
        let kind = "x".repeat(MAX_EVENT_BYTES - base + extra);
        if extra < 2 {
            journal.append(&control, &kind, &state()).unwrap();
            assert_eq!(
                fs::metadata(&f.paths.log).unwrap().len(),
                (original.len() + MAX_EVENT_BYTES + extra) as u64
            );
            if extra == 0 {
                assert_eq!(journal.load(&control).unwrap(), state());
            } else {
                // Fixed Go writes exactly 2 MiB JSON plus LF, then its reader
                // rejects the LF-inclusive line. Preserve this incompatibility.
                assert_code(journal.load(&control), "journal.memory_log_limit");
            }
        } else {
            assert_code(
                journal.append(&control, &kind, &state()),
                "journal.memory_log_limit",
            );
            assert_eq!(fs::read(&f.paths.log).unwrap(), original);
        }
    }
}

#[test]
fn go_mailbox_v1_durable_checkpoint_retains_old_request_and_archive_chain() {
    let f = Fixture::new();
    let old = with_operation("old request", true);
    f.seed(std::iter::repeat_n(old.clone(), CHECKPOINT_EVENTS));
    let before = fs::read(&f.paths.log).unwrap();
    let mut journal = f.open();
    let control = OperationControl::default();
    let mut current = old.clone();
    journal.checkpoint(&control, &mut current, false).unwrap();
    assert!(!current.archive.is_empty());
    assert_eq!(
        fs::read(f.paths.history.join(format!("{}.jsonl", current.archive))).unwrap(),
        before
    );
    assert_eq!(
        read_segment(&f.paths.log, &control).unwrap().events.len(),
        1
    );
    assert_eq!(journal.load(&control).unwrap(), current);
    let first_archive = current.archive.clone();
    current.operation = with_operation("new request", true).operation;
    journal.append(&control, "next_request", &current).unwrap();
    assert_eq!(
        journal
            .historical_request(&control, "old request")
            .unwrap()
            .unwrap()
            .operation
            .unwrap()
            .request,
        "old request"
    );
    assert!(
        journal
            .historical_request(&control, "unknown")
            .unwrap()
            .is_none()
    );
    journal.close().unwrap();
    // Rebuild a long active fixture that still names its older durable segment.
    f.seed(std::iter::repeat_n(current.clone(), CHECKPOINT_EVENTS));
    let mut journal = f.acquire(&control).unwrap();
    journal.checkpoint(&control, &mut current, false).unwrap();
    assert_ne!(current.archive, first_archive);
    assert!(
        f.paths
            .history
            .join(format!("{first_archive}.jsonl"))
            .is_file()
    );
    assert_eq!(
        journal
            .historical_request(&control, "old request")
            .unwrap()
            .unwrap()
            .operation
            .unwrap()
            .result_bytes,
        old.operation.unwrap().result_bytes
    );
    journal.close().unwrap();
    let oldest = f.paths.history.join(format!("{first_archive}.jsonl"));
    let preserved = oldest.with_extension("retained");
    fs::rename(&oldest, &preserved).unwrap();
    assert_code(
        historical_request(&f.paths, "unknown", &control),
        "live.channel_journal_missing",
    );
    // Go returns a recent match without opening unrelated older history.
    assert_eq!(
        historical_request(&f.paths, "new request", &control)
            .unwrap()
            .unwrap()
            .operation
            .unwrap()
            .request,
        "new request"
    );
    fs::copy(&preserved, &oldest).unwrap();
    let mut corrupt = fs::read(&oldest).unwrap();
    corrupt[30] ^= 1;
    fs::write(&oldest, corrupt).unwrap();
    assert!(historical_request(&f.paths, "old request", &control).is_err());
}

#[test]
fn go_mailbox_v1_durable_checkpoint_faults_preserve_original_or_named_predecessor() {
    for point in [
        FaultPoint::ArchiveDurable,
        FaultPoint::BeforeCheckpointReplace,
        FaultPoint::CheckpointDurable,
    ] {
        let f = Fixture::new();
        f.seed(std::iter::repeat_n(state(), CHECKPOINT_EVENTS));
        let original = fs::read(&f.paths.log).unwrap();
        let expected = read_log_bytes(&original)
            .events
            .last()
            .unwrap()
            .checksum
            .clone();
        let mut journal = f.open();
        let control = OperationControl::default();
        let mut state = state();
        inject(&mut journal, point, Action::Fail);
        assert!(journal.checkpoint(&control, &mut state, false).is_err());
        assert!(state.archive.is_empty()); // Go restores the caller on failure
        assert_eq!(
            fs::read(f.paths.history.join(format!("{expected}.jsonl"))).unwrap(),
            original
        );
        journal.close().unwrap();
        let mut resumed = f.acquire(&control).unwrap();
        let mut loaded = resumed.load(&control).unwrap();
        if point == FaultPoint::CheckpointDurable {
            assert_eq!(loaded.archive, expected);
            assert_eq!(
                read_segment(&f.paths.log, &control).unwrap().events.len(),
                1
            );
        } else {
            assert!(loaded.archive.is_empty());
            assert_eq!(fs::read(&f.paths.log).unwrap(), original);
            resumed.checkpoint(&control, &mut loaded, false).unwrap();
            assert_eq!(loaded.archive, expected);
        }
        resumed.close().unwrap();
    }
}

#[test]
fn go_mailbox_v1_durable_checkpoint_never_replaces_conflicting_archive() {
    let f = Fixture::new();
    f.seed(std::iter::repeat_n(state(), CHECKPOINT_EVENTS));
    let original = fs::read(&f.paths.log).unwrap();
    let expected = read_log_bytes(&original)
        .events
        .last()
        .unwrap()
        .checksum
        .clone();
    let archive = f.paths.history.join(format!("{expected}.jsonl"));
    fs::create_dir(&f.paths.history).unwrap();
    fs::write(&archive, b"foreign bytes").unwrap();
    let mut journal = f.open();
    let mut state = state();
    assert_code(
        journal.checkpoint(&OperationControl::default(), &mut state, false),
        "journal.memory_checkpoint_conflict",
    );
    assert_eq!(fs::read(&archive).unwrap(), b"foreign bytes");
    assert_eq!(fs::read(&f.paths.log).unwrap(), original);
    assert!(state.archive.is_empty());
}

#[test]
fn go_mailbox_v1_durable_pending_checkpoint_requires_proven_zero_and_preserves_facts() {
    let mut state = with_operation("pending", false);
    state.operation.as_mut().unwrap().stage = OperationStage::Running;
    state.input = Some(
        serde_json::from_value::<InputAttempt>(serde_json::json!({
            "id": "7".repeat(32), "exchange": "8".repeat(32), "runtime": "2".repeat(32),
            "kind": "invoke", "afterMillis": 123, "outcome": {"disposition":"not_sent"}
        }))
        .unwrap(),
    );
    state.validate().unwrap();
    let f = Fixture::new();
    f.seed(std::iter::repeat_n(state.clone(), CHECKPOINT_EVENTS));
    let control = OperationControl::default();
    let mut journal = f.open();
    let original = fs::read(&f.paths.log).unwrap();
    assert!(
        journal
            .checkpoint(&control, &mut state, false)
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(fs::read(&f.paths.log).unwrap(), original);
    journal.close().unwrap();
    let mut journal = f.acquire(&control).unwrap();
    let original_state = state.clone();
    journal.checkpoint(&control, &mut state, true).unwrap();
    assert_eq!(state.input, original_state.input);
    assert_eq!(state.operation, original_state.operation);
    assert_eq!(state.progress_version, original_state.progress_version);
    let mut loaded = journal.load(&control).unwrap();
    loaded.archive.clear();
    assert_eq!(loaded, original_state);
}

#[test]
fn go_mailbox_v1_durable_foreign_connection_or_external_head_change_is_rejected() {
    let f = Fixture::new();
    let mut journal = f.open();
    let control = OperationControl::default();
    journal.append(&control, "initial", &state()).unwrap();
    let mut different = state();
    different.owner = "9".repeat(32);
    different.id = format!("CON-{}", different.owner);
    assert_code(
        journal.append(&control, "foreign", &different),
        "live.channel_project_owner_mismatch",
    );
    journal.close().unwrap();
    let mut journal = f.acquire(&control).unwrap();
    let mut bytes = fs::read(&f.paths.log).unwrap();
    let events = read_log_bytes(&bytes).events;
    bytes.extend(prepare_event(&events, "external", &state()).unwrap());
    fs::write(&f.paths.log, &bytes).unwrap();
    assert_code(
        journal.append(&control, "stale", &state()),
        "journal.memory_checkpoint_conflict",
    );
    assert_eq!(fs::read(&f.paths.log).unwrap(), bytes);
}

#[test]
fn go_mailbox_v1_durable_process_helper() {
    let Some(root) = std::env::var_os("LYCHEEDEV_DURABLE_JOURNAL_FIXTURE") else {
        return;
    };
    let f = Fixture::at(PathBuf::from(root));
    let control = OperationControl::default();
    let mut journal = f.acquire(&control).unwrap();
    let mode = std::env::var("LYCHEEDEV_DURABLE_JOURNAL_MODE").unwrap();
    if mode == "lease" {
        fs::write(f.root.join("helper-ready"), b"held").unwrap();
        let mut input = [0u8; 1];
        std::io::stdin().read_exact(&mut input).unwrap();
        std::process::exit(91);
    }
    let point = match mode.as_str() {
        "artifact" => FaultPoint::ArtifactDurable,
        "archive" => FaultPoint::ArchiveDurable,
        "checkpoint" => FaultPoint::CheckpointDurable,
        _ => panic!("unknown child mode"),
    };
    inject(&mut journal, point, Action::Crash);
    if mode == "artifact" {
        journal
            .append(&control, "crash", &with_operation("crash", true))
            .unwrap();
    } else {
        journal.checkpoint(&control, &mut state(), false).unwrap();
    }
    panic!("crash cut did not execute");
}
struct OwnedChild(std::process::Child);
impl std::ops::Deref for OwnedChild {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Reap this exact owned child even when an assertion unwinds.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(f: &Fixture, mode: &str) -> OwnedChild {
    let output = File::create(f.root.join(format!("child-{mode}.log"))).unwrap();
    OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .arg("go_mailbox_v1_durable_process_helper")
            .arg("--nocapture")
            .env("LYCHEEDEV_DURABLE_JOURNAL_FIXTURE", &f.root)
            .env("LYCHEEDEV_DURABLE_JOURNAL_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap(),
    )
}
fn wait_child(child: &mut std::process::Child) -> std::process::ExitStatus {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= until {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("fixture child exceeded deadline; evidence retained");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn go_mailbox_v1_durable_process_exit_releases_only_lease_not_claim() {
    let f = Fixture::new();
    f.seed([state()]);
    f.publish_fixture_claim();
    let mut child = child(&f, "lease");
    let until = Instant::now() + Duration::from_secs(10);
    while !f.root.join("helper-ready").exists() {
        if child.try_wait().unwrap().is_some() || Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child did not acquire lease; evidence retained");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_code(
        f.acquire(&OperationControl::default()),
        "journal.resource_busy",
    );
    child.stdin.take().unwrap().write_all(b"x").unwrap();
    assert_eq!(wait_child(&mut child).code(), Some(91));
    let mut recovered = f.acquire(&OperationControl::default()).unwrap();
    assert_eq!(
        recovered.load(&OperationControl::default()).unwrap(),
        state()
    );
    recovered.close().unwrap();
    assert_eq!(fs::read(f.marker()).unwrap(), json(&f.owner).unwrap());
}

#[test]
fn go_mailbox_v1_durable_crash_cuts_reopen_only_original_or_durable_checkpoint() {
    for mode in ["artifact", "archive", "checkpoint"] {
        let f = Fixture::new();
        f.seed(std::iter::repeat_n(
            state(),
            if mode == "artifact" {
                1
            } else {
                CHECKPOINT_EVENTS
            },
        ));
        f.publish_fixture_claim();
        let original = fs::read(&f.paths.log).unwrap();
        let expected = read_log_bytes(&original)
            .events
            .last()
            .unwrap()
            .checksum
            .clone();
        let mut child = child(&f, mode);
        assert_eq!(wait_child(&mut child).code(), Some(91));
        let control = OperationControl::default();
        let mut journal = f.acquire(&control).unwrap();
        let recovered = journal.load(&control).unwrap();
        if mode == "checkpoint" {
            assert_eq!(recovered.archive, expected);
            assert_eq!(
                read_segment(&f.paths.log, &control).unwrap().events.len(),
                1
            );
        } else {
            assert_eq!(fs::read(&f.paths.log).unwrap(), original);
            assert!(recovered.archive.is_empty());
        }
        if mode != "artifact" {
            assert_eq!(
                fs::read(f.paths.history.join(format!("{expected}.jsonl"))).unwrap(),
                original
            );
        }
        journal.close().unwrap();
    }
}

#[test]
fn go_mailbox_v1_durable_reader_matches_fixed_go_chain_bytes() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/rust-parity/mailbox-journal-golden.json"
    ))
    .unwrap();
    assert_eq!(
        golden["oracleCommit"],
        "3057a9666ab078270b63c7f06de748069b0d1f4b"
    );
    let f = Fixture::new();
    fs::create_dir_all(f.paths.log.parent().unwrap()).unwrap();
    fs::write(&f.paths.log, golden["chain"].as_str().unwrap().as_bytes()).unwrap();
    let segment = read_segment(&f.paths.log, &OperationControl::default()).unwrap();
    for (i, event) in segment.events.iter().enumerate() {
        assert_eq!(event.checksum, golden["checksums"][i].as_str().unwrap());
        if i != 0 {
            assert_eq!(event.previous, segment.events[i - 1].checksum);
        }
    }
    assert_eq!(segment.bytes, golden["chain"].as_str().unwrap().as_bytes());
}

#[test]
fn go_mailbox_v1_durable_workspace_unicode_and_cross_workspace_claims() {
    let f = Fixture::new();
    // Go has simple-rune casing: U+0130 -> i and capital sigma -> sigma,
    // regardless of its position. Rust str::to_lowercase would differ.
    let root = f.root.join("Project-İ-ΟΣ");
    fs::create_dir(&root).unwrap();
    let paths = ChannelPaths::new(&root, &state().id).unwrap();
    let canonical = paths.project.to_str().unwrap();
    let ordinary = canonical.strip_prefix(r"\\?\").unwrap_or(canonical);
    let expected_path = ordinary.replace("Project-İ-ΟΣ", "project-i-οσ");
    // The generated fixture parent's spelling is already all lower-case except
    // Windows' drive letter, which Go also lower-cases.
    let expected: String = expected_path
        .chars()
        .map(|c| c.to_lowercase().next().unwrap())
        .collect();
    assert_eq!(
        paths.workspace_id().unwrap(),
        storage::sha256(expected.as_bytes())[..32]
    );
    f.seed([state()]);
    f.publish_fixture_claim();
    let mut foreign = f.owner.clone();
    foreign.workspace_id = paths.workspace_id().unwrap();
    assert_code(
        DurableChannelJournal::acquire(paths, &f.addon, foreign, &OperationControl::default()),
        "journal.resource_busy",
    );
}

#[test]
fn go_mailbox_v1_durable_checkpoint_rotates_at_byte_threshold_below_event_threshold() {
    let f = Fixture::new();
    let mut events = Vec::new();
    let mut bytes = Vec::new();
    while bytes.len() < CHECKPOINT_BYTES {
        let line = prepare_event(&events, &"x".repeat(1 << 20), &state()).unwrap();
        events.push(serde_json::from_slice(&line).unwrap());
        bytes.extend(line);
    }
    assert!(events.len() < CHECKPOINT_EVENTS);
    fs::create_dir_all(f.paths.log.parent().unwrap()).unwrap();
    fs::write(&f.paths.log, &bytes).unwrap();
    let mut journal = f.open();
    let mut state = state();
    journal
        .checkpoint(&OperationControl::default(), &mut state, false)
        .unwrap();
    assert_eq!(
        fs::read(f.paths.history.join(format!("{}.jsonl", state.archive))).unwrap(),
        bytes
    );
    assert_eq!(
        read_segment(&f.paths.log, &OperationControl::default())
            .unwrap()
            .events
            .len(),
        1
    );
}

#[test]
fn go_mailbox_v1_durable_history_segment_limit_and_expected_tail_are_enforced() {
    let f = Fixture::new();
    fs::create_dir_all(&f.paths.history).unwrap();
    let mut s = state();
    let mut bytes = prepare_event(&[], "oldest", &s).unwrap();
    for _ in 1..MAX_HISTORY_SEGMENTS {
        let checksum = read_log_bytes(&bytes).events[0].checksum.clone();
        fs::write(f.paths.history.join(format!("{checksum}.jsonl")), &bytes).unwrap();
        s.archive = checksum;
        bytes = prepare_event(&[], "checkpoint", &s).unwrap();
    }
    fs::write(&f.paths.log, &bytes).unwrap();
    let control = OperationControl::default();
    assert!(
        historical_request(&f.paths, "absent", &control)
            .unwrap()
            .is_none()
    );
    // The 1024 bound belongs to historical lookup, not the Go writer. A
    // checkpoint may create a further segment and must not pre-scan old ones.
    f.seed(std::iter::repeat_n(s.clone(), CHECKPOINT_EVENTS));
    let mut journal = f.open();
    journal.checkpoint(&control, &mut s, false).unwrap();
    journal.close().unwrap();
    assert_code(
        historical_request(&f.paths, "absent", &control),
        "live.channel_history_limit",
    );
    let replaced = f.paths.history.join(format!("{}.jsonl", s.archive));
    // This is a valid independent chain, but it is not the named predecessor.
    fs::write(
        &replaced,
        prepare_event(&[], "foreign chain", &state()).unwrap(),
    )
    .unwrap();
    assert_code(
        historical_request(&f.paths, "absent", &control),
        "live.channel_history_corrupt",
    );
}

#[test]
fn go_mailbox_v1_durable_nonregular_log_and_oversized_artifact_are_rejected() {
    let f = Fixture::new();
    fs::create_dir_all(&f.paths.log).unwrap();
    assert_code(
        load(&f.paths, &OperationControl::default()),
        "journal.memory_log_not_regular",
    );
    let f = Fixture::new();
    let mut journal = f.open();
    let control = OperationControl::default();
    let s = with_operation("request", true);
    let original = fs::read(&f.paths.log).unwrap();
    let artifacts = snapshot(&s).unwrap();
    let hash = artifacts.state.artifacts.get("result").unwrap();
    let path = f.paths.artifact(hash).unwrap();
    fs::create_dir_all(&f.paths.artifacts).unwrap();
    File::create(&path)
        .unwrap()
        .set_len((MAX_PAYLOAD + 1) as u64)
        .unwrap();
    assert_code(
        journal.append(&control, "must_not_append", &s),
        "live.channel_artifact_invalid",
    );
    assert_eq!(fs::read(&f.paths.log).unwrap(), original);
}

#[test]
fn go_mailbox_v1_durable_result_failure_keeps_prior_journal_and_verified_bytes() {
    let f = Fixture::new();
    let mut journal = f.open();
    let control = OperationControl::default();
    let s = with_operation("request", false);
    journal.append(&control, "result_verified", &s).unwrap();
    let original = fs::read(&f.paths.log).unwrap();
    inject(&mut journal, FaultPoint::ResultDurable, Action::Fail);
    assert!(
        journal
            .write_result(&control, s.operation.as_ref().unwrap())
            .is_err()
    );
    assert_eq!(fs::read(&f.paths.log).unwrap(), original);
    assert_code(
        journal.append(&control, "release_ready", &s),
        "live.channel_journal_reload_required",
    );
    journal.close().unwrap();
    let mut resumed = f.acquire(&control).unwrap();
    assert_eq!(resumed.load(&control).unwrap(), s);
    resumed
        .write_result(&control, s.operation.as_ref().unwrap())
        .unwrap();
    assert_eq!(fs::read(&f.paths.log).unwrap(), original);
}

#[test]
fn go_mailbox_v1_durable_existing_claim_never_reseeds_missing_or_empty_log() {
    let f = Fixture::new();
    // Deliberately invalid admission fixture: a surviving claim whose original
    // log was lost must never become a new connection through this adapter.
    f.publish_fixture_claim();
    let control = OperationControl::default();
    let marker = fs::read(f.marker()).unwrap();
    assert_code(f.acquire(&control), "live.channel_journal_missing");
    assert!(!f.paths.log.exists());
    assert_eq!(fs::read(f.marker()).unwrap(), marker);
    fs::create_dir_all(f.paths.log.parent().unwrap()).unwrap();
    fs::write(&f.paths.log, b"").unwrap();
    assert_code(f.acquire(&control), "live.channel_journal_missing");
    assert_eq!(fs::read(&f.paths.log).unwrap(), b"");
    assert_eq!(fs::read(f.marker()).unwrap(), marker);
    // Failed acquisition also releases its exact execution lease. Restoring
    // fixture evidence externally does not exercise a production reseed API.
    f.seed([state()]);
    let mut restored = f.acquire(&control).unwrap();
    restored.close().unwrap();
}

#[test]
fn go_mailbox_v1_durable_history_hit_does_not_hydrate_older_event_artifacts() {
    let f = Fixture::new();
    let prior = snapshot(&with_operation("older", true)).unwrap();
    let first = prepare_event(&[], "fixture_old", &prior.state).unwrap();
    let predecessor = vec![serde_json::from_slice::<MemoryEvent>(&first).unwrap()];
    let newer = with_operation("newer", true);
    let second = prepare_event(&predecessor, "fixture_new", &newer).unwrap();
    let mut bytes = first;
    bytes.extend(second);
    fs::create_dir_all(f.paths.log.parent().unwrap()).unwrap();
    fs::write(&f.paths.log, bytes).unwrap();
    let mut journal = f.open();
    let control = OperationControl::default();
    assert_eq!(
        journal.historical_request(&control, "newer").unwrap(),
        Some(newer)
    );
    // The older event's referenced artifacts were intentionally never created.
    assert!(matches!(
        journal.historical_request(&control, "older"),
        Err(ChannelError::External { .. })
    ));
    assert!(matches!(
        journal.historical_request(&control, "absent"),
        Err(ChannelError::External { .. })
    ));
    journal.close().unwrap();
}

#[test]
fn go_mailbox_v1_durable_checkpoint_does_not_scan_unrelated_missing_history() {
    let f = Fixture::new();
    let mut s = with_operation("current", true);
    s.archive = "a".repeat(64);
    f.seed(std::iter::repeat_n(s.clone(), CHECKPOINT_EVENTS));
    let before = fs::read(&f.paths.log).unwrap();
    let mut journal = f.open();
    let control = OperationControl::default();
    journal.checkpoint(&control, &mut s, false).unwrap();
    assert_eq!(
        fs::read(f.paths.history.join(format!("{}.jsonl", s.archive))).unwrap(),
        before
    );
    assert_eq!(
        journal.historical_request(&control, "current").unwrap(),
        Some(s)
    );
    assert_code(
        journal.historical_request(&control, "absent"),
        "live.channel_journal_missing",
    );
    journal.close().unwrap();
}

#[test]
fn go_mailbox_v1_durable_checkpoint_preserves_go_two_mib_json_write_boundary() {
    let f = Fixture::new();
    let mut s = state();
    s.blocker = Some(Blocker::default());
    let mut sizing = s.clone();
    sizing.archive = "a".repeat(64);
    let base_json = prepare_event(&[], "checkpoint", &sizing).unwrap().len() - 1;
    s.blocker.as_mut().unwrap().kind = "x".repeat(MAX_EVENT_BYTES - base_json);
    s.validate().unwrap();
    // Five readable records exceed 8 MiB. The ensuing checkpoint adds its
    // archive digest and lands at exactly 2 MiB JSON before the final newline.
    f.seed(std::iter::repeat_n(s.clone(), 5));
    let before = fs::read(&f.paths.log).unwrap();
    assert!(before.len() >= CHECKPOINT_BYTES);
    let mut journal = f.open();
    let control = OperationControl::default();
    journal.checkpoint(&control, &mut s, false).unwrap();
    let bytes = fs::read(&f.paths.log).unwrap();
    assert_eq!(bytes.len(), MAX_EVENT_BYTES + 1);
    let event: MemoryEvent = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(journal.head, event.checksum);
    assert_eq!(
        fs::read(f.paths.history.join(format!("{}.jsonl", s.archive))).unwrap(),
        before
    );
    // Known upstream inconsistency: writer succeeds, subsequent reader rejects
    // the LF-inclusive line. Hash capture must not index a failed replay.
    assert_code(journal.load(&control), "journal.memory_log_limit");
    journal.close().unwrap();
    assert_code(f.acquire(&control), "journal.memory_log_limit");
}

#[test]
fn go_mailbox_v1_durable_raw_byte_report_artifact_result_and_history_are_exact() {
    let f = Fixture::new();
    let mut journal = f.open();
    let control = OperationControl::default();
    let mut s = with_operation("raw report", false);
    let raw = b" { \"s\":\"\xff\", \"x\":1e400, \"x\":-0 } \n".to_vec();
    let op = s.operation.as_mut().unwrap();
    op.result_bytes = ResultBytes(raw.clone());
    op.report_bytes = raw.len() as u32;
    op.report_checksum = checksum(&raw);
    s.validate().unwrap();
    journal.append(&control, "result_verified", &s).unwrap();
    assert_eq!(
        journal
            .load(&control)
            .unwrap()
            .operation
            .unwrap()
            .result_bytes
            .0,
        raw
    );
    journal
        .write_result(&control, s.operation.as_ref().unwrap())
        .unwrap();
    let op = s.operation.as_ref().unwrap();
    let bytes = fs::read(f.paths.results.join(format!("{}.json", op.id))).unwrap();
    let persisted: Operation = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(persisted.result_bytes.0, raw);
    assert_eq!(persisted.report_checksum, checksum(&raw));
    let op = s.operation.as_mut().unwrap();
    op.stage = OperationStage::Complete;
    op.cleanup_method = CleanupMethod::Released;
    journal.append(&control, "complete", &s).unwrap();
    // Reach the actual checkpoint threshold through durable snapshots, then
    // remove this request from active state so history must hydrate its artifact.
    for _ in 0..CHECKPOINT_EVENTS {
        journal.append(&control, "fixture_progress", &s).unwrap();
    }
    journal.checkpoint(&control, &mut s, false).unwrap();
    assert!(!s.archive.is_empty());
    let archived = read_segment(
        &f.paths.history.join(format!("{}.jsonl", s.archive)),
        &control,
    )
    .unwrap();
    let snapshot: State = serde_json::from_str(archived.events.last().unwrap().data.get()).unwrap();
    assert!(
        snapshot
            .operation
            .as_ref()
            .unwrap()
            .result_bytes
            .0
            .is_empty()
    );
    assert_eq!(snapshot.artifacts["result"], storage::sha256(&raw));
    s.operation = with_operation("new request", true).operation;
    journal.append(&control, "next_request", &s).unwrap();
    // The first checkpoint itself still names the old request. Rotate again
    // so no active event can satisfy it; the reader must cross an archive.
    for _ in 0..CHECKPOINT_EVENTS {
        journal
            .append(&control, "fixture_new_progress", &s)
            .unwrap();
    }
    journal.checkpoint(&control, &mut s, false).unwrap();
    let active = read_segment(&f.paths.log, &control).unwrap();
    assert_eq!(active.events.len(), 1);
    for event in &active.events {
        let active_state: State = serde_json::from_str(event.data.get()).unwrap();
        assert_eq!(active_state.operation.unwrap().request, "new request");
    }
    journal.close().unwrap();
    let before = fs::read(&f.paths.log).unwrap();
    let historic = historical_request(&f.paths, "raw report", &control)
        .unwrap()
        .unwrap();
    assert_eq!(historic.operation.unwrap().result_bytes.0, raw);
    assert_eq!(fs::read(&f.paths.log).unwrap(), before);
}
