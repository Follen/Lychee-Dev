//! Formal Go 3.2.0 disk publication, separate from runtime/input authority.
//!
//! Oracle: `3057a9666ab078270b63c7f06de748069b0d1f4b`,
//! delivery/{slots,slot_route,slot_retirement}.go and channel/native_windows.go.
//! The shared core DTO/encoder is the only protocol implementation. A short
//! lease covers publication or verification plus physical input; release it
//! BEFORE waiting for a receipt. The durable reservation protects that wait.
//! Receipt/process/runtime proof verification remains the caller's obligation.
use crate::{
    installation::{self, Receipt, VerifiedInstallation},
    slots::{self, SlotFile, SlotPool},
};
use lycheedev_core::{
    ToolkitError,
    control::OperationControl,
    coordination::InstallationMaintenanceLease,
    mailbox::{self, ExpectedEnvelope, SlotEnvelope},
    selection::valid_digest,
    storage::{self, ExclusiveLock},
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::Duration,
};

const PAYLOAD_LIMIT: usize = 2 << 20;
const RECEIPT_LIMIT: usize = 1 << 20;
const INERT: &[u8] = b"LycheeDevSlotEnvelope = nil\n";

/// Preserve both causes of Go errors.Join(ErrConflict, ErrSlotReserved).
/// The exact reservation is captured under the lease for the caller's blocker
/// diagnostics. It must not be reread after unlocking and attributed to this error.
#[derive(Debug, Clone)]
pub enum PublicationError {
    Failure(ToolkitError),
    Reserved {
        reservation: SlotFile,
    },
    /// Ordered acquisition/close causes. Acquisition failures precede lock
    /// closes; normal close reports receipt, publication, then maintenance.
    /// Operations return Reserved separately, preserving its captured owner when
    /// callers join an operation error with this checked-close result.
    CloseFailures(Vec<ToolkitError>),
}
pub type Result<T> = std::result::Result<T, PublicationError>;
impl PublicationError {
    pub fn is_code(&self, code: &str) -> bool {
        match self {
            Self::Failure(error) => error.code == code,
            Self::Reserved { .. } => {
                matches!(
                    code,
                    "delivery.installation_conflict" | "delivery.slot_reserved"
                )
            }
            Self::CloseFailures(causes) => causes.iter().any(|cause| cause.code == code),
        }
    }
}
impl From<ToolkitError> for PublicationError {
    fn from(error: ToolkitError) -> Self {
        Self::Failure(error)
    }
}
impl std::fmt::Display for PublicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failure(error) => write!(f, "{error}"),
            Self::Reserved { .. } => {
                f.write_str("delivery.installation_conflict: delivery.slot_reserved")
            }
            Self::CloseFailures(causes) => {
                for (index, cause) in causes.iter().enumerate() {
                    if index != 0 {
                        f.write_str("\n")?;
                    }
                    write!(f, "{cause}")?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for PublicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Failure(error) => Some(error),
            Self::Reserved { .. } => None,
            Self::CloseFailures(causes) => {
                causes.first().map(|error| error as &dyn std::error::Error)
            }
        }
    }
}
fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("delivery.invalid_installation", message, 4)
}
fn conflict(message: &str) -> PublicationError {
    ToolkitError::new("delivery.installation_conflict", message, 3).into()
}
fn retirement_conflict() -> PublicationError {
    ToolkitError::new(
        "delivery.slot_retirement_conflict",
        "slot retirement identity or successor proof differs",
        5,
    )
    .into()
}
fn payload(envelope: &SlotEnvelope) -> Result<Vec<u8>> {
    mailbox::slot_payload(envelope).map_err(|error| ToolkitError::new(error.0, error.0, 4).into())
}

