//! Fixture-only cache access. Reads and writes exercise production admission.
//! Enumeration and deliberate loss injectors never escape test binaries.
#![allow(dead_code)]
use lycheedev_core::{Result, control::OperationControl, metadata::DataCacheMetadata};
use serde::{Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};
pub struct Cache(PathBuf);
impl Cache {
    pub fn new(home: &Path) -> Self {
        Self(home.to_owned())
    }
    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Result<T> {
        DataCacheMetadata::read_controlled_with_check(
            &self.0,
            key,
            &OperationControl::default(),
            || Ok(()),
        )
    }
    pub fn generation(&self, key: &str) -> Result<Option<i64>> {
        match DataCacheMetadata::read_document_raw_controlled_with_check(
            &self.0,
            key,
            &OperationControl::default(),
            || Ok(()),
        ) {
            Ok(raw) => Ok(Some(raw.generation)),
            Err(error) if error.code == "vault.record_missing" && error.details.is_null() => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
    pub fn put<T: Serialize>(&self, key: &str, value: &T, expected: Option<i64>) -> Result<()> {
        DataCacheMetadata::put_controlled_with_check(
            &self.0,
            key,
            value,
            expected,
            &OperationControl::default(),
            || Ok(()),
        )
    }
    pub fn list<T: DeserializeOwned>(&self, prefix: &str, maximum: usize) -> Result<Vec<T>> {
        let path = self.0.join("state/data/cache-v1.sqlite");
        if !path.try_exists()? {
            return Ok(Vec::new());
        }
        let connection = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let result = (|| {
            let mut stmt = connection.prepare(
                "SELECT value FROM documents WHERE substr(key,1,?1)=?2 ORDER BY key LIMIT ?3",
            )?;
            let rows = stmt.query_map(rusqlite::params![prefix.len(), prefix, maximum], |r| {
                r.get::<_, Vec<u8>>(0)
            })?;
            rows.map(|row| Ok(serde_json::from_slice(&row?)?)).collect()
        })();
        let close = connection.close().map_err(|(_, error)| error.into());
        lycheedev_core::metadata::owned_write::finish_owned(result, close)
    }
    pub fn delete(&self, key: &str) -> Result<bool> {
        let connection = rusqlite::Connection::open(self.0.join("state/data/cache-v1.sqlite"))?;
        let result = connection
            .execute("DELETE FROM documents WHERE key=?1", [key])
            .map(|count| count == 1)
            .map_err(Into::into);
        let close = connection.close().map_err(|(_, error)| error.into());
        lycheedev_core::metadata::owned_write::finish_owned(result, close)
    }
}
