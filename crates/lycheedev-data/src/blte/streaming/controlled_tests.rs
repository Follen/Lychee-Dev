use super::*;
use lycheedev_core::control::{OperationControl, TrackedCancellation};
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

#[test]
fn streaming_empty_read_keeps_original_scope_without_reading_more_source() {
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let budget = Budget::controlled(
        Cost::defaults(),
        control.deadline(),
        OperationControl::default(),
    );
    let mut source = io::Cursor::new(b"BLTE\0\0\0\0Nabc".to_vec());
    let keys = BTreeMap::new();
    let check = || control.check();
    let mut reader = Reader::new_with_check(
        &mut source,
        12,
        3,
        Md5::digest(b"abc").into(),
        &keys,
        &budget,
        Some(&check),
    )
    .unwrap();
    token.cancel();
    assert_eq!(
        stream_io_error(reader.read(&mut []).unwrap_err()).code,
        "command.cancelled"
    );
    drop(reader);
    assert_eq!(source.position(), 8);
}
#[test]
fn original_scope_is_checked_inside_decode_after_encoded_input_was_read() {
    let control = OperationControl::default();
    let budget = Budget::controlled(
        Cost::defaults(),
        control.deadline(),
        OperationControl::default(),
    );
    let mut source = io::Cursor::new(b"BLTE\0\0\0\0Nabc".to_vec());
    let keys = BTreeMap::new();
    let calls = Cell::new(0);
    let check = || {
        let n = calls.get() + 1;
        calls.set(n);
        if n >= 4 {
            Err(fault("test.original_scope", 7, "cancel inside decoder"))
        } else {
            Ok(())
        }
    };
    let mut reader = Reader::new_with_check(
        &mut source,
        12,
        3,
        Md5::digest(b"abc").into(),
        &keys,
        &budget,
        Some(&check),
    )
    .unwrap();
    let error = stream_io_error(reader.read(&mut [0; 3]).unwrap_err());
    assert_eq!(error.code, "test.original_scope");
    drop(reader);
    assert_eq!(source.position(), 12);
    assert!(calls.get() >= 4);
}
