use super::*;
use crate::mailbox::lookup::Record;
use crate::mailbox::{Kind, ZERO_TOKEN};
use lycheedev_core::control::TrackedCancellation;
use std::time::{Duration, Instant};

struct FakeReader {
    calls: usize,
    fail: Option<String>,
    cancel: Option<TrackedCancellation>,
    cancel_at_return: Option<TrackedCancellation>,
    returned_record: bool,
}
impl SelectedReader for FakeReader {
    fn lookup(
        &mut self,
        _: &Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure> {
        self.calls += 1;
        let result = (|| {
            budget.charge(RuntimeCost {
                read_calls: 1,
                requested_bytes: 16,
                ..RuntimeCost::default()
            })?;
            if let Some(cancel) = &self.cancel {
                cancel.cancel();
            }
            if let Some(fail) = &self.fail {
                return Err(fail.clone());
            }
            budget.check()?;
            let record = Record {
                address: 1,
                nonce: "0".repeat(32),
                runtime: "1".repeat(32),
                ticket: "0".repeat(32),
                kind: 1,
                state: 1,
                sequence: 1,
                checksum: 1,
                payload: vec![],
            };
            let accepted = match accept {
                Some(accept) => accept(&record),
                None => true,
            };
            budget.check()?;
            self.returned_record = true;
            // Model a stop after the lookup's last check, while the caller
            // still must admit the returned record under its original ledger.
            if let Some(cancel) = &self.cancel_at_return {
                cancel.cancel();
            }
            Ok(accepted.then_some(record))
        })();
        result.map_err(|cause| RuntimeBindingFailure {
            cause,
            stage: "lookup",
            root: Box::default(),
            usage: budget.usage(),
            first_read_failure: None,
        })
    }
}
fn control() -> OperationControl {
    OperationControl::default()
}
fn state(control: &OperationControl) -> ReadState<FakeReader> {
    state_with_limits(control, RuntimeLimits::defaults())
}
fn state_with_limits(control: &OperationControl, limits: RuntimeLimits) -> ReadState<FakeReader> {
    ReadState {
        reader: FakeReader {
            calls: 0,
            fail: None,
            cancel: None,
            cancel_at_return: None,
            returned_record: false,
        },
        budget: runtime_budget(control, limits).unwrap(),
        last_failure: None,
    }
}
fn identity() -> Selector {
    Selector {
        kind: Kind::Identity,
        nonce: ZERO_TOKEN,
        runtime: ZERO_TOKEN,
        ticket: ZERO_TOKEN,
        body: None,
    }
}
#[test]
fn cancelled_formal_open_stops_before_issuing_any_runtime_reader() {
    let control = control();
    control.tracked_cancel_handle().unwrap().cancel();
    let window = WindowIdentity {
        hwnd: 0,
        process: lycheedev_windows::ProcessIdentity {
            pid: 0,
            created: u64::MAX,
        },
    };
    let failure = NativeReader::open(window, lycheedev_core::VERSION, &control)
        .err()
        .unwrap();
    assert_eq!(failure.cause, "memory.runtime_cancelled");
    assert_eq!(failure.stage, "budget");
    assert_eq!(failure.usage, RuntimeCost::default());
    assert!(!failure.root.full_text_pass);
    assert!(failure.first_read_failure.is_none());
}

#[test]
fn mailbox_native_reader_retains_one_ledger_across_named_reads() {
    let c = control();
    let mut reader = state(&c);
    let id = reader.budget.request_id();
    assert_eq!(reader.find(&c, &identity(), None).unwrap().len(), 1);
    let used = reader.budget.usage();
    let mut reject = |_: &Record| false;
    assert!(
        reader
            .find(&c, &identity(), Some(&mut reject))
            .unwrap()
            .is_empty()
    );
    assert_eq!(reader.reader.calls, 2);
    assert_eq!(reader.budget.request_id(), id);
    assert_eq!(reader.budget.usage().read_calls, used.read_calls + 1);
    assert_eq!(
        reader.budget.usage().requested_bytes,
        used.requested_bytes + 16
    );
}
#[test]
fn mailbox_native_reader_checks_current_child_inside_original_budget() {
    let parent = control();
    let child = parent
        .ordered_child(Instant::now() + Duration::from_secs(5))
        .unwrap();
    let mut reader = state(&parent);
    reader.reader.cancel = Some(child.tracked_cancel_handle().unwrap());
    assert!(
        reader
            .find(&child, &identity(), None)
            .unwrap_err()
            .any_cause(&|error| matches!(error, ChannelError::Cancelled))
    );
    assert_eq!(reader.reader.calls, 1);
    assert_eq!(reader.budget.usage().read_calls, 1);
    assert!(parent.check().is_ok());
}
#[test]
fn mailbox_native_reader_keeps_independent_fault_and_cancel() {
    let c = control();
    let mut reader = state(&c);
    reader.reader.fail = Some("mailbox.path_changed\nwindow.reused".into());
    reader.reader.cancel = Some(c.tracked_cancel_handle().unwrap());
    let error = reader.find(&c, &identity(), None).unwrap_err();
    assert!(error.any_cause(&|error| matches!(error, ChannelError::Cancelled)));
    assert!(error.any_cause(&|error| matches!(
        error,
        ChannelError::Contract(ContractError("memory.mailbox_unavailable: path_changed"))
    )));
    assert!(error.to_string().contains("window.reused"));
    assert_eq!(
        reader.last_failure.as_ref().unwrap().cause,
        "mailbox.path_changed\nwindow.reused"
    );
    // A subsequent cancelled request did not perform a new read or inherit the
    // prior lookup's diagnostic as if it belonged to this request.
    assert!(reader.find(&c, &identity(), None).is_err());
    assert!(reader.last_failure.is_none());
    assert_eq!(reader.reader.calls, 1);
}
#[test]
fn mailbox_native_reader_only_classifies_exact_transient_leaves() {
    for (source, expected) in [
        (
            "mailbox.path_changed",
            "memory.mailbox_unavailable: path_changed",
        ),
        (
            "mailbox.lua_state_null",
            "memory.mailbox_unavailable: lua_state_null",
        ),
        (
            "mailbox.string_layout",
            "memory.mailbox_unavailable: string_layout",
        ),
    ] {
        assert!(
            matches!(classify_reader_error(source), ChannelError::Contract(ContractError(value)) if value == expected)
        );
    }
    for source in [
        "mailbox.string_layout_extra",
        "root.anchor_changed",
        "reader.runtime_metadata_changed",
        "memory.runtime_private_read: hresult=0x80070005",
    ] {
        assert!(matches!(
            classify_reader_error(source),
            ChannelError::External { .. }
        ));
    }
    assert!(
        matches!(classify_reader_error("mailbox.path_changed\nwindow.reused"), ChannelError::Joined { causes } if causes.len() == 2)
    );
}

#[test]
fn mailbox_formal_limits_include_trusted_image_and_keep_other_caps() {
    let limits = native_limits();
    let ordinary = RuntimeLimits::defaults();
    // Fixed Go EXE<=512MiB, complete EOF probe, header<=1MiB, scan at most
    // 2*128MiB + 2*1MiB +4096; remainder is shared with guarded Lua reads.
    let maximum_issuance = (512u64 << 20) + 1 + (1 << 20) + (2 << 20) + (256 << 20) + 4096;
    assert!(limits.requested_bytes > maximum_issuance);
    // Full root scan plus complete hash plus all 16 maximum-size candidate
    // lookups (512 each), their root/layout rechecks, final binding and lookup.
    let maximum_open_and_lookup_reads = 8192 + 513 + 32 + 16 * (512 + 54) + 18 + 512 + 36;
    assert!(limits.read_calls > maximum_open_and_lookup_reads);
    // Conservative comparison charging: leading-byte test + all three pattern
    // lengths, the two-pass RPM copy charge and complete EXE hash charge.
    let worst_work = (128u64 << 20) * (1 + 58 + 49 + 49 + 4) + (512 << 20);
    assert!(limits.work_units > worst_work);
    assert_eq!(limits.os_call_units, ordinary.os_call_units);
    assert_eq!(limits.query_calls, ordinary.query_calls);
    assert_eq!(limits.allocation_bytes, ordinary.allocation_bytes);
    let c = control();
    let mut b = runtime_budget(&c, limits).unwrap();
    assert_eq!(b.deadline(), c.deadline());
    let request = b.request_id();
    b.charge(RuntimeCost {
        read_calls: maximum_open_and_lookup_reads,
        requested_bytes: maximum_issuance,
        work_units: worst_work,
        ..RuntimeCost::default()
    })
    .unwrap();
    assert_eq!(b.request_id(), request);
    c.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(b.check().unwrap_err(), "memory.runtime_cancelled");
}

#[test]
fn native_reader_finish_always_closes_and_preserves_operation_cleanup_and_control_errors() {
    use std::cell::Cell;
    let calls = Cell::new(0);
    let original = ChannelError::External {
        causes: vec!["fixture.read_failed".into()],
    };
    let error = finish_with_close::<()>(Err(original), || {
        calls.set(calls.get() + 1);
        Err(ChannelError::Joined {
            causes: vec![
                ChannelError::External {
                    causes: vec!["memory.runtime_process_close: fixture".into()],
                },
                ChannelError::Cancelled,
            ],
        })
    })
    .unwrap_err();
    assert_eq!(calls.get(), 1);
    assert!(error.to_string().contains("fixture.read_failed"));
    assert!(error.to_string().contains("memory.runtime_process_close"));
    assert!(error.any_cause(&|cause| matches!(cause, ChannelError::Cancelled)));
    assert_eq!(finish_with_close(Ok(17), || Ok(())).unwrap(), 17);
    assert!(finish_with_close(Ok(17), || Err(ChannelError::DeadlineExceeded)).is_err());
}

#[test]
fn mailbox_native_reader_record_allocation_failure_keeps_independent_child_cancel() {
    let parent = control();
    let child = parent.ordered_child(parent.deadline()).unwrap();
    // This legal read establishes the exact admission point after the current
    // scope and actual fixture read. Removing one byte allows that whole read,
    // but rejects the following Record allocation under the same ledger.
    let mut baseline = state(&parent);
    assert_eq!(baseline.find(&child, &identity(), None).unwrap().len(), 1);
    assert!(baseline.reader.returned_record);
    let mut limits = RuntimeLimits::defaults();
    limits.allocation_bytes = baseline.budget.usage().allocation_bytes - 1;
    let mut reader = state_with_limits(&parent, limits);
    let ledger = reader.budget.request_id();
    reader.reader.cancel_at_return = Some(child.tracked_cancel_handle().unwrap());
    let error = reader.find(&child, &identity(), None).unwrap_err();
    assert!(
        reader.reader.returned_record,
        "failure occurred after the real fixture lookup returned its record"
    );
    assert_eq!(reader.reader.calls, 1);
    assert_eq!(reader.budget.usage().read_calls, 1);
    assert_eq!(
        reader.budget.request_id(),
        ledger,
        "allocation uses the original ledger"
    );
    assert!(
        parent.check().is_ok(),
        "the original open parent remains active"
    );
    assert_eq!(child.check().unwrap_err().code, "command.cancelled");
    assert!(error.any_cause(&|cause| matches!(cause, ChannelError::External { causes } if causes.iter().any(|cause| cause == "memory.runtime_budget:allocations"))), "record allocation cause was lost: {error:?}");
    assert!(
        error.any_cause(&|cause| matches!(cause, ChannelError::Cancelled)),
        "independent child cancellation was lost: {error:?}"
    );
}
