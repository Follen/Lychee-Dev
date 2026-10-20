//! Manifest-only discovery uses the same authenticated resource loader and
//! checked workspace teardown as table preparation, without importing rows.
use super::api::{CatalogTable, DatasetCatalog};
use crate::{definitions, fault, remote::provider::OpenedScope, resource::Budget};
use lycheedev_core::{Invocation, Result, selection::PinnedSet};
use std::sync::Arc;

pub(super) fn read(inv: &Invocation, budget: &Arc<Budget>) -> Result<(DatasetCatalog, PinnedSet)> {
    inv.control.check()?;
    // The manifest has at most 16,384 names of at most 128 bytes. This allowance
    // covers the decoded inventory, conversion overlap, and serialized owner.
    // Loader transient parsing remains independently charged before its reads.
    let allocation = budget.reserve_retained(8 << 20)?.into_owned(budget)?;
    let (scope, pin) = OpenedScope::open(inv, budget)?.admit_pinned(fault(
        "records.data_pin_required",
        4,
        "Catalog requires an exact data pin",
    ))?;
    let result = (|| {
        let data = pin
            .data
            .as_ref()
            .ok_or_else(|| fault("records.data_pin_required", 4, "Missing data pin"))?;
        let (identities, receipt) = definitions::catalog_controlled(
            &inv.home,
            &data.definition_commit,
            inv.flag_bool("--offline"),
            budget,
            &inv.control,
        )?;
        let mut tables = Vec::with_capacity(identities.len());
        for identity in identities {
            budget.check()?;
            tables.push(CatalogTable {
                name: identity.name,
                table_hash: identity.hash,
                db2_file_data_id: identity.db2_file_data_id,
                dbc_file_data_id: identity.dbc_file_data_id,
                availability: "not_checked",
            });
        }
        scope.check()?;
        Ok((
            DatasetCatalog {
                provenance: receipt,
                snapshot_id: pin.id.clone(),
                definition_commit: data.definition_commit.clone(),
                scope: "exact_definition_manifest_not_installed_tables",
                complete: true,
                tables,
                _allocation: Some(allocation),
                _lifetime: None,
            },
            pin,
        ))
    })();
    scope.finish(result)
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::fixture;
    use super::*;
    use crate::engine::{Engine, EngineLimits};

    #[tokio::test(flavor = "current_thread")]
    async fn catalog_uses_exact_manifest_without_importing_table_rows() {
        let table = fixture::table(
            "Sample",
            921,
            &[("ID", "uint")],
            &[vec![fixture::Cell::U(1)]],
        );
        let fx = fixture::Fixture::new(&[table]);
        let invocation = fx.invocation("data agent serve", &[]);
        let control = invocation.control.clone();
        let mut engine = Engine::new(invocation, EngineLimits::default()).unwrap();
        let catalog = engine.catalog(control).await.unwrap();
        assert_eq!(catalog.snapshot_id, fx.pin);
        assert!(catalog.complete);
        assert_eq!(
            catalog.scope,
            "exact_definition_manifest_not_installed_tables"
        );
        assert_eq!(catalog.tables.len(), 1);
        assert_eq!(catalog.tables[0].name, "Sample");
        assert_eq!(catalog.tables[0].availability, "not_checked");
        assert_eq!(engine.cached_source_count(), 0);
        let encoded = serde_json::to_value(&catalog).unwrap();
        assert!(encoded.get("_allocation").is_none());
        drop(catalog);
        engine.close().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn catalog_cancel_does_not_poison_following_request() {
        let table = fixture::table(
            "Sample",
            921,
            &[("ID", "uint")],
            &[vec![fixture::Cell::U(1)]],
        );
        let fx = fixture::Fixture::new(&[table]);
        let invocation = fx.invocation("data agent serve", &[]);
        let control = invocation.control.clone();
        let mut engine = Engine::new(invocation, EngineLimits::default()).unwrap();
        let cancelled = lycheedev_core::control::OperationControl::new(
            control.deadline(),
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
        );
        assert!(engine.catalog(cancelled).await.is_err());
        let catalog = engine.catalog(control).await.unwrap();
        assert_eq!(catalog.tables.len(), 1);
        drop(catalog);
        engine.close().unwrap();
    }
    #[test]
    fn catalog_owner_releases_session_admission_after_serialization() {
        let memory = super::super::lifetime::SessionMemory::new(16384).unwrap();
        let lifetime = memory
            .begin(10000, lycheedev_core::control::OperationControl::default())
            .unwrap();
        let allocation = lifetime
            .budget
            .reserve_retained(4096)
            .unwrap()
            .into_owned(&lifetime.budget)
            .unwrap();
        let catalog = DatasetCatalog {
            provenance: serde_json::Value::Null,
            snapshot_id: "pin".into(),
            definition_commit: "c".repeat(40),
            scope: "exact_definition_manifest_not_installed_tables",
            complete: true,
            tables: vec![CatalogTable {
                name: "Sample".into(),
                table_hash: 1,
                db2_file_data_id: 921,
                dbc_file_data_id: 0,
                availability: "not_checked",
            }],
            _allocation: Some(allocation),
            _lifetime: Some(lifetime.clone()),
        };
        lifetime.settle().unwrap();
        assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, 4096);
        serde_json::to_writer(std::io::sink(), &catalog).unwrap();
        assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, 4096);
        drop(catalog);
        assert_eq!(memory.budget.snapshot().unwrap().retained_bytes, 0);
    }
}
