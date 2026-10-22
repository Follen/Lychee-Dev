//! Formal filesystem admission fixtures; never enumerate clients or send input.
//! Every unique fixture tree is retained for the controlled runner's evidence.
use super::super::{
    SCHEMA,
    journal::StateJournal,
    state::{Operation, OperationPolicy, OperationStage},
};
use super::*;
use std::{
    fs::File,
    io::{Read, Write},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

enum Action {
    Fail,
    Cancel,
    Crash,
    Rendezvous {
        ready: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    },
}
pub(super) struct Fault {
    pub(super) point: FaultPoint,
    action: Action,
}
impl Fault {
    pub(super) fn trigger(self, control: &OperationControl) -> ChannelResult<()> {
        match self.action {
            Action::Fail => Err(error("fixture.admission_io_failure")),
            Action::Cancel => {
                control.tracked_cancel_handle().unwrap().cancel();
                check_control(control)
            }
            Action::Crash => std::process::exit(91),
            Action::Rendezvous { ready, release } => {
                ready
                    .send(())
                    .map_err(|_| error("fixture.admission_rendezvous_failed"))?;
                let timeout =
                    Duration::from_secs(10).min(control.remaining().map_err(storage_error)?);
                release
                    .recv_timeout(timeout)
                    .map_err(|_| error("fixture.admission_rendezvous_failed"))?;
                check_control(control)
            }
        }
    }
}
fn inject(book: &mut ConnectionBook, point: FaultPoint, action: Action) {
    book.fault = Some(Fault { point, action });
}
struct FixedClock;
impl Clock for FixedClock {
    fn unix_millis(&self) -> i64 {
        1000
    }
}
struct Tokens(u64);
impl TokenSource for Tokens {
    fn token(&mut self) -> ChannelResult<String> {
        let value = format!("{:032x}", self.0);
        self.0 += 1;
        Ok(value)
    }
}
fn identity() -> Identity {
    Identity {
        schema: SCHEMA.into(),
        runtime: "2".repeat(32),
        owner: String::new(),
        fence: 0,
        next_slot: 1,
        slots: 500,
        inventory: None,
        input_state: SCHEMA.into(),
        character: "character".into(),
        realm: "realm".into(),
        guid: "guid".into(),
        build: "12.1.0.70000".into(),
        product: "retail".into(),
        release: "3.2.0".into(),
    }
}
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    target: ConnectionTarget,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-channel-admission-{}-{}-{}",
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
        let client = root.join("client");
        fs::create_dir_all(client.join("Interface/AddOns")).unwrap();
        let target = ConnectionTarget {
            client: ClientRecord {
                directory: client.to_str().unwrap().into(),
                product: "retail".into(),
                product_code: "wow".into(),
                full_build: "12.1.0.70000".into(),
                interface: 120100,
                toc: "Lychee Dev.toc".into(),
                identity_source: "version.txt".into(),
                ..ClientRecord::default()
            },
            window: WindowRecord {
                handle: 19,
                process_id: 17,
                process_started_at: 18,
                executable: client.join("Wow.exe").to_str().unwrap().into(),
                class: "GxWindowClass".into(),
                title: "World of Warcraft".into(),
            },
        };
        Self { root, target }
    }
    fn book(&self) -> ConnectionBook {
        ConnectionBook::open(&self.root).unwrap()
    }
    fn scope(&self) -> PathBuf {
        fs::canonicalize(self.target.addon_parent())
            .unwrap()
            .join(".lycheedev-window-owners")
    }
    fn marker(&self) -> PathBuf {
        marker(&self.scope(), &self.target.resource())
    }
    fn create(&self, book: &mut ConnectionBook) -> ConnectionRecord {
        self.create_number(book, 1).unwrap()
    }
    fn create_number(
        &self,
        book: &mut ConnectionBook,
        token: u64,
    ) -> AdmissionResult<ConnectionRecord> {
        book.create(
            self.target.clone(),
            identity(),
            &FixedClock,
            &mut Tokens(token),
            &OperationControl::default(),
        )
    }
    fn inspect(&self) -> OwnerInspection {
        inspect_window_owner(
            &self.target.addon_parent(),
            &self.target.resource(),
            &OperationControl::default(),
        )
    }
}
fn assert_code<T>(result: AdmissionResult<T>, code: &str) {
    let err = result.err().unwrap_or_else(|| panic!("expected {code}"));
    fn has(e: &AdmissionError, code: &str) -> bool {
        match e {
            AdmissionError::Channel(e) => {
                e.any_cause(&|e| matches!(e, ChannelError::Contract(c) if c.0 == code))
            }
            AdmissionError::Joined(es) => es.iter().any(|e| has(e, code)),
            _ => false,
        }
    }
    assert!(has(&err, code), "wanted {code}: {err:?}");
}
fn close_state(book: &ConnectionBook, id: &str, unknown: bool) -> State {
    let control = OperationControl::default();
    let (record, mut journal) = book.acquire(id, &control).unwrap();
    let mut state = record.state;
    state.closed = true;
    state.bound = false;
    if unknown {
        let mut op = Operation::prepared(
            format!("LMO-{}", "3".repeat(32)),
            "4".repeat(32),
            "return 1".into(),
            30,
            OperationPolicy::Opaque,
        );
        op.request = "original-request".into();
        op.stage = OperationStage::ExecutionUnknown;
        state.operation = Some(op);
    }
    journal
        .append(&control, "connection_closed", &state)
        .unwrap();
    journal.close().unwrap();
    state
}

