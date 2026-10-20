//! Controlled initialization for callers that retain an ordinary Metadata.
//! Compatibility open/read/write behavior stays in the parent module.
use super::{historical_write::finish_historical, *};
use crate::control::OperationControl;
use std::time::Duration;

impl Metadata {
    pub(crate) fn schema_version_controlled(&self, control: &OperationControl) -> Result<i32> {
        let result = checked_sql_with_check(
            control,
            control.deadline(),
            &mut || Ok(()),
            &mut || {},
            || {
                self.connection
                    .pragma_query_value(None, "user_version", |row| row.get(0))
            },
        );
        crate::selection::selection_finish(result, control.check())
    }

    /// Open the existing metadata API under the caller's original stop scope.
    /// Unlike `open`, SQLite lock waits never use an internal five-second sleep.
    /// This only controls opening; subsequent methods retain their own contracts.
    pub fn open_controlled(home: &Path, write: bool, control: &OperationControl) -> Result<Self> {
        Self::open_controlled_with_check(home, write, control, || Ok(()))
    }

    /// Check an independent caller budget within schema and SQLite contention.
    /// A stopped callback is returned only after all acquired owners are closed.
    pub fn open_controlled_with_check(
        home: &Path,
        write: bool,
        control: &OperationControl,
        check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        Self::open_controlled_observed_with_check(home, write, control, check, || {})
    }

    #[cfg(test)]
    fn open_controlled_observed(
        home: &Path,
        write: bool,
        control: &OperationControl,
        on_busy: impl FnMut(),
    ) -> Result<Self> {
        Self::open_controlled_observed_with_check(home, write, control, || Ok(()), on_busy)
    }

    pub(super) fn open_controlled_observed_with_check(
        home: &Path,
        write: bool,
        control: &OperationControl,
        check: impl FnMut() -> Result<()>,
        on_busy: impl FnMut(),
    ) -> Result<Self> {
        Self::open_controlled_with_events(home, write, control, check, on_busy, || {})
    }

    // Narrow lifecycle observations for deterministic fault injection. Ordinary
    // callers use empty observers; checks and ownership stay in this one path.
    pub(super) fn open_controlled_with_events(
        home: &Path,
        write: bool,
        control: &OperationControl,
        mut check: impl FnMut() -> Result<()>,
        mut on_busy: impl FnMut(),
        mut on_open: impl FnMut(),
    ) -> Result<Self> {
        // This must precede even workspace validation or directory inspection.
        metadata_check(control, &mut check)?;
        crate::workspace::open_controlled_with_check(home, control, &mut check)?;
        metadata_check(control, &mut check)?;
        let schema = if write {
            Some(historical_write::historical_lease_observed_with_check(
                home,
                "metadata-schema",
                control,
                &mut check,
                &mut on_busy,
            )?)
        } else {
            None
        };
        let opened = (|| {
            metadata_check(control, &mut check)?;
            let path = home.join("state/toolkit.sqlite");
            if write {
                std::fs::create_dir_all(path.parent().expect("metadata parent"))?;
                metadata_check(control, &mut check)?;
            }
            if !path.exists() && !write {
                if super::evidence_catalog::present(home)? {
                    let checks = std::cell::RefCell::new(&mut check);
                    return super::evidence_catalog::read_carrier(home, control, &|| {
                        (*checks.borrow_mut())()
                    });
                }
                return Err(ToolkitError::new(
                    "vault.record_missing",
                    "workspace metadata does not exist",
                    3,
                ));
            }
            let flags = OpenFlags::SQLITE_OPEN_NOFOLLOW
                | if write {
                    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
                } else {
                    OpenFlags::SQLITE_OPEN_READ_ONLY
                };
            // One absolute deadline spans open, schema reads, BEGIN and COMMIT.
            let until = control.deadline();
            let bound_home = std::path::absolute(home)?;
            let connection =
                checked_sql_with_check(control, until, &mut check, &mut on_busy, || {
                    Connection::open_with_flags(&path, flags)
                })?;
            on_open();
            let initialized =
                initialize_connection(&connection, write, control, &mut check, &mut on_busy);
            let metadata = Self {
                connection,
                home: bound_home,
                writable: write,
                evidence_gate_held: false,
                catalog_only: false,
            };
            match initialized {
                Ok(()) => Ok(metadata),
                Err(error) => finish_historical(Err(error), metadata.close()),
            }
        })();
        let released = schema.map_or(Ok(()), |lease| lease.close());
        // A schema-lock close failure cannot drop an already opened connection
        // without checking its close too. Preserve all typed cleanup causes.
        let metadata = match (opened, released) {
            (Ok(metadata), Ok(())) => metadata,
            (Ok(metadata), Err(error)) => {
                return finish_historical(Err(error), metadata.close());
            }
            (Err(error), cleanup) => return finish_historical(Err(error), cleanup),
        };
        match metadata_check(control, &mut check) {
            Ok(()) => Ok(metadata),
            Err(error) => finish_historical(Err(error), metadata.close()),
        }
    }

