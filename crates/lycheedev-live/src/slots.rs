use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const BOOTSTRAP_SLOTS: u16 = 20;
pub const MAX_INSTANCES: u16 = 8;
pub const LANE_SLOTS: u16 = 60;
pub const NORMAL_EXCHANGE_SLOTS: u16 = 2;
pub const REQUEST_RESERVE: u16 = 4;
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Instance {
    pub pid: u32,
    pub created: u64,
    pub module_sha256: String,
    pub runtime: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lane {
    pub instance: Instance,
    pub lane: u16,
    pub cursor: u16,
    pub fused: bool,
    pub recovery_used: bool,
    pub pending: Option<Reservation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reservation {
    pub request: String,
    pub first: u16,
    pub last: u16,
    pub consumed: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Ledger {
    pub generation: u64,
    pub bootstrap_cursor: u16,
    pub lanes: BTreeMap<u16, Lane>,
}
pub type Result<T> = std::result::Result<T, String>;
impl Ledger {
    pub fn claim(&mut self, instance: Instance) -> Result<u16> {
        if let Some((&id, _)) = self.lanes.iter().find(|(_, l)| l.instance == instance) {
            return Ok(id);
        }
        let lane = (0..MAX_INSTANCES)
            .find(|i| !self.lanes.contains_key(i))
            .ok_or("slots.instance_backpressure")?;
        self.lanes.insert(
            lane,
            Lane {
                instance,
                lane,
                cursor: 0,
                fused: false,
                recovery_used: false,
                pending: None,
            },
        );
        Ok(lane)
    }
    pub fn bootstrap(&mut self) -> Result<u16> {
        if self.bootstrap_cursor >= BOOTSTRAP_SLOTS - MAX_INSTANCES * 2 {
            return Err("slots.bootstrap_exhausted".into());
        }
        self.bootstrap_cursor += 1;
        Ok(MAX_INSTANCES * 2 + self.bootstrap_cursor)
    }
    pub fn bootstrap_pair(&self, instance: &Instance) -> Result<[u16; 2]> {
        let (&lane, _) = self
            .lanes
            .iter()
            .find(|(_, l)| &l.instance == instance)
            .ok_or("slots.instance_unknown")?;
        Ok([lane * 2 + 1, lane * 2 + 2])
    }
    pub fn retire(&mut self, instance: &Instance, proof: &Retirement) -> Result<()> {
        let (&id, lane) = self
            .lanes
            .iter()
            .find(|(_, l)| &l.instance == instance)
            .ok_or("slots.instance_unknown")?;
        // The production route seam seals replacement with two current samples. A
        // bare PID/creation exit declaration cannot dispose of an uncertain task.
        if !proof.confirms(instance)
            || (lane.pending.is_some() && !matches!(proof, Retirement::RuntimeReplaced { .. }))
        {
            return Err("slots.retirement_unproven".into());
        }
        self.lanes.remove(&id);
        Ok(())
    }
}
#[derive(Debug)]
pub enum Retirement {
    ProcessExited {
        pid: u32,
        created: u64,
    },
    RuntimeReplaced {
        pid: u32,
        created: u64,
        old_runtime: String,
        new_runtime: String,
        samples: [u64; 2],
    },
}
impl Retirement {
    fn confirms(&self, i: &Instance) -> bool {
        match self {
            Self::ProcessExited { pid, created } => *pid == i.pid && *created == i.created,
            Self::RuntimeReplaced {
                pid,
                created,
                old_runtime,
                new_runtime,
                samples,
            } => {
                *pid == i.pid
                    && *created == i.created
                    && old_runtime == &i.runtime
                    && new_runtime != old_runtime
                    && samples[0] > 0
                    && samples[1] > samples[0]
            }
        }
    }
}
impl Lane {
    pub fn physical(&self, offset: u16) -> Result<u16> {
        if offset >= LANE_SLOTS {
            return Err("slots.route_exhausted".into());
        }
        Ok(BOOTSTRAP_SLOTS + self.lane * LANE_SLOTS + offset + 1)
    }
    pub fn reserve(&mut self, request: &str) -> Result<Reservation> {
        if self.recovery_used {
            return Err("slots.capacity_reload_required".into());
        }
        if self.fused {
            return Err("slots.route_fused".into());
        }
        if self.pending.is_some() {
            return Err("slots.request_pending".into());
        }
        // Last physical slot always remains available for explicit session close.
        if self.cursor + REQUEST_RESERVE >= LANE_SLOTS {
            return Err("slots.capacity_reload_required".into());
        }
        let r = Reservation {
            request: request.into(),
            first: self.physical(self.cursor)?,
            last: self.physical(self.cursor + REQUEST_RESERVE - 1)?,
            consumed: 0,
        };
        self.pending = Some(r.clone());
        Ok(r)
    }
    pub fn consume(&mut self, slot: u16, dispatched: bool) -> Result<()> {
        let expected = self.physical(self.cursor)?;
        if slot != expected {
            return Err("slots.foreign_or_out_of_order".into());
        }
        if self.fused {
            return Err("slots.route_fused".into());
        }
        let r = self.pending.as_mut().ok_or("slots.no_reservation")?;
        if slot > r.last {
            return Err("slots.reserved_budget_exhausted".into());
        }
        r.consumed += 1;
        self.cursor += 1;
        if !dispatched {
            self.fused = true;
        }
        Ok(())
    }
    /// Exact bootstrap control is a separate finite operation, never a wake retry.
    pub fn recover(&mut self, request: &str, proof: RecoveryProof) -> Result<()> {
        if self.recovery_used {
            return Err("slots.recovery_budget_exhausted".into());
        }
        let r = self.pending.as_ref().ok_or("slots.no_reservation")?;
        if r.request != request
            || !proof.exact_control_receipt
            || proof.runtime != self.instance.runtime
            || self.physical(self.cursor)? > r.last
        {
            return Err("slots.recovery_unproven".into());
        }
        self.fused = false;
        self.recovery_used = true;
        Ok(())
    }
    pub fn settle(&mut self, request: &str) -> Result<()> {
        if self.pending.as_ref().is_none_or(|r| r.request != request) {
            return Err("slots.request_mismatch".into());
        }
        self.pending = None;
        Ok(())
    }
    pub fn close_slot(&self) -> Result<u16> {
        self.physical(LANE_SLOTS - 1)
    }
}
pub struct RecoveryProof {
    pub runtime: String,
    pub exact_control_receipt: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    fn i(n: u32) -> Instance {
        Instance {
            pid: n,
            created: n as u64,
            module_sha256: "binary".into(),
            runtime: format!("runtime{n}"),
        }
    }
    #[test]
    fn eight_routes_never_consume_foreign() {
        let mut l = Ledger::default();
        for n in 1..=8 {
            assert_eq!(l.claim(i(n)).unwrap(), n as u16 - 1)
        }
        assert!(l.claim(i(9)).is_err());
        let foreign = l.lanes[&1].physical(0).unwrap();
        let a = l.lanes.get_mut(&0).unwrap();
        a.reserve("r").unwrap();
        assert!(a.consume(foreign, true).is_err());
        assert_eq!(a.cursor, 0);
    }
    #[test]
    fn late_wakes_fuse_once_and_recover_settle() {
        let mut l = Ledger::default();
        l.claim(i(1)).unwrap();
        let a = l.lanes.get_mut(&0).unwrap();
        let r = a.reserve("r").unwrap();
        a.consume(r.first, true).unwrap();
        a.consume(r.first + 1, false).unwrap();
        for _ in 0..1000 {
            assert!(a.consume(r.first + 2, false).is_err())
        }
        assert_eq!(a.cursor, 2);
        a.recover(
            "r",
            RecoveryProof {
                runtime: "runtime1".into(),
                exact_control_receipt: true,
            },
        )
        .unwrap();
        a.consume(r.first + 2, true).unwrap();
        a.settle("r").unwrap();
        assert!(a.close_slot().unwrap() > r.last);
    }
    #[test]
    fn bootstrap_exhaustion_preserves_running_reserve() {
        let mut l = Ledger::default();
        l.claim(i(1)).unwrap();
        let r = l.lanes.get_mut(&0).unwrap().reserve("r").unwrap();
        assert_eq!(l.bootstrap_pair(&i(1)).unwrap(), [1, 2]);
        for _ in 0..4 {
            l.bootstrap().unwrap();
        }
        assert!(l.bootstrap().is_err());
        let a = l.lanes.get_mut(&0).unwrap();
        a.consume(r.first, true).unwrap();
        a.consume(r.first + 1, true).unwrap();
        a.settle("r").unwrap();
        assert_eq!(a.cursor, 2);
    }
    #[test]
    fn normal_two_slots_and_capacity_rotation() {
        let mut l = Ledger::default();
        l.claim(i(1)).unwrap();
        let a = l.lanes.get_mut(&0).unwrap();
        let mut tasks = 0;
        while let Ok(r) = a.reserve("r") {
            a.consume(r.first, true).unwrap();
            a.consume(r.first + 1, true).unwrap();
            a.settle("r").unwrap();
            tasks += 1;
        }
        assert_eq!(tasks, 28);
        assert_eq!(a.cursor, 56);
        assert_eq!(a.close_slot().unwrap(), 80);
    }
    #[test]
    fn no_age_based_retirement() {
        let mut l = Ledger::default();
        l.claim(i(1)).unwrap();
        assert!(
            l.retire(&i(1), &Retirement::ProcessExited { pid: 1, created: 2 })
                .is_err()
        );
        l.lanes.get_mut(&0).unwrap().reserve("r").unwrap();
        assert!(
            l.retire(&i(1), &Retirement::ProcessExited { pid: 1, created: 1 })
                .is_err()
        );
    }
}
