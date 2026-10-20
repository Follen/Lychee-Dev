use super::*;
use crate::{storage, workspace};
use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Point {
    Opened,
    Mutated,
    Committed,
    CommitReturned,
    RolledBack,
    Closed,
}
std::thread_local! {
    static FAULTS: RefCell<Vec<(Point, ToolkitError)>> = const { RefCell::new(Vec::new()) };
    static EVENTS: RefCell<Vec<Point>> = const { RefCell::new(Vec::new()) };
    static PAGE_CAP: Cell<Option<u64>> = const { Cell::new(None) };
}
pub(super) fn observe(point: Point) -> Result<()> {
    EVENTS.with(|events| events.borrow_mut().push(point));
    FAULTS.with(|faults| {
        let mut faults = faults.borrow_mut();
        match faults.iter().position(|(p, _)| *p == point) {
            Some(i) => Err(faults.remove(i).1),
            None => Ok(()),
        }
    })
}
pub(super) fn page_cap(normal: u64) -> u64 {
    PAGE_CAP.with(|cap| cap.get().unwrap_or(normal).min(normal))
}
fn fault(point: Point, code: &str) {
    FAULTS.with(|faults| {
        faults
            .borrow_mut()
            .push((point, ToolkitError::new(code, code, 5)))
    });
}
fn seen(point: Point) -> bool {
    EVENTS.with(|events| events.borrow().contains(&point))
}
fn reset() {
    FAULTS.with(|faults| faults.borrow_mut().clear());
    EVENTS.with(|events| events.borrow_mut().clear());
    PAGE_CAP.with(|cap| cap.set(None));
}
struct Fixture {
    home: PathBuf,
    control: OperationControl,
}
impl Fixture {
    fn new() -> Self {
        reset();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-private-metadata-{}-{}-{}",
            std::process::id(),
            storage::sha256(storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self {
            home,
            control: OperationControl::default(),
        }
    }
    fn limit(&self, value: u64) {
        let mut config = workspace::default_config();
        config["dataCache"]["maxBytes"] = value.into();
        storage::atomic_json(&self.home.join("config.json"), &config).unwrap();
    }
    fn path(&self) -> PathBuf {
        self.home.join("state/data/cache-v1.sqlite")
    }
    fn put<T: Serialize>(&self, value: &T, generation: Option<i64>) -> Result<()> {
        DataCacheMetadata::put_controlled_with_check(
            &self.home,
            "remote-config/fixture",
            value,
            generation,
            &self.control,
            || Ok(()),
        )
    }
    fn read(&self) -> Result<RawDocument> {
        DataCacheMetadata::read_document_raw_controlled_with_check(
            &self.home,
            "remote-config/fixture",
            &self.control,
            || Ok(()),
        )
    }
    fn inventory(&self) -> crate::workspace::data_disk::DerivedUsage {
        let resources = workspace::data_resources::acquire(
            &self.home,
            workspace::data_resources::Policy::Ordinary,
            &self.control,
        )
        .unwrap();
        // This helper is used only while the fixture remains below its limit.
        let owner = resources.reserve_derived_cache(Default::default()).unwrap();
        let usage = owner.report().existing;
        owner.finish(Ok(())).unwrap();
        resources.close().unwrap();
        usage
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        reset();
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.home).unwrap();
        }
    }
}

