//! A single authoritative SQLite store. No attached writable databases, cache
//! ownership, VACUUM, rebuild, or best-effort corruption fallback is exposed.
use super::*;
use crate::workspace::data_disk::EvidenceQuota;

pub(super) const NAME: &str = "evidence-v2.sqlite";
const PAGE_BYTES: u64 = 65_536;
const APP: i32 = 0x4c444532;
const SCHEMA: &str = "CREATE TABLE documents (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID";
const TOMBSTONES: &str = "CREATE TABLE deleted_generations (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL) WITHOUT ROWID";

fn journal_bound(main: u64) -> Result<u64> {
    let pages = main.checked_add(PAGE_BYTES - 1).ok_or_else(integrity)? / PAGE_BYTES;
    pages
        .checked_mul(PAGE_BYTES + 8)
        .and_then(|bytes| {
            pages
                .checked_add(2)?
                .checked_mul(2 * PAGE_BYTES)?
                .checked_add(bytes)
        })
        .and_then(|bytes| bytes.checked_add(8))
        .ok_or_else(integrity)
}
fn sql<T>(
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
    run: impl FnMut() -> rusqlite::Result<T>,
) -> Result<T> {
    super::super::checked_sql_with_check(
        control,
        control.deadline(),
        &mut || check(),
        &mut || {},
        run,
    )
}
/// The quota lease is already held before any SQLite recovery. A read does not
/// create an absent database. Every schema/bootstrap write is preadmitted.
pub(super) fn open(
    home: &Path,
    owner: &mut EvidenceQuota<'_>,
    create: bool,
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
) -> Result<Option<Metadata>> {
    for sidecar in ["evidence-v2.sqlite-wal", "evidence-v2.sqlite-shm"] {
        if owner.length(sidecar)?.is_some() {
            return Err(integrity());
        }
    }
    let main = owner.pin_existing_main()?;
    if main.is_none() && owner.length("evidence-v2.sqlite-journal")?.is_some() {
        return Err(integrity());
    }
    if main.is_none() && !create {
        return Ok(None);
    }
    if create {
        owner.admit_and_pin_main(journal_bound, PAGE_BYTES)?;
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NOFOLLOW
        | if create {
            OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::empty()
        };
    let connection = sql(control, check, || {
        Connection::open_with_flags(owner.path(), flags)
    })?;
    let result = (|| {
        connection.busy_timeout(std::time::Duration::ZERO)?;
        sql(control, check, || {
            connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA cache_size=-2000; PRAGMA cache_spill=ON; PRAGMA temp_store=MEMORY; PRAGMA mmap_size=0; PRAGMA synchronous=FULL;")
        })?;
        // Schema reads may recover a hot rollback journal, while quota is held.
        let version: i32 = sql(control, check, || {
            connection.pragma_query_value(None, "user_version", |row| row.get(0))
        })?;
        let app: i32 = sql(control, check, || {
            connection.pragma_query_value(None, "application_id", |row| row.get(0))
        })?;
        let count: i64 = sql(control, check, || {
            connection.query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))
        })?;
        let fresh = version == 0 && app == 0 && count == 0;
        if !fresh && (version != 1 || app != APP || count != 2) {
            return Err(integrity());
        }
        if fresh {
            if !create {
                return Err(integrity());
            }
            sql(control, check, || {
                connection.execute_batch("PRAGMA page_size=65536; PRAGMA auto_vacuum=NONE;")
            })?;
        } else {
            for (name, expected) in [("documents", SCHEMA), ("deleted_generations", TOMBSTONES)] {
                let schema: String = sql(control, check, || {
                    connection.query_row("SELECT sql FROM sqlite_schema WHERE name=?1 AND type='table' AND tbl_name=?1", [name], |row| row.get(0))
                })?;
                if schema != expected {
                    return Err(integrity());
                }
            }
        }
        let page: u64 = sql(control, check, || {
            connection.pragma_query_value(None, "page_size", |row| row.get(0))
        })?;
        let vacuum: i64 = sql(control, check, || {
            connection.pragma_query_value(None, "auto_vacuum", |row| row.get(0))
        })?;
        let journal: String = sql(control, check, || {
            connection.pragma_query_value(None, "journal_mode", |row| row.get(0))
        })?;
        if page != PAGE_BYTES || vacuum != 0 || !journal.eq_ignore_ascii_case("delete") {
            return Err(integrity());
        }
        if !fresh {
            let unknown: bool = sql(control, check, || {
                connection.query_row("SELECT EXISTS(SELECT 1 FROM documents WHERE NOT (substr(key,1,8)='capture/' OR substr(key,1,14)='evidence/keep/' OR substr(key,1,16)='evidence/remove/'))", [], |row| row.get(0))
            })?;
            if unknown {
                return Err(integrity());
            }
        }
        owner.verify_recovery()?;
        if create {
            let pages = owner.admit_write(journal_bound, PAGE_BYTES)?;
            let actual: u64 = sql(control, check, || {
                connection.pragma_update_and_check(None, "max_page_count", pages, |row| row.get(0))
            })?;
            if actual != pages {
                return Err(integrity());
            }
        }
        if fresh {
            sql(control, check, || {
                connection.execute_batch("BEGIN IMMEDIATE")
            })?;
            let written = (|| {
                sql(control, check, || connection.execute_batch(SCHEMA))?;
                sql(control, check, || connection.execute_batch(TOMBSTONES))?;
                sql(control, check, || {
                    connection.pragma_update(None, "application_id", APP)
                })?;
                sql(control, check, || {
                    connection.pragma_update(None, "user_version", 1)
                })?;
                sql(control, check, || connection.execute_batch("COMMIT"))
            })();
            let rollback = if connection.is_autocommit() {
                Ok(())
            } else {
                connection.execute_batch("ROLLBACK").map_err(Into::into)
            };
            finish_owned(written, rollback)?;
        }
        control.check()?;
        check()
    })();
    match result {
        Ok(()) => Ok(Some(Metadata {
            connection,
            home: std::path::absolute(home)?,
            writable: create,
            evidence_gate_held: true,
            catalog_only: false,
        })),
        Err(error) => finish_owned(
            Err(error),
            connection.close().map_err(|(_, error)| error.into()),
        ),
    }
}

pub(super) fn admit_mutation(
    metadata: &mut Metadata,
    owner: &mut EvidenceQuota<'_>,
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    admit_connection(metadata, owner, control, check)?;
    metadata.writable = true;
    Ok(())
}

pub(super) fn admit_connection(
    metadata: &Metadata,
    owner: &mut EvidenceQuota<'_>,
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    let pages = owner.admit_write(journal_bound, PAGE_BYTES)?;
    let actual: u64 = sql(control, check, || {
        metadata
            .connection
            .pragma_update_and_check(None, "max_page_count", pages, |row| row.get(0))
    })?;
    if actual != pages {
        return Err(integrity());
    }
    Ok(())
}
