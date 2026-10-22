//! One-shot, read-only target observation through the runtime instruction root.
//! No returned field grants input, reconstructs a reader capability or is a saved
//! root/heap pointer. Every subsequent operation must open its own current scope.
use crate::{
    mailbox::lookup::{Record, Selector},
    runtime_binding::{
        RuntimeBindingDescription, RuntimeBindingFailure, RuntimeBoundMemory, RuntimeCoverage,
        runtime_budget,
    },
};
use lycheedev_core::control::OperationControl;
use lycheedev_windows::{
    WindowIdentity,
    process::{RuntimeBudget, RuntimeCost, RuntimeLimits, RuntimeUsage},
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCounters {
    pub os_call_units: u64,
    pub query_calls: u64,
    pub read_calls: u64,
    pub requested_bytes: u64,
    pub allocation_bytes: u64,
    pub work_units: u64,
}
impl From<RuntimeCost> for RuntimeCounters {
    fn from(value: RuntimeCost) -> Self {
        Self {
            os_call_units: value.os_call_units,
            query_calls: value.query_calls,
            read_calls: value.read_calls,
            requested_bytes: value.requested_bytes,
            allocation_bytes: value.allocation_bytes,
            work_units: value.work_units,
        }
    }
}
/// Reservations are cumulative admissions, not measured RSS or exact syscall counts.
#[derive(Clone, Debug, Serialize)]
pub struct RuntimeBudgetObservation {
    pub limits: RuntimeCounters,
    pub used: RuntimeCounters,
}
fn budget_observation(limits: RuntimeLimits, used: RuntimeUsage) -> RuntimeBudgetObservation {
    RuntimeBudgetObservation {
        limits: limits.into(),
        used: used.into(),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIdentityObservation {
    pub runtime: String,
    pub guid: String,
    pub character: String,
    pub realm: String,
    pub product: String,
    pub build: String,
    pub release: String,
    /// Observed loaded-slot inventory when published; absence is not zero.
    pub inventory: Option<i32>,
    pub slots: i32,
    pub compatibility: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostic_codes: Vec<&'static str>,
    pub activation_only: bool,
    pub record_sequence: u32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationAuthority {
    ObservationOnly,
}
/// Serializable facts only. Neither this DTO nor a round-tripped copy is
/// accepted by RuntimeBoundMemory, route ownership or an input API.
#[derive(Debug, Serialize)]
pub struct RuntimeTargetObservation {
    pub binding: RuntimeBindingDescription,
    pub identity: RuntimeIdentityObservation,
    pub budget: RuntimeBudgetObservation,
    pub authority: ObservationAuthority,
}
#[derive(Clone, Debug, Serialize)]
pub struct RuntimeObservationFailure {
    pub cause: String,
    pub stage: &'static str,
    pub coverage: RuntimeCoverage,
    /// None means budget construction failed before a ledger was returned.
    pub budget: Option<RuntimeBudgetObservation>,
}
impl std::fmt::Display for RuntimeObservationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cause)
    }
}
impl std::error::Error for RuntimeObservationFailure {}
type Result<T> = std::result::Result<T, RuntimeObservationFailure>;

/// Current selected window -> OS module -> runtime instruction root -> named
/// Identity record. A single ledger includes open, scan, metadata, lookup and
/// DTO decoding. The held reader is dropped on both success and every failure.
pub fn observe_runtime(
    window: WindowIdentity,
    control: &OperationControl,
) -> Result<RuntimeTargetObservation> {
    observe_with(control, RuntimeLimits::defaults(), |budget| {
        RuntimeBoundMemory::open(window, lycheedev_core::VERSION, budget)
    })
}

// Internal fixture seam, never a constructor for a production runtime binding.
pub(crate) trait ObservationReader {
    fn identity(
        &mut self,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure>;
    fn description(
        &mut self,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<RuntimeBindingDescription, RuntimeBindingFailure>;
    fn coverage(&self) -> RuntimeCoverage;
}
impl ObservationReader for RuntimeBoundMemory {
    fn identity(
        &mut self,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<Option<Record>, RuntimeBindingFailure> {
        self.lookup(&Selector::Identity, budget)
    }
    fn description(
        &mut self,
        budget: &mut RuntimeBudget,
    ) -> std::result::Result<RuntimeBindingDescription, RuntimeBindingFailure> {
        RuntimeBoundMemory::description(self, budget)
    }
    fn coverage(&self) -> RuntimeCoverage {
        self.latest_root_evidence().into()
    }
}
fn binding_failure(
    error: RuntimeBindingFailure,
    limits: RuntimeLimits,
) -> RuntimeObservationFailure {
    RuntimeObservationFailure {
        cause: error.cause,
        stage: error.stage,
        coverage: error.root.as_ref().into(),
        budget: Some(budget_observation(limits, error.usage)),
    }
}
pub(crate) fn observe_with<R: ObservationReader>(
    control: &OperationControl,
    limits: RuntimeLimits,
    open: impl FnOnce(&mut RuntimeBudget) -> std::result::Result<R, RuntimeBindingFailure>,
) -> Result<RuntimeTargetObservation> {
    let mut budget =
        runtime_budget(control, limits).map_err(|cause| RuntimeObservationFailure {
            cause,
            stage: "budget",
            coverage: RuntimeCoverage::default(),
            budget: None,
        })?;
    let mut reader = open(&mut budget).map_err(|error| binding_failure(error, limits))?;
    let record = reader
        .identity(&mut budget)
        .map_err(|error| binding_failure(error, limits))?;
    let record = record.ok_or_else(|| RuntimeObservationFailure {
        cause: "live.runtime_identity_unavailable".into(),
        stage: "identity",
        coverage: reader.coverage(),
        budget: Some(budget_observation(limits, budget.usage())),
    })?;
    let description = reader
        .description(&mut budget)
        .map_err(|error| binding_failure(error, limits))?;
    let identity = decode_identity(record, &description, &mut budget).map_err(|cause| {
        RuntimeObservationFailure {
            cause,
            stage: "identity",
            coverage: reader.coverage(),
            budget: Some(budget_observation(limits, budget.usage())),
        }
    })?;
    Ok(RuntimeTargetObservation {
        binding: description,
        identity,
        budget: budget_observation(limits, budget.usage()),
        authority: ObservationAuthority::ObservationOnly,
    })
}

fn decode_identity(
    record: Record,
    description: &RuntimeBindingDescription,
    budget: &mut RuntimeBudget,
) -> std::result::Result<RuntimeIdentityObservation, String> {
    use crate::mailbox as protocol;
    // Named lookup has already verified wire and path. The same cumulative
    // request ledger accounts this bounded JSON decoding separately.
    budget.charge(RuntimeCost {
        allocation_bytes: record.payload.len() as u64 * 8
            + std::mem::size_of::<protocol::Identity>() as u64,
        work_units: record.payload.len() as u64 * 4 + 256,
        ..RuntimeCost::default()
    })?;
    let borrowed = record.contract()?;
    let value = protocol::decode_identity(&borrowed, &description.expected_release)
        .map_err(|e| e.to_string())?;
    if value.product != description.client.product {
        return Err("live.runtime_identity_product".into());
    }
    if value.build != description.client.full_build {
        return Err("live.runtime_identity_build".into());
    }
    budget.check()?;
    Ok(RuntimeIdentityObservation {
        runtime: value.runtime,
        guid: value.guid,
        character: value.character,
        realm: value.realm,
        product: value.product,
        build: value.build,
        release: value.release,
        inventory: value.inventory,
        slots: value.slots,
        compatibility: "current",
        diagnostic_codes: Vec::new(),
        activation_only: false,
        record_sequence: record.sequence,
    })
}
