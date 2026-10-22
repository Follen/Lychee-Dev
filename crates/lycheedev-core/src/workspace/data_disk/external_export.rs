//! Durable accounting only. No external pathname, recovery or replay authority.
use super::*;
use lycheedev_windows::filesystem::destination::{
    MANAGED_DECLARATION_BYTES, MANAGED_STAGE_SLOTS, ManagedStageDeclaration, ManagedStageKind,
    ManagedStageName, StagedPublication,
};
use sha2::{Digest, Sha256};
use std::{ffi::OsStr, io::Read};

const KIND: ManagedStageKind = ManagedStageKind::ExternalExport;
const WIRE_KIND: u8 = 3;
fn record(slot: u8, length: u64) -> [u8; MANAGED_DECLARATION_BYTES] {
    let mut bytes = [0; MANAGED_DECLARATION_BYTES];
    bytes[..8].copy_from_slice(b"LDST0002");
    bytes[8] = WIRE_KIND;
    bytes[9] = slot;
    bytes[16..24].copy_from_slice(&length.to_le_bytes());
    let digest = Sha256::digest(&bytes[..96]);
    bytes[96..].copy_from_slice(&digest);
    bytes
}
fn directory<'a>(owner: &'a DerivedReservation<'_>) -> &'a Directory {
    owner
        .directories
        .iter()
        .find(|d| d.path() == owner.paths.shared_blobs())
        .expect("held quota records")
}
pub(super) fn run<T>(
    owner: &DerivedReservation<'_>,
    publish: impl FnOnce() -> (Result<T>, StagedPublication, bool),
) -> Result<T> {
    let parent = directory(owner);
    for slot in 0..MANAGED_STAGE_SLOTS {
        let name = ManagedStageDeclaration::record_name(KIND, slot)?;
        if parent
            .optional_metadata_controlled(OsStr::new(&name), &mut Controlled(&owner.control))?
            .is_some()
        {
            continue;
        }
        // The quota lease serializes participating writers. CREATE_NEW still
        // rejects a foreign entrant racing the occupancy observation.
        let mut declaration =
            ManagedStageDeclaration::create(parent, KIND, slot, &mut Controlled(&owner.control))?;
        let bytes = record(slot, owner.report.additional_peak.export_temporary);
        if let Err(error) = declaration.persist(&bytes, &mut Controlled(&owner.control)) {
            return join(
                Err(error),
                declaration.close().into_iter().map(ToolkitError::io),
                &owner.control,
            );
        }
        let (result, publication, clean_close) = publish();
        let cleanup = if clean_close && publication != StagedPublication::Unknown {
            declaration.withdraw()
        } else {
            declaration.close()
        };
        return join(
            result,
            cleanup.into_iter().map(ToolkitError::io),
            &owner.control,
        );
    }
    Err(ToolkitError::new(
        "vault.data_stage_slots_exhausted",
        "All external export accounting slots are occupied; pending charges are retained",
        6,
    ))
}

/// Record bytes are measured by the ordinary indexes scan. Only the valid
/// declaration's external allowance is added here. Torn records authorize no
/// external creation and occupy their existing slot without being adopted.
pub(super) fn inventory(
    paths: &DataCachePaths,
    checks: &Checks<'_>,
    held: &[Directory],
) -> Result<u64> {
    with_inventory_directory(&paths.shared_blobs(), held, checks, |parent| {
        let mut pending = 0;
        for slot in shared_blobs::durable::existing_record_slots(parent, KIND, checks)? {
            let name = ManagedStageDeclaration::record_name(KIND, slot)?;
            let Some(mut file) =
                parent.open_optional_file_controlled(OsStr::new(&name), &mut Controlled(checks))?
            else {
                continue;
            };
            let result = (|| {
                if file.metadata()?.len() != MANAGED_DECLARATION_BYTES as u64 {
                    return Ok(None);
                }
                let mut bytes = [0; MANAGED_DECLARATION_BYTES];
                file.read_exact(&mut bytes)?;
                checks.check()?;
                if file.read(&mut [0])? != 0
                    || bytes[8] != WIRE_KIND
                    || bytes[9] != slot
                    || ManagedStageName::from_declaration(&bytes).is_err()
                {
                    return Ok(None);
                }
                Ok(Some(u64::from_le_bytes(
                    bytes[16..24].try_into().expect("fixed record"),
                )))
            })();
            if let Some(length) = finish_file(file, result, checks)? {
                pending = sum([pending, length])?;
            }
        }
        Ok(pending)
    })
}

#[cfg(test)]
mod tests;
