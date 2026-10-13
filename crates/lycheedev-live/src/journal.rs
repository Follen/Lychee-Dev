//! Durable native journal and result storage with installation claims and OS-wide advancement lock.
use crate::lifecycle::{Phase, Request, Session, Store};
use lycheedev_core::{
    coordination::{Claim, InstallationGuard},
    storage,
};
use lycheedev_windows::NamedLock;
use std::{
    fs,
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, String>;
pub struct DurableStore {
    journal: PathBuf,
    result_root: PathBuf,
    installation: PathBuf,
    owner: String,
    _advance: NamedLock,
}
fn token(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl DurableStore {
    pub fn create(home: &Path, installation: &Path, session: &Session) -> Result<Self> {
        if !token(&session.owner) || session.schema != "lycheedev.lifecycle.v2" {
            return Err("live.journal_identity".into());
        }
        let advance = NamedLock::acquire(&format!(
            "Local\\LycheeDev.Advance.{}.{}",
            session.instance.pid, session.instance.created
        ))?;
        let root = home.join("live").join("v2");
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
        let journal = root.join(format!("CON-{}.json", session.owner));
        if journal.exists() {
            return Err("live.journal_exists_resume_original".into());
        }
        let mut gate = InstallationGuard::acquire(installation).map_err(|e| e.to_string())?;
        let resource = format!(
            "process:{}:{}",
            session.instance.pid, session.instance.created
        );
        if gate
            .registry
            .claims
            .iter()
            .any(|c| c.pending && c.resource == resource)
        {
            return Err("live.original_lifecycle_required".into());
        }
        let lifecycle = journal.to_string_lossy().into_owned();
        // Save the recoverable lifecycle first. No external effect is possible until its claim exists.
        storage::atomic_json(&journal, session).map_err(|e| e.to_string())?;
        gate.claim(Claim {
            owner: session.owner.clone(),
            kind: "native-live".into(),
            resource,
            process_id: session.instance.pid,
            created_at: session.instance.created.to_string(),
            lifecycle_ref: lifecycle,
            pending: true,
        })
        .map_err(|e| e.to_string())?;
        Ok(Self {
            journal,
            result_root: root.join("results"),
            installation: installation.to_owned(),
            owner: session.owner.clone(),
            _advance: advance,
        })
    }
    pub fn resume(home: &Path, installation: &Path, owner: &str) -> Result<(Self, Session)> {
        if !token(owner) {
            return Err("live.owner_invalid".into());
        }
        let root = fs::canonicalize(home.join("live").join("v2")).map_err(|e| e.to_string())?;
        let journal = root.join(format!("CON-{owner}.json"));
        if fs::metadata(&journal).map_err(|e| e.to_string())?.len() > 4 << 20 {
            return Err("live.journal_budget".into());
        }
        let session: Session = storage::read_json(&journal).map_err(|e| e.to_string())?;
        if session.owner != owner || session.schema != "lycheedev.lifecycle.v2" {
            return Err("live.journal_identity".into());
        }
        let advance = NamedLock::acquire(&format!(
            "Local\\LycheeDev.Advance.{}.{}",
            session.instance.pid, session.instance.created
        ))?;
        let mut gate = InstallationGuard::acquire(installation).map_err(|e| e.to_string())?;
        let resource = format!(
            "process:{}:{}",
            session.instance.pid, session.instance.created
        );
        let lifecycle = journal.to_string_lossy();
        if ((session.phase == Phase::Reloading && session.reload.is_some())
            || (session.phase == Phase::Opening
                && session
                    .capacity_rotation
                    .as_ref()
                    .is_some_and(|r| r.reopening)))
            && session.pending_action.is_none()
            && session.input_messages.is_none()
        {
            gate.reactivate(owner, &lifecycle)
                .map_err(|e| e.to_string())?;
        }
        if !gate.registry.claims.iter().any(|c| c.owner == owner)
            && session.phase == Phase::Activating
            && session.pending_action.is_none()
            && session.request.is_none()
        {
            if gate
                .registry
                .claims
                .iter()
                .any(|c| c.pending && c.resource == resource)
            {
                return Err("live.original_lifecycle_required".into());
            }
            gate.claim(Claim {
                owner: owner.into(),
                kind: "native-live".into(),
                resource: resource.clone(),
                process_id: session.instance.pid,
                created_at: session.instance.created.to_string(),
                lifecycle_ref: lifecycle.to_string(),
                pending: true,
            })
            .map_err(|e| e.to_string())?;
        }
        if !gate.registry.claims.iter().any(|c| {
            c.owner == owner
                && c.resource == resource
                && c.lifecycle_ref == lifecycle
                && (c.pending
                    || matches!(
                        session.phase,
                        Phase::Closed | Phase::Cancelled | Phase::Abandoned
                    ))
        }) {
            return Err("live.claim_mismatch".into());
        }
        Ok((
            Self {
                journal,
                result_root: root.join("results"),
                installation: installation.to_owned(),
                owner: owner.into(),
                _advance: advance,
            },
            session,
        ))
    }
    fn result_path(&self, r: &Request) -> Result<PathBuf> {
        if !token(&r.id) {
            return Err("live.request_id".into());
        }
        Ok(self.result_root.join(format!("{}.result", r.id)))
    }
    pub fn reactivate(&self) -> Result<()> {
        InstallationGuard::acquire(&self.installation)
            .map_err(|e| e.to_string())?
            .reactivate(&self.owner, &self.journal.to_string_lossy())
            .map_err(|e| e.to_string())
    }
    /// Finish post-journal terminal cutpoints without rewriting a successful receipt.
    pub fn complete_terminal(&mut self, session: &Session) -> Result<()> {
        if session.owner != self.owner {
            return Err("live.owner_changed".into());
        }
        if matches!(
            session.phase,
            Phase::Settled | Phase::Cancelled | Phase::Abandoned
        ) && session.capacity_rotation.is_none()
            && let Some(r) = &session.request
        {
            let path = self.result_root.join(format!("{}.metadata.json", r.id));
            if path.exists() {
                let saved: Session = storage::read_json(&path).map_err(|e| e.to_string())?;
                if serde_json::to_value(&saved).map_err(|e| e.to_string())?
                    != serde_json::to_value(session).map_err(|e| e.to_string())?
                {
                    return Err("live.terminal_metadata_conflict".into());
                }
            } else {
                storage::atomic_json(&path, session).map_err(|e| e.to_string())?;
            }
        }
        if (session.phase == Phase::Closed && session.cleanup_complete)
            || matches!(session.phase, Phase::Cancelled | Phase::Abandoned)
        {
            let mut gate =
                InstallationGuard::acquire(&self.installation).map_err(|e| e.to_string())?;
            let lifecycle = self.journal.to_string_lossy();
            let claim = gate
                .registry
                .claims
                .iter()
                .find(|c| c.owner == self.owner && c.lifecycle_ref == lifecycle)
                .ok_or("live.claim_mismatch")?;
            if claim.pending {
                gate.retire(&self.owner, &lifecycle)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
impl Store for DurableStore {
    fn reactivate(&mut self) -> Result<()> {
        DurableStore::reactivate(self)
    }
    fn checkpoint(&mut self, session: &Session) -> Result<()> {
        if session.owner != self.owner {
            return Err("live.owner_changed".into());
        }
        storage::atomic_json(&self.journal, session).map_err(|e| e.to_string())?;
        self.complete_terminal(session)
    }
    fn persist_result(&mut self, request: &Request, body: &[u8]) -> Result<String> {
        if body.len() > 524288 {
            return Err("live.result_budget".into());
        }
        let path = self.result_path(request)?;
        let digest = storage::sha256(body);
        if path.exists() {
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 524288 {
                return Err("live.result_budget".into());
            }
            let existing = fs::read(&path).map_err(|e| e.to_string())?;
            if existing != body {
                return Err("live.result_conflict".into());
            }
        } else {
            storage::atomic_write(&path, body).map_err(|e| e.to_string())?;
        }
        Ok(digest)
    }
    fn verify_result(&mut self, request: &Request) -> Result<()> {
        let path = self.result_path(request)?;
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 524288 {
            return Err("live.result_budget".into());
        }
        let body = fs::read(path).map_err(|e| e.to_string())?;
        if request.result_digest.as_deref() != Some(&storage::sha256(&body)) {
            return Err("live.result_corrupt".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slots::Instance;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    fn setup() -> (PathBuf, Session) {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("lycheedev-live-journal-{}-{n}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let s = Session::new(
            Instance {
                pid: std::process::id(),
                created: n,
                module_sha256: "h".into(),
                runtime: "1".repeat(32),
            },
            "actor".into(),
            "2".repeat(32),
            format!("{n:032x}"),
        );
        (root, s)
    }
    #[test]
    fn cross_workspace_cannot_bypass_pending_claim() {
        let (root, s) = setup();
        let install = root.join("addon");
        let store = DurableStore::create(&root.join("home-a"), &install, &s).unwrap();
        drop(store);
        let mut foreign = s.clone();
        foreign.owner = "f".repeat(32);
        assert!(DurableStore::create(&root.join("home-b"), &install, &foreign).is_err());
        let (_store, recovered) =
            DurableStore::resume(&root.join("home-a"), &install, &s.owner).unwrap();
        assert_eq!(recovered.phase, Phase::Activating);
    }
    #[test]
    fn persisted_result_is_verified_and_not_overwritten() {
        let (root, mut s) = setup();
        let install = root.join("addon");
        let mut store = DurableStore::create(&root.join("home"), &install, &s).unwrap();
        s.phase = Phase::Idle;
        s.prepare("a".repeat(32), "return 1".into(), 30, 100)
            .unwrap();
        let r = s.request.as_mut().unwrap();
        r.result_digest = Some(store.persist_result(r, b"result").unwrap());
        store.verify_result(r).unwrap();
        assert!(store.persist_result(r, b"different").is_err());
        fs::write(store.result_path(r).unwrap(), b"broken").unwrap();
        assert!(store.verify_result(r).is_err());
    }
    #[test]
    fn closed_checkpoint_retires_exact_claim() {
        let (root, mut s) = setup();
        let install = root.join("addon");
        let mut store = DurableStore::create(&root.join("home"), &install, &s).unwrap();
        s.phase = Phase::Closed;
        s.cleanup_complete = true;
        store.checkpoint(&s).unwrap();
        drop(store);
        let gate = InstallationGuard::acquire(&install).unwrap();
        gate.require_idle().unwrap();
        drop(gate);
        assert!(DurableStore::resume(&root.join("home"), &install, &s.owner).is_ok());
    }
    #[test]
    fn preclaim_crash_resumes_same_activation() {
        let (root, s) = setup();
        let home = root.join("home");
        let install = root.join("addon");
        let dir = home.join("live/v2");
        fs::create_dir_all(&dir).unwrap();
        storage::atomic_json(&dir.join(format!("CON-{}.json", s.owner)), &s).unwrap();
        let (_, recovered) = DurableStore::resume(&home, &install, &s.owner).unwrap();
        assert_eq!(recovered.phase, Phase::Activating);
    }
}
