use super::*;
use crate::mailbox::{Kind, ZERO_TOKEN};
use crate::mailbox::lookup::Record;
use lycheedev_core::control::TrackedCancellation;
use std::time::{Duration, Instant};

struct FakeReader {
    calls: usize,
    fail: Option<String>,
    cancel: Option<TrackedCancellation>,
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
    ReadState {
        reader: FakeReader {
            calls: 0,
            fail: None,
            cancel: None,
        },
        budget: runtime_budget(control, RuntimeLimits::defaults()).unwrap(),
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
