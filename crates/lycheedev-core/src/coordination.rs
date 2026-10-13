//! Shared installation occupancy. A pending claim requires explicit business retirement.
use crate::{
    Result, ToolkitError,
    storage::{self, ExclusiveLock},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Claim {
    pub owner: String,
    pub kind: String,
    pub resource: String,
    pub process_id: u32,
    pub created_at: String,
    pub lifecycle_ref: String,
    pub pending: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRegistry {
    pub schema: String,
    pub claims: Vec<Claim>,
}
pub struct InstallationGuard {
    pub target: PathBuf,
    pub registry: ClaimRegistry,
    path: PathBuf,
    _lock: ExclusiveLock,
}
impl InstallationGuard {
    pub fn acquire(target: &Path) -> Result<Self> {
        let parent = fs::canonicalize(
            target
                .parent()
                .ok_or_else(|| ToolkitError::invalid("installation requires parent"))?,
        )?;
        let target = parent.join(
            target
                .file_name()
                .ok_or_else(|| ToolkitError::invalid("installation requires basename"))?,
        );
        storage::guard_path(&parent, &target)?;
        let target = if target.exists() {
            fs::canonicalize(&target)?
        } else {
            target
        };
        let canonical = target
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_ascii_lowercase();
        let lock = ExclusiveLock::resource(
            &parent.join(".lycheedev-locks"),
            &format!("installation-maintenance:{canonical}"),
        )?;
        let path = parent.join(".lycheedev-claims.json");
        let registry: ClaimRegistry = if path.exists() {
            storage::guard_path(&parent, &path)?;
            storage::read_json(&path)?
        } else {
            ClaimRegistry {
                schema: "lycheedev.claims.v1".into(),
                claims: vec![],
            }
        };
        if registry.schema != "lycheedev.claims.v1" || registry.claims.len() > 1024 {
            return Err(ToolkitError::new(
                "coordination.claim_format",
                "unknown or unbounded claim registry",
                4,
            ));
        }
        Ok(Self {
            target,
            registry,
            path,
            _lock: lock,
        })
    }
    pub fn require_idle(&self) -> Result<()> {
        if self.registry.claims.iter().any(|c| c.pending) {
            Err(ToolkitError::new(
                "delivery.installation_in_use",
                "pending live ownership requires explicit recovery and retirement",
                6,
            ))
        } else {
            Ok(())
        }
    }
    pub fn claim(&mut self, claim: Claim) -> Result<()> {
        if claim.owner.is_empty() || claim.lifecycle_ref.is_empty() || claim.kind != "native-live" {
            return Err(ToolkitError::invalid("invalid installation claim"));
        }
        if let Some(existing) = self.registry.claims.iter().find(|c| c.owner == claim.owner) {
            if serde_json::to_value(existing)? == serde_json::to_value(&claim)? {
                return Ok(());
            }
            return Err(ToolkitError::new(
                "coordination.owner_conflict",
                "claim owner identity differs",
                4,
            ));
        }
        if self.registry.claims.len() >= 1024 {
            return Err(ToolkitError::new(
                "coordination.backpressure",
                "installation claim capacity exhausted",
                6,
            ));
        }
        let mut next = self.registry.clone();
        next.claims.push(claim);
        storage::atomic_json(&self.path, &next)?;
        self.registry = next;
        Ok(())
    }
    pub fn reactivate(&mut self, owner: &str, lifecycle_ref: &str) -> Result<()> {
        let matches: Vec<_> = self
            .registry
            .claims
            .iter()
            .enumerate()
            .filter(|(_, claim)| {
                claim.owner == owner
                    && claim.lifecycle_ref == lifecycle_ref
                    && claim.kind == "native-live"
            })
            .map(|(index, _)| index)
            .collect();
        if owner.is_empty() || lifecycle_ref.is_empty() || matches.len() != 1 {
            return Err(ToolkitError::new(
                "coordination.claim_missing",
                "exact existing owner, kind and lifecycle are required",
                4,
            ));
        }
        let index = matches[0];
        let existing = &self.registry.claims[index];
        if existing.resource.is_empty() {
            return Err(ToolkitError::invalid("claim resource identity missing"));
        }
        if self
            .registry
            .claims
            .iter()
            .enumerate()
            .any(|(other, claim)| {
                other != index && claim.pending && claim.resource == existing.resource
            })
        {
            return Err(ToolkitError::new(
                "coordination.resource_in_use",
                "another pending owner still occupies the same resource",
                6,
            ));
        }
        if existing.pending {
            return Ok(());
        }
        let mut next = self.registry.clone();
        next.claims[index].pending = true;
        storage::atomic_json(&self.path, &next)?;
        self.registry = next;
        Ok(())
    }
    pub fn retire(&mut self, owner: &str, lifecycle_ref: &str) -> Result<()> {
        let mut next = self.registry.clone();
        let claim = next
            .claims
            .iter_mut()
            .find(|c| c.owner == owner && c.lifecycle_ref == lifecycle_ref)
            .ok_or_else(|| {
                ToolkitError::new(
                    "coordination.claim_missing",
                    "exact owner and lifecycle are required",
                    4,
                )
            })?;
        claim.pending = false;
        storage::atomic_json(&self.path, &next)?;
        self.registry = next;
        Ok(())
    }
}