#[test]
fn private_roundtrip_strict_cas_and_purpose_binding_leave_core_authority_unchanged() {
    let f = Fixture::new();
    let mut core = Metadata::open(&f.home, true).unwrap();
    core.create("remote-config/fixture", &"legacy").unwrap();
    core.create("capture/fixture", &"protected").unwrap();
    assert_eq!(f.read().unwrap_err().code, "vault.record_missing");
    f.put(&"first", None).unwrap();
    assert_eq!(f.read().unwrap().generation, 1);
    assert_eq!(
        f.put(&"first", None).unwrap_err().code,
        "vault.generation_conflict"
    );
    assert_eq!(
        f.put(&"wrong", Some(2)).unwrap_err().code,
        "vault.generation_conflict"
    );
    f.put(&"second", Some(1)).unwrap();
    assert_eq!(f.read().unwrap().bytes, br#""second""#);
    assert_eq!(f.read().unwrap().generation, 2);
    assert_eq!(
        core.read::<String>("remote-config/fixture").unwrap(),
        "legacy"
    );
    assert_eq!(core.read::<String>("capture/fixture").unwrap(), "protected");
    assert_eq!(
        DataCacheMetadata::put_controlled_with_check(
            &f.home,
            "capture/forbidden",
            &true,
            None,
            &f.control,
            || Ok(())
        )
        .unwrap_err()
        .code,
        "vault.data_metadata_integrity"
    );
    core.close().unwrap();
    let db = Connection::open(f.path()).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "page_size", |r| r.get::<_, u64>(0))
            .unwrap(),
        PAGE_BYTES
    );
    assert_eq!(
        db.pragma_query_value(None, "auto_vacuum", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    db.close().unwrap();
}

#[test]
fn bootstrap_peak_accepts_exact_boundary_rejects_one_byte_less_and_checks_overflow() {
    let minimum = journal_bound(0).unwrap() + 2 * PAGE_BYTES;
    let f = Fixture::new();
    f.limit(minimum - 1);
    assert_eq!(
        f.put(&true, None).unwrap_err().code,
        "vault.data_cache_budget_exceeded"
    );
    assert!(!f.path().exists());
    f.limit(minimum);
    f.put(&true, None).unwrap();
    assert_eq!(fs::metadata(f.path()).unwrap().len(), 2 * PAGE_BYTES);
    assert!(!f.path().with_file_name("cache-v1.sqlite-journal").exists());
    assert_eq!(
        f.inventory().private_metadata_namespace_bytes,
        2 * PAGE_BYTES
    );
    assert_eq!(
        journal_bound(u64::MAX).unwrap_err().code,
        "vault.data_cache_size_overflow"
    );
}

#[test]
fn actual_namespace_residuals_charge_once_and_never_adopt_core_or_evidence() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    let root = f.path().parent().unwrap().to_owned();
    fs::write(root.join("unknown-retained-stage"), b"retain seven").unwrap();
    fs::write(f.home.join("blobs/protected-fixture"), b"evidence").unwrap();
    let mut core = Metadata::open(&f.home, true).unwrap();
    core.create("capture/fixture", &true).unwrap();
    core.close().unwrap();
    let core_before = fs::read(f.home.join("state/toolkit.sqlite")).unwrap();
    let usage = f.inventory();
    assert_eq!(
        usage.private_metadata_namespace_bytes,
        fs::metadata(f.path()).unwrap().len() + 12
    );
    assert_eq!(
        usage.total().unwrap(),
        usage.private_metadata_namespace_bytes
    );
    assert_eq!(
        fs::read(root.join("unknown-retained-stage")).unwrap(),
        b"retain seven"
    );
    assert_eq!(
        fs::read(f.home.join("blobs/protected-fixture")).unwrap(),
        b"evidence"
    );
    assert_eq!(
        fs::read(f.home.join("state/toolkit.sqlite")).unwrap(),
        core_before
    );
}

#[test]
fn reduced_limit_allows_zero_growth_read_but_refuses_new_transaction_peak() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    let before = fs::read(f.path()).unwrap();
    f.limit(1);
    assert_eq!(f.read().unwrap().bytes, b"true");
    assert_eq!(
        f.put(&false, Some(1)).unwrap_err().code,
        "vault.data_cache_budget_exceeded"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert_eq!(f.read().unwrap().generation, 1);
}

