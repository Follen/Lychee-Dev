use super::*;
use crate::mailbox::native_backend::tests::{Events, FakeSource};
use std::collections::VecDeque;
fn traced(directory: Option<PathBuf>) -> TracedReader<FakeSource> {
    TracedReader::new(
        FakeSource {
            queue: VecDeque::new(),
            events: Events::default(),
            close_error: None,
        },
        directory,
    )
}
fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
#[test]
fn native_trace_defers_io_caps_256_and_never_copies_payload() {
    let mut reader = traced(Some(PathBuf::from("not-created-or-accessed")));
    let c = control();
    for _ in 0..276 {
        reader
            .find(&c, &selector(Kind::Body, ZERO_TOKEN), None, true)
            .unwrap();
    }
    assert_eq!(reader.lookups.len(), 256);
    assert_eq!(reader.traces.len(), 256);
    assert_eq!(reader.omitted, 20);
    let mut writes = Vec::new();
    reader
        .flush_with(&c, |_, path, bytes| {
            writes.push(path.to_path_buf());
            let text = String::from_utf8_lossy(bytes);
            assert!(
                !text.contains("payload") && !text.contains("records") && !text.contains("address")
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(writes.len(), 257);
    assert!(reader.traces.is_empty());
    reader
        .find(&c, &selector(Kind::Input, ZERO_TOKEN), None, true)
        .unwrap();
    assert_eq!(reader.sequence, 256);
    assert_eq!(reader.omitted, 21);
    assert!(reader.traces.is_empty());
}
#[test]
fn native_trace_failed_write_preserves_only_unwritten_suffix_and_capacity() {
    let mut reader = traced(Some(PathBuf::from("traces")));
    let c = control();
    for _ in 0..3 {
        reader
            .find(&c, &selector(Kind::Input, ZERO_TOKEN), None, true)
            .unwrap();
    }
    let mut calls = 0;
    let error = reader
        .flush_with(&c, |_, _, _| {
            calls += 1;
            if calls == 2 {
                Err(external("disk-fault"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(
        error.to_string().contains("live.channel_trace_flush")
            && error.to_string().contains("disk-fault")
    );
    assert_eq!(reader.traces.len(), 2);
    assert_eq!(reader.traces.front().unwrap().sequence, 2);
    let mut paths = Vec::new();
    reader
        .flush_with(&c, |_, path, _| {
            paths.push(path.to_path_buf());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        paths,
        [
            PathBuf::from("traces/002.json"),
            PathBuf::from("traces/003.json")
        ]
    );
    assert_eq!(reader.sequence, 3);
}
#[test]
fn native_trace_cancelled_lookup_flushes_under_independent_bounded_control() {
    let mut reader = traced(Some(PathBuf::from("traces")));
    let cancelled = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    assert!(
        reader
            .find(&cancelled, &selector(Kind::Input, ZERO_TOKEN), None, true)
            .is_err()
    );
    reader.close().unwrap();
    let c = OperationControl::uncancelable(Instant::now() + DIAGNOSTIC_FLUSH_LIMIT);
    let mut calls = 0;
    reader
        .flush_with(&c, |scope, _, bytes| {
            calls += 1;
            assert!(scope.deadline() <= c.deadline());
            assert!(String::from_utf8_lossy(bytes).contains("command.deadline"));
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 1);
}
#[test]
fn native_trace_one_shared_deadline_stops_without_per_item_renewal() {
    let mut reader = traced(Some(PathBuf::from("traces")));
    let c = control();
    for _ in 0..3 {
        reader
            .find(&c, &selector(Kind::Input, ZERO_TOKEN), None, true)
            .unwrap();
    }
    let cancelled = lycheedev_core::control::TrackedCancellation::new();
    let flush =
        OperationControl::tracked(Instant::now() + Duration::from_secs(10), cancelled.clone());
    let mut calls = 0;
    let error = reader
        .flush_with(&flush, |scope, _, _| {
            calls += 1;
            cancelled.cancel();
            check_control(scope)
        })
        .unwrap_err();
    assert!(error.any_cause(&|e| matches!(e, ChannelError::Cancelled)));
    assert_eq!(calls, 1);
    assert_eq!(reader.traces.len(), 3);
}

#[test]
fn native_trace_io_failure_and_simultaneous_cancel_are_both_retained() {
    let mut reader = traced(Some(PathBuf::from("traces")));
    let cancellation = lycheedev_core::control::TrackedCancellation::new();
    let c = OperationControl::tracked(
        Instant::now() + Duration::from_secs(10),
        cancellation.clone(),
    );
    reader
        .find(&c, &selector(Kind::Input, ZERO_TOKEN), None, true)
        .unwrap();
    let error = reader
        .flush_with(&c, |_, _, _| {
            cancellation.cancel();
            Err(external("disk-fault"))
        })
        .unwrap_err();
    assert!(error.to_string().contains("disk-fault"));
    assert!(error.any_cause(&|e| matches!(e, ChannelError::Cancelled)));
    assert_eq!(reader.traces.len(), 1);
}

#[test]
fn native_trace_preserves_outer_json_keys() {
    let mut reader = traced(Some(PathBuf::from("traces")));
    let c = control();
    reader
        .find(&c, &selector(Kind::Input, ZERO_TOKEN), None, true)
        .unwrap();
    reader
        .flush_with(&c, |_, _, bytes| {
            let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let object = value.as_object().unwrap();
            for name in ["Nonce", "Runtime", "Ticket", "Kind", "Matches", "Lookup"] {
                assert!(object.contains_key(name), "{name}");
            }
            for name in ["nonce", "runtime", "ticket", "kind", "matches", "lookup"] {
                assert!(!object.contains_key(name), "{name}");
            }
            Ok(())
        })
        .unwrap();
}
