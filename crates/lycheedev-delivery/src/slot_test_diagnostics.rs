//! Test-only spans for the full slot fixture/install/repair boundaries.
//! Counters cover these instrumented phases in this test process, not all
//! libtest threads. No file-level tracing, scheduling gate or control refresh.
#![cfg(test)]
use lycheedev_core::{ToolkitError, control::OperationControl};
use std::{
    io::Write,
    sync::{
        OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};
static ORIGIN: OnceLock<Instant> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(0);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct SlotPhase<'a> {
    phase: &'static str,
    id: u64,
    started: Instant,
    control: Option<&'a OperationControl>,
    outcome: Option<String>,
}
impl<'a> SlotPhase<'a> {
    pub(crate) fn begin(phase: &'static str, control: Option<&'a OperationControl>) -> Self {
        ORIGIN.get_or_init(Instant::now);
        let span = Self {
            phase,
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            started: Instant::now(),
            control,
            outcome: None,
        };
        let active = ACTIVE.fetch_add(1, Ordering::SeqCst) + 1;
        // Construct the guard before emission, so any unwind also returns its
        // counter. A failed diagnostic write never changes the operation result.
        span.record("begin", active);
        span
    }
    pub(crate) fn finish(mut self, error: Option<&ToolkitError>) {
        self.outcome = Some(error.map_or_else(|| "ok".to_owned(), |error| error.code.clone()));
    }
    fn record(&self, event: &str, active: usize) {
        let now = Instant::now();
        let thread = std::thread::current();
        let message = serde_json::json!({
            "schema": "lycheedev.slot-test-phase.v1",
            "event": event,
            "pid": std::process::id(),
            "threadId": format!("{:?}", thread.id()),
            "test": thread.name(),
            "phase": self.phase,
            "id": self.id,
            "sinceOriginMicros": now.saturating_duration_since(*ORIGIN.get().unwrap()).as_micros(),
            "elapsedMicros": now.saturating_duration_since(self.started).as_micros(),
            "remainingMicros": self.control.map(|control| control.deadline().saturating_duration_since(now).as_micros()),
            "activeInstrumentedPhases": active,
            "outcome": self.outcome,
            "unwinding": std::thread::panicking(),
        });
        // Direct stderr preserves successful spans under the existing captured
        // libtest invocation. One short locked write per boundary only.
        let _ = writeln!(std::io::stderr().lock(), "{message}");
    }
}
impl Drop for SlotPhase<'_> {
    fn drop(&mut self) {
        let active = ACTIVE.fetch_sub(1, Ordering::SeqCst) - 1;
        self.record("end", active);
    }
}