const CRASH_ENV: &str = "LYCHEE_PRIVATE_METADATA_CRASH_HOME";
fn crash_child(mode: &str) -> Option<PathBuf> {
    if std::env::var("LYCHEE_PRIVATE_METADATA_CRASH_MODE")
        .ok()
        .as_deref()
        == Some(mode)
    {
        return std::env::var_os(CRASH_ENV).map(PathBuf::from);
    }
    None
}
fn spawn_crash(f: &Fixture, test: &str, mode: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env(CRASH_ENV, &f.home)
        .env("LYCHEE_PRIVATE_METADATA_CRASH_MODE", mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(
                status.code(),
                Some(87),
                "the exact child test must execute its deliberate crash"
            );
            break;
        }
        if Instant::now() >= deadline {
            let killed = child.kill();
            let waited = child.wait();
            panic!("crash fixture timed out; kill={killed:?}, wait={waited:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn genuine_hot_journal_recovers_old_value_and_generation_even_after_limit_shrink() {
    if let Some(home) = crash_child("existing") {
        let db = Connection::open(home.join("state/data/cache-v1.sqlite")).unwrap();
        db.execute_batch("PRAGMA synchronous=FULL; PRAGMA cache_size=1; BEGIN IMMEDIATE;")
            .unwrap();
        let changed = serde_json::to_vec(&"changed".repeat(90_000)).unwrap();
        db.execute("UPDATE OR ROLLBACK documents SET generation=2,value=?1 WHERE key='remote-config/fixture'", [&changed]).unwrap();
        db.cache_flush().unwrap();
        // No Drop/ROLLBACK: a real process exit leaves SQLite's own hot journal.
        std::process::exit(87);
    }
    let f = Fixture::new();
    let original = "original".repeat(80_000);
    f.put(&original, None).unwrap();
    let before = fs::read(f.path()).unwrap();
    spawn_crash(
        &f,
        "metadata::data_cache::tests::genuine_hot_journal_recovers_old_value_and_generation_even_after_limit_shrink",
        "existing",
    );
    let journal = f.path().with_file_name("cache-v1.sqlite-journal");
    assert!(fs::metadata(&journal).unwrap().len() > PAGE_BYTES);
    assert_ne!(
        fs::read(f.path()).unwrap(),
        before,
        "child must actually flush uncommitted database pages"
    );
    let readonly = Connection::open_with_flags(f.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let failure = readonly
        .query_row("SELECT generation FROM documents", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_err();
    assert!(
        matches!(failure, rusqlite::Error::SqliteFailure(e, _) if e.extended_code == rusqlite::ffi::SQLITE_READONLY_ROLLBACK)
    );
    readonly.close().unwrap();
    f.limit(1);
    let recovered = f.read().unwrap();
    assert_eq!(recovered.generation, 1);
    assert_eq!(
        serde_json::from_slice::<String>(&recovered.bytes).unwrap(),
        original
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert!(!journal.exists());
    assert_eq!(
        f.put(&false, Some(1)).unwrap_err().code,
        "vault.data_cache_budget_exceeded"
    );
}

#[test]
fn interrupted_bootstrap_is_normally_recovered_then_recreated_under_admission() {
    if let Some(home) = crash_child("bootstrap") {
        let path = home.join("state/data/cache-v1.sqlite");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db = Connection::open(path).unwrap();
        db.execute_batch("PRAGMA page_size=65536; PRAGMA auto_vacuum=NONE; PRAGMA synchronous=FULL; PRAGMA cache_size=1; BEGIN IMMEDIATE;").unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db.pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        db.pragma_update(None, "user_version", 1).unwrap();
        db.execute(
            "INSERT OR ROLLBACK INTO documents VALUES('remote-config/fixture',1,?1)",
            [vec![b'x'; 700_000]],
        )
        .unwrap();
        db.cache_flush().unwrap();
        std::process::exit(87);
    }
    let f = Fixture::new();
    spawn_crash(
        &f,
        "metadata::data_cache::tests::interrupted_bootstrap_is_normally_recovered_then_recreated_under_admission",
        "bootstrap",
    );
    assert!(f.path().with_file_name("cache-v1.sqlite-journal").exists());
    assert_eq!(f.read().unwrap_err().code, "vault.record_missing");
    assert_eq!(fs::metadata(f.path()).unwrap().len(), 0);
    assert!(!f.path().with_file_name("cache-v1.sqlite-journal").exists());
    f.put(&true, None).unwrap();
    assert_eq!(f.read().unwrap().generation, 1);
}

#[test]
fn sqlite_full_rolls_back_without_raising_page_limit_or_replaying() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    let before = fs::read(f.path()).unwrap();
    reset();
    PAGE_CAP.with(|cap| cap.set(Some(2)));
    let error = f.put(&"large".repeat(120_000), Some(1)).unwrap_err();
    assert_eq!(error.code, "vault.metadata");
    assert!(error.message.to_ascii_lowercase().contains("full"));
    assert!(!seen(Point::Committed));
    assert!(seen(Point::Closed));
    PAGE_CAP.with(|cap| cap.set(None));
    assert_eq!(f.read().unwrap().bytes, b"true");
    assert_eq!(f.read().unwrap().generation, 1);
    assert_eq!(fs::read(f.path()).unwrap(), before);
}

#[test]
fn mutation_primary_rollback_and_checked_close_errors_are_all_retained() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    reset();
    fault(Point::Mutated, "fixture.primary");
    fault(Point::RolledBack, "fixture.rollback");
    fault(Point::Closed, "fixture.close");
    let error = f.put(&false, Some(1)).unwrap_err();
    assert_eq!(error.code, "fixture.primary");
    let details = error.details.to_string();
    assert!(details.contains("fixture.rollback"));
    assert!(details.contains("fixture.close"));
    assert!(seen(Point::RolledBack));
    assert!(seen(Point::Closed));
    assert!(!seen(Point::Committed));
    assert_eq!(f.read().unwrap().bytes, b"true");
    f.put(&false, Some(1)).unwrap(); // real completed rollback is reusable
}

#[test]
fn post_commit_error_preserves_exact_committed_fact_and_does_not_replay() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    reset();
    fault(Point::Committed, "fixture.late-stop");
    fault(Point::Closed, "fixture.close");
    let error = f.put(&false, Some(1)).unwrap_err();
    assert_eq!(error.code, "fixture.late-stop");
    assert!(error.details.to_string().contains("fixture.close"));
    let receipt = &error.details["publicationReceipts"][0];
    assert_eq!(receipt["state"], "committed");
    assert_eq!(receipt["key"], "remote-config/fixture");
    assert_eq!(receipt["generation"], 2);
    let path: PathBuf = serde_json::from_value(receipt["path"].clone()).unwrap();
    assert_eq!(path, f.path());
    assert_eq!(f.read().unwrap().generation, 2);
    assert_eq!(f.read().unwrap().bytes, b"false");
    assert_eq!(
        f.put(&false, Some(1)).unwrap_err().code,
        "vault.generation_conflict"
    );
    assert_eq!(f.read().unwrap().generation, 2);
}

#[test]
fn independent_stop_after_actual_open_checks_close_and_original_control_remains_live() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    reset();
    fault(Point::Closed, "fixture.close");
    let error = DataCacheMetadata::read_document_raw_controlled_with_check(
        &f.home,
        "remote-config/fixture",
        &f.control,
        || {
            if seen(Point::Opened) {
                Err(ToolkitError::new(
                    "fixture.independent",
                    "fixture.independent",
                    3,
                ))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.independent");
    assert!(seen(Point::Closed));
    assert!(error.details.to_string().contains("fixture.close"));
    assert!(f.control.check().is_ok());
    reset();
    assert_eq!(f.read().unwrap().bytes, b"true");
}

#[test]
fn sqlite_busy_uses_original_deadline_then_closes_and_preserves_valid_data() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    let holder = Connection::open(f.path()).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(150));
    let before = super::super::sql_busy_observations();
    let error = DataCacheMetadata::read_document_raw_controlled_with_check(
        &f.home,
        "remote-config/fixture",
        &control,
        || Ok(()),
    )
    .unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert!(super::super::sql_busy_observations() > before);
    assert!(seen(Point::Closed));
    holder.execute_batch("ROLLBACK").unwrap();
    holder.close().unwrap();
    assert_eq!(f.read().unwrap().bytes, b"true");
}

#[test]
fn corrupt_or_foreign_metadata_is_explicit_and_retains_remaining_files() {
    for case in ["body", "purpose", "trigger", "wal", "shm", "orphan-journal"] {
        let f = Fixture::new();
        f.put(&true, None).unwrap();
        match case {
            "body" => fs::write(f.path(), b"not a database").unwrap(),
            "purpose" => {
                let db = Connection::open(f.path()).unwrap();
                db.pragma_update(None, "application_id", 0).unwrap();
                db.close().unwrap();
            }
            "trigger" => {
                let db = Connection::open(f.path()).unwrap();
                db.execute_batch(
                    "CREATE TRIGGER forbidden AFTER UPDATE ON documents BEGIN SELECT 1; END;",
                )
                .unwrap();
                db.close().unwrap();
            }
            "wal" => fs::write(
                f.path().with_file_name("cache-v1.sqlite-wal"),
                b"unknown bytes",
            )
            .unwrap(),
            "shm" => fs::write(
                f.path().with_file_name("cache-v1.sqlite-shm"),
                b"unknown bytes",
            )
            .unwrap(),
            "orphan-journal" => {
                fs::remove_file(f.path()).unwrap();
                fs::write(
                    f.path().with_file_name("cache-v1.sqlite-journal"),
                    b"unknown bytes",
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let paths: Vec<_> = fs::read_dir(f.path().parent().unwrap())
            .unwrap()
            .map(|p| p.unwrap().path())
            .collect();
        let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
        let error = f.read().unwrap_err();
        assert_ne!(error.code, "vault.record_missing", "{case}");
        if case == "orphan-journal" {
            assert_eq!(
                f.put(&false, None).unwrap_err().code,
                "vault.data_metadata_integrity"
            );
            assert!(!f.path().exists());
        }
        for (path, bytes) in paths.iter().zip(before) {
            assert_eq!(fs::read(path).unwrap(), bytes, "{case}");
        }
    }
}

#[test]
fn ambiguous_commit_error_retains_unknown_receipt_even_if_connection_is_autocommit() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    reset();
    fault(Point::CommitReturned, "fixture.commit-unknown");
    let error = f.put(&false, Some(1)).unwrap_err();
    assert_eq!(error.code, "fixture.commit-unknown");
    assert_eq!(error.details["publicationReceipts"][0]["state"], "unknown");
    assert_eq!(error.details["publicationReceipts"][0]["generation"], 2);
    assert!(seen(Point::Closed));
    assert_eq!(f.read().unwrap().generation, 2);
    assert_eq!(f.read().unwrap().bytes, b"false");
    assert_eq!(
        f.put(&false, Some(1)).unwrap_err().code,
        "vault.generation_conflict"
    );
}

#[test]
fn original_cancellation_after_mutation_still_rolls_back_and_checked_closes() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    reset();
    let token = crate::control::TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(10), token.clone());
    fault(Point::Closed, "fixture.close");
    let error = DataCacheMetadata::put_controlled_with_check(
        &f.home,
        "remote-config/fixture",
        &false,
        Some(1),
        &control,
        || {
            if seen(Point::Mutated) {
                token.cancel();
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.exit_code, 7);
    assert!(error.details.to_string().contains("fixture.close"));
    assert!(seen(Point::RolledBack));
    assert!(seen(Point::Closed));
    assert!(!seen(Point::Committed));
    assert_eq!(f.read().unwrap().bytes, b"true");
    assert_eq!(f.read().unwrap().generation, 1);
}

#[test]
fn read_capability_never_creates_main_and_ignores_unrelated_registered_body_loss() {
    let f = Fixture::new();
    assert_eq!(f.read().unwrap_err().code, "vault.record_missing");
    assert!(!f.path().exists());
    f.put(&true, None).unwrap();
    let reference = crate::workspace::data_disk::publish_shared_blob_with_check(
        &f.home,
        b"body",
        4,
        &f.control,
        &|| Ok(()),
    )
    .unwrap();
    fs::remove_file(crate::evidence::blob_path(&f.home, &reference.sha256).unwrap()).unwrap();
    let before = fs::read(f.path()).unwrap();
    f.limit(1);
    assert_eq!(f.read().unwrap().bytes, b"true");
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert_eq!(
        f.put(&false, Some(1)).unwrap_err().code,
        "vault.data_shared_blob_integrity"
    );
}

#[test]
fn torn_journal_follows_sqlite_normal_handling_and_keeps_original_row() {
    let f = Fixture::new();
    f.put(&true, None).unwrap();
    let journal = f.path().with_file_name("cache-v1.sqlite-journal");
    fs::write(&journal, b"torn").unwrap();
    let row = f.read().unwrap();
    assert_eq!(row.generation, 1);
    assert_eq!(row.bytes, b"true");
    // SQLite may ignore or clean an incomplete header; the application adds
    // no journal parser/unlink and accounts whatever the engine leaves behind.
    let remaining = fs::metadata(&journal).map(|m| m.len()).unwrap_or(0);
    assert_eq!(
        f.inventory().private_metadata_namespace_bytes,
        fs::metadata(f.path()).unwrap().len() + remaining
    );
}

#[test]
fn migrated_purposes_are_private_and_never_import_or_remove_shared_evidence() {
    let f = Fixture::new();
    let mut shared = Metadata::open(&f.home, true).unwrap();
    shared.put("capture/retained", &"evidence", None).unwrap();
    shared.put("pin/retained", &"selection", None).unwrap();
    let keys = [
        "definition/object",
        "definition-ref/revision",
        "decoded-v1/object",
        "hotfix-wago-page/page",
        "listfile/community-csv",
    ];
    for key in keys {
        shared.put(key, &"historical", None).unwrap();
        assert_eq!(
            DataCacheMetadata::read_document_raw_controlled_with_check(
                &f.home,
                key,
                &f.control,
                || Ok(())
            )
            .unwrap_err()
            .code,
            "vault.record_missing"
        );
        DataCacheMetadata::put_controlled_with_check(
            &f.home,
            key,
            &"private",
            None,
            &f.control,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(
            DataCacheMetadata::read_controlled_with_check::<String>(
                &f.home,
                key,
                &f.control,
                || Ok(())
            )
            .unwrap(),
            "private"
        );
        assert_eq!(shared.read::<String>(key).unwrap(), "historical");
    }
    assert_eq!(
        shared.read::<String>("capture/retained").unwrap(),
        "evidence"
    );
    assert_eq!(shared.read::<String>("pin/retained").unwrap(), "selection");
    shared.close().unwrap();
    assert!(f.inventory().private_metadata_namespace_bytes > 0);
}

#[test]
fn migrated_cache_quota_rejection_never_falls_back_to_shared_database() {
    let f = Fixture::new();
    let mut shared = Metadata::open(&f.home, true).unwrap();
    shared
        .put("definition/fixture", &"historical", None)
        .unwrap();
    f.limit(1);
    assert!(
        DataCacheMetadata::put_controlled_with_check(
            &f.home,
            "definition/fixture",
            &"new",
            None,
            &f.control,
            || Ok(())
        )
        .is_err()
    );
    assert_eq!(
        shared.read::<String>("definition/fixture").unwrap(),
        "historical"
    );
    assert_eq!(
        DataCacheMetadata::read_document_raw_controlled_with_check(
            &f.home,
            "definition/fixture",
            &f.control,
            || Ok(())
        )
        .unwrap_err()
        .code,
        "vault.record_missing"
    );
    shared.close().unwrap();
}

#[test]
fn migrated_cache_supplemental_stop_checks_close_and_cannot_become_absence() {
    let f = Fixture::new();
    DataCacheMetadata::put_controlled_with_check(
        &f.home,
        "listfile/fixture",
        &true,
        None,
        &f.control,
        || Ok(()),
    )
    .unwrap();
    reset();
    let error = DataCacheMetadata::read_controlled_with_check::<bool>(
        &f.home,
        "listfile/fixture",
        &f.control,
        || {
            if seen(Point::Opened) {
                Err(ToolkitError::new(
                    "records.query_resource_budget",
                    "fixture budget",
                    3,
                ))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert!(seen(Point::Closed));
    f.control.check().unwrap();
    reset();
    assert!(
        DataCacheMetadata::read_controlled_with_check::<bool>(
            &f.home,
            "listfile/fixture",
            &f.control,
            || Ok(())
        )
        .unwrap()
    );
}
