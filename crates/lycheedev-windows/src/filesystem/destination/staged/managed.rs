//! Finite predeclared staging names. No random temporary is created while
//! persisting a declaration, and no occupied name is adopted or overwritten.
use super::*;

pub const MANAGED_STAGE_SLOTS: u8 = 64;
pub const MANAGED_DECLARATION_BYTES: usize = 128;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedStageKind {
    Spool,
    Shard,
}
impl ManagedStageKind {
    fn label(self) -> &'static str {
        match self {
            Self::Spool => "spool",
            Self::Shard => "shard",
        }
    }
}
/// Obtained only after the original declaration handle has completed sync.
/// It names a slot, not an existing file, and confers no deletion authority.
#[derive(Clone, Debug)]
pub struct ManagedStageName {
    kind: ManagedStageKind,
    slot: u8,
}
impl ManagedStageName {
    /// Restore only the finite name described by an authenticated immutable
    /// declaration. This grants CREATE_NEW, never adoption/deletion. Core must
    /// hold the matching slot lease and authenticate any retained payload.
    pub fn from_declaration(bytes: &[u8; MANAGED_DECLARATION_BYTES]) -> io::Result<Self> {
        use sha2::{Digest, Sha256};
        let kind = match bytes[8] {
            1 => ManagedStageKind::Spool,
            2 => ManagedStageKind::Shard,
            _ => return Err(invalid("invalid declaration kind")),
        };
        if &bytes[..8] != b"LDST0002"
            || bytes[9] >= MANAGED_STAGE_SLOTS
            || bytes[10..16] != [0; 6]
            || bytes[88..96] != [0; 8]
            || u64::from_le_bytes(bytes[16..24].try_into().expect("fixed length")) > (1 << 40)
            || kind == ManagedStageKind::Spool && bytes[56..88] != [0; 32]
            || Sha256::digest(&bytes[..96]).as_slice() != &bytes[96..]
        {
            return Err(invalid("invalid managed declaration identity"));
        }
        Ok(Self {
            kind,
            slot: bytes[9],
        })
    }
    pub fn as_string(&self) -> String {
        format!(
            ".lycheedev-data-{}-{:02}.stage",
            self.kind.label(),
            self.slot
        )
    }
}
pub struct ManagedStageDeclaration<'a> {
    stage: StagedFile<'a>,
    name: ManagedStageName,
    attempted: bool,
}
impl<'a> ManagedStageDeclaration<'a> {
    pub fn record_name(kind: ManagedStageKind, slot: u8) -> io::Result<String> {
        if slot >= MANAGED_STAGE_SLOTS {
            return Err(invalid("managed stage slot exceeds finite namespace"));
        }
        Ok(format!(".lycheedev-data-{}-{slot:02}.record", kind.label()))
    }
    pub fn payload_name(kind: ManagedStageKind, slot: u8) -> io::Result<String> {
        Self::record_name(kind, slot)?;
        Ok(ManagedStageName { kind, slot }.as_string())
    }
    pub fn create<C: FileControl>(
        parent: &'a Directory,
        kind: ManagedStageKind,
        slot: u8,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        let name = Self::record_name(kind, slot).map_err(C::Error::from)?;
        if parent.path().file_name() != Some(std::ffi::OsStr::new("shared-blobs-v1")) {
            return Err(invalid("managed declaration requires quota bookkeeping directory").into());
        }
        let stage = StagedFile::create_exact_managed(parent, name, control)?;
        Ok(Self {
            stage,
            name: ManagedStageName { kind, slot },
            attempted: false,
        })
    }
    /// The caller encodes/authenticates the fixed record. A failed write/sync
    /// retains this bounded slot; it cannot authorize payload creation.
    pub fn persist<C: FileControl>(
        &mut self,
        bytes: &[u8; MANAGED_DECLARATION_BYTES],
        control: &mut C,
    ) -> Result<ManagedStageName, C::Error> {
        if self.attempted {
            return Err(invalid("managed declaration already attempted").into());
        }
        let declared = ManagedStageName::from_declaration(bytes).map_err(C::Error::from)?;
        if declared.kind != self.name.kind || declared.slot != self.name.slot {
            return Err(invalid("declaration differs from its original finite slot").into());
        }
        control.check()?;
        self.attempted = true;
        // A panic/early Drop after this point must never erase the declaration
        // while a returned authorization may already have created a payload.
        self.stage.stage.published = true;
        self.stage.write_all(bytes).map_err(C::Error::from)?;
        control.check()?;
        self.stage.seal().map_err(C::Error::from)?;
        control.check()?;
        Ok(self.name.clone())
    }
    /// Record retention is not a body-publication fact. This exact fixed-name
    /// record survives checked close even after a torn write or failed sync.
    pub fn close(mut self) -> Vec<io::Error> {
        self.stage.stage.published = true;
        self.stage.publication = StagedPublication::Published;
        self.stage.close()
    }
    /// Only an original CREATE_NEW record owner can withdraw without reauth.
    /// The caller must have checked closed every payload authorized by it.
    pub fn withdraw(mut self) -> Vec<io::Error> {
        self.stage.stage.published = false;
        self.stage.close()
    }
}
impl<'a> StagedFile<'a> {
    fn create_exact_managed<C: FileControl>(
        parent: &'a Directory,
        name: String,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        parent.verify_controlled(control)?;
        control.charge(FileCost {
            io_operations: 4,
            allocation_bytes: 4096,
            work_units: 4096,
        })?;
        control.check()?;
        let file = relative_file(
            parent.parent_handle(),
            std::ffi::OsStr::new(&name),
            RelativeOpen::Stage,
        )
        .map_err(C::Error::from)?;
        // No fallible operation after acquisition before ownership is returned.
        Ok(Self {
            stage: Stage {
                file: Some(file),
                name,
                published: false,
            },
            parent,
            publication: StagedPublication::NotPublished,
            sealed: false,
            rebuildable_cache: false,
        })
    }
    pub fn create_predeclared<C: FileControl>(
        parent: &'a Directory,
        name: &ManagedStageName,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        Self::create_exact_managed(parent, name.as_string(), control)
    }
}
impl<'a> PrivateSpool<'a> {
    pub fn create_predeclared<C: FileControl>(
        parent: &'a Directory,
        name: &ManagedStageName,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        if name.kind != ManagedStageKind::Spool {
            return Err(invalid("private spool requires a spool declaration").into());
        }
        StagedFile::create_predeclared(parent, name, control).map(PrivateSpool::from_managed_stage)
    }
}

#[cfg(test)]
mod tests;
