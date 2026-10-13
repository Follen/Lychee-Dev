//! One persisted lifecycle from activation to close. Observe after uncertain input; never replay.
use crate::slots::Instance;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    Activating,
    ActivationAwaiting,
    Opening,
    OpenPublished,
    OpenAwaiting,
    Idle,
    Prepared,
    ExecutePublished,
    Executing,
    ResultObserved,
    ResultDurable,
    SettlePublished,
    SettleAwaiting,
    Settled,
    Closing,
    ClosePublished,
    CloseAwaiting,
    Closed,
    Unknown,
    Reloading,
    ReloadAwaiting,
    Cancelled,
    Abandoned,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reload {
    pub request: String,
    pub from_runtime: String,
    pub to_runtime: Option<String>,
    pub samples: [u32; 2],
    pub deadline_millis: i64,
    pub original_phase: Phase,
    pub original_action: Option<Action>,
    pub original_messages: Option<u32>,
    pub close_first: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub sequence: u64,
    pub digest: String,
    pub code: String,
    pub issued_runtime: String,
    pub budget_seconds: u16,
    pub expires_at: u64,
    // Observed bytes are transient. Crash recovery rereads the game's retained BODY;
    // the durable result lives in its separate checked file, never a JSON byte array.
    #[serde(skip)]
    pub result: Option<Vec<u8>>,
    pub result_digest: Option<String>,
    #[serde(default)]
    pub result_bytes: u32,
    #[serde(default)]
    pub result_checksum: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityRotation {
    pub next: Request,
    pub next_epoch: String,
    pub deadline_millis: i64,
    pub reopening: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub schema: String,
    pub instance: Instance,
    pub actor: String,
    pub epoch: String,
    pub owner: String,
    pub fence: u64,
    pub phase: Phase,
    pub sequence: u64,
    pub request: Option<Request>,
    pub pending_action: Option<Action>,
    pub input_messages: Option<u32>,
    pub cleanup_complete: bool,
    #[serde(default)]
    pub transport: serde_json::Value,
    #[serde(default)]
    pub reload: Option<Reload>,
    #[serde(default)]
    pub execution_unknown: bool,
    #[serde(default)]
    pub termination: Option<Termination>,
    #[serde(default)]
    pub capacity_rotation: Option<CapacityRotation>,
    #[serde(default)]
    pub capacity_history: Vec<Reload>,
}
/// Host termination preserves the original uncertain effect; it never confirms a game ACK.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Termination {
    pub original_phase: Phase,
    pub original_action: Option<Action>,
    pub original_messages: Option<u32>,
}
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Activate,
    Open,
    PublishOpen,
    PublishExecute,
    InputExecute,
    PersistResult,
    PublishSettle,
    InputSettle,
    Observe,
    Close,
    PublishClose,
    Reload,
}
#[derive(Debug, Clone)]
pub enum Observation {
    None,
    Activated {
        runtime: String,
    },
    RuntimeChanged(String),
    Opened,
    Accepted {
        request: String,
        digest: String,
    },
    Reported {
        request: String,
        digest: String,
        body: Vec<u8>,
    },
    Settled {
        request: String,
        digest: String,
        resources_released: bool,
        display_cleared: bool,
    },
    Closed,
    ProcessExited,
    Reloaded {
        runtime: String,
        samples: [u32; 2],
    },
}
pub trait Store {
    fn checkpoint(&mut self, session: &Session) -> Result<()>;
    fn persist_result(&mut self, request: &Request, body: &[u8]) -> Result<String>;
    fn verify_result(&mut self, request: &Request) -> Result<()>;
    fn reactivate(&mut self) -> Result<()> {
        Err("live.store_reactivation_required".into())
    }
}
pub trait Transport {
    fn observe(&mut self, session: &Session) -> Result<Observation>;
    fn act(&mut self, session: &Session, action: &Action) -> Result<ActionReceipt>;
    fn wait_event(&mut self, _: std::time::Duration) -> Result<bool> {
        Ok(false)
    }
    fn prepare_request(&mut self, _: &mut Session) -> Result<()> {
        Ok(())
    }
    fn prepare_connection(&mut self, _: &mut Session) -> Result<()> {
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct ActionReceipt {
    pub queued: u32,
    pub complete: bool,
    pub facts: serde_json::Value,
}
impl Session {
    pub fn new(instance: Instance, actor: String, epoch: String, owner: String) -> Self {
        Self {
            schema: "lycheedev.lifecycle.v2".into(),
            instance,
            actor,
            epoch,
            owner,
            fence: 1,
            phase: Phase::Activating,
            sequence: 0,
            request: None,
            pending_action: None,
            input_messages: None,
            cleanup_complete: false,
            transport: serde_json::json!({}),
            reload: None,
            execution_unknown: false,
            termination: None,
            capacity_rotation: None,
            capacity_history: Vec::new(),
        }
    }
    pub fn prepare(
        &mut self,
        id: String,
        code: String,
        budget: u16,
        expires_at: u64,
    ) -> Result<()> {
        if self.capacity_rotation.is_some() {
            return Err("live.capacity_rotation_pending".into());
        }
        if self.phase == Phase::Settled && self.cleanup_complete {
            self.request = None;
            self.phase = Phase::Idle;
            self.cleanup_complete = false;
            if let Some(meta) = self.transport.as_object_mut() {
                meta.remove("executeExchange");
                meta.remove("settleExchange");
            }
        }
        if self.phase != Phase::Idle || self.request.is_some() {
            return Err("live.session_busy".into());
        }
        if !(1..=120).contains(&budget)
            || code.is_empty()
            || code.len() > 262144
            || code.as_bytes()[0] == 27
        {
            return Err("live.request_invalid".into());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("live.sequence_exhausted")?;
        self.request = Some(Request {
            id,
            sequence: self.sequence,
            digest: hex::encode(Sha256::digest(code.as_bytes())),
            code,
            issued_runtime: self.instance.runtime.clone(),
            budget_seconds: budget,
            expires_at,
            result: None,
            result_digest: None,
            result_bytes: 0,
            result_checksum: 0,
        });
        self.phase = Phase::Prepared;
        self.execution_unknown = false;
        self.pending_action = None;
        self.input_messages = None;
        self.termination = None;
        Ok(())
    }
    pub fn needs_capacity_rotation(&self) -> bool {
        self.sequence >= 28
    }
    /// Freeze the next immutable operation before any automatic close/reload.
    pub fn queue_capacity(
        &mut self,
        id: String,
        code: String,
        budget: u16,
        next_epoch: String,
        deadline_millis: i64,
    ) -> Result<()> {
        if !self.needs_capacity_rotation()
            || self.capacity_history.len() >= 128
            || self.phase != Phase::Settled
            || !self.cleanup_complete
            || self.pending_action.is_some()
            || self.capacity_rotation.is_some()
            || next_epoch == self.epoch
            || next_epoch.len() != 32
        {
            return Err("live.capacity_rotation_admission".into());
        }
        if !(1..=120).contains(&budget)
            || code.is_empty()
            || code.len() > 262144
            || code.as_bytes()[0] == 27
        {
            return Err("live.request_invalid".into());
        }
        self.capacity_rotation = Some(CapacityRotation {
            next: Request {
                id,
                sequence: 0,
                digest: hex::encode(Sha256::digest(code.as_bytes())),
                code,
                issued_runtime: self.instance.runtime.clone(),
                budget_seconds: budget,
                expires_at: u64::MAX,
                result: None,
                result_digest: None,
                result_bytes: 0,
                result_checksum: 0,
            },
            next_epoch,
            deadline_millis,
            reopening: false,
        });
        Ok(())
    }
    fn reopen_capacity(&mut self, store: &mut impl Store) -> Result<()> {
        let rotation = self
            .capacity_rotation
            .as_mut()
            .ok_or("live.capacity_rotation_missing")?;
        let reload = self.reload.as_ref().ok_or("live.capacity_reload_missing")?;
        let runtime = reload
            .to_runtime
            .as_ref()
            .ok_or("live.capacity_reload_unproven")?;
        if self.phase != Phase::Closed
            || !self.cleanup_complete
            || self.execution_unknown
            || runtime == &self.instance.runtime
            || reload.samples[0] == 0
            || reload.samples[1] <= reload.samples[0]
        {
            return Err("live.capacity_reload_unproven".into());
        }
        self.capacity_history.push(reload.clone());
        self.instance.runtime = runtime.clone();
        self.epoch = rotation.next_epoch.clone();
        self.fence = self.fence.checked_add(1).ok_or("live.fence_exhausted")?;
        self.sequence = 0;
        self.request = None;
        self.pending_action = None;
        self.input_messages = None;
        self.cleanup_complete = false;
        self.reload = None;
        self.phase = Phase::Opening;
        self.transport = serde_json::json!({});
        rotation.reopening = true;
        store.checkpoint(self)?;
        store.reactivate()
    }
    pub fn request_close(&mut self) -> Result<()> {
        if !matches!(self.phase, Phase::Idle | Phase::Settled)
            || (self.phase == Phase::Settled && !self.cleanup_complete)
        {
            return Err("live.pending_requires_recovery".into());
        }
        self.phase = Phase::Closing;
        Ok(())
    }
    pub fn cancel(&mut self) -> Result<()> {
        if self
            .capacity_rotation
            .as_ref()
            .is_some_and(|r| !r.reopening)
            && self.reload.is_none()
            && self.pending_action.is_none()
        {
            self.request = Some(self.capacity_rotation.take().unwrap().next);
            self.phase = Phase::Prepared;
            self.input_messages = None;
        }
        if self.phase == Phase::Cancelled {
            return Ok(());
        }
        if self.phase != Phase::Prepared
            || self.request.is_none()
            || self.pending_action.is_some()
            || self.input_messages.is_some()
            || !self.transport["executeExchange"].is_null()
        {
            return Err("live.cancel_requires_unpublished_prepared".into());
        }
        self.terminate(Phase::Cancelled);
        self.execution_unknown = false;
        Ok(())
    }
    pub fn abandon(&mut self) -> Result<()> {
        if self.phase == Phase::Abandoned {
            return Ok(());
        }
        if matches!(
            self.phase,
            Phase::Idle | Phase::Prepared | Phase::Cancelled | Phase::Closed
        ) || (self.phase == Phase::Settled && self.cleanup_complete)
        {
            return Err("live.abandon_requires_unresolved".into());
        }
        self.execution_unknown = self
            .request
            .as_ref()
            .is_some_and(|r| r.result_digest.is_none());
        self.terminate(Phase::Abandoned);
        Ok(())
    }
    fn terminate(&mut self, phase: Phase) {
        self.termination = Some(Termination {
            original_phase: self.phase.clone(),
            original_action: self.pending_action.clone(),
            original_messages: self.input_messages,
        });
        self.phase = phase;
        // Host ownership retires separately. Neither flag asserts runtime cleanup.
        self.cleanup_complete = false;
    }
    pub fn request_reload(&mut self, request: String, deadline_millis: i64) -> Result<()> {
        if let Some(reload) = &self.reload {
            if reload.request == request {
                return Ok(());
            }
            if reload.to_runtime.is_none() {
                return Err("live.reload_pending".into());
            }
            return Err("live.reload_fresh_connection_required".into());
        }
        let close_first =
            self.phase == Phase::Idle || (self.phase == Phase::Settled && self.cleanup_complete);
        self.execution_unknown = self.phase != Phase::Cancelled
            && self
                .request
                .as_ref()
                .is_some_and(|r| r.result_digest.is_none());
        self.reload = Some(Reload {
            request,
            from_runtime: self.instance.runtime.clone(),
            to_runtime: None,
            samples: [0, 0],
            deadline_millis,
            original_phase: self.phase.clone(),
            original_action: self.pending_action.clone(),
            original_messages: self.input_messages,
            close_first,
        });
        self.phase = if close_first {
            Phase::Closing
        } else {
            Phase::Reloading
        };
        self.pending_action = None;
        self.input_messages = None;
        Ok(())
    }
    /// An explicit reload may destroy the old runtime. Observe and persist any
    /// already available exact report first, without dispatching business/input.
    pub fn capture_available_report(
        &mut self,
        store: &mut impl Store,
        transport: &mut (impl Transport + ?Sized),
    ) -> Result<()> {
        if let Some(r) = &self.request
            && r.result_digest.is_some()
        {
            return store.verify_result(r);
        }
        if self.request.is_none() {
            return Ok(());
        }
        match transport.observe(self) {
            Ok(observation @ Observation::Reported { .. }) => {
                self.reconcile(observation)?;
                let r = self.request.as_ref().ok_or("live.request_missing")?;
                let body = r.result.as_ref().ok_or("live.result_missing")?;
                let digest = store.persist_result(r, body)?;
                if digest != lycheedev_core::storage::sha256(body) {
                    return Err("live.result_digest".into());
                }
                self.request.as_mut().unwrap().result_digest = Some(digest);
                self.phase = Phase::ResultDurable;
                store.checkpoint(self)
            }
            Err(reason)
                if reason.contains("actor_changed") || reason.contains("target_changed") =>
            {
                Err(reason)
            }
            Err(reason) => {
                self.transport["reloadReportObservationError"] = serde_json::json!(reason);
                Ok(())
            }
            _ => Ok(()),
        }
    }
    /// A single step persists intent before every externally visible effect.
    pub fn advance(
        &mut self,
        store: &mut impl Store,
        transport: &mut (impl Transport + ?Sized),
        now: u64,
    ) -> Result<Action> {
        if self.schema != "lycheedev.lifecycle.v2" {
            return Err("live.journal_version".into());
        }
        if let Some(rotation) = &self.capacity_rotation
            && !rotation.reopening
            && self.reload.is_none()
            && self.phase == Phase::Settled
        {
            let request = hex::encode(Sha256::digest(
                format!("{}:capacity:{}", self.owner, rotation.next.id).as_bytes(),
            ))[..32]
                .to_owned();
            let deadline = rotation.deadline_millis;
            self.request_reload(request, deadline)?;
            store.checkpoint(self)?;
        }
        if self.capacity_rotation.is_some() && self.phase == Phase::Closed {
            self.reopen_capacity(store)?;
        }
        if matches!(self.phase, Phase::Cancelled | Phase::Abandoned) {
            store.checkpoint(self)?;
            return Ok(Action::Observe);
        }
        let observation = transport.observe(self)?;
        self.reconcile(observation)?;
        store.checkpoint(self)?;
        if self.capacity_rotation.is_some() && self.phase == Phase::Closed {
            self.reopen_capacity(store)?;
        }
        if self.phase == Phase::Idle && self.capacity_rotation.as_ref().is_some_and(|r| r.reopening)
        {
            let next = self.capacity_rotation.take().unwrap().next;
            self.prepare(next.id, next.code, next.budget_seconds, u64::MAX)?;
            transport.prepare_request(self)?;
            store.checkpoint(self)?;
        }
        if let Some(action) = self.pending_action.clone() {
            // A checkpoint without delivery receipt has unknown effect. Observe the original action.
            if matches!(
                action,
                Action::InputExecute
                    | Action::InputSettle
                    | Action::Activate
                    | Action::Open
                    | Action::Close
                    | Action::Reload
            ) {
                return Ok(Action::Observe);
            }
        }
        let action = match self.phase {
            Phase::Activating => Action::Activate,
            Phase::Opening => Action::PublishOpen,
            Phase::OpenPublished => Action::Open,
            Phase::Idle
            | Phase::ActivationAwaiting
            | Phase::OpenAwaiting
            | Phase::Executing
            | Phase::SettleAwaiting
            | Phase::CloseAwaiting
            | Phase::Settled
            | Phase::Closed
            | Phase::Unknown => Action::Observe,
            Phase::Cancelled | Phase::Abandoned => Action::Observe,
            Phase::ReloadAwaiting => Action::Observe,
            Phase::Reloading => Action::Reload,
            Phase::Prepared => {
                if self.request.as_ref().unwrap().expires_at <= now {
                    return Err("live.request_expired".into());
                }
                Action::PublishExecute
            }
            Phase::ExecutePublished => Action::InputExecute,
            Phase::ResultObserved => Action::PersistResult,
            Phase::ResultDurable => Action::PublishSettle,
            Phase::SettlePublished => Action::InputSettle,
            Phase::Closing => Action::PublishClose,
            Phase::ClosePublished => Action::Close,
        };
        if action == Action::Observe {
            return Ok(action);
        }
        self.pending_action = Some(action.clone());
        self.input_messages = None;
        store.checkpoint(self)?;
        if action == Action::PersistResult {
            let r = self.request.as_ref().ok_or("live.request_missing")?;
            let body = r.result.as_ref().ok_or("live.result_missing")?;
            let digest = store.persist_result(r, body)?;
            if digest != hex::encode(Sha256::digest(body)) {
                return Err("live.result_digest".into());
            }
            self.request.as_mut().unwrap().result_digest = Some(digest);
            self.phase = Phase::ResultDurable;
        } else {
            if action == Action::PublishSettle {
                store.verify_result(self.request.as_ref().ok_or("live.request_missing")?)?;
            }
            let receipt = transport.act(self, &action)?;
            if let Some(patch) = receipt.facts.as_object() {
                if !self.transport.is_object() {
                    self.transport = serde_json::json!({})
                }
                for (k, v) in patch {
                    self.transport[k] = v.clone();
                }
            }
            self.input_messages = Some(receipt.queued);
            if !receipt.complete {
                store.checkpoint(self)?;
                return Ok(Action::Observe);
            }
            self.phase = match action {
                Action::Activate => Phase::ActivationAwaiting,
                Action::Reload => Phase::ReloadAwaiting,
                Action::Open => Phase::OpenAwaiting,
                Action::PublishOpen => Phase::OpenPublished,
                Action::PublishExecute => Phase::ExecutePublished,
                Action::InputExecute => Phase::Executing,
                Action::PublishSettle => Phase::SettlePublished,
                Action::InputSettle => Phase::SettleAwaiting,
                Action::Close => Phase::CloseAwaiting,
                Action::PublishClose => Phase::ClosePublished,
                _ => self.phase.clone(),
            };
        }
        self.pending_action = None;
        store.checkpoint(self)?;
        Ok(action)
    }
    fn reconcile(&mut self, observation: Observation) -> Result<()> {
        match observation {
            Observation::Reloaded { runtime, samples } => {
                let reload = self.reload.as_mut().ok_or("live.reload_unsolicited")?;
                if !matches!(self.phase, Phase::Reloading | Phase::ReloadAwaiting)
                    || runtime == reload.from_runtime
                    || samples[0] == 0
                    || samples[1] <= samples[0]
                {
                    return Err("live.reload_unproven".into());
                }
                reload.to_runtime = Some(runtime);
                reload.samples = samples;
                self.phase = Phase::Closed;
                self.cleanup_complete = true;
                self.pending_action = None;
            }
            Observation::Activated { runtime } => {
                if matches!(self.phase, Phase::Activating | Phase::ActivationAwaiting)
                    && self.request.is_none()
                {
                    self.instance.runtime = runtime;
                    self.phase = Phase::Opening;
                    self.pending_action = None;
                }
            }
            Observation::RuntimeChanged(runtime) => {
                if runtime != self.instance.runtime {
                    if matches!(self.phase, Phase::Activating | Phase::ActivationAwaiting)
                        && self.request.is_none()
                    {
                        self.instance.runtime = runtime;
                        self.phase = Phase::Opening;
                    } else {
                        self.phase = Phase::Unknown;
                    }
                    self.pending_action = None;
                }
            }
            Observation::ProcessExited => {
                if self
                    .request
                    .as_ref()
                    .is_some_and(|r| r.result_digest.is_none())
                {
                    self.phase = Phase::Unknown;
                } else {
                    self.phase = Phase::Closed;
                    self.cleanup_complete = true;
                }
                self.pending_action = None;
            }
            Observation::Opened => {
                if matches!(
                    self.phase,
                    Phase::Opening | Phase::OpenPublished | Phase::OpenAwaiting
                ) {
                    self.phase = Phase::Idle;
                    self.pending_action = None;
                }
            }
            Observation::Accepted { request, digest } => {
                self.exact(&request, &digest)?;
                self.phase = Phase::Executing;
                self.pending_action = None;
            }
            Observation::Reported {
                request,
                digest,
                body,
            } => {
                self.exact(&request, &digest)?;
                if !matches!(
                    self.phase,
                    Phase::ResultDurable
                        | Phase::SettlePublished
                        | Phase::SettleAwaiting
                        | Phase::Settled
                ) {
                    self.request.as_mut().unwrap().result = Some(body);
                    let r = self.request.as_mut().unwrap();
                    let body = r.result.as_ref().unwrap();
                    r.result_bytes = body.len() as u32;
                    r.result_checksum = crate::mailbox::checksum(body);
                    self.phase = Phase::ResultObserved;
                    self.pending_action = None;
                }
            }
            Observation::Settled {
                request,
                digest,
                resources_released,
                display_cleared,
            } => {
                self.exact(&request, &digest)?;
                if matches!(
                    self.phase,
                    Phase::Closing | Phase::ClosePublished | Phase::CloseAwaiting | Phase::Closed
                ) {
                    return Ok(());
                }
                if self.request.as_ref().unwrap().result_digest.is_none() {
                    return Err("live.settle_without_persistence".into());
                }
                self.phase = Phase::Settled;
                self.cleanup_complete = resources_released && display_cleared;
                self.pending_action = None;
            }
            Observation::Closed => {
                if !matches!(
                    self.phase,
                    Phase::Closing | Phase::ClosePublished | Phase::CloseAwaiting
                ) {
                    return Err("live.unsolicited_close".into());
                }
                self.phase = if self
                    .reload
                    .as_ref()
                    .is_some_and(|r| r.to_runtime.is_none() && r.close_first)
                {
                    Phase::Reloading
                } else {
                    Phase::Closed
                };
                self.cleanup_complete = true;
                self.pending_action = None;
            }
            Observation::None => {}
        }
        Ok(())
    }
    fn exact(&self, id: &str, digest: &str) -> Result<()> {
        let r = self.request.as_ref().ok_or("live.unsolicited_receipt")?;
        if r.id != id || r.digest != digest || r.issued_runtime != self.instance.runtime {
            return Err("live.receipt_mismatch".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct S {
        saved: Vec<Session>,
        result: Option<String>,
        fail: bool,
    }
    impl Store for S {
        fn reactivate(&mut self) -> Result<()> {
            Ok(())
        }
        fn checkpoint(&mut self, s: &Session) -> Result<()> {
            self.saved.push(s.clone());
            Ok(())
        }
        fn persist_result(&mut self, _: &Request, b: &[u8]) -> Result<String> {
            if self.fail {
                return Err("disk.full".into());
            }
            let d = hex::encode(Sha256::digest(b));
            self.result = Some(d.clone());
            Ok(d)
        }
        fn verify_result(&mut self, r: &Request) -> Result<()> {
            if self.result.as_ref() != r.result_digest.as_ref() {
                return Err("disk.result_missing".into());
            }
            Ok(())
        }
    }
    #[derive(Default)]
    struct T {
        obs: Option<Observation>,
        actions: Vec<Action>,
        fail: bool,
    }
    impl Transport for T {
        fn observe(&mut self, _: &Session) -> Result<Observation> {
            Ok(self.obs.take().unwrap_or(Observation::None))
        }
        fn act(&mut self, _: &Session, a: &Action) -> Result<ActionReceipt> {
            self.actions.push(a.clone());
            if self.fail {
                return Err("input.unknown".into());
            }
            Ok(ActionReceipt {
                queued: 1,
                complete: true,
                facts: serde_json::Value::Null,
            })
        }
    }
    fn session() -> Session {
        let mut s = Session::new(
            Instance {
                pid: 1,
                created: 2,
                module_sha256: "hash".into(),
                runtime: "runtime".into(),
            },
            "actor".into(),
            "epoch".into(),
            "owner".into(),
        );
        s.phase = Phase::Idle;
        s.prepare("r".into(), "return 1".into(), 30, 100).unwrap();
        s
    }
    #[test]
    fn durable_result_precedes_settle() {
        let (mut s, mut store, mut t) = (session(), S::default(), T::default());
        assert_eq!(
            s.advance(&mut store, &mut t, 1).unwrap(),
            Action::PublishExecute
        );
        s.advance(&mut store, &mut t, 1).unwrap();
        let r = s.request.as_ref().unwrap();
        t.obs = Some(Observation::Reported {
            request: r.id.clone(),
            digest: r.digest.clone(),
            body: b"result".to_vec(),
        });
        store.fail = true;
        assert!(s.advance(&mut store, &mut t, 1).is_err());
        assert!(!t.actions.contains(&Action::PublishSettle));
        store.fail = false;
        s.advance(&mut store, &mut t, 1).unwrap();
        assert_eq!(s.phase, Phase::ResultDurable);
        s.advance(&mut store, &mut t, 1).unwrap();
        assert!(t.actions.contains(&Action::PublishSettle));
    }
    #[test]
    fn capacity_freezes_next_then_close_reload_reopen_without_replaying_business() {
        let (mut s, mut store, mut t) = (session(), S::default(), T::default());
        s.phase = Phase::Settled;
        s.cleanup_complete = true;
        s.sequence = 28;
        s.request.as_mut().unwrap().result_digest = Some("old verified".into());
        let old = s.instance.clone();
        let old_actor = s.actor.clone();
        s.queue_capacity(
            "next".into(),
            "return 2".into(),
            17,
            "e".repeat(32),
            i64::MAX,
        )
        .unwrap();
        assert_eq!(s.phase, Phase::Settled);
        assert_eq!(s.request.as_ref().unwrap().id, "r");
        assert_eq!(
            s.advance(&mut store, &mut t, 0).unwrap(),
            Action::PublishClose
        );
        s.advance(&mut store, &mut t, 0).unwrap();
        t.obs = Some(Observation::Closed);
        assert_eq!(s.advance(&mut store, &mut t, 0).unwrap(), Action::Reload);
        let keys = t.actions.len();
        s.advance(&mut store, &mut t, 0).unwrap();
        assert_eq!(t.actions.len(), keys);
        t.obs = Some(Observation::Reloaded {
            runtime: "new".into(),
            samples: [1, 2],
        });
        assert_eq!(
            s.advance(&mut store, &mut t, 0).unwrap(),
            Action::PublishOpen
        );
        assert_eq!(s.instance.pid, old.pid);
        assert_eq!(s.instance.created, old.created);
        assert_eq!(s.actor, old_actor);
        assert_eq!(s.instance.runtime, "new");
        assert_eq!(s.epoch, "e".repeat(32));
        assert_eq!(s.fence, 2);
        assert_eq!(s.capacity_history.len(), 1);
        assert!(s.capacity_rotation.as_ref().unwrap().reopening);
        s.advance(&mut store, &mut t, 0).unwrap();
        t.obs = Some(Observation::Opened);
        assert_eq!(
            s.advance(&mut store, &mut t, 0).unwrap(),
            Action::PublishExecute
        );
        let request = s.request.as_ref().unwrap();
        assert_eq!(
            (
                &request.id,
                request.budget_seconds,
                request.sequence,
                request.issued_runtime.as_str()
            ),
            (&"next".into(), 17, 1, "new")
        );
        assert!(s.capacity_rotation.is_none());
        assert!(!t.actions.contains(&Action::InputExecute));
    }
    #[test]
    fn capacity_denies_unclean_unknown_and_unproven_replacement() {
        let mut s = session();
        s.sequence = 28;
        assert!(
            s.queue_capacity("n".into(), "return 1".into(), 30, "e".repeat(32), i64::MAX)
                .is_err()
        );
        s.phase = Phase::Settled;
        s.cleanup_complete = true;
        s.pending_action = Some(Action::InputSettle);
        assert!(
            s.queue_capacity("n".into(), "return 1".into(), 30, "e".repeat(32), i64::MAX)
                .is_err()
        );
        s.pending_action = None;
        s.queue_capacity("n".into(), "return 1".into(), 30, "e".repeat(32), i64::MAX)
            .unwrap();
        s.phase = Phase::Closed;
        assert!(s.reopen_capacity(&mut S::default()).is_err());
    }
    #[test]
    fn unknown_input_is_observed_never_replayed() {
        let (mut s, mut store, mut t) = (session(), S::default(), T::default());
        s.advance(&mut store, &mut t, 1).unwrap();
        t.fail = true;
        assert!(s.advance(&mut store, &mut t, 1).is_err());
        let before = t.actions.len();
        s.advance(&mut store, &mut t, 1).unwrap();
        assert_eq!(before, t.actions.len());
    }
    #[test]
    fn cancel_requires_absence_of_publication_intent_even_when_zero_messages() {
        let mut s = session();
        s.pending_action = Some(Action::PublishExecute);
        assert!(s.cancel().is_err());
        s.pending_action = None;
        s.input_messages = Some(0);
        assert!(s.cancel().is_err());
        s.input_messages = None;
        s.cancel().unwrap();
        s.cancel().unwrap();
        assert_eq!(s.phase, Phase::Cancelled);
        assert!(!s.execution_unknown);
        assert!(!s.cleanup_complete);
    }
    #[test]
    fn abandonment_retains_uncertain_effect_and_terminal_resume_has_no_transport_call() {
        let mut s = session();
        assert!(s.abandon().is_err());
        s.phase = Phase::ExecutePublished;
        s.pending_action = Some(Action::InputExecute);
        s.input_messages = Some(2);
        s.abandon().unwrap();
        let original = s.termination.as_ref().unwrap();
        assert_eq!(original.original_action, Some(Action::InputExecute));
        assert_eq!(original.original_messages, Some(2));
        assert!(s.execution_unknown);
        assert!(!s.cleanup_complete);
        s.abandon().unwrap();
        let (mut store, mut t) = (S::default(), T::default());
        t.obs = Some(Observation::RuntimeChanged("changed".into()));
        s.advance(&mut store, &mut t, 0).unwrap();
        assert!(t.obs.is_some());
        assert!(t.actions.is_empty());
        assert_eq!(s.phase, Phase::Abandoned);
    }
    #[test]
    fn runtime_change_preserves_uncertainty() {
        let (mut s, mut store, mut t) = (session(), S::default(), T::default());
        s.advance(&mut store, &mut t, 1).unwrap();
        t.obs = Some(Observation::RuntimeChanged("other".into()));
        s.advance(&mut store, &mut t, 1).unwrap();
        assert_eq!(s.phase, Phase::Unknown);
        assert!(!s.cleanup_complete);
    }
    #[test]
    fn foreign_receipt_not_accepted() {
        let (mut s, mut store, mut t) = (session(), S::default(), T::default());
        t.obs = Some(Observation::Accepted {
            request: "foreign".into(),
            digest: s.request.as_ref().unwrap().digest.clone(),
        });
        assert!(s.advance(&mut store, &mut t, 1).is_err());
    }
    #[test]
    fn settled_with_pending_cleanup_cannot_close() {
        let mut s = session();
        s.phase = Phase::Settled;
        s.cleanup_complete = false;
        assert_eq!(
            s.request_close().unwrap_err(),
            "live.pending_requires_recovery"
        );
        assert_eq!(s.phase, Phase::Settled);
        s.cleanup_complete = true;
        s.request_close().unwrap();
        assert_eq!(s.phase, Phase::Closing);
    }
    #[test]
    fn unknown_reload_is_not_replayed_and_needs_two_fresh_samples() {
        let (mut s, mut store, mut transport) = (session(), S::default(), T::default());
        s.request = None;
        s.phase = Phase::Closed;
        s.cleanup_complete = true;
        s.request_reload("reload".into(), i64::MAX).unwrap();
        transport.fail = true;
        assert!(s.advance(&mut store, &mut transport, 0).is_err());
        let sent = transport.actions.len();
        s.advance(&mut store, &mut transport, 0).unwrap();
        assert_eq!(sent, transport.actions.len());
        transport.obs = Some(Observation::Reloaded {
            runtime: "new".into(),
            samples: [2, 2],
        });
        assert!(s.advance(&mut store, &mut transport, 0).is_err());
        transport.obs = Some(Observation::Reloaded {
            runtime: "new".into(),
            samples: [2, 3],
        });
        s.advance(&mut store, &mut transport, 0).unwrap();
        assert_eq!(s.phase, Phase::Closed);
        assert_eq!(
            s.reload.as_ref().unwrap().to_runtime.as_deref(),
            Some("new")
        );
        assert!(s.request_reload("reload".into(), i64::MAX).is_ok());
        assert!(s.request_reload("other".into(), i64::MAX).is_err());
    }
    #[test]
    fn idle_reload_closes_then_replaces_and_copies_no_business_input() {
        let (mut s, mut store, mut t) = (session(), S::default(), T::default());
        s.request = None;
        s.phase = Phase::Idle;
        s.request_reload("idle".into(), i64::MAX).unwrap();
        assert_eq!(s.phase, Phase::Closing);
        assert_eq!(
            s.advance(&mut store, &mut t, 0).unwrap(),
            Action::PublishClose
        );
        s.advance(&mut store, &mut t, 0).unwrap();
        t.obs = Some(Observation::Closed);
        assert_eq!(s.advance(&mut store, &mut t, 0).unwrap(), Action::Reload);
        assert_eq!(s.phase, Phase::ReloadAwaiting);
        assert!(!t.actions.contains(&Action::InputExecute));
    }
    #[test]
    fn explicit_reload_preserves_unknown_original_and_persists_available_report_first() {
        let mut s = session();
        s.phase = Phase::Executing;
        s.pending_action = Some(Action::InputExecute);
        s.input_messages = Some(2);
        s.request_reload("force".into(), i64::MAX).unwrap();
        assert!(s.execution_unknown);
        let reload = s.reload.as_ref().unwrap();
        assert_eq!(reload.original_action, Some(Action::InputExecute));
        assert_eq!(reload.original_messages, Some(2));
        let (mut s, mut store, mut t) = (session(), S::default(), T::default());
        let r = s.request.as_ref().unwrap();
        t.obs = Some(Observation::Reported {
            request: r.id.clone(),
            digest: r.digest.clone(),
            body: b"report".to_vec(),
        });
        store.fail = true;
        assert!(s.capture_available_report(&mut store, &mut t).is_err());
        assert!(t.actions.is_empty());
        store.fail = false;
        let r = s.request.as_ref().unwrap();
        t.obs = Some(Observation::Reported {
            request: r.id.clone(),
            digest: r.digest.clone(),
            body: b"report".to_vec(),
        });
        s.capture_available_report(&mut store, &mut t).unwrap();
        s.request_reload("force".into(), i64::MAX).unwrap();
        assert!(!s.execution_unknown);
        assert!(s.request.as_ref().unwrap().result_digest.is_some());
        assert!(t.actions.is_empty());
    }
}
