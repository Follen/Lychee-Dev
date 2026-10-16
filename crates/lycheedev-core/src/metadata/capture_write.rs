//! The already-metered immutable capture bytes retain their original identity.
//! This private consumer keeps dependency validation and publication in one
//! actual SQLite writer transaction, including every busy retry and cleanup.
use super::*;
use crate::control::OperationControl;
use std::cell::Cell;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CaptureWriteState {
    NotCommitted,
    Committed,
    Unknown,
}
impl Metadata {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn put_capture_serialized(
        &mut self,
        key: &str,
        raw: &[u8],
        references: &[String],
        control: &OperationControl,
        check: impl FnMut() -> Result<()>,
        admit_reference: impl FnMut(usize) -> Result<()>,
        validate_reference: impl FnMut(&str, &[u8]) -> Result<()>,
        publish: impl FnOnce() -> Result<()>,
        state: &Cell<CaptureWriteState>,
    ) -> Result<()> {
        self.put_capture_serialized_observed(
            key,
            raw,
            references,
            control,
            check,
            admit_reference,
            validate_reference,
            publish,
            state,
            &mut || {},
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn put_capture_serialized_observed(
        &mut self,
        key: &str,
        raw: &[u8],
        references: &[String],
        control: &OperationControl,
        mut check: impl FnMut() -> Result<()>,
        mut admit_reference: impl FnMut(usize) -> Result<()>,
        mut validate_reference: impl FnMut(&str, &[u8]) -> Result<()>,
        publish: impl FnOnce() -> Result<()>,
        state: &Cell<CaptureWriteState>,
        on_busy: &mut impl FnMut(),
    ) -> Result<()> {
        state.set(CaptureWriteState::NotCommitted);
        metadata_check(control, &mut check)?;
        if raw.len() > 1_048_576 {
            return Err(ToolkitError::invalid("metadata document exceeds 1 MiB"));
        }
        self.connection.busy_timeout(std::time::Duration::ZERO)?;
        let result = (|| {
            let until = control.deadline();
            let mut transaction =
                checked_sql_with_check(control, until, &mut check, on_busy, || {
                    rusqlite::Transaction::new_unchecked(
                        &self.connection,
                        rusqlite::TransactionBehavior::Immediate,
                    )
                })?;
            let written = (|| {
                for reference in references {
                    let length: Option<usize> =
                        checked_sql_with_check(control, until, &mut check, on_busy, || {
                            transaction
                                .query_row(
                                    "SELECT length(value) FROM documents WHERE key=?1",
                                    [reference],
                                    |row| row.get(0),
                                )
                                .optional()
                        })?;
                    let length = length.ok_or_else(|| {
                        ToolkitError::new(
                            "vault.record_missing",
                            format!("document {reference} does not exist"),
                            3,
                        )
                    })?;
                    if length > 1_048_576 {
                        return Err(ToolkitError::invalid("metadata document exceeds 1 MiB"));
                    }
                    admit_reference(length)?;
                    let document: Vec<u8> =
                        checked_sql_with_check(control, until, &mut check, on_busy, || {
                            transaction.query_row(
                                "SELECT value FROM documents WHERE key=?1",
                                [reference],
                                |row| row.get(0),
                            )
                        })?;
                    validate_reference(reference, &document)?;
                }
                metadata_check(control, &mut check)?;
                publish()?;
                let existing: Option<bool> =
                    checked_sql_with_check(control, until, &mut check, on_busy, || {
                        transaction
                            .query_row(
                                "SELECT value=?2 FROM documents WHERE key=?1",
                                params![key, raw],
                                |row| row.get(0),
                            )
                            .optional()
                    })?;
                match existing {
                    Some(true) => {
                        // A previous identical immutable manifest is already
                        // committed; rolling back this read transaction changes
                        // neither its identity nor that known publication fact.
                        state.set(CaptureWriteState::Committed);
                        return metadata_check(control, &mut check);
                    }
                    Some(false) => {
                        return Err(ToolkitError::new(
                            "vault.generation_conflict",
                            "immutable document already differs",
                            4,
                        ));
                    }
                    None => {
                        checked_sql_with_check(control, until, &mut check, on_busy, || {
                            transaction.execute(
                                "INSERT INTO documents(key,generation,value) VALUES(?1,1,?2)",
                                params![key, raw],
                            )
                        })?;
                    }
                }
                checked_sql_with_check(control, until, &mut check, on_busy, || {
                    state.set(CaptureWriteState::Unknown);
                    let committed = transaction.execute_batch("COMMIT");
                    match &committed {
                        Ok(()) => state.set(CaptureWriteState::Committed),
                        Err(rusqlite::Error::SqliteFailure(inner, _))
                            if matches!(
                                inner.code,
                                rusqlite::ErrorCode::DatabaseBusy
                                    | rusqlite::ErrorCode::DatabaseLocked
                            ) =>
                        {
                            state.set(CaptureWriteState::NotCommitted)
                        }
                        _ => {}
                    }
                    committed
                })?;
                // The successful COMMIT state is recorded before this can stop.
                metadata_check(control, &mut check)
            })();
            let active = !transaction.is_autocommit();
            let rollback = if active {
                transaction
                    .execute_batch("ROLLBACK")
                    .map_err(ToolkitError::from)
            } else {
                Ok(())
            };
            if active && rollback.is_ok() && state.get() == CaptureWriteState::Unknown {
                state.set(CaptureWriteState::NotCommitted);
            }
            transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
            historical_write::finish_historical(written, rollback)
        })();
        let restore = self
            .connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(ToolkitError::from);
        let result = historical_write::finish_historical(result, restore);
        let result = crate::selection::selection_finish(result, control.check());
        crate::selection::selection_finish(result, check())
    }
}
#[cfg(test)]
mod tests;