#[test]
fn mailbox_admission_readonly_absence_and_project_open_create_nothing() {
    let f = Fixture::new();
    let book = f.book();
    assert!(!f.inspect().occupied);
    assert!(
        book.find_selected(
            &f.target,
            &TargetFilter::default(),
            &OperationControl::default()
        )
        .unwrap()
        .is_none()
    );
    assert!(!f.scope().exists());
    assert!(!f.root.join(".lycheedev").exists());
    assert!(
        book.status("../outside", &OperationControl::default())
            .is_err()
    );
    assert!(!f.root.join(".lycheedev").exists());
}

#[test]
fn mailbox_admission_new_connection_has_exact_paths_owner_and_first_log() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    assert_eq!(
        record.paths.log_path(),
        fs::canonicalize(&f.root).unwrap().join(format!(
            ".lycheedev/live/connections/{}.jsonl",
            record.state.id
        ))
    );
    assert_eq!(record.metadata.owner.resource, "window/17/18/19");
    assert_eq!(
        record.metadata.owner.intent_sha256,
        storage::sha256(format!("window/17/18/19/{}", record.state.id).as_bytes())
    );
    assert_eq!(
        fs::read(f.marker()).unwrap(),
        json(&record.metadata.owner).unwrap()
    );
    assert_eq!(record.state.progress_version, 1);
    let first = fs::read(record.paths.log_path()).unwrap();
    let replay = super::super::journal::read_log_bytes(&first);
    assert!(replay.failure.is_none());
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].kind, "connection_created");
    assert_eq!(
        book.status(&record.state.id, &OperationControl::default())
            .unwrap()
            .state,
        record.state
    );
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(ConnectionBook::target_path(&record.paths)).unwrap())
            .unwrap();
    assert_eq!(saved["target"]["window"]["handle"], "19");
    assert_eq!(saved["target"]["window"]["processStartedAt"], "18");
    assert!(saved["target"]["client"].get("interfaceKnown").is_none());
}

#[test]
fn mailbox_admission_reuse_preserves_con_identity_log_and_absolute_budget() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    let bytes = fs::read(record.paths.log_path()).unwrap();
    for _ in 0..2 {
        let found = book
            .find_selected(
                &f.target,
                &TargetFilter::default(),
                &OperationControl::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(found.state, record.state);
        let (reloaded, mut journal) = book
            .acquire(&found.state.id, &OperationControl::default())
            .unwrap();
        assert_eq!(reloaded.state.connect_budget, record.state.connect_budget);
        journal.close().unwrap();
    }
    assert_eq!(fs::read(record.paths.log_path()).unwrap(), bytes);
    assert!(f.create_number(&mut book, 2).err().unwrap().is_busy());
    assert!(
        !book
            .paths(&format!("CON-{:032x}", 2))
            .unwrap()
            .log_path()
            .exists()
    );
}

#[test]
fn mailbox_admission_target_filters_and_same_build_instances_never_cross() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    for filter in [
        TargetFilter {
            build: "other".into(),
            ..TargetFilter::default()
        },
        TargetFilter {
            product: "classic".into(),
            ..TargetFilter::default()
        },
        TargetFilter {
            character: "other".into(),
            ..TargetFilter::default()
        },
        TargetFilter {
            realm: "other".into(),
            ..TargetFilter::default()
        },
    ] {
        assert_code(
            book.find_selected(&f.target, &filter, &OperationControl::default()),
            "live.channel_target_mismatch",
        );
    }
    let mut other = f.target.clone();
    other.window.process_id += 1;
    assert!(
        book.find_selected(
            &other,
            &TargetFilter::default(),
            &OperationControl::default()
        )
        .unwrap()
        .is_none()
    );
    let other_record = book
        .create(
            other,
            identity(),
            &FixedClock,
            &mut Tokens(2),
            &OperationControl::default(),
        )
        .unwrap();
    assert_ne!(record.metadata.owner, other_record.metadata.owner);
    let mut changed = f.target.clone();
    changed.window.title.push('x');
    assert_code(
        book.find_selected(
            &changed,
            &TargetFilter::default(),
            &OperationControl::default(),
        ),
        "live.channel_ownership_changed",
    );
}

