//! Go v3.2.3 query memory policy, independent of Rust-only safety accounting.
use super::{Budget, Cost, State, fault};
use lycheedev_core::{Result, control::OperationControl, workspace::data_resources::DataResources};
use serde::Serialize;
use std::{sync::Arc, time::Instant};

const MEMORY_STEP: u64 = 64 << 20;
type MemoryProbe = Arc<dyn Fn() -> Result<u64> + Send + Sync>;

/// Approved Go accounting limits. These are not allocated heap or process RSS.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryBudget {
    pub mode: &'static str,
    #[serde(skip_serializing_if = "is_zero")]
    pub concurrent_queries: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub ceiling_bytes: u64,
    pub retained_bytes: u64,
    pub metadata_bytes: u64,
    pub peak_scratch_bytes: u64,
    pub growths: u64,
}
fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Clone)]
pub(super) enum MemorySource {
    Platform,
    #[cfg(test)]
    Probe(MemoryProbe),
}
impl MemorySource {
    fn probe(&self) -> Option<MemoryProbe> {
        match self {
            Self::Platform => {
                #[cfg(windows)]
                {
                    Some(Arc::new(
                        lycheedev_core::workspace::data_resources::available_query_memory,
                    ))
                }
                #[cfg(not(windows))]
                {
                    None
                }
            }
            #[cfg(test)]
            Self::Probe(probe) => Some(probe.clone()),
        }
    }
}
pub(super) struct AdaptiveMemory {
    available: MemoryProbe,
    ceiling: u64,
    shares: usize,
    growths: u64,
}
pub(super) struct Growth {
    logical: Cost,
    safety: Cost,
    logical_growth: bool,
    pub(super) sampled_memory: bool,
}
pub(super) fn overflow(
    dimension: &str,
    used: u64,
    requested: u64,
    limit: u64,
) -> lycheedev_core::ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        format!(
            "records.query_resource_budget: {dimension} (used={used} requested={requested} limit={limit})"
        ),
    )
}
pub(super) fn not_admitted() -> lycheedev_core::ToolkitError {
    fault(
        "records.internal",
        8,
        "Query accounting used before workspace admission",
    )
}
fn observe(probe: &MemoryProbe) -> Result<u64> {
    let bytes = probe().map_err(|cause| {
        fault(
            "records.query_resource_budget",
            3,
            format!("memory_status: {}", cause.message),
        )
        .with_details(serde_json::json!({"stage":"memory_status","cause":cause}))
    })?;
    // Go converts the native uint64 minimum to int64 before policy arithmetic.
    // Impossible signed-native availability must not become vast permission.
    if bytes > i64::MAX as u64 {
        return Err(fault(
            "records.query_resource_budget",
            3,
            "memory_status: available memory exceeds signed native range",
        ));
    }
    Ok(bytes)
}

impl Budget {
    /// Construct the original control scope without observing host memory.
    /// The same object is activated only after actual workspace admission.
    /// Explicit `new`/`controlled` budgets remain fixed, as injected Go budgets do.
    pub fn query_controlled(deadline: Instant, control: OperationControl) -> Self {
        let mut budget = Self::controlled(Cost::defaults(), deadline, control);
        budget.state.lock().expect("new query state").pending = Some(MemorySource::Platform);
        budget.phase_profile = crate::phase_profile::Profile::from_env();
        budget
    }

    /// Requires a held admission owner, not a caller-supplied numeric capacity.
    /// Reuse is idempotent: nested readers never resample or reset the ceiling.
    pub fn initialize_admitted(&self, admission: &DataResources) -> Result<()> {
        self.check()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        let Some(source) = state.pending.as_ref() else {
            return Ok(());
        };
        let Some(probe) = source.probe() else {
            state.pending = None;
            return Ok(());
        };
        let shares = admission.concurrent_queries();
        if shares == 0 {
            return Err(fault(
                "records.internal",
                8,
                "Admitted query capacity is zero",
            ));
        }
        let ceiling = observe(&probe)? / 2 / shares as u64;
        self.check()?;
        let mut limits = state.limits;
        limits.retained_bytes = limits.retained_bytes.min(ceiling);
        limits.metadata_bytes = limits.metadata_bytes.min(ceiling);
        // Request parsing can have bounded Rust-only charges before admission.
        // Never erase those allocations or activate a ceiling smaller than them.
        let maximum = Cost {
            retained_bytes: ceiling,
            metadata_bytes: ceiling,
            ..limits
        };
        Budget::admitted(state.used, Cost::default(), maximum)?;
        let mut safety_limits = limits;
        safety_limits.retained_bytes = safety_limits.retained_bytes.max(state.used.retained_bytes);
        safety_limits.metadata_bytes = safety_limits.metadata_bytes.max(state.used.metadata_bytes);
        state.limits = limits;
        state.safety_limits = safety_limits;
        state.memory = Some(AdaptiveMemory {
            available: probe,
            ceiling,
            shares,
            growths: 0,
        });
        state.pending = None;
        Ok(())
    }

