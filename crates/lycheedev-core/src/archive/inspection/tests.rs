use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
fn fixture() -> (PathBuf, PathBuf) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = std::env::temp_dir().join(format!(
        "lychee-archive-inspection-{}-{}-{}",
        std::process::id(),
        storage::utc_now().replace([':', '.'], "_"),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&parent).unwrap();
    let root = parent.join("home");
    (parent, root)
}
fn plan(root: &Path) -> Value {
    json!({"schema":"lycheedev.archive.v1","root":root,"journalPath":journal_path(root).unwrap(),"state":"moved","createdAt":"2026-10-15T00:00:00Z","steps":[],"archivePath":"display-only-original-archive"})
}
#[test]
fn doctor_reads_original_journal_without_preparing_or_constraining_archive_destination() {
    let (parent, root) = fixture();
    let control = OperationControl::default();
    assert!(journal_controlled(&root, &control).unwrap().is_none());
    assert!(!root.exists());
    let path = journal_path(&root).unwrap();
    let mut raw = serde_json::to_vec(&plan(&root)).unwrap();
    raw.extend_from_slice(b" trailing second data is not a journal");
    fs::write(&path, &raw).unwrap();
    assert_eq!(
        journal_controlled(&root, &control).unwrap_err().code,
        "vault.archive_state"
    );
    let raw = serde_json::to_vec(&plan(&root)).unwrap();
    fs::write(&path, &raw).unwrap();
    let value = journal_controlled(&root, &control).unwrap().unwrap();
    assert_eq!(value.state, "moved");
    assert_eq!(value.archive_path, "display-only-original-archive");
    assert_eq!(fs::read(path).unwrap(), raw);
    assert!(!root.exists());
    fs::remove_dir_all(parent).unwrap();
}
#[test]
fn journal_rejects_unknown_fields_invalid_time_and_actual_oversize() {
    let (parent, root) = fixture();
    let control = OperationControl::default();
    let path = journal_path(&root).unwrap();
    let mut value = plan(&root);
    value["unknown"] = json!(1);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        journal_controlled(&root, &control).unwrap_err().code,
        "vault.archive_state"
    );
    value.as_object_mut().unwrap().remove("unknown");
    // chrono's RFC3339 reader accepts a space date/time separator, so use a
    // genuinely out-of-range timestamp to exercise the typed time rejection.
    value["createdAt"] = json!("2026-10-15T25:00:00Z");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        journal_controlled(&root, &control).unwrap_err().code,
        "vault.archive_state"
    );
    let mut raw = serde_json::to_vec(&plan(&root)).unwrap();
    raw.resize(65537, b' ');
    fs::write(&path, raw).unwrap();
    assert_eq!(
        journal_controlled(&root, &control).unwrap_err().code,
        "vault.archive_state"
    );
    fs::remove_dir_all(parent).unwrap();
}
#[test]
fn pre_cancelled_journal_does_not_turn_missing_into_success() {
    let (parent, root) = fixture();
    let stop = crate::control::TrackedCancellation::new();
    let control = OperationControl::tracked(
        std::time::Instant::now() + std::time::Duration::from_secs(10),
        stop.clone(),
    );
    stop.cancel();
    assert_eq!(
        journal_controlled(&root, &control).unwrap_err().code,
        "command.cancelled"
    );
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn journal_identity_compares_exact_saved_strings_not_path_equivalence() {
    let (parent, root) = fixture();
    let control = OperationControl::default();
    let path = journal_path(&root).unwrap();
    let mut value = plan(&root);
    value["root"] = json!(format!("{}/", root.display()));
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        journal_controlled(&root, &control).unwrap_err().message,
        "vault.archive_state: archive journal identity"
    );
    fs::remove_dir_all(parent).unwrap();
}
