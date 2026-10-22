//! A bounded, typed cold-index pipeline. Only pure public source observations may
//! run ahead; authentication, cache I/O, controls and publication stay ordered.
use super::profile::{Phase, Profile};
use crate::{
    facts::{Facts, ParseMode},
    git,
};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, metadata::owned_write::finish_owned,
    selection::SourcePin,
};
use std::{
    collections::VecDeque,
    path::Path,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};

// Queue backpressure is independent of the logical CPU count.
const MAX_PENDING_SLOTS: usize = 8;
const MAX_SOURCE_BYTES: usize = crate::facts::WORKER_SOURCE_BYTES;
const MAX_FACT_BYTES: usize = 4 << 20;
// This reservation is for owned parser temporaries, NOT tree-sitter's opaque C
// allocator. Fixed input/worker bounds limit that extra transient; real process
// peak memory remains a measured acceptance requirement, not a claimed RSS cap.
const PARSER_SCRATCH_BYTES: usize = crate::facts::LUA_WORKER_SCRATCH_BYTES;
const WORKER_STACK_BYTES: usize = 2 << 20;
// This is the existing original-source retention admission, not a whole-index
// RSS limit. Existing coordinator Facts/Symbols/JSON allocations are unchanged;
// speculative owned storage shares the remaining original-source allowance.
const SOURCE_RETAINED_LIMIT: usize = 256 << 20;
#[cfg(test)]
const GIT_BATCH_BYTES: usize = (64 << 20) + 4096 * 128;
const JOB_METADATA_BYTES: usize = 8192;

/// Moving the original bytes through one slot authenticates the result's owner;
/// no source snapshot clone, mutable shared parser or speculative cache handle.
struct ParseInput {
    mode: ParseMode,
    data: Vec<u8>,
}
struct ParseOutput {
    mode: ParseMode,
    data: Vec<u8>,
    facts: Option<Facts>,
    parse_elapsed: Duration,
}
struct Worker {
    input: SyncSender<ParseInput>,
    output: Receiver<ParseOutput>,
}
enum Preparation {
    ParseWorker(usize),
    Sequential(Vec<u8>),
}
struct Pending {
    file: git::TreeFile,
    preparation: Preparation,
    reservation: usize,
}
/// A received result remains charged until its ordered consumer returns. Only
/// its finished parser scratch may be released before then; Facts and source
/// bytes must coexist with any newly admitted job.
struct Ready {
    file: git::TreeFile,
    data: Vec<u8>,
    prepared: Option<PreparedFacts>,
    reservation: usize,
}

/// None preserves the exact complete parser path. A worker cap is never a new
/// public syntax diagnostic, truncation, or operation-budget failure.
pub(super) struct PreparedFacts {
    pub(super) mode: ParseMode,
    pub(super) facts: Facts,
    pub(super) parse_elapsed: Duration,
}

pub(super) fn eligible(file: &git::TreeFile, data: &[u8]) -> bool {
    !file.asset
        && !file.oversized
        && file.bytes <= MAX_SOURCE_BYTES
        && data.len() <= MAX_SOURCE_BYTES
        && file
            .path
            .as_bytes()
            .get(file.path.len().saturating_sub(4)..)
            .is_some_and(|suffix| {
                [b".lua", b".xml", b".toc"]
                    .iter()
                    .any(|extension| suffix.eq_ignore_ascii_case(*extension))
            })
        && std::str::from_utf8(data).is_ok()
}

/// Cache admission never queues decoded Facts. A possible hit is consumed only
/// after all earlier documents; a cohort boundary first drains and retries.
pub(super) enum Admission {
    Parse,
    Barrier,
    Drain,
}
pub(super) trait Consumer {
    fn admit(&mut self, _file: &git::TreeFile, _data: &[u8], _ordered: bool) -> Admission {
        Admission::Parse
    }
    fn consume(
        &mut self,
        file: &git::TreeFile,
        data: Vec<u8>,
        prepared: Option<PreparedFacts>,
    ) -> Result<()>;
}
impl<F> Consumer for F
where
    F: FnMut(&git::TreeFile, Vec<u8>, Option<PreparedFacts>) -> Result<()>,
{
    fn consume(
        &mut self,
        file: &git::TreeFile,
        data: Vec<u8>,
        prepared: Option<PreparedFacts>,
    ) -> Result<()> {
        self(file, data, prepared)
    }
}

fn reservation(data: &Vec<u8>) -> Option<usize> {
    reservation_for_capacity(data.capacity())
}

