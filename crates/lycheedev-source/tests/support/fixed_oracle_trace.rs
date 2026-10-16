//! Test-only bridge between the fixed historical engine and the clean dependency.
//! Every reference row is evaluated by the historical engine, not by OwnedQuery.
//! Missing, truncated, reordered, or surplus reference rows are hard failures.
#![allow(dead_code)]
use serde_json::{Value, json};
use std::{cell::RefCell, fmt::Debug, path::PathBuf};
#[path = "fixed_oracle_storage.rs"]
mod storage;
use storage::TraceOutput;

const SCHEMA: &str = "lycheedev.emmy-fixed-oracle.v1";
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }
struct State {
    reference: bool,
    expected: Vec<Value>,
    ordinal: usize,
    mismatches: usize,
    first_mismatch: Option<String>,
    output: TraceOutput,
}
pub struct Guard;
fn file_name() -> String {
    let thread = std::thread::current();
    let name = thread.name().expect("named Rust test thread");
    assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    format!("{}--{name}.jsonl", env!("CARGO_CRATE_NAME"))
}
fn emit(output: &mut TraceOutput, value: &Value) {
    output.emit(value).expect("write bounded oracle evidence");
}
fn begin(reference: bool) -> Guard {
    let name = file_name();
    let target = env!("CARGO_CRATE_NAME");
    let test = std::thread::current().name().unwrap().to_owned();
    let expected = if reference {
        Vec::new()
    } else {
        let directory = std::env::var_os("LYCHEEDEV_EMMY_ORACLE_REFERENCE")
            .expect("the controlled runner must supply complete fixed-engine reference evidence");
        let mut frames = storage::read_frames(&PathBuf::from(directory), &name).expect(
            "complete bounded reference evidence must exist; never substitute owned output",
        );
        assert!(
            frames.len() >= 2,
            "reference header and terminal are mandatory"
        );
        let header = frames.remove(0);
        assert_eq!(
            header,
            json!({"kind":"header","schema":SCHEMA,"role":"reference","target":target,"test":test})
        );
        let terminal = frames.pop().unwrap();
        assert_eq!(
            terminal,
            json!({"kind":"end","rows":frames.len(),"complete":true})
        );
        for (index, row) in frames.iter().enumerate() {
            assert_eq!(row["kind"], "row");
            assert_eq!(row["ordinal"], index);
            assert!(row["label"].is_string() && row["value"].is_string());
            assert_eq!(row.as_object().unwrap().len(), 4);
        }
        frames
    };
    let directory = std::env::var_os("LYCHEEDEV_EMMY_ORACLE_CAPTURE")
        .expect("the controlled runner must supply a fresh owned evidence directory");
    let mut output = TraceOutput::new(&PathBuf::from(directory), &name)
        .expect("never overwrite prior oracle evidence");
    emit(
        &mut output,
        &json!({"kind":"header","schema":SCHEMA,"role":if reference {"reference"} else {"candidate"},"target":target,"test":test}),
    );
    STATE.with(|state| {
        assert!(state.borrow().is_none(), "one trace owner per test");
        *state.borrow_mut() = Some(State {
            reference,
            expected,
            ordinal: 0,
            mismatches: 0,
            first_mismatch: None,
            output,
        });
    });
    Guard
}
pub fn begin_reference() -> Guard {
    begin(true)
}
pub fn begin_candidate() -> Guard {
    begin(false)
}
/// Record only a value independently produced by the fixed historical engine.
pub fn record(label: &str, expected: &impl Debug) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut().expect("trace guard before reference queries");
        assert!(state.reference);
        let row = json!({"kind":"row","ordinal":state.ordinal,"label":label,"value":format!("{expected:?}")});
        emit(&mut state.output, &row);
        state.ordinal += 1;
    });
}
/// Compare all debug fields without skipping a failed case (used by the 448-case aggregate).
pub fn matches(label: &str, actual: &impl Debug) -> bool {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut().expect("trace guard before candidate queries");
        assert!(!state.reference);
        let row = json!({"kind":"row","ordinal":state.ordinal,"label":label,"value":format!("{actual:?}")});
        emit(&mut state.output, &row);
        let ordinal = state.ordinal;
        state.ordinal += 1;
        let expected = state.expected.get(ordinal).expect("candidate emitted surplus reference comparison");
        assert_eq!(expected["label"], label, "reference producer label/order");
        let equal = expected == &row;
        if !equal {
            state.mismatches += 1;
            state.first_mismatch.get_or_insert_with(|| format!("{ordinal}:{label}"));
        }
        equal
    })
}
/// Keep recording later queries after a value difference. The guard still
/// fails this test; no field is ignored and no baseline is changed.
pub fn check(label: &str, actual: &impl Debug) {
    let _ = matches(label, actual);
}
impl Drop for Guard {
    fn drop(&mut self) {
        // Release the thread-local borrow before any fallible serialization or
        // I/O. A prior assertion remains the primary test failure.
        let Some(mut state) = STATE.with(|state| state.borrow_mut().take()) else {
            return;
        };
        let panicking = std::thread::panicking();
        // Complete captured data may still have semantic differences. Keep
        // every real row and let the runner report the independent nonzero exit.
        let complete = !state.output.failed()
            && if state.reference {
                !panicking
            } else {
                state.ordinal == state.expected.len()
            };
        let result = storage::finish(&mut state.output, state.ordinal, complete);
        if panicking {
            // A missing/partial failure terminal is rejected by the reader.
            // Never turn a write error during unwinding into a process abort.
            if let Err(error) = result {
                use std::io::Write;
                let _ = writeln!(std::io::stderr().lock(), "oracle failure terminal: {error}");
            }
            return;
        }
        result.expect("finish bounded oracle evidence");
        assert!(complete, "candidate omitted fixed-engine reference rows");
        assert_eq!(
            state.mismatches, 0,
            "complete fixed-engine value differences; first={:?}; full actual rows are retained",
            state.first_mismatch
        );
    }
}
