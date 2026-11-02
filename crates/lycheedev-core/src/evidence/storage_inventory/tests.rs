use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-protection-inventory-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], ""),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn capture(&self, body: &[u8], kind: &str, locator: &str) -> Value {
        commit_capture_bounded(
            &self.0,
            CaptureInput {
                bytes: body,
                maximum_bytes: body.len(),
                media_type: "text/plain",
                provenance: json!({"kind":kind,"locator":locator}),
                complete: true,
                truncated: false,
            },
            &OperationControl::default(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn historical_candidates_are_authenticated_deduplicated_and_never_adopted() {
    let f = Fixture::new();
    let one = f.capture(b"same body", "data-query", "one");
    let two = f.capture(b"same body", "data-query-csv", "two");
    f.capture(b"ordinary source", "source-fixture", "other");
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    metadata
        .put("definition/old", &json!({"historical":true}), None)
        .unwrap();
    metadata.close().unwrap();
    fs::write(f.0.join("tmp/unknown-old-stage"), b"retained").unwrap();
    let result = report(&f.0, &OperationControl::default()).unwrap();
    assert_eq!(result["totalDataCoverageComplete"], false);
    let old = &result["historicalProtected"];
    assert_eq!(old["historicalCaptureCount"], 3);
    assert_eq!(old["dataProducerCandidates"]["captureCount"], 2);
    assert_eq!(
        old["dataProducerCandidates"]["verifiedDistinctBodies"]["files"],
        1
    );
    assert_eq!(
        old["dataProducerCandidates"]["verifiedDistinctBodies"]["bytes"],
        9
    );
    assert_eq!(old["knownDataNamespaceRecordCounts"]["definition/"], 1);
    assert_eq!(old["legacyTmpOutsideManagedNamespace"]["bytes"], 8);
    assert!(!f.0.join("state/data/evidence-v2.sqlite").exists());
    assert!(
        fs::read_dir(f.0.join("indexes/data-derived-v1/shared-blobs-v1"))
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(
        read_capture(&f.0, one["id"].as_str().unwrap()).unwrap(),
        one
    );
    assert_eq!(
        read_capture(&f.0, two["id"].as_str().unwrap()).unwrap(),
        two
    );
    assert_eq!(
        fs::read(f.0.join("tmp/unknown-old-stage")).unwrap(),
        b"retained"
    );
}

#[test]
fn missing_candidate_body_is_explicit_not_counted_as_verified_or_zero_unknown() {
    let f = Fixture::new();
    let capture = f.capture(b"missing", "asset", "missing");
    let path = blob_path(&f.0, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
    fs::remove_file(path).unwrap();
    let result = report(&f.0, &OperationControl::default()).unwrap();
    let candidates = &result["historicalProtected"]["dataProducerCandidates"];
    assert_eq!(candidates["missingDistinctBodies"], 1);
    assert_eq!(candidates["verifiedDistinctBodies"]["files"], 0);
    assert_eq!(
        read_capture(&f.0, capture["id"].as_str().unwrap()).unwrap(),
        capture
    );
}

#[test]
fn corrupt_historical_capture_stops_inventory_without_repair_or_deletion() {
    let f = Fixture::new();
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    metadata
        .put("capture/untrusted", &json!({"id":"untrusted"}), None)
        .unwrap();
    metadata.close().unwrap();
    assert!(report(&f.0, &OperationControl::default()).is_err());
    let metadata = Metadata::open(&f.0, false).unwrap();
    assert_eq!(
        metadata.read::<Value>("capture/untrusted").unwrap(),
        json!({"id":"untrusted"})
    );
    metadata.close().unwrap();
}

#[test]
fn cancelled_inventory_returns_original_control_without_touching_payload() {
    use crate::control::TrackedCancellation;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let capture = f.capture(b"payload", "data-query", "cancel");
    let stop = TrackedCancellation::new();
    stop.cancel();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop);
    assert_eq!(
        report(&f.0, &control).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(
        read_capture(&f.0, capture["id"].as_str().unwrap()).unwrap(),
        capture
    );
}

#[test]
fn relative_home_inventory_keeps_managed_tmp_out_of_legacy_totals() {
    let name = format!(
        "lychee-relative-inventory-{}-{}",
        std::process::id(),
        storage::utc_now().replace([':', '.'], "")
    );
    let f = Fixture(std::env::current_dir().unwrap().join(&name));
    workspace::initialize(&f.0).unwrap();
    f.capture(b"relative", "data-query", "relative");
    fs::write(f.0.join("tmp/legacy"), b"old").unwrap();
    let result = report(Path::new(&name), &OperationControl::default()).unwrap();
    assert_eq!(
        result["historicalProtected"]["legacyTmpOutsideManagedNamespace"]["bytes"],
        3
    );
}

#[test]
fn authenticated_legacy_body_forms_are_protected_not_rejected_or_verified() {
    let f = Fixture::new();
    let base = f.capture(b"legacy", "data-query", "historical-forms");
    let mut capture: CaptureRef = serde_json::from_value(base.clone()).unwrap();
    capture.id.clear();
    capture.blob.bytes = 0;
    capture.blob.sha256 = capture.blob.sha256.to_ascii_uppercase();
    let unsigned = String::from_utf8(storage::identity_json(&capture).unwrap()).unwrap();
    let id = format!("CAP-{}", storage::sha256(unsigned.as_bytes()));
    let raw = unsigned.replace("\"id\":\"\"", &format!("\"id\":\"{id}\""));
    let value: Value = serde_json::from_str(&raw).unwrap();
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    metadata
        .put(&format!("capture/{id}"), &value, None)
        .unwrap();
    metadata.close().unwrap();
    let result = report(&f.0, &OperationControl::default()).unwrap();
    let candidates = &result["historicalProtected"]["dataProducerCandidates"];
    assert_eq!(candidates["captureCount"], 2);
    assert_eq!(
        candidates["unsupportedAuthenticatedCaptureCounts"]["noncanonicalDigest"],
        1
    );
    assert_eq!(candidates["verifiedDistinctBodies"]["files"], 1);
    assert_eq!(candidates["missingDistinctBodies"], 0);
}