#[test]
fn mailbox_admission_foreign_project_and_copied_metadata_fail_closed() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    let other = f.root.join("other-project");
    fs::create_dir(&other).unwrap();
    let mut foreign = ConnectionBook::open(&other).unwrap();
    let err = foreign
        .find_selected(
            &f.target,
            &TargetFilter::default(),
            &OperationControl::default(),
        )
        .err()
        .unwrap();
    assert!(matches!(
        err,
        AdmissionError::Occupied { foreign: true, .. }
    ));
    assert!(f.create_number(&mut foreign, 2).err().unwrap().is_busy());
    let copy = foreign.paths(&record.state.id).unwrap();
    fs::create_dir_all(copy.log_path().parent().unwrap()).unwrap();
    fs::copy(
        ConnectionBook::target_path(&record.paths),
        ConnectionBook::target_path(&copy),
    )
    .unwrap();
    fs::copy(record.paths.log_path(), copy.log_path()).unwrap();
    assert_code(
        foreign
            .status(&record.state.id, &OperationControl::default())
            .map_err(Into::into),
        "live.channel_project_owner_mismatch",
    );
    assert_eq!(f.inspect().owner.unwrap(), record.metadata.owner);
}

#[test]
fn mailbox_admission_same_process_second_hwnd_is_rejected_after_local_evidence() {
    let f = Fixture::new();
    let mut book = f.book();
    f.create(&mut book);
    let mut other = f.target.clone();
    other.window.handle += 1;
    let err = book
        .create(
            other.clone(),
            identity(),
            &FixedClock,
            &mut Tokens(2),
            &OperationControl::default(),
        )
        .err()
        .unwrap();
    assert!(matches!(
        err,
        AdmissionError::Occupied { foreign: false, .. }
    ));
    let orphan = book
        .status(&format!("CON-{:032x}", 2), &OperationControl::default())
        .unwrap();
    assert_eq!(orphan.metadata.target, other);
    assert!(!marker(&f.scope(), &other.resource()).exists());
    assert!(
        book.acquire(&orphan.state.id, &OperationControl::default())
            .is_err()
    );
    // The same reusable PID with a new creation identity is a distinct process.
    other.window.process_started_at += 1;
    assert!(
        book.create(
            other,
            identity(),
            &FixedClock,
            &mut Tokens(3),
            &OperationControl::default()
        )
        .is_ok()
    );
}

#[test]
fn mailbox_admission_invalid_claim_is_occupied_and_never_overwritten() {
    let f = Fixture::new();
    fs::create_dir(f.scope()).unwrap();
    fs::write(f.marker(), b"{\"schema\":").unwrap();
    let inspection = f.inspect();
    assert!(inspection.occupied);
    assert!(inspection.error.is_some());
    let mut book = f.book();
    assert!(f.create_number(&mut book, 1).is_err());
    assert_eq!(fs::read(f.marker()).unwrap(), b"{\"schema\":");
    assert!(!f.root.join(".lycheedev").exists());
    assert!(
        acquire_installation_maintenance(&f.target.addon_parent(), &OperationControl::default())
            .is_err()
    );
}

#[test]
fn mailbox_admission_owner_parser_preserves_connection_and_upper_hex() {
    let f = Fixture::new();
    fs::create_dir(f.scope()).unwrap();
    for prefix in ["CON-"] {
        let owner = WindowOwner {
            schema: OWNER_SCHEMA.into(),
            workspace_id: "A".repeat(32),
            resource: f.target.resource(),
            operation_id: format!("{prefix}{}", "F".repeat(32)),
            intent_sha256: String::new(),
        };
        fs::write(f.marker(), json(&owner).unwrap()).unwrap();
        assert_eq!(f.inspect().owner, Some(owner));
        assert!(f.create_number(&mut f.book(), 1).err().unwrap().is_busy());
    }
}

#[test]
fn mailbox_admission_claim_requires_canonical_json_but_metadata_does_not() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    let mut owner = json(&record.metadata.owner).unwrap();
    owner.push(b'\n');
    fs::write(f.marker(), &owner).unwrap();
    assert!(f.inspect().occupied);
    assert!(f.inspect().error.is_some());
    let mut metadata = json(&record.metadata).unwrap();
    metadata.push(b'\n');
    fs::write(ConnectionBook::target_path(&record.paths), metadata).unwrap();
    assert_eq!(
        book.status(&record.state.id, &OperationControl::default())
            .unwrap()
            .metadata,
        record.metadata
    );
}

