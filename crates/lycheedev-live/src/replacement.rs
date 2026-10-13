//! Qualification consumes snapshots from a Channel that has already verified
//! fixed OS/module/actor identity and the current named Mailbox path.
use crate::{lifecycle::Session, slots::Instance, transport::Snapshot};
#[derive(Default)]
pub struct Tracker {
    first: Option<(Instance, String, String, u64, Snapshot)>,
}
pub struct Proof {
    original: Instance,
    owner: String,
    epoch: String,
    fence: u64,
    runtime: String,
    samples: [u32; 2],
}
impl Proof {
    pub fn runtime(&self) -> &str {
        &self.runtime
    }
    pub fn samples(&self) -> [u32; 2] {
        self.samples
    }
    pub(crate) fn matches(&self, s: &Session) -> bool {
        self.original == s.instance
            && self.owner == s.owner
            && self.epoch == s.epoch
            && self.fence == s.fence
    }
}
impl Tracker {
    pub fn observe(&mut self, s: &Session, current: &Snapshot) -> Result<Option<Proof>, String> {
        if current.guid != s.actor
            || !current.owner.is_empty()
            || current.runtime == s.instance.runtime
            || current.sample == 0
            || current.input_blocked
        {
            return Err("live.replacement_identity_unproven".into());
        }
        if let Some((instance, owner, epoch, fence, first)) = &self.first {
            let same_scope = instance == &s.instance
                && owner == &s.owner
                && epoch == &s.epoch
                && *fence == s.fence;
            if same_scope
                && first.runtime == current.runtime
                && first.build == current.build
                && first.bindings == current.bindings
                && current.sample > first.sample
                && current.game_millis > first.game_millis
            {
                return Ok(Some(Proof {
                    original: s.instance.clone(),
                    owner: s.owner.clone(),
                    epoch: s.epoch.clone(),
                    fence: s.fence,
                    runtime: current.runtime.clone(),
                    samples: [first.sample, current.sample],
                }));
            }
            if same_scope
                && first.runtime == current.runtime
                && first.build == current.build
                && first.bindings == current.bindings
            {
                return Ok(None);
            }
        }
        self.first = Some((
            s.instance.clone(),
            s.owner.clone(),
            s.epoch.clone(),
            s.fence,
            current.clone(),
        ));
        Ok(None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> Session {
        Session::new(
            Instance {
                pid: 1,
                created: 2,
                module_sha256: "hash".into(),
                runtime: "old".into(),
            },
            "actor".into(),
            "epoch".into(),
            "owner".into(),
        )
    }
    fn sample(sequence: u32, millis: u64, runtime: &str) -> Snapshot {
        Snapshot {
            runtime: runtime.into(),
            guid: "actor".into(),
            build: "build".into(),
            owner: "".into(),
            fence: 0,
            epoch: "".into(),
            next_slot: 0,
            challenge: "".into(),
            game_millis: millis,
            bindings: serde_json::json!({"wake":"ALT-CTRL-F12"}),
            input_blocked: false,
            sample: sequence,
            address: 0x1000,
        }
    }
    #[test]
    fn fresh_sequence_and_clock_both_required_and_scope_cannot_borrow() {
        let s = session();
        let mut tracker = Tracker::default();
        assert!(
            tracker
                .observe(&s, &sample(1, 10, "new"))
                .unwrap()
                .is_none()
        );
        assert!(
            tracker
                .observe(&s, &sample(1, 11, "new"))
                .unwrap()
                .is_none()
        );
        assert!(
            tracker
                .observe(&s, &sample(2, 10, "new"))
                .unwrap()
                .is_none()
        );
        let mut foreign = s.clone();
        foreign.epoch = "other".into();
        assert!(
            tracker
                .observe(&foreign, &sample(2, 11, "new"))
                .unwrap()
                .is_none()
        );
        let proof = tracker
            .observe(&foreign, &sample(3, 12, "new"))
            .unwrap()
            .unwrap();
        assert!(proof.matches(&foreign));
        assert!(!proof.matches(&s));
    }
    #[test]
    fn actor_owned_and_mixed_runtime_rejected() {
        let s = session();
        let mut tracker = Tracker::default();
        let mut bad = sample(1, 10, "new");
        bad.guid = "foreign".into();
        assert!(tracker.observe(&s, &bad).is_err());
        bad = sample(1, 10, "new");
        bad.owner = "foreign".into();
        assert!(tracker.observe(&s, &bad).is_err());
        assert!(
            tracker
                .observe(&s, &sample(1, 10, "new"))
                .unwrap()
                .is_none()
        );
        assert!(
            tracker
                .observe(&s, &sample(2, 11, "third"))
                .unwrap()
                .is_none()
        );
        assert!(
            tracker
                .observe(&s, &sample(3, 12, "third"))
                .unwrap()
                .is_some()
        );
    }
}
