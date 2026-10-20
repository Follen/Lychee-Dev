//! Opt-in query wall-clock observations, separate from accounting and results.
//! A query and its safety-only views own one recorder. Spans only borrow it;
//! existing workers can share it without retaining any per-span allocation.
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub(crate) enum Phase {
    Admission,
    Prepare,
    Payload,
    TextureDecode,
    PixelTransformAndHash,
    Encode,
    SourceCapture,
    ArtifactCapture,
    ManifestCapture,
    Output,
    SourceClose,
    EncodedIdentityHash,
    DecodedCacheOpen,
    DecodedCacheLookup,
    DecodedCacheVerify,
    DecodedStreamPublish,
    EncodedChunkRead,
    EncodedChunkMd5,
    ZlibInflate,
    WholeContentKeyHash,
    DecodedCacheCommit,
    PublisherSourceSpool,
    PublisherShardCopy,
    PublisherFinalVerify,
    PublisherStageCreate,
    PublisherPrivateSpoolReadReady,
    PublisherFinalStageSeal,
    PublisherDigestLock,
    PublisherShardOpen,
    PublisherRename,
    PublisherWinnerOpen,
    PublisherCheckedClose,
    SqlRequest,
    SqlExecute,
    SqlSourcePrepare,
    SqlSourceVerify,
    SqlResult,
    SqlSerialize,
    SqlCapture,
    GenerationOpen,
    GenerationQuota,
    GenerationManifest,
    GenerationObject,
    GenerationDirectory,
    GenerationBatch,
    GenerationRow,
    GenerationPrepare,
    GenerationImportEncode,
    GenerationObjectCommit,
    GenerationManifestPublish,
}
const NAMES: [&str; 11] = [
    "admission",
    "prepare_inclusive",
    "payload_inclusive",
    "texture_decode",
    "pixel_transform_and_hash",
    "encode",
    "source_capture_metadata_inclusive",
    "artifact_capture_metadata_inclusive",
    "manifest_capture_metadata_inclusive",
    "output",
    "source_close",
];
// Inclusive details are intentionally separate from the original eleven
// scopes. In particular publish contains chunk reads/inflate/CKey work, and
// prepare and payload can both call the decoded cache. Never add these to the
// original phases, or interpret their remainder as physical disk time.
const DETAIL_NAMES: [&str; 10] = [
    "encoded_identity_hash",
    "decoded_cache_open",
    "decoded_cache_lookup",
    "decoded_cache_verify_inclusive",
    "decoded_stream_publish_inclusive",
    "encoded_chunk_read",
    "encoded_chunk_md5",
    "zlib_inflate",
    "whole_content_key_hash",
    "decoded_cache_commit",
];
// Exact Core publisher operations, nested inside stream publish. SourceSpool
// contains the existing decoder details; failures can include nested cleanup.
// Private-spool read readiness and final-stage durable sealing have distinct
// counters. These are a separate diagnostic level, never extra command time.
const PUBLISHER_NAMES: [&str; 11] = [
    "source_to_spool_inclusive",
    "spool_to_shard_inclusive",
    "final_verify_inclusive",
    "stage_create",
    "private_spool_read_ready",
    "final_stage_seal",
    "digest_lock",
    "shard_open",
    "rename",
    "winner_open",
    "checked_close",
];
// SQL scopes nest within execution/source preparation. Cursor advance includes
// batch decode; each call, including EOF/error, is observed only when enabled.
const SQL_NAMES: [&str; 18] = [
    "request_compile_admit",
    "execute_resolved_inclusive",
    "source_prepare_inclusive",
    "source_verify_inclusive",
    "result_projection",
    "capture_serialize",
    "capture_publish_metadata_inclusive",
    "generation_open_inclusive",
    "generation_read_admission",
    "generation_manifest_authenticate",
    "generation_object_open_authenticate_inclusive",
    "generation_directory_validate_inclusive",
    "generation_cursor_batch_decode",
    "generation_cursor_advance_inclusive",
    "generation_prepare_inclusive",
    "generation_import_encode_inclusive",
    "generation_object_commit",
    "generation_manifest_publish",
];
const COUNTERS: usize = NAMES.len() + DETAIL_NAMES.len() + PUBLISHER_NAMES.len() + SQL_NAMES.len();
const TIMING: &str = "wall-clock; phases may nest or overlap across workers; not additive; budget lifetime and phases are diagnostics, not accepted command total or CPU time";

