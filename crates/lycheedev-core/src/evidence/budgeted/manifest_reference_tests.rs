use super::*;
struct Meter;
impl CaptureAccounting for Meter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
}
fn root() -> std::path::PathBuf {
    let home = std::env::temp_dir().join(format!(
        "lychee-demux-reference-{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    workspace::initialize(&home).unwrap();
    home
}
fn input<'a>(bytes: &'a [u8]) -> CaptureInput<'a> {
    CaptureInput {
        bytes,
        maximum_bytes: 1 << 20,
        media_type: "application/json",
        provenance: serde_json::json!({"kind":"asset-demux-manifest","locator":"fixture"}),
        complete: true,
        truncated: false,
    }
}
#[test]
fn extra_references_are_authenticated_but_not_serialized_into_identity_dependencies() {
    let home = root();
    let control = OperationControl::default();
    let source = commit_capture_budgeted(&home, input(b"source"), &control, &Meter).unwrap();
    let frame = commit_capture_budgeted(&home, input(b"frame"), &control, &Meter).unwrap();
    let source_id = source["id"].as_str().unwrap();
    let frame_id = frame["id"].as_str().unwrap();
    let written = commit_capture_budgeted_with_references(
        &home,
        input(b"manifest"),
        &[source_id],
        &[frame_id],
        &control,
        &Meter,
    )
    .unwrap();
    assert_eq!(written["dependencies"], serde_json::json!([source_id]));
    assert_eq!(
        super::super::read_capture(&home, written["id"].as_str().unwrap()).unwrap(),
        written
    );
    std::fs::remove_dir_all(home).unwrap();
}
#[test]
fn missing_frame_or_omitted_declared_source_fails_before_manifest_body_publication() {
    let home = root();
    let control = OperationControl::default();
    let missing = format!("CAP-{}", "a".repeat(64));
    for (n, (dependencies, references)) in [
        (vec![], vec![missing.as_str()]),
        (vec![missing.as_str()], vec![]),
    ]
    .into_iter()
    .enumerate()
    {
        let body = format!("never-published-{n}");
        assert!(
            commit_capture_budgeted_with_references(
                &home,
                input(body.as_bytes()),
                &dependencies,
                &references,
                &control,
                &Meter
            )
            .is_err()
        );
        let path = super::super::blob_path(&home, &storage::sha256(body.as_bytes())).unwrap();
        assert!(!path.exists());
    }
    std::fs::remove_dir_all(home).unwrap();
}
