//! Authenticated DBD selection and WDC table preparation for the current engine.
use crate::{definitions, fault, query_source, resource::Budget, schema, table};
use crate::request::DataContext;
use lycheedev_core::{Result, selection};
use std::sync::Arc;
pub(crate) struct Prepared {
    pub pin: selection::PinnedSet,
    pub bundle: definitions::Bundle,
    pub file: query_source::File,
    pub layout: table::Layout,
    pub definition: schema::Definition,
    pub availability: table::Availability,
    // Preserve the exact validated request through every secondary table open,
    // including navigation batches that no longer carry a DataContext.
    pub limits: table::TableLimits,
}
pub(crate) fn prepare_scoped(
    inv: &DataContext,
    name: &str,
    budget: &Arc<Budget>,
    remote: Option<&mut dyn crate::remote::provider::FileScope>,
    resolved: Option<selection::PinnedSet>,
) -> Result<Prepared> {
    Ok(
        prepare_following_scoped(inv, &[name], budget, remote, resolved, &mut |_| {
            Ok(Vec::new())
        })?
        .remove(0),
    )
}

/// The explicit outer scope owns admission through the caller's scan and capture.
/// Legacy callers retain their previous path; this helper never acquires a lease.
pub(crate) fn prepare_following_scoped(
    inv: &DataContext,
    names: &[&str],
    budget: &Arc<Budget>,
    mut remote: Option<&mut dyn crate::remote::provider::FileScope>,
    resolved: Option<selection::PinnedSet>,
    following: &mut dyn FnMut(&[Prepared]) -> Result<Vec<&'static str>>,
) -> Result<Vec<Prepared>> {
    budget.check()?;
    if let Some(scope) = remote.as_ref() {
        scope.check_consumer(inv, budget)?;
    }
    let pin = match resolved {
        Some(pin) => pin,
        None => match remote.as_ref() {
            Some(scope) => scope.resolve_pin()?,
            None => inv.resolve_pinned()?,
        },
    };
    let data = pin
        .data
        .as_ref()
        .ok_or_else(|| fault("command.failed", 5, "records.data_pin_required"))?;
    if let Some(scope) = remote.as_ref() {
        scope.check_pin(data)?;
    }
    let mut bundles = definitions::prepare_many_controlled(
        &inv.home,
        &data.definition_commit,
        names,
        inv.source.offline,
        budget.deadline,
        Some(budget),
        Some(&inv.control),
    )?;
    let ids: Vec<_> = bundles
        .iter()
        .map(|(bundle, _)| bundle.identity.db2_file_data_id)
        .collect();
    let mut prepared = Vec::new();
    let mut accept = |files: Vec<query_source::File>| {
        if files.len() != bundles.len() {
            return Err(fault(
                "records.internal",
                8,
                "Table file batch cardinality differs",
            ));
        }
        let batch_start = prepared.len();
        for ((bundle, raw), file) in std::mem::take(&mut bundles).into_iter().zip(files) {
            budget.check()?;
            // CLI admission fixes the physical file allowance. Derive table
            // limits after source/definition preparation to preserve error order.
            let limits = query_source::table_limits(inv.source.max_file_bytes);
            let (layout, availability) = table::inspect_available_budgeted(
                &file.bytes,
                file.bytes.missing(),
                limits,
                budget.clone(),
            )?;
            if layout.table_hash != bundle.identity.hash {
                return Err(fault(
                    "schema.invalid_definition",
                    4,
                    "schema.invalid_definition",
                ));
            }
            let definition = schema::Document::parse_with_check(&raw, || {
                inv.control.check()?;
                budget.check()
            })?
            .select(&data.full_build, &format!("{:08X}", layout.layout_hash))?;
            prepared.push(Prepared {
                pin: pin.clone(),
                bundle,
                file,
                layout,
                definition,
                availability,
                limits,
            });
        }
        let names = following(&prepared[batch_start..])?;
        if names.is_empty() {
            return Ok(Vec::new());
        }
        budget.check()?;
        bundles = definitions::prepare_many_controlled(
            &inv.home,
            &data.definition_commit,
            &names,
            inv.source.offline,
            budget.deadline,
            Some(budget),
            Some(&inv.control),
        )?;
        if bundles
            .iter()
            .any(|(bundle, _)| bundle.manifest != prepared[0].bundle.manifest)
        {
            return Err(fault(
                "records.definition_identity",
                4,
                "Definition manifest changed between table batches",
            ));
        }
        Ok(bundles
            .iter()
            .map(|(bundle, _)| bundle.identity.db2_file_data_id)
            .collect())
    };
    match remote.as_mut() {
        Some(scope) => scope.read_table_batches(data, &ids, &mut accept)?,
        None => query_source::read_table_batches_budgeted(inv, data, &ids, budget, &mut accept)?,
    }
    Ok(prepared)
}
