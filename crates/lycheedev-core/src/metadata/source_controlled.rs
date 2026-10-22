//! Controlled single-document operations for Source cache and startup owners.
//! Business validation runs once; only SQL statements retry real contention.
use super::*;
use crate::{control::OperationControl, metadata::owned_write::finish_owned};
use std::time::Duration;

impl Metadata {
    pub fn generation_controlled(
        &self,
        key: &str,
        control: &OperationControl,
    ) -> Result<Option<i64>> {
        if (self.catalog_only || super::evidence_catalog::EvidenceKey::parse(key).is_some())
            && super::evidence_catalog::present(&self.home)?
        {
            return super::evidence_catalog::read(&self.home, key, control, &|| Ok(()))
                .map(|row| row.map(|row| row.generation));
        }
        control.check()?;
        self.connection.busy_timeout(Duration::ZERO)?;
        let result = checked_sql(control, control.deadline(), &mut || {}, || {
            self.connection
                .query_row(
                    "SELECT generation FROM documents WHERE key=?1",
                    [key],
                    |row| row.get(0),
                )
                .optional()
        });
        let result = finish_owned(
            result,
            self.connection
                .busy_timeout(Duration::from_secs(5))
                .map_err(Into::into),
        );
        crate::selection::selection_finish(result, control.check())
    }

    /// Consume exactly one authenticated startup intent. No unrelated documents
    /// are decoded; failed validation and cancellation leave the intent intact.
    /// An attempted COMMIT is never replayed together with the validator.
    pub fn consume_controlled<T: DeserializeOwned, U>(
        &mut self,
        key: &str,
        control: &OperationControl,
        validate: impl FnOnce(T) -> Result<U>,
    ) -> Result<U> {
        self.consume_controlled_observed(key, None, control, validate, &mut || {})
    }
    pub fn consume_generation_controlled<T: DeserializeOwned, U>(
        &mut self,
        key: &str,
        expected: i64,
        control: &OperationControl,
        validate: impl FnOnce(T) -> Result<U>,
    ) -> Result<U> {
        self.consume_controlled_observed(key, Some(expected), control, validate, &mut || {})
    }
    fn consume_controlled_observed<T: DeserializeOwned, U>(
        &mut self,
        key: &str,
        expected: Option<i64>,
        control: &OperationControl,
        validate: impl FnOnce(T) -> Result<U>,
        on_busy: &mut impl FnMut(),
    ) -> Result<U> {
        self.require_write_capability()?;
        if !self.evidence_gate_held && super::evidence_catalog::EvidenceKey::parse(key).is_some() {
            let home = self.home.clone();
            return super::evidence_catalog::mutate(&home, key, control, &|| Ok(()), |metadata| {
                metadata.consume_controlled_observed(key, expected, control, validate, on_busy)
            });
        }
        let evidence_gate = if self.evidence_gate_held {
            None
        } else {
            Some(crate::metadata::evidence_catalog::acquire_gate(
                &self.home,
                control,
                || Ok(()),
            )?)
        };
        let result = (|| {
            control.check()?;
            self.connection.busy_timeout(Duration::ZERO)?;
            let mut commit_attempted = false;
            let mut committed = false;
            let result = (|| {
                let mut transaction = checked_sql(control, control.deadline(), on_busy, || {
                    rusqlite::Transaction::new_unchecked(
                        &self.connection,
                        rusqlite::TransactionBehavior::Immediate,
                    )
                })?;
                let consumed = (|| {
                    let row: Option<(i64, i64, Option<Vec<u8>>)> = checked_sql(
                        control,
                        control.deadline(),
                        on_busy,
                        || {
                            transaction.query_row("SELECT generation,length(CAST(value AS BLOB)),CASE WHEN length(CAST(value AS BLOB))<=1048576 THEN CAST(value AS BLOB) ELSE NULL END FROM documents WHERE key=?1", [key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()
                        },
                    )?;
                    let raw = match row {
                        Some((generation, _, _))
                            if expected.is_some_and(|expected| expected != generation) =>
                        {
                            return Err(ToolkitError::new(
                                "vault.generation_conflict",
                                "document changed before checked deletion",
                                4,
                            ));
                        }
                        Some((_, length, Some(raw)))
                            if (0..=1_048_576).contains(&length) && raw.len() as i64 == length =>
                        {
                            raw
                        }
                        Some(_) => {
                            return Err(ToolkitError::invalid("metadata document exceeds 1 MiB"));
                        }
                        None => {
                            return Err(ToolkitError::new(
                                "vault.record_missing",
                                format!("document {key} does not exist"),
                                3,
                            ));
                        }
                    };
                    control.check()?;
                    let value = validate(serde_json::from_slice(&raw)?)?;
                    control.check()?;
                    checked_sql(control, control.deadline(), on_busy, || {
                        remember_deleted_generation(&transaction, key)
                    })?;
                    checked_sql(control, control.deadline(), on_busy, || {
                        transaction.execute("DELETE FROM documents WHERE key=?1", [key])
                    })?;
                    checked_sql(control, control.deadline(), on_busy, || {
                        commit_attempted = true;
                        transaction.execute_batch("COMMIT")
                    })?;
                    committed = true;
                    Ok(value)
                })();
                let rollback = if !transaction.is_autocommit() {
                    transaction
                        .execute_batch("ROLLBACK")
                        .map_err(ToolkitError::from)
                } else {
                    Ok(())
                };
                transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
                finish_owned(consumed, rollback)
            })();
            let result = finish_owned(
                result,
                self.connection
                    .busy_timeout(Duration::from_secs(5))
                    .map_err(Into::into),
            );
            crate::selection::selection_finish(result, control.check()).map_err(|mut error| {
            if commit_attempted {
                error.details = serde_json::json!({"schema":"lycheedev.metadata-mutation.v1","key":key,"mutation":"consume","state":if committed {"committed"} else {"unconfirmed"},"cause":error.clone()});
            }
            error
        })
        })();
        crate::metadata::owned_write::finish_owned(
            result,
            evidence_gate.map_or(Ok(()), crate::storage::ExclusiveLock::close),
        )
    }
}

#[cfg(test)]
mod tests;
