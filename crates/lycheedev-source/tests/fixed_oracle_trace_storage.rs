//! Storage regressions only; the original ten semantic targets / 36 tests and
//! their 16,384-row assertions remain unchanged and run independently.
#[path = "support/fixed_oracle_storage.rs"]
mod storage;
#[path = "support/fixed_oracle_trace.rs"]
mod trace;
use lycheedev_core::{control::OperationControl, process};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
use storage::{MAX_PART_BYTES, MAX_TOTAL_BYTES, TraceOutput, finish, read_frames};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-oracle-storage-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        if std::thread::panicking() {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr().lock(),
                "retained oracle fixture evidence: {}",
                self.0.display()
            );
        } else {
            fs::remove_dir_all(&self.0)
                .expect("remove successful test's owned temporary directory");
        }
    }
}
const NAME: &str = "storage--full_rows.jsonl";

#[test]
fn large_case_rolls_complete_rows_without_changing_any_field_or_ordinal() {
    let root = Temp::new();
    let mut output = TraceOutput::new(&root.0, NAME).unwrap();
    let header = json!({"kind":"header","schema":"fixture","role":"reference","target":"storage","test":"full_rows"});
    output.emit(&header).unwrap();
    let value = "字".repeat(2 << 20);
    for ordinal in 0..4 {
        output
            .emit(&json!({"kind":"row","ordinal":ordinal,"label":"exact.coverage","value":value}))
            .unwrap();
    }
    finish(&mut output, 4, true).unwrap();
    drop(output);
    let files: Vec<_> = fs::read_dir(&root.0).unwrap().map(|p| p.unwrap()).collect();
    assert!(files.len() > 1);
    assert!(
        files
            .iter()
            .all(|p| p.metadata().unwrap().len() <= MAX_PART_BYTES)
    );
    let frames = read_frames(&root.0, NAME).unwrap();
    assert_eq!(frames.len(), 6);
    assert_eq!(frames[0], header);
    for ordinal in 0..4 {
        assert_eq!(
            frames[ordinal + 1],
            json!({"kind":"row","ordinal":ordinal,"label":"exact.coverage","value":value})
        );
    }
    assert_eq!(frames[5], json!({"kind":"end","rows":4,"complete":true}));
}

#[test]
fn oversized_single_row_is_rejected_before_write_and_cannot_claim_completion() {
    let root = Temp::new();
    let mut output = TraceOutput::new(&root.0, NAME).unwrap();
    output.emit(&json!({"kind":"header"})).unwrap();
    let before = fs::metadata(root.0.join(NAME)).unwrap().len();
    let error = output
        .emit(&json!("x".repeat(MAX_PART_BYTES as usize)))
        .unwrap_err();
    assert_eq!(error.to_string(), "oracle.trace_row_budget");
    assert_eq!(fs::metadata(root.0.join(NAME)).unwrap().len(), before);
    assert!(output.failed());
    finish(&mut output, 0, true).unwrap();
    assert_eq!(
        read_frames(&root.0, NAME).unwrap().last().unwrap(),
        &json!({"kind":"end","rows":0,"complete":false})
    );
}

#[test]
fn aggregate_evidence_budget_stays_finite_and_never_writes_a_surplus_part() {
    let root = Temp::new();
    let mut output = TraceOutput::new(&root.0, NAME).unwrap();
    let row = json!("x".repeat(MAX_PART_BYTES as usize - 4));
    for _ in 0..4 {
        output.emit(&row).unwrap();
    }
    assert_eq!(
        output.emit(&row).unwrap_err().to_string(),
        "oracle.trace_total_budget"
    );
    let files: Vec<_> = fs::read_dir(&root.0).unwrap().map(|p| p.unwrap()).collect();
    assert_eq!(files.len(), 4);
    assert!(
        files
            .iter()
            .all(|p| p.metadata().unwrap().len() <= MAX_PART_BYTES)
    );
    assert!(
        files
            .iter()
            .map(|p| p.metadata().unwrap().len())
            .sum::<u64>()
            <= MAX_TOTAL_BYTES
    );
}

#[test]
fn readers_reject_gaps_invalid_names_empty_parts_and_unterminated_rows() {
    for (suffix, bytes, reason) in [
        (".part2", "{}\n", "oracle.trace_part_gap"),
        (".part01", "{}\n", "oracle.trace_part_name"),
        (".part1", "", "oracle.trace_budget"),
        (".part1", "{}", "oracle.truncated_line"),
        (".part1", "\n", "oracle.empty_line"),
    ] {
        let root = Temp::new();
        fs::write(root.0.join(NAME), b"{}\n").unwrap();
        fs::write(root.0.join(format!("{NAME}{suffix}")), bytes).unwrap();
        assert_eq!(read_frames(&root.0, NAME).unwrap_err().to_string(), reason);
    }
}

#[test]
fn guard_keeps_the_original_test_failure_without_aborting_during_unwind() {
    let root = Temp::new();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "trace_row_failure_peer",
            "--nocapture",
        ])
        .env("LYCHEEDEV_EMMY_ORACLE_CAPTURE", &root.0);
    let control = OperationControl::new(
        std::time::Instant::now() + std::time::Duration::from_secs(10),
        Default::default(),
    );
    // Reuse the existing Core owned Job, bounded pipes, original control and
    // checked child wait. No raw Command::spawn/status/output execution.
    let result = process::run(command, None, 1 << 20, 1 << 20, &control);
    let output = match result {
        Ok(output) => output,
        Err(error) => {
            fs::write(root.0.join("peer-control-error.txt"), format!("{error:?}")).unwrap();
            panic!("controlled trace peer failed: {error:?}");
        }
    };
    fs::write(root.0.join("peer-stdout.bin"), &output.stdout).unwrap();
    fs::write(root.0.join("peer-stderr.bin"), &output.stderr).unwrap();
    fs::write(
        root.0.join("peer-status.json"),
        json!({"status":output.status,"transport":"core.process.run","deadlineSeconds":10})
            .to_string(),
    )
    .unwrap();
    assert_eq!(
        output.status, 101,
        "normal libtest failure, never native abort"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("oracle.trace_row_budget"),
        "the original failure remains explicit: {stderr}"
    );
    assert!(
        !stderr.contains("panic in a destructor"),
        "no second panic during unwinding"
    );
    let name = format!("{}--trace_row_failure_peer.jsonl", env!("CARGO_CRATE_NAME"));
    let frames = read_frames(&root.0, &name).unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[1], json!({"kind":"end","rows":0,"complete":false}));
    assert!(
        fs::read_dir(&root.0)
            .unwrap()
            .all(|p| p.unwrap().metadata().unwrap().len() <= MAX_PART_BYTES)
    );
}

#[test]
#[ignore = "spawned by the normal parent test to inspect its exact failing exit"]
fn trace_row_failure_peer() {
    let _trace = trace::begin_reference();
    trace::record("oversized", &"x".repeat(MAX_PART_BYTES as usize));
    panic!("oversized row must fail before this point");
}
