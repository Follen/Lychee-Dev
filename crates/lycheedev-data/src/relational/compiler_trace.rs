//! Opt-in unit-test phase markers for native stack-abort diagnosis. Ordinary
//! libtest output capture is lost when the process aborts, so these bounded
//! markers use stderr directly. Production binaries contain none of this code.
use std::{cell::Cell, io::Write};
thread_local! {
    static LABEL: Cell<Option<&'static str>> = const { Cell::new(None) };
}
pub(super) struct Trace(Option<&'static str>);
pub(super) fn enter(label: &'static str) -> Trace {
    Trace(LABEL.replace(Some(label)))
}
impl Drop for Trace {
    fn drop(&mut self) {
        LABEL.set(self.0);
    }
}
pub(super) fn stage(phase: &str) {
    if let Some(label) = LABEL.get() {
        // Diagnostics do not replace the compiler result on an IO failure.
        let _ = writeln!(std::io::stderr().lock(), "SQL compiler {label}: {phase}");
    }
}