#[derive(Clone, Copy, Default)]
struct Counter {
    elapsed: Duration,
    calls: u64,
}

pub(crate) struct Profile {
    started: Instant,
    counters: Mutex<[Counter; COUNTERS]>,
    #[cfg(test)]
    output: Option<TestOutput>,
}

impl Profile {
    pub(crate) fn from_env() -> Option<Arc<Self>> {
        Self::enabled(
            std::env::var_os("LYCHEEDEV_DATA_PHASE_PROFILE").is_some_and(|v| v == "1"),
            Instant::now,
        )
    }

    fn enabled(enabled: bool, now: impl FnOnce() -> Instant) -> Option<Arc<Self>> {
        // Do not allocate a recorder or read a clock on the disabled path.
        enabled.then(|| {
            Arc::new(Self {
                started: now(),
                counters: Mutex::new([Counter::default(); COUNTERS]),
                #[cfg(test)]
                output: None,
            })
        })
    }

    fn record(&self, phase: Phase, elapsed: Duration) {
        // A diagnostic lock must never become a business failure, including
        // during error unwinding. Fixed counters keep memory bounded.
        let mut counters = self.counters.lock().unwrap_or_else(|p| p.into_inner());
        let counter = &mut counters[phase as usize];
        counter.elapsed = counter.elapsed.saturating_add(elapsed);
        counter.calls = counter.calls.saturating_add(1);
    }

    fn value(&self) -> serde_json::Value {
        let counters = *self.counters.lock().unwrap_or_else(|p| p.into_inner());
        let phases: Vec<_> = NAMES
            .iter()
            .zip(counters)
            .map(|(name, counter)| {
                serde_json::json!({
                    "phase":name,
                    "calls":counter.calls,
                    "elapsedMicros":counter.elapsed.as_micros(),
                })
            })
            .collect();
        let details: Vec<_> = DETAIL_NAMES
            .iter()
            .zip(counters[NAMES.len()..].iter())
            .map(|(name, counter)| {
                serde_json::json!({
                    "phase":name,
                    "calls":counter.calls,
                    "elapsedMicros":counter.elapsed.as_micros(),
                })
            })
            .collect();
        let publisher: Vec<_> = PUBLISHER_NAMES
            .iter()
            .zip(counters[NAMES.len() + DETAIL_NAMES.len()..].iter())
            .map(|(name, counter)| {
                serde_json::json!({
                    "phase":name,
                    "calls":counter.calls,
                    "elapsedMicros":counter.elapsed.as_micros(),
                })
            })
            .collect();
        let sql: Vec<_> = SQL_NAMES.iter()
            .zip(counters[NAMES.len() + DETAIL_NAMES.len() + PUBLISHER_NAMES.len()..].iter())
            .map(|(name, counter)| serde_json::json!({
                "phase":name, "calls":counter.calls,
                "elapsedMicros":counter.elapsed.as_micros(),
            })).collect();
        serde_json::json!({
            "schema":"lycheedev.data-query-phases.v5",
            "elapsedMicros":self.started.elapsed().as_micros(),
            "timing":TIMING,
            "captureTiming":"capture_metadata_inclusive includes capture and metadata work; metadata open/write/close are not separately observed",
            "phases":phases,
            "details":details,
            "publisher":publisher,
            "sql":sql,
        })
    }

