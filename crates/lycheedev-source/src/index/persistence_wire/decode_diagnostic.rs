//! Opt-in diagnostic only. Every decoder and calibration control check remains
//! real. There is no unchecked/no-op decoder, feature, product command or codec.
use super::*;
use lycheedev_core::storage;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    hint::black_box,
    path::PathBuf,
    time::{Duration, Instant},
};

const REPETITIONS: usize = 3;
const MAX_CHECKS: u64 = 64_000_000;
type SummaryIndex = Index<Counted<Document>, Counted<AssetRecord>, Counted<Symbol>>;
thread_local! {
    static CHECKS: Cell<Option<u64>> = const { Cell::new(None) };
}
pub(super) fn observe_check() {
    CHECKS.with(|slot| {
        if let Some(count) = slot.get() {
            assert!(count < MAX_CHECKS, "diagnostic check budget");
            slot.set(Some(count + 1));
        }
    });
}
struct Counting;
impl Counting {
    fn start() -> Self {
        CHECKS.with(|slot| {
            assert!(slot.get().is_none());
            slot.set(Some(0));
        });
        Self
    }
    fn value(&self) -> u64 {
        CHECKS.with(|slot| slot.get().unwrap())
    }
}
impl Drop for Counting {
    fn drop(&mut self) {
        CHECKS.with(|slot| slot.set(None));
    }
}
fn counting_disabled() -> bool {
    CHECKS.with(|slot| slot.get().is_none())
}
fn snapshot(index: &SummaryIndex, control: &OperationControl) -> Result<Value> {
    // Hash the complete summary/gaps without allocating a second JSON tree or
    // unbounded output buffer. Escaped JSON is subject to the existing cap.
    use sha2::{Digest, Sha256};
    struct Proof<'a> {
        hash: Sha256,
        bytes: usize,
        control: &'a OperationControl,
        failure: Option<ToolkitError>,
    }
    impl std::io::Write for Proof<'_> {
        fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
            let result = (|| -> Result<()> {
                self.control.check()?;
                self.bytes = self
                    .bytes
                    .checked_add(raw.len())
                    .filter(|n| *n <= MAXIMUM)
                    .ok_or_else(budget)?;
                for chunk in raw.chunks(65536) {
                    self.control.check()?;
                    self.hash.update(chunk);
                }
                Ok(())
            })();
            if let Err(error) = result {
                self.failure = Some(error);
                return Err(std::io::Error::other("diagnostic proof stopped"));
            }
            Ok(raw.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut proof = Proof {
        hash: Sha256::new(),
        bytes: 0,
        control,
        failure: None,
    };
    let result = serde_json::to_writer(&mut proof, &(&index.summary, &index.compatibility_gaps));
    if let Some(error) = proof.failure {
        return Err(error);
    }
    result?;
    Ok(
        json!({"summaryAndGapsSHA256":format!("{:x}", proof.hash.finalize()),
        "documents":index.documents.len(),"assets":index.assets.len(),"symbols":index.symbols.len()}),
    )
}

fn fresh(deadline: Instant) -> OperationControl {
    // Matches CLI source-index's tracked root, zero parent cancellations.
    // All repetitions share one fixed deadline, not a renewed 180s allowance.
    OperationControl::uncancelable(deadline)
}
fn diagnose(raw: &[u8], identity: &str, deadline: Instant) -> Result<Value> {
    assert!(counting_disabled());
    let (expected, checks) = {
        let control = fresh(deadline);
        let counting = Counting::start();
        let index: SummaryIndex = decode(raw, identity, &control)?;
        let checks = counting.value();
        drop(counting);
        let expected = snapshot(&index, &control)?;
        drop(index);
        (expected, checks)
    };
    assert!((1..=MAX_CHECKS).contains(&checks));
    assert!(counting_disabled());
    let mut rows = Vec::with_capacity(REPETITIONS);
    for repetition in 0..REPETITIONS {
        // Alternate order to expose drift; these are separate observations,
        // never subtractive attribution or paired performance acceptance.
        let order = if repetition % 2 == 0 {
            [0, 1, 2]
        } else {
            [2, 1, 0]
        };
        let mut nanos = [0u128; 3];
        for phase in order {
            let control = fresh(deadline);
            control.check()?;
            assert!(counting_disabled());
            match phase {
                0 => {
                    let start = Instant::now();
                    let index: SummaryIndex =
                        decode(black_box(raw), black_box(identity), &control)?;
                    nanos[0] = start.elapsed().as_nanos();
                    // The result, validation snapshot and owner are destroyed
                    // before the next phase/repetition. Teardown is not timed.
                    assert_eq!(snapshot(&index, &control)?, expected);
                    black_box(&index);
                    drop(index);
                }
                1 => {
                    let start = Instant::now();
                    for _ in 0..checks {
                        black_box(&control).check()?;
                    }
                    nanos[1] = start.elapsed().as_nanos();
                }
                _ => {
                    let start = Instant::now();
                    for i in 0..checks {
                        // Calibration remains deadline-responsive. These
                        // maintenance checks are explicitly not subtracted.
                        if i % 4096 == 0 {
                            control.check()?;
                        }
                        black_box(i);
                    }
                    nanos[2] = start.elapsed().as_nanos();
                }
            }
            assert!(counting_disabled());
            control.check()?;
            drop(control);
        }
        rows.push(json!({"repetition":repetition,"order":order,
            "decodeNanos":nanos[0],"realControlLoopNanos":nanos[1],"emptyLoopNanos":nanos[2]}));
    }
    Ok(
        json!({"schema":"lycheedev.source-decode-control-diagnostic.v1",
        "diagnosticOnly":true,"performanceAcceptance":false,"debugAssertions":cfg!(debug_assertions),
        "optimizedArtifactVerification":"required from external owner: sealed release or symbols test artifact",
        "maximumChecksPerCountedPassOrLoop":MAX_CHECKS,
        "os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "checkCalls":checks,"repetitions":REPETITIONS,"rows":rows,"validated":expected,
        "controlTopology":"tracked-root-no-parent-cancellations-fixed-total-180s-deadline",
        "emptyLoopMaintenanceStride":4096,"countingEnabledDuringTiming":false,
        "limits":"test-build TLS hooks and disabled-counter branch remain; isolated control-loop cache/code context differs; deadline is cooperative and external owner timeout is still required; summary/gaps JSON proof has separate 128MiB cap and may reject escape expansion; 64m checks is per counted pass/loop not invocation total; no subtraction, causal claim, product speedup or acceptance; no cold-index attribution"}),
    )
}

