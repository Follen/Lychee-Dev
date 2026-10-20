//! Unified authoritative evidence catalog. Data records have their own measured
//! database; historical records remain protected in their original store.
//! This module is activated only together with all consumers and writer gates.
use super::*;
use crate::{
    control::OperationControl, storage::ExclusiveLock, workspace::data_disk::EvidenceQuota,
};
use historical_write::finish_historical;
mod checked_delete;
mod historical_anchor;
mod keys;
pub(crate) use checked_delete::run as checked_delete;
mod store;
pub(crate) use keys::{EvidenceKey, intersects};

pub(crate) const GATE: &str = "evidence-catalog-v2:writer";
fn integrity() -> ToolkitError {
    ToolkitError::new(
        "evidence.catalog_integrity",
        "Evidence store identity, ownership or duplicate records differ; existing evidence retained",
        4,
    )
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceDomain {
    Historical,
    Data,
}

pub(crate) struct Catalog {
    historical: Option<Metadata>,
    data: Option<Metadata>,
    historical_anchor: Option<historical_anchor::Anchor>,
}
impl Catalog {
    fn close(self) -> Result<()> {
        let first = self.data.map_or(Ok(()), Metadata::close);
        let first = finish_historical(first, self.historical.map_or(Ok(()), Metadata::close));
        finish_historical(
            first,
            self.historical_anchor
                .map_or(Ok(()), historical_anchor::Anchor::close),
        )
    }
    fn open_historical(
        &mut self,
        home: &Path,
        write: bool,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Metadata> {
        if write {
            self.historical_anchor
                .as_mut()
                .ok_or_else(integrity)?
                .ensure_main(control, check)?;
        }
        Metadata::open_controlled_with_check(home, write, control, || check())
    }
    fn has_history(
        metadata: &Metadata,
        key: &str,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<bool> {
        checked_sql_with_check(
            control,
            control.deadline(),
            &mut || check(),
            &mut || {},
            || {
                let exists: bool = metadata.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='deleted_generations' AND type='table')", [], |row| row.get(0))?;
                if !exists {
                    return Ok(false);
                }
                metadata.connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM deleted_generations WHERE key=?1)",
                    [key],
                    |row| row.get(0),
                )
            },
        )
    }
    /// Live rows and generation history both retain their original owner.
    /// Metadata::delete need not have created an evidence removal receipt.
    fn owner(
        &self,
        key: &str,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Option<EvidenceDomain>> {
        let Some(reserved) = EvidenceKey::parse(key) else {
            return Ok(Some(EvidenceDomain::Historical));
        };
        let id = reserved.capture_id();
        let keys = [
            key.to_owned(),
            format!("capture/{id}"),
            format!("evidence/keep/{id}"),
            format!("evidence/remove/{id}"),
        ];
        let owns = |metadata: &Metadata| -> Result<bool> {
            for candidate in &keys {
                if Self::length_in(metadata, candidate, control, check)?.is_some()
                    || Self::has_history(metadata, candidate, control, check)?
                {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        let old = self
            .historical
            .as_ref()
            .map(&owns)
            .transpose()?
            .unwrap_or(false);
        let data = self.data.as_ref().map(&owns).transpose()?.unwrap_or(false);
        match (old, data) {
            (true, true) => Err(integrity()),
            (true, false) => Ok(Some(EvidenceDomain::Historical)),
            (false, true) => Ok(Some(EvidenceDomain::Data)),
            (false, false) => Ok(None),
        }
    }
    pub(crate) fn visit_historical(
        &self,
        prefix: &str,
        after: &str,
        control: &OperationControl,
        visit: impl FnMut(&str, i64, &[u8]) -> Result<()>,
    ) -> Result<(usize, String)> {
        match &self.historical {
            Some(metadata) => super::raw_page::visit_raw_page(
                &metadata.connection,
                prefix,
                after,
                1000,
                control,
                std::time::Duration::ZERO,
                || Ok(()),
                visit,
            ),
            None => Ok((0, after.to_owned())),
        }
    }
    fn raw_in(
        metadata: &Metadata,
        key: &str,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Option<RawDocument>> {
        control.check()?;
        check()?;
        let row: Option<(i64, i64, Option<Vec<u8>>)> = checked_sql_with_check(
            control,
            control.deadline(),
            &mut || check(),
            &mut || {},
            || {
                metadata.connection.query_row("SELECT generation,length(CAST(value AS BLOB)),CASE WHEN length(CAST(value AS BLOB))<=1048576 THEN CAST(value AS BLOB) ELSE NULL END FROM documents WHERE key=?1", [key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional()
            },
        )?;
        control.check()?;
        check()?;
        match row {
            None => Ok(None),
            Some((generation, length, Some(bytes)))
                if generation > 0
                    && (0..=1_048_576).contains(&length)
                    && length as usize == bytes.len() =>
            {
                Ok(Some(RawDocument { generation, bytes }))
            }
            Some(_) => Err(integrity()),
        }
    }
    pub(crate) fn document(
        &self,
        key: &str,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Option<RawDocument>> {
        let old = self
            .historical
            .as_ref()
            .map(|metadata| Self::raw_in(metadata, key, control, check))
            .transpose()?
            .flatten();
        let new = if EvidenceKey::parse(key).is_some() {
            self.data
                .as_ref()
                .map(|metadata| Self::raw_in(metadata, key, control, check))
                .transpose()?
                .flatten()
        } else {
            None
        };
        match (old, new) {
            (Some(old), Some(new)) => {
                if old.generation != new.generation || old.bytes != new.bytes {
                    return Err(integrity());
                }
                Ok(Some(old))
            }
            (old, new) => Ok(old.or(new)),
        }
    }
}

/// One scope owns all connections and closes them before the gate and quota.
/// No caller can escape a writable connection or reacquire quota in `run`.
pub(crate) fn with_catalog<T>(
    home: &Path,
    domain: Option<EvidenceDomain>,
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
    run: impl FnOnce(&mut Catalog, &mut EvidenceQuota<'_>) -> Result<T>,
) -> Result<T> {
    let mut owner = if domain.is_some() {
        EvidenceQuota::acquire(home, control, check)?
    } else {
        EvidenceQuota::acquire_read(home, control, check)?
    };
    let result = (|| {
        let gate = ExclusiveLock::resource_wait_checked(
            &home.join("locks"),
            GATE,
            Some(control.deadline()),
            || {
                control.check()?;
                check()
            },
        )?;
        let mut catalog = Catalog {
            historical: None,
            data: None,
            historical_anchor: None,
        };
        let result = (|| {
            catalog.data = store::open(home, &mut owner, false, control, check)?;
            // Native absence is the only historical-store miss. No corruption,
            // permission, cancellation or ambiguous SQLite error is swallowed.
            let anchor = historical_anchor::Anchor::open(home, control, check)?;
            let old_present = anchor.present();
            catalog.historical_anchor = Some(anchor);
            if old_present || domain == Some(EvidenceDomain::Historical) {
                catalog.historical = Some(catalog.open_historical(
                    home,
                    domain == Some(EvidenceDomain::Historical),
                    control,
                    check,
                )?);
            }
            if let Some(metadata) = &mut catalog.historical {
                metadata.evidence_gate_held = true;
            }
            run(&mut catalog, &mut owner)
        })();
        let result = finish_historical(result, catalog.close());
        finish_historical(result, gate.close())
    })();
    owner.finish(result)
}

pub(crate) fn read(
    home: &Path,
    key: &str,
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
) -> Result<Option<RawDocument>> {
    with_catalog(home, None, control, check, |catalog, _| {
        catalog.document(key, control, check)
    })
}

impl Catalog {
    fn length_in(
        metadata: &Metadata,
        key: &str,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Option<usize>> {
        checked_sql_with_check(
            control,
            control.deadline(),
            &mut || check(),
            &mut || {},
            || {
                metadata
                    .connection
                    .query_row(
                        "SELECT length(CAST(value AS BLOB)) FROM documents WHERE key=?1",
                        [key],
                        |row| row.get(0),
                    )
                    .optional()
            },
        )
    }
    /// Dependency bytes are admitted BEFORE either store materializes a BLOB.
    pub(crate) fn admitted_document(
        &self,
        key: &str,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
        admit: &mut impl FnMut(usize) -> Result<()>,
    ) -> Result<Option<RawDocument>> {
        let old = self
            .historical
            .as_ref()
            .map(|metadata| Self::length_in(metadata, key, control, check))
            .transpose()?
            .flatten();
        let new = self
            .data
            .as_ref()
            .map(|metadata| Self::length_in(metadata, key, control, check))
            .transpose()?
            .flatten();
        if old.is_some_and(|n| n > 1_048_576) || new.is_some_and(|n| n > 1_048_576) {
            return Err(integrity());
        }
        let length = old
            .unwrap_or(0)
            .checked_add(new.unwrap_or(0))
            .ok_or_else(integrity)?;
        if old.is_some() || new.is_some() {
            admit(length)?;
        }
        self.document(key, control, check)
    }
    fn next_key(
        metadata: &Metadata,
        prefix: &str,
        after: &str,
        data: bool,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Option<String>> {
        let sql = if data {
            "SELECT key FROM documents WHERE substr(key,1,length(?1))=?1 AND key>?2 AND (substr(key,1,8)='capture/' OR substr(key,1,14)='evidence/keep/' OR substr(key,1,16)='evidence/remove/') ORDER BY key LIMIT 1"
        } else {
            "SELECT key FROM documents WHERE substr(key,1,length(?1))=?1 AND key>?2 ORDER BY key LIMIT 1"
        };
        checked_sql_with_check(
            control,
            control.deadline(),
            &mut || check(),
            &mut || {},
            || {
                metadata
                    .connection
                    .query_row(sql, params![prefix, after], |row| row.get(0))
                    .optional()
            },
        )
    }
    pub(crate) fn visit(
        &self,
        prefix: &str,
        after: &str,
        limit: usize,
        control: &OperationControl,
        check: &dyn Fn() -> Result<()>,
        visit: &mut impl FnMut(&str, i64, &[u8]) -> Result<()>,
    ) -> Result<(usize, String)> {
        if !(1..=1000).contains(&limit) {
            return Err(ToolkitError::invalid("vault: invalid document page"));
        }
        let mut last = after.to_owned();
        let mut count = 0;
        while count < limit {
            control.check()?;
            check()?;
            let old = self
                .historical
                .as_ref()
                .map(|metadata| Self::next_key(metadata, prefix, &last, false, control, check))
                .transpose()?
                .flatten();
            let new = self
                .data
                .as_ref()
                .map(|metadata| Self::next_key(metadata, prefix, &last, true, control, check))
                .transpose()?
                .flatten();
            let next = match (old, new) {
                (Some(old), Some(new)) => old.min(new),
                (Some(key), None) | (None, Some(key)) => key,
                (None, None) => break,
            };
            let row = self
                .document(&next, control, check)?
                .ok_or_else(integrity)?;
            visit(&next, row.generation, &row.bytes)?;
            last = next;
            count += 1;
        }
        control.check()?;
        check()?;
        Ok((count, last))
    }
}

pub(crate) fn visit(
    home: &Path,
    prefix: &str,
    after: &str,
    limit: usize,
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
    mut visit: impl FnMut(&str, i64, &[u8]) -> Result<()>,
) -> Result<(usize, String)> {
    with_catalog(home, None, control, check, |catalog, _| {
        catalog.visit(prefix, after, limit, control, check, &mut visit)
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn put_capture(
    home: &Path,
    domain: EvidenceDomain,
    key: &str,
    raw: &[u8],
    references: &[String],
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
    mut admit_reference: impl FnMut(usize) -> Result<()>,
    mut validate_reference: impl FnMut(&str, &[u8]) -> Result<()>,
    publish: impl FnOnce(&mut EvidenceQuota<'_>) -> Result<()>,
    state: &std::cell::Cell<CaptureWriteState>,
) -> Result<()> {
    state.set(CaptureWriteState::NotCommitted);
    if !matches!(EvidenceKey::parse(key), Some(EvidenceKey::Capture(id)) if !id.is_empty())
        || raw.len() > 1_048_576
    {
        return Err(integrity());
    }
    with_catalog(home, Some(domain), control, check, |catalog, owner| {
        for reference in references {
            let row = catalog
                .admitted_document(reference, control, check, &mut admit_reference)?
                .ok_or_else(|| {
                    ToolkitError::new(
                        "vault.record_missing",
                        format!("document {reference} does not exist"),
                        3,
                    )
                })?;
            validate_reference(reference, &row.bytes)?;
        }
        // Never create a second copy of a historical CAP merely because its
        // producer is now Data-aware. Existing identity retains its owner.
        let historical = catalog
            .historical
            .as_ref()
            .map(|metadata| Catalog::length_in(metadata, key, control, check))
            .transpose()?
            .flatten()
            .is_some();
        let data = catalog
            .data
            .as_ref()
            .map(|metadata| Catalog::length_in(metadata, key, control, check))
            .transpose()?
            .flatten()
            .is_some();
        if historical && data {
            return Err(integrity());
        }
        publish(owner)?;
        if historical || data {
            let row = catalog
                .document(key, control, check)?
                .ok_or_else(integrity)?;
            if row.bytes != raw {
                return Err(ToolkitError::new(
                    "vault.generation_conflict",
                    "immutable document already differs",
                    4,
                ));
            }
            state.set(CaptureWriteState::Committed);
            control.check()?;
            return check();
        }
        let destination = catalog.owner(key, control, check)?.unwrap_or(domain);
        if destination == EvidenceDomain::Data {
            if catalog.data.is_none() {
                catalog.data = store::open(home, owner, true, control, check)?;
            }
            store::admit_mutation(
                catalog.data.as_mut().ok_or_else(integrity)?,
                owner,
                control,
                check,
            )?;
        } else if catalog
            .historical
            .as_ref()
            .is_none_or(|metadata| !metadata.writable)
        {
            if let Some(metadata) = catalog.historical.take() {
                metadata.close()?;
            }
            let mut metadata = catalog.open_historical(home, true, control, check)?;
            metadata.evidence_gate_held = true;
            catalog.historical = Some(metadata);
        }
        let metadata = match destination {
            EvidenceDomain::Historical => catalog.historical.as_mut(),
            EvidenceDomain::Data => catalog.data.as_mut(),
        }
        .ok_or_else(integrity)?;
        // All dependencies were authenticated under the universal gate. This
        // local transaction writes only its own authoritative store.
        metadata.put_capture_serialized(
            key,
            raw,
            &[],
            control,
            || check(),
            |_| Ok(()),
            |_, _| Ok(()),
            || Ok(()),
            state,
        )
    })
}

pub(crate) fn present(home: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(home.join("state/data").join(store::NAME)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
pub(crate) fn required(row: Option<RawDocument>) -> Result<RawDocument> {
    row.ok_or_else(|| ToolkitError::new("vault.record_missing", "vault.record_missing", 3))
}

pub(crate) fn acquire_gate(
    home: &Path,
    control: &OperationControl,
    mut check: impl FnMut() -> Result<()>,
) -> Result<ExclusiveLock> {
    ExclusiveLock::resource_wait_checked(
        &home.join("locks"),
        GATE,
        Some(control.deadline()),
        || {
            control.check()?;
            check()
        },
    )
}

/// Keep and removal bind mutations, tombstones and recovery receipt to the
/// CAP's existing physical owner; reference reads still merge both stores.
pub(crate) fn with_existing_writer<T>(
    home: &Path,
    id: &str,
    control: &OperationControl,
    run: impl FnOnce(&HistoricalWriter<'_>) -> Result<T>,
) -> Result<T> {
    with_catalog(
        home,
        Some(EvidenceDomain::Data),
        control,
        &|| Ok(()),
        |catalog, owner| {
            let capture = format!("capture/{id}");
            let removal = format!("evidence/remove/{id}");
            let owns = |metadata: &Metadata| -> Result<bool> {
                Ok(
                    Catalog::length_in(metadata, &capture, control, &|| Ok(()))?.is_some()
                        || Catalog::length_in(metadata, &removal, control, &|| Ok(()))?.is_some(),
                )
            };
            let old = catalog
                .historical
                .as_ref()
                .map(&owns)
                .transpose()?
                .unwrap_or(false);
            let data = catalog
                .data
                .as_ref()
                .map(&owns)
                .transpose()?
                .unwrap_or(false);
            if old && data {
                return Err(integrity());
            }
            if !old && !data {
                return Err(ToolkitError::new(
                    "vault.record_missing",
                    "vault.record_missing",
                    2,
                ));
            }
            if !data {
                if let Some(metadata) = catalog.historical.take() {
                    metadata.close()?;
                }
                let mut metadata = catalog.open_historical(home, true, control, &|| Ok(()))?;
                metadata.evidence_gate_held = true;
                catalog.historical = Some(metadata);
            }
            let connection = if data {
                &catalog.data.as_ref().ok_or_else(integrity)?.connection
            } else {
                &catalog
                    .historical
                    .as_ref()
                    .ok_or_else(integrity)?
                    .connection
            };
            // Pure retries of completed removal/keep facts must remain readable
            // after a limit reduction. Admit only immediately before a real
            // owning-store transaction; the quota lease is already held.
            let quota = std::cell::RefCell::new(owner);
            let admit = || {
                if data {
                    store::admit_connection(
                        catalog.data.as_ref().ok_or_else(integrity)?,
                        &mut **quota.borrow_mut(),
                        control,
                        &|| Ok(()),
                    )
                } else {
                    Ok(())
                }
            };
            let writer =
                HistoricalWriter::for_catalog(connection, home, control, catalog, Some(&admit));
            run(&writer)
        },
    )
}

/// Generic metadata APIs keep their CAS semantics but cannot accidentally write
/// a second owner for a reserved key. The held capability prevents re-entry.
pub(crate) fn mutate<T>(
    home: &Path,
    key: &str,
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
    run: impl FnOnce(&mut Metadata) -> Result<T>,
) -> Result<T> {
    EvidenceKey::parse(key).ok_or_else(integrity)?;
    with_catalog(
        home,
        Some(EvidenceDomain::Data),
        control,
        check,
        |catalog, owner| {
            let data = catalog.owner(key, control, check)? == Some(EvidenceDomain::Data);
            if data {
                store::admit_mutation(
                    catalog.data.as_mut().ok_or_else(integrity)?,
                    owner,
                    control,
                    check,
                )?;
                let metadata = catalog.data.as_mut().ok_or_else(integrity)?;
                metadata.evidence_gate_held = true;
                run(metadata)
            } else {
                if let Some(metadata) = catalog.historical.take() {
                    metadata.close()?;
                }
                let mut metadata = catalog.open_historical(home, true, control, check)?;
                metadata.evidence_gate_held = true;
                catalog.historical = Some(metadata);
                run(catalog.historical.as_mut().ok_or_else(integrity)?)
            }
        },
    )
}

/// A read-only handle when a workspace contains new Data evidence but has
/// never had a historical database. It does not create an unbudgeted historical
/// SQLite file. Reserved reads/visitors resolve through this catalog; ordinary
/// historical keys correctly remain absent. query_only prevents false writes.
pub(crate) fn read_carrier(
    home: &Path,
    control: &OperationControl,
    check: &dyn Fn() -> Result<()>,
) -> Result<Metadata> {
    match std::fs::symlink_metadata(home.join("state/toolkit.sqlite")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => return Err(integrity()),
        Err(error) => return Err(error.into()),
    }
    with_catalog(home, None, control, check, |_, _| Ok(()))?;
    let connection = Connection::open_in_memory()?;
    let result = (|| {
        control.check()?;
        check()?;
        connection.execute_batch("CREATE TABLE documents (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID; PRAGMA user_version=1; PRAGMA query_only=ON;")?;
        control.check()?;
        check()?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(Metadata {
            connection,
            home: std::path::absolute(home)?,
            writable: false,
            evidence_gate_held: false,
            catalog_only: true,
        }),
        Err(error) => finish_historical(
            Err(error),
            connection.close().map_err(|(_, error)| error.into()),
        ),
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn with_batch_writer<T>(
    home: &Path,
    keys: &[&str],
    control: &OperationControl,
    run: impl FnOnce(&HistoricalWriter<'_>) -> Result<T>,
) -> Result<T> {
    with_catalog(
        home,
        Some(EvidenceDomain::Data),
        control,
        &|| Ok(()),
        |catalog, owner| {
            let mut selected = None;
            for key in keys {
                let data = catalog.owner(key, control, &|| Ok(()))? == Some(EvidenceDomain::Data);
                if selected.is_some_and(|previous| previous != data) {
                    return Err(ToolkitError::invalid(
                        "one metadata mutation cannot span evidence stores",
                    ));
                }
                selected = Some(data);
            }
            let data = selected.unwrap_or(false);
            if data {
                store::admit_mutation(
                    catalog.data.as_mut().ok_or_else(integrity)?,
                    owner,
                    control,
                    &|| Ok(()),
                )?;
            } else {
                if let Some(metadata) = catalog.historical.take() {
                    metadata.close()?;
                }
                let mut metadata = catalog.open_historical(home, true, control, &|| Ok(()))?;
                metadata.evidence_gate_held = true;
                catalog.historical = Some(metadata);
            }
            let connection = if data {
                &catalog.data.as_ref().ok_or_else(integrity)?.connection
            } else {
                &catalog
                    .historical
                    .as_ref()
                    .ok_or_else(integrity)?
                    .connection
            };
            let writer = HistoricalWriter::for_catalog(connection, home, control, catalog, None);
            run(&writer)
        },
    )
}
