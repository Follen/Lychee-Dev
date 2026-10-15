//! Fixed-Go metadata mutation boundary for historical Book retirement.
//! Every multi-document transition is one SQLite CAS transaction. No business
//! callback runs inside that transaction; no new native execution is exposed.
use super::*;
use crate::{control::OperationControl, storage::ExclusiveLock};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

pub struct HistoricalMutation<'a> {
    pub key: &'a str,
    pub expected_generation: i64,
    pub bytes: &'a [u8],
}
pub struct HistoricalWriter<'a> {
    connection: &'a Connection,
    home: &'a Path,
    control: &'a OperationControl,
}
/// Preserve the first typed cause and retain every later cleanup failure.
pub fn finish_historical<T>(result: Result<T>, cleanup: Result<()>) -> Result<T> {
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(mut original), Err(close)) => {
            let primary = original.clone();
            original.message = format!("{}\n{}", original.message, close.message);
            original.details = serde_json::json!({"schema":crate::storage::FILE_CLEANUP_SCHEMA,"primary":primary,"cleanup":close,"control":null});
            Err(original)
        }
    }
}
/// The same hashed lock namespace as fixed Go AcquireLease. Retained lock files
/// are never removed. Only an actual lock-contention error is retried.
pub fn historical_lease(
    home: &Path,
    key: &str,
    control: &OperationControl,
) -> Result<ExclusiveLock> {
    historical_lease_with_check(home, key, control, &mut || Ok(()))
}
// Kept narrow for controlled open: existing historical writers have no new
// budget contract, while supplemental checks reach resource creation and waits.
pub(super) fn historical_lease_with_check(
    home: &Path,
    key: &str,
    control: &OperationControl,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<ExclusiveLock> {
    historical_lease_observed_with_check(home, key, control, check, &mut || {})
}
pub(super) fn historical_lease_observed_with_check(
    home: &Path,
    key: &str,
    control: &OperationControl,
    check: &mut impl FnMut() -> Result<()>,
    on_busy: &mut impl FnMut(),
) -> Result<ExclusiveLock> {
    loop {
        metadata_check(control, check)?;
        match ExclusiveLock::resource_checked(&home.join("locks"), key, || {
            metadata_check(control, check)
        }) {
            Err(error) if error.code == "workspace.busy" && error.details.is_null() => {
                on_busy();
                metadata_check(control, check)?;
                std::thread::sleep(Duration::from_millis(25).min(control.remaining()?));
            }
            result => return result,
        }
    }
}

impl Metadata {
    pub fn with_historical_write<T>(
        home: &Path,
        control: &OperationControl,
        run: impl FnOnce(&HistoricalWriter<'_>) -> Result<T>,
    ) -> Result<T> {
        control.check()?;
        crate::workspace::open(home)?;
        control.check()?;
        let schema = historical_lease(home, "metadata-schema", control)?;
        let opened = (|| {
            std::fs::create_dir_all(home.join("state"))?;
            control.check()?;
            let until = Instant::now() + Duration::from_secs(5);
            let connection = checked_sql(control, until, &mut || {}, || {
                Connection::open_with_flags(
                    home.join("state/toolkit.sqlite"),
                    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
                )
            })?;
            let setup = (|| {
                connection.busy_timeout(Duration::ZERO)?;
                let version: i32 = checked_sql(control, until, &mut || {}, || {
                    connection.pragma_query_value(None, "user_version", |r| r.get(0))
                })?;
                if !matches!(version, 0 | 1) {
                    return Err(ToolkitError::new(
                        "vault.unsupported_workspace",
                        "vault.unsupported_workspace",
                        4,
                    ));
                }
                if version == 0 {
                    let count: i64 = checked_sql(control, until, &mut || {}, || {
                        connection.query_row(
                            "SELECT count(*) FROM sqlite_master WHERE type='table'",
                            [],
                            |r| r.get(0),
                        )
                    })?;
                    if count != 0 {
                        return Err(ToolkitError::new(
                            "vault.unsupported_workspace",
                            "vault.unsupported_workspace",
                            4,
                        ));
                    }
                    let mut transaction = checked_sql(control, until, &mut || {}, || {
                        rusqlite::Transaction::new_unchecked(
                            &connection,
                            rusqlite::TransactionBehavior::Immediate,
                        )
                    })?;
                    let created = (|| {
                        checked_sql(control, until, &mut || {}, || {
                            transaction.execute_batch("CREATE TABLE documents (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID; PRAGMA user_version=1;")
                        })?;
                        checked_sql(control, until, &mut || {}, || {
                            transaction.execute_batch("COMMIT")
                        })?;
                        Ok(())
                    })();
                    finish_transaction(&mut transaction, created)?;
                }
                checked_sql(control, until, &mut || {}, || {
                    connection.execute_batch("PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS deleted_generations (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL) WITHOUT ROWID;")
                })?;
                checked_sql(control, until, &mut || {}, || {
                    connection.pragma_update(None, "journal_mode", "WAL")
                })?;
                control.check()
            })();
            match setup {
                Ok(()) => Ok(connection),
                Err(error) => finish_historical(Err(error), close(connection)),
            }
        })();
        // Do not discard a successfully opened connection on a lease-close
        // error: explicitly close it too and preserve the original close cause.
        let connection = match (opened, schema.close()) {
            (Ok(connection), Ok(())) => connection,
            (Ok(connection), Err(error)) => {
                return finish_historical(Err(error), close(connection));
            }
            (Err(error), cleanup) => return finish_historical(Err(error), cleanup),
        };
        let result = run(&HistoricalWriter {
            connection: &connection,
            home,
            control,
        });
        finish_historical(result, close(connection)).and_then(|value| {
            control.check()?;
            Ok(value)
        })
    }
}
fn close(connection: Connection) -> Result<()> {
    connection
        .close()
        .map_err(|(_connection, error)| error.into())
}
impl HistoricalWriter<'_> {
    pub fn document(&self, key: &str) -> Result<RawDocument> {
        let row: Option<(i64, i64, Option<Vec<u8>>)> = checked_sql(
            self.control,
            Instant::now() + Duration::from_secs(5),
            &mut || {},
            || {
                self.connection.query_row("SELECT generation,length(CAST(value AS BLOB)),CASE WHEN length(CAST(value AS BLOB))<=1048576 THEN CAST(value AS BLOB) ELSE NULL END FROM documents WHERE key=?1", [key], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()
            },
        )?;
        self.control.check()?;
        match row {
            Some((generation, length, Some(bytes)))
                if (0..=1_048_576).contains(&length) && length == bytes.len() as i64 =>
            {
                Ok(RawDocument { generation, bytes })
            }
            None => Err(ToolkitError::new(
                "vault.record_missing",
                "vault.record_missing",
                2,
            )),
            _ => Err(ToolkitError::invalid("metadata document exceeds 1 MiB")),
        }
    }
    pub fn commit(&self, changes: &[HistoricalMutation<'_>]) -> Result<()> {
        self.commit_observed(changes, &mut || {})
    }
    fn commit_observed(
        &self,
        changes: &[HistoricalMutation<'_>],
        on_busy: &mut impl FnMut(),
    ) -> Result<()> {
        self.control.check()?;
        if changes.is_empty() || changes.len() > 64 {
            return Err(ToolkitError::new(
                "command.failed",
                "vault: invalid mutation count",
                5,
            ));
        }
        let mut seen = BTreeSet::new();
        for change in changes {
            self.control.check()?;
            if change.key.is_empty()
                || change.key.len() > 512
                || !seen.insert(change.key)
                || change.expected_generation < 0
                || change.expected_generation == i64::MAX
                || change.bytes.len() > 1_048_576
                || !crate::raw_json::is_valid(change.bytes)
            {
                return Err(ToolkitError::new(
                    "command.failed",
                    "vault: invalid mutation",
                    5,
                ));
            }
        }
        let lease = historical_lease(self.home, "metadata-write", self.control)?;
        let result = (|| {
            let until = Instant::now() + Duration::from_secs(5);
            let mut transaction = checked_sql(self.control, until, on_busy, || {
                rusqlite::Transaction::new_unchecked(
                    self.connection,
                    rusqlite::TransactionBehavior::Immediate,
                )
            })?;
            let result = (|| {
                for change in changes {
                    let count = checked_sql(self.control, until, on_busy, || {
                        if change.expected_generation == 0 {
                            transaction.execute("INSERT INTO documents(key,generation,value) VALUES(?1,COALESCE((SELECT generation+1 FROM deleted_generations WHERE key=?1),1),?2) ON CONFLICT(key) DO NOTHING", params![change.key,change.bytes])
                        } else {
                            transaction.execute("UPDATE documents SET generation=generation+1,value=?2 WHERE key=?1 AND generation=?3", params![change.key,change.bytes,change.expected_generation])
                        }
                    })?;
                    if count != 1 {
                        return Err(ToolkitError::new(
                            "vault.generation_conflict",
                            format!("vault.generation_conflict: {}", change.key),
                            3,
                        ));
                    }
                }
                checked_sql(self.control, until, on_busy, || {
                    transaction.execute_batch("COMMIT")
                })?;
                Ok(())
            })();
            finish_transaction(&mut transaction, result)
        })();
        finish_historical(result, lease.close())
    }
}
#[cfg(test)]
mod tests;

fn finish_transaction(
    transaction: &mut rusqlite::Transaction<'_>,
    result: Result<()>,
) -> Result<()> {
    if result.is_ok() {
        transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
        return result;
    }
    // Cleanup must run even after the caller stopped. One nonblocking rollback;
    // retain its failure with the original cause, never claim successful CAS.
    let rollback = transaction
        .execute_batch("ROLLBACK")
        .map_err(ToolkitError::from);
    if rollback.is_ok() {
        transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
    }
    finish_historical(result, rollback)
}