    /// Read one existing document under the original caller deadline. Keep
    /// ordinary read's decoding and missing-record diagnostics unchanged.
    pub fn read_controlled<T: DeserializeOwned>(
        &self,
        key: &str,
        control: &OperationControl,
    ) -> Result<T> {
        self.read_controlled_with_check(key, control, || Ok(()))
    }
    /// Preserve typed read diagnostics while checking an independent caller
    /// budget inside every row-read wait. This does not extend the deadline.
    pub fn read_controlled_with_check<T: DeserializeOwned>(
        &self,
        key: &str,
        control: &OperationControl,
        check: impl FnMut() -> Result<()>,
    ) -> Result<T> {
        self.read_controlled_observed_with_check(key, control, check, || {})
    }
    #[cfg(test)]
    fn read_controlled_observed<T: DeserializeOwned>(
        &self,
        key: &str,
        control: &OperationControl,
        on_busy: impl FnMut(),
    ) -> Result<T> {
        self.read_controlled_observed_with_check(key, control, || Ok(()), on_busy)
    }
    pub(crate) fn read_bytes_controlled_with_check(
        &self,
        key: &str,
        control: &OperationControl,
        check: impl FnMut() -> Result<()>,
    ) -> Result<Vec<u8>> {
        self.read_projected_controlled_with_check(key, control, check, || {}, Ok)
    }
    fn read_controlled_observed_with_check<T: DeserializeOwned>(
        &self,
        key: &str,
        control: &OperationControl,
        check: impl FnMut() -> Result<()>,
        on_busy: impl FnMut(),
    ) -> Result<T> {
        self.read_projected_controlled_with_check(key, control, check, on_busy, |bytes| {
            serde_json::from_slice(&bytes).map_err(Into::into)
        })
    }
    fn read_projected_controlled_with_check<T>(
        &self,
        key: &str,
        control: &OperationControl,
        mut check: impl FnMut() -> Result<()>,
        mut on_busy: impl FnMut(),
        decode: impl FnOnce(Vec<u8>) -> Result<T>,
    ) -> Result<T> {
        if (self.catalog_only || super::evidence_catalog::EvidenceKey::parse(key).is_some())
            && super::evidence_catalog::present(&self.home)?
        {
            let checks = std::cell::RefCell::new(&mut check);
            let row = super::evidence_catalog::required(super::evidence_catalog::read(
                &self.home,
                key,
                control,
                &|| (*checks.borrow_mut())(),
            )?)?;
            return decode(row.bytes);
        }
        metadata_check(control, &mut check)?;
        self.connection.busy_timeout(Duration::ZERO)?;
        let result = (|| {
            let bytes: Option<(i64, Option<Vec<u8>>)> = checked_sql_with_check(
                control,
                control.deadline(),
                &mut check,
                &mut on_busy,
                || {
                    self.connection
                        .query_row("SELECT length(CAST(value AS BLOB)),CASE WHEN length(CAST(value AS BLOB))<=1048576 THEN CAST(value AS BLOB) ELSE NULL END FROM documents WHERE key=?1", [key], |row| {
                            Ok((row.get(0)?,row.get(1)?))
                        })
                        .optional()
                },
            )?;
            metadata_check(control, &mut check)?;
            let value = match bytes {
                Some((length, Some(raw)))
                    if (0..=1048576).contains(&length) && raw.len() as i64 == length =>
                {
                    decode(raw)?
                }
                Some(_) => return Err(ToolkitError::invalid("metadata document exceeds 1 MiB")),
                None => {
                    return Err(ToolkitError::new(
                        "vault.record_missing",
                        format!("document {key} does not exist"),
                        3,
                    ));
                }
            };
            metadata_check(control, &mut check)?;
            Ok(value)
        })();
        let restore = self
            .connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(ToolkitError::from);
        finish_historical(result, restore)
    }

