use super::*;
use lycheedev_core::control::TrackedCancellation;
use std::{
    io::{Cursor, Read, Write},
    pin::Pin,
    task::{Context, Poll},
};
struct Ready<R>(R);
impl<R: Read + Unpin> AsyncRead for Ready<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        out: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.0.read(out.initialize_unfilled()) {
            Ok(n) => {
                out.advance(n);
                Poll::Ready(Ok(()))
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}
fn packed(raw: &[u8]) -> Vec<u8> {
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(raw).unwrap();
    gzip.finish().unwrap()
}
fn read(
    input: Vec<u8>,
    gzip: bool,
    maximum: usize,
    control: &OperationControl,
    budget: &Budget,
    admit: &mut dyn FnMut(usize) -> Result<()>,
) -> Result<Vec<u8>> {
    run(async {
        if gzip {
            super::gzip::collect_admitted(
                Ready(Cursor::new(input)),
                ReadLimit::Prefix(maximum),
                control,
                Some(budget),
                control.deadline(),
                admit,
            )
            .await
        } else {
            collect_admitted(
                Ready(Cursor::new(input)),
                ReadLimit::Prefix(maximum),
                control,
                Some(budget),
                control.deadline(),
                admit,
            )
            .await
        }
    })
}
#[test]
fn raw_and_gzip_prefix_admission_records_exact_delivered_count_without_extra_probe() {
    for gzip in [false, true] {
        let raw = vec![b'x'; 20_000];
        let input = if gzip { packed(&raw) } else { raw.clone() };
        let control = OperationControl::default();
        let budget = Budget::new(Cost::defaults(), control.deadline(), None);
        let mut accepted = Vec::new();
        let out = read(input, gzip, 10_000, &control, &budget, &mut |n| {
            accepted.push(n);
            Ok(())
        })
        .unwrap();
        assert_eq!(out, raw[..10_000]);
        assert_eq!(accepted.iter().sum::<usize>(), 10_000);
        assert_eq!(accepted.len(), 2);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 10_000);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }
}
#[test]
fn raw_and_gzip_consumer_quota_failure_preserves_network_and_exact_typed_cause() {
    for gzip in [false, true] {
        let raw = b"consumer body";
        let input = if gzip { packed(raw) } else { raw.to_vec() };
        let control = OperationControl::default();
        let budget = Budget::new(Cost::defaults(), control.deadline(), None);
        let mut calls = 0;
        let mut expected = fault("fixture.admission", 3, "retained quota");
        expected.details = serde_json::json!({"owner":"definitions"});
        let error = read(input, gzip, 100, &control, &budget, &mut |n| {
            calls += 1;
            assert_eq!(n, raw.len());
            Err(expected.clone())
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error.code, expected.code);
        assert_eq!(error.details, expected.details);
        assert_eq!(budget.snapshot().unwrap().network_bytes, raw.len() as u64);
    }
}
#[test]
fn empty_prefix_never_reads_or_calls_consumer_for_raw_or_gzip() {
    for gzip in [false, true] {
        let control = OperationControl::default();
        let budget = Budget::new(Cost::defaults(), control.deadline(), None);
        let result = read(
            b"invalid gzip is never consumed".to_vec(),
            gzip,
            0,
            &control,
            &budget,
            &mut |_| panic!("empty prefix invoked admission"),
        )
        .unwrap();
        assert!(result.is_empty());
        assert_eq!(budget.snapshot().unwrap(), Cost::default());
    }
}
#[test]
fn raw_stop_after_read_preserves_observed_bytes_without_running_consumer() {
    struct Cancelling {
        token: TrackedCancellation,
    }
    impl Read for Cancelling {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            out[..4].copy_from_slice(b"read");
            self.token.cancel();
            Ok(4)
        }
    }
    let token = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(10), token.clone());
    let budget = Budget::new(Cost::defaults(), control.deadline(), None);
    let result = run(collect_admitted(
        Ready(Cancelling { token }),
        ReadLimit::Prefix(8),
        &control,
        Some(&budget),
        control.deadline(),
        &mut |_| panic!("stopped read admitted retention"),
    ));
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert_eq!(budget.snapshot().unwrap().network_bytes, 4);
}
#[test]
fn raw_and_gzip_network_quota_failure_precedes_consumer_admission() {
    for gzip in [false, true] {
        let raw = b"over quota";
        let input = if gzip { packed(raw) } else { raw.to_vec() };
        let control = OperationControl::default();
        let budget = Budget::new(
            Cost {
                network_bytes: 3,
                ..Cost::defaults()
            },
            control.deadline(),
            None,
        );
        let error = read(input, gzip, 100, &control, &budget, &mut |_| {
            panic!("rejected network read reached consumer")
        })
        .unwrap_err();
        assert_eq!(error.code, "records.query_resource_budget");
        assert_eq!(error.details["observedReadBytes"], 4);
        assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
    }
}
