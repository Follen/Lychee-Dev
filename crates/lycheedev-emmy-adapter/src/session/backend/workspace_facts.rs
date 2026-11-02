//! Small facts-only memo owned by one immutable analysis/intake generation.
//! It is shared across files and selected model scopes, not a second semantic DB.
use super::{OwnedQuery, QueryResult};
use crate::session::SelectedProjectionBackend;
use emmylua_code_analysis::DeclKind;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock, TryLockError};

/// Reuse only with the same immutable analysis and admitted contribution order.
/// A new or mutated analysis generation requires a fresh cache. Foreign model
/// scopes in that generation share this Arc rather than copying the name sets.
#[derive(Default)]
pub struct WorkspaceFactsCache {
    deprecated: OnceLock<Arc<DeprecatedNames>>,
    deprecated_build: Mutex<()>,
}
impl WorkspaceFactsCache {
    pub fn new() -> Self {
        Self::default()
    }
}

pub(super) struct DeprecatedNames {
    globals: HashSet<String>,
    members: HashSet<String>,
}
impl DeprecatedNames {
    pub(super) fn global(&self, name: &str) -> bool {
        self.globals.contains(name)
    }
    pub(super) fn member(&self, name: &str) -> bool {
        self.members.contains(name)
    }
}

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn workspace_deprecated_names(&mut self) -> QueryResult<Arc<DeprecatedNames>, E> {
        self.checkpoint()?;
        let cache = Arc::clone(&self.workspace_facts);
        if let Some(names) = cache.deprecated.get() {
            self.checkpoint()?;
            return Ok(Arc::clone(names));
        }
        // Avoid an uncancellable blocking mutex/OnceLock initializer. Only the
        // winning builder scans facts; waiters keep the caller's typed control.
        let _building = loop {
            self.checkpoint()?;
            match cache.deprecated_build.try_lock() {
                Ok(guard) => break guard,
                Err(TryLockError::Poisoned(error)) => break error.into_inner(),
                Err(TryLockError::WouldBlock) => std::thread::yield_now(),
            }
        };
        if let Some(names) = cache.deprecated.get() {
            self.checkpoint()?;
            return Ok(Arc::clone(names));
        }
        let order = self.owner_contribution_order()?;
        let mut names = DeprecatedNames {
            globals: HashSet::new(),
            members: HashSet::new(),
        };
        // Exactly build_deprecated_file's predicates. Nothing is published on
        // a caller stop, including a stop after the final fact was read.
        for file in order.iter().copied() {
            self.checkpoint()?;
            let Some(facts) = self.model.file_facts_of(file) else {
                continue;
            };
            for decl in &facts.decls {
                self.checkpoint()?;
                if matches!(decl.kind, DeclKind::Global) && decl.deprecated {
                    names.globals.insert(decl.name.to_string());
                }
            }
            for member in &facts.members {
                self.checkpoint()?;
                if member.deprecated {
                    names.members.insert(member.key.to_path());
                }
            }
        }
        self.checkpoint()?;
        let names = Arc::new(names);
        // The construction mutex admits one initializer. Set cannot replace an
        // earlier successful publication, including after a poisoned builder.
        let _ = cache.deprecated.set(Arc::clone(&names));
        Ok(Arc::clone(cache.deprecated.get().unwrap_or(&names)))
    }
}
