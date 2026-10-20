//! Legacy checked-deletion API with one owning transaction and a catalog-wide
//! read view. Cross-store writes are rejected before the first mutation.
use super::*;

pub(crate) fn run<T>(
    home: &Path,
    keys: &[String],
    expected: &[(String, i64)],
    control: &OperationControl,
    validate: impl FnOnce(
        &[Document],
        &TransactionReader<'_>,
    ) -> Result<(T, Option<(String, serde_json::Value)>)>,
) -> Result<T> {
    with_catalog(
        home,
        Some(EvidenceDomain::Data),
        control,
        &|| Ok(()),
        |catalog, owner| {
            let mut destination = None;
            for key in keys {
                let selected = catalog
                    .owner(key, control, &|| Ok(()))?
                    .unwrap_or(EvidenceDomain::Historical);
                if destination.is_some_and(|previous| previous != selected) {
                    return Err(ToolkitError::invalid(
                        "checked deletion cannot mutate two evidence stores",
                    ));
                }
                destination = Some(selected);
            }
            let destination = destination.unwrap_or(EvidenceDomain::Historical);
            if destination == EvidenceDomain::Data {
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
            let metadata = match destination {
                EvidenceDomain::Data => catalog.data.as_ref(),
                EvidenceDomain::Historical => catalog.historical.as_ref(),
            }
            .ok_or_else(integrity)?;
            let connection = &metadata.connection;
            let mut transaction = checked_sql(control, control.deadline(), &mut || {}, || {
                rusqlite::Transaction::new_unchecked(
                    connection,
                    rusqlite::TransactionBehavior::Immediate,
                )
            })?;
            let result = (|| {
                let mut documents = Vec::new();
                let mut bytes = 0usize;
                let mut accept = |key: &str, generation, raw: &[u8]| -> Result<()> {
                    control.check()?;
                    bytes = bytes.checked_add(raw.len()).ok_or_else(integrity)?;
                    if documents.len() >= 100_000 || bytes > 64 << 20 {
                        return Err(ToolkitError::new(
                            "evidence.reference_scan_limit",
                            "reference scan exceeds 100000 documents or 64 MiB",
                            4,
                        ));
                    }
                    documents.push(Document {
                        key: key.into(),
                        generation,
                        value: serde_json::from_slice(raw)?,
                    });
                    Ok(())
                };
                // Preserve the old API's complete ordered snapshot, including a
                // historical empty key if a legacy writer created one.
                if let Some(row) = catalog.document("", control, &|| Ok(()))? {
                    accept("", row.generation, &row.bytes)?;
                }
                let mut after = String::new();
                loop {
                    let (count, last) =
                        catalog.visit("", &after, 1000, control, &|| Ok(()), &mut accept)?;
                    if count < 1000 {
                        break;
                    }
                    after = last;
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
                let reader = TransactionReader {
                    connection: &transaction,
                    control,
                    catalog: Some(catalog),
                };
                let (value, recovery) = validate(&documents, &reader)?;
                control.check()?;
                let recovery = recovery
                    .map(|(key, record)| -> Result<_> {
                        let selected =
                            catalog
                                .owner(&key, control, &|| Ok(()))?
                                .unwrap_or_else(|| {
                                    if EvidenceKey::parse(&key).is_some() {
                                        destination
                                    } else {
                                        EvidenceDomain::Historical
                                    }
                                });
                        if selected != destination {
                            return Err(ToolkitError::invalid(
                                "checked recovery cannot mutate another evidence store",
                            ));
                        }
                        let raw = serde_json::to_vec(&record)?;
                        if raw.len() > 1_048_576 {
                            return Err(ToolkitError::invalid("recovery document exceeds 1 MiB"));
                        }
                        Ok((key, raw))
                    })
                    .transpose()?;
                // Every cross-store check completed before either write.
                if let Some((key, raw)) = recovery {
                    checked_sql(control, control.deadline(), &mut || {}, || {
                        transaction.execute(INSERT_DOCUMENT, params![key, raw])
                    })?;
                }
                for key in keys {
                    checked_sql(control, control.deadline(), &mut || {}, || {
                        remember_deleted_generation(&transaction, key)
                    })?;
                    checked_sql(control, control.deadline(), &mut || {}, || {
                        transaction.execute("DELETE FROM documents WHERE key=?1", [key])
                    })?;
                }
                checked_sql(control, control.deadline(), &mut || {}, || {
                    transaction.execute_batch("COMMIT")
                })?;
                Ok(value)
            })();
            let rollback = if transaction.is_autocommit() {
                Ok(())
            } else {
                transaction.execute_batch("ROLLBACK").map_err(Into::into)
            };
            transaction.set_drop_behavior(rusqlite::DropBehavior::Ignore);
            crate::selection::selection_finish(finish_historical(result, rollback), control.check())
        },
    )
}