    pub fn memory_snapshot(&self) -> Result<MemoryBudget> {
        let state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        if state.pending.is_some() {
            return Err(not_admitted());
        }
        let (mode, concurrent_queries, ceiling_bytes, growths) = match &state.memory {
            Some(memory) => (
                "windows_adaptive",
                memory.shares,
                memory.ceiling,
                memory.growths,
            ),
            None => ("fixed", 0, 0, 0),
        };
        Ok(MemoryBudget {
            mode,
            concurrent_queries,
            ceiling_bytes,
            growths,
            retained_bytes: state.limits.retained_bytes,
            metadata_bytes: state.limits.metadata_bytes,
            peak_scratch_bytes: state.peak_scratch,
        })
    }

    /// Current approved Go allowance, including temporary decoder scratch.
    /// The original ceiling is not permission to skip a fresh growth probe.
    pub fn remaining_retained(&self) -> Result<u64> {
        let state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        if state.pending.is_some() {
            return Err(not_admitted());
        }
        state
            .limits
            .retained_bytes
            .checked_sub(state.logical.retained_bytes)
            .and_then(|n| n.checked_sub(state.logical_scratch))
            .ok_or_else(|| {
                fault(
                    "records.internal",
                    8,
                    "Logical resource ledger exceeds its limit",
                )
            })
    }

    #[cfg(test)]
    pub(crate) fn query_with_probe(
        deadline: Instant,
        control: OperationControl,
        probe: MemoryProbe,
    ) -> Self {
        let budget = Self::query_controlled(deadline, control);
        budget.state.lock().unwrap().pending = Some(MemorySource::Probe(probe));
        budget
    }
}

impl State {
    pub(super) fn maximum(&self, mut limits: Cost) -> Cost {
        if let Some(memory) = &self.memory {
            limits.retained_bytes = memory.ceiling;
            limits.metadata_bytes = memory.ceiling;
        }
        limits
    }

    /// Stage both overlapping ledgers under the same immutable ceiling. Each
    /// ledger must fit that ceiling; they are not additive memory allowances.
    /// A single fresh observation suffices for this serialized allocation need.
    pub(super) fn growth(
        &self,
        retained: u64,
        metadata: u64,
        safety_retained: u64,
        safety_metadata: u64,
    ) -> Result<Growth> {
        let logical_growth =
            retained > self.limits.retained_bytes || metadata > self.limits.metadata_bytes;
        let safety_growth = safety_retained > self.safety_limits.retained_bytes
            || safety_metadata > self.safety_limits.metadata_bytes;
        let mut out = Growth {
            logical: self.limits,
            safety: self.safety_limits,
            logical_growth,
            sampled_memory: false,
        };
        let Some(memory) = &self.memory else {
            return Ok(out);
        };
        if !logical_growth && !safety_growth {
            return Ok(out);
        }
        let headroom = observe(&memory.available)? / 2 / memory.shares as u64;
        out.sampled_memory = true;
        for (used, needed, limit, dimension) in [
            (
                self.logical.retained_bytes + self.logical_scratch,
                retained,
                &mut out.logical.retained_bytes,
                "retained_bytes",
            ),
            (
                self.logical.metadata_bytes,
                metadata,
                &mut out.logical.metadata_bytes,
                "metadata_bytes",
            ),
            (
                self.used.retained_bytes,
                safety_retained,
                &mut out.safety.retained_bytes,
                "retained_bytes",
            ),
            (
                self.used.metadata_bytes,
                safety_metadata,
                &mut out.safety.metadata_bytes,
                "metadata_bytes",
            ),
        ] {
            if needed <= *limit {
                continue;
            }
            // All hard dimensions were checked before this function. The
            // incremental need, not existing accounted allocation, needs fresh
            // headroom. Calculate via subtraction to avoid overflow.
            let requested = needed.checked_sub(used).ok_or_else(|| {
                fault("records.internal", 8, "Query growth is below current usage")
            })?;
            let remaining = memory
                .ceiling
                .checked_sub(used)
                .ok_or_else(|| overflow(dimension, used, requested, memory.ceiling))?;
            let upper = used + headroom.min(remaining);
            if needed > upper {
                return Err(overflow(dimension, used, requested, upper));
            }
            let extra = (MEMORY_STEP - needed % MEMORY_STEP) % MEMORY_STEP;
            *limit = needed + extra.min(upper - needed);
        }
        Ok(out)
    }
    pub(super) fn apply_growth(&mut self, growth: Growth) {
        self.limits = growth.logical;
        self.safety_limits = growth.safety;
        if growth.logical_growth
            && let Some(memory) = &mut self.memory
        {
            memory.growths += 1;
        }
    }
}

#[cfg(test)]
mod tests;
