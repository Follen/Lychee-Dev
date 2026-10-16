//! Removal alone needs scans, authentication and mutation under one SQLite
//! snapshot: capture publishers use SQLite transactions, not this write lease.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemovalCommit {
    NotCommitted,
    Committed,
    Unknown,
}
pub(crate) struct RemovalTransaction<'a> {
    writer: HistoricalWriter<'a>,
}
impl HistoricalWriter<'_> {
    pub(crate) fn removal_transaction<T>(
        &self,
        state: &mut RemovalCommit,
        run: impl FnOnce(&RemovalTransaction<'_>) -> Result<T>,
    ) -> Result<T> {
        self.removal_transaction_observed(state, run, &mut || {})
    }
    fn removal_transaction_observed<T>(
        &self,
        state: &mut RemovalCommit,
        run: impl FnOnce(&RemovalTransaction<'_>) -> Result<T>,
        on_busy: &mut impl FnMut(),
    ) -> Result<T> {
        self.control.check()?;
        assert!(
            self.write_lease_held,
            "removal requires its metadata write lease"
        );
        *state = RemovalCommit::NotCommitted;
        let mut transaction = checked_sql(self.control, self.control.deadline(), on_busy, || {
            rusqlite::Transaction::new_unchecked(
                self.connection,
                rusqlite::TransactionBehavior::Immediate,
            )
        })?;
        let result = run(&RemovalTransaction {
            writer: HistoricalWriter {
                connection: &transaction,
                home: self.home,
                control: self.control,
                write_lease_held: true,
            },
        })
        .and_then(|value| {
            checked_sql(self.control, self.control.deadline(), on_busy, || {
                // Only SQLite busy/locked retries. Never replay the consumer or
                // a file disposition after an uncertain COMMIT.
                *state = RemovalCommit::Unknown;
                let result = transaction.execute_batch("COMMIT");
                if result.is_ok() {
                    *state = RemovalCommit::Committed;
                }
                result
            })?;
            Ok(value)
        });
        let rollback = if transaction.is_autocommit() {
            Ok(())
        } else {
            let result = transaction
                .execute_batch("ROLLBACK")
                .map_err(ToolkitError::from);
            if result.is_ok() {
                *state = RemovalCommit::NotCommitted;
            }
            result
        };
        transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
        crate::selection::selection_finish(
            finish_historical(result, rollback),
            self.control.check(),
        )
    }
}
impl RemovalTransaction<'_> {
    pub(crate) fn document(&self, key: &str) -> Result<RawDocument> {
        self.writer.document(key)
    }
    pub(crate) fn visit_raw_page(
        &self,
        prefix: &str,
        after: &str,
        visit: impl FnMut(&str, i64, &[u8]) -> Result<()>,
    ) -> Result<(usize, String)> {
        self.writer.visit_raw_page(prefix, after, visit)
    }
    pub(crate) fn put_recovery(&self, key: &str, expected: i64, bytes: &[u8]) -> Result<()> {
        self.writer.control.check()?;
        if !key.starts_with("evidence/remove/")
            || key.len() > 512
            || expected < 0
            || expected == i64::MAX
            || bytes.len() > 1_048_576
            || !crate::raw_json::is_valid(bytes)
        {
            return Err(ToolkitError::invalid("invalid removal recovery mutation"));
        }
        let count = checked_sql(
            self.writer.control,
            self.writer.control.deadline(),
            &mut || {},
            || {
                if expected == 0 {
                    self.writer.connection.execute("INSERT INTO documents(key,generation,value) VALUES(?1,COALESCE((SELECT generation+1 FROM deleted_generations WHERE key=?1),1),?2) ON CONFLICT(key) DO NOTHING", params![key,bytes])
                } else {
                    self.writer.connection.execute("UPDATE documents SET generation=generation+1,value=?2 WHERE key=?1 AND generation=?3",params![key,bytes,expected])
                }
            },
        )?;
        if count != 1 {
            return Err(conflict(key));
        }
        self.writer.control.check()
    }
    pub(crate) fn delete_manifest(&self, key: &str, expected: i64) -> Result<()> {
        self.writer.control.check()?;
        if !(key.starts_with("capture/") || key.starts_with("evidence/keep/")) || expected <= 0 {
            return Err(ToolkitError::invalid("invalid capture removal"));
        }
        checked_sql(
            self.writer.control,
            self.writer.control.deadline(),
            &mut || {},
            || {
                self.writer.connection.execute("INSERT INTO deleted_generations(key,generation) SELECT key,generation FROM documents WHERE key=?1 AND generation=?2 ON CONFLICT(key) DO UPDATE SET generation=excluded.generation",params![key,expected])
            },
        )?;
        let count = checked_sql(
            self.writer.control,
            self.writer.control.deadline(),
            &mut || {},
            || {
                self.writer.connection.execute(
                    "DELETE FROM documents WHERE key=?1 AND generation=?2",
                    params![key, expected],
                )
            },
        )?;
        if count != 1 {
            return Err(conflict(key));
        }
        self.writer.control.check()
    }
}
fn conflict(key: &str) -> ToolkitError {
    ToolkitError::new(
        "vault.generation_conflict",
        format!("vault.generation_conflict: {key}"),
        3,
    )
}

#[cfg(test)]
mod tests;
