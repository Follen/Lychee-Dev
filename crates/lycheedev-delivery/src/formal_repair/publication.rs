use super::*;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Replacement {
    schema: String,
    target: PathBuf,
    previous: Receipt,
    next: Receipt,
}
/// Returns true exactly when the prepared addon leaves the private snapshot.
/// No std::rename, overwrite, copy/delete fallback or recursive deletion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Point {
    Prepared,
    ArchivePublished,
    PreviousArchived,
}
pub(super) fn publish(
    target: &Path,
    prepared: &Path,
    archive: Option<&Path>,
    wanted: &Receipt,
    current: &Value,
    control: &OperationControl,
) -> Result<bool> {
    publish_with(
        target,
        prepared,
        archive,
        wanted,
        current,
        control,
        &mut |_| Ok(()),
    )
}
pub(super) fn publish_with(
    target: &Path,
    prepared: &Path,
    archive: Option<&Path>,
    wanted: &Receipt,
    current: &Value,
    control: &OperationControl,
    checkpoint: &mut impl FnMut(Point) -> Result<()>,
) -> Result<bool> {
    if let Some(archive) = archive {
        if current["state"] != "managed" {
            return Err(conflict("upgrade requires unchanged managed installation"));
        }
        if present(archive)? {
            return Err(conflict(
                "archive already exists; use explicit original upgrade recovery",
            ));
        }
        let previous: Receipt = serde_json::from_value(current["receipt"].clone())?;
        let parent = archive
            .parent()
            .ok_or_else(|| invalid("archive parent missing"))?;
        control.check()?;
        let staging = snapshot::create_private(parent, ".lycheedev-prepare-", target, control)?;
        let record = Replacement {
            schema: "lycheedev.replacement.v1".into(),
            target: target.into(),
            previous,
            next: wanted.clone(),
        };
        let result = (|| {
            storage::atomic_write_controlled(
                &staging.join(snapshot::OWNER),
                &storage::bounded_json(
                    &snapshot::ownership(target, &wanted.version, &wanted.commit),
                    4096,
                )?,
                control,
            )?;
            // Prepare the full verified next tree and durable record before the
            // requested archive becomes visible, exactly as fixed Go upgrade.
            move_checked(prepared, &staging.join("next"), control)?;
            storage::atomic_write_controlled(
                &staging.join("replacement.json"),
                &storage::bounded_json(&record, 1 << 20)?,
                control,
            )?;
            require_receipt(
                &installation::inspect_main_controlled(&staging.join("next"), "addon", control)?,
                wanted,
                "prepared replacement changed",
            )?;
            require_receipt(
                &installation::inspect_main_controlled(target, "addon", control)?,
                &record.previous,
                "main addon changed during preparation",
            )?;
            checkpoint(Point::Prepared)?;
            move_checked(&staging, archive, control)?;
            checkpoint(Point::ArchivePublished)?;
            resume(target, archive, &record, control, checkpoint)?;
            Ok(true)
        })();
        return result.map_err(|mut error| {
            error.details=json!({"schema":"lycheedev.repair-replacement.v1","archive":archive,"retainedPreparation":staging,"cause":error.clone()});error
        });
    }
    if current["state"] == "managed" {
        let observed: Receipt = serde_json::from_value(current["receipt"].clone())?;
        if same_receipt(&observed, wanted)? {
            return Ok(false);
        }
    }
    if current["state"] != "absent" {
        return Err(conflict(
            "existing target requires explicit managed upgrade",
        ));
    }
    move_checked(prepared, target, control)?;
    Ok(true)
}
fn resume(
    target: &Path,
    archive: &Path,
    record: &Replacement,
    control: &OperationControl,
    checkpoint: &mut impl FnMut(Point) -> Result<()>,
) -> Result<()> {
    if record.schema != "lycheedev.replacement.v1"
        || record.target != target
        || record.previous.component != "addon"
        || record.next.component != "addon"
    {
        return Err(invalid("replacement target identity differs"));
    }
    let current = installation::inspect_main_controlled(target, "addon", control)?;
    let previous =
        installation::inspect_main_controlled(&archive.join("previous"), "addon", control)?;
    let next = installation::inspect_main_controlled(&archive.join("next"), "addon", control)?;
    if previous["state"] != "absent" {
        return Err(conflict("replacement previous destination appeared"));
    }
    require_receipt(&current, &record.previous, "replacement original changed")?;
    require_receipt(&next, &record.next, "replacement next changed")?;
    move_checked(target, &archive.join("previous"), control)?;
    checkpoint(Point::PreviousArchived)?;
    // A stop or publication fault after the first move retains both original
    // and next plus replacement.json. It does not undo or delete either tree.
    move_checked(&archive.join("next"), target, control)
}
pub(super) fn move_checked(source: &Path, target: &Path, control: &OperationControl) -> Result<()> {
    control.check()?;
    storage::guard_path(
        source
            .parent()
            .ok_or_else(|| invalid("source parent missing"))?,
        source,
    )?;
    storage::guard_path(
        target
            .parent()
            .ok_or_else(|| invalid("target parent missing"))?,
        target,
    )?;
    if present(target)? {
        return Err(conflict("publication target appeared"));
    }
    storage::durable_move_noreplace(source, target)?;
    control.check()
}
fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