/// Installation identity from an already authenticated durable record. It is
/// a source/path binding, NOT a claim that the current addon is still clean.
/// This lets exact receipt/process cleanup survive unrelated addon-file drift.
#[derive(Debug, Clone)]
pub struct RecordedInstallationIdentity {
    canonical_path: PathBuf,
    manifest_sha256: Option<String>,
}
impl RecordedInstallationIdentity {
    /// The caller authenticates the record and its selected installation before
    /// calling this. Structural validation here does not establish provenance.
    /// Lease acquisition checks the canonical path and current slot pool.
    /// Ordinary publication/input verification reads the current addon receipt.
    /// A path-only record cannot publish. New business publication needs a clean
    /// VerifiedInstallation; original repair cleanup needs RepairPublication.
    pub fn from_authenticated_record(canonical_path: PathBuf) -> Result<Self> {
        if !canonical_path.is_absolute() {
            return Err(invalid("recorded installation identity is malformed").into());
        }
        Ok(Self {
            canonical_path,
            manifest_sha256: None,
        })
    }
}
impl From<&VerifiedInstallation> for RecordedInstallationIdentity {
    fn from(identity: &VerifiedInstallation) -> Self {
        Self {
            canonical_path: identity.canonical_path.clone(),
            manifest_sha256: Some(identity.manifest_sha256.clone()),
        }
    }
}

/// Cleanup-only authority for one exact original-CON unbind or finalization. The fields are
/// private and immutable: this cannot authorize bind, business execution, a new
/// runtime, or an arbitrary replacement envelope. It is not an INPUT witness.
///
/// As with RecordedInstallationIdentity, the caller authenticates provenance:
/// read the original CON under its execution lease, durably request close, and
/// supply that journal's exact cleanup transaction, original consumer and
/// Identity.Release. Recheck the original client window and ownership before
/// acquisition and again after admission, as Go connectionGuard(repair=true)
/// does. Do not use the current CLI release or an unauthenticated descriptor.
#[derive(Debug, Clone)]
pub struct RepairPublication {
    identity: RecordedInstallationIdentity,
    release: String,
    consumer: String,
    envelope: SlotEnvelope,
}
impl RepairPublication {
    pub fn from_authenticated_record(
        identity: &RecordedInstallationIdentity,
        original_release: &str,
        original_consumer: &str,
        exact_unbind: &SlotEnvelope,
    ) -> Result<Self> {
        // Use the shared Go payload validator, not a stricter cleanup wire.
        // Other action fields remain byte-exact, including fields Go ignores.
        if original_release.is_empty()
            || original_consumer.is_empty()
            || exact_unbind.action != "unbind"
        {
            return Err(invalid(
                "repair publication requires the original release, consumer and unbind",
            )
            .into());
        }
        payload(exact_unbind)?;
        Ok(Self {
            identity: identity.clone(),
            release: original_release.into(),
            consumer: original_consumer.into(),
            envelope: exact_unbind.clone(),
        })
    }
    /// Finalization-only counterpart to the unbind constructor. The caller
    /// must authenticate the exact durable original-CON transaction and its
    /// operation: confirm_ready with validated report candidate for confirm;
    /// release_ready after result artifact + result_durable for release.
    /// This never grants prepare/commit or arbitrary envelope authority.
    pub fn from_authenticated_finalization(
        identity: &RecordedInstallationIdentity,
        original_release: &str,
        original_consumer: &str,
        exact_finalization: &SlotEnvelope,
    ) -> Result<Self> {
        if original_release.is_empty()
            || original_consumer.is_empty()
            || !matches!(exact_finalization.action.as_str(), "confirm" | "release")
        {
            return Err(invalid("repair finalization requires original confirm or release").into());
        }
        payload(exact_finalization)?;
        Ok(Self {
            identity: identity.clone(),
            release: original_release.into(),
            consumer: original_consumer.into(),
            envelope: exact_finalization.clone(),
        })
    }
    pub fn envelope(&self) -> &SlotEnvelope {
        &self.envelope
    }
    pub fn consumer(&self) -> &str {
        &self.consumer
    }
    fn check_lease(&self, store: &DiskStore) -> Result<()> {
        if self.identity.canonical_path != store.addon || self.release != store.version {
            return Err(conflict(
                "repair publication differs from the original installation or release",
            ));
        }
        Ok(())
    }
}

