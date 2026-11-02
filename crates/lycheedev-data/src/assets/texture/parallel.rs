//! One operation-owned CPU team for the selected authenticated mip. The input
//! and complete RGBA output are already admitted by decode_verified; workers
//! borrow those allocations and write disjoint four-row bands in place. There
//! is no producer queue, speculative decode, new source scope or output copy.
use super::*;
use std::thread;

const MIN_PIXELS_PER_WORKER: usize = 64 << 10;
const WORKER_STACK_BYTES: usize = 2 << 20;
// Covers scoped-thread control/closures and OS thread bookkeeping
// conservatively. Handle Vec capacity is charged separately below.
// This is admitted storage, not a process RSS guarantee.
const WORKER_METADATA_BYTES: usize = 8192;
const WORKER_BYTES: usize = WORKER_STACK_BYTES
    + WORKER_METADATA_BYTES
    + std::mem::size_of::<thread::ScopedJoinHandle<'static, Result<()>>>();

pub(super) fn fill(
    pixels: &mut [u8],
    raw: &[u8],
    data: &[u8],
    description: &Description,
    control: &OperationControl,
    budget: &Budget,
) -> Result<()> {
    let cpus = thread::available_parallelism().map_or(1, usize::from);
    fill_with_limit(pixels, raw, data, description, control, budget, cpus).map(|_| ())
}

fn wanted_workers(description: &Description, cpus: usize) -> usize {
    let pixels = description.width as usize * description.height as usize;
    cpus.min((description.height as usize).div_ceil(4))
        .min(pixels / MIN_PIXELS_PER_WORKER)
        .max(1)
}

// The returned count is useful to regress actual admission, not a public
// tuning mode. All production calls derive their CPU bound from the OS.
#[allow(clippy::too_many_arguments)]
fn fill_with_limit(
    pixels: &mut [u8],
    raw: &[u8],
    data: &[u8],
    description: &Description,
    control: &OperationControl,
    budget: &Budget,
    cpus: usize,
) -> Result<usize> {
    control.check()?;
    budget.check()?;
    #[cfg(test)]
    let test_hooks = tests::current_hooks();
    #[cfg(test)]
    let test_hooks = test_hooks.as_deref();
    let requested = wanted_workers(description, cpus);
    // Only use current private headroom. No memory probe, adaptive allowance
    // growth, independent budget or logical Go charge is permitted here. The
    // caller is one CPU participant and needs no additional worker stack.
    let (extra, workers) = budget.try_reserve_workers(requested - 1, WORKER_BYTES)?;
    let count = extra + 1;
    if count == 1 {
        fill_strip(pixels, 0, raw, data, description, control, Some(budget))?;
        return Ok(1);
    }
    let rows = (description.height as usize).div_ceil(4).div_ceil(count) * 4;
    let strip_bytes = rows * description.width as usize * 4;
    let strips = pixels.len().div_ceil(strip_bytes);
    let unfilled = thread::scope(|scope| -> Result<Option<usize>> {
        let mut handles: Vec<thread::ScopedJoinHandle<'_, Result<()>>> = Vec::new();
        // No fixed CPU ceiling: capacity follows the admitted workers and is
        // included in WORKER_BYTES. Allocation failure falls back before any
        // thread has started, borrowing the same existing output buffer.
        if handles.try_reserve_exact(extra).is_err() {
            return Ok(Some(0));
        }
        let mut unfilled = None;
        let mut caller = Ok(());
        for (index, strip) in pixels.chunks_mut(strip_bytes).enumerate() {
            let first_row = index * rows;
            if index + 1 == strips {
                caller = (|| {
                    #[cfg(test)]
                    tests::before_strip(test_hooks, first_row)?;
                    fill_strip(
                        strip,
                        first_row,
                        raw,
                        data,
                        description,
                        control,
                        Some(budget),
                    )
                })();
            } else {
                // A stop after a previous spawn still goes through the common
                // ordered join path; it never detaches an admitted worker.
                if let Err(error) = control.check().and_then(|()| budget.check()) {
                    caller = Err(error);
                    break;
                }
                // One worker owns many block rows for the whole operation;
                // never construct a new pool or thread for each pixel/block.
                let spawn = || {
                    thread::Builder::new()
                        .stack_size(WORKER_STACK_BYTES)
                        .spawn_scoped(scope, move || {
                            #[cfg(test)]
                            tests::before_strip(test_hooks, first_row)?;
                            fill_strip(
                                strip,
                                first_row,
                                raw,
                                data,
                                description,
                                control,
                                Some(budget),
                            )
                        })
                };
                #[cfg(test)]
                let rejected = test_hooks.is_some_and(|hooks| hooks.reject_spawn_at == Some(index));
                #[cfg(not(test))]
                let rejected = false;
                let started = if rejected {
                    Err(std::io::Error::other("test rejected texture worker"))
                } else {
                    spawn()
                };
                match started {
                    Ok(handle) => handles.push(handle),
                    Err(_) => {
                        // The failed spawn drops its untouched borrow. Join all
                        // earlier strips before borrowing the remaining suffix
                        // again outside the scope; OS scarcity stays a serial
                        // fallback, not a new public image failure.
                        unfilled = Some(first_row);
                        break;
                    }
                }
            }
        }
        let mut first_error = None;
        // Observe outcomes in image order, never in completion order. Every
        // handle is joined even after cancellation, failure or worker panic.
        for handle in handles {
            let result = handle
                .join()
                .unwrap_or_else(|_| Err(fault("records.internal", 8)));
            if first_error.is_none() {
                first_error = result.err();
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        caller?;
        Ok(unfilled)
    });
    // Refund only this team's storage, after every thread is joined. Caller
    // allocations and any independent reservations remain owned and charged.
    drop(workers);
    if let Some(first_row) = unfilled? {
        let start = first_row * description.width as usize * 4;
        #[cfg(test)]
        tests::before_strip(test_hooks, first_row)?;
        fill_strip(
            &mut pixels[start..],
            first_row,
            raw,
            data,
            description,
            control,
            Some(budget),
        )?;
    }
    control.check()?;
    budget.check()?;
    Ok(strips)
}

#[cfg(test)]
#[path = "parallel_tests.rs"]
mod tests;
