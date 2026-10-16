//! Test-only bridge between the fixed historical engine and the clean dependency.
//! Every reference row is evaluated by the historical engine, not by OwnedQuery.
//! Missing, truncated, reordered, or surplus reference rows are hard failures.
#![allow(dead_code)]
use serde_json::{Value, json};
use std::{cell::RefCell, fmt::Debug, fs, io::Write, path::PathBuf};

const MAX_TRACE_BYTES: u64 = 16 << 20;
const SCHEMA: &str = "lycheedev.emmy-fixed-oracle.v1";
thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }
struct State {
    reference: bool,
    expected: Vec<Value>,
    ordinal: usize,
    output: fs::File,
}
pub struct Guard;
fn file_name() -> String {
    let thread = std::thread::current();
    let name = thread.name().expect("named Rust test thread");
    assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
    format!("{}--{name}.jsonl", env!("CARGO_CRATE_NAME"))
}
fn emit(output: &mut fs::File, value: &Value) {
    serde_json::to_writer(&mut *output, value).expect("write oracle evidence");
    output.write_all(b"\n").expect("write oracle newline");
    output.flush().expect("flush oracle evidence");
    assert!(
        output.metadata().unwrap().len() <= MAX_TRACE_BYTES,
        "trace evidence exceeds existing per-log cap"
    );
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
        let path = PathBuf::from(directory).join(&name);
        assert!(
            fs::metadata(&path).unwrap().len() <= MAX_TRACE_BYTES,
            "reference evidence byte cap"
        );
        let text = fs::read_to_string(path)
            .expect("reference evidence must exist; never substitute owned output");
        let mut frames: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
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
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(PathBuf::from(directory).join(name))
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
        let expected = state.expected.get(state.ordinal).expect("candidate emitted surplus reference comparison");
        assert_eq!(expected["label"], label, "reference producer label/order");
        state.ordinal += 1;
        expected == &row
    })
}
pub fn check(label: &str, actual: &impl Debug) {
    assert!(
        matches(label, actual),
        "complete fixed-engine value mismatch at {label}: {actual:?}"
    );
}
impl Drop for Guard {
    fn drop(&mut self) {
        STATE.with(|state| {
            let Some(mut state) = state.borrow_mut().take() else {
                return;
            };
            let complete = !std::thread::panicking()
                && (state.reference || state.ordinal == state.expected.len());
            emit(
                &mut state.output,
                &json!({"kind":"end","rows":state.ordinal,"complete":complete}),
            );
            if !std::thread::panicking() {
                assert!(complete, "candidate omitted fixed-engine reference rows");
            }
        });
    }
}
