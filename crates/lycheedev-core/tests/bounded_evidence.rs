use lycheedev_core::{
    control::{OperationControl, TrackedCancellation},
    evidence::{self, CaptureInput},
    metadata::Metadata,
    request::RequestContext,
    storage, workspace,
};
use serde_json::json;
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-bounded-evidence-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self(path)
    }
    fn initialized() -> Self {
        let temp = Self::new();
        workspace::initialize(&temp.0).unwrap();
        temp
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn draft(bytes: &[u8], maximum: usize) -> CaptureInput<'_> {
    CaptureInput {
        bytes,
        maximum_bytes: maximum,
        media_type: "application/octet-stream",
        provenance: json!({"kind":"fixture","locator":"bounded-body"}),
        complete: true,
        truncated: false,
    }
}
#[test]
fn actual_129mib_capture_requires_explicit_admission_and_verifies_publicly() {
    let temp = Temp::initialized();
    let bytes = vec![0x51; 129 << 20];
    let error = evidence::commit_capture(
        &temp.0,
        &bytes,
        "application/octet-stream",
        json!({"kind":"fixture","locator":"bounded-body"}),
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, "vault.blob_limit");
    assert_eq!(error.exit_code, 4);
    let error = evidence::commit_capture_bounded(
        &temp.0,
        draft(&bytes, bytes.len() - 1),
        &OperationControl::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, "vault.blob_limit");
    assert!(fs::read_dir(temp.0.join("blobs")).unwrap().next().is_none());
    let capture = evidence::commit_capture_bounded(
        &temp.0,
        draft(&bytes, bytes.len()),
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(capture["blob"]["bytes"], bytes.len());
    let path = evidence::blob_path(&temp.0, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), bytes.len() as u64);
    let context = RequestContext {
        home: temp.0.clone(),
        cwd: temp.0.clone(),
        control: OperationControl::default(),
    };
    let outcome = evidence::dispatch(
        &context,
        evidence::EvidenceRequest::Verify { id: capture["id"].as_str().unwrap().into() },
    ).unwrap();
    assert_eq!(outcome.data["verified"], true);
    assert_eq!(outcome.data["capture"], capture);
    assert_eq!(
        evidence::read_blob_bounded(
            &temp.0,
            &capture["blob"],
            bytes.len() - 1,
            &OperationControl::default()
        )
        .unwrap_err()
        .code,
        "vault.blob_limit"
    );
    assert_eq!(
        evidence::read_blob_bounded(
            &temp.0,
            &capture["blob"],
            bytes.len(),
            &OperationControl::default()
        )
        .unwrap(),
        bytes
    );
    let forged = json!({"sha256":capture["blob"]["sha256"],"bytes":8});
    assert_eq!(
        evidence::read_blob_bounded(&temp.0, &forged, 8, &OperationControl::default())
            .unwrap_err()
            .code,
        "vault.blob_integrity"
    );
    let token = TrackedCancellation::new();
    token.cancel();
    let cancelled = OperationControl::tracked(
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        token,
    );
    assert_eq!(
        evidence::verify_blob_bounded(&temp.0, &capture["blob"], bytes.len() as u64, &cancelled)
            .unwrap_err()
            .exit_code,
        7
    );
    assert_eq!(
        evidence::commit_capture_bounded(&temp.0, draft(&bytes, bytes.len()), &cancelled)
            .unwrap_err()
            .exit_code,
        7
    );
    assert_eq!(
        Metadata::open(&temp.0, false)
            .unwrap()
            .list::<serde_json::Value>("capture/", 20)
            .unwrap()
            .len(),
        1
    );
    let mut changed = fs::OpenOptions::new().write(true).open(path).unwrap();
    changed.seek(SeekFrom::Start(65536)).unwrap();
    changed.write_all(&[0x52]).unwrap();
    drop(changed);
    assert_eq!(
        evidence::verify_blob_bounded(
            &temp.0,
            &capture["blob"],
            bytes.len() as u64,
            &OperationControl::default()
        )
        .unwrap_err()
        .code,
        "vault.blob_integrity"
    );
}
#[test]
fn workspace_errors_and_explicit_small_caps_keep_existing_defaults() {
    let temp = Temp::new();
    let expected = workspace::open(&temp.0).unwrap_err();
    for error in [
        evidence::publish_blob(&temp.0, b"x", 0).unwrap_err(),
        evidence::commit_capture(
            &temp.0,
            b"x",
            "text/plain",
            json!({"kind":"fixture","locator":"missing"}),
            true,
            false,
        )
        .unwrap_err(),
    ] {
        assert_eq!(error.code, expected.code);
        assert_eq!(error.exit_code, expected.exit_code);
        assert_eq!(error.details, expected.details);
    }
    fs::create_dir(&temp.0).unwrap();
    assert_eq!(
        evidence::publish_blob_bounded(&temp.0, b"x", 0, &OperationControl::default())
            .unwrap_err()
            .code,
        expected.code
    );
    let invalid = evidence::commit_capture(
        &temp.0,
        b"x",
        "",
        json!({"kind":"fixture","locator":"missing"}),
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(invalid.code, "evidence.invalid_capture");
    let home = Temp::initialized();
    assert_eq!(
        evidence::publish_blob(&home.0, b"small body", 9)
            .unwrap_err()
            .code,
        "vault.blob_limit"
    );
    let blob = evidence::publish_blob(&home.0, b"small body", 10).unwrap();
    assert_eq!(blob["sha256"], storage::sha256(b"small body"));
    assert_eq!(
        evidence::read_blob(&home.0, &blob, 10).unwrap(),
        b"small body"
    );
}

#[test]
fn streamed_blob_faults_remove_private_stage_before_any_capture_is_visible() {
    struct Producer {
        bytes: io::Cursor<Vec<u8>>,
        cancel: Option<TrackedCancellation>,
        terminal_error: bool,
    }
    impl Read for Producer {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let n = self.bytes.read(buffer)?;
            if let Some(token) = self.cancel.take() {
                token.cancel();
            }
            if n == 0 && self.terminal_error {
                return Err(io::Error::other(lycheedev_core::ToolkitError::new(
                    "records.content_integrity", "terminal CKey mismatch", 4,
                )));
            }
            Ok(n)
        }
    }
    for scenario in ["digest", "terminal", "short", "cancel"] {
        let temp = Temp::initialized();
        let metadata = Metadata::open(&temp.0, true).unwrap();
        let body = vec![0x67; 3 * 65536];
        let reference = evidence::BlobRef {
            sha256: if scenario == "digest" { "a".repeat(64) } else { storage::sha256(&body) },
            bytes: body.len() as u64,
        };
        let control = OperationControl::default();
        let cancel = control.tracked_cancel_handle().unwrap();
        let mut producer = Producer {
            bytes: io::Cursor::new(if scenario == "short" { body[..65536].to_vec() } else { body }),
            cancel: (scenario == "cancel").then(|| cancel),
            terminal_error: scenario == "terminal",
        };
        let error = evidence::publish_blob_reader_bounded(&temp.0, &mut producer, &reference,
            reference.bytes, &control).err().expect("fault must stop publication");
        if scenario == "cancel" {
            assert_eq!(error.exit_code, 7);
            assert_eq!(producer.bytes.position(), 65536);
        } else if scenario == "terminal" {
            assert_eq!(error.code, "records.content_integrity");
        }
        let destination = evidence::blob_path(&temp.0, &reference.sha256).unwrap();
        assert!(!destination.exists(), "{scenario}");
        assert!(fs::read_dir(destination.parent().unwrap()).unwrap().next().is_none(), "{scenario}");
        assert!(metadata.list::<serde_json::Value>("", 20).unwrap().is_empty());
    }
}