#[test]
fn mailbox_admission_metadata_unknown_duplicate_null_and_nested_merge() {
    let bytes = br#"{"schema":"first","SCHEMA":"lycheedev.channel-target.v1","schema":null,"extra":{"a":1},"target":{"client":{"product":"retail","interface":120100},"window":{"handle":"18446744073709551615","processId":17}},"target":{"client":{"product":null,"fullBuild":"b"},"window":{"handle":null,"processStartedAt":"18"}},"owner":{"operationId":"before"},"OWNER":{"operationId":"after","workspaceId":"w"}}"#;
    let parsed = decode_metadata(bytes).unwrap();
    assert_eq!(parsed.schema, TARGET_SCHEMA);
    assert_eq!(parsed.target.client.product, "retail");
    assert_eq!(parsed.target.client.full_build, "b");
    assert_eq!(parsed.target.client.interface, 120100);
    assert_eq!(parsed.target.window.handle, u64::MAX);
    assert_eq!(parsed.target.window.process_id, 17);
    assert_eq!(parsed.target.window.process_started_at, 18);
    assert_eq!(parsed.owner.operation_id, "after");
    assert!(decode_metadata(br#"{"target":{"window":{"handle":19}}}"#).is_err());
    assert!(decode_metadata(br#"{"target":{"client":{"interface":"120100"}}}"#).is_err());
}

#[test]
fn mailbox_admission_owner_limit_precedes_persistence_and_entry_limit_follows_it() {
    let f = Fixture::new();
    fs::create_dir(f.scope()).unwrap();
    for n in 0..256 {
        fs::write(f.scope().join(format!("fixture-{n}.json")), b"{}").unwrap();
    }
    assert_code(
        f.create_number(&mut f.book(), 1),
        "journal.window_owner_limit",
    );
    assert!(!f.root.join(".lycheedev").exists());
    let g = Fixture::new();
    fs::create_dir(g.scope()).unwrap();
    for n in 0..1024 {
        fs::write(g.scope().join(format!("retained-{n}.part")), b"").unwrap();
    }
    let mut book = g.book();
    assert_code(g.create_number(&mut book, 1), "journal.window_owner_limit");
    assert!(
        book.status(&format!("CON-{:032x}", 1), &OperationControl::default())
            .is_ok()
    );
    assert!(!g.inspect().occupied);
}

#[test]
fn mailbox_admission_cancel_and_expired_deadline_never_create_scope() {
    for expired in [false, true] {
        let f = Fixture::new();
        let mut book = f.book();
        let control = OperationControl::uncancelable(if expired {
            Instant::now() - Duration::from_secs(1)
        } else {
            Instant::now() + Duration::from_secs(5)
        });
        if !expired {
            control.tracked_cancel_handle().unwrap().cancel();
        }
        let err = book
            .create(
                f.target.clone(),
                identity(),
                &FixedClock,
                &mut Tokens(1),
                &control,
            )
            .err()
            .unwrap();
        assert!(matches!(
            err,
            AdmissionError::Channel(ChannelError::Cancelled | ChannelError::DeadlineExceeded)
        ));
        assert!(!f.scope().exists());
        assert!(!f.root.join(".lycheedev").exists());
    }
}

#[test]
fn mailbox_admission_execution_is_try_only_and_claim_outlives_lease() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    let control = OperationControl::default();
    let (_, mut first) = book.acquire(&record.state.id, &control).unwrap();
    assert!(
        book.acquire(&record.state.id, &control)
            .err()
            .unwrap()
            .is_busy()
    );
    assert!(acquire_installation_maintenance(&f.target.addon_parent(), &control).is_err());
    first.close().unwrap();
    assert_eq!(f.inspect().owner, Some(record.metadata.owner));
    let (_, mut second) = book.acquire(&record.state.id, &control).unwrap();
    second.close().unwrap();
}

#[test]
fn mailbox_admission_waiting_short_gate_uses_original_control() {
    let f = Fixture::new();
    let mut gate =
        acquire_installation_maintenance(&f.target.addon_parent(), &OperationControl::default())
            .unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(35));
    let mut book = f.book();
    assert!(matches!(
        book.create(
            f.target.clone(),
            identity(),
            &FixedClock,
            &mut Tokens(1),
            &control
        ),
        Err(AdmissionError::Channel(ChannelError::DeadlineExceeded))
    ));
    assert!(!f.root.join(".lycheedev").exists());
    gate.close().unwrap();
}

#[test]
fn mailbox_admission_retirement_requires_closed_and_preserves_unknown_state() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    assert_code(
        book.retire_closed(&record.state.id, &OperationControl::default()),
        "journal.invalid_transition",
    );
    let expected = close_state(&book, &record.state.id, true);
    let bytes = fs::read(record.paths.log_path()).unwrap();
    let (_, mut held) = book
        .acquire(&record.state.id, &OperationControl::default())
        .unwrap();
    assert!(
        book.retire_closed(&record.state.id, &OperationControl::default())
            .err()
            .unwrap()
            .is_busy()
    );
    held.close().unwrap();
    for _ in 0..2 {
        let retired = book
            .retire_closed(&record.state.id, &OperationControl::default())
            .unwrap();
        assert_eq!(retired.state, expected);
    }
    assert!(!f.inspect().occupied);
    assert_eq!(fs::read(record.paths.log_path()).unwrap(), bytes);
    assert_eq!(
        book.status(&record.state.id, &OperationControl::default())
            .unwrap()
            .state
            .operation
            .unwrap()
            .stage,
        OperationStage::ExecutionUnknown
    );
    let mut maintenance =
        acquire_installation_maintenance(&f.target.addon_parent(), &OperationControl::default())
            .unwrap();
    maintenance.close().unwrap();
}

#[test]
fn mailbox_admission_old_closed_record_never_retires_replacement_owner() {
    let f = Fixture::new();
    let mut book = f.book();
    let old = f.create(&mut book);
    close_state(&book, &old.state.id, false);
    book.retire_closed(&old.state.id, &OperationControl::default())
        .unwrap();
    let replacement = f.create_number(&mut book, 2).unwrap();
    book.retire_closed(&old.state.id, &OperationControl::default())
        .unwrap();
    assert_eq!(f.inspect().owner, Some(replacement.metadata.owner));
}

#[test]
fn mailbox_admission_missing_existing_log_never_reseeds_or_releases_claim() {
    for empty in [false, true] {
        let f = Fixture::new();
        let mut book = f.book();
        let record = f.create(&mut book);
        if empty {
            fs::write(record.paths.log_path(), b"").unwrap();
        } else {
            fs::remove_file(record.paths.log_path()).unwrap();
        }
        assert_code(
            book.acquire(&record.state.id, &OperationControl::default()),
            "live.channel_journal_missing",
        );
        assert!(
            book.find_selected(
                &f.target,
                &TargetFilter::default(),
                &OperationControl::default()
            )
            .is_err()
        );
        assert!(
            book.retire_closed(&record.state.id, &OperationControl::default())
                .is_err()
        );
        assert_eq!(f.inspect().owner, Some(record.metadata.owner));
        assert_eq!(record.paths.log_path().exists(), empty);
    }
}

#[test]
fn mailbox_admission_faults_retain_exact_publication_boundary() {
    for point in [
        FaultPoint::MetadataDurable,
        FaultPoint::Seed(SeedPoint::AppendPartial),
        FaultPoint::Seed(SeedPoint::AppendWritten),
        FaultPoint::Seed(SeedPoint::AppendSynced),
        FaultPoint::Seed(SeedPoint::AppendClosed),
        FaultPoint::LogDurable,
        FaultPoint::ClaimDurable,
    ] {
        for action in [Action::Fail, Action::Cancel] {
            let f = Fixture::new();
            let mut book = f.book();
            inject(&mut book, point, action);
            assert!(f.create_number(&mut book, 1).is_err());
            assert_eq!(f.inspect().occupied, point == FaultPoint::ClaimDurable);
            let id = format!("CON-{:032x}", 1);
            assert!(book.metadata(&id, &OperationControl::default()).is_ok());
            if point == FaultPoint::MetadataDurable {
                assert!(!book.paths(&id).unwrap().log_path().exists());
            } else if point == FaultPoint::Seed(SeedPoint::AppendPartial) {
                assert!(book.status(&id, &OperationControl::default()).is_err());
            } else {
                assert!(book.status(&id, &OperationControl::default()).is_ok());
            }
            if point == FaultPoint::ClaimDurable {
                let (_, mut journal) = book.acquire(&id, &OperationControl::default()).unwrap();
                journal.close().unwrap();
            } else {
                assert!(book.acquire(&id, &OperationControl::default()).is_err());
            }
        }
    }
}

#[test]
fn mailbox_admission_orphan_is_evidence_not_authority_to_republish_or_reseed() {
    let f = Fixture::new();
    let mut book = f.book();
    inject(&mut book, FaultPoint::LogDurable, Action::Fail);
    assert!(f.create_number(&mut book, 1).is_err());
    let id = format!("CON-{:032x}", 1);
    let paths = book.paths(&id).unwrap();
    let bytes = fs::read(paths.log_path()).unwrap();
    assert_code(
        f.create_number(&mut book, 1),
        "live.channel_connection_exists",
    );
    assert_eq!(fs::read(paths.log_path()).unwrap(), bytes);
    assert!(!f.inspect().occupied);
    // Fixed-Go Connect has no marker to reuse and may generate a NEW CON.
    let replacement = f.create_number(&mut book, 2).unwrap();
    assert_eq!(f.inspect().owner, Some(replacement.metadata.owner));
    assert_eq!(fs::read(paths.log_path()).unwrap(), bytes);
}

#[cfg(windows)]
fn directory_junction(target: &Path, link: &Path) {
    use std::os::windows::fs::MetadataExt;

    // Windows junction fixture. Directory redirects do not require the
    // file-symlink privilege below.
    let junction = Command::new("node")
        .args([
            "-e",
            "require('node:fs').symlinkSync(process.argv[1], process.argv[2], 'junction');",
        ])
        .arg(target)
        .arg(link)
        .output()
        .expect("create Windows admission junction fixture with Node");
    assert!(
        junction.status.success(),
        "junction fixture failed: {}",
        String::from_utf8_lossy(&junction.stderr)
    );
    let attributes = fs::symlink_metadata(link).unwrap().file_attributes();
    assert_ne!(attributes & 0x400, 0, "fixture must be a reparse point");
    assert_ne!(attributes & 0x10, 0, "fixture must redirect a directory");
    assert!(fs::metadata(link).unwrap().is_dir());
    assert_eq!(
        fs::canonicalize(link).unwrap(),
        fs::canonicalize(target).unwrap()
    );
}

#[cfg(windows)]
#[test]
fn mailbox_admission_aliases_share_scope_but_redirected_scope_fails() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    let claim_bytes = fs::read(f.marker()).unwrap();
    let alias = f.root.join("addon-alias");
    directory_junction(&f.target.addon_parent(), &alias);
    let inspection =
        inspect_window_owner(&alias, &f.target.resource(), &OperationControl::default());
    assert_eq!(inspection.owner, Some(record.metadata.owner));
    assert!(inspection.occupied);
    assert!(inspection.error.is_none());
    assert_eq!(fs::read(f.marker()).unwrap(), claim_bytes);

    let g = Fixture::new();
    let outside = g.root.join("outside");
    fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("foreign-evidence");
    fs::write(&sentinel, b"preserve foreign evidence").unwrap();
    directory_junction(&outside, &g.scope());
    assert_code(
        g.create_number(&mut g.book(), 1),
        "journal.redirected_window_owner",
    );
    assert_eq!(fs::read(&sentinel).unwrap(), b"preserve foreign evidence");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
#[ignore = "requires Windows file-symlink privilege or Developer Mode; run explicitly with --ignored"]
fn mailbox_admission_redirected_claim_file_fails() {
    use std::os::windows::fs::{MetadataExt, symlink_file};

    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    let claim_bytes = fs::read(f.marker()).unwrap();
    let log_bytes = fs::read(record.paths.log_path()).unwrap();
    let outside = f.root.join("outside");
    fs::create_dir(&outside).unwrap();
    let actual = outside.join("claim-backup.json");
    fs::rename(f.marker(), &actual).unwrap();
    // A junction would change this to a directory-leaf test. Keep a genuine
    // redirect to a valid claim file, with an explicit ignored count by default.
    if let Err(error) = symlink_file(&actual, f.marker()) {
        if error.raw_os_error() == Some(1314) {
            panic!(
                "environment unavailable: ERROR_PRIVILEGE_NOT_HELD (1314); enable Developer Mode \
                 or grant file-symlink privilege; claim redirection assertions were not executed"
            );
        }
        panic!("claim file symlink fixture failed: {error}");
    }
    let metadata = fs::symlink_metadata(f.marker()).unwrap();
    assert!(metadata.file_type().is_symlink());
    assert_ne!(metadata.file_attributes() & 0x400, 0);
    assert!(fs::metadata(f.marker()).unwrap().is_file());
    assert_eq!(
        fs::canonicalize(f.marker()).unwrap(),
        fs::canonicalize(&actual).unwrap()
    );
    assert_eq!(fs::read(f.marker()).unwrap(), claim_bytes);

    let inspection = f.inspect();
    assert!(inspection.occupied);
    assert!(inspection.owner.is_none());
    assert_code::<()>(
        Err(inspection.error.unwrap().into()),
        "journal.redirected_window_owner",
    );
    assert_eq!(fs::read(&actual).unwrap(), claim_bytes);
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    assert_eq!(fs::read(record.paths.log_path()).unwrap(), log_bytes);
}

#[test]
fn mailbox_admission_process_helper() {
    let Some(root) = std::env::var_os("LYCHEEDEV_FORMAL_ADMISSION_ROOT") else {
        return;
    };
    let f = Fixture::at(PathBuf::from(root));
    let mut book = f.book();
    let mode = std::env::var("LYCHEEDEV_FORMAL_ADMISSION_MODE").unwrap();
    if mode == "lease" {
        let (_, _journal) = book
            .acquire(&format!("CON-{:032x}", 1), &OperationControl::default())
            .unwrap();
        fs::write(f.root.join("helper-ready"), b"held").unwrap();
        let mut byte = [0];
        std::io::stdin().read_exact(&mut byte).unwrap();
        std::process::exit(91);
    }
    let point = match mode.as_str() {
        "metadata" => FaultPoint::MetadataDurable,
        "partial" => FaultPoint::Seed(SeedPoint::AppendPartial),
        "log" => FaultPoint::LogDurable,
        "claim" => FaultPoint::ClaimDurable,
        "retired" => FaultPoint::ClaimRetired,
        _ => panic!("unknown helper mode"),
    };
    inject(&mut book, point, Action::Crash);
    if mode == "retired" {
        book.retire_closed(&format!("CON-{:032x}", 1), &OperationControl::default())
            .unwrap();
    } else {
        f.create_number(&mut book, 1).unwrap();
    }
    panic!("expected no-destructor process exit");
}
struct OwnedChild(std::process::Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn spawn(f: &Fixture, mode: &str) -> OwnedChild {
    let output = File::create(f.root.join(format!("child-{mode}.log"))).unwrap();
    OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .arg("mailbox_admission_process_helper")
            .arg("--nocapture")
            .env("LYCHEEDEV_FORMAL_ADMISSION_ROOT", &f.root)
            .env("LYCHEEDEV_FORMAL_ADMISSION_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output))
            .spawn()
            .unwrap(),
    )
}
fn wait(child: &mut OwnedChild) {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(91));
            return;
        }
        assert!(
            Instant::now() < until,
            "child exceeded fixture deadline; evidence retained"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn mailbox_admission_real_process_crash_before_and_after_claim() {
    for mode in ["metadata", "partial", "log", "claim"] {
        let f = Fixture::new();
        let mut child = spawn(&f, mode);
        wait(&mut child);
        let book = f.book();
        let id = format!("CON-{:032x}", 1);
        assert!(book.metadata(&id, &OperationControl::default()).is_ok());
        assert_eq!(f.inspect().occupied, mode == "claim");
        if mode == "claim" {
            let (record, mut journal) = book.acquire(&id, &OperationControl::default()).unwrap();
            assert_eq!(record.state.connect_budget.unwrap().started_at_ms, 1000);
            journal.close().unwrap();
        } else {
            assert!(book.acquire(&id, &OperationControl::default()).is_err());
        }
        if mode == "partial" {
            assert_code(
                book.status(&id, &OperationControl::default())
                    .map_err(Into::into),
                "journal.memory_torn_tail",
            );
        }
        if mode == "log" {
            assert!(book.status(&id, &OperationControl::default()).is_ok());
        }
    }
}

#[test]
fn mailbox_admission_real_process_exit_releases_driver_not_durable_claim() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    let mut child = spawn(&f, "lease");
    let until = Instant::now() + Duration::from_secs(10);
    while !f.root.join("helper-ready").exists() {
        assert!(
            child.0.try_wait().unwrap().is_none() && Instant::now() < until,
            "child failed to acquire lease"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        book.acquire(&record.state.id, &OperationControl::default())
            .err()
            .unwrap()
            .is_busy()
    );
    assert!(f.create_number(&mut book, 2).err().unwrap().is_busy());
    child.0.stdin.take().unwrap().write_all(b"x").unwrap();
    wait(&mut child);
    assert_eq!(f.inspect().owner, Some(record.metadata.owner));
    let (_, mut journal) = book
        .acquire(&record.state.id, &OperationControl::default())
        .unwrap();
    journal.close().unwrap();
}

#[test]
fn mailbox_admission_real_process_crash_after_retirement_keeps_closed_evidence() {
    let f = Fixture::new();
    let mut book = f.book();
    let record = f.create(&mut book);
    let expected = close_state(&book, &record.state.id, true);
    let mut child = spawn(&f, "retired");
    wait(&mut child);
    assert!(!f.inspect().occupied);
    assert_eq!(
        book.retire_closed(&record.state.id, &OperationControl::default())
            .unwrap()
            .state,
        expected
    );
    assert!(record.paths.log_path().is_file());
}

#[test]
fn mailbox_admission_quoted_os_numbers_match_uint_string_lexemes() {
    for field in ["handle", "processStartedAt"] {
        for (literal, want) in [
            ("\"0\"", 0),
            ("\"00019\"", 19),
            ("\"18446744073709551615\"", u64::MAX),
            ("null", 77),
            ("\"null\"", 77),
        ] {
            let input =
                format!(r#"{{"target":{{"window":{{"{field}":"77","{field}":{literal}}}}}}}"#);
            let value = decode_metadata(input.as_bytes()).unwrap().target.window;
            assert_eq!(
                if field == "handle" {
                    value.handle
                } else {
                    value.process_started_at
                },
                want,
                "{input}"
            );
        }
        for literal in [
            "\"+19\"",
            "\"-0\"",
            "\"-19\"",
            "\"\"",
            "\" 19\"",
            "\"19 \"",
            "\"１９\"",
            "\"19.0\"",
            "\"1e1\"",
            "\"18446744073709551616\"",
            "19",
            "19.0",
        ] {
            let input = format!(r#"{{"target":{{"window":{{"{field}":{literal}}}}}}}"#);
            assert!(
                decode_metadata(input.as_bytes()).is_err(),
                "unexpectedly accepted {input}"
            );
        }
    }
    let input = br#"{"target":{"window":{"handle":"0019","HANDLE":null,"handle":"null","processStartedAt":"0018","PROCESSSTARTEDAT":null,"processStartedAt":"null"}}}"#;
    let value = decode_metadata(input).unwrap().target.window;
    assert_eq!((value.handle, value.process_started_at), (19, 18));
}

#[test]
fn mailbox_admission_signed_interface_preserves_negative_zero_without_float_coercion() {
    for (literal, want) in [
        ("-0", 0),
        ("0", 0),
        ("-1", -1),
        ("9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
        ("null", 17),
    ] {
        let input =
            format!(r#"{{"target":{{"client":{{"interface":17,"interface":{literal}}}}}}}"#);
        assert_eq!(
            decode_metadata(input.as_bytes())
                .unwrap()
                .target
                .client
                .interface,
            want,
            "{input}"
        );
    }
    for literal in [
        "0.0",
        "-0.0",
        "1e0",
        "0e0",
        "\"-0\"",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        let input = format!(r#"{{"target":{{"client":{{"interface":{literal}}}}}}}"#);
        assert!(
            decode_metadata(input.as_bytes()).is_err(),
            "unexpectedly accepted {input}"
        );
    }
    assert_eq!(
        decode_metadata(
            br#"{"target":{"client":{"interface":17,"INTERFACE":-0,"interface":null}}}"#
        )
        .unwrap()
        .target
        .client
        .interface,
        0
    );
}

#[test]
fn mailbox_admission_two_installations_same_con_cannot_replace_winning_target() {
    let f = Fixture::new();
    let target_a = f.target.clone();
    let mut target_b = f.target.clone();
    let directory_b = f.root.join("second-client");
    fs::create_dir_all(directory_b.join("Interface/AddOns")).unwrap();
    target_b.client.directory = directory_b.to_str().unwrap().into();
    target_b.window.executable = directory_b.join("Wow.exe").to_str().unwrap().into();
    target_b.window.process_id += 1;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_a_tx, release_a_rx) = std::sync::mpsc::channel();
    let (release_b_tx, release_b_rx) = std::sync::mpsc::channel();
    let root_a = f.root.clone();
    let root_b = f.root.clone();
    let a = target_a.clone();
    let b = target_b.clone();
    let ready_b = ready_tx.clone();
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(move || {
            let mut book = ConnectionBook::open(&root_a).unwrap();
            inject(
                &mut book,
                FaultPoint::TargetStaged,
                Action::Rendezvous {
                    ready: ready_tx,
                    release: release_a_rx,
                },
            );
            book.create(
                a,
                identity(),
                &FixedClock,
                &mut Tokens(1),
                &OperationControl::default(),
            )
        });
        let second = scope.spawn(move || {
            let mut book = ConnectionBook::open(&root_b).unwrap();
            inject(
                &mut book,
                FaultPoint::TargetStaged,
                Action::Rendezvous {
                    ready: ready_b,
                    release: release_b_rx,
                },
            );
            let mut other_identity = identity();
            other_identity.runtime = "b".repeat(32);
            book.create(
                b,
                other_identity,
                &FixedClock,
                &mut Tokens(1),
                &OperationControl::default(),
            )
        });
        // Both have passed the old pre-check and durably staged their own
        // complete target before either can perform the publication move.
        // All rendezvous waits are bounded, including failed fixture setup.
        let first_ready = ready_rx.recv_timeout(Duration::from_secs(10));
        let second_ready = ready_rx.recv_timeout(Duration::from_secs(10));
        let _ = release_a_tx.send(());
        let _ = release_b_tx.send(());
        assert!(
            first_ready.is_ok() && second_ready.is_ok(),
            "both installations must reach target staging"
        );
        (first.join().unwrap(), second.join().unwrap())
    });
    let (winner, loser) = match (first, second) {
        (Ok(winner), Err(loser)) | (Err(loser), Ok(winner)) => (winner, loser),
        _ => panic!("exactly one installation may admit the shared CON"),
    };
    assert_code::<ConnectionRecord>(Err(loser), "live.channel_connection_exists");
    let book = f.book();
    let saved = book
        .status(&winner.state.id, &OperationControl::default())
        .unwrap();
    assert_eq!(saved.metadata, winner.metadata);
    assert_eq!(saved.state, winner.state);
    assert_eq!(
        fs::read(ConnectionBook::target_path(&winner.paths)).unwrap(),
        json(&winner.metadata).unwrap()
    );
    let log = super::super::journal::read_log_bytes(&fs::read(winner.paths.log_path()).unwrap());
    assert!(log.failure.is_none());
    assert_eq!(log.events.len(), 1);
    for target in [target_a, target_b] {
        let inspection = inspect_window_owner(
            &target.addon_parent(),
            &target.resource(),
            &OperationControl::default(),
        );
        assert!(inspection.error.is_none());
        if target == winner.metadata.target {
            assert_eq!(inspection.owner, Some(winner.metadata.owner.clone()));
        } else {
            assert!(!inspection.occupied);
        }
    }
}

#[test]
fn mailbox_admission_staged_target_failure_and_cancel_publish_nothing() {
    for action in [Action::Fail, Action::Cancel] {
        let f = Fixture::new();
        let mut book = f.book();
        inject(&mut book, FaultPoint::TargetStaged, action);
        assert!(f.create_number(&mut book, 1).is_err());
        let paths = book.paths(&format!("CON-{:032x}", 1)).unwrap();
        assert!(!ConnectionBook::target_path(&paths).exists());
        assert!(!paths.log_path().exists());
        assert!(!f.inspect().occupied);
        assert!(
            fs::read_dir(paths.log_path().parent().unwrap())
                .unwrap()
                .next()
                .is_none()
        );
    }
}