/// A caller-owned, original-budget short lease. It must not survive a receipt
/// wait, diagnostic write, or callback that acquires installation maintenance.
/// No durable claim is created or retired here. Dropping it never consumes a slot.
///
/// Lock order is existing Rust installation maintenance, then the exact Go
/// publication namespace. This excludes current Rust installation writers; it
/// does not replace the separate Go window-admission/connection execution lease.
/// Keep this value alive from verify + verify_route through the physical burst.
/// Do not acquire it while already holding an InstallationMaintenanceLease.
pub struct PublicationLease<'a> {
    store: DiskStore,
    control: &'a OperationControl,
    // Drop publication before maintenance. Neither lock file is ever deleted.
    _publication: ExclusiveLock,
    _maintenance: InstallationMaintenanceLease,
}
impl<'a> PublicationLease<'a> {
    pub fn acquire(
        identity: &VerifiedInstallation,
        version: &str,
        control: &'a OperationControl,
    ) -> Result<Self> {
        Self::acquire_recorded(&identity.into(), version, control)
    }
    /// Restore an authenticated source identity without pretending that the
    /// addon is currently clean. Ordinary publish/pre-input verify require clean
    /// addon bytes; repair-only cleanup and retirement inspect the original pool.
    pub fn acquire_recorded(
        identity: &RecordedInstallationIdentity,
        version: &str,
        control: &'a OperationControl,
    ) -> Result<Self> {
        control.check()?;
        let maintenance = wait_lock(
            control,
            || InstallationMaintenanceLease::acquire(&identity.canonical_path),
            InstallationMaintenanceLease::close,
        )?;
        let (maintenance, (publication, store)) = with_acquired(
            maintenance,
            |maintenance| {
                control.check()?;
                if maintenance.target != identity.canonical_path {
                    return Err(invalid("installation canonical identity changed").into());
                }
                let parent = maintenance
                    .target
                    .parent()
                    .ok_or_else(|| invalid("addon has no parent"))?
                    .to_path_buf();
                let scope = parent.join(".lycheedev-slot-locks");
                let lock_path = scope.join(format!("{}.lock", storage::sha256(b"publication")));
                storage::guard_path(&parent, &scope)?;
                storage::guard_path(&parent, &lock_path)?;
                let publication = wait_lock(
                    control,
                    || ExclusiveLock::resource(&scope, "publication"),
                    ExclusiveLock::close,
                )?;
                with_acquired(
                    publication,
                    |_| {
                        storage::guard_path(&parent, &lock_path)?;
                        control.check()?;
                        let mut store = DiskStore {
                            parent,
                            addon: maintenance.target.clone(),
                            version: version.into(),
                            manifest_sha256: identity.manifest_sha256.clone(),
                            _receipt_file: None,
                        };
                        store.inspect(control)?;
                        Ok(store)
                    },
                    ExclusiveLock::close,
                )
            },
            InstallationMaintenanceLease::close,
        )?;
        Ok(Self {
            store,
            control,
            _publication: publication,
            _maintenance: maintenance,
        })
    }
    /// Read-only allocation snapshot. A hint is never overwrite authority.
    pub fn inspect(&mut self) -> Result<SlotPool> {
        Ok(self.store.inspect(self.control)?)
    }
    /// End the short lease BEFORE a receipt wait. Consume the original retained
    /// receipt first, then both locks, checking every close even after failure or
    /// cancellation. Drop is only the unwind/caller-omitted-close fallback.
    /// Join this result with the operation result without flattening Reserved.
    /// A close failure never rolls back publication or authorizes replay.
    pub fn close(self) -> Result<()> {
        self.close_with(
            |file, control| storage::finish_file(file, Ok(()), control),
            ExclusiveLock::close,
            InstallationMaintenanceLease::close,
        )
    }
    fn close_with(
        self,
        finish_receipt: impl FnOnce(File, &OperationControl) -> lycheedev_core::Result<()>,
        close_publication: impl FnOnce(ExclusiveLock) -> lycheedev_core::Result<()>,
        close_maintenance: impl FnOnce(InstallationMaintenanceLease) -> lycheedev_core::Result<()>,
    ) -> Result<()> {
        let Self {
            mut store,
            control,
            _publication,
            _maintenance,
        } = self;
        let receipt = store
            ._receipt_file
            .take()
            .map_or(Ok(()), |file| finish_receipt(file, control));
        let publication = close_publication(_publication);
        let maintenance = close_maintenance(_maintenance);
        close_causes([receipt, publication, maintenance])
    }
    /// Publish only the immutable cleanup transaction admitted from the original CON. No
    /// current addon receipt is needed: the validated slot inventory, exact
    /// reservation and original release still apply, matching Go CloseForRepair.
    pub fn publish_repair(&mut self, repair: &RepairPublication) -> Result<()> {
        repair.check_lease(&self.store)?;
        publish(
            &mut self.store,
            self.control,
            &repair.consumer,
            &repair.envelope,
        )
    }
    /// Keep this SAME lease through fresh INPUT/route revalidation and the
    /// physical burst. This only verifies disk intent, never authorizes input.
    pub fn verify_repair(&mut self, repair: &RepairPublication) -> Result<()> {
        repair.check_lease(&self.store)?;
        verify(
            &mut self.store,
            self.control,
            &repair.consumer,
            &repair.envelope,
        )
    }
    pub fn verify_repair_route(&mut self, repair: &RepairPublication) -> Result<()> {
        repair.check_lease(&self.store)?;
        verify_route(&mut self.store, self.control, &repair.envelope)
    }
    pub fn publish(&mut self, consumer: &str, envelope: &SlotEnvelope) -> Result<()> {
        self.store.verify_addon(self.control)?;
        publish(&mut self.store, self.control, consumer, envelope)
    }
    /// Revalidate under the SAME lease retained through physical input.
    pub fn verify(&mut self, consumer: &str, envelope: &SlotEnvelope) -> Result<()> {
        self.store.verify_addon(self.control)?;
        verify(&mut self.store, self.control, consumer, envelope)
    }
    /// Check actual skipped files, including the old bytes of a torn write.
    /// Call after verify, under this lease; it retires no foreign reservation.
    pub fn verify_route(&mut self, envelope: &SlotEnvelope) -> Result<()> {
        verify_route(&mut self.store, self.control, envelope)
    }
    /// The caller must FIRST durably record its exact verified runtime receipt.
    /// ExpectedEnvelope is the shared receipt-correlation DTO. As in Go, this
    /// disk step checks index/nonce/consumer, not a second invented receipt wire.
    /// A newer nonce is an idempotent no-op, never marked consumed by an old ACK.
    pub fn confirm_consumed(&mut self, consumer: &str, expected: &ExpectedEnvelope) -> Result<()> {
        confirm_consumed(&mut self.store, self.control, consumer, expected)
    }
    /// Requires the caller's OS-proven ended PID/creation-time consumer. This
    /// only reconciles exact disk intent; it never fabricates a consumed receipt.
    pub fn retire_process(&mut self, consumer: &str, envelope: &SlotEnvelope) -> Result<()> {
        retire_process(&mut self.store, self.control, consumer, envelope)
    }
    /// Requires caller-verified fresh bind or durable different-runtime proof
    /// in the SAME selected PID/creation-time consumer. Descriptor discovery,
    /// a timeout, or a queued reload is not proof. Token ordering has no meaning.
    pub fn retire_runtime(
        &mut self,
        consumer: &str,
        envelope: &SlotEnvelope,
        current_runtime: &str,
    ) -> Result<()> {
        retire_runtime(
            &mut self.store,
            self.control,
            consumer,
            envelope,
            current_runtime,
        )
    }
}
#[cfg(test)]
fn close_results(
    publication: lycheedev_core::Result<()>,
    maintenance: lycheedev_core::Result<()>,
) -> Result<()> {
    close_causes([publication, maintenance])
}
fn close_causes<const N: usize>(results: [lycheedev_core::Result<()>; N]) -> Result<()> {
    let mut causes: Vec<_> = results
        .into_iter()
        .filter_map(std::result::Result::err)
        .collect();
    match causes.len() {
        0 => Ok(()),
        1 => Err(PublicationError::Failure(causes.remove(0))),
        _ => Err(PublicationError::CloseFailures(causes)),
    }
}