fn reservation_for_capacity(capacity: usize) -> Option<usize> {
    capacity
        .checked_add(MAX_FACT_BYTES)?
        .checked_add(PARSER_SCRATCH_BYTES)?
        .checked_add(JOB_METADATA_BYTES)
}

fn fits(
    retained: usize,
    pending: usize,
    next: usize,
    workers: usize,
    incoming: usize,
    batch_bytes: usize,
) -> bool {
    retained
        .checked_add(batch_bytes)
        .and_then(|n| n.checked_add(incoming))
        .and_then(|n| n.checked_add(workers.checked_mul(WORKER_STACK_BYTES)?))
        .and_then(|n| n.checked_add(pending))
        .and_then(|n| n.checked_add(next))
        .is_some_and(|n| n <= SOURCE_RETAINED_LIMIT)
}

/// Select before creating any threads, after the real Git allocation is known.
/// This is CPU availability plus our existing byte admission, not a guess at
/// idle system RAM. Reserve a complete maximum-sized eligible job per worker;
/// smaller actual inputs remain accounted by `fits` on every dispatch. With
/// the current 32 MiB scratch + 4 MiB Facts + 2 MiB stack, even an otherwise
/// empty 256 MiB allowance naturally admits at most six active workers. There
/// is no separate fixed CPU ceiling, and selection is constant-time even when
/// a caller supplies usize::MAX: byte capacity is calculated before spawn.
fn adaptive_worker_count(
    cpu_limit: usize,
    retained: usize,
    incoming: usize,
    batch_bytes: usize,
) -> usize {
    let Some(per_worker) = reservation_for_capacity(MAX_SOURCE_BYTES)
        .and_then(|job| job.checked_add(WORKER_STACK_BYTES))
    else {
        return 0;
    };
    let Some(remaining) = SOURCE_RETAINED_LIMIT
        .checked_sub(retained)
        .and_then(|bytes| bytes.checked_sub(incoming))
        .and_then(|bytes| bytes.checked_sub(batch_bytes))
    else {
        return 0;
    };
    let workers = cpu_limit.min(remaining / per_worker);
    if workers >= 2 { workers } else { 0 }
}

fn worker_failed() -> ToolkitError {
    ToolkitError::new(
        "codebase.source_worker_failed",
        "Owned Source facts worker stopped before returning its document",
        8,
    )
}

fn run_worker(
    input: Receiver<ParseInput>,
    output: SyncSender<ParseOutput>,
    control: &OperationControl,
    observe: bool,
) {
    while let Ok(input) = input.recv() {
        if output.send(prepare_input(input, control, observe)).is_err() {
            break;
        }
    }
}

fn prepare_input(input: ParseInput, control: &OperationControl, observe: bool) -> ParseOutput {
    let started = observe.then(Instant::now);
    let facts = crate::facts::for_worker(&input.data, input.mode, control, MAX_FACT_BYTES);
    let parse_elapsed = started.map_or(Duration::ZERO, |at| at.elapsed());
    ParseOutput {
        mode: input.mode,
        data: input.data,
        facts,
        parse_elapsed,
    }
}

/// Run the original Git visitor unchanged. Its read/authentication error is
/// sequenced after every document it already handed to this invocation. A
/// worker never selects a public error or consumes a shared operation counter.
pub(super) fn visit(
    home: &Path,
    pin: &SourcePin,
    files: &[git::TreeFile],
    control: &OperationControl,
    profile: &Profile,
    retained_metadata: usize,
    consume: impl Consumer,
) -> Result<usize> {
    let available_cpus = thread::available_parallelism().map_or(1, usize::from);
    visit_with_cpu_limit_and_retained(
        home,
        pin,
        files,
        control,
        profile,
        available_cpus,
        retained_metadata,
        consume,
    )
}

#[cfg(test)]
fn visit_with_cpu_limit(
    home: &Path,
    pin: &SourcePin,
    files: &[git::TreeFile],
    control: &OperationControl,
    profile: &Profile,
    cpu_limit: usize,
    consume: impl FnMut(&git::TreeFile, Vec<u8>, Option<PreparedFacts>) -> Result<()>,
) -> Result<usize> {
    visit_with_cpu_limit_and_retained(home, pin, files, control, profile, cpu_limit, 0, consume)
}

