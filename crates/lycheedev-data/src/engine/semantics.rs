//! Pinned semantic enum/flag metadata, not inferred from column names.
use super::{
    api::{SourceMode, TableRequest},
    lifetime::RequestLifetime,
};
use crate::request::DataContext;
use crate::{
    definitions,
    resource::{Budget, OwnedRetainedReservation},
};
use lycheedev_core::{Result, selection::PinnedSet};
use std::sync::Arc;
#[derive(serde::Serialize)]
pub struct OwnedSemantics {
    table: TableRequest,
    columns: Vec<String>,
    rows: Vec<Vec<crate::query_value::Scalar>>,
    provenance: Option<definitions::semantics::SemanticBundle>,
    complete: bool,
    #[serde(skip)]
    allocation: Option<OwnedRetainedReservation>,
    #[serde(skip)]
    lifetime: Arc<RequestLifetime>,
}
impl Drop for OwnedSemantics {
    fn drop(&mut self) {
        drop(std::mem::take(&mut self.columns));
        drop(std::mem::take(&mut self.rows));
        drop(self.provenance.take());
        drop(std::mem::take(&mut self.table.name));
        drop(std::mem::take(&mut self.table.hotfix_captures));
        self.allocation.take();
        let _ = self.lifetime.settle();
    }
}
pub(super) fn read(
    inv: &DataContext,
    table: TableRequest,
    lifetime: Arc<RequestLifetime>,
    maximum: usize,
) -> Result<(OwnedSemantics, PinnedSet)> {
    let budget: &Arc<Budget> = &lifetime.budget;
    budget.check()?;
    if table.mode != SourceMode::Static || !table.hotfix_captures.is_empty() {
        return Err(crate::fault(
            "data.semantics.source_mode",
            2,
            "Semantic metadata requires a static table without Hotfix captures",
        ));
    }
    let allocation = budget.reserve_retained(maximum)?.into_owned(budget)?;
    let (scope, pin) =
        crate::remote::provider::OpenedScope::open(inv, budget)?.admit_pinned(crate::fault(
            "records.data_pin_required",
            4,
            "Semantic metadata requires an exact data pin",
        ))?;
    let result = (|| {
        let data = pin
            .data
            .as_ref()
            .ok_or_else(|| crate::fault("records.data_pin_required", 4, "Missing data pin"))?;
        let (source, provenance) = definitions::semantics::source(
            &inv.home,
            &data.definition_commit,
            &data.full_build,
            &table.name,
            inv.source.offline,
            budget,
            &inv.control,
        )?;
        let bytes =
            crate::capture::json_size(&(&source.columns, &source.rows, &provenance), budget)?;
        if bytes > maximum {
            return Err(crate::fault(
                "records.query_resource_budget",
                3,
                "Semantic result exceeds its bound",
            ));
        }
        scope.check()?;
        Ok((
            OwnedSemantics {
                table,
                columns: source.columns,
                rows: source.rows,
                provenance: Some(provenance),
                complete: source.complete,
                allocation: Some(allocation),
                lifetime: lifetime.clone(),
            },
            pin,
        ))
    })();
    scope.finish(result)
}
