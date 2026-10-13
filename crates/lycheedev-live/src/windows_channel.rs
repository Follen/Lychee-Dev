use crate::{
    binding::{BoundMemory, RootRule},
    host::Target,
    lifecycle::{Action, ActionReceipt},
    mailbox::{Record, Selector},
    managed::{ManagedIdentity, ManagedInstallation},
    optical::{Signal, Tracker},
    transport::{Channel, Snapshot},
};
use lycheedev_windows::{capture::CaptureStream, geometry::system_ticks};
use serde_json::{Value, json};
use std::{
    fs,
    time::{Duration, Instant},
};
pub type Result<T> = std::result::Result<T, String>;
pub struct WindowsChannel<M> {
    memory: BoundMemory,
    target: Target,
    managed: M,
    installation: ManagedIdentity,
    capture: Option<CaptureStream>,
    signal: Tracker,
    observed: Option<(Snapshot, Instant)>,
    session: Option<crate::lifecycle::Session>,
    control: lycheedev_core::control::OperationControl,
    activation_only: bool,
    replacement: crate::replacement::Tracker,
}
impl<M: ManagedInstallation> WindowsChannel<M> {
    pub fn open(
        target: Target,
        managed: M,
        rule: &RootRule,
        control: lycheedev_core::control::OperationControl,
    ) -> Result<Self> {
        target.window.verify()?;
        let memory = BoundMemory::open(target.window.process.pid, rule)?;
        if memory.hash() != target.module_sha256 {
            return Err("memory.module_identity_changed".into());
        }
        let installation = managed
            .verify_clean(
                &target.installation,
                lycheedev_core::VERSION,
                crate::SLOT_TRANSPORT,
                crate::SLOT_COUNT,
            )
            .map_err(|e| e.to_string())?;
        Ok(Self {
            memory,
            target,
            managed,
            installation,
            capture: None,
            signal: Tracker::default(),
            observed: None,
            session: None,
            control,
            activation_only: false,
            replacement: Default::default(),
        })
    }
    fn optical(&mut self) -> Result<Signal> {
        if self.capture.is_none() {
            self.capture = Some(CaptureStream::open_signal(self.target.window.clone())?)
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        for _ in 0..256 {
            let now = system_ticks()?;
            if let Ok(v) = self.signal.evidence(now) {
                return Ok(v);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("live.input_signal_waiting".into());
            }
            let frame = self
                .capture
                .as_ref()
                .unwrap()
                .next(remaining.min(Duration::from_millis(500)))?;
            let _ = self
                .signal
                .accept(&frame.bgra, frame.relative_time, system_ticks()?);
        }
        Err("live.input_signal_frame_budget".into())
    }
    fn publication(&self, slot: u16, digest: &str) -> Result<()> {
        let session = self.session.as_ref().ok_or("live.route_session_missing")?;
        crate::routes::verify(
            &self.target.installation,
            self.installation.generation,
            session,
            slot,
        )?;
        let parent = self
            .installation
            .canonical_path
            .parent()
            .ok_or("live.installation_parent")?;
        let pool: Value = lycheedev_core::storage::read_json(&parent.join(".lycheedev-slots.json"))
            .map_err(|e| e.to_string())?;
        if pool["schema"] != "lycheedev.slots.v4"
            || pool["generation"].as_u64() != Some(self.installation.generation)
        {
            return Err("live.slot_generation_changed".into());
        }
        let row = &pool["modules"][slot as usize - 1];
        if row["pendingHash"].is_string() || row["payloadHash"] != digest {
            return Err("live.slot_publication_pending_or_changed".into());
        }
        let path = parent.join(format!("Lychee Dev Slot {slot:02}/Payload.lua"));
        lycheedev_core::storage::guard_path(parent, &path).map_err(|e| e.to_string())?;
        if lycheedev_core::storage::sha256(&fs::read(path).map_err(|e| e.to_string())?) != digest {
            return Err("live.slot_payload_changed".into());
        }
        Ok(())
    }
}
impl<M: ManagedInstallation> Channel for WindowsChannel<M> {
    fn reserve_request(
        &mut self,
        session: &crate::lifecycle::Session,
        sample: &Snapshot,
    ) -> Result<()> {
        crate::routes::reserve_request(
            &self.target.installation,
            self.installation.generation,
            session,
            sample,
        )
    }
    fn settle_request(
        &mut self,
        session: &crate::lifecycle::Session,
        sample: &Snapshot,
    ) -> Result<()> {
        crate::routes::settle_request(
            &self.target.installation,
            self.installation.generation,
            session,
            sample,
        )
    }
    fn replacement_samples(
        &mut self,
        session: &crate::lifecycle::Session,
        current: &Snapshot,
    ) -> Result<Option<crate::replacement::Proof>> {
        self.replacement.observe(session, current)
    }
    fn retire_route(
        &mut self,
        session: &crate::lifecycle::Session,
        proof: &crate::replacement::Proof,
    ) -> Result<()> {
        crate::routes::retire_qualified(
            &self.target.installation,
            self.installation.generation,
            session,
            proof,
        )
    }
    fn reload(
        &mut self,
        session: &crate::lifecycle::Session,
        qualified: &Snapshot,
    ) -> Result<ActionReceipt> {
        self.verify()?;
        self.managed
            .verify_clean(
                &self.target.installation,
                lycheedev_core::VERSION,
                crate::SLOT_TRANSPORT,
                500,
            )
            .map_err(|e| e.to_string())?;
        let current = self.snapshot()?;
        if current.runtime != qualified.runtime
            || current.guid != qualified.guid
            || current.input_blocked
        {
            return Err("live.reload_input_changed".into());
        }
        if !self.activation_only && self.optical()?.state != "ready" {
            return Err("live.reload_optical_changed".into());
        }
        let control = self.control.clone();
        let deadline = session
            .reload
            .as_ref()
            .ok_or("live.reload_missing")?
            .deadline_millis;
        let receipt = lycheedev_windows::input::fixed_reload(
            &self.target.window.clone(),
            |step| {
                self.verify()?;
                if chrono::Utc::now().timestamp_millis() >= deadline {
                    return Err("live.reload_deadline_pending".into());
                }
                let fresh = self.snapshot()?;
                if fresh.runtime != qualified.runtime
                    || fresh.guid != qualified.guid
                    || fresh.build != qualified.build
                {
                    return Err("live.reload_identity_changed".into());
                }
                if fresh.input_blocked {
                    let record = self
                        .memory
                        .lookup(&Selector::Input)?
                        .ok_or("live.input_unavailable")?;
                    let info: Value =
                        serde_json::from_slice(&record.payload).map_err(|e| e.to_string())?;
                    if step < 5 || info["reason"] != "input_keyboard_focus" {
                        return Err("live.reload_input_blocked".into());
                    }
                }
                Ok(())
            },
            |duration| {
                if control.remaining().map_err(|e| e.to_string())? < duration {
                    return Err(
                        "command.deadline: fixed reload pacing exceeds remaining budget".into(),
                    );
                }
                std::thread::sleep(duration);
                control.check().map_err(|e| e.to_string())
            },
        );
        match receipt {
            Ok(r) => Ok(ActionReceipt {
                queued: r.messages_queued,
                complete: r.submission_complete && r.release_complete,
                facts: json!({"fixedReload":{"literal":"Esc*3 Enter /reload Enter","submissionComplete":r.submission_complete,"releaseComplete":r.release_complete}}),
            }),
            Err(e) => Ok(ActionReceipt {
                queued: e.receipt.messages_queued,
                complete: false,
                facts: json!({"fixedReload":{"literal":"Esc*3 Enter /reload Enter","error":e.reason,"releaseComplete":e.receipt.release_complete}}),
            }),
        }
    }
    fn activation_only(&self) -> bool {
        self.activation_only
    }
    fn activate(&mut self, qualified: &Snapshot) -> Result<ActionReceipt> {
        self.verify()?;
        self.managed
            .verify_clean(
                &self.target.installation,
                lycheedev_core::VERSION,
                crate::SLOT_TRANSPORT,
                500,
            )
            .map_err(|e| e.to_string())?;
        let current = self.snapshot()?;
        if !self.activation_only
            || current.runtime != qualified.runtime
            || current.guid != qualified.guid
            || current.address != qualified.address
            || current.bindings != qualified.bindings
        {
            return Err("live.activation_identity_changed".into());
        }
        let key = if current.bindings["wake"] == "ALT-CTRL-F12" {
            0x7b
        } else {
            0x7a
        };
        // This separately journaled bootstrap key only enables the addon. The
        // Lua callback checks current login/combat/focus before explicit Enable;
        // it cannot load a slot or run probe code. Disabled has no optical ticker.
        let receipt = lycheedev_windows::input::chord(
            &self.target.window.clone(),
            &[0x11, 0x12, key],
            || {
                self.verify()?;
                self.managed
                    .verify_clean(
                        &self.target.installation,
                        lycheedev_core::VERSION,
                        crate::SLOT_TRANSPORT,
                        500,
                    )
                    .map_err(|e| e.to_string())?;
                let sample = self.snapshot()?;
                if sample.runtime != qualified.runtime
                    || sample.guid != qualified.guid
                    || sample.bindings != qualified.bindings
                {
                    return Err("live.activation_identity_changed".into());
                }
                Ok(())
            },
        );
        match receipt {
            Ok(r) => Ok(ActionReceipt {
                queued: r.messages_queued,
                complete: r.submission_complete && r.release_complete,
                facts: json!({"activationInput":true,"businessInput":false}),
            }),
            Err(e) => Ok(ActionReceipt {
                queued: e.receipt.messages_queued,
                complete: false,
                facts: json!({"activationInput":true,"businessInput":false,"inputError":e.reason,"releaseComplete":e.receipt.release_complete}),
            }),
        }
    }
    fn claim_route(&mut self, s: &crate::lifecycle::Session) -> Result<u16> {
        let lane =
            crate::routes::claim(&self.target.installation, self.installation.generation, s)?;
        self.session = Some(s.clone());
        Ok(lane)
    }
    fn verify(&mut self) -> Result<()> {
        self.control.check().map_err(|e| e.to_string())?;
        self.target.window.verify()?;
        self.memory.check()
    }
    fn snapshot(&mut self) -> Result<Snapshot> {
        self.verify()?;
        let record = self
            .memory
            .lookup(&Selector::Input)?
            .ok_or("live.input_unavailable")?;
        let v: Value = serde_json::from_slice(&record.payload).map_err(|e| e.to_string())?;
        let activation_only = v["activationOnly"] == true;
        if record.kind != 5
            || v["schema"] != "lycheedev.input.v4"
            || v["runtime"] != record.runtime
            || v["slots"] != 500
            || (!activation_only && v["inventory"] != 500)
            || (activation_only && v["inventory"] != 0 && v["inventory"] != 500)
        {
            return Err("live.input_protocol_mismatch".into());
        }
        let text = |name: &str| {
            v[name]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("live.input_field:{name}"))
        };
        let bindings = v["bindings"].clone();
        let wake = bindings["wake"].as_str().unwrap_or("");
        if bindings["close"] != "ALT-CTRL-["
            || !((wake == "ALT-CTRL-F12" && bindings["submit"] == "ALT-CTRL-SHIFT-F12")
                || (wake == "ALT-CTRL-F11" && bindings["submit"] == "ALT-CTRL-SHIFT-F11"))
        {
            return Err("live.input_binding_unavailable".into());
        }
        let s = Snapshot {
            runtime: record.runtime,
            guid: text("guid")?,
            build: text("build")?,
            owner: text("owner")?,
            fence: v["fence"].as_u64().ok_or("live.input_fence")?,
            epoch: text("sessionEpoch")?,
            next_slot: v["nextSlot"].as_u64().unwrap_or(0) as u16,
            challenge: text("challenge")?,
            game_millis: v["sampleMillis"].as_u64().ok_or("live.input_clock")?,
            bindings,
            input_blocked: v["inputBlocked"].as_bool().ok_or("live.input_state")?,
            sample: record.sequence,
            address: record.address,
        };
        if s.guid != self.target.actor || s.build != self.target.build {
            return Err("live.target_changed".into());
        }
        self.observed = Some((s.clone(), Instant::now()));
        self.activation_only = activation_only;
        Ok(s)
    }
    fn record(&mut self, selector: &Selector) -> Result<Option<Record>> {
        self.verify()?;
        self.memory.lookup(selector)
    }
    fn publish(&mut self, slot: u16, payload: &[u8]) -> Result<()> {
        self.verify()?;
        let session = self.session.as_ref().ok_or("live.route_session_missing")?;
        crate::routes::verify(
            &self.target.installation,
            self.installation.generation,
            session,
            slot,
        )?;
        self.managed
            .verify_clean(
                &self.target.installation,
                lycheedev_core::VERSION,
                crate::SLOT_TRANSPORT,
                500,
            )
            .map_err(|e| e.to_string())?;
        self.managed
            .publish_slot(&self.installation, slot, None, payload)
            .map_err(|e| e.to_string())?;
        self.publication(slot, &lycheedev_core::storage::sha256(payload))
    }
    fn send(
        &mut self,
        action: &Action,
        qualified: &Snapshot,
        slot: u16,
        payload_sha256: &str,
    ) -> Result<ActionReceipt> {
        self.verify()?;
        self.managed
            .verify_clean(
                &self.target.installation,
                lycheedev_core::VERSION,
                crate::SLOT_TRANSPORT,
                500,
            )
            .map_err(|e| e.to_string())?;
        self.publication(slot, payload_sha256)?;
        let signal = self.optical()?;
        if signal.state != "ready" {
            return Err("live.input_signal_changed".into());
        }
        let fresh = self.snapshot()?;
        if fresh.runtime != qualified.runtime
            || fresh.guid != qualified.guid
            || fresh.owner != qualified.owner
            || fresh.fence != qualified.fence
            || fresh.epoch != qualified.epoch
            || fresh.challenge != qualified.challenge
            || fresh.next_slot != qualified.next_slot
            || fresh.bindings != qualified.bindings
            || fresh.input_blocked
        {
            return Err("live.input_sample_changed".into());
        }
        // Qualification happens after expensive file verification and optical startup.
        let key = if fresh.bindings["wake"] == "ALT-CTRL-F12" {
            0x7b
        } else {
            0x7a
        };
        let keys = match action {
            Action::Open | Action::InputExecute => vec![0x11, 0x12, key],
            Action::InputSettle => vec![0x11, 0x12, 0x10, key],
            Action::Close => vec![0x11, 0x12, 0xdb],
            _ => return Err("live.input_action".into()),
        };
        let guard_snapshot = fresh.clone();
        let receipt = lycheedev_windows::input::chord(&self.target.window.clone(), &keys, || {
            self.control.check().map_err(|e| e.to_string())?;
            self.verify()?;
            self.publication(slot, payload_sha256)?;
            let optical = self.signal.evidence(system_ticks()?)?;
            if optical.state != "ready" {
                return Err("live.input_signal_changed".into());
            }
            let observed = self.observed.as_ref().ok_or("live.input_sample_missing")?;
            if observed.1.elapsed() > Duration::from_millis(1500) {
                return Err("live.input_sample_stale".into());
            }
            let actual = self.snapshot()?;
            if actual.runtime != guard_snapshot.runtime
                || actual.sample != guard_snapshot.sample
                || actual.address != guard_snapshot.address
                || actual.bindings != guard_snapshot.bindings
                || actual.next_slot != guard_snapshot.next_slot
                || actual.challenge != guard_snapshot.challenge
                || actual.input_blocked
            {
                return Err("live.input_sample_changed".into());
            }
            Ok(())
        });
        self.signal.after_input(system_ticks()?);
        match receipt {
            Ok(r) => Ok(ActionReceipt {
                queued: r.messages_queued,
                complete: r.submission_complete && r.release_complete,
                facts: json!({}),
            }),
            Err(e) => Ok(ActionReceipt {
                queued: e.receipt.messages_queued,
                complete: false,
                facts: json!({"inputError":e.reason,"releaseComplete":e.receipt.release_complete}),
            }),
        }
    }
    fn wait_event(&mut self, budget: Duration) -> Result<bool> {
        if self.capture.is_none() {
            self.capture = Some(CaptureStream::open_signal(self.target.window.clone())?)
        }
        let frame = self
            .capture
            .as_ref()
            .unwrap()
            .next(budget.min(Duration::from_secs(60)))?;
        let _ = self
            .signal
            .accept(&frame.bgra, frame.relative_time, system_ticks()?);
        Ok(true)
    }
}