pub(super) fn visit_with_cpu_limit_and_retained(
    home: &Path,
    pin: &SourcePin,
    files: &[git::TreeFile],
    control: &OperationControl,
    profile: &Profile,
    cpu_limit: usize,
    retained_metadata: usize,
    mut consume: impl Consumer,
) -> Result<usize> {
    let mut candidates = files.iter().filter(|file| eligible(file, b"")).count();
    let cpu_limit = cpu_limit.min(candidates);
    if cpu_limit < 2 {
        return git::visit_blobs_with_control(home, pin, files, control, |file, data| {
            consume.consume(file, data, None)
        });
    }
    thread::scope(|scope| {
        let mut handles = Vec::new();
        let mut pipeline = Pipeline {
            workers: Vec::new(),
            pending: VecDeque::new(),
            pending_bytes: 0,
            retained_bytes: retained_metadata,
            free_workers: VecDeque::new(),
            incoming_bytes: files
                .iter()
                .filter(|f| !f.oversized && (!f.asset || f.bytes <= 64 << 20))
                .map(|f| f.bytes)
                .max()
                .unwrap_or(0),
            failed: false,
            batch_bytes: 0,
            profile,
        };
        let mut tried_this_batch = false;
        let mut thread_capacity_available = true;
        let visited = git::visit_blob_batches_with_control(home, pin, files, control, |event| {
            let result = (|| match event {
                git::BlobBatchVisit::BatchStart { allocation_bytes } => {
                    debug_assert!(pipeline.pending.is_empty());
                    pipeline.batch_bytes = allocation_bytes;
                    tried_this_batch = false;
                    Ok(())
                }
                git::BlobBatchVisit::Blob(file, data) => {
                    let admission = match consume.admit(file, &data, pipeline.pending.is_empty()) {
                        Admission::Drain => {
                            pipeline.drain(&mut consume)?;
                            consume.admit(file, &data, true)
                        }
                        admission => admission,
                    };
                    if !matches!(admission, Admission::Parse) {
                        // Possible cache hits (or deferred lookup errors) are
                        // ordered barriers. No decoded Facts owner enters the
                        // worker queue, and prior publication errors still win.
                        pipeline.drain(&mut consume)?;
                        consume.consume(file, data, None)?;
                        if !file.asset {
                            pipeline.retained_bytes =
                                pipeline.retained_bytes.saturating_add(file.bytes);
                        }
                        candidates = candidates.saturating_sub(usize::from(eligible(file, b"")));
                        return Ok(());
                    }
                    if pipeline.workers.is_empty()
                        && !tried_this_batch
                        && thread_capacity_available
                        && eligible(file, &data)
                    {
                        tried_this_batch = true;
                        let count = adaptive_worker_count(
                            cpu_limit.min(candidates),
                            pipeline.retained_bytes,
                            pipeline.incoming_bytes,
                            pipeline.batch_bytes,
                        );
                        // Lazily create one operation-owned scoped set, never
                        // a per-file pool. Later batches reuse these channels;
                        // their actual allocation/retained growth can reduce
                        // active jobs via `fits`, including every existing stack.
                        for ordinal in 0..count {
                            let (input, receive_input) = mpsc::sync_channel(1);
                            let (send_output, output) = mpsc::sync_channel(1);
                            let observe = profile.is_enabled();
                            match thread::Builder::new()
                                .name(format!("source-facts-{ordinal}"))
                                .stack_size(WORKER_STACK_BYTES)
                                .spawn_scoped(scope, move || {
                                    run_worker(receive_input, send_output, control, observe)
                                }) {
                                Ok(handle) => {
                                    handles.push(handle);
                                    pipeline.workers.push(Worker { input, output });
                                }
                                // Thread availability is optimization admission,
                                // never a new public operation failure.
                                Err(_) => {
                                    thread_capacity_available = false;
                                    break;
                                }
                            }
                        }
                        if pipeline.workers.len() < 2 {
                            pipeline.workers.clear();
                            for handle in handles.drain(..) {
                                let _ = handle.join(); // No input was dispatched.
                            }
                        }
                        pipeline.free_workers = (0..pipeline.workers.len()).collect();
                    }
                    candidates = candidates.saturating_sub(usize::from(eligible(file, b"")));
                    pipeline.push(file, data, &mut consume)
                }
                git::BlobBatchVisit::BatchEnd => pipeline.drain(&mut consume),
            })();
            pipeline.failed |= result.is_err();
            result
        });
        // If push failed, its earlier logical error is already selected.
        // Drain consumes only when Git itself failed later, or completed.
        // On any primary failure all remaining pure jobs are still joined.
        let result = pipeline.finish(visited, &mut consume);
        drop(pipeline);
        let mut joined = Ok(());
        for handle in handles {
            if handle.join().is_err() {
                joined = finish_owned(joined, Err(worker_failed()));
            }
        }
        finish_owned(result, joined)
    })
}

