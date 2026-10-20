//! Durable finite-name staging. Immutable declarations precede payload creation.
//! Source identity is a routing constraint, never standalone deletion authority.
use super::*;
use lycheedev_windows::filesystem::destination::{
    MANAGED_STAGE_SLOTS, ManagedStageDeclaration, ManagedStageKind, ManagedStageName,
};
use std::collections::BTreeMap;

const BYTES: usize = 128;
const MAGIC: &[u8; 8] = b"LDST0002";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceIdentity([u8; 32]);
impl SourceIdentity {
    /// Length-framed immutable encoded identity, CKey, key context and decoding
    /// policy. No credentials are stored; the full source still authenticates EOF.
    pub fn from_parts(parts: &[&[u8]]) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"lycheedev.data-source.v2\0");
        hash.update((parts.len() as u64).to_le_bytes());
        for part in parts {
            hash.update((part.len() as u64).to_le_bytes());
            hash.update(part);
        }
        Self(hash.finalize().into())
    }
    pub(super) fn memory(reference: &BlobRef) -> Self {
        Self::from_parts(&[
            b"authenticated-memory",
            reference.sha256.as_bytes(),
            &reference.bytes.to_le_bytes(),
        ])
    }
}
#[derive(Clone, Debug)]
struct Declaration {
    kind: ManagedStageKind,
    slot: u8,
    source: SourceIdentity,
    expected: u64,
    digest: [u8; 32],
}
impl Declaration {
    fn encode(&self) -> Result<[u8; BYTES]> {
        if self.slot >= MANAGED_STAGE_SLOTS || self.expected > evidence::MAX_CAPTURE_BYTES {
            return Err(integrity());
        }
        let mut bytes = [0; BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8] = match self.kind {
            ManagedStageKind::Spool => 1,
            ManagedStageKind::Shard => 2,
        };
        bytes[9] = self.slot;
        bytes[16..24].copy_from_slice(&self.expected.to_le_bytes());
        bytes[24..56].copy_from_slice(&self.source.0);
        bytes[56..88].copy_from_slice(&self.digest);
        let digest = Sha256::digest(&bytes[..96]);
        bytes[96..].copy_from_slice(&digest);
        Ok(bytes)
    }
    fn decode(kind: ManagedStageKind, slot: u8, bytes: &[u8]) -> Result<Self> {
        if bytes.len() != BYTES || &bytes[..8] != MAGIC {
            return Err(integrity());
        }
        let result = Self {
            kind,
            slot,
            source: SourceIdentity(bytes[24..56].try_into().map_err(|_| integrity())?),
            expected: u64::from_le_bytes(bytes[16..24].try_into().map_err(|_| integrity())?),
            digest: bytes[56..88].try_into().map_err(|_| integrity())?,
        };
        if result.encode()?.as_slice() != bytes
            || kind == ManagedStageKind::Spool && result.digest != [0; 32]
        {
            return Err(integrity());
        }
        Ok(result)
    }
    fn record_name(&self) -> Result<String> {
        ManagedStageDeclaration::record_name(self.kind, self.slot).map_err(Into::into)
    }
    fn payload_name(&self) -> Result<String> {
        ManagedStageDeclaration::payload_name(self.kind, self.slot).map_err(Into::into)
    }
    fn payload_parent(&self, home: &Path, paths: &DataCachePaths) -> Result<PathBuf> {
        match self.kind {
            ManagedStageKind::Spool => Ok(paths.staging.clone()),
            ManagedStageKind::Shard => Ok(evidence::blob_path(home, &hex::encode(self.digest))?
                .parent()
                .expect("shard")
                .to_owned()),
        }
    }
}
fn slot_lock(
    owner: &DerivedReservation<'_>,
    kind: ManagedStageKind,
    slot: u8,
) -> Result<ExclusiveLock> {
    ExclusiveLock::resource_checked(
        &owner.home.join("locks"),
        &format!("data-stage-v2:{kind:?}:{slot}"),
        || owner.control.check(),
    )
}
fn record_directory<'a>(owner: &'a DerivedReservation<'_>) -> &'a Directory {
    owner
        .directories
        .iter()
        .find(|d| d.path() == owner.paths.shared_blobs())
        .expect("held shared records")
}
fn read_declaration_strict(
    directory: &Directory,
    kind: ManagedStageKind,
    slot: u8,
    checks: &Checks<'_>,
) -> Result<Option<Declaration>> {
    let name = ManagedStageDeclaration::record_name(kind, slot)?;
    read_record(directory, &name, BYTES, checks)?
        .map(|raw| Declaration::decode(kind, slot, &raw))
        .transpose()
}
/// Torn/foreign fixed records occupy their slot and remain physically counted.
/// They authorize no payload. Never hide a compound I/O/checked-close failure.
fn read_declaration(
    directory: &Directory,
    kind: ManagedStageKind,
    slot: u8,
    checks: &Checks<'_>,
) -> Result<Option<Declaration>> {
    match read_declaration_strict(directory, kind, slot, checks) {
        Err(error)
            if error.code == "vault.data_shared_blob_integrity" && error.details.is_null() =>
        {
            Ok(None)
        }
        result => result,
    }
}
/// Known declarations replace the measured spool sample with max(expected,
/// measured). Missing payloads are charged too. No second stat can undercount
/// growth while producer writes outside the quota lease.
pub(in crate::workspace::data_disk) fn spool_reservations(
    paths: &DataCachePaths,
    checks: &Checks<'_>,
    held: &[Directory],
) -> Result<BTreeMap<String, u64>> {
    with_inventory_directory(&paths.shared_blobs(), held, checks, |directory| {
        let mut result = BTreeMap::new();
        for slot in 0..MANAGED_STAGE_SLOTS {
            // A torn record is bounded metadata, not a valid reservation or
            // payload authorization. It is retained and cannot be overwritten.
            match read_declaration(directory, ManagedStageKind::Spool, slot, checks) {
                Ok(Some(record)) => {
                    result.insert(record.payload_name()?, record.expected);
                }
                Ok(None) => {}
                Err(error)
                    if error.code == "vault.data_shared_blob_integrity"
                        && error.details.is_null() => {}
                Err(error) => return Err(error),
            }
        }
        Ok(result)
    })
}
/// A held process-shared slot, never a PID/timestamp. Only try-lock is used
/// while quota is held, so a producer may reacquire quota for settlement.
pub(crate) struct StageSlot {
    declaration: Declaration,
    pub(crate) name: ManagedStageName,
    lease: Option<ExclusiveLock>,
}
impl StageSlot {
    pub(crate) fn create_spool(
        owner: &DerivedReservation<'_>,
        source: &SourceIdentity,
        expected: u64,
    ) -> Result<Self> {
        Self::create(owner, ManagedStageKind::Spool, source, expected, [0; 32])
    }
    pub(crate) fn create_shard(
        owner: &DerivedReservation<'_>,
        source: &SourceIdentity,
        reference: &BlobRef,
    ) -> Result<Self> {
        let mut digest = [0; 32];
        hex::decode_to_slice(&reference.sha256, &mut digest).map_err(|_| integrity())?;
        // Reuse the same durable declaration and empty/recoverable slot rather
        // than requiring extra record/body headroom after every crash.
        for slot in 0..MANAGED_STAGE_SLOTS {
            let Some(record) = read_declaration(
                record_directory(owner),
                ManagedStageKind::Shard,
                slot,
                &owner.control,
            )?
            else {
                continue;
            };
            if record.source != *source
                || record.expected != reference.bytes
                || record.digest != digest
            {
                continue;
            }
            let lease = slot_lock(owner, record.kind, slot)?;
            let name = ManagedStageName::from_declaration(&record.encode()?);
            match name {
                Ok(name) => {
                    return Ok(Self {
                        declaration: record,
                        name,
                        lease: Some(lease),
                    });
                }
                Err(error) => return join(Err(error.into()), lease.close().err(), &owner.control),
            }
        }
        Self::create(
            owner,
            ManagedStageKind::Shard,
            source,
            reference.bytes,
            digest,
        )
    }
    fn create(
        owner: &DerivedReservation<'_>,
        kind: ManagedStageKind,
        source: &SourceIdentity,
        expected: u64,
        digest: [u8; 32],
    ) -> Result<Self> {
        for slot in 0..MANAGED_STAGE_SLOTS {
            owner.control.check()?;
            let directory = record_directory(owner);
            let name = ManagedStageDeclaration::record_name(kind, slot)?;
            if directory
                .optional_metadata_controlled(OsStr::new(&name), &mut Controlled(&owner.control))?
                .is_some()
            {
                continue;
            }
            let lease = match slot_lock(owner, kind, slot) {
                Ok(lease) => lease,
                Err(error) if error.code == "workspace.busy" && error.details.is_null() => continue,
                Err(error) => return Err(error),
            };
            let record = Declaration {
                kind,
                slot,
                source: source.clone(),
                expected,
                digest,
            };
            let result = (|| {
                // Check named payload occupancy too: unknown legacy/foreign
                // occupant is not adopted even though its declaration is absent.
                let parent = record.payload_parent(&owner.home, &owner.paths)?;
                if parent.try_exists()? {
                    let dir = Directory::open_controlled(&parent, &mut Controlled(&owner.control))?;
                    let exists = dir
                        .optional_metadata_controlled(
                            OsStr::new(&record.payload_name()?),
                            &mut Controlled(&owner.control),
                        )
                        .map(|v| v.is_some());
                    if join(exists, close_directory(dir), &owner.control)? {
                        return Ok(None);
                    }
                }
                let mut declaration = ManagedStageDeclaration::create(
                    directory,
                    kind,
                    slot,
                    &mut Controlled(&owner.control),
                )?;
                let result =
                    declaration.persist(&record.encode()?, &mut Controlled(&owner.control));
                join(
                    result,
                    declaration.close().into_iter().map(ToolkitError::io),
                    &owner.control,
                )
                .map(Some)
            })();
            match result {
                Ok(Some(name)) => {
                    return Ok(Self {
                        declaration: record,
                        name,
                        lease: Some(lease),
                    });
                }
                other => {
                    let result = join(other, lease.close().err(), &owner.control)?;
                    if result.is_none() {
                        continue;
                    }
                }
            }
        }
        Err(ToolkitError::new(
            "vault.data_stage_slots_exhausted",
            "All finite durable stage slots are occupied; source-authenticated recovery is required",
            6,
        ))
    }
    pub(crate) fn finish(mut self, owner: &DerivedReservation<'_>, clean: bool) -> Result<()> {
        let result = if clean {
            remove_record(owner, &self.declaration)
        } else {
            Ok(())
        };
        join(
            result,
            self.lease.take().and_then(|l| l.close().err()),
            &owner.control,
        )
    }
    pub(crate) fn release(mut self) -> Result<()> {
        self.lease.take().map_or(Ok(()), ExclusiveLock::close)
    }
}
// Required bounded cleanup of an already-settled original transaction must
// still execute after cancellation. Identity/length/EOF checks are unchanged;
// finish_file/join preserve the original operation stop and real close errors.
struct RecordCleanup;
impl FileControl for RecordCleanup {
    type Error = ToolkitError;
    fn charge(&mut self, _: FileCost) -> Result<()> {
        Ok(())
    }
    fn check(&mut self) -> Result<()> {
        Ok(())
    }
}
fn remove_record(owner: &DerivedReservation<'_>, record: &Declaration) -> Result<()> {
    let directory = record_directory(owner);
    let candidate = directory
        .open_cleanup_candidate_owner_controlled(
            OsStr::new(&record.record_name()?),
            &mut RecordCleanup,
        )?
        .ok_or_else(integrity)?;
    let raw = record.encode()?;
    let authenticated = candidate.authenticate(|file| {
        let mut bytes = [0; BYTES];
        if file.metadata()?.len() != BYTES as u64 {
            return Err(integrity());
        }
        file.read_exact(&mut bytes)?;
        if bytes != raw || file.read(&mut [0])? != 0 {
            return Err(integrity());
        }
        Ok(())
    });
    match authenticated {
        Ok(candidate) => {
            let (file, result) = candidate.discard();
            finish_file(file, result.map_err(Into::into), &owner.control)
        }
        Err((error, candidate)) => finish_file(candidate.into_file(), Err(error), &owner.control),
    }
}

