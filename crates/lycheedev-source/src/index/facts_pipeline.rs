//! A bounded, typed cold-index pipeline. Only pure public Lua observations may
//! run ahead; authentication, cache I/O, controls and publication stay ordered.
use super::profile::{Phase, Profile};
use crate::{facts::Facts, git};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, metadata::historical_write::finish_historical,
    selection::SourcePin,
};
use std::{
    collections::VecDeque,
    path::Path,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};

const MAX_WORKERS: usize = 4;
const MAX_SOURCE_BYTES: usize = 64 << 10;
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
struct LuaInput {
    data: Vec<u8>,
}
struct LuaOutput {
    data: Vec<u8>,
    facts: Option<Facts>,
    parse_elapsed: Duration,
}
struct Worker {
    input: SyncSender<LuaInput>,
    output: Receiver<LuaOutput>,
}
enum Preparation {
    LuaWorker(usize),
    Sequential(Vec<u8>),
}
struct Pending {
    file: git::TreeFile,
    preparation: Preparation,
    reservation: usize,
}

/// None preserves the exact complete parser path. A worker cap is never a new
/// public syntax diagnostic, truncation, or operation-budget failure.
pub(super) struct PreparedFacts {
    pub(super) facts: Facts,
    pub(super) parse_elapsed: Duration,
}

fn eligible(file: &git::TreeFile, data: &[u8]) -> bool {
    !file.asset
        && !file.oversized
        && file.bytes <= MAX_SOURCE_BYTES
        && data.len() <= MAX_SOURCE_BYTES
        && file
            .path
            .as_bytes()
            .get(file.path.len().saturating_sub(4)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(b".lua"))
        && !file.path.contains("/Blizzard_APIDocumentationGenerated/")
        && std::str::from_utf8(data).is_ok()
}