    #[cfg(test)]
    pub(crate) fn observed() -> (Arc<Self>, TestOutput) {
        let output = Arc::new(Mutex::new(Vec::new()));
        let profile = Arc::new(Self {
            started: Instant::now(),
            counters: Mutex::new([Counter::default(); COUNTERS]),
            output: Some(output.clone()),
        });
        (profile, output)
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let value = self.value();
        #[cfg(test)]
        if let Some(output) = &self.output {
            output.lock().unwrap_or_else(|p| p.into_inner()).push(value);
            return;
        }
        // The last shared owner emits once, even after a failed operation.
        // A closed stderr never replaces the original query result.
        use std::io::Write;
        let _ = writeln!(std::io::stderr().lock(), "{value}");
    }
}

pub(crate) struct Span<'a> {
    active: Option<ActiveSpan<'a>>,
}
struct ActiveSpan<'a> {
    profile: &'a Profile,
    phase: Phase,
    started: Instant,
}
impl<'a> Span<'a> {
    pub(crate) fn new(profile: Option<&'a Profile>, phase: Phase) -> Self {
        Self::with_clock(profile, phase, Instant::now)
    }

    fn with_clock(
        profile: Option<&'a Profile>,
        phase: Phase,
        now: impl FnOnce() -> Instant,
    ) -> Self {
        Self {
            active: profile.map(|profile| ActiveSpan {
                profile,
                phase,
                started: now(),
            }),
        }
    }

    fn finish(&mut self, elapsed: impl FnOnce(Instant) -> Duration) {
        if let Some(active) = self.active.take() {
            active.profile.record(active.phase, elapsed(active.started));
        }
    }
}
impl Drop for Span<'_> {
    fn drop(&mut self) {
        self.finish(|started| started.elapsed());
    }
}