/// Authenticate every byte through the original cleanup owner. Even an empty
/// candidate needs a fully authenticated source; caller memory is already hashed.
fn remove_payload(
    owner: &DerivedReservation<'_>,
    record: &Declaration,
    bytes: &[u8],
) -> Result<()> {
    if bytes.len() as u64 != record.expected {
        return Err(integrity());
    }
    remove_payload_reader(owner, record, &mut std::io::Cursor::new(bytes))
}
fn remove_payload_reader(
    owner: &DerivedReservation<'_>,
    record: &Declaration,
    source: &mut impl Read,
) -> Result<()> {
    let parent_path = record.payload_parent(&owner.home, &owner.paths)?;
    // Ancestors remain pinned through original-handle authentication/disposal.
    let mut anchors = Vec::new();
    let mut current = PathBuf::new();
    let result = (|| {
        for component in parent_path.components() {
            current.push(component.as_os_str());
            if matches!(component, std::path::Component::Prefix(_)) {
                continue;
            }
            // Native absence is an empty payload namespace only after the
            // already-held existing ancestors have been checked.
            if !current.try_exists()? {
                return Ok(());
            }
            anchors.push(storage::hold_cleanup_ancestor(
                &current,
                owner.control.control,
            )?);
        }
        let directory = Directory::open_controlled(&parent_path, &mut Controlled(&owner.control))?;
        let result = (|| {
            let name = record.payload_name()?;
            let metadata = directory
                .optional_metadata_controlled(OsStr::new(&name), &mut Controlled(&owner.control))?;
            if metadata.is_none() {
                return Ok(());
            }
            let candidate = directory
                .open_cleanup_candidate_owner_controlled(
                    OsStr::new(&name),
                    &mut Controlled(&owner.control),
                )?
                .ok_or_else(|| {
                    ToolkitError::new(
                        "workspace.busy",
                        "Managed payload is still held; retained",
                        6,
                    )
                })?;
            let result = candidate.authenticate(|file| {
                let length = usize::try_from(file.metadata()?.len()).map_err(|_| integrity())?;
                if length as u64 > record.expected {
                    return Err(integrity());
                }
                let mut buffer = [0; 32768];
                let mut expected = [0; 32768];
                let mut offset = 0;
                while offset < length {
                    owner.control.check()?;
                    let size = buffer.len().min(length - offset);
                    file.read_exact(&mut buffer[..size])?;
                    source.read_exact(&mut expected[..size])?;
                    if buffer[..size] != expected[..size] {
                        return Err(integrity());
                    }
                    offset += size;
                }
                if file.read(&mut [0])? != 0 {
                    return Err(integrity());
                }
                owner.control.check()
            });
            match result {
                Ok(candidate) => {
                    let (file, result) = candidate.discard();
                    finish_file(file, result.map_err(Into::into), &owner.control)
                }
                Err((error, candidate)) => {
                    finish_file(candidate.into_file(), Err(error), &owner.control)
                }
            }
        })();
        join(result, close_directory(directory), &owner.control)
    })();
    let mut cleanup = Vec::new();
    while let Some(anchor) = anchors.pop() {
        cleanup.extend(storage::close_file(anchor).err().map(ToolkitError::io));
    }
    join(result, cleanup, &owner.control)
}
/// Called only with a freshly fully authenticated memory source and quota.
/// Returns whether this source owned a recoverable new-protocol transaction;
/// legacy unknown stages are never swept or adopted.
pub(super) fn recover_memory(
    owner: &DerivedReservation<'_>,
    source: &SourceIdentity,
    reference: &BlobRef,
    bytes: &[u8],
) -> Result<bool> {
    let mut recovered = false;
    for slot in 0..MANAGED_STAGE_SLOTS {
        let Some(record) = read_declaration(
            record_directory(owner),
            ManagedStageKind::Shard,
            slot,
            &owner.control,
        )?
        else {
            continue;
        };
        if record.source != *source
            || record.expected != reference.bytes
            || hex::encode(record.digest) != reference.sha256
        {
            continue;
        }
        let lease = match slot_lock(owner, record.kind, record.slot) {
            Ok(lease) => lease,
            Err(error) if error.code == "workspace.busy" && error.details.is_null() => continue,
            Err(error) => return Err(error),
        };
        let result = (|| {
            let blob = ExclusiveLock::resource_wait_checked(
                &owner.home.join("locks"),
                &format!("blob:{}", reference.sha256),
                Some(owner.control.control.deadline()),
                || owner.control.check(),
            )?;
            let result = remove_payload(owner, &record, bytes);
            let result = join(result, blob.close().err(), &owner.control);
            result?;
            // Preserve the declaration until the caller has repaired/completed
            // its original intent. It still proves v2 attribution after crash.
            Ok(())
        })();
        join(result, lease.close().err(), &owner.control)?;
        recovered = true;
    }
    Ok(recovered)
}
/// Retire old declarations only after completion is durably recorded. Their
/// payloads have already been authenticated and checked closed by recovery.
pub(super) fn finish_memory_recovery(
    owner: &DerivedReservation<'_>,
    source: &SourceIdentity,
    reference: &BlobRef,
    bytes: &[u8],
) -> Result<()> {
    for slot in 0..MANAGED_STAGE_SLOTS {
        let Some(record) = read_declaration(
            record_directory(owner),
            ManagedStageKind::Shard,
            slot,
            &owner.control,
        )?
        else {
            continue;
        };
        if record.source != *source
            || record.expected != reference.bytes
            || hex::encode(record.digest) != reference.sha256
        {
            continue;
        }
        let lease = match slot_lock(owner, record.kind, record.slot) {
            Ok(lease) => lease,
            Err(error) if error.code == "workspace.busy" && error.details.is_null() => continue,
            Err(error) => return Err(error),
        };
        let result =
            remove_payload(owner, &record, bytes).and_then(|()| remove_record(owner, &record));
        join(result, lease.close().err(), &owner.control)?;
    }
    Ok(())
}

