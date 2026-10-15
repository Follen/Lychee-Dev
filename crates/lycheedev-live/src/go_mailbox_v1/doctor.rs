//! Fixed-Go read-only Live diagnosis. No publisher, sender or claim-acquisition
//! trait is available to this module. Bootstrap never authorizes business input.
#![forbid(unsafe_code)]
use super::{
    Identity, IdentityPublication, InputBindings, InputObservation, Kind, PREVIOUS_SCHEMA, SCHEMA,
    SLOT_COUNT, Selector, ZERO_TOKEN,
    admission::ConnectionTarget,
    connection::ProjectResult,
    decode_identity, decode_input,
    driver::{error, pending},
    durable_journal::WindowOwner,
    inspect_input,
    observations::{ChannelError, ChannelResult, RecordReader, check_control},
    parse_token,
    reload::ContinuationScheduler,
    state::SlotEnvelope,
};
use lycheedev_core::control::OperationControl;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const INPUT_SAMPLE_LIMIT: Duration = Duration::from_millis(1500);
pub const TOP_LEVEL_LIMIT: Duration = Duration::from_secs(10);
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub state: String,
    pub detail: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub next_step: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct BootstrapBindings {
    pub wake: String,
    pub reload: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct BootstrapState {
    pub schema: String,
    pub bootstrap: String,
    pub release: String,
    pub phase: String,
    pub reason: String,
    pub binding_failure: String,
    pub bindings: BootstrapBindings,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct SlotAssessment {
    pub state: String,
    pub count: i32,
    pub pending: i32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DoctorDeployment {
    pub state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub commit: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slots: Option<SlotAssessment>,
}
impl DoctorDeployment {
    pub fn needs_install(&self, release: &str) -> bool {
        self.state != "managed"
            || self.version != release
            || self.slots.as_ref().is_none_or(|s| {
                s.count != SLOT_COUNT || !matches!(s.state.as_str(), "managed" | "pending")
            })
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorResult {
    pub protocol: String,
    pub healthy: bool,
    pub checks: Vec<DoctorCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ConnectionTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment: Option<DoctorDeployment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bootstrap: Option<BootstrapState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<InputObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<WindowOwner>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<ProjectResult>,
    /// Internal typed causes accompany public checks. Do not erase an original
    /// control/read/close error by reconstructing it from a diagnostic string.
    #[serde(skip)]
    pub errors: Vec<ChannelError>,
}
impl Default for DoctorResult {
    fn default() -> Self {
        Self {
            protocol: SCHEMA.into(),
            healthy: true,
            checks: vec![],
            target: None,
            deployment: None,
            bootstrap: None,
            identity: None,
            input: None,
            owner: None,
            connection: None,
            errors: vec![],
        }
    }
}
impl DoctorResult {
    pub fn add(&mut self, id: &str, state: &str, detail: impl Into<String>, next: &str) {
        if matches!(state, "blocked" | "error") {
            self.healthy = false;
        }
        self.checks.push(DoctorCheck {
            id: id.into(),
            state: state.into(),
            detail: detail.into(),
            next_step: next.into(),
        });
    }
    pub fn failure(&mut self, id: &str, state: &str, error: ChannelError, next: &str) {
        self.add(id, state, error.to_string(), next);
        self.errors.push(error);
    }
    pub fn inspect_deployment(&mut self, deployment: DoctorDeployment, release: &str) {
        if deployment.state != "managed" || deployment.version != release {
            self.add("live.installation", "blocked", &deployment.state, "live doctor --fix --release <current distribution> installs or upgrades a clean managed addon");
        } else {
            self.add(
                "live.installation",
                "ok",
                format!("clean managed addon {}", deployment.version),
                "",
            );
        }
        match &deployment.slots {
            Some(slots)
                if slots.count == SLOT_COUNT
                    && matches!(slots.state.as_str(), "managed" | "pending") =>
            {
                self.add(
                    "live.slots",
                    "ok",
                    format!(
                        "{} slots, {} pending reservations",
                        slots.count, slots.pending
                    ),
                    "",
                )
            }
            _ => self.add(
                "live.slots",
                "blocked",
                "500 managed slots are not available",
                "repair the managed installation before activation",
            ),
        }
        self.deployment = Some(deployment);
    }
    pub fn inspect_input(&mut self, input: InputObservation) {
        if input.input_blocked == Some(true) {
            self.add(
                "live.input",
                "blocked",
                &input.reason,
                "resolve the reported input condition, then resume; no Esc is sent",
            );
        } else if let Some(InputBindings { wake, reload, .. }) = &input.bindings {
            self.add(
                "live.input",
                "ok",
                format!("execute {wake}, reload {reload}"),
                "",
            );
        } else {
            self.add(
                "live.input",
                "blocked",
                "live.channel_input_state_invalid",
                "wait for a fresh input sample; check bindings and bridge state",
            );
        }
        if let Some(attempt) = &input.input_attempt {
            if matches!(
                attempt.stage.as_str(),
                "load_failed" | "no_dispatch" | "exception" | "rejected"
            ) {
                let next = if attempt.reason == "slot_addon_disabled" {
                    "enable the reported slot in the selected character's addon settings, reload once, then resume the original CON; do not replay submitted input"
                } else {
                    "inspect the latest callback failure and original CON; diagnosis does not replay input"
                };
                self.add(
                    "live.input_attempt",
                    "error",
                    format!(
                        "latest callback {} at slot {}: {}",
                        attempt.stage, attempt.slot, attempt.reason
                    ),
                    next,
                );
            }
        }
        self.input = Some(input);
    }
}
/// Read-only capability surface. No input/publish method can be called even by
/// accident. Implementations own their diagnostic reader and checked-close it.
pub trait DoctorRuntime: RecordReader {
    fn uptime_millis(&self) -> i64;
    fn checked_close(&mut self) -> ChannelResult<()>;
}
fn selector(kind: Kind, runtime: super::Token) -> Selector {
    Selector {
        kind,
        runtime,
        nonce: if kind == Kind::Input {
            runtime
        } else {
            ZERO_TOKEN
        },
        ticket: ZERO_TOKEN,
        body: None,
    }
}
fn envelope(identity: &Identity) -> SlotEnvelope {
    SlotEnvelope {
        runtime: identity.runtime.clone(),
        owner: identity.owner.clone(),
        fence: identity.fence,
        index: identity.next_slot,
        guid: identity.guid.clone(),
        build: identity.build.clone(),
        action: "reload".into(),
        ..Default::default()
    }
}
/// Cross-check the published header and payload; the return type contains no
/// fresh INPUT witness or callback/slot authority.
pub fn decode_bootstrap(
    record: &super::Record<'_>,
    release: &str,
) -> ChannelResult<BootstrapState> {
    let boot: BootstrapState = serde_json::from_slice(record.payload)
        .map_err(|_| error("live.channel_bootstrap_invalid"))?;
    if record.header.kind != Kind::Bootstrap
        || boot.schema != SCHEMA
        || boot.bootstrap != hex::encode(record.header.runtime)
        || boot.release != release
    {
        return Err(error("live.channel_bootstrap_invalid"));
    }
    Ok(boot)
}
fn discover(
    runtime: &mut impl DoctorRuntime,
    control: &OperationControl,
    release: &str,
    character: &str,
    realm: &str,
) -> ChannelResult<Vec<Identity>> {
    let records = runtime.find(control, &selector(Kind::Identity, ZERO_TOKEN), None, false)?;
    let mut result = Vec::new();
    for record in records {
        let record = record
            .contract()
            .map_err(|e| ChannelError::External { causes: vec![e] })?;
        let identity = match decode_identity(&record, release)? {
            IdentityPublication::Current(i) | IdentityPublication::UpgradeCapability(i) => i,
            IdentityPublication::ManualActivationRequired(_) => {
                return Err(ChannelError::Joined {
                    causes: vec![
                        error("live.channel_mailbox_incompatible"),
                        error("live.channel_legacy_identity_requires_activation"),
                    ],
                });
            }
        };
        if (!character.is_empty() && identity.character != character)
            || (!realm.is_empty() && identity.realm != realm)
        {
            continue;
        }
        if !result
            .iter()
            .any(|i: &Identity| i.runtime == identity.runtime && i.next_slot == identity.next_slot)
        {
            result.push(identity);
        }
    }
    result.sort_by(|a, b| {
        b.runtime
            .cmp(&a.runtime)
            .then(b.next_slot.cmp(&a.next_slot))
    });
    Ok(result)
}
fn capability(
    runtime: &mut impl DoctorRuntime,
    identity: &Identity,
    control: &OperationControl,
) -> ChannelResult<bool> {
    if identity.input_state != PREVIOUS_SCHEMA {
        return Ok(identity.input_state == SCHEMA);
    }
    let records = runtime.find(
        control,
        &selector(Kind::Input, parse_token(&identity.runtime)?),
        None,
        true,
    )?;
    if records.len() != 1 {
        return Ok(false);
    }
    let record = records[0]
        .contract()
        .map_err(|e| ChannelError::External { causes: vec![e] })?;
    Ok(inspect_input(
        &record,
        &envelope(identity).expected(),
        0,
        runtime.uptime_millis(),
    )
    .is_ok_and(|s| s.has_two_key_reload_capability()))
}
fn observe_input(
    runtime: &mut impl DoctorRuntime,
    identity: &Identity,
    control: &OperationControl,
) -> ChannelResult<InputObservation> {
    check_control(control)?;
    let records = runtime.find(
        control,
        &selector(Kind::Input, parse_token(&identity.runtime)?),
        None,
        true,
    )?;
    check_control(control)?;
    if records.len() != 1 {
        return Err(pending());
    }
    let record = records[0]
        .contract()
        .map_err(|e| ChannelError::External { causes: vec![e] })?;
    decode_input(
        &record,
        &envelope(identity).expected(),
        0,
        runtime.uptime_millis(),
    )
    .map_err(|e| {
        if e.0 == "live.channel_input_state_stale" {
            error("live.channel_input_observation_stale")
        } else {
            pending()
        }
    })
}
/// Diagnose a runtime using its current named reader. Caller owns target,
/// deployment and owner inspection. Every return attempts checked-close.
pub fn inspect_runtime(
    report: &mut DoctorResult,
    runtime: &mut impl DoctorRuntime,
    target: &ConnectionTarget,
    release: &str,
    character: &str,
    realm: &str,
    control: &OperationControl,
    scheduler: &mut impl ContinuationScheduler,
) {
    inspect_runtime_open(
        report, runtime, target, release, character, realm, control, scheduler,
    );
    if let Err(e) = runtime.checked_close() {
        report.failure(
            "live.memory",
            "error",
            e,
            "retry diagnosis for the selected client",
        );
    }
}
fn inspect_runtime_open(
    report: &mut DoctorResult,
    runtime: &mut impl DoctorRuntime,
    target: &ConnectionTarget,
    release: &str,
    character: &str,
    realm: &str,
    control: &OperationControl,
    scheduler: &mut impl ContinuationScheduler,
) {
    match runtime.find(control, &selector(Kind::Bootstrap, ZERO_TOKEN), None, true) {
        Ok(records) if records.len() == 1 => {
            let boot = records[0]
                .contract()
                .map_err(|e| ChannelError::External { causes: vec![e] })
                .and_then(|r| decode_bootstrap(&r, release));
            match boot {
                Ok(boot) => {
                    if boot.phase == "failed" {
                        report.add("live.bootstrap", "blocked", &boot.reason, "inspect the addon startup failure; the manual reload binding remains available");
                    } else {
                        report.add("live.bootstrap", "ok", &boot.phase, "");
                    }
                    report.bootstrap = Some(boot);
                }
                Err(e) => {
                    report.add(
                        "live.bootstrap",
                        "error",
                        "invalid bootstrap publication",
                        "install the current addon and reload the selected client",
                    );
                    report.errors.push(e);
                }
            }
        }
        other => {
            report.add(
                "live.bootstrap",
                "warn",
                "bootstrap diagnostic is not loaded",
                "install the current addon and reload the selected client",
            );
            if let Err(e) = other {
                report.errors.push(e);
            }
        }
    }
    let candidates = match discover(runtime, control, release, character, realm) {
        Ok(c) => c,
        Err(e) => {
            report.failure(
                "live.runtime",
                "blocked",
                e,
                "reload the selected client manually after verifying the installation",
            );
            return;
        }
    };
    let Some(identity) = candidates.into_iter().next() else {
        report.add(
            "live.runtime",
            "blocked",
            "no current runtime publication",
            "enter the world, enable the addon, then /reload once; resume the same CON if present",
        );
        return;
    };
    report.identity = Some(identity.clone());
    if identity.build != target.client.full_build || identity.product != target.client.product {
        report.add(
            "live.runtime",
            "error",
            "runtime and installation identity differ",
            "select the matching installation and PID",
        );
        return;
    }
    let can_reload = match capability(runtime, &identity, control) {
        Ok(v) => v,
        Err(e) => {
            report.failure(
                "live.runtime",
                "blocked",
                e,
                "retry diagnosis for the selected client",
            );
            return;
        }
    };
    if !can_reload || identity.slots != SLOT_COUNT {
        report.add(
            "live.runtime",
            "blocked",
            "older runtime requires activation",
            "reload the selected client manually; old input protocols do not authorize keys",
        );
        return;
    }
    if identity.input_state == PREVIOUS_SCHEMA {
        report.add(
            "live.runtime",
            "blocked",
            "loaded runtime needs the protocol-name update",
            "live doctor --fix reloads once using its verified two-key INPUT",
        );
    } else {
        report.add("live.runtime", "ok", &identity.runtime, "");
    }
    if identity.inventory != Some(SLOT_COUNT) {
        report.add(
            "live.inventory",
            "blocked",
            "client has not discovered all 500 slots",
            "restart the selected client once after installation",
        );
    } else {
        report.add(
            "live.inventory",
            "ok",
            "500 discovered slot metadata entries; enabled/loadable state is not checked",
            "",
        );
    }
    let sampled = match control.ordered_child(Instant::now() + INPUT_SAMPLE_LIMIT) {
        Ok(c) => c,
        Err(e) => {
            report.failure(
                "live.input",
                "blocked",
                e.into(),
                "wait for a fresh input sample; check bindings and bridge state",
            );
            return;
        }
    };
    let start = scheduler.monotonic_millis();
    loop {
        let observed = observe_input(runtime, &identity, &sampled);
        match observed {
            Ok(input) => { report.inspect_input(input); return; }
            Err(e) if e.all_causes(&|e| matches!(e, ChannelError::Pending { causes } if causes.is_empty())
                || matches!(e, ChannelError::Contract(c) if c.0 == "live.channel_input_observation_stale")) => (),
            Err(e) => { report.failure("live.input", "blocked", e, "wait for a fresh input sample; check bindings and bridge state"); return; }
        }
        if scheduler.monotonic_millis().saturating_sub(start) >= 1500 {
            report.failure(
                "live.input",
                "blocked",
                ChannelError::DeadlineExceeded,
                "wait for a fresh input sample; check bindings and bridge state",
            );
            return;
        }
        if let Err(e) = scheduler.wait(&sampled, Duration::from_millis(25)) {
            report.failure(
                "live.input",
                "blocked",
                e,
                "wait for a fresh input sample; check bindings and bridge state",
            );
            return;
        }
    }
}
#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;
