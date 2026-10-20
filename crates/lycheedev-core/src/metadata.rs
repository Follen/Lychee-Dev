use crate::{Result, ToolkitError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;

// Fixed Go v3.2.3 keeps a removed key's last generation independently of its
// document. Every writer, including immutable capture publication, must use
// that same history so a delete/recreate cannot authorize an older CAS.
const DELETED_GENERATIONS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS deleted_generations (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL) WITHOUT ROWID";
const INSERT_DOCUMENT: &str = "INSERT INTO documents(key,generation,value) VALUES(?1,COALESCE((SELECT generation+1 FROM deleted_generations WHERE key=?1),1),?2)";

// Call only inside the same write transaction as the corresponding DELETE.
// A missing document does not erase an earlier tombstone.
fn remember_deleted_generation(connection: &Connection, key: &str) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO deleted_generations(key,generation) SELECT key,generation FROM documents WHERE key=?1 ON CONFLICT(key) DO UPDATE SET generation=excluded.generation",
        [key],
    )
}

#[derive(Debug, Clone)]
pub struct RawDocument {
    pub generation: i64,
    pub bytes: Vec<u8>,
}

pub struct Metadata {
    connection: Connection,
    // Bound at open, never supplied by a later capture caller.
    home: std::path::PathBuf,
    writable: bool,
    // Set only by the catalog scope which owns the actual interprocess gate.
    evidence_gate_held: bool,
    catalog_only: bool,
}
#[derive(Debug, Clone)]
pub struct Document {
    pub key: String,
    pub generation: i64,
    pub value: serde_json::Value,
}
/// Read exact document bytes while the caller's write transaction is held.
/// Kept separate from public Document so existing JSON-value consumers retain
/// their API and do not pay for a second retained copy of every scanned row.
pub(crate) struct TransactionReader<'a> {
    connection: &'a Connection,
    control: &'a crate::control::OperationControl,
    catalog: Option<&'a evidence_catalog::Catalog>,
}
impl TransactionReader<'_> {
    pub(crate) fn read_raw(&self, key: &str) -> Result<Vec<u8>> {
        self.control.check()?;
        if let Some(catalog) = self.catalog {
            return evidence_catalog::required(catalog.document(key, self.control, &|| Ok(()))?)
                .map(|row| row.bytes);
        }
        let raw: Option<Vec<u8>> = self
            .connection
            .query_row("SELECT value FROM documents WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        self.control.check()?;
        raw.ok_or_else(|| {
            ToolkitError::new(
                "vault.record_missing",
                format!("document {key} does not exist"),
                3,
            )
        })
    }
}
impl Metadata {
    fn require_write_capability(&self) -> Result<()> {
        if !self.writable {
            return Err(ToolkitError::invalid("metadata handle is read-only"));
        }
        Ok(())
    }

