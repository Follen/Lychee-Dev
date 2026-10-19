//! Rebuildable Data metadata, purpose-bound to one independent SQLite database.
//! Normal DELETE-journal crash recovery belongs to SQLite. Quota measures logical
//! file lengths, not physical allocation or malicious-journal transient growth.
//! No caller can obtain a Connection, Metadata, transaction or arbitrary SQL.
use super::*;
use crate::{
    control::OperationControl,
    workspace::data_disk::{MetadataQuota, MetadataReadQuota},
};
use historical_write::finish_historical;
use std::time::Duration;

const PAGE_BYTES: u64 = 65_536;
const SECTOR_MAX: u64 = 65_536;
const APPLICATION_ID: i32 = 0x4c444331;
const DOCUMENT_LIMIT: usize = 1_048_576;
const SCHEMA: &str = "CREATE TABLE documents (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID";

/// A purpose, not a transferable connection. Capture APIs accept only their
/// original Core Metadata; this namespace cannot be rebound to it.
pub struct DataCacheMetadata;

fn invalid() -> ToolkitError {
    ToolkitError::new(
        "vault.data_metadata_integrity",
        "Private Data metadata identity or schema is invalid; existing files retained",
        4,
    )
}
fn missing() -> ToolkitError {
    ToolkitError::new("vault.record_missing", "vault.record_missing", 2)
}
fn conflict() -> ToolkitError {
    ToolkitError::new(
        "vault.generation_conflict",
        "document changed or already exists",
        4,
    )
}
fn validate_key(key: &str) -> Result<()> {
    if key.len() > 4096
        || ![
            "remote-catalog/",
            "remote-config/",
            "cdn-route/",
            "cdn-range/",
            "cdn-location/",
            "decoded-v1/",
            "definition-ref/",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix) && key.len() > prefix.len())
    {
        return Err(invalid());
    }
    Ok(())
}
fn sql<T>(
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
    run: impl FnMut() -> rusqlite::Result<T>,
) -> Result<T> {
    checked_sql_with_check(
        control,
        control.deadline(),
        &mut || check(),
        &mut || {},
        run,
    )
}
fn check(control: &OperationControl, extra: &dyn Fn() -> Result<()>) -> Result<()> {
    control.check()?;
    extra()
}
fn close(connection: Connection) -> Result<()> {
    // No retry of an unknown SQLite close. As in the existing Core owner, the
    // returned connection is dropped only after its typed close error is saved.
    let result = connection
        .close()
        .map_err(|(_connection, error)| error.into());
    #[cfg(test)]
    let result = finish_historical(result, tests::observe(tests::Point::Closed));
    result
}

/// Conservative rollback-journal bound with normal cache spilling enabled.
/// Bundled SQLite 3.50.2 pager_write journals each original page at most once.
/// A stress sync clears NEED_SYNC and enters DBMOD; another header requires a
/// newly journalled page, except the first CACHEMOD spill (including bootstrap).
/// Each header plus preceding sector padding uses <2*65536 bytes. Two extra
/// headers cover initial + first new-page spill, plus the sync sentinel byte.
/// No attached databases, savepoints, triggers, deletes or VACUUM are exposed.
fn journal_bound(main: u64) -> Result<u64> {
    let overflow = || {
        ToolkitError::new(
            "vault.data_cache_size_overflow",
            "Data metadata size overflow",
            4,
        )
    };
    let pages = main.checked_add(PAGE_BYTES - 1).ok_or_else(overflow)? / PAGE_BYTES;
    let journal_max = pages
        .checked_mul(PAGE_BYTES + 8)
        .and_then(|records| {
            pages
                .checked_add(2)?
                .checked_mul(2 * SECTOR_MAX)?
                .checked_add(records)
        })
        .and_then(|n| n.checked_add(8))
        .ok_or_else(overflow)?;
    Ok(journal_max)
}

impl DataCacheMetadata {
    pub fn read_document_raw_controlled_with_check(
        home: &Path,
        key: &str,
        control: &OperationControl,
        supplemental: impl Fn() -> Result<()>,
    ) -> Result<RawDocument> {
        check(control, &supplemental)?;
        validate_key(key)?;
        let owner = MetadataReadQuota::acquire(home, control, &supplemental)?;
        let result = (|| {
            reject_foreign_sidecars(|name| owner.length(name))?;
            if owner.length("cache-v1.sqlite")?.is_none() {
                // A normal private bootstrap always creates main before journal.
                // An orphan journal is unknown state, never a healthy cache miss.
                return Err(if owner.length("cache-v1.sqlite-journal")?.is_some() {
                    invalid()
                } else {
                    missing()
                });
            }
            let connection = open(owner.path(), false, control, &supplemental)?;
            let read = (|| {
                let initialized = validate_connection(&connection, control, &supplemental)?;
                owner.verify_recovery()?;
                if !initialized {
                    return Err(missing());
                }
                read_row(&connection, key, control, &supplemental)
            })();
            finish_historical(read, close(connection))
        })();
        owner.finish(result)
    }

