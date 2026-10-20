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
    FactsWorkerWait,
    FactsRefill,
    FactsPublishLock,
    FactsPublishStage,
    FactsPublishWrite,
    FactsPublishSeal,
    FactsPublishRename,
    FactsPublishStageClose,
    FactsPublishLockClose,
    FactsParseSequential,
    FactsWorkerAbandoned,
    FactsPublishLockVerify,
    FactsPublishLockOpen,
    FactsPublishLockKernelAttempt,
    FactsPublishLockContentionBackoff,
    FactsPublishLockFailureClose,
    CachedIndexRead,
    CachedIndexSha256,
    CachedIndexTypedDecode,
    CachedIndexCheckedClose,
}
const NAMES: [&str; 35] = [
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
    "facts_worker_wait",
    "facts_refill_dispatched",
    "facts_publish_lock",
    "facts_publish_stage",
    "facts_publish_write",
    "facts_publish_seal",
    "facts_publish_rename",
    "facts_publish_stage_close",
    "facts_publish_lock_close",
    "facts_parse_sequential",
    "facts_worker_abandoned",
    "facts_publish_lock_verify",
    "facts_publish_lock_open",
    "facts_publish_lock_kernel_attempt",
    "facts_publish_lock_contention_backoff",
    "facts_publish_lock_failure_close",
    "cached_index_read",
    "cached_index_sha256",
    "cached_index_typed_decode",
    "cached_index_checked_close",
];
const CACHED_INDEX_TIMING: &str = "cached_index_read/sha256/typed_decode/checked_close are mutually exclusive scopes nested in cached_index; do not add them to their parent. read counts each actual read attempt, including EOF and failure, not native syscalls. sha256 counts each nonempty-block update and the final digest formatting/comparison only when reached. typed_decode includes all original typed validation. checked_close includes finish_file's consuming close, final control check and error aggregation, not only OS close. Path guards/open, input buffer growth/copies and other checks remain in the parent residual; residual is not disk time. Per-chunk clock/counter overhead with profiling enabled is not assumed negligible. Failed operations report only scopes actually entered, not successful-path counts.";