#[cfg(test)]
pub(crate) type TestOutput = Arc<Mutex<Vec<serde_json::Value>>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_counter_indices_are_separate_and_exact() {
        let (profile, output) = Profile::observed();
        for (index, phase) in [
            Phase::SqlRequest, Phase::SqlExecute, Phase::SqlSourcePrepare,
            Phase::SqlSourceVerify, Phase::SqlResult, Phase::SqlSerialize,
            Phase::SqlCapture, Phase::GenerationOpen, Phase::GenerationQuota,
            Phase::GenerationManifest, Phase::GenerationObject, Phase::GenerationDirectory,
            Phase::GenerationBatch, Phase::GenerationRow, Phase::GenerationPrepare,
            Phase::GenerationImportEncode, Phase::GenerationObjectCommit, Phase::GenerationManifestPublish,
        ].into_iter().enumerate() {
            let mut span = Span::new(Some(&profile), phase);
            span.finish(|_| Duration::from_micros(index as u64 + 1));
        }
        let value = profile.value();
        assert_eq!(value["sql"].as_array().unwrap().len(), SQL_NAMES.len());
        for (index, phase) in value["sql"].as_array().unwrap().iter().enumerate() {
            assert_eq!(phase["phase"], SQL_NAMES[index]);
            assert_eq!(phase["calls"], 1);
            assert_eq!(phase["elapsedMicros"], index + 1);
        }
        for field in ["phases", "details", "publisher"] {
            assert!(value[field].as_array().unwrap().iter().all(|p| p["calls"] == 0));
        }
        drop(profile); assert_eq!(output.lock().unwrap().len(), 1);
    }

    #[test]
    fn disabled_observation_has_no_clock_recorder_or_counts() {
        let profile = Profile::enabled(false, || panic!("disabled creation read a clock"));
        assert!(profile.is_none());
        let mut span = Span::with_clock(profile.as_deref(), Phase::Payload, || {
            panic!("disabled span read a clock")
        });
        assert!(span.active.is_none());
        span.finish(|_| panic!("disabled span read elapsed time"));
        drop(span);
        assert!(profile.is_none(), "disabled scopes never create counters");
    }

    #[test]
    fn repeated_and_failed_scopes_preserve_the_original_result() {
        let (profile, output) = Profile::observed();
        let result: Result<(), &'static str> = (|| {
            let _span = Span::new(Some(&profile), Phase::Payload);
            Err("original")
        })();
        {
            let _span = Span::new(Some(&profile), Phase::Payload);
        }
        assert_eq!(result, Err("original"));
        assert_eq!(profile.value()["phases"][2]["calls"], 2);
        assert!(output.lock().unwrap().is_empty());
        drop(profile);
        let output = output.lock().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["phases"].as_array().unwrap().len(), NAMES.len());
        assert_eq!(output[0]["timing"], TIMING);
    }

    #[test]
    fn borrowed_spans_do_not_add_output_owners_or_duplicate_finished_counts() {
        let (profile, output) = Profile::observed();
        let mut span = Span::new(Some(&profile), Phase::Encode);
        assert_eq!(Arc::strong_count(&profile), 1);
        span.finish(|_| Duration::from_micros(17));
        span.finish(|_| panic!("a finished span must not read another clock"));
        drop(span);
        assert_eq!(profile.value()["phases"][5]["calls"], 1);
        assert_eq!(profile.value()["phases"][5]["elapsedMicros"], 17);
        drop(profile);
        assert_eq!(output.lock().unwrap().len(), 1);
    }

    #[test]
    fn detail_counters_are_separate_from_the_original_eleven_scopes() {
        let (profile, output) = Profile::observed();
        let mut span = Span::new(Some(&profile), Phase::ZlibInflate);
        span.finish(|_| Duration::from_micros(23));
        let value = profile.value();
        assert_eq!(value["schema"], "lycheedev.data-query-phases.v5");
        assert_eq!(value["phases"].as_array().unwrap().len(), 11);
        assert!(
            value["phases"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["calls"] == 0)
        );
        assert_eq!(value["details"][7]["phase"], "zlib_inflate");
        assert_eq!(value["details"][7]["calls"], 1);
        assert_eq!(value["details"][7]["elapsedMicros"], 23);
        drop(span);
        drop(profile);
        assert_eq!(output.lock().unwrap().len(), 1);
    }

    #[test]
    fn publisher_observation_is_disabled_without_clock_and_keeps_three_levels_separate() {
        let mut disabled = Span::with_clock(None, Phase::PublisherSourceSpool, || {
            panic!("disabled publisher read a clock")
        });
        disabled.finish(|_| panic!("disabled publisher read elapsed time"));
        let (profile, output) = Profile::observed();
        let mut publisher = Span::new(Some(&profile), Phase::PublisherSourceSpool);
        let mut decoder = Span::new(Some(&profile), Phase::ZlibInflate);
        decoder.finish(|_| Duration::from_micros(7));
        publisher.finish(|_| Duration::from_micros(19));
        let value = profile.value();
        assert_eq!(value["phases"].as_array().unwrap().len(), 11);
        assert_eq!(value["details"].as_array().unwrap().len(), 10);
        assert_eq!(value["publisher"].as_array().unwrap().len(), 11);
        assert_eq!(value["details"][7]["elapsedMicros"], 7);
        assert_eq!(value["publisher"][0]["phase"], "source_to_spool_inclusive");
        assert_eq!(value["publisher"][0]["calls"], 1);
        assert_eq!(value["publisher"][0]["elapsedMicros"], 19);
        assert!(value["phases"].as_array().unwrap().iter().all(|p| p["calls"] == 0));
        drop(decoder);
        drop(publisher);
        drop(profile);
        assert_eq!(output.lock().unwrap().len(), 1);
    }

    #[test]
    fn private_read_ready_and_final_seal_are_disabled_without_clocks() {
        let profile = Profile::enabled(false, || panic!("disabled creation read a clock"));
        for phase in [
            Phase::PublisherPrivateSpoolReadReady,
            Phase::PublisherFinalStageSeal,
        ] {
            let mut span = Span::with_clock(profile.as_deref(), phase, || {
                panic!("disabled seal span read a clock")
            });
            assert!(span.active.is_none());
            span.finish(|_| panic!("disabled seal span read elapsed time"));
            drop(span);
        }
        assert!(profile.is_none());
    }

    #[test]
    fn private_read_ready_and_final_seal_keep_separate_durations_and_counts() {
        let (profile, output) = Profile::observed();
        for (phase, elapsed) in [
            (Phase::PublisherPrivateSpoolReadReady, 3),
            (Phase::PublisherFinalStageSeal, 29),
            (Phase::PublisherPrivateSpoolReadReady, 5),
        ] {
            let mut span = Span::new(Some(&profile), phase);
            span.finish(|_| Duration::from_micros(elapsed));
            span.finish(|_| panic!("finished seal span read another clock"));
        }
        let value = profile.value();
        assert_eq!(value["schema"], "lycheedev.data-query-phases.v5");
        assert_eq!(value["timing"], TIMING);
        assert_eq!(value["phases"].as_array().unwrap().len(), 11);
        assert_eq!(value["details"].as_array().unwrap().len(), 10);
        assert_eq!(value["publisher"].as_array().unwrap().len(), 11);
        assert_eq!(value["publisher"][4]["phase"], "private_spool_read_ready");
        assert_eq!(value["publisher"][4]["calls"], 2);
        assert_eq!(value["publisher"][4]["elapsedMicros"], 8);
        assert_eq!(value["publisher"][5]["phase"], "final_stage_seal");
        assert_eq!(value["publisher"][5]["calls"], 1);
        assert_eq!(value["publisher"][5]["elapsedMicros"], 29);
        for key in ["phases", "details"] {
            assert!(value[key].as_array().unwrap().iter().all(|p| {
                p["calls"] == 0 && p["elapsedMicros"] == 0
            }));
        }
        assert!(value["publisher"].as_array().unwrap().iter().enumerate().all(
            |(index, p)| matches!(index, 4 | 5) || (p["calls"] == 0 && p["elapsedMicros"] == 0)
        ));
        drop(profile);
        let output = output.lock().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["publisher"], value["publisher"]);
    }

    #[test]
    fn existing_workers_can_share_bounded_counters_without_losing_spans() {
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Profile>();
        send_sync::<crate::resource::Budget>();
        let (profile, output) = Profile::observed();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let profile = &profile;
                scope.spawn(move || {
                    for _ in 0..64 {
                        let _span = Span::new(Some(profile), Phase::TextureDecode);
                    }
                });
            }
        });
        assert_eq!(profile.value()["phases"][3]["calls"], 256);
        drop(profile);
        assert_eq!(output.lock().unwrap().len(), 1);
    }

    #[test]
    fn counters_saturate_without_touching_another_phase() {
        let (profile, _) = Profile::observed();
        {
            let mut counters = profile.counters.lock().unwrap();
            counters[Phase::SourceCapture as usize] = Counter {
                elapsed: Duration::MAX,
                calls: u64::MAX,
            };
        }
        profile.record(Phase::SourceCapture, Duration::from_nanos(1));
        let counters = profile.counters.lock().unwrap();
        assert_eq!(
            counters[Phase::SourceCapture as usize].elapsed,
            Duration::MAX
        );
        assert_eq!(counters[Phase::SourceCapture as usize].calls, u64::MAX);
        assert_eq!(
            counters[Phase::ArtifactCapture as usize].elapsed,
            Duration::ZERO
        );
        assert_eq!(counters[Phase::ArtifactCapture as usize].calls, 0);
    }

    #[test]
    fn capture_names_keep_metadata_inclusive_and_all_phases_fixed() {
        let (profile, _) = Profile::observed();
        for phase in [
            Phase::SourceCapture,
            Phase::ArtifactCapture,
            Phase::ManifestCapture,
        ] {
            let _span = Span::new(Some(&profile), phase);
            assert!(NAMES[phase as usize].ends_with("_capture_metadata_inclusive"));
        }
        let value = profile.value();
        assert_eq!(value["phases"].as_array().unwrap().len(), 11);
        assert_eq!(value["phases"][6]["calls"], 1);
        assert_eq!(value["phases"][7]["calls"], 1);
        assert_eq!(value["phases"][8]["calls"], 1);
    }
}