struct Pipeline<'a> {
    workers: Vec<Worker>,
    pending: VecDeque<Pending>,
    pending_bytes: usize,
    retained_bytes: usize,
    free_workers: VecDeque<usize>,
    incoming_bytes: usize,
    failed: bool,
    batch_bytes: usize,
    profile: &'a Profile,
}
impl Pipeline<'_> {
    fn receive_one(&mut self) -> Result<Option<Ready>> {
        let Some(pending) = self.pending.pop_front() else {
            return Ok(None);
        };
        let (data, prepared) = match pending.preparation {
            Preparation::ParseWorker(worker) => {
                let wait = self.profile.span(Phase::FactsWorkerWait);
                let output = self.workers[worker]
                    .output
                    .recv()
                    .map_err(|_| worker_failed())?;
                drop(wait);
                self.free_workers.push_back(worker);
                self.profile
                    .record(Phase::FactsWorker, output.parse_elapsed);
                if output.facts.is_none() {
                    self.profile
                        .record(Phase::FactsWorkerAbandoned, Duration::ZERO);
                }
                let prepared = output.facts.map(|facts| PreparedFacts {
                    mode: output.mode,
                    facts,
                    parse_elapsed: output.parse_elapsed,
                });
                (output.data, prepared)
            }
            Preparation::Sequential(data) => (data, None),
        };
        // prepare_input returns only after SyntaxDocument/WorkerGuard and all
        // parser temporaries have dropped. Keep the whole bounded Facts owner,
        // original capacity and metadata charged. Abandoned/ordinary inputs
        // retain their original reservation and are not early-refill points.
        let reservation = if prepared.is_some() {
            pending.reservation - PARSER_SCRATCH_BYTES
        } else {
            pending.reservation
        };
        self.pending_bytes -= pending.reservation - reservation;
        Ok(Some(Ready {
            file: pending.file,
            data,
            prepared,
            reservation,
        }))
    }
    fn consume_ready(&mut self, ready: Ready, consume: &mut impl Consumer) -> Result<()> {
        consume.consume(&ready.file, ready.data, ready.prepared)?;
        self.pending_bytes -= ready.reservation;
        self.retained_bytes = self.retained_bytes.saturating_add(ready.file.bytes);
        Ok(())
    }
    fn consume_one(&mut self, consume: &mut impl Consumer) -> Result<()> {
        if let Some(ready) = self.receive_one()? {
            self.consume_ready(ready, consume)?;
        }
        Ok(())
    }
    fn dispatch(&mut self, file: &git::TreeFile, data: Vec<u8>, reserved: usize) -> Result<()> {
        let worker = self.free_workers.pop_front().expect("worker slot admitted");
        self.workers[worker]
            .input
            .send(ParseInput {
                mode: ParseMode::for_path(&file.path),
                data,
            })
            .map_err(|_| worker_failed())?;
        self.pending.push_back(Pending {
            file: file.clone(),
            preparation: Preparation::ParseWorker(worker),
            reservation: reserved,
        });
        self.pending_bytes += reserved;
        Ok(())
    }
    fn drain(&mut self, consume: &mut impl Consumer) -> Result<()> {
        while !self.pending.is_empty() {
            self.consume_one(consume)?;
        }
        Ok(())
    }
    fn push(
        &mut self,
        file: &git::TreeFile,
        data: Vec<u8>,
        consume: &mut impl Consumer,
    ) -> Result<()> {
        let reserved = (!self.workers.is_empty() && eligible(file, &data))
            .then(|| reservation(&data))
            .flatten();
        if let Some(reserved) = reserved {
            while !self.pending.is_empty()
                && (self.free_workers.is_empty()
                    || self.pending.len() == (self.workers.len() * 2).min(MAX_PENDING_SLOTS)
                    || !fits(
                        self.retained_bytes,
                        self.pending_bytes,
                        reserved,
                        self.workers.len(),
                        self.incoming_bytes,
                        self.batch_bytes,
                    ))
            {
                let ready = self.receive_one()?.expect("pending front exists");
                // Refill at most this already authenticated incoming document,
                // before the earlier consumer's synchronous cache publication.
                // Count the current ready owner as a slot and in pending_bytes;
                // no completed Facts/source owner disappears from admission.
                if ready.prepared.is_some()
                    && !self.free_workers.is_empty()
                    && self.pending.len() + 2 <= (self.workers.len() * 2).min(MAX_PENDING_SLOTS)
                    && fits(
                        self.retained_bytes,
                        self.pending_bytes,
                        reserved,
                        self.workers.len(),
                        self.incoming_bytes,
                        self.batch_bytes,
                    )
                {
                    let dispatched = self.dispatch(file, data, reserved);
                    if dispatched.is_ok() {
                        self.profile.record(Phase::FactsRefill, Duration::ZERO);
                    }
                    // Even a failed speculative send cannot replace the error
                    // of the earlier document. No later consumer is invoked if
                    // this one fails; invocation-owned workers are still joined.
                    self.consume_ready(ready, consume)?;
                    return dispatched;
                }
                self.consume_ready(ready, consume)?;
            }
            if fits(
                self.retained_bytes,
                self.pending_bytes,
                reserved,
                self.workers.len(),
                self.incoming_bytes,
                self.batch_bytes,
            ) {
                return self.dispatch(file, data, reserved);
            }
        }
        // A small non-worker document is only an owned ordered input slot.
        // Its original admission, parser and I/O still run on the coordinator.
        // This lets bounded pure parsing overlap adjacent unsupported formats or other
        // ineligible sources without admitting/publishing them ahead of turn.
        if !self.workers.is_empty()
            && !file.asset
            && !file.oversized
            && data.len() <= MAX_SOURCE_BYTES
        {
            if let Some(reserved) = data.capacity().checked_add(JOB_METADATA_BYTES) {
                while !self.pending.is_empty()
                    && (self.pending.len() == (self.workers.len() * 2).min(MAX_PENDING_SLOTS)
                        || !fits(
                            self.retained_bytes,
                            self.pending_bytes,
                            reserved,
                            self.workers.len(),
                            self.incoming_bytes,
                            self.batch_bytes,
                        ))
                {
                    self.consume_one(consume)?;
                }
                if fits(
                    self.retained_bytes,
                    self.pending_bytes,
                    reserved,
                    self.workers.len(),
                    self.incoming_bytes,
                    self.batch_bytes,
                ) {
                    self.pending.push_back(Pending {
                        file: file.clone(),
                        preparation: Preparation::Sequential(data),
                        reservation: reserved,
                    });
                    self.pending_bytes += reserved;
                    return Ok(());
                }
            }
        }
        // Large inputs, assets and exhausted shared allowance are barriers.
        // The complete original path remains available with no new failure.
        self.drain(consume)?;
        consume.consume(file, data, None)?;
        if !file.asset {
            self.retained_bytes = self.retained_bytes.saturating_add(file.bytes);
        }
        Ok(())
    }
    fn finish(&mut self, visited: Result<usize>, consume: &mut impl Consumer) -> Result<usize> {
        if !self.failed {
            self.drain(consume)?;
        }
        visited
    }
}