pub(super) struct Profile {
    started: Option<Instant>,
    elapsed: [Cell<Duration>; NAMES.len()],
    calls: [Cell<u64>; NAMES.len()],
}
impl Profile {
    pub(super) fn new() -> Self {
        Self::enabled(std::env::var_os("LYCHEEDEV_SOURCE_PHASE_PROFILE").is_some_and(|v| v == "1"))
    }
    pub(super) fn enabled(enabled: bool) -> Self {
        Self {
            started: enabled.then(Instant::now),
            elapsed: std::array::from_fn(|_| Cell::new(Duration::ZERO)),
            calls: std::array::from_fn(|_| Cell::new(0)),
        }
    }
    pub(super) fn is_enabled(&self) -> bool {
        self.started.is_some()
    }
    #[cfg(test)]
    pub(super) fn calls(&self, phase: Phase) -> u64 {
        self.calls[phase as usize].get()
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
        serde_json::json!({"schema":"lycheedev.source-index-phases.v2","elapsedMicros":self.started.map(|s|s.elapsed().as_micros()),"timing":"wall-clock; worker phase sums consumed job durations; phases overlap; not additive","phases":phases,"cachedIndexTiming":CACHED_INDEX_TIMING,
            "lockTiming":"verify/open/kernel_attempt/contention_backoff/failure_close are nested in facts_publish_lock; not additive with their parent. kernel_attempt counts every kernel try, including immediate success; contention_backoff measures actual sleep calls only after real contention, including scheduler delay. failure_close measures only failed-acquisition handle close; successful close remains facts_publish_lock_close. Verification includes the unchanged controlled ancestry checks; lock preparation and other checks remain in the parent."})
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
        assert_eq!(value["phases"].as_array().unwrap().len(), NAMES.len());
        assert_eq!(
            value["timing"],
            "wall-clock; worker phase sums consumed job durations; phases overlap; not additive"
        );
    }
    #[test]
    fn cached_subphases_disabled_keep_all_clocks_and_counters_empty() {
        let p = Profile::enabled(false);
        for phase in [
            Phase::CachedIndexRead,
            Phase::CachedIndexSha256,
            Phase::CachedIndexTypedDecode,
            Phase::CachedIndexCheckedClose,
        ] {
            let span = p.span(phase);
            assert!(span.started.is_none());
            drop(span);
            assert_eq!(p.calls(phase), 0);
            assert_eq!(p.elapsed[phase as usize].get(), Duration::ZERO);
        }
        assert!(p.started.is_none());
    }
    #[test]
    fn cached_subphases_preserve_failure_and_only_record_entered_scopes() {
        for failure in [Phase::CachedIndexRead, Phase::CachedIndexTypedDecode] {
            let p = Profile::enabled(true);
            let parent = p.span(Phase::CachedIndex);
            let result: Result<(), &str> = (|| {
                {
                    let _read = p.span(Phase::CachedIndexRead);
                    if matches!(failure, Phase::CachedIndexRead) {
                        return Err("original read failure");
                    }
                }
                {
                    let _sha = p.span(Phase::CachedIndexSha256);
                }
                let _decode = p.span(Phase::CachedIndexTypedDecode);
                Err("original decode failure")
            })();
            // Scope observation does not transform the result. This is a
            // profiler RAII test, not a simulated filesystem close result.
            let returned = {
                let _close = p.span(Phase::CachedIndexCheckedClose);
                result
            };
            drop(parent);
            let read_failed = matches!(failure, Phase::CachedIndexRead);
            assert_eq!(
                returned,
                Err(if read_failed {
                    "original read failure"
                } else {
                    "original decode failure"
                })
            );
            assert_eq!(p.calls(Phase::CachedIndexRead), 1);
            assert_eq!(p.calls(Phase::CachedIndexSha256), u64::from(!read_failed));
            assert_eq!(
                p.calls(Phase::CachedIndexTypedDecode),
                u64::from(!read_failed)
            );
            assert_eq!(p.calls(Phase::CachedIndexCheckedClose), 1);
            let children: Duration = p.elapsed[31..].iter().map(Cell::get).sum();
            assert!(children <= p.elapsed[Phase::CachedIndex as usize].get());
        }
    }
    #[test]
    fn cached_subphases_append_without_relabeling_the_original_thirty_one() {
        assert_eq!(
            &NAMES[..31],
            &[
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
                "facts_worker_wait",
                "facts_refill_dispatched",
                "facts_publish_lock",
                "facts_publish_stage",
                "facts_publish_write",
                "facts_publish_seal",
                "facts_publish_rename",
                "facts_publish_stage_close",
                "facts_publish_lock_close",
                "facts_parse_sequential",
                "facts_worker_abandoned",
                "facts_publish_lock_verify",
                "facts_publish_lock_open",
                "facts_publish_lock_kernel_attempt",
                "facts_publish_lock_contention_backoff",
                "facts_publish_lock_failure_close",
            ]
        );
        assert_eq!(
            &NAMES[31..],
            &[
                "cached_index_read",
                "cached_index_sha256",
                "cached_index_typed_decode",
                "cached_index_checked_close",
            ]
        );
        for enabled in [false, true] {
            let p = Profile::enabled(enabled);
            p.record(
                Phase::FactsPublishLockKernelAttempt,
                Duration::from_micros(7),
            );
            p.record(
                Phase::FactsPublishLockKernelAttempt,
                Duration::from_micros(11),
            );
            p.record(
                Phase::FactsPublishLockContentionBackoff,
                Duration::from_micros(13),
            );
            let value = p.value();
            let phases = value["phases"].as_array().unwrap();
            assert_eq!(phases.len(), 35);
            assert_eq!(phases[28]["phase"], "facts_publish_lock_kernel_attempt");
            assert_eq!(phases[28]["calls"], if enabled { 2 } else { 0 });
            assert_eq!(phases[28]["elapsedMicros"], if enabled { 18 } else { 0 });
            assert_eq!(phases[29]["calls"], if enabled { 1 } else { 0 });
            assert_eq!(phases[29]["elapsedMicros"], if enabled { 13 } else { 0 });
            // A successful close and a failure close are different events;
            // merely recording attempts does not invent either one.
            assert_eq!(phases[23]["calls"], 0);
            assert_eq!(phases[30]["calls"], 0);
            assert!(
                value["lockTiming"]
                    .as_str()
                    .unwrap()
                    .contains("nested in facts_publish_lock; not additive")
            );
        }
    }
    #[test]
    fn real_facts_publication_keeps_original_phases_and_reports_zero_cached_work() {
        use lycheedev_core::{
            cache::SourceFactsCache, control::OperationControl, storage, workspace,
        };
        struct Home(std::path::PathBuf);
        impl Drop for Home {
            fn drop(&mut self) {
                let result = std::fs::remove_dir_all(&self.0);
                if !std::thread::panicking() {
                    result.unwrap();
                }
            }
        }
        let home = Home(std::env::temp_dir().join(format!(
            "lychee-facts-profile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        )));
        workspace::initialize(&home.0).unwrap();
        let control = OperationControl::default();
        let owner = SourceFactsCache::open(&home.0, &control).unwrap();
        let p = Profile::enabled(true);
        let raw = b"function CompleteFacts() return 1 end";
        let digest = storage::sha256(raw);
        let result = super::super::document_facts_profiled(
            &home.0,
            "Example.lua",
            raw,
            &digest,
            &control,
            Some(&p),
            &owner,
            None,
        );
        let (_, hit) = owner.finish(result, &control).unwrap();
        assert!(!hit);
        let value = p.value();
        assert_eq!(value["schema"], "lycheedev.source-index-phases.v2");
        assert_eq!(value["cachedIndexTiming"], CACHED_INDEX_TIMING);
        let phases = value["phases"].as_array().unwrap();
        assert_eq!(phases.len(), 35);
        for (i, phase) in phases.iter().enumerate() {
            assert_eq!(phase["phase"], NAMES[i]);
            if NAMES[i].starts_with("facts_publish_lock") || NAMES[i].starts_with("cached_index") {
                assert_eq!(phase["calls"], 0, "unexpected lock event: {}", NAMES[i]);
                assert_eq!(
                    phase["elapsedMicros"], 0,
                    "invented lock timing: {}",
                    NAMES[i]
                );
            }
        }
        for phase in [
            Phase::FactsPublish,
            Phase::FactsPublishStage,
            Phase::FactsPublishWrite,
            Phase::FactsPublishSeal,
            Phase::FactsPublishRename,
            Phase::FactsPublishStageClose,
        ] {
            assert_eq!(p.calls(phase), 1);
        }
        assert_eq!(std::fs::read_dir(home.0.join("locks")).unwrap().count(), 0);
        // Publication is already readable as a complete authenticated envelope
        // when the per-document caller returns; no worker/batch flush is needed.
        let (_, hit) =
            super::super::document_facts(&home.0, "Example.lua", raw, &digest, &control).unwrap();
        assert!(hit);
    }
}
