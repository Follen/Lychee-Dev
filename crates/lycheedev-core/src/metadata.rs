use crate::{Result, ToolkitError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;

pub struct Metadata {
    connection: Connection,
}
#[derive(Debug, Clone)]
pub struct Document {
    pub key: String,
    pub generation: i64,
    pub value: serde_json::Value,
}
impl Metadata {
    pub fn open(home: &Path, write: bool) -> Result<Self> {
        crate::workspace::open(home)?;
        let path = home.join("state/toolkit.sqlite");
        if write {
            std::fs::create_dir_all(path.parent().expect("metadata parent"))?;
        }
        if !path.exists() && !write {
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
        }
        Ok(Self { connection })
    }
    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Result<T> {
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
    /// The caller owns already serialized JSON. Keep insertion, replacement and
    /// byte-identical idempotence in the same transaction as `put`.
    pub(crate) fn put_serialized(
        &mut self,
        key: &str,
        raw: &[u8],
        expected: Option<i64>,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<()> {
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
            } else if expected.is_none() && same_bytes {
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
            transaction.execute(
                "INSERT INTO documents(key,generation,value) VALUES(?1,1,?2)",
                params![key, raw],
            )?;
        }
        check()?;
        transaction.commit()?;
        Ok(())
    }
    pub fn list<T: DeserializeOwned>(&self, prefix: &str, limit: usize) -> Result<Vec<T>> {
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
        Ok(self
            .connection
            .execute("DELETE FROM documents WHERE key=?1", [key])?
            != 0)
    }
    /// Validate and consume one document in the same write transaction.
    /// Unrelated workspace documents are not loaded for a single-use intent.
    pub fn consume<T: DeserializeOwned, U>(
        &mut self,
        key: &str,
        validate: impl FnOnce(T) -> Result<U>,
    ) -> Result<U> {
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let raw: Option<Vec<u8>> = transaction
            .query_row("SELECT value FROM documents WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        let raw = raw.ok_or_else(|| {
            ToolkitError::new(
                "vault.record_missing",
                format!("document {key} does not exist"),
                3,
            )
        })?;
        let result = validate(serde_json::from_slice(&raw)?)?;
        transaction.execute("DELETE FROM documents WHERE key=?1", [key])?;
        transaction.commit()?;
        Ok(result)
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
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (count, bytes): (i64, i64) = transaction.query_row(
            "SELECT count(*),coalesce(sum(length(value)),0) FROM documents",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if count > 100000 || bytes > 64 << 20 {
            return Err(ToolkitError::new(
                "evidence.reference_scan_limit",
                "reference scan exceeds 100000 documents or 64 MiB",
                4,
            ));
        }
        let mut documents = vec![];
        {
            let mut statement =
                transaction.prepare("SELECT key,generation,value FROM documents ORDER BY key")?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })?;
            for row in rows {
                let (key, generation, raw) = row?;
                documents.push(Document {
                    key,
                    generation,
                    value: serde_json::from_slice(&raw)?,
                });
            }
        }
        for (key, generation) in expected {
            if documents
                .iter()
                .find(|doc| &doc.key == key)
                .is_none_or(|doc| doc.generation != *generation)
            {
                return Err(ToolkitError::new(
                    "vault.generation_conflict",
                    "document changed before checked deletion",
                    4,
                ));
            }
        }
        let (value, recovery) = validate(&documents)?;
        if let Some((key, record)) = recovery {
            let raw = serde_json::to_vec(&record)?;
            if raw.len() > 1_048_576 {
                return Err(ToolkitError::invalid("recovery document exceeds 1 MiB"));
            }
            transaction.execute(
                "INSERT INTO documents(key,generation,value) VALUES(?1,1,?2)",
                params![key, raw],
            )?;
        }
        for key in keys {
            transaction.execute("DELETE FROM documents WHERE key=?1", [key])?;
        }
        transaction.commit()?;
        Ok(value)
    }
}
