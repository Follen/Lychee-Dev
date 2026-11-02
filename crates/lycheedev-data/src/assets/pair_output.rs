//! Manifest-last publication shared by current file exports.
use crate::request::DataContext;
use crate::{assets::output, fault, resource::Budget};
use lycheedev_core::{
    Result,
    storage::{Destination, ExclusiveLock},
};
const MAX_BYTES: usize = 32 << 20;
pub(crate) fn publish_pair_after_data(
    ctx: &DataContext,
    data: &Destination,
    sidecar: &Destination,
    _overwrite: bool,
    raw: &[u8],
    manifest: &[u8],
    budget: &Budget,
    after_data: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let parent = data
        .path()
        .parent()
        .ok_or_else(|| fault("records.export_path", 2, "Missing output parent"))?;
    let name = data
        .path()
        .file_name()
        .ok_or_else(|| fault("records.export_path", 2, "Missing output name"))?;
    let resource = format!("data-csv:{}", name.to_string_lossy().to_lowercase());
    let scope = parent.join(".lycheedev-locks");
    let lease = ExclusiveLock::resource_wait_controlled_with_check(
        &scope,
        &resource,
        &ctx.control,
        || budget.check(),
    )?;
    let mut data_state = lycheedev_core::storage::StagedPublication::NotPublished;
    let mut manifest_state = lycheedev_core::storage::StagedPublication::NotPublished;
    let result = (|| {
        ctx.control.check()?;
        budget.check()?;
        // Recheck the original parents under the lease. Do not acquire new
        // unchecked owners or silently follow replaced directory paths.
        output::check_destination_scoped(ctx, data, budget)?;
        output::check_destination_scoped(ctx, sidecar, budget)?;
        // The two native copies are sequential and share one 64 KiB stack
        // buffer allowance. Private safety storage is refunded after both.
        let _buffer = budget.reserve_retained(65536)?;
        output::publish_reader_scoped(
            ctx,
            data,
            &mut std::io::Cursor::new(raw),
            raw.len() as u64,
            raw.len().max(MAX_BYTES),
            Some(budget),
            &mut |state| data_state = state,
        )?;
        after_data()?;
        ctx.control.check()?;
        budget.check()?;
        output::publish_reader_scoped(
            ctx,
            sidecar,
            &mut std::io::Cursor::new(manifest),
            manifest.len() as u64,
            manifest.len().max(MAX_BYTES),
            Some(budget),
            &mut |state| manifest_state = state,
        )
    })();
    let result = lycheedev_core::metadata::owned_write::finish_owned(result, lease.close());
    let result = finish_pair_check(result, ctx.control.check());
    let result = finish_pair_check(result, budget.check());
    result.map_err(|error| {
        output::publication_context(
            output::publication_context(error, data.path(), data_state),
            sidecar.path(),
            manifest_state,
        )
    })
}
fn finish_pair_check<T>(result: Result<T>, stop: Result<()>) -> Result<T> {
    if let (Err(primary), Err(stop)) = (&result, &stop) {
        if primary.code == stop.code
            && primary.exit_code == stop.exit_code
            && primary.message == stop.message
            && primary.details == stop.details
        {
            return result;
        }
    }
    lycheedev_core::metadata::owned_write::finish_owned(result, stop)
}