    pub fn open(home: &Path, write: bool) -> Result<Self> {
        crate::workspace::open(home)?;
        let path = home.join("state/toolkit.sqlite");
        if write {
            std::fs::create_dir_all(path.parent().expect("metadata parent"))?;
        }
        if !path.exists() && !write {
            if evidence_catalog::present(home)? {
                return evidence_catalog::read_carrier(
                    home,
                    &crate::control::OperationControl::default(),
                    &|| Ok(()),
                );
            }
            return Err(ToolkitError::new(
                "vault.record_missing",
                "workspace metadata does not exist",
                3,
            ));
        }
        let flags = if write {
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let connection = Connection::open_with_flags(path, flags)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
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
            connection.execute_batch("PRAGMA synchronous=FULL; BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS documents (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID; PRAGMA user_version=1; COMMIT;")?;
        }
        if write {
            connection.pragma_update(None, "synchronous", "FULL")?;
            connection.execute_batch(DELETED_GENERATIONS_SCHEMA)?;
        }
        Ok(Self {
            connection,
            home: std::path::absolute(home)?,
            writable: write,
            evidence_gate_held: false,
            catalog_only: false,
        })
    }
    /// Read one historical Go document as original bytes. Unlike `read`, this
    /// path never UTF-8/JSON-decodes before the caller's Go-compatible decoder.
    /// Open, schema inspection and the single-row read share the original stop
    /// control; SQLite's busy handler cannot hide cancellation for five seconds.
    /// The 1 MiB ceiling is the fixed-Go documents CHECK in vault/metadata.go.
    pub fn read_raw_controlled(
        home: &Path,
        key: &str,
        control: &crate::control::OperationControl,
    ) -> Result<Vec<u8>> {
        Self::read_document_raw_controlled(home, key, control).map(|document| document.bytes)
    }
    pub fn read_document_raw_controlled(
        home: &Path,
        key: &str,
        control: &crate::control::OperationControl,
    ) -> Result<RawDocument> {
        Self::read_document_raw_controlled_with_check(home, key, control, || Ok(()))
    }
    /// Also check an independent caller budget inside every SQLite wait.
    /// The supplemental check never extends the original five-second cutoff.
    pub fn read_document_raw_controlled_with_check(
        home: &Path,
        key: &str,
        control: &crate::control::OperationControl,
        check: impl FnMut() -> Result<()>,
    ) -> Result<RawDocument> {
        Self::read_raw_controlled_observed_with_check(home, key, control, check, || {})
    }
    #[cfg(test)]
    fn read_raw_controlled_observed(
        home: &Path,
        key: &str,
        control: &crate::control::OperationControl,
        on_busy: impl FnMut(),
    ) -> Result<RawDocument> {
        Self::read_raw_controlled_observed_with_check(home, key, control, || Ok(()), on_busy)
    }
    fn read_raw_controlled_observed_with_check(
        home: &Path,
        key: &str,
        control: &crate::control::OperationControl,
        mut check: impl FnMut() -> Result<()>,
        mut on_busy: impl FnMut(),
    ) -> Result<RawDocument> {
        metadata_check(control, &mut check)?;
        if evidence_catalog::EvidenceKey::parse(key).is_some() && evidence_catalog::present(home)? {
            let checks = std::cell::RefCell::new(&mut check);
            return evidence_catalog::required(evidence_catalog::read(
                home,
                key,
                control,
                &|| (*checks.borrow_mut())(),
            )?);
        }
        crate::workspace::open(home)?;
        metadata_check(control, &mut check)?;
        let path = home.join("state/toolkit.sqlite");
        match std::fs::metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ToolkitError::new(
                    "vault.record_missing",
                    "vault.record_missing",
                    2,
                ));
            }
            Err(error) => return Err(error.into()),
        }
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let connection = checked_sql_with_check(control, until, &mut check, &mut on_busy, || {
            Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        })?;
        let result = (|| {
            // Any post-open failure stays inside this connection's close scope.
            metadata_check(control, &mut check)?;
            connection.busy_timeout(std::time::Duration::ZERO)?;
            let version: i32 =
                checked_sql_with_check(control, until, &mut check, &mut on_busy, || {
                    connection.pragma_query_value(None, "user_version", |row| row.get(0))
                })?;
            metadata_check(control, &mut check)?;
            if version != 1 {
                return Err(ToolkitError::new(
                    "vault.unsupported_workspace",
                    "vault.unsupported_workspace",
                    4,
                ));
            }
            // One SQL statement is an atomic read snapshot. Reject an invalid
            // oversized row before allocating its bytes, even in a corrupted
            // database that has bypassed the ordinary table CHECK constraint.
            let row: Option<(i64, i64, Option<Vec<u8>>)> = checked_sql_with_check(
                control,
                until,
                &mut check,
                &mut on_busy,
                || {
                    connection.query_row(
                    "SELECT generation,length(CAST(value AS BLOB)),CASE WHEN length(CAST(value AS BLOB))<=1048576 THEN CAST(value AS BLOB) ELSE NULL END FROM documents WHERE key=?1",
                    [key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                ).optional()
                },
            )?;
            metadata_check(control, &mut check)?;
            match row {
                None => Err(ToolkitError::new(
                    "vault.record_missing",
                    "vault.record_missing",
                    2,
                )),
                Some((generation, length, Some(raw)))
                    if (0..=1_048_576).contains(&length) && raw.len() as i64 == length =>
                {
                    Ok(RawDocument {
                        generation,
                        bytes: raw,
                    })
                }
                Some(_) => Err(ToolkitError::invalid("metadata document exceeds 1 MiB")),
            }
        })();
        // No statement/row escapes this scope. Close is checked on success and
        // failure; a close failure cannot replace the original stop/error.
        match connection.close() {
            Ok(()) => match result {
                Ok(raw) => {
                    metadata_check(control, &mut check)?;
                    Ok(raw)
                }
                Err(error) => Err(error),
            },
            Err((_connection, close)) => match result {
                Ok(_) => Err(close.into()),
                Err(mut original) => {
                    original.details = serde_json::json!({"primaryDetails":original.details,"metadataCloseError":close.to_string()});
                    Err(original)
                }
            },
        }
    }
    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Result<T> {
        if (self.catalog_only || evidence_catalog::EvidenceKey::parse(key).is_some())
            && evidence_catalog::present(&self.home)?
        {
            let control = crate::control::OperationControl::default();
            let row = evidence_catalog::required(evidence_catalog::read(
                &self.home,
                key,
                &control,
                &|| Ok(()),
            )?)?;
            return serde_json::from_slice(&row.bytes).map_err(Into::into);
        }
        let bytes: Option<Vec<u8>> = self
            .connection
            .query_row("SELECT value FROM documents WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        match bytes {
            Some(raw) => Ok(serde_json::from_slice(&raw)?),
            None => Err(ToolkitError::new(
                "vault.record_missing",
                format!("document {key} does not exist"),
                3,
            )),
        }
    }
    pub fn generation(&self, key: &str) -> Result<Option<i64>> {
        if (self.catalog_only || evidence_catalog::EvidenceKey::parse(key).is_some())
            && evidence_catalog::present(&self.home)?
        {
            return evidence_catalog::read(
                &self.home,
                key,
                &crate::control::OperationControl::default(),
                &|| Ok(()),
            )
            .map(|row| row.map(|row| row.generation));
        }
        Ok(self
            .connection
            .query_row(
                "SELECT generation FROM documents WHERE key=?1",
                [key],
                |row| row.get(0),
            )
            .optional()?)
    }
    pub fn put<T: Serialize>(&mut self, key: &str, value: &T, expected: Option<i64>) -> Result<()> {
        let raw = serde_json::to_vec(value)?;
        self.put_serialized(key, &raw, expected, || Ok(()))
    }
    /// Atomic creation: an existing key is a typed conflict even when its
    /// document bytes match. Existing put callers retain byte idempotence.
    pub fn create<T: Serialize>(&mut self, key: &str, value: &T) -> Result<()> {
        let raw = serde_json::to_vec(value)?;
        self.put_serialized_policy(key, &raw, None, true, || Ok(()))
    }
    pub fn create_controlled<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        control: &crate::control::OperationControl,
    ) -> Result<()> {
        self.create_controlled_with_check(key, value, control, || Ok(()))
    }
    /// Strict creation with an independent stop check at every SQL boundary.
    pub fn create_controlled_with_check<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        control: &crate::control::OperationControl,
        check: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        self.put_controlled_policy_with_check(key, value, None, control, true, check, || {})
    }
    /// Cancellation-aware publication. SQLite never sleeps inside an attempt:
    /// lock contention is retried with a stop check before every statement,
    /// including COMMIT. Every uncommitted exit explicitly checks rollback.
    pub fn put_controlled<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        expected: Option<i64>,
        control: &crate::control::OperationControl,
    ) -> Result<()> {
        self.put_controlled_with_check(key, value, expected, control, || Ok(()))
    }
    /// Keep invocation and supplemental budget checks independent; no control
    /// lineage is combined and neither check runs under the other's mutex.
    pub fn put_controlled_with_check<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        expected: Option<i64>,
        control: &crate::control::OperationControl,
        check: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        self.put_controlled_policy_with_check(key, value, expected, control, false, check, || {})
    }
    #[cfg(test)]
    fn put_controlled_observed<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        expected: Option<i64>,
        control: &crate::control::OperationControl,
        on_busy: impl FnMut(),
    ) -> Result<()> {
        self.put_controlled_policy(key, value, expected, control, false, on_busy)
    }
    #[cfg(test)]
    fn put_controlled_policy<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        expected: Option<i64>,
        control: &crate::control::OperationControl,
        strict_create: bool,
        on_busy: impl FnMut(),
    ) -> Result<()> {
        self.put_controlled_policy_with_check(
            key,
            value,
            expected,
            control,
            strict_create,
            || Ok(()),
            on_busy,
        )
    }
    fn put_controlled_policy_with_check<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
        expected: Option<i64>,
        control: &crate::control::OperationControl,
        strict_create: bool,
        mut check: impl FnMut() -> Result<()>,
        mut on_busy: impl FnMut(),
    ) -> Result<()> {
        self.require_write_capability()?;
        if !self.evidence_gate_held && evidence_catalog::EvidenceKey::parse(key).is_some() {
            let home = self.home.clone();
            let checks = std::cell::RefCell::new(&mut check);
            return evidence_catalog::mutate(
                &home,
                key,
                control,
                &|| (*checks.borrow_mut())(),
                |metadata| {
                    metadata.put_controlled_policy_with_check(
                        key,
                        value,
                        expected,
                        control,
                        strict_create,
                        || (*checks.borrow_mut())(),
                        &mut on_busy,
                    )
                },
            );
        }
        let evidence_gate = if self.evidence_gate_held {
            None
        } else {
            Some(crate::metadata::evidence_catalog::acquire_gate(
                &self.home,
                control,
                || check(),
            )?)
        };
        let result = (|| {
            metadata_check(control, &mut check)?;
            let raw = serde_json::to_vec(value)?;
            if raw.len() > 1_048_576 {
                return Err(ToolkitError::invalid("metadata document exceeds 1 MiB"));
            }
            metadata_check(control, &mut check)?;
            self.connection.busy_timeout(std::time::Duration::ZERO)?;
            let result = (|| {
                let until = control.deadline();
                let mut transaction =
                    checked_sql_with_check(control, until, &mut check, &mut on_busy, || {
                        rusqlite::Transaction::new_unchecked(
                            &self.connection,
                            rusqlite::TransactionBehavior::Immediate,
                        )
                    })?;
                let written = (|| {
                    let existing: Option<(i64, bool)> =
                        checked_sql_with_check(control, until, &mut check, &mut on_busy, || {
                            transaction
                                .query_row(
                                    "SELECT generation,value=?2 FROM documents WHERE key=?1",
                                    params![key, &raw],
                                    |row| Ok((row.get(0)?, row.get(1)?)),
                                )
                                .optional()
                        })?;
                    match existing {
                        Some((generation, _)) if expected == Some(generation) => {
                            checked_sql_with_check(
                                control,
                                until,
                                &mut check,
                                &mut on_busy,
                                || {
                                    transaction.execute(
                                "UPDATE documents SET generation=generation+1,value=?2 WHERE key=?1",
                                params![key, &raw],
                            )
                                },
                            )?;
                        }
                        Some((_, true)) if expected.is_none() && !strict_create => {
                            return metadata_check(control, &mut check);
                        }
                        None if expected.is_none() => {
                            checked_sql_with_check(
                                control,
                                until,
                                &mut check,
                                &mut on_busy,
                                || transaction.execute(INSERT_DOCUMENT, params![key, &raw]),
                            )?;
                        }
                        _ => {
                            return Err(ToolkitError::new(
                                "vault.generation_conflict",
                                "document changed or disappeared",
                                4,
                            ));
                        }
                    }
                    // Transaction::commit consumes the guard on SQLITE_BUSY. Keep it
                    // alive for checked retries; rollback below handles other exits.
                    checked_sql_with_check(control, until, &mut check, &mut on_busy, || {
                        transaction.execute_batch("COMMIT")
                    })?;
                    Ok(())
                })();
                // Successful byte-idempotence also exits an active transaction.
                // Explicitly roll back every uncommitted exit, even after stop.
                let rollback = if !transaction.is_autocommit() {
                    transaction
                        .execute_batch("ROLLBACK")
                        .map_err(ToolkitError::from)
                } else {
                    Ok(())
                };
                transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
                historical_write::finish_historical(written, rollback)
            })();
            let restore = self
                .connection
                .busy_timeout(std::time::Duration::from_secs(5));
            historical_write::finish_historical(result, restore.map_err(ToolkitError::from))
        })();
        crate::metadata::historical_write::finish_historical(
            result,
            evidence_gate.map_or(Ok(()), crate::storage::ExclusiveLock::close),
        )
    }
    /// The caller owns already serialized JSON. Keep insertion, replacement and
    /// byte-identical idempotence in the same transaction as `put`.
    pub(crate) fn put_serialized(
        &mut self,
        key: &str,
        raw: &[u8],
        expected: Option<i64>,
        check: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        self.put_serialized_policy(key, raw, expected, false, check)
    }
    fn put_serialized_policy(
        &mut self,
        key: &str,
        raw: &[u8],
        expected: Option<i64>,
        strict_create: bool,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        self.require_write_capability()?;
        if !self.evidence_gate_held && evidence_catalog::EvidenceKey::parse(key).is_some() {
            let home = self.home.clone();
            let control = crate::control::OperationControl::default();
            let checks = std::cell::RefCell::new(&mut check);
            return evidence_catalog::mutate(
                &home,
                key,
                &control,
                &|| (*checks.borrow_mut())(),
                |metadata| {
                    metadata.put_serialized_policy(key, raw, expected, strict_create, || {
                        (*checks.borrow_mut())()
                    })
                },
            );
        }
        let gate_control = crate::control::OperationControl::default();
        let evidence_gate = if self.evidence_gate_held {
            None
        } else {
            Some(crate::metadata::evidence_catalog::acquire_gate(
                &self.home,
                &gate_control,
                || check(),
            )?)
        };
        let result = (|| {
            if raw.len() > 1_048_576 {
                return Err(ToolkitError::invalid("metadata document exceeds 1 MiB"));
            }
            check()?;
            let transaction = self
                .connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let existing: Option<(i64, bool)> = transaction
                .query_row(
                    "SELECT generation,value=?2 FROM documents WHERE key=?1",
                    params![key, raw],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            check()?;
            if let Some((generation, same_bytes)) = existing {
                if expected == Some(generation) {
                    transaction.execute(
                        "UPDATE documents SET generation=generation+1,value=?2 WHERE key=?1",
                        params![key, raw],
                    )?;
                } else if expected.is_none() && same_bytes && !strict_create {
                    return Ok(());
                } else {
                    return Err(ToolkitError::new(
                        "vault.generation_conflict",
                        "document changed or already exists",
                        4,
                    ));
                }
            } else {
                if expected.is_some() {
                    return Err(ToolkitError::new(
                        "vault.generation_conflict",
                        "document disappeared",
                        4,
                    ));
                }
                transaction.execute(INSERT_DOCUMENT, params![key, raw])?;
            }
            check()?;
            transaction.commit()?;
            Ok(())
        })();
        crate::metadata::historical_write::finish_historical(
            result,
            evidence_gate.map_or(Ok(()), crate::storage::ExclusiveLock::close),
        )
    }
    /// Validate immutable references and publish their owner under the same
    /// SQLite write transaction used by checked removal. Admission precedes
    /// each reference allocation; publication cannot run on a missing or
    /// invalid reference. The original control bounds BEGIN and COMMIT waits.
    #[cfg(test)]
    pub(crate) fn put_serialized_referencing(
        &mut self,
        key: &str,
        raw: &[u8],
        references: &[String],
        control: &crate::control::OperationControl,
        mut check: impl FnMut() -> Result<()>,
        mut admit_reference: impl FnMut(usize) -> Result<()>,
        mut validate_reference: impl FnMut(&str, &[u8]) -> Result<()>,
        publish: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let evidence_gate = if self.evidence_gate_held {
            None
        } else {
            Some(crate::metadata::evidence_catalog::acquire_gate(
                &self.home,
                control,
                || check(),
            )?)
        };
        let result = (|| {
            if !self.evidence_gate_held && evidence_catalog::present(&self.home)? {
                return Err(ToolkitError::invalid(
                    "legacy reference mutation requires the authoritative catalog",
                ));
            }

            check()?;
            if raw.len() > 1_048_576 {
                return Err(ToolkitError::invalid("metadata document exceeds 1 MiB"));
            }
            self.connection.busy_timeout(std::time::Duration::ZERO)?;
            let result = (|| {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let mut transaction = checked_sql(control, until, &mut || {}, || {
                    rusqlite::Transaction::new_unchecked(
                        &self.connection,
                        rusqlite::TransactionBehavior::Immediate,
                    )
                })?;
                for reference in references {
                    check()?;
                    let length: Option<usize> = checked_sql(control, until, &mut || {}, || {
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
                    admit_reference(length)?;
                    check()?;
                    let document: Vec<u8> = checked_sql(control, until, &mut || {}, || {
                        transaction.query_row(
                            "SELECT value FROM documents WHERE key=?1",
                            [reference],
                            |row| row.get(0),
                        )
                    })?;
                    validate_reference(reference, &document)?;
                }
                check()?;
                publish()?;
                check()?;
                let existing: Option<bool> = checked_sql(control, until, &mut || {}, || {
                    transaction
                        .query_row(
                            "SELECT value=?2 FROM documents WHERE key=?1",
                            params![key, raw],
                            |row| row.get(0),
                        )
                        .optional()
                })?;
                match existing {
                    Some(true) => return check(),
                    Some(false) => {
                        return Err(ToolkitError::new(
                            "vault.generation_conflict",
                            "immutable document already differs",
                            4,
                        ));
                    }
                    None => {
                        check()?;
                        checked_sql(control, until, &mut || {}, || {
                            transaction.execute(INSERT_DOCUMENT, params![key, raw])
                        })?;
                    }
                }
                check()?;
                checked_sql(control, until, &mut || {}, || {
                    transaction.execute_batch("COMMIT")
                })?;
                transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
                Ok(())
            })();
            let restore = self
                .connection
                .busy_timeout(std::time::Duration::from_secs(5));
            result?;
            restore?;
            Ok(())
        })();
        crate::metadata::historical_write::finish_historical(
            result,
            evidence_gate.map_or(Ok(()), crate::storage::ExclusiveLock::close),
        )
    }
    pub fn list<T: DeserializeOwned>(&self, prefix: &str, limit: usize) -> Result<Vec<T>> {
        if (self.catalog_only || evidence_catalog::intersects(prefix))
            && evidence_catalog::present(&self.home)?
        {
            let control = crate::control::OperationControl::default();
            return evidence_catalog::with_catalog(
                &self.home,
                None,
                &control,
                &|| Ok(()),
                |catalog, _| {
                    let mut rows = Vec::new();
                    // list has an inclusive prefix, unlike the public page
                    // API's exclusive continuation cursor.
                    if limit != 0 {
                        if let Some(row) = catalog.document(prefix, &control, &|| Ok(()))? {
                            rows.push(serde_json::from_slice(&row.bytes)?);
                        }
                    }
                    let mut after = prefix.to_owned();
                    while rows.len() < limit {
                        let remaining = (limit - rows.len()).min(1000);
                        let (count, last) = catalog.visit(
                            prefix,
                            &after,
                            remaining,
                            &control,
                            &|| Ok(()),
                            &mut |_, _, bytes| {
                                rows.push(serde_json::from_slice(bytes)?);
                                Ok(())
                            },
                        )?;
                        if count < remaining {
                            break;
                        }
                        after = last;
                    }
                    Ok(rows)
                },
            );
        }
        let mut statement = self.connection.prepare(
            "SELECT value FROM documents WHERE key>=?1 AND key<?2 ORDER BY key LIMIT ?3",
        )?;
        let end = format!("{prefix}\u{10ffff}");
        let rows = statement.query_map(params![prefix, end, limit as i64], |row| {
            row.get::<_, Vec<u8>>(0)
        })?;
        rows.map(|row| Ok(serde_json::from_slice(&row?)?)).collect()
    }
    pub fn delete(&mut self, key: &str) -> Result<bool> {
        self.require_write_capability()?;
        if !self.evidence_gate_held && evidence_catalog::EvidenceKey::parse(key).is_some() {
            let home = self.home.clone();
            let control = crate::control::OperationControl::default();
            return evidence_catalog::mutate(&home, key, &control, &|| Ok(()), |metadata| {
                metadata.delete(key)
            });
        }
        let gate_control = crate::control::OperationControl::default();
        let evidence_gate = if self.evidence_gate_held {
            None
        } else {
            Some(crate::metadata::evidence_catalog::acquire_gate(
                &self.home,
                &gate_control,
                || Ok(()),
            )?)
        };
        let result = (|| {
            let transaction = self
                .connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            remember_deleted_generation(&transaction, key)?;
            let deleted = transaction.execute("DELETE FROM documents WHERE key=?1", [key])? != 0;
            transaction.commit()?;
            Ok(deleted)
        })();
        crate::metadata::historical_write::finish_historical(
            result,
            evidence_gate.map_or(Ok(()), crate::storage::ExclusiveLock::close),
        )
    }
    /// Validate and consume one document in the same write transaction.
    /// Unrelated workspace documents are not loaded for a single-use intent.
    pub fn consume<T: DeserializeOwned, U>(
        &mut self,
        key: &str,
        validate: impl FnOnce(T) -> Result<U>,
    ) -> Result<U> {
        self.consume_controlled(key, &crate::control::OperationControl::default(), validate)
    }
    /// Hold a single SQLite write transaction through bounded reference validation and precise deletions.
    pub fn delete_checked<T>(
        &mut self,
        keys: &[String],
        expected: &[(String, i64)],
        validate: impl FnOnce(&[Document]) -> Result<T>,
    ) -> Result<T> {
        self.delete_checked_record(keys, expected, |documents| Ok((validate(documents)?, None)))
    }
    /// Commit an owned recovery record alongside the checked deletion, never after it.
    pub fn delete_checked_record<T>(
        &mut self,
        keys: &[String],
        expected: &[(String, i64)],
        validate: impl FnOnce(&[Document]) -> Result<(T, Option<(String, serde_json::Value)>)>,
    ) -> Result<T> {
        self.delete_checked_record_controlled(
            keys,
            expected,
            &crate::control::OperationControl::default(),
            validate,
        )
    }
    pub(crate) fn delete_checked_controlled<T>(
        &mut self,
        keys: &[String],
        expected: &[(String, i64)],
        control: &crate::control::OperationControl,
        validate: impl FnOnce(&[Document], &TransactionReader<'_>) -> Result<T>,
    ) -> Result<T> {
        self.delete_checked_record_controlled_with_raw(
            keys,
            expected,
            control,
            |documents, reader| Ok((validate(documents, reader)?, None)),
        )
    }
    pub(crate) fn delete_checked_record_controlled<T>(
        &mut self,
        keys: &[String],
        expected: &[(String, i64)],
        control: &crate::control::OperationControl,
        validate: impl FnOnce(&[Document]) -> Result<(T, Option<(String, serde_json::Value)>)>,
    ) -> Result<T> {
        self.delete_checked_record_controlled_with_raw(keys, expected, control, |documents, _| {
            validate(documents)
        })
    }
    pub(crate) fn delete_checked_record_controlled_with_raw<T>(
        &mut self,
        keys: &[String],
        expected: &[(String, i64)],
        control: &crate::control::OperationControl,
        validate: impl FnOnce(
            &[Document],
            &TransactionReader<'_>,
        ) -> Result<(T, Option<(String, serde_json::Value)>)>,
    ) -> Result<T> {
        self.require_write_capability()?;
        evidence_catalog::checked_delete(&self.home, keys, expected, control, validate)
    }
}

fn checked_sql<T>(
    control: &crate::control::OperationControl,
    until: std::time::Instant,
    on_busy: &mut impl FnMut(),
    execute: impl FnMut() -> rusqlite::Result<T>,
) -> Result<T> {
    checked_sql_with_check(control, until, &mut || Ok(()), on_busy, execute)
}

// Check serially: supplemental code must not be invoked inside OperationControl
// or while holding a supplemental budget's mutex around OperationControl.
fn metadata_check(
    control: &crate::control::OperationControl,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    control.check()?;
    check()
}

// Test-only, thread-local observations of this actual SQL contention branch.
// Callers compare entry/exit counts; no callback, reset or cross-test hook exists.
#[cfg(test)]
std::thread_local! {
    static SQL_BUSY_OBSERVATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn sql_busy_observations() -> u64 {
    SQL_BUSY_OBSERVATIONS.with(std::cell::Cell::get)
}

fn checked_sql_with_check<T>(
    control: &crate::control::OperationControl,
    until: std::time::Instant,
    check: &mut impl FnMut() -> Result<()>,
    on_busy: &mut impl FnMut(),
    mut execute: impl FnMut() -> rusqlite::Result<T>,
) -> Result<T> {
    loop {
        metadata_check(control, check)?;
        match execute() {
            // The owner must perform any post-success check in its cleanup
            // scope. In particular, do not silently Drop a newly opened handle.
            Ok(value) => return Ok(value),
            Err(error) => {
                let busy = matches!(&error, rusqlite::Error::SqliteFailure(inner, _)
                    if matches!(inner.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked));
                let error = ToolkitError::from(error);
                metadata_check_after_sql_error(control, check, &error)?;
                if !busy || std::time::Instant::now() >= until {
                    // Keep the legacy control-first precedence at this cutoff.
                    metadata_check_after_sql_error(control, check, &error)?;
                    return Err(error);
                }
                on_busy();
                #[cfg(test)]
                SQL_BUSY_OBSERVATIONS.with(|count| count.set(count.get() + 1));
                metadata_check_after_sql_error(control, check, &error)?;
                std::thread::sleep(
                    std::time::Duration::from_millis(5).min(
                        control
                            .deadline()
                            .saturating_duration_since(std::time::Instant::now()),
                    ),
                );
            }
        }
    }
}
fn metadata_check_after_sql_error(
    control: &crate::control::OperationControl,
    check: &mut impl FnMut() -> Result<()>,
    sqlite_error: &ToolkitError,
) -> Result<()> {
    control.check()?;
    check().map_err(|mut stop| {
        // SQL errors are not cleanup failures. Preserve the complete typed
        // supplemental cause, including its previous details, and the SQL cause.
        let original = stop.clone();
        stop.details = serde_json::json!({
            "originalSupplemental": original,
            "sqliteError": sqlite_error,
        });
        stop
    })
}

#[cfg(test)]
mod controlled_tests {
    use super::*;
    use crate::control::{OperationControl, TrackedCancellation};
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    #[test]
    fn controlled_pin_document_rolls_back_when_cancelled_during_sqlite_lock_wait() {
        // Test both BEGIN contention and a reader blocking COMMIT. The busy
        // observer establishes the precise blocked state, without sleep races.
        for commit_blocked in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "lychee-controlled-pin-{}-{}-{}",
                std::process::id(),
                commit_blocked,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            ));
            crate::workspace::initialize(&path).unwrap();
            let mut metadata = Metadata::open(&path, true).unwrap();
            metadata.put("seed", &true, None).unwrap();
            let blocker = Connection::open(path.join("state/toolkit.sqlite")).unwrap();
            if commit_blocked {
                blocker
                    .execute_batch("BEGIN; SELECT * FROM documents;")
                    .unwrap();
            } else {
                blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
            }
            let cancel = TrackedCancellation::new();
            let control =
                OperationControl::tracked(Instant::now() + Duration::from_secs(10), cancel.clone());
            let (blocked_tx, blocked_rx) = mpsc::channel();
            let (resume_tx, resume_rx) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                let mut first = true;
                metadata.put_controlled_observed("pin/test", &true, None, &control, || {
                    if first {
                        first = false;
                        blocked_tx.send(()).unwrap();
                        resume_rx.recv().unwrap();
                    }
                })
            });
            blocked_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            cancel.cancel();
            // Release the backend only after cancellation has linearized.
            blocker.execute_batch("ROLLBACK").unwrap();
            resume_tx.send(()).unwrap();
            assert_eq!(worker.join().unwrap().unwrap_err().exit_code, 7);
            let mut metadata = Metadata::open(&path, true).unwrap();
            assert_eq!(metadata.generation("pin/test").unwrap(), None);
            metadata
                .put_controlled("pin/test", &true, None, &OperationControl::default())
                .unwrap();
            assert_eq!(metadata.generation("pin/test").unwrap(), Some(1));
            drop(metadata);
            drop(blocker);
            std::fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn dependency_transaction_stops_at_begin_and_commit_and_preserves_reference() {
        // A held writer prevents BEGIN; a held reader prevents COMMIT. Neither
        // lock is released until the original control has actually stopped.
        for commit_blocked in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "lychee-capture-transaction-{}-{}-{}",
                std::process::id(),
                commit_blocked,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&path).unwrap();
            crate::workspace::initialize(&path).unwrap();
            let mut metadata = Metadata::open(&path, true).unwrap();
            metadata
                .put("capture/source", &serde_json::json!({"id":"source"}), None)
                .unwrap();
            let blocker = Connection::open(path.join("state/toolkit.sqlite")).unwrap();
            blocker
                .execute_batch(if commit_blocked {
                    "BEGIN; SELECT * FROM documents;"
                } else {
                    "BEGIN IMMEDIATE;"
                })
                .unwrap();
            let control =
                OperationControl::uncancelable(Instant::now() + Duration::from_millis(60));
            let published = std::cell::Cell::new(false);
            let error = metadata
                .put_serialized_referencing(
                    "capture/owner",
                    br#"{"dependencies":["source"]}"#,
                    &["capture/source".into()],
                    &control,
                    || control.check(),
                    |_| Ok(()),
                    |key, raw| {
                        assert_eq!(key, "capture/source");
                        assert_eq!(
                            serde_json::from_slice::<serde_json::Value>(raw).unwrap()["id"],
                            "source"
                        );
                        Ok(())
                    },
                    || {
                        published.set(true);
                        Ok(())
                    },
                )
                .unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.exit_code),
                ("command.deadline", 5)
            );
            assert_eq!(published.get(), commit_blocked);
            blocker.execute_batch("ROLLBACK").unwrap();
            assert_eq!(metadata.generation("capture/owner").unwrap(), None);
            assert_eq!(metadata.generation("capture/source").unwrap(), Some(1));
            // A failed controlled operation restores the connection and leaves
            // the original source available to a subsequent normal operation.
            metadata
                .put_serialized_referencing(
                    "capture/owner",
                    br#"{"dependencies":["source"]}"#,
                    &["capture/source".into()],
                    &OperationControl::default(),
                    || Ok(()),
                    |_| Ok(()),
                    |_, _| Ok(()),
                    || Ok(()),
                )
                .unwrap();
            assert_eq!(metadata.generation("capture/owner").unwrap(), Some(1));
            drop(metadata);
            drop(blocker);
            std::fs::remove_dir_all(path).unwrap();
        }
    }

    #[test]
    fn dependency_transaction_cancellation_after_publication_rolls_back_manifest() {
        let path = std::env::temp_dir().join(format!(
            "lychee-capture-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        crate::workspace::initialize(&path).unwrap();
        let mut metadata = Metadata::open(&path, true).unwrap();
        metadata.put("capture/source", &true, None).unwrap();
        let cancellation = TrackedCancellation::new();
        let control = OperationControl::tracked(
            Instant::now() + Duration::from_secs(10),
            cancellation.clone(),
        );
        let error = metadata
            .put_serialized_referencing(
                "capture/owner",
                br#"{"dependencies":["source"]}"#,
                &["capture/source".into()],
                &control,
                || control.check(),
                |_| Ok(()),
                |_, _| Ok(()),
                || {
                    cancellation.cancel();
                    Ok(())
                },
            )
            .unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("command.cancelled", 7)
        );
        assert_eq!(metadata.generation("capture/owner").unwrap(), None);
        assert_eq!(metadata.generation("capture/source").unwrap(), Some(1));
        drop(metadata);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[cfg(test)]
mod strict_create_tests {
    use super::*;
    use crate::control::{OperationControl, TrackedCancellation};
    use std::time::{Duration, Instant};
    #[test]
    fn strict_create_conflicts_preserve_legacy_put_idempotence() {
        let home = std::env::temp_dir().join(format!(
            "lychee-strict-create-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        crate::workspace::initialize(&home).unwrap();
        let mut metadata = Metadata::open(&home, true).unwrap();
        let control = OperationControl::default();
        for controlled in [false, true] {
            let key = if controlled {
                "strict/controlled"
            } else {
                "strict/plain"
            };
            let create = |metadata: &mut Metadata, value: bool| {
                if controlled {
                    metadata.create_controlled(key, &value, &control)
                } else {
                    metadata.create(key, &value)
                }
            };
            create(&mut metadata, true).unwrap();
            assert_eq!(metadata.generation(key).unwrap(), Some(1));
            for value in [true, false] {
                assert_eq!(
                    create(&mut metadata, value).unwrap_err().code,
                    "vault.generation_conflict"
                );
                assert_eq!(metadata.read::<bool>(key).unwrap(), true);
                assert_eq!(metadata.generation(key).unwrap(), Some(1));
            }
            metadata.put(key, &true, None).unwrap();
            metadata.put_controlled(key, &true, None, &control).unwrap();
            assert_eq!(metadata.generation(key).unwrap(), Some(1));
        }
        let cancellation = TrackedCancellation::new();
        let cancelled = OperationControl::tracked(
            Instant::now() + Duration::from_secs(10),
            cancellation.clone(),
        );
        cancellation.cancel();
        assert_eq!(
            metadata
                .create_controlled("strict/cancelled", &true, &cancelled)
                .unwrap_err()
                .code,
            "command.cancelled"
        );
        assert_eq!(metadata.generation("strict/cancelled").unwrap(), None);
        let expired = OperationControl::uncancelable(Instant::now());
        assert_eq!(
            metadata
                .create_controlled("strict/expired", &true, &expired)
                .unwrap_err()
                .code,
            "command.deadline"
        );
        assert_eq!(metadata.generation("strict/expired").unwrap(), None);
        metadata.create("strict/recovery", &true).unwrap();
    }
    #[test]
    fn strict_controlled_create_preserves_cancellation_during_lock_wait() {
        let home = std::env::temp_dir().join(format!(
            "lychee-strict-create-lock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        crate::workspace::initialize(&home).unwrap();
        let mut metadata = Metadata::open(&home, true).unwrap();
        let blocker = Connection::open(home.join("state/toolkit.sqlite")).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
        let cancellation = TrackedCancellation::new();
        let control = OperationControl::tracked(
            Instant::now() + Duration::from_secs(10),
            cancellation.clone(),
        );
        let mut observed = false;
        let error = metadata
            .put_controlled_policy("strict/blocked", &true, None, &control, true, || {
                observed = true;
                cancellation.cancel();
            })
            .unwrap_err();
        assert!(observed);
        assert_eq!(error.code, "command.cancelled");
        blocker.execute_batch("ROLLBACK;").unwrap();
        assert_eq!(metadata.generation("strict/blocked").unwrap(), None);
        metadata
            .create_controlled("strict/blocked", &true, &OperationControl::default())
            .unwrap();
        assert_eq!(metadata.generation("strict/blocked").unwrap(), Some(1));
    }
}

#[cfg(test)]
mod historical_read_tests {
    use super::*;
    use crate::control::{OperationControl, TrackedCancellation};
    use std::time::{Duration, Instant};
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "lychee-history-read-{}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            crate::workspace::initialize(&path).unwrap();
            drop(Metadata::open(&path, true).unwrap());
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn historical_raw_read_keeps_invalid_utf8_and_exact_document_limit() {
        let f = Fixture::new();
        let metadata = Metadata::open(&f.0, true).unwrap();
        let raw = b"{\"phase\":\"\xff\"}";
        metadata
            .connection
            .execute("INSERT INTO documents VALUES('raw',1,?1)", [raw.as_slice()])
            .unwrap();
        let limit = vec![b' '; 1_048_576];
        metadata
            .connection
            .execute("INSERT INTO documents VALUES('limit',1,?1)", [&limit])
            .unwrap();
        let large = vec![b' '; 1_048_577];
        metadata
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        metadata
            .connection
            .execute("INSERT INTO documents VALUES('large',1,?1)", [&large])
            .unwrap();
        metadata
            .connection
            .execute("INSERT INTO documents VALUES('text',1,?1)", ["你好"])
            .unwrap();
        drop(metadata);
        let c = OperationControl::default();
        assert_eq!(
            Metadata::read_raw_controlled(&f.0, "text", &c).unwrap(),
            "你好".as_bytes()
        );
        assert_eq!(Metadata::read_raw_controlled(&f.0, "raw", &c).unwrap(), raw);
        assert_eq!(
            Metadata::read_raw_controlled(&f.0, "limit", &c)
                .unwrap()
                .len(),
            1_048_576
        );
        assert!(Metadata::read_raw_controlled(&f.0, "large", &c).is_err());
    }
    #[test]
    fn historical_raw_read_cancel_during_sqlite_wait_closes_without_writes() {
        let f = Fixture::new();
        let blocker = Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let cancellation = TrackedCancellation::new();
        let control = OperationControl::tracked(
            Instant::now() + Duration::from_secs(30),
            cancellation.clone(),
        );
        let mut waited = false;
        let error = Metadata::read_raw_controlled_observed(&f.0, "missing", &control, || {
            waited = true;
            cancellation.cancel();
        })
        .unwrap_err();
        assert!(waited);
        assert_eq!(error.code, "command.cancelled");
        blocker.execute_batch("ROLLBACK").unwrap();
        drop(blocker);
        let error = Metadata::read_raw_controlled(&f.0, "missing", &OperationControl::default())
            .unwrap_err();
        assert_eq!(error.message, "vault.record_missing");
    }
    #[test]
    fn historical_raw_read_expired_budget_precedes_open() {
        let f = Fixture::new();
        let path = f.0.join("must-not-create");
        let control = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
        assert_eq!(
            Metadata::read_raw_controlled(&path, "x", &control)
                .unwrap_err()
                .code,
            "command.deadline"
        );
        assert!(!path.exists());
    }
    #[test]
    fn historical_raw_read_schema_is_readonly_and_fixed_go_error() {
        for version in [0, 2, -1] {
            let f = Fixture::new();
            let connection = Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
            connection
                .pragma_update(None, "user_version", version)
                .unwrap();
            let error =
                Metadata::read_raw_controlled(&f.0, "x", &OperationControl::default()).unwrap_err();
            assert_eq!(error.code, "vault.unsupported_workspace");
            let after: i32 = connection
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(after, version);
            drop(connection);
        }
    }
}

pub mod historical_write;
pub use historical_write::{HistoricalMutation, HistoricalWriter};

mod controlled_open;

#[cfg(test)]
mod supplemental_tests;

mod raw_page;

mod capture_write;
pub(crate) use capture_write::CaptureWriteState;
mod source_controlled;

mod listfile_controlled;

#[cfg(test)]
mod generation_tests;

mod data_cache;
pub use data_cache::DataCacheMetadata;

pub(crate) mod evidence_catalog;
pub use evidence_catalog::EvidenceDomain;

pub(crate) use evidence_catalog::with_existing_writer as with_evidence_writer;