    /// None is strict create. A successful COMMIT is recorded before any later
    /// check/close can fail, so an error never authorizes blind replay.
    pub fn put_controlled_with_check<T: Serialize>(
        home: &Path,
        key: &str,
        value: &T,
        expected: Option<i64>,
        control: &OperationControl,
        supplemental: impl Fn() -> Result<()>,
    ) -> Result<()> {
        check(control, &supplemental)?;
        validate_key(key)?;
        let raw = crate::storage::bounded_json(value, DOCUMENT_LIMIT)?;
        check(control, &supplemental)?;
        Self::put_raw(home, key, &raw, expected, control, &supplemental)
    }
    fn put_raw(
        home: &Path,
        key: &str,
        raw: &[u8],
        expected: Option<i64>,
        control: &OperationControl,
        supplemental: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        let mut owner = MetadataQuota::acquire(home, control, supplemental)?;
        let result = (|| {
            reject_foreign_sidecars(|name| owner.length(name))?;
            if owner.length("cache-v1.sqlite")?.is_none()
                && owner.length("cache-v1.sqlite-journal")?.is_some()
            {
                return Err(invalid());
            }
            // Reserve bootstrap before SQLite may create any payload bytes.
            let journal_max = journal_bound(owner.length("cache-v1.sqlite")?.unwrap_or(0))?;
            owner.admit_write(journal_max, PAGE_BYTES)?;
            owner.prepare_receipt()?;
            let connection = open(owner.path(), true, control, supplemental)?;
            let written = (|| {
                let initialized = validate_connection(&connection, control, supplemental)?;
                // Authentication/recovery can finish before write admission is
                // recalculated, but never permits a malicious enlarged database
                // to escape the original pre-open allowance.
                owner.verify_recovery()?;
                let actual_main = owner.length("cache-v1.sqlite")?.unwrap_or(0);
                let journal_after = journal_bound(actual_main)?;
                let pages_after = owner.admit_write(journal_after, PAGE_BYTES)?;
                #[cfg(test)]
                let pages_after = tests::page_cap(pages_after);
                let actual: u64 = sql(control, supplemental, || {
                    connection
                        .pragma_update_and_check(None, "max_page_count", pages_after, |r| r.get(0))
                })?;
                if actual != pages_after {
                    return Err(invalid());
                }
                mutate(
                    &connection,
                    key,
                    raw,
                    expected,
                    initialized,
                    control,
                    supplemental,
                    |generation| owner.attempting_commit(key, generation),
                    |index, state| owner.commit_state(index, state),
                )
            })();
            finish_historical(written, close(connection))
        })();
        owner.finish(result)
    }
}
fn reject_foreign_sidecars(length: impl Fn(&str) -> Result<Option<u64>>) -> Result<()> {
    for name in ["cache-v1.sqlite-wal", "cache-v1.sqlite-shm"] {
        if length(name)?.is_some() {
            return Err(invalid());
        }
    }
    Ok(())
}
fn open(
    path: std::path::PathBuf,
    create: bool,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NOFOLLOW
        | if create {
            OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::empty()
        };
    let connection = sql(control, supplemental, || {
        Connection::open_with_flags(&path, flags)
    })?;
    let setup = (|| {
        connection.busy_timeout(Duration::ZERO)?;
        #[cfg(test)]
        tests::observe(tests::Point::Opened)?;
        check(control, supplemental)?;
        // Keep SQLite's normal spilling and its existing ~2 MiB cache target.
        // MEMORY avoids unowned temp-directory statement journals, without
        // forcing the entire database dirty set into memory as spill=OFF would.
        sql(control, supplemental, || {
            connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA cache_size=-2000; PRAGMA cache_spill=ON; PRAGMA temp_store=MEMORY; PRAGMA mmap_size=0;")
        })?;
        check(control, supplemental)
    })();
    match setup {
        Ok(()) => Ok(connection),
        Err(error) => finish_historical(Err(error), close(connection)),
    }
}
fn validate_connection(
    connection: &Connection,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
) -> Result<bool> {
    // The first schema read lets mature SQLite recover an ordinary hot journal.
    let version: i32 = sql(control, supplemental, || {
        connection.pragma_query_value(None, "user_version", |r| r.get(0))
    })?;
    let app: i32 = sql(control, supplemental, || {
        connection.pragma_query_value(None, "application_id", |r| r.get(0))
    })?;
    let count: i64 = sql(control, supplemental, || {
        connection.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))
    })?;
    if version == 0 && app == 0 && count == 0 {
        sql(control, supplemental, || {
            connection.pragma_update(None, "page_size", PAGE_BYTES)
        })?;
        sql(control, supplemental, || {
            connection.pragma_update(None, "auto_vacuum", "NONE")
        })?;
    } else if version != 1 || app != APPLICATION_ID || count != 1 {
        return Err(invalid());
    } else {
        let schema: String = sql(control, supplemental, || {
            connection.query_row("SELECT sql FROM sqlite_schema WHERE name='documents' AND type='table' AND tbl_name='documents'", [], |r| r.get(0))
        })?;
        if schema != SCHEMA {
            return Err(invalid());
        }
    }
    let page: u64 = sql(control, supplemental, || {
        connection.pragma_query_value(None, "page_size", |r| r.get(0))
    })?;
    let vacuum: i64 = sql(control, supplemental, || {
        connection.pragma_query_value(None, "auto_vacuum", |r| r.get(0))
    })?;
    let mode: String = sql(control, supplemental, || {
        connection.pragma_query_value(None, "journal_mode", |r| r.get(0))
    })?;
    if page != PAGE_BYTES || vacuum != 0 || !mode.eq_ignore_ascii_case("delete") {
        return Err(invalid());
    }
    sql(control, supplemental, || {
        connection.pragma_update(None, "synchronous", "FULL")
    })?;
    sql(control, supplemental, || {
        connection.pragma_update(None, "locking_mode", "NORMAL")
    })?;
    check(control, supplemental)?;
    Ok(version == 1)
}
fn read_row(
    connection: &Connection,
    key: &str,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
) -> Result<RawDocument> {
    let row: Option<(i64, i64, Option<Vec<u8>>)> = sql(control, supplemental, || {
        connection.query_row("SELECT generation,length(CAST(value AS BLOB)),CASE WHEN length(CAST(value AS BLOB))<=1048576 THEN CAST(value AS BLOB) ELSE NULL END FROM documents WHERE key=?1", [key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()
    })?;
    check(control, supplemental)?;
    match row {
        Some((generation, length, Some(bytes)))
            if generation > 0
                && (0..=DOCUMENT_LIMIT as i64).contains(&length)
                && bytes.len() as i64 == length =>
        {
            Ok(RawDocument { generation, bytes })
        }
        None => Err(missing()),
        _ => Err(invalid()),
    }
}
fn mutate(
    connection: &Connection,
    key: &str,
    raw: &[u8],
    expected: Option<i64>,
    initialized: bool,
    control: &OperationControl,
    supplemental: &dyn Fn() -> Result<()>,
    attempting_commit: impl FnOnce(i64) -> usize,
    settle: impl Fn(usize, &'static str),
) -> Result<()> {
    sql(control, supplemental, || {
        connection.execute_batch("BEGIN IMMEDIATE")
    })?;
    let mut attempted = None;
    let result = (|| {
        if !initialized {
            sql(control, supplemental, || connection.execute_batch(SCHEMA))?;
            sql(control, supplemental, || {
                connection.pragma_update(None, "application_id", APPLICATION_ID)
            })?;
            sql(control, supplemental, || {
                connection.pragma_update(None, "user_version", 1)
            })?;
        }
        let old: Option<i64> = sql(control, supplemental, || {
            connection
                .query_row(
                    "SELECT generation FROM documents WHERE key=?1",
                    [key],
                    |r| r.get(0),
                )
                .optional()
        })?;
        let next = match (old, expected) {
            (None, None) => 1,
            (Some(old), Some(expected)) if old == expected => {
                old.checked_add(1).ok_or_else(conflict)?
            }
            _ => return Err(conflict()),
        };
        if old.is_none() {
            sql(control, supplemental, || {
                connection.execute(
                    "INSERT OR ROLLBACK INTO documents(key,generation,value) VALUES(?1,?2,?3)",
                    params![key, next, raw],
                )
            })?;
        } else {
            sql(control, supplemental, || {
                connection.execute(
                    "UPDATE OR ROLLBACK documents SET generation=?2,value=?3 WHERE key=?1",
                    params![key, next, raw],
                )
            })?;
        }
        #[cfg(test)]
        tests::observe(tests::Point::Mutated)?;
        check(control, supplemental)?;
        let index = attempting_commit(next);
        attempted = Some(index);
        let commit_result = sql(control, supplemental, || connection.execute_batch("COMMIT"));
        #[cfg(test)]
        let commit_result =
            finish_historical(commit_result, tests::observe(tests::Point::CommitReturned));
        commit_result?;
        settle(index, "committed"); // exact fact before any fallible post-COMMIT check
        #[cfg(test)]
        tests::observe(tests::Point::Committed)?;
        check(control, supplemental)
    })();
    // Cleanup is unconditional even after cancellation. SQLite rollback is not
    // retried through a stopped/busy helper; preserve failure then checked-close.
    let rollback = if connection.is_autocommit() {
        Ok(())
    } else {
        let rolled = connection.execute_batch("ROLLBACK").map_err(Into::into);
        if rolled.is_ok() {
            if let Some(index) = attempted {
                settle(index, "rolled_back");
            }
        }
        #[cfg(test)]
        let rolled = finish_historical(rolled, tests::observe(tests::Point::RolledBack));
        rolled
    };
    finish_historical(result, rollback)
}

#[cfg(test)]
mod tests;