fn reservation(data: &Vec<u8>) -> Option<usize> {
    data.capacity()
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

fn worker_failed() -> ToolkitError {
    ToolkitError::new(
        "codebase.source_worker_failed",
        "Owned Source facts worker stopped before returning its document",
        8,
    )
}

fn run_worker(
    input: Receiver<LuaInput>,
    output: SyncSender<LuaOutput>,
    control: &OperationControl,
    observe: bool,
) {
    while let Ok(LuaInput { data }) = input.recv() {
        if output
            .send(prepare_input(LuaInput { data }, control, observe))
            .is_err()
        {
            break;
        }
    }
}

fn prepare_input(input: LuaInput, control: &OperationControl, observe: bool) -> LuaOutput {
    let started = observe.then(Instant::now);
    let facts = crate::facts::lua_for_worker(&input.data, control, MAX_FACT_BYTES);
    let parse_elapsed = started.map_or(Duration::ZERO, |at| at.elapsed());
    LuaOutput {
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
    consume: impl FnMut(&git::TreeFile, Vec<u8>, Option<PreparedFacts>) -> Result<()>,
) -> Result<usize> {
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(MAX_WORKERS);
    visit_with_workers(home, pin, files, control, profile, workers, consume)
}

fn visit_with_workers(
    home: &Path,
    pin: &SourcePin,
    files: &[git::TreeFile],
    control: &OperationControl,
    profile: &Profile,
    count: usize,
    mut consume: impl FnMut(&git::TreeFile, Vec<u8>, Option<PreparedFacts>) -> Result<()>,
) -> Result<usize> {
    if count < 2 || !files.iter().any(|f| eligible(f, b"")) {
        return git::visit_blobs_with_control(home, pin, files, control, |file, data| {
            consume(file, data, None)
        });
    }
    thread::scope(|scope| {
        let mut handles = Vec::new();
        let mut workers = Vec::new();
        for ordinal in 0..count.min(MAX_WORKERS) {
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
                    workers.push(Worker { input, output });
                }
                // Thread capacity is an optimization admission condition. It
                // never replaces the existing complete single-threaded path.
                Err(_) => break,
            }
        }
        if workers.len() < 2 {
            drop(workers);
            for handle in handles {
                let _ = handle.join(); // No input was dispatched to this worker.
            }
            return git::visit_blobs_with_control(home, pin, files, control, |file, data| {
                consume(file, data, None)
            });
        }
        let result = {
            let mut pipeline = Pipeline {
                workers: &workers,
                pending: VecDeque::new(),
                pending_bytes: 0,
                retained_bytes: 0,
                free_workers: (0..workers.len()).collect(),
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
            let visited =
                git::visit_blob_batches_with_control(home, pin, files, control, |event| {
                    let result = match event {
                        git::BlobBatchVisit::BatchStart { allocation_bytes } => {
                            debug_assert!(pipeline.pending.is_empty());
                            pipeline.batch_bytes = allocation_bytes;
                            Ok(())
                        }
                        git::BlobBatchVisit::Blob(file, data) => {
                            pipeline.push(file, data, &mut consume)
                        }
                        git::BlobBatchVisit::BatchEnd => pipeline.drain(&mut consume),
                    };
                    pipeline.failed |= result.is_err();
                    result
                });
            // If push failed, its earlier logical error is already selected.
            // Drain consumes only when Git itself failed later, or completed.
            // On any primary failure all remaining pure jobs are still joined.
            pipeline.finish(visited, &mut consume)
        };
        drop(workers);
        let mut joined = Ok(());
        for handle in handles {
            if handle.join().is_err() {
                joined = finish_historical(joined, Err(worker_failed()));
            }
        }
        finish_historical(result, joined)
    })
}

struct Pipeline<'a> {
    workers: &'a [Worker],
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
    fn consume_one(
        &mut self,
        consume: &mut impl FnMut(&git::TreeFile, Vec<u8>, Option<PreparedFacts>) -> Result<()>,
    ) -> Result<()> {
        let Some(pending) = self.pending.pop_front() else {
            return Ok(());
        };
        let (data, prepared) = match pending.preparation {
            Preparation::LuaWorker(worker) => {
                let output = self.workers[worker]
                    .output
                    .recv()
                    .map_err(|_| worker_failed())?;
                self.free_workers.push_back(worker);
                self.profile
                    .record(Phase::FactsWorker, output.parse_elapsed);
                let prepared = output.facts.map(|facts| PreparedFacts {
                    facts,
                    parse_elapsed: output.parse_elapsed,
                });
                (output.data, prepared)
            }
            Preparation::Sequential(data) => (data, None),
        };
        consume(&pending.file, data, prepared)?;
        self.pending_bytes -= pending.reservation;
        self.retained_bytes = self.retained_bytes.saturating_add(pending.file.bytes);
        Ok(())
    }
    fn drain(
        &mut self,
        consume: &mut impl FnMut(&git::TreeFile, Vec<u8>, Option<PreparedFacts>) -> Result<()>,
    ) -> Result<()> {
        while !self.pending.is_empty() {
            self.consume_one(consume)?;
        }
        Ok(())
    }
    fn push(
        &mut self,
        file: &git::TreeFile,
        data: Vec<u8>,
        consume: &mut impl FnMut(&git::TreeFile, Vec<u8>, Option<PreparedFacts>) -> Result<()>,
    ) -> Result<()> {
        let reserved = eligible(file, &data).then(|| reservation(&data)).flatten();
        if let Some(reserved) = reserved {
            while !self.pending.is_empty()
                && (self.free_workers.is_empty()
                    || self.pending.len() == self.workers.len() * 2
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
                let worker = self.free_workers.pop_front().expect("worker slot admitted");
                self.workers[worker]
                    .input
                    .send(LuaInput { data })
                    .map_err(|_| worker_failed())?;
                self.pending.push_back(Pending {
                    file: file.clone(),
                    preparation: Preparation::LuaWorker(worker),
                    reservation: reserved,
                });
                self.pending_bytes += reserved;
                return Ok(());
            }
        }
        // A small non-worker document is only an owned ordered input slot.
        // Its original admission, parser and I/O still run on the coordinator.
        // This lets pure Lua work overlap adjacent XML/TOC processing without
        // changing the XML catalog or admitting/publishing it ahead of its turn.
        if !file.asset && !file.oversized && data.len() <= MAX_SOURCE_BYTES {
            if let Some(reserved) = data.capacity().checked_add(JOB_METADATA_BYTES) {
                while !self.pending.is_empty()
                    && (self.pending.len() == self.workers.len() * 2
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
        consume(file, data, None)?;
        if !file.asset {
            self.retained_bytes = self.retained_bytes.saturating_add(file.bytes);
        }
        Ok(())
    }
    fn finish(
        &mut self,
        visited: Result<usize>,
        consume: &mut impl FnMut(&git::TreeFile, Vec<u8>, Option<PreparedFacts>) -> Result<()>,
    ) -> Result<usize> {
        if !self.failed {
            self.drain(consume)?;
        }
        visited
    }
}

#[cfg(test)]
#[path = "facts_pipeline_tests.rs"]
mod tests;
