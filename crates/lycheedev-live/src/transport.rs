//! Native transport uses exact durable exchanges; all lifecycle decisions remain in Session.
use crate::{
    lifecycle::{Action, ActionReceipt, Observation, Phase, Session, Transport},
    mailbox::{Record, Selector},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub runtime: String,
    pub guid: String,
    pub build: String,
    pub owner: String,
    pub fence: u64,
    pub epoch: String,
    pub next_slot: u16,
    pub challenge: String,
    pub game_millis: u64,
    pub bindings: Value,
    pub input_blocked: bool,
    pub sample: u32,
    pub address: u64,
}
/// A channel qualifies memory/optics/target immediately before directed input.
pub trait Channel {
    fn verify(&mut self) -> Result<()>;
    fn snapshot(&mut self) -> Result<Snapshot>;
    fn record(&mut self, selector: &Selector) -> Result<Option<Record>>;
    fn publish(&mut self, slot: u16, payload: &[u8]) -> Result<()>;
    fn send(
        &mut self,
        action: &Action,
        qualified: &Snapshot,
        slot: u16,
        payload_sha256: &str,
    ) -> Result<ActionReceipt>;
    fn wait_event(&mut self, _: std::time::Duration) -> Result<bool> {
        Ok(false)
    }
    fn claim_route(&mut self, _: &Session) -> Result<u16> {
        Ok(0)
    }
    fn activation_only(&self) -> bool {
        false
    }
    fn activate(&mut self, _: &Snapshot) -> Result<ActionReceipt> {
        Err("live.activation_unavailable".into())
    }
    fn reload(&mut self, _: &Session, _: &Snapshot) -> Result<ActionReceipt> {
        Err("live.reload_unavailable".into())
    }
    fn replacement_samples(
        &mut self,
        _: &Session,
        _: &Snapshot,
    ) -> Result<Option<crate::replacement::Proof>> {
        Ok(None)
    }
    fn retire_route(&mut self, _: &Session, _: &crate::replacement::Proof) -> Result<()> {
        Ok(())
    }
    fn reserve_request(&mut self, _: &Session, _: &Snapshot) -> Result<()> {
        Ok(())
    }
    fn settle_request(&mut self, _: &Session, _: &Snapshot) -> Result<()> {
        Ok(())
    }
}
pub struct NativeTransport<C> {
    pub channel: C,
    pub lane: u16,
}
impl<C: Channel> NativeTransport<C> {
    pub fn new(channel: C, lane: u16) -> Result<Self> {
        if lane >= 8 {
            return Err("live.route_invalid".into());
        }
        Ok(Self { channel, lane })
    }
    fn receipt(&mut self, s: &Session, nonce: &str) -> Result<Option<Record>> {
        let record = self.channel.record(&Selector::Receipt {
            nonce: nonce.into(),
        })?;
        if let Some(r) = &record {
            let v: Value = serde_json::from_slice(&r.payload).map_err(|e| e.to_string())?;
            if r.runtime != s.instance.runtime
                || v["runtime"] != s.instance.runtime
                || v["sessionEpoch"] != s.epoch
                || v["fence"].as_u64() != Some(s.fence)
                || v["nonce"] != nonce
            {
                return Err("live.receipt_identity".into());
            }
        }
        Ok(record)
    }
    fn publish(&mut self, s: &Session, action: &str, snapshot: &Snapshot) -> Result<ActionReceipt> {
        let r = s.request.as_ref();
        let ticket = r.map(|r| r.id.clone()).unwrap_or_else(|| "0".repeat(32));
        let slot = if action == "open" {
            self.lane * 2 + 1
        } else {
            snapshot.next_slot
        };
        if slot == 0 || slot > 500 {
            return Err("live.slot_invalid".into());
        }
        let nonce = lycheedev_core::storage::sha256(
            format!(
                "{}:{}:{action}:{}",
                s.owner,
                s.epoch,
                r.map(|r| r.sequence).unwrap_or(0)
            )
            .as_bytes(),
        )[..32]
            .to_owned();
        let mut e = json!({"schema":"lycheedev.slot.v4","index":slot,"runtime":s.instance.runtime,"owner":s.owner,"fence":s.fence,"sessionEpoch":s.epoch,"nonce":nonce,"ticket":ticket,"requestSeq":r.map(|r|r.sequence).unwrap_or(0),"digest":r.map(|r|r.digest.as_str()).unwrap_or(""),"action":action,"guid":s.actor,"build":snapshot.build,"challenge":snapshot.challenge,"lane":self.lane});
        if action == "execute" {
            let r = r.ok_or("live.request_missing")?;
            e["code"] = json!(r.code);
            e["budget"] = json!(r.budget_seconds);
            e["expires"] = json!(r.expires_at);
        }
        if action == "settle" {
            let r = r.ok_or("live.request_missing")?;
            if r.result_digest.is_none() {
                return Err("live.result_not_durable".into());
            }
            e["resultBytes"] = json!(r.result_bytes);
            e["resultChecksum"] = json!(r.result_checksum);
        }
        let payload = format!("LycheeDevSlotEnvelope = {}\n", lua(&e)?).into_bytes();
        self.channel.publish(slot, &payload)?;
        let key = match action {
            "open" => "openExchange",
            "execute" => "executeExchange",
            "settle" => "settleExchange",
            "close" => "closeExchange",
            _ => return Err("live.action_invalid".into()),
        };
        Ok(ActionReceipt {
            queued: 0,
            complete: true,
            facts: json!({key:{"slot":slot,"nonce":nonce,"payloadSha256":lycheedev_core::storage::sha256(&payload)}}),
        })
    }
    fn input(
        &mut self,
        s: &Session,
        action: &Action,
        key: &str,
        snapshot: &Snapshot,
    ) -> Result<ActionReceipt> {
        let x = &s.transport[key];
        let slot = x["slot"]
            .as_u64()
            .filter(|v| *v > 0 && *v <= 500)
            .ok_or("live.exchange_missing")? as u16;
        let digest = x["payloadSha256"].as_str().ok_or("live.exchange_missing")?;
        self.channel.send(action, snapshot, slot, digest)
    }
}
impl<C: Channel> Transport for NativeTransport<C> {
    fn prepare_connection(&mut self, s: &mut Session) -> Result<()> {
        if matches!(
            s.phase,
            Phase::Activating
                | Phase::ActivationAwaiting
                | Phase::Reloading
                | Phase::ReloadAwaiting
                | Phase::Closed
        ) {
            return Ok(());
        }
        self.lane = self.channel.claim_route(s)?;
        s.transport["lane"] = json!(self.lane);
        Ok(())
    }
    fn prepare_request(&mut self, s: &mut Session) -> Result<()> {
        if s.request.is_none()
            || matches!(
                s.phase,
                Phase::Reloading | Phase::ReloadAwaiting | Phase::Closed
            )
        {
            return Ok(());
        }
        let sample = self.channel.snapshot()?;
        if sample.runtime != s.instance.runtime || sample.guid != s.actor {
            return Err("live.target_changed".into());
        }
        if let Some(r) = s.request.as_mut()
            && r.expires_at == u64::MAX
        {
            r.expires_at = sample.game_millis / 1000 + 120;
        }
        Ok(())
    }
    fn wait_event(&mut self, budget: std::time::Duration) -> Result<bool> {
        self.channel.wait_event(budget)
    }
    fn observe(&mut self, s: &Session) -> Result<Observation> {
        self.channel.verify()?;
        let sample = match self.channel.snapshot() {
            Ok(sample) => sample,
            Err(reason)
                if reason == "live.input_unavailable" && s.phase == Phase::ReloadAwaiting =>
            {
                return Ok(Observation::None);
            }
            Err(reason) => return Err(reason),
        };
        if sample.guid != s.actor && !s.actor.is_empty() {
            return Err("live.actor_changed".into());
        }
        if matches!(s.phase, Phase::Reloading | Phase::ReloadAwaiting) {
            if sample.runtime == s.instance.runtime {
                return Ok(Observation::None);
            }
            if self.channel.activation_only() || sample.input_blocked {
                return Ok(Observation::None);
            }
            if !sample.owner.is_empty() {
                return Err("live.reload_runtime_owned".into());
            }
            if let Some(proof) = self.channel.replacement_samples(s, &sample)? {
                self.channel.retire_route(s, &proof)?;
                return Ok(Observation::Reloaded {
                    runtime: proof.runtime().into(),
                    samples: proof.samples(),
                });
            }
            return Ok(Observation::None);
        }
        if sample.runtime != s.instance.runtime
            && !matches!(s.phase, Phase::Activating | Phase::ActivationAwaiting)
        {
            return Ok(Observation::RuntimeChanged(sample.runtime));
        }
        if matches!(s.phase, Phase::Activating | Phase::ActivationAwaiting) {
            if self.channel.activation_only() {
                return Ok(Observation::None);
            }
            return Ok(Observation::Activated {
                runtime: sample.runtime,
            });
        }
        for (key, state) in [
            ("closeExchange", "closed"),
            ("settleExchange", "settled"),
            ("openExchange", "opened"),
        ] {
            if let Some(nonce) = s.transport[key]["nonce"].as_str()
                && let Some(record) = self.receipt(s, nonce)?
            {
                let v: Value =
                    serde_json::from_slice(&record.payload).map_err(|e| e.to_string())?;
                if v["state"] == "rejected" {
                    return Err(format!(
                        "live.rejected:{}",
                        v["reason"].as_str().unwrap_or("unknown")
                    ));
                }
                if v["state"] == state {
                    return match state {
                        "closed" => Ok(Observation::Closed),
                        "opened" => {
                            if matches!(
                                s.phase,
                                Phase::Opening | Phase::OpenPublished | Phase::OpenAwaiting
                            ) {
                                Ok(Observation::Opened)
                            } else {
                                continue;
                            }
                        }
                        "settled" => {
                            if matches!(
                                s.phase,
                                Phase::Closing
                                    | Phase::ClosePublished
                                    | Phase::CloseAwaiting
                                    | Phase::Closed
                            ) {
                                continue;
                            }
                            if let Some(r) = &s.request {
                                if v["requestSeq"].as_u64() == Some(r.sequence)
                                    && v["digest"] == r.digest
                                    && v["ticket"] == r.id
                                {
                                    if v["resourcesReleased"] == true && v["displayCleared"] == true
                                    {
                                        self.channel.settle_request(s, &sample)?;
                                    }
                                    Ok(Observation::Settled {
                                        request: r.id.clone(),
                                        digest: r.digest.clone(),
                                        resources_released: v["resourcesReleased"].as_bool()
                                            == Some(true),
                                        display_cleared: v["displayCleared"].as_bool()
                                            == Some(true),
                                    })
                                } else {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }
                        _ => continue,
                    };
                }
            }
        }
        if let (Some(r), Some(nonce)) =
            (&s.request, s.transport["executeExchange"]["nonce"].as_str())
            && let Some(head) = self.receipt(s, nonce)?
        {
            let v: Value = serde_json::from_slice(&head.payload).map_err(|e| e.to_string())?;
            if v["requestSeq"].as_u64() != Some(r.sequence)
                || v["digest"] != r.digest
                || v["ticket"] != r.id
            {
                return Err("live.report_identity".into());
            }
            match v["state"].as_str() {
                Some("running") => {
                    return Ok(Observation::Accepted {
                        request: r.id.clone(),
                        digest: r.digest.clone(),
                    });
                }
                Some("reported") => {
                    if let Some(body) = self.channel.record(&Selector::Body { head })? {
                        let result: Value = serde_json::from_slice(&body.payload)
                            .map_err(|_| "live.result_json")?;
                        if !result["ok"].is_boolean() {
                            return Err("live.result_schema".into());
                        }
                        return Ok(Observation::Reported {
                            request: r.id.clone(),
                            digest: r.digest.clone(),
                            body: body.payload,
                        });
                    }
                }
                Some("rejected") => {
                    return Err(format!(
                        "live.rejected:{}",
                        v["reason"].as_str().unwrap_or("unknown")
                    ));
                }
                _ => {}
            }
        }
        Ok(Observation::None)
    }
    fn act(&mut self, s: &Session, action: &Action) -> Result<ActionReceipt> {
        self.channel.verify()?;
        let sample = self.channel.snapshot()?;
        if sample.runtime != s.instance.runtime || sample.guid != s.actor {
            return Err("live.target_changed".into());
        }
        if *action == Action::Activate {
            if !self.channel.activation_only() {
                return Err("live.activation_not_requested".into());
            }
            return self.channel.activate(&sample);
        }
        if *action == Action::Reload {
            let reload = s.reload.as_ref().ok_or("live.reload_missing")?;
            if chrono::Utc::now().timestamp_millis() >= reload.deadline_millis {
                return Err("live.reload_deadline_pending".into());
            }
            return self.channel.reload(s, &sample);
        }
        if *action == Action::PublishOpen {
            self.lane = self.channel.claim_route(s)?
        }
        if sample.input_blocked {
            return Err("live.input_blocked".into());
        }
        if *action == Action::PublishExecute {
            self.channel.reserve_request(s, &sample)?;
        }
        match action {
            Action::PublishOpen => self.publish(s, "open", &sample),
            Action::Open => self.input(s, action, "openExchange", &sample),
            Action::PublishExecute => self.publish(s, "execute", &sample),
            Action::InputExecute => self.input(s, action, "executeExchange", &sample),
            Action::PublishSettle => self.publish(s, "settle", &sample),
            Action::InputSettle => self.input(s, action, "settleExchange", &sample),
            Action::PublishClose => self.publish(s, "close", &sample),
            Action::Close => self.input(s, action, "closeExchange", &sample),
            _ => Err("live.action_not_native_input".into()),
        }
    }
}
pub fn lua(v: &Value) -> Result<String> {
    match v {
        Value::Null => Ok("nil".into()),
        Value::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => {
            let mut out = String::from("\"");
            for b in s.bytes() {
                match b {
                    b'"' => out.push_str("\\\""),
                    b'\\' => out.push_str("\\\\"),
                    32..=126 => out.push(b as char),
                    _ => out.push_str(&format!("\\{b:03}")),
                }
            }
            out.push('"');
            Ok(out)
        }
        Value::Object(o) => {
            let mut parts = Vec::new();
            for (k, v) in o {
                parts.push(format!("[{}]={}", lua(&Value::String(k.clone()))?, lua(v)?));
            }
            Ok(format!("{{{}}}", parts.join(",")))
        }
        Value::Array(_) => Err("live.envelope_array_unsupported".into()),
    }
}
