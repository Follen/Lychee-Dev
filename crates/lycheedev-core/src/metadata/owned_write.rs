//! Fixed-Go metadata mutation boundary for historical Book retirement.
//! Every multi-document transition is one SQLite CAS transaction. The private
//! removal callback scans/authenticates within its fenced transaction.
use super::*;
pub(crate) mod removal;
use crate::{control::OperationControl, storage::ExclusiveLock};
use std::{
    collections::BTreeSet,
    time::Duration,
};

#[cfg(test)]
use std::time::Instant;
pub struct OwnedMutation<'a> {
    pub key: &'a str,
    pub expected_generation: i64,
    pub bytes: &'a [u8],
}
pub struct OwnedWriter<'a> {
    connection: &'a Connection,
    home: &'a Path,
    control: &'a OperationControl,
    write_lease_held: bool,
    catalog: Option<&'a super::evidence_catalog::Catalog>,
    admission: Option<&'a dyn Fn() -> Result<()>>,
}
/// Preserve the first typed cause and retain every later cleanup failure.
pub fn finish_owned<T>(result: Result<T>, cleanup: Result<()>) -> Result<T> {
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
pub fn owned_lease(
    home: &Path,
    key: &str,
    control: &OperationControl,
) -> Result<ExclusiveLock> {
    owned_lease_with_check(home, key, control, &mut || Ok(()))
}
// Kept narrow for controlled open: existing historical writers have no new
// budget contract, while supplemental checks reach resource creation and waits.
pub(super) fn owned_lease_with_check(
    home: &Path,
    key: &str,
    control: &OperationControl,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<ExclusiveLock> {
    owned_lease_observed_with_check(home, key, control, check, &mut || {})
}
pub(super) fn owned_lease_observed_with_check(
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
    pub fn with_owned_write<T>(
        home: &Path,
        control: &OperationControl,
        run: impl FnOnce(&OwnedWriter<'_>) -> Result<T>,
    ) -> Result<T> {
        control.check()?;
        crate::workspace::open_controlled(home, control)?;
        control.check()?;
        let schema = owned_lease(home, "metadata-schema", control)?;
        let opened = (|| {
            std::fs::create_dir_all(home.join("state"))?;
            control.check()?;
            let until = control.deadline();
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
                Err(error) => finish_owned(Err(error), close(connection)),
            }
        })();
        // Do not discard a successfully opened connection on a lease-close
        // error: explicitly close it too and preserve the original close cause.
        let connection = match (opened, schema.close()) {
            (Ok(connection), Ok(())) => connection,
            (Ok(connection), Err(error)) => {
                return finish_owned(Err(error), close(connection));
            }
            (Err(error), cleanup) => return finish_owned(Err(error), cleanup),
        };
        let result = run(&OwnedWriter {
            connection: &connection,
            home,
            control,
            write_lease_held: false,
            catalog: None,
            admission: None,
        });
        crate::selection::selection_finish(
            finish_owned(result, close(connection)),
            control.check(),
        )
    }
}
fn close(connection: Connection) -> Result<()> {
    connection
        .close()
        .map_err(|(_connection, error)| error.into())
}
impl OwnedWriter<'_> {
    /// Hold the fixed metadata-write namespace across inspect -> mutation.
    /// Nested commit/delete borrow this owner instead of acquiring it twice.
    pub(crate) fn with_write_lease<T>(
        &self,
        run: impl FnOnce(&OwnedWriter<'_>) -> Result<T>,
    ) -> Result<T> {
        self.control.check()?;
        if self.write_lease_held {
            return run(self);
        }
        let gate = if self.catalog.is_some() {
            None
        } else {
            Some(super::evidence_catalog::acquire_gate(
                self.home,
                self.control,
                || Ok(()),
            )?)
        };
        let result = (|| {
            let lease = owned_lease(self.home, "metadata-write", self.control)?;
            let writer = OwnedWriter {
                connection: self.connection,
                home: self.home,
                control: self.control,
                write_lease_held: true,
                catalog: self.catalog,
                admission: self.admission,
            };
            finish_owned(run(&writer), lease.close())
        })();
        crate::selection::selection_finish(
            finish_owned(result, gate.map_or(Ok(()), ExclusiveLock::close)),
            self.control.check(),
        )
    }

    pub fn document(&self, key: &str) -> Result<RawDocument> {
        if let Some(catalog) = self.catalog {
            return super::evidence_catalog::required(catalog.document(
                key,
                self.control,
                &|| Ok(()),
            )?);
        }
        if !self.write_lease_held
            && super::evidence_catalog::EvidenceKey::parse(key).is_some()
            && super::evidence_catalog::present(self.home)?
        {
            return super::evidence_catalog::required(super::evidence_catalog::read(
                self.home,
                key,
                self.control,
                &|| Ok(()),
            )?);
        }
        let row: Option<(i64, i64, Option<Vec<u8>>)> = checked_sql(
            self.control,
            self.control.deadline(),
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
    /// Keyset pages on this same checked metadata owner. Visitors are never
    /// replayed after the first row, and unrelated document kinds stay unread.
    pub(crate) fn visit_raw_page(
        &self,
        prefix: &str,
        after: &str,
        visit: impl FnMut(&str, i64, &[u8]) -> Result<()>,
    ) -> Result<(usize, String)> {
        if let Some(catalog) = self.catalog {
            return catalog.visit(prefix, after, 1000, self.control, &|| Ok(()), &mut {
                visit
            });
        }
        if !self.write_lease_held
            && super::evidence_catalog::intersects(prefix)
            && super::evidence_catalog::present(self.home)?
        {
            return super::evidence_catalog::visit(
                self.home,
                prefix,
                after,
                1000,
                self.control,
                &|| Ok(()),
                visit,
            );
        }
        super::raw_page::visit_raw_page(
            self.connection,
            prefix,
            after,
            1000,
            self.control,
            Duration::ZERO,
            || Ok(()),
            visit,
        )
    }
    /// Fixed-Go DeleteDocuments: one <=64-key CAS transaction and persistent
    /// tombstones. An unrelated document is never decoded or scanned.
    pub(crate) fn delete(&self, removals: &[(String, i64)]) -> Result<()> {
        if self.catalog.is_none()
            && removals
                .iter()
                .any(|(key, _)| super::evidence_catalog::EvidenceKey::parse(key).is_some())
        {
            if self.write_lease_held {
                return Err(ToolkitError::invalid(
                    "reserved evidence mutation requires a catalog scope before acquiring its write lease",
                ));
            }
            let keys: Vec<_> = removals.iter().map(|(key, _)| key.as_str()).collect();
            return super::evidence_catalog::with_batch_writer(
                self.home,
                &keys,
                self.control,
                |writer| writer.delete(removals),
            );
        }
        if !self.write_lease_held {
            return self.with_write_lease(|writer| writer.delete(removals));
        }
        self.control.check()?;
        if removals.is_empty() || removals.len() > 64 {
            return Err(ToolkitError::new(
                "command.failed",
                "vault: invalid deletion count",
                5,
            ));
        }
        let mut seen = BTreeSet::new();
        for (key, generation) in removals {
            self.control.check()?;
            if key.is_empty() || key.len() > 512 || !seen.insert(key) || *generation <= 0 {
                return Err(ToolkitError::new(
                    "command.failed",
                    "vault: invalid deletion",
                    5,
                ));
            }
        }
        if let Some(admit) = self.admission {
            admit()?;
        }
        let lease = if self.write_lease_held {
            None
        } else {
            Some(owned_lease(self.home, "metadata-write", self.control)?)
        };
        let result = (|| {
            let until = self.control.deadline();
            let mut transaction = checked_sql(self.control, until, &mut || {}, || {
                rusqlite::Transaction::new_unchecked(
                    self.connection,
                    rusqlite::TransactionBehavior::Immediate,
                )
            })?;
            let result = (|| {
                for (key, generation) in removals {
                    checked_sql(self.control, until, &mut || {}, || {
                        transaction.execute(
                        "INSERT INTO deleted_generations(key,generation) SELECT key,generation FROM documents WHERE key=?1 AND generation=?2 ON CONFLICT(key) DO UPDATE SET generation=excluded.generation",
                        params![key, generation])
                    })?;
                    let count = checked_sql(self.control, until, &mut || {}, || {
                        transaction.execute(
                            "DELETE FROM documents WHERE key=?1 AND generation=?2",
                            params![key, generation],
                        )
                    })?;
                    if count != 1 {
                        return Err(ToolkitError::new(
                            "vault.generation_conflict",
                            format!("vault.generation_conflict: {key}"),
                            3,
                        ));
                    }
                }
                checked_sql(self.control, until, &mut || {}, || {
                    transaction.execute_batch("COMMIT")
                })?;
                Ok(())
            })();
            finish_transaction(&mut transaction, result)
        })();
        let result = finish_owned(result, lease.map_or(Ok(()), ExclusiveLock::close));
        crate::selection::selection_finish(result, self.control.check())
    }
    pub fn commit(&self, changes: &[OwnedMutation<'_>]) -> Result<()> {
        self.commit_observed(changes, &mut || {})
    }
    fn commit_observed(
        &self,
        changes: &[OwnedMutation<'_>],
        on_busy: &mut impl FnMut(),
    ) -> Result<()> {
        if self.catalog.is_none()
            && changes
                .iter()
                .any(|change| super::evidence_catalog::EvidenceKey::parse(change.key).is_some())
        {
            if self.write_lease_held {
                return Err(ToolkitError::invalid(
                    "reserved evidence mutation requires a catalog scope before acquiring its write lease",
                ));
            }
            let keys: Vec<_> = changes.iter().map(|change| change.key).collect();
            return super::evidence_catalog::with_batch_writer(
                self.home,
                &keys,
                self.control,
                |writer| writer.commit_observed(changes, on_busy),
            );
        }
        if !self.write_lease_held {
            return self.with_write_lease(|writer| writer.commit_observed(changes, on_busy));
        }
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
        if let Some(admit) = self.admission {
            admit()?;
        }
        let lease = if self.write_lease_held {
            None
        } else {
            Some(owned_lease(self.home, "metadata-write", self.control)?)
        };
        let result = (|| {
            let until = self.control.deadline();
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
        crate::selection::selection_finish(
            finish_owned(result, lease.map_or(Ok(()), ExclusiveLock::close)),
            self.control.check(),
        )
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
    let rollback = if transaction.is_autocommit() {
        Ok(())
    } else {
        transaction
            .execute_batch("ROLLBACK")
            .map_err(ToolkitError::from)
    };
    transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
    finish_owned(result, rollback)
}

#[cfg(test)]
mod target_lifecycle_tests {
    use super::*;
    #[test]
    fn shared_raw_page_restores_zero_sql_timeout_before_checked_writer_retry() {
        let path = std::env::temp_dir().join(format!(
            "lychee-target-writer-timeout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        crate::workspace::initialize(&path).unwrap();
        let token = crate::control::TrackedCancellation::new();
        let control =
            OperationControl::tracked(Instant::now() + Duration::from_secs(60), token.clone());
        let result = Metadata::with_owned_write(&path, &control, |writer| {
            writer.commit(&[OwnedMutation {
                key: "target/a",
                expected_generation: 0,
                bytes: b"null",
            }])?;
            assert_eq!(
                writer
                    .visit_raw_page("target/", "target/", |_, _, _| Ok(()))?
                    .0,
                1
            );
            let timeout: i64 = writer
                .connection
                .pragma_query_value(None, "busy_timeout", |r| r.get(0))?;
            assert_eq!(timeout, 0);
            let blocker = Connection::open(path.join("state/toolkit.sqlite"))?;
            blocker.execute_batch("BEGIN IMMEDIATE")?;
            let mut saw_busy = false;
            let result = writer.commit_observed(
                &[OwnedMutation {
                    key: "target/b",
                    expected_generation: 0,
                    bytes: b"null",
                }],
                &mut || {
                    saw_busy = true;
                    token.cancel();
                },
            );
            assert!(saw_busy);
            assert_eq!(result.unwrap_err().code, "command.cancelled");
            blocker.execute_batch("ROLLBACK")?;
            blocker.close().map_err(|(_, e)| ToolkitError::from(e))?;
            Ok(())
        });
        assert_eq!(result.unwrap_err().code, "command.cancelled");
        assert!(control.stop_cause().is_ok());
        std::fs::remove_dir_all(path).unwrap();
    }
}

impl<'a> OwnedWriter<'a> {
    pub(super) fn for_catalog(
        connection: &'a Connection,
        home: &'a Path,
        control: &'a OperationControl,
        catalog: &'a super::evidence_catalog::Catalog,
        admission: Option<&'a dyn Fn() -> Result<()>>,
    ) -> Self {
        Self {
            connection,
            home,
            control,
            write_lease_held: false,
            catalog: Some(catalog),
            admission,
        }
    }
}