#[cfg(test)]
#[path = "facts_pipeline_tests.rs"]
mod tests;

#[cfg(test)]
#[test]
fn retained_cohort_metadata_reduces_real_worker_admission_without_changing_facts() {
    let fixture = super::facts_cache_tests::Fixture::new();
    let raw = (0..8)
        .map(|n| (format!("{n}.lua"), format!("function F{n}() Call() end")))
        .collect::<std::collections::BTreeMap<_, _>>();
    let pin = fixture.pin(&raw);
    let control = OperationControl::default();
    let files = git::tree_with_control(&fixture.home, &pin, &control).unwrap();
    let mut observed = Vec::new();
    for retained in [0, SOURCE_RETAINED_LIMIT - (1 << 20)] {
        let mut output = Vec::new();
        let mut prepared_count = 0;
        visit_with_cpu_limit_and_retained(
            &fixture.home,
            &pin,
            &files,
            &control,
            &Profile::new(),
            8,
            retained,
            |file: &git::TreeFile, bytes: Vec<u8>, prepared: Option<PreparedFacts>| {
                prepared_count += usize::from(prepared.is_some());
                let facts = prepared.map_or_else(
                    || ParseMode::for_path(&file.path).parse(&bytes),
                    |p| p.facts,
                );
                output.push((
                    file.path.clone(),
                    bytes,
                    serde_json::to_value(facts).unwrap(),
                ));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(prepared_count, if retained == 0 { 8 } else { 0 });
        observed.push(output);
    }
    assert_eq!(observed[0], observed[1]);
}
