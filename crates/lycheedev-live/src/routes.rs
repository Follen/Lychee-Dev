//! Installation-scoped native routes, coordinated with durable logical ownership.
use crate::{lifecycle::Session, slots::Ledger};
use lycheedev_core::{coordination::InstallationGuard, storage};
use serde::{Deserialize, Serialize};
use std::path::Path;
pub type Result<T> = std::result::Result<T, String>;
#[derive(Serialize, Deserialize)]
struct Book {
    schema: String,
    generation: u64,
    ledger: Ledger,
    owners: std::collections::BTreeMap<u16, String>,
    #[serde(default)]
    retired: std::collections::BTreeMap<String, (crate::slots::Instance, String, [u32; 2])>,
}
fn open(path: &Path, generation: u64) -> Result<Book> {
    if path.exists() {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 512 * 1024 {
            return Err("live.route_book_budget".into());
        }
        let b: Book = storage::read_json(path).map_err(|e| e.to_string())?;
        if b.schema != "lycheedev.native-routes.v1"
            || b.generation != generation
            || b.ledger.lanes.len() > 8
            || b.retired.len() > 1024
        {
            return Err("live.route_book_mismatch".into());
        }
        Ok(b)
    } else {
        Ok(Book {
            schema: "lycheedev.native-routes.v1".into(),
            generation,
            ledger: Ledger {
                generation,
                ..Ledger::default()
            },
            owners: Default::default(),
            retired: Default::default(),
        })
    }
}
pub fn claim(installation: &Path, generation: u64, session: &Session) -> Result<u16> {
    let gate = InstallationGuard::acquire(installation).map_err(|e| e.to_string())?;
    if !gate.registry.claims.iter().any(|c| {
        c.pending
            && c.owner == session.owner
            && c.resource
                == format!(
                    "process:{}:{}",
                    session.instance.pid, session.instance.created
                )
    }) {
        return Err("live.route_claim_owner_mismatch".into());
    }
    let path = installation
        .parent()
        .ok_or("live.installation_parent")?
        .join(".lycheedev-native-routes.json");
    let mut b = open(&path, generation)?;
    if b.retired
        .values()
        .any(|(instance, _, _)| instance == &session.instance)
    {
        return Err("live.route_runtime_retired".into());
    }
    let lane = b.ledger.claim(session.instance.clone())?;
    if let Some(owner) = b.owners.get(&lane) {
        if owner != &session.owner {
            return Err("live.route_foreign_owner".into());
        }
    } else {
        b.owners.insert(lane, session.owner.clone());
    }
    storage::atomic_json(&path, &b).map_err(|e| e.to_string())?;
    Ok(lane)
}
pub fn verify(installation: &Path, generation: u64, session: &Session, slot: u16) -> Result<()> {
    let _gate = InstallationGuard::acquire(installation).map_err(|e| e.to_string())?;
    let path = installation
        .parent()
        .ok_or("live.installation_parent")?
        .join(".lycheedev-native-routes.json");
    let b = open(&path, generation)?;
    let (&lane, l) = b
        .ledger
        .lanes
        .iter()
        .find(|(_, l)| l.instance == session.instance)
        .ok_or("live.route_missing")?;
    if b.owners.get(&lane) != Some(&session.owner)
        || !(slot >= 21 + lane * 60 && slot <= 80 + lane * 60
            || slot == lane * 2 + 1
            || slot == lane * 2 + 2)
    {
        return Err("live.route_foreign_slot".into());
    }
    if l.instance != session.instance {
        return Err("live.route_identity_changed".into());
    }
    Ok(())
}
/// Reserve the exact task before its payload is published. The channel supplies a qualified
/// private INPUT sample; persistent CON ownership and this task reservation are separate.
pub fn reserve_request(
    installation: &Path,
    generation: u64,
    session: &Session,
    sample: &crate::transport::Snapshot,
) -> Result<()> {
    let gate = InstallationGuard::acquire(installation).map_err(|e| e.to_string())?;
    if !gate.registry.claims.iter().any(|c| {
        c.pending
            && c.owner == session.owner
            && c.resource
                == format!(
                    "process:{}:{}",
                    session.instance.pid, session.instance.created
                )
    }) {
        return Err("live.route_claim_owner_mismatch".into());
    }
    if sample.runtime != session.instance.runtime
        || sample.guid != session.actor
        || sample.owner != session.owner
        || sample.epoch != session.epoch
        || sample.fence != session.fence
    {
        return Err("live.route_sample_scope".into());
    }
    let path = installation
        .parent()
        .ok_or("live.installation_parent")?
        .join(".lycheedev-native-routes.json");
    let mut book = open(&path, generation)?;
    let (&lane, _) = book
        .ledger
        .lanes
        .iter()
        .find(|(_, lane)| lane.instance == session.instance)
        .ok_or("live.route_missing")?;
    if book.owners.get(&lane) != Some(&session.owner) {
        return Err("live.route_foreign_owner".into());
    }
    let route = book.ledger.lanes.get_mut(&lane).unwrap();
    let request = session.request.as_ref().ok_or("live.request_missing")?;
    if sample.next_slot != route.physical(route.cursor)? {
        return Err("live.route_cursor_mismatch".into());
    }
    if let Some(pending) = &route.pending {
        return if pending.request == request.id {
            Ok(())
        } else {
            Err("slots.request_pending".into())
        };
    }
    route.reserve(&request.id)?;
    storage::atomic_json(&path, &book).map_err(|e| e.to_string())
}
/// Release only an exact operation after the channel validated its SETTLE receipt.
/// CON ownership remains available for subsequent operations; uncertain work stays reserved.
pub fn settle_request(
    installation: &Path,
    generation: u64,
    session: &Session,
    sample: &crate::transport::Snapshot,
) -> Result<()> {
    let gate = InstallationGuard::acquire(installation).map_err(|e| e.to_string())?;
    if !gate.registry.claims.iter().any(|c| {
        c.pending
            && c.owner == session.owner
            && c.resource
                == format!(
                    "process:{}:{}",
                    session.instance.pid, session.instance.created
                )
    }) {
        return Err("live.route_claim_owner_mismatch".into());
    }
    if sample.runtime != session.instance.runtime
        || sample.guid != session.actor
        || sample.owner != session.owner
        || sample.epoch != session.epoch
        || sample.fence != session.fence
    {
        return Err("live.route_sample_scope".into());
    }
    let path = installation
        .parent()
        .ok_or("live.installation_parent")?
        .join(".lycheedev-native-routes.json");
    let mut book = open(&path, generation)?;
    let (&lane, _) = book
        .ledger
        .lanes
        .iter()
        .find(|(_, lane)| lane.instance == session.instance)
        .ok_or("live.route_missing")?;
    if book.owners.get(&lane) != Some(&session.owner) {
        return Err("live.route_foreign_owner".into());
    }
    let route = book.ledger.lanes.get_mut(&lane).unwrap();
    let request = session.request.as_ref().ok_or("live.request_missing")?;
    if request.result_digest.is_none() {
        return Err("live.result_not_durable".into());
    }
    let execute = session.transport["executeExchange"]["slot"]
        .as_u64()
        .filter(|slot| (1..=500).contains(slot))
        .ok_or("live.exchange_missing")? as u16;
    let settle = session.transport["settleExchange"]["slot"]
        .as_u64()
        .filter(|slot| (1..=500).contains(slot))
        .ok_or("live.exchange_missing")? as u16;
    if settle != execute + 1 || sample.next_slot != settle + 1 {
        return Err("live.route_settle_cursor".into());
    }
    if route.pending.is_none() {
        return if route.physical(route.cursor)? == sample.next_slot {
            Ok(())
        } else {
            Err("live.route_request_not_pending".into())
        };
    }
    let pending = route.pending.as_ref().unwrap();
    if pending.request != request.id
        || pending.first != execute
        || route.physical(route.cursor)? != execute
    {
        return Err("live.route_request_mismatch".into());
    }
    route.consume(execute, true)?;
    route.consume(settle, true)?;
    route.settle(&request.id)?;
    storage::atomic_json(&path, &book).map_err(|e| e.to_string())
}
pub fn retire_qualified(
    installation: &Path,
    generation: u64,
    session: &Session,
    proof: &crate::replacement::Proof,
) -> Result<()> {
    if !proof.matches(session) {
        return Err("live.route_retirement_scope".into());
    }
    let new_runtime = proof.runtime();
    let samples = proof.samples();
    let gate = InstallationGuard::acquire(installation).map_err(|e| e.to_string())?;
    if !gate.registry.claims.iter().any(|claim| {
        claim.pending
            && claim.owner == session.owner
            && claim.resource
                == format!(
                    "process:{}:{}",
                    session.instance.pid, session.instance.created
                )
    }) {
        return Err("live.route_retirement_owner".into());
    }
    if new_runtime == session.instance.runtime || samples[0] == 0 || samples[1] <= samples[0] {
        return Err("live.route_retirement_unproven".into());
    }
    let path = installation
        .parent()
        .ok_or("live.installation_parent")?
        .join(".lycheedev-native-routes.json");
    let mut book = open(&path, generation)?;
    let retirement_key = format!("{}:{}", session.owner, session.instance.runtime);
    if let Some((instance, runtime, _)) = book.retired.get(&retirement_key).or_else(|| {
        book.retired
            .get(&session.owner)
            .filter(|(instance, _, _)| instance == &session.instance)
    }) {
        return if instance == &session.instance && runtime == new_runtime {
            Ok(())
        } else {
            Err("live.route_retirement_conflict".into())
        };
    }
    let (&lane, _) = book
        .ledger
        .lanes
        .iter()
        .find(|(_, lane)| lane.instance == session.instance)
        .ok_or("live.route_missing")?;
    if book.owners.get(&lane) != Some(&session.owner) {
        return Err("live.route_foreign_owner".into());
    }
    book.ledger.retire(
        &session.instance,
        &crate::slots::Retirement::RuntimeReplaced {
            pid: session.instance.pid,
            created: session.instance.created,
            old_runtime: session.instance.runtime.clone(),
            new_runtime: new_runtime.into(),
            samples: [samples[0] as u64, samples[1] as u64],
        },
    )?;
    book.owners.remove(&lane);
    book.retired.insert(
        retirement_key,
        (session.instance.clone(), new_runtime.into(), samples),
    );
    storage::atomic_json(&path, &book).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{replacement::Tracker, slots::Instance, transport::Snapshot};
    use lycheedev_core::coordination::Claim;
    fn session(n: u32) -> Session {
        Session::new(
            Instance {
                pid: n,
                created: n as u64 + 100,
                module_sha256: "binary".into(),
                runtime: format!("runtime{n}"),
            },
            "actor".into(),
            format!("epoch{n}"),
            format!("owner{n}"),
        )
    }
    fn sample(sequence: u32) -> Snapshot {
        Snapshot {
            runtime: "replacement".into(),
            guid: "actor".into(),
            build: "build".into(),
            owner: "".into(),
            fence: 0,
            epoch: "".into(),
            next_slot: 0,
            challenge: "".into(),
            game_millis: sequence as u64,
            bindings: serde_json::json!({}),
            input_blocked: false,
            sample: sequence,
            address: 0x1000,
        }
    }
    #[test]
    fn exact_operation_reservation_releases_for_next_task_and_foreign_lane_is_unchanged() {
        use crate::lifecycle::Phase;
        let root = std::env::temp_dir().join(format!(
            "lycheedev-route-reserve-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let install = root.join("addon");
        let mut a = session(100);
        let mut b = session(200);
        for s in [&a, &b] {
            InstallationGuard::acquire(&install)
                .unwrap()
                .claim(Claim {
                    owner: s.owner.clone(),
                    kind: "native-live".into(),
                    resource: format!("process:{}:{}", s.instance.pid, s.instance.created),
                    process_id: s.instance.pid,
                    created_at: s.instance.created.to_string(),
                    lifecycle_ref: format!("fixture-{}", s.owner),
                    pending: true,
                })
                .unwrap();
            claim(&install, 1, s).unwrap();
        }
        let current = |s: &Session, slot: u16| {
            let mut snap = sample(1);
            snap.runtime = s.instance.runtime.clone();
            snap.guid = s.actor.clone();
            snap.owner = s.owner.clone();
            snap.epoch = s.epoch.clone();
            snap.fence = s.fence;
            snap.next_slot = slot;
            snap
        };
        b.phase = Phase::Idle;
        b.prepare("foreign".into(), "return 2".into(), 30, 999)
            .unwrap();
        reserve_request(&install, 1, &b, &current(&b, 81)).unwrap();
        let path = root.join(".lycheedev-native-routes.json");
        let before: Book = storage::read_json(&path).unwrap();
        let foreign = serde_json::to_value(&before.ledger.lanes[&1]).unwrap();
        a.phase = Phase::Idle;
        for n in 0..28u16 {
            a.prepare(format!("request{n}"), "return 1".into(), 30, 999)
                .unwrap();
            let first = 21 + n * 2;
            reserve_request(&install, 1, &a, &current(&a, first)).unwrap();
            reserve_request(&install, 1, &a, &current(&a, first)).unwrap();
            let mut wrong = current(&a, first);
            wrong.epoch = "foreign".into();
            assert!(reserve_request(&install, 1, &a, &wrong).is_err());
            a.request.as_mut().unwrap().result_digest = Some("durable fixture digest".into());
            a.transport["executeExchange"] = serde_json::json!({"slot":first});
            a.transport["settleExchange"] = serde_json::json!({"slot":first+1});
            assert!(settle_request(&install, 1, &b, &current(&a, first + 2)).is_err());
            settle_request(&install, 1, &a, &current(&a, first + 2)).unwrap();
            settle_request(&install, 1, &a, &current(&a, first + 2)).unwrap();
            a.phase = Phase::Settled;
            a.cleanup_complete = true;
        }
        a.prepare("request28".into(), "return 1".into(), 30, 999)
            .unwrap();
        assert_eq!(
            reserve_request(&install, 1, &a, &current(&a, 77)).unwrap_err(),
            "slots.capacity_reload_required"
        );
        let after: Book = storage::read_json(&path).unwrap();
        assert_eq!(
            serde_json::to_value(&after.ledger.lanes[&1]).unwrap(),
            foreign
        );
        assert!(after.ledger.lanes[&0].pending.is_none());
        assert!(after.ledger.lanes[&1].pending.is_some());
        assert_eq!(after.ledger.lanes[&0].cursor, 56);
        InstallationGuard::acquire(&install)
            .unwrap()
            .registry
            .claims
            .iter()
            .for_each(|claim| assert!(claim.pending));
    }
    #[test]
    fn exact_runtime_retirement_preserves_other_instance_and_reuses_only_replacement() {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-routes-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let install = root.join("addon");
        let a = session(100);
        let b = session(200);
        for s in [&a, &b] {
            InstallationGuard::acquire(&install)
                .unwrap()
                .claim(Claim {
                    owner: s.owner.clone(),
                    kind: "native-live".into(),
                    resource: format!("process:{}:{}", s.instance.pid, s.instance.created),
                    process_id: s.instance.pid,
                    created_at: s.instance.created.to_string(),
                    lifecycle_ref: format!("fixture-{}", s.owner),
                    pending: true,
                })
                .unwrap();
        }
        assert_eq!(claim(&install, 1, &a).unwrap(), 0);
        assert_eq!(claim(&install, 1, &b).unwrap(), 1);
        assert!(verify(&install, 1, &a, 81).is_err());
        assert!(verify(&install, 1, &b, 21).is_err());
        let before: Book = storage::read_json(&root.join(".lycheedev-native-routes.json")).unwrap();
        let foreign_before = serde_json::to_value(&before.ledger.lanes[&1]).unwrap();
        let mut tracker = Tracker::default();
        assert!(tracker.observe(&a, &sample(1)).unwrap().is_none());
        let proof = tracker.observe(&a, &sample(2)).unwrap().unwrap();
        assert!(retire_qualified(&install, 1, &b, &proof).is_err());
        retire_qualified(&install, 1, &a, &proof).unwrap();
        retire_qualified(&install, 1, &a, &proof).unwrap();
        assert!(claim(&install, 1, &a).is_err());
        let after: Book = storage::read_json(&root.join(".lycheedev-native-routes.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&after.ledger.lanes[&1]).unwrap(),
            foreign_before
        );
        assert_eq!(after.owners[&1], b.owner);
        let mut replaced = a.clone();
        replaced.instance.runtime = "replacement".into();
        assert_eq!(claim(&install, 1, &replaced).unwrap(), 0);
        assert_eq!(claim(&install, 1, &b).unwrap(), 1);
    }
}