// Construction only admits ToolkitError causes. Keep this separate from the
// public operation error so Reserved and its captured metadata cannot be lost
// in a cleanup join. The primary error remains first, with each consumed lock's
// close error appended exactly once while the nested scopes unwind normally.
#[derive(Debug)]
struct AcquisitionFailure(Vec<ToolkitError>);
type AcquisitionResult<T> = std::result::Result<T, AcquisitionFailure>;
impl From<ToolkitError> for AcquisitionFailure {
    fn from(error: ToolkitError) -> Self {
        Self(vec![error])
    }
}
impl From<AcquisitionFailure> for PublicationError {
    fn from(mut error: AcquisitionFailure) -> Self {
        if error.0.len() == 1 {
            Self::Failure(error.0.remove(0))
        } else {
            Self::CloseFailures(error.0)
        }
    }
}
fn with_acquired<G, T>(
    guard: G,
    operation: impl FnOnce(&G) -> AcquisitionResult<T>,
    close: impl FnOnce(G) -> lycheedev_core::Result<()>,
) -> AcquisitionResult<(G, T)> {
    match operation(&guard) {
        Ok(value) => Ok((guard, value)),
        Err(mut error) => {
            // Cleanup never consults the cancelled/expired operation control.
            // close consumes the only owner, even when it reports a failure.
            if let Err(cause) = close(guard) {
                error.0.push(cause);
            }
            Err(error)
        }
    }
}
fn wait_lock<T>(
    control: &OperationControl,
    mut attempt: impl FnMut() -> lycheedev_core::Result<T>,
    close: impl FnOnce(T) -> lycheedev_core::Result<()>,
) -> AcquisitionResult<T> {
    loop {
        control.check()?;
        match attempt() {
            Ok(guard) => {
                return with_acquired(guard, |_| Ok(control.check()?), close)
                    .map(|(guard, ())| guard);
            }
            Err(error) if error.code == "workspace.busy" => {
                std::thread::sleep(control.remaining()?.min(Duration::from_millis(25)));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Search strictly forward; zero starts at the first slot. Pending always wins
/// over consumed/retired flags. The caller inspects the pool under its lease;
/// this pure hint neither authenticates the pool nor authorizes publication.
pub fn next_free_slot(pool: &SlotPool, after: i32) -> Option<i32> {
    if pool.files.len() > slots::COUNT as usize || after < 0 || after as usize >= pool.files.len() {
        return None;
    }
    pool.files
        .iter()
        .enumerate()
        .skip(after as usize)
        .find_map(|(index, slot)| {
            (slot.pending_hash.is_empty()
                && (slot.nonce.is_empty()
                    || slot.consumed
                    || !slot.retired_runtime.is_empty()
                    || slot.retired_process))
                .then_some(index as i32 + 1)
        })
}

// One algorithm, with test-only fault injection at its persistence boundary.
// A failed writer may have committed; never repair or roll it back on error.
trait Store {
    fn inspect(&mut self, control: &OperationControl) -> lycheedev_core::Result<SlotPool>;
    fn read_payload(
        &mut self,
        index: i32,
        control: &OperationControl,
    ) -> lycheedev_core::Result<Vec<u8>>;
    fn save(&mut self, pool: &SlotPool, control: &OperationControl) -> lycheedev_core::Result<()>;
    fn replace_payload(
        &mut self,
        index: i32,
        bytes: &[u8],
        control: &OperationControl,
    ) -> lycheedev_core::Result<()>;
}
struct DiskStore {
    parent: PathBuf,
    addon: PathBuf,
    version: String,
    manifest_sha256: Option<String>,
    // After a clean publication/input check, retain the read-only receipt handle
    // through the physical burst. Cleanup alone never needs this file to exist.
    _receipt_file: Option<File>,
}
impl Store for DiskStore {
    fn inspect(&mut self, control: &OperationControl) -> lycheedev_core::Result<SlotPool> {
        control.check()?;
        slots::inspect_typed(&self.parent, &self.version, control)
    }
    fn read_payload(
        &mut self,
        index: i32,
        control: &OperationControl,
    ) -> lycheedev_core::Result<Vec<u8>> {
        control.check()?;
        let bytes = slots::read_bounded(
            &slots::directory(&self.parent, index as u16).join("Payload.lua"),
            PAYLOAD_LIMIT,
        )?;
        control.check()?;
        Ok(bytes)
    }
    fn save(&mut self, pool: &SlotPool, control: &OperationControl) -> lycheedev_core::Result<()> {
        slots::save_pool(&self.parent, pool, control)
    }
    fn replace_payload(
        &mut self,
        index: i32,
        bytes: &[u8],
        control: &OperationControl,
    ) -> lycheedev_core::Result<()> {
        let path = slots::directory(&self.parent, index as u16).join("Payload.lua");
        storage::guard_path(&self.parent, &path)?;
        storage::slot_atomic_write_controlled(&path, bytes, control)
    }
}
impl DiskStore {
    fn verify_addon(&mut self, control: &OperationControl) -> lycheedev_core::Result<()> {
        self.verify_addon_with_finish(control, |file, result| {
            storage::finish_file(file, result, control)
        })
    }
    fn verify_addon_with_finish(
        &mut self,
        control: &OperationControl,
        finish: impl FnOnce(File, lycheedev_core::Result<()>) -> lycheedev_core::Result<()>,
    ) -> lycheedev_core::Result<()> {
        control.check()?;
        // Fail before opening when this is only a recorded cleanup identity.
        self.manifest_digest()?;
        let marker = self.addon.join(installation::MARKER);
        storage::guard_path(&self.addon, &marker)?;
        let (receipt_file, digest) =
            storage::locked_sha256(&marker, RECEIPT_LIMIT as u64, control)?;
        self.verify_opened_receipt(receipt_file, &digest, control, finish)
    }
    fn manifest_digest(&self) -> lycheedev_core::Result<&str> {
        self.manifest_sha256.as_deref().ok_or_else(|| {
            ToolkitError::new(
                "delivery.installation_not_clean",
                "publication requires an authenticated clean installation snapshot",
                3,
            )
        })
    }
    fn verify_opened_receipt(
        &mut self,
        mut file: File,
        digest: &str,
        control: &OperationControl,
        finish: impl FnOnce(File, lycheedev_core::Result<()>) -> lycheedev_core::Result<()>,
    ) -> lycheedev_core::Result<()> {
        let result = self.validate_receipt(&mut file, digest, control);
        match result {
            Ok(()) if self._receipt_file.is_none() => {
                // Transfer the original successful owner, not a cloned or
                // reopened substitute. Keep it until the physical burst ends.
                self._receipt_file = Some(file);
                Ok(())
            }
            result => {
                // Every normal rejection checks close, including digest, seek,
                // read, parse, inventory and control failures. On revalidation,
                // close this temporary handle; never replace/drop the original
                // receipt retained across the successful publication and burst.
                finish(file, result)
            }
        }
    }
    fn validate_receipt(
        &self,
        receipt_file: &mut (impl Read + Seek),
        digest: &str,
        control: &OperationControl,
    ) -> lycheedev_core::Result<()> {
        let manifest_sha256 = self.manifest_digest()?;
        if digest != manifest_sha256 {
            return Err(invalid("installation receipt identity changed"));
        }
        receipt_file
            .seek(SeekFrom::Start(0))
            .map_err(ToolkitError::io)?;
        let mut bytes = Vec::new();
        let mut block = [0u8; 65536];
        loop {
            control.check()?;
            let count = receipt_file.read(&mut block).map_err(ToolkitError::io)?;
            if count == 0 {
                break;
            }
            if count > RECEIPT_LIMIT.saturating_sub(bytes.len()) {
                return Err(invalid("installation receipt exceeds byte limit"));
            }
            bytes.extend_from_slice(&block[..count]);
        }
        let receipt: Receipt = serde_json::from_slice(&bytes)
            .map_err(|error| invalid(&format!("invalid installation receipt: {error}")))?;
        if storage::sha256(&bytes) != manifest_sha256
            || receipt.schema != "lycheedev.installation.v1"
            || receipt.component != "addon"
            || receipt.version != self.version
            || self.version.is_empty()
            || !valid_digest(&receipt.commit, 40)
        {
            return Err(invalid(
                "installation receipt does not match selected release",
            ));
        }
        crate::release::verify_inventory_with_control(
            &self.addon,
            &receipt.resources,
            Some("addon"),
            control,
        )?;
        control.check()
    }
}

fn slot_index(pool: &SlotPool, index: i32) -> Result<usize> {
    if index < 1 || index as usize > pool.files.len() {
        return Err(invalid("slot index is outside the current inventory").into());
    }
    Ok(index as usize - 1)
}
fn envelope_index(pool: &SlotPool, envelope: &SlotEnvelope, consumer: &str) -> Result<usize> {
    if pool.schema != slots::SCHEMA || envelope.schema != mailbox::SCHEMA || consumer.is_empty() {
        return Err(invalid("publication requires current schema and consumer").into());
    }
    slot_index(pool, envelope.index)
}
fn publish(
    store: &mut impl Store,
    control: &OperationControl,
    consumer: &str,
    envelope: &SlotEnvelope,
) -> Result<()> {
    let mut pool = store.inspect(control)?;
    let index = envelope_index(&pool, envelope, consumer)?;
    let entry = &mut pool.files[index];
    if !entry.nonce.is_empty()
        && !entry.consumed
        && entry.retired_runtime.is_empty()
        && !entry.retired_process
        && (entry.nonce != envelope.nonce
            || entry.consumer != consumer
            || entry.runtime != envelope.runtime)
    {
        return Err(PublicationError::Reserved {
            reservation: entry.clone(),
        });
    }
    let bytes = payload(envelope)?;
    let hash = storage::sha256(&bytes);
    if entry.nonce == envelope.nonce
        && ((!entry.pending_hash.is_empty() && entry.pending_hash != hash)
            || (entry.pending_hash.is_empty() && entry.payload_hash != hash))
    {
        return Err(conflict("changed slot intent"));
    }
    if entry.nonce == envelope.nonce && entry.payload_hash == hash && entry.pending_hash.is_empty()
    {
        return Ok(());
    }
    entry.pending_hash = hash.clone();
    entry.retired_runtime.clear();
    entry.retired_process = false;
    entry.nonce.clone_from(&envelope.nonce);
    entry.runtime.clone_from(&envelope.runtime);
    entry.consumer = consumer.into();
    entry.consumed = false;
    store.save(&pool, control)?;
    store.replace_payload(envelope.index, &bytes, control)?;
    pool.files[index].payload_hash = hash;
    pool.files[index].pending_hash.clear();
    store.save(&pool, control)?;
    Ok(())
}
fn verify(
    store: &mut impl Store,
    control: &OperationControl,
    consumer: &str,
    envelope: &SlotEnvelope,
) -> Result<()> {
    let pool = store.inspect(control)?;
    let entry = &pool.files[envelope_index(&pool, envelope, consumer)?];
    let bytes = payload(envelope)?;
    if entry.nonce != envelope.nonce
        || entry.runtime != envelope.runtime
        || entry.consumer != consumer
        || !entry.pending_hash.is_empty()
        || entry.consumed
        || !entry.retired_runtime.is_empty()
        || entry.retired_process
        || entry.payload_hash != storage::sha256(&bytes)
    {
        return Err(conflict(
            "slot publication differs or is no longer input-authorized",
        ));
    }
    Ok(())
}
fn confirm_consumed(
    store: &mut impl Store,
    control: &OperationControl,
    consumer: &str,
    expected: &ExpectedEnvelope,
) -> Result<()> {
    let mut pool = store.inspect(control)?;
    let index = slot_index(&pool, expected.index)?;
    let entry = &mut pool.files[index];
    if expected.nonce.is_empty() || consumer.is_empty() {
        return Err(conflict("receipt nonce or consumer is empty"));
    }
    if entry.nonce != expected.nonce {
        return Ok(());
    }
    if entry.consumer != consumer || !entry.pending_hash.is_empty() {
        return Err(conflict("receipt differs from exact committed reservation"));
    }
    entry.consumed = true;
    store.save(&pool, control)?;
    Ok(())
}
fn retire_process(
    store: &mut impl Store,
    control: &OperationControl,
    consumer: &str,
    envelope: &SlotEnvelope,
) -> Result<()> {
    if consumer.is_empty()
        || !(1..=mailbox::SLOT_COUNT).contains(&envelope.index)
        || envelope.nonce.is_empty()
    {
        return Err(invalid("ended-process retirement identity is incomplete").into());
    }
    let mut pool = store.inspect(control)?;
    let index = slot_index(&pool, envelope.index)?;
    let entry = &mut pool.files[index];
    if entry.nonce != envelope.nonce {
        return Ok(());
    }
    if entry.consumer != consumer || entry.runtime != envelope.runtime {
        return Err(retirement_conflict());
    }
    if entry.consumed || entry.retired_process {
        return Ok(());
    }
    reconcile_payload(store, control, envelope.index, entry)?;
    entry.retired_process = true;
    store.save(&pool, control)?;
    Ok(())
}
fn retire_runtime(
    store: &mut impl Store,
    control: &OperationControl,
    consumer: &str,
    envelope: &SlotEnvelope,
    current_runtime: &str,
) -> Result<()> {
    // Deliberately the Go lower-level length check, not invented chronology or
    // token syntax. The Live proof verifier owns stronger current-token checks.
    if consumer.is_empty()
        || envelope.nonce.is_empty()
        || current_runtime == envelope.runtime
        || current_runtime.len() != 32
        || !(1..=mailbox::SLOT_COUNT).contains(&envelope.index)
    {
        return Err(invalid("replacement-runtime retirement identity is incomplete").into());
    }
    let mut pool = store.inspect(control)?;
    let index = slot_index(&pool, envelope.index)?;
    let entry = &mut pool.files[index];
    if entry.nonce != envelope.nonce {
        return Ok(());
    }
    if entry.consumer != consumer || entry.runtime != envelope.runtime {
        return Err(retirement_conflict());
    }
    if entry.consumed {
        return Ok(());
    }
    if !entry.retired_runtime.is_empty() && entry.retired_runtime != current_runtime {
        return Err(retirement_conflict());
    }
    reconcile_payload(store, control, envelope.index, entry)?;
    entry.retired_runtime = current_runtime.into();
    store.save(&pool, control)?;
    Ok(())
}
fn reconcile_payload(
    store: &mut impl Store,
    control: &OperationControl,
    index: i32,
    entry: &mut SlotFile,
) -> Result<()> {
    let hash = storage::sha256(&store.read_payload(index, control)?);
    if hash != entry.payload_hash && hash != entry.pending_hash {
        return Err(invalid("retirement found unjournaled payload bytes").into());
    }
    entry.payload_hash = hash;
    entry.pending_hash.clear();
    Ok(())
}
fn verify_route(
    store: &mut impl Store,
    control: &OperationControl,
    envelope: &SlotEnvelope,
) -> Result<()> {
    let start = if envelope.start_slot == 0 {
        envelope.index
    } else {
        envelope.start_slot
    };
    if start < 1
        || start > envelope.index
        || envelope.index > mailbox::SLOT_COUNT
        || (start != envelope.index && envelope.schema != mailbox::SCHEMA)
    {
        return Err(invalid("invalid forward slot route").into());
    }
    for index in start..envelope.index {
        control.check()?;
        let bytes = store.read_payload(index, control)?;
        if bytes != INERT && !foreign_route_payload(&bytes, index, &envelope.runtime) {
            return Err(conflict(&format!("slot route blocked at {index}")));
        }
    }
    Ok(())
}
fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|part| part == needle)
}
fn foreign_route_payload(bytes: &[u8], index: i32, runtime: &str) -> bool {
    // Byte matching reproduces Go's anchored multiline regexp without decoding
    // or executing Lua, or imposing UTF-8 on arbitrary rejected file contents.
    let mut runtimes = bytes.split(|byte| *byte == b'\n').filter_map(|line| {
        let token = line.strip_prefix(b"  runtime = \"")?.strip_suffix(b"\",")?;
        (token.len() == 32
            && token
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)))
        .then_some(token)
    });
    let Some(found) = runtimes.next() else {
        return false;
    };
    if runtimes.next().is_some() || found == runtime.as_bytes() {
        return false;
    }
    let action = bytes.split(|byte| *byte == b'\n').any(|line| {
        let Some(action) = line
            .strip_prefix(b"  action = \"")
            .and_then(|s| s.strip_suffix(b"\","))
        else {
            return false;
        };
        matches!(
            action,
            b"bind" | b"prepare" | b"commit" | b"confirm" | b"release" | b"unbind"
        )
    });
    action
        && contains(
            bytes,
            format!("  schema = \"{}\",\n", mailbox::SCHEMA).as_bytes(),
        )
        && contains(bytes, format!("  index = {index}, fence = ").as_bytes())
}

#[cfg(test)]
#[path = "slot_publication_tests.rs"]
mod tests;