    /// Consume this handle and report SQLite's close result. No statement or
    /// transaction can outlive the owning Metadata through its public API.
    pub fn close(self) -> Result<()> {
        self.connection
            .close()
            .map_err(|(_connection, error)| error.into())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod consumer_tests;

// The request-owned capture route repeats the same live schema/authentication
// checks as opening a writer, but borrows the existing SQLite connection.
fn initialize_connection(
    connection: &Connection,
    write: bool,
    control: &OperationControl,
    check: &mut impl FnMut() -> Result<()>,
    on_busy: &mut impl FnMut(),
) -> Result<()> {
    let until = control.deadline();
    metadata_check(control, check)?;
    connection.busy_timeout(Duration::ZERO)?;
    let version: i32 = checked_sql_with_check(control, until, check, on_busy, || {
        connection.pragma_query_value(None, "user_version", |row| row.get(0))
    })?;
    metadata_check(control, check)?;
    // Deliberately retain open's existing schema/version diagnostics.
    // Historical Go migration has its separate, stricter boundary.
    if version > 1 {
        return Err(ToolkitError::new(
            "vault.schema_newer",
            "workspace schema is newer than this toolkit",
            4,
        ));
    }
    if version == 0 {
        if !write {
            return Err(ToolkitError::new(
                "vault.schema_invalid",
                "uninitialized workspace schema",
                4,
            ));
        }
        checked_sql_with_check(control, until, check, on_busy, || {
            connection.pragma_update(None, "synchronous", "FULL")
        })?;
        let mut transaction = checked_sql_with_check(control, until, check, on_busy, || {
            rusqlite::Transaction::new_unchecked(
                connection,
                rusqlite::TransactionBehavior::Immediate,
            )
        })?;
        let created = (|| {
            checked_sql_with_check(control, until, check, on_busy, || {
                transaction.execute_batch("CREATE TABLE IF NOT EXISTS documents (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID")
            })?;
            checked_sql_with_check(control, until, check, on_busy, || {
                transaction.pragma_update(None, "user_version", 1)
            })?;
            checked_sql_with_check(control, until, check, on_busy, || {
                transaction.execute_batch("COMMIT")
            })?;
            Ok(())
        })();
        // Roll back even after cancellation/deadline. Each attempt is
        // nonblocking; close below is checked after this guard ends.
        // SQLite can itself roll back some storage errors already.
        let rollback = if created.is_err() && !transaction.is_autocommit() {
            transaction
                .execute_batch("ROLLBACK")
                .map_err(ToolkitError::from)
        } else {
            Ok(())
        };
        // Do not silently retry a failed rollback through Drop.
        transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
        finish_historical(created, rollback)?;
    }
    if write {
        checked_sql_with_check(control, until, check, on_busy, || {
            connection.pragma_update(None, "synchronous", "FULL")
        })?;
        checked_sql_with_check(control, until, check, on_busy, || {
            connection.execute_batch(DELETED_GENERATIONS_SCHEMA)
        })?;
    }
    metadata_check(control, check)
}

impl Metadata {
    pub(crate) fn check_capture_binding(&self, home: &Path) -> Result<()> {
        if !self.writable
            || (home != self.home.as_path() && std::path::absolute(home)? != self.home)
        {
            return Err(ToolkitError::invalid(
                "capture metadata belongs to a different workspace or is read-only",
            ));
        }
        if !self.connection.is_autocommit() {
            return Err(ToolkitError::invalid(
                "capture metadata already has an active transaction",
            ));
        }
        Ok(())
    }
    pub(crate) fn check_capture_owner_controlled(
        &self,
        home: &Path,
        control: &OperationControl,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        metadata_check(control, &mut check)?;
        self.check_capture_binding(home)?;
        crate::workspace::open_controlled_with_check(home, control, &mut check)?;
        let schema = historical_write::historical_lease_observed_with_check(
            home,
            "metadata-schema",
            control,
            &mut check,
            &mut || {},
        )?;
        // Do not memoize user_version or dependency authentication. No writer
        // transaction is kept alive between source, codec, artifact and manifest.
        let checked =
            initialize_connection(&self.connection, true, control, &mut check, &mut || {});
        let checked = finish_historical(checked, schema.close());
        let checked = crate::selection::selection_finish(checked, control.check());
        crate::selection::selection_finish(checked, check())
    }
}