#[test]
#[ignore = "optimized owner-authenticated Source benchmark Index fixture and sealed test artifact required"]
fn benchmark_authenticated_index_decode_controls() {
    assert!(
        !cfg!(debug_assertions),
        "debug assertions must be off; owner must additionally verify sealed --release or --profile symbols artifact"
    );
    let input = PathBuf::from(
        std::env::var_os("LYCHEEDEV_SOURCE_INDEX_EXPORT_INPUT")
            .expect("existing Source benchmark manifest"),
    );
    let bytes = std::env::var("LYCHEEDEV_SOURCE_INDEX_EXPORT_BYTES")
        .expect("manifest size")
        .parse::<u64>()
        .expect("unsigned manifest size");
    let sha = std::env::var("LYCHEEDEV_SOURCE_INDEX_EXPORT_SHA256").expect("manifest SHA256");
    let control = OperationControl::default();
    let (raw, identity) = super::export_tests::diagnostic_input(&input, bytes, &sha, &control)
        .expect("authenticated fixture");
    let raw_sha = storage::sha256(&raw);
    let raw_bytes = raw.len();
    let mut report =
        diagnose(&raw, &identity, control.deadline()).expect("real-control diagnostic");
    drop(raw);
    // No two complete Index input buffers overlap. Reauthenticate the original
    // manifest and body after all timings, before issuing a successful report.
    let (raw, final_identity) =
        super::export_tests::diagnostic_input(&input, bytes, &sha, &control)
            .expect("fixture final integrity");
    assert_eq!(final_identity, identity);
    assert_eq!(raw.len(), raw_bytes);
    assert_eq!(storage::sha256(&raw), raw_sha);
    drop(raw);
    control.check().unwrap();
    report["fixture"] = json!({"manifestSHA256":sha,"manifestBytes":bytes,"indexSHA256":raw_sha,"indexBytes":raw_bytes,"identity":identity,"finalIntegrityVerified":true});
    println!("{}", serde_json::to_string(&report).unwrap());
}

#[test]
fn decode_diagnostic_counter_is_scoped_and_keeps_real_cancellation() {
    assert!(counting_disabled());
    let control = OperationControl::default();
    {
        let count = Counting::start();
        check_control(&control).unwrap();
        assert_eq!(count.value(), 1);
        control.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(
            check_control(&control).unwrap_err().code,
            "command.cancelled"
        );
        assert_eq!(count.value(), 2);
    }
    assert!(counting_disabled());
    let count = Counting::start();
    let control = OperationControl::uncancelable(Instant::now());
    assert_eq!(
        check_control(&control).unwrap_err().code,
        "command.deadline"
    );
    assert_eq!(count.value(), 1);
}
#[test]
fn decode_diagnostic_uses_complete_decoder_and_drops_counting_on_error() {
    let (index, pin) = super::tests::fixture();
    let identity = crate::index::fingerprint(&pin);
    let raw = encode(&index, &identity, &OperationControl::default()).unwrap();
    let report = diagnose(&raw, &identity, Instant::now() + Duration::from_secs(180)).unwrap();
    assert_eq!(report["validated"]["symbols"], 3);
    assert_eq!(report["rows"].as_array().unwrap().len(), REPETITIONS);
    assert_eq!(report["countingEnabledDuringTiming"], false);
    assert!(counting_disabled());
    let mut corrupt = raw;
    corrupt.push(0);
    assert!(
        diagnose(
            &corrupt,
            &identity,
            Instant::now() + Duration::from_secs(180)
        )
        .is_err()
    );
    assert!(counting_disabled());
}
