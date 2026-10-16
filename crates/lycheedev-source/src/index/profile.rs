//! Optional aggregate wall-clock observations. Never part of public results or cache identity.
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub(super) enum Phase {
    Metadata,
    CachedIndex,
    IndexLock,
    GitTree,
    GitBlobs,
    FactsRead,
    FactsParse,
    FactsSerialize,
    FactsPublish,
    OriginalPublish,
    Symbols,
    IndexSerialize,
    IndexPublish,
    Manifest,
    FactsWorker,
}
const NAMES: [&str; 15] = [
    "metadata",
    "cached_index",
    "index_lock",
    "git_tree",
    "git_blobs_inclusive",
    "facts_read",
    "facts_parse",
    "facts_serialize",
    "facts_publish",
    "original_publish",
    "symbols",
    "index_serialize",
    "index_publish",
    "manifest",
    "facts_worker_consumed",
];

pub(super) struct Profile {
    started: Option<Instant>,
    elapsed: [Cell<Duration>; 15],
    calls: [Cell<u64>; 15],
}
impl Profile {
    pub(super) fn new() -> Self {
        Self::enabled(std::env::var_os("LYCHEEDEV_SOURCE_PHASE_PROFILE").is_some_and(|v| v == "1"))
    }
    fn enabled(enabled: bool) -> Self {
        Self {
            started: enabled.then(Instant::now),
            elapsed: std::array::from_fn(|_| Cell::new(Duration::ZERO)),
            calls: std::array::from_fn(|_| Cell::new(0)),
        }
    }
    pub(super) fn is_enabled(&self) -> bool {
        self.started.is_some()
    }
    pub(super) fn record(&self, phase: Phase, elapsed: Duration) {
        if self.is_enabled() {
            let i = phase as usize;
            self.elapsed[i].set(self.elapsed[i].get().saturating_add(elapsed));
            self.calls[i].set(self.calls[i].get().saturating_add(1));
        }
    }
    pub(super) fn span(&self, phase: Phase) -> Span<'_> {
        Span {
            profile: self,
            phase,
            started: self.started.map(|_| Instant::now()),
        }
    }
    fn value(&self) -> serde_json::Value {
        let phases: Vec<_> = NAMES.iter().enumerate().map(|(i, name)| serde_json::json!({"phase":name,"calls":self.calls[i].get(),"elapsedMicros":self.elapsed[i].get().as_micros()})).collect();
        serde_json::json!({"schema":"lycheedev.source-index-phases.v1","elapsedMicros":self.started.map(|s|s.elapsed().as_micros()),"timing":"wall-clock; worker phase sums consumed job durations; phases overlap; not additive","phases":phases})
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        if self.started.is_some() {
            // Diagnostic output is best-effort, including on a failed operation.
            use std::io::Write;
            let _ = writeln!(std::io::stderr().lock(), "{}", self.value());
        }
    }
}
pub(super) struct Span<'a> {
    profile: &'a Profile,
    phase: Phase,
    started: Option<Instant>,
}
impl Drop for Span<'_> {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            let i = self.phase as usize;
            self.profile.elapsed[i].set(
                self.profile.elapsed[i]
                    .get()
                    .saturating_add(started.elapsed()),
            );
            self.profile.calls[i].set(self.profile.calls[i].get().saturating_add(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_observation_has_no_clock_or_counts() {
        let p = Profile::enabled(false);
        {
            let _span = p.span(Phase::FactsParse);
        }
        assert!(p.started.is_none());
        assert_eq!(p.calls[Phase::FactsParse as usize].get(), 0);
        assert_eq!(p.elapsed[Phase::FactsParse as usize].get(), Duration::ZERO);
    }
    #[test]
    fn repeated_and_failed_scopes_are_observed_without_changing_result() {
        let p = Profile::enabled(true);
        let result: Result<(), &'static str> = (|| {
            let _span = p.span(Phase::FactsParse);
            Err("original")
        })();
        {
            let _span = p.span(Phase::FactsParse);
        }
        assert_eq!(result, Err("original"));
        assert_eq!(p.calls[Phase::FactsParse as usize].get(), 2);
        let value = p.value();
        assert_eq!(value["phases"].as_array().unwrap().len(), 15);
        assert_eq!(
            value["timing"],
            "wall-clock; worker phase sums consumed job durations; phases overlap; not additive"
        );
    }
}
