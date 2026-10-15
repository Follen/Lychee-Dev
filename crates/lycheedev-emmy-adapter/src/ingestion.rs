//! Bounded, cancellable owned-text conversion for the unmodified public batch API.
use emmylua_code_analysis::{BatchChange, EmmyLuaAnalysis, FileId, file_path_to_uri};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug)]
pub enum IngestionFailure<E> {
    Interrupted(E),
    InvalidInput(&'static str),
    Allocation(std::collections::TryReserveError),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IngestionTextCost {
    /// Bytes copied into Strings, then retained separately by upstream's Arc<str>.
    pub retained_bytes: usize,
    /// Largest single String -> Arc<str> conversion allocation overlap.
    pub largest_file_bytes: usize,
}

#[derive(Debug)]
pub struct IngestedBatch {
    pub file_ids: Vec<FileId>,
    /// Lookup order for this completed full rebuild, shared by immutable owned
    /// query scopes. This is not valid after an incremental database mutation.
    pub contribution_order: Arc<[FileId]>,
    pub text_cost: IngestionTextCost,
}

/// Preserve the original input order, invalid-path omission, deletion semantics,
/// and exactly one full cache rebuild. `retained_limit` is the already admitted
/// source-byte ceiling, not a fresh or expanded allowance. The input Arcs remain
/// owned by their callers; the returned cost is additional to that retained text.
///
/// Conversion checks control before allocation and for every <=16 KiB UTF-8
/// chunk. The public parse/index call is synchronous with no per-file callback;
/// callers must retain the existing owned-worker hard deadline and discard this
/// entire analysis on any returned error. No partial model is a valid result.
pub fn ingest_public_batch<E>(
    analysis: &mut EmmyLuaAnalysis,
    files: Vec<(PathBuf, Option<Arc<str>>)>,
    retained_limit: usize,
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<IngestedBatch, IngestionFailure<E>> {
    check().map_err(IngestionFailure::Interrupted)?;
    let mut cost = IngestionTextCost::default();
    // Complete byte admission before allocating any copied source text.
    for (path, text) in &files {
        check().map_err(IngestionFailure::Interrupted)?;
        if file_path_to_uri(path).is_none() {
            continue;
        }
        if let Some(text) = text {
            cost.retained_bytes = cost
                .retained_bytes
                .checked_add(text.len())
                .filter(|bytes| *bytes <= retained_limit)
                .ok_or(IngestionFailure::InvalidInput(
                    "public engine text exceeds admitted byte limit",
                ))?;
            cost.largest_file_bytes = cost.largest_file_bytes.max(text.len());
        }
    }
    let mut mapped = Vec::new();
    mapped
        .try_reserve_exact(files.len())
        .map_err(IngestionFailure::Allocation)?;
    for (path, text) in files {
        check().map_err(IngestionFailure::Interrupted)?;
        let Some(uri) = file_path_to_uri(&path) else {
            continue;
        };
        let copied = match text {
            None => None,
            Some(text) => {
                check().map_err(IngestionFailure::Interrupted)?;
                let mut copied = String::new();
                copied
                    .try_reserve_exact(text.len())
                    .map_err(IngestionFailure::Allocation)?;
                let mut start = 0;
                while start < text.len() {
                    check().map_err(IngestionFailure::Interrupted)?;
                    let mut end = start.saturating_add(16 * 1024).min(text.len());
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    copied.push_str(&text[start..end]);
                    start = end;
                }
                check().map_err(IngestionFailure::Interrupted)?;
                Some(copied)
            }
        };
        mapped.push((uri, copied));
    }
    check().map_err(IngestionFailure::Interrupted)?;
    let (_, file_ids) = analysis
        .db
        .apply_batch_with_ids(BatchChange { files: mapped });
    check().map_err(IngestionFailure::Interrupted)?;
    // Fixed upstream rebuilds each workspace index in VFS FileId order.
    // Public configuration registers libraries in increasing IDs, and only
    // clears them as a whole; its stable lookup order is main/libraries/std/
    // remote. Preserve the VFS order within each individual workspace.
    let mut contribution_order = analysis.db.file_ids();
    check().map_err(IngestionFailure::Interrupted)?;
    contribution_order.sort_by_key(|file| {
        let workspace = analysis.db.workspace_id_of(*file);
        match workspace {
            Some(id) if id.is_main() => (0, 0),
            Some(id) if id.is_library() => (1, id.id),
            Some(id) if id.is_std() => (2, 0),
            _ => (3, 0),
        }
    });
    check().map_err(IngestionFailure::Interrupted)?;
    Ok(IngestedBatch {
        file_ids,
        contribution_order: Arc::from(contribution_order),
        text_cost: cost,
    })
}