pub(super) fn has_shard(
    owner: &DerivedReservation<'_>,
    source: &SourceIdentity,
    reference: &BlobRef,
) -> Result<bool> {
    for slot in 0..MANAGED_STAGE_SLOTS {
        if let Some(record) = read_declaration(
            record_directory(owner),
            ManagedStageKind::Shard,
            slot,
            &owner.control,
        )? {
            if record.source == *source
                && record.expected == reference.bytes
                && hex::encode(record.digest) == reference.sha256
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
/// Caller holds quota and canonical blob lease, with fully verified original
/// spool. Never calls the producer again and restores the spool cursor.
pub(crate) fn recover_shards(
    owner: &DerivedReservation<'_>,
    source: &SourceIdentity,
    reference: &BlobRef,
    spool: &mut lycheedev_windows::filesystem::destination::PrivateSpool<'_>,
    except: Option<&ManagedStageName>,
) -> Result<()> {
    for slot in 0..MANAGED_STAGE_SLOTS {
        let Some(record) = read_declaration(
            record_directory(owner),
            ManagedStageKind::Shard,
            slot,
            &owner.control,
        )?
        else {
            continue;
        };
        if record.source != *source
            || record.expected != reference.bytes
            || hex::encode(record.digest) != reference.sha256
        {
            continue;
        }
        let held = except.is_some_and(|name| {
            record.payload_name().ok().as_deref() == Some(name.as_string().as_str())
        });
        let lease = if held {
            None
        } else {
            Some(slot_lock(owner, record.kind, record.slot)?)
        };
        let result = spool
            .rewind_read_ready()
            .map_err(ToolkitError::io)
            .and_then(|()| remove_payload_reader(owner, &record, spool));
        let result = join(
            result,
            spool.rewind_read_ready().err().map(ToolkitError::io),
            &owner.control,
        );
        join(
            result,
            lease.and_then(|lease| lease.close().err()),
            &owner.control,
        )?;
    }
    Ok(())
}
pub(super) fn finish_shards(
    owner: &DerivedReservation<'_>,
    source: &SourceIdentity,
    reference: &BlobRef,
) -> Result<()> {
    for slot in 0..MANAGED_STAGE_SLOTS {
        let Some(record) = read_declaration(
            record_directory(owner),
            ManagedStageKind::Shard,
            slot,
            &owner.control,
        )?
        else {
            continue;
        };
        if record.source != *source
            || record.expected != reference.bytes
            || hex::encode(record.digest) != reference.sha256
        {
            continue;
        }
        let lease = match slot_lock(owner, record.kind, record.slot) {
            Ok(lease) => lease,
            Err(error) if error.code == "workspace.busy" && error.details.is_null() => continue,
            Err(error) => return Err(error),
        };
        let parent = record.payload_parent(&owner.home, &owner.paths)?;
        let result = (|| {
            let directory = Directory::open_controlled(&parent, &mut Controlled(&owner.control))?;
            let present = directory
                .optional_metadata_controlled(
                    OsStr::new(&record.payload_name()?),
                    &mut Controlled(&owner.control),
                )
                .map(|value| value.is_some());
            if join(present, close_directory(directory), &owner.control)? {
                return Err(integrity());
            }
            remove_record(owner, &record)
        })();
        join(result, lease.close().err(), &owner.control)?;
    }
    Ok(())
}

/// A validation-only recovery pass needs no new spool or payload allocation,
/// even when every slot or every quota byte is already occupied. It consumes
/// this producer exactly once and explicitly returns a retry-required outcome.
#[allow(clippy::too_many_arguments)]
pub(super) fn recover_spool_once(
    home: &Path,
    source: &SourceIdentity,
    reader: &mut impl Read,
    expected: u64,
    recovery: Option<&BlobRef>,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
    accounting: &impl crate::evidence::CaptureAccounting,
) -> Result<()> {
    use lycheedev_windows::filesystem::owned_cleanup::CleanupCandidate;
    let checks = Checks {
        control,
        supplemental: Some(supplemental),
    };
    let owner = DerivedReservation::acquire_inner_recovering(
        None,
        home,
        crate::workspace::DEFAULT_DATA_CACHE_MAX_BYTES,
        AdditionalPeak::default(),
        control,
        Some(supplemental),
        true,
        recovery,
    )?;
    let mut selected = None;
    let mut slot_lease = None;
    let mut parent = None;
    let mut candidate: Option<CleanupCandidate> = None;
    let mut ancestors = Vec::new();
    let prepared = (|| {
        for (kind, slot) in [ManagedStageKind::Spool, ManagedStageKind::Shard]
            .into_iter()
            .flat_map(|kind| (0..MANAGED_STAGE_SLOTS).map(move |slot| (kind, slot)))
        {
            let record =
                match read_declaration(record_directory(&owner), kind, slot, &owner.control) {
                    Ok(Some(record)) => record,
                    Ok(None) => continue,
                    Err(error)
                        if error.code == "vault.data_shared_blob_integrity"
                            && error.details.is_null() =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error),
                };
            if record.source != *source || record.expected != expected {
                continue;
            }
            let lease = match slot_lock(&owner, record.kind, slot) {
                Ok(lease) => lease,
                Err(error) if error.code == "workspace.busy" && error.details.is_null() => continue,
                Err(error) => return Err(error),
            };
            slot_lease = Some(lease);
            let path = record.payload_parent(&owner.home, &owner.paths)?;
            let mut current = PathBuf::new();
            for component in path.components() {
                current.push(component.as_os_str());
                if matches!(component, std::path::Component::Prefix(_)) {
                    continue;
                }
                if !current.try_exists()? {
                    break;
                }
                ancestors.push(storage::hold_cleanup_ancestor(&current, control)?);
            }
            if path.try_exists()? {
                parent = Some(Directory::open_controlled(&path, &mut Controlled(&checks))?);
                let directory = parent.as_ref().expect("pinned payload parent");
                let name = record.payload_name()?;
                if directory
                    .optional_metadata_controlled(OsStr::new(&name), &mut Controlled(&checks))?
                    .is_some()
                {
                    candidate = Some(
                        directory
                            .open_cleanup_candidate_owner_controlled(
                                OsStr::new(&name),
                                &mut Controlled(&checks),
                            )?
                            .ok_or_else(|| {
                                ToolkitError::new(
                                    "workspace.busy",
                                    "Declared payload is still held",
                                    6,
                                )
                            })?,
                    );
                }
            }
            selected = Some(record);
            break;
        }
        Ok(())
    })();
    // Never hold quota, a blob lease or evidence gate while consuming source.
    let prepared = owner.finish(prepared);
    let result = prepared.and_then(|()| {
        let Some(record) = selected.as_ref() else { return Ok(()); };
        let mut authenticate_source = |old: &mut Option<&mut std::fs::File>| -> Result<BlobRef> {
            let old_length = old.as_mut().map(|file| file.metadata().map(|m|m.len())).transpose()?.unwrap_or(0);
            if old_length > expected { return Err(integrity()); }
            let mut received = 0;
            let mut hash = Sha256::new();
            let mut bytes = [0;32768];
            let mut prior = [0;32768];
            while received < expected {
                checks.check()?;
                accounting.admit_work(1)?;
                let want = (expected-received).min(bytes.len() as u64) as usize;
                let size = reader.read(&mut bytes[..want]).map_err(crate::evidence::stream_source_error)?;
                if size == 0 { return Err(integrity()); }
                hash.update(&bytes[..size]);
                let compare = old_length.saturating_sub(received).min(size as u64) as usize;
                if compare != 0 {
                    old.as_mut().expect("existing prefix").read_exact(&mut prior[..compare])?;
                    if bytes[..compare] != prior[..compare] { return Err(integrity()); }
                }
                received += size as u64;
            }
            checks.check()?;
            accounting.admit_work(1)?;
            // The producer's business authentication and partial-decode policy
            // execute on a genuine nonempty EOF probe, even for empty payloads.
            if reader.read(&mut bytes[..1]).map_err(crate::evidence::stream_source_error)? != 0 { return Err(integrity()); }
            if let Some(old) = old { if old.read(&mut prior[..1])? != 0 { return Err(integrity()); } }
            checks.check()?;
            let reference = BlobRef { sha256: hex::encode(hash.finalize()), bytes: expected };
            if recovery.is_some_and(|expected| !same_blob(expected,&reference))
                || record.kind == ManagedStageKind::Shard && hex::encode(record.digest) != reference.sha256 { return Err(integrity()); }
            Ok(reference)
        };
        let mut authenticated = None;
        let authenticated_source = if let Some(original) = candidate.take() {
            let mut reference = None;
            match original.authenticate(|file| { reference = Some(authenticate_source(&mut Some(file))?); Ok::<(),ToolkitError>(()) }) {
                Ok(owner) => { authenticated = Some(owner); Ok(reference.expect("authenticated source")) },
                Err((error,original)) => { candidate = Some(original); Err(error) },
            }
        } else { authenticate_source(&mut None) };
        let result = authenticated_source.and_then(|reference| {
            let owner = DerivedReservation::acquire_inner_recovering(None,home,crate::workspace::DEFAULT_DATA_CACHE_MAX_BYTES,AdditionalPeak::default(),control,Some(supplemental),true,recovery)?;
            let mut blob_guard = None;
            let result = (|| {
                if record.kind == ManagedStageKind::Shard {
                    blob_guard = Some(ExclusiveLock::resource_wait_checked(&owner.home.join("locks"),&format!("blob:{}",reference.sha256),Some(control.deadline()),|| checks.check())?);
                }
                if let Some(original) = authenticated.take() {
                    let (file,result) = original.discard();
                    finish_file(file,result.map_err(Into::into),&owner.control)?;
                } else if let Some(parent) = parent.as_ref() {
                    if parent.optional_metadata_controlled(OsStr::new(&record.payload_name()?),&mut Controlled(&checks))?.is_some() { return Err(integrity()); }
                } else if record.payload_parent(&owner.home,&owner.paths)?.try_exists()? { return Err(integrity()); }
                if record.kind == ManagedStageKind::Shard { withdraw_recovered_attribution(&owner,record,&reference)?; }
                if let Some(guard) = blob_guard.take() { guard.close()?; }
                if let Some(directory) = parent.take() { join(Ok(()),close_directory(directory),&checks)?; }
                let mut errors = Vec::new();
                while let Some(anchor) = ancestors.pop() { errors.extend(storage::close_file(anchor).err().map(ToolkitError::io)); }
                join(Ok(()),errors,&checks)?;
                if let Some(lease) = slot_lease.take() { lease.close()?; }
                remove_record(&owner,record)
            })();
            let result = join(result,blob_guard.and_then(|guard|guard.close().err()),&checks);
            owner.finish(result)?;
            Err(ToolkitError::new("vault.data_stage_recovered_retry_required", "The authenticated interrupted stage was reclaimed; retry the request to publish using a fresh source",6)
                .with_details(serde_json::json!({"schema":"lycheedev.data-stage-recovery.v2","recovered":true,"publication":{"state":"not_published","reused":false},"sourceConsumed":true,"automaticReplay":false,"authenticatedSource":reference})))
        });
        // A failed quota re-acquisition cannot leak an authenticated owner or
        // convert failure into implicit deletion through Drop.
        join(result,authenticated.take().and_then(|owner| storage::close_file(owner.into_file()).err()).map(ToolkitError::io),&checks)
    });
    let mut cleanup = Vec::new();
    if let Some(candidate) = candidate {
        cleanup.extend(
            storage::close_file(candidate.into_file())
                .err()
                .map(ToolkitError::io),
        );
    }
    if let Some(parent) = parent {
        cleanup.extend(close_directory(parent));
    }
    while let Some(anchor) = ancestors.pop() {
        cleanup.extend(storage::close_file(anchor).err().map(ToolkitError::io));
    }
    if let Some(lease) = slot_lease {
        cleanup.extend(lease.close().err());
    }
    join(result, cleanup, &checks)
}

/// Extra final-shard stages are not in the generic Data roots. Legacy pending
/// attribution already reserves one such stage; completed/missing attribution
/// does not. Count additional declared stages without double-counting that one.
pub(super) fn shard_reservations(directory: &Directory, checks: &Checks<'_>) -> Result<u64> {
    let mut by_digest = BTreeMap::<String, (u64, u64)>::new();
    for slot in 0..MANAGED_STAGE_SLOTS {
        let record = match read_declaration(directory, ManagedStageKind::Shard, slot, checks) {
            Ok(Some(record)) => record,
            Ok(None) => continue,
            Err(error)
                if error.code == "vault.data_shared_blob_integrity" && error.details.is_null() =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        let (bytes, count) = by_digest
            .entry(hex::encode(record.digest))
            .or_insert((record.expected, 0));
        if *bytes != record.expected {
            return Err(integrity());
        }
        *count += 1;
    }
    let mut total = 0;
    for (digest, (bytes, count)) in by_digest {
        let original = read_record(directory, &format!("{digest}.intent"), INTENT_BYTES, checks)?;
        let complete = read_record(
            directory,
            &format!("{digest}.complete"),
            COMPLETE_BYTES,
            checks,
        )?;
        let repair = read_record(
            directory,
            &format!("{digest}.repair.intent"),
            INTENT_BYTES,
            checks,
        )?;
        let covered =
            u64::from(original.is_some() && complete.is_none()) + u64::from(repair.is_some());
        total = sum([
            total,
            bytes
                .checked_mul(count.saturating_sub(covered))
                .ok_or_else(integrity)?,
        ])?;
    }
    Ok(total)
}
pub(super) fn has_any_shard(
    directory: &Directory,
    reference: &BlobRef,
    checks: &Checks<'_>,
) -> Result<bool> {
    for slot in 0..MANAGED_STAGE_SLOTS {
        if let Some(record) = read_declaration(directory, ManagedStageKind::Shard, slot, checks)? {
            if record.expected == reference.bytes && hex::encode(record.digest) == reference.sha256
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn remove_exact_bookkeeping(owner: &DerivedReservation<'_>, name: &str, raw: &[u8]) -> Result<()> {
    let directory = record_directory(owner);
    if directory
        .optional_metadata_controlled(OsStr::new(name), &mut Controlled(&owner.control))?
        .is_none()
    {
        return Ok(());
    }
    let candidate = directory
        .open_cleanup_candidate_owner_controlled(OsStr::new(name), &mut Controlled(&owner.control))?
        .ok_or_else(integrity)?;
    let result = candidate.authenticate(|file| {
        if file.metadata()?.len() != raw.len() as u64 {
            return Err(integrity());
        }
        let mut bytes = [0; 128];
        if raw.len() > bytes.len() {
            return Err(integrity());
        }
        file.read_exact(&mut bytes[..raw.len()])?;
        if &bytes[..raw.len()] != raw || file.read(&mut [0])? != 0 {
            return Err(integrity());
        }
        owner.control.check()
    });
    match result {
        Ok(candidate) => {
            let (file, result) = candidate.discard();
            finish_file(file, result.map_err(Into::into), &owner.control)
        }
        Err((error, candidate)) => finish_file(candidate.into_file(), Err(error), &owner.control),
    }
}
/// Only an authenticated new-protocol declaration and fully checked missing
/// final body permit retiring an unfinished reservation. Committed attribution
/// remains protected; other declared stages continue retaining their own peaks.
fn withdraw_recovered_attribution(
    owner: &DerivedReservation<'_>,
    record: &Declaration,
    reference: &BlobRef,
) -> Result<()> {
    let directory = record_directory(owner);
    let raw = intent(reference)?;
    let done = read_record(
        directory,
        &format!("{}.complete", reference.sha256),
        COMPLETE_BYTES,
        &owner.control,
    )?;
    if done
        .as_ref()
        .is_some_and(|bytes| bytes.as_slice() != completion(&raw))
    {
        return Err(integrity());
    }
    let mut others = false;
    for slot in 0..MANAGED_STAGE_SLOTS {
        if slot == record.slot {
            continue;
        }
        if let Some(other) =
            read_declaration(directory, ManagedStageKind::Shard, slot, &owner.control)?
        {
            if other.digest == record.digest {
                others = true;
                break;
            }
        }
    }
    if !others && done.is_none() && !inspect_body(&owner.home, reference, &owner.control, false)? {
        remove_exact_bookkeeping(owner, &format!("{}.intent", reference.sha256), &raw)?;
    }
    if !others {
        remove_exact_bookkeeping(owner, &format!("{}.repair.intent", reference.sha256), &raw)?;
    }
    Ok(())
}

pub(super) fn finish_repair(owner: &DerivedReservation<'_>, reference: &BlobRef) -> Result<()> {
    remove_exact_bookkeeping(
        owner,
        &format!("{}.repair.intent", reference.sha256),
        &intent(reference)?,
    )
}

#[cfg(test)]
mod tests;

pub(super) fn require_shards_absent(
    owner: &DerivedReservation<'_>,
    source: &SourceIdentity,
    reference: &BlobRef,
) -> Result<()> {
    for slot in 0..MANAGED_STAGE_SLOTS {
        let Some(record) = read_declaration(
            record_directory(owner),
            ManagedStageKind::Shard,
            slot,
            &owner.control,
        )?
        else {
            continue;
        };
        if record.source != *source
            || record.expected != reference.bytes
            || hex::encode(record.digest) != reference.sha256
        {
            continue;
        }
        let lease = slot_lock(owner, record.kind, record.slot)?;
        let parent = record.payload_parent(&owner.home, &owner.paths)?;
        let result = (|| {
            if !parent.try_exists()? {
                return Ok(());
            }
            let directory = Directory::open_controlled(&parent, &mut Controlled(&owner.control))?;
            let present = directory
                .optional_metadata_controlled(
                    OsStr::new(&record.payload_name()?),
                    &mut Controlled(&owner.control),
                )
                .map(|entry| entry.is_some());
            if join(present, close_directory(directory), &owner.control)? {
                return Err(pending(reference, &parent));
            }
            Ok(())
        })();
        join(result, lease.close().err(), &owner.control)?;
    }
    Ok(())
}
