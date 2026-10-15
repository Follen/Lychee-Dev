//! Synchronous query-borrowed inputs. A schema descriptor never owns a provider
//! or acquires one by name: computed lexical sources remain ordinary rows.
use super::*;

pub(crate) type RowYield<'visit> = dyn FnMut(&[Scalar]) -> Result<()> + 'visit;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EqualityOutcome {
    Declined,
    Handled,
}
pub(crate) trait RowProvider {
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()>;
    fn scan_selected(&self, _columns: &[usize], emit: &mut RowYield<'_>) -> Result<()> {
        self.scan(emit)
    }
    fn has_equal(&self) -> bool {
        false
    }
    fn equal(
        &self,
        _column: &str,
        _value: &Scalar,
        _emit: &mut RowYield<'_>,
    ) -> Result<EqualityOutcome> {
        Ok(EqualityOutcome::Declined)
    }
}
impl RowProvider for Source {
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        for row in &self.rows {
            emit(row)?;
        }
        Ok(())
    }
}
/// Query-owned, stable physical roots. Resolution happens in the real schema
/// traversal; neither compile-time dependency discovery nor a replay requests
/// a source. The executor caches each resolved schema across both passes.
pub(crate) trait RootResolver {
    fn resolve(&self, table: SourceTableUse<'_>) -> Result<&Source>;
    fn provider(&self, schema: &Source) -> Option<&dyn RowProvider>;
}
#[derive(Clone, Copy)]
pub(crate) struct PreparedSource<'query> {
    pub(crate) schema: &'query Source,
    pub(crate) provider: &'query dyn RowProvider,
}
pub(super) fn find<'a>(source: &Source, work: &Work<'a>) -> Option<&'a dyn RowProvider> {
    work.providers
        .iter()
        .find(|entry| std::ptr::eq(entry.schema, source))
        .map(|entry| entry.provider)
        .or_else(|| work.resolver.and_then(|resolver| resolver.provider(source)))
}
pub(super) fn visit(
    source: &Source,
    selected: Option<&[usize]>,
    work: &mut Work<'_>,
    emit: &mut dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()>,
) -> Result<()> {
    work.consume(0)?;
    if let Some(provider) = find(source, work) {
        let mut visit = |row: &[Scalar]| emit(row, work);
        if let Some(selected) = selected {
            provider.scan_selected(selected, &mut visit)?;
        } else {
            provider.scan(&mut visit)?;
        }
    } else {
        for row in &source.rows {
            emit(row, work)?;
        }
    }
    work.consume(0)
}

/// Match Go neededColumns: stars, subqueries and unresolved output aliases
/// retain full rows and must not call ScanSelected, even with all ordinals.
pub(super) fn projectable(term: &Term, columns: &[String], alias: ColumnOwners<'_>) -> bool {
    match term {
        Term::InSubquery(..) => false,
        Term::Column(name) => column(columns, alias, name).is_ok(),
        Term::Ordinal(index) => *index < columns.len(),
        Term::Unary(_, t) | Term::NullTest(t, _) | Term::Aggregate(_, Some(t), _) => {
            projectable(t, columns, alias)
        }
        Term::Binary(a, _, b) => projectable(a, columns, alias) && projectable(b, columns, alias),
        Term::Between(a, b, c, _) => [a, b, c].iter().all(|t| projectable(t, columns, alias)),
        Term::In(t, args, _) => {
            projectable(t, columns, alias) && args.iter().all(|t| projectable(t, columns, alias))
        }
        Term::Call(_, args) => args.iter().all(|t| projectable(t, columns, alias)),
        Term::Case(base, cases, other) => {
            base.iter()
                .chain(other.iter())
                .all(|t| projectable(t, columns, alias))
                && cases
                    .iter()
                    .all(|(a, b)| projectable(a, columns, alias) && projectable(b, columns, alias))
        }
        _ => true,
    }
}
pub(super) fn can_project(program: &Program, columns: &[String], alias: ColumnOwners<'_>) -> bool {
    program.projection.iter().all(|p| {
        p.expr
            .as_ref()
            .is_some_and(|t| projectable(t, columns, alias))
    }) && program
        .filter
        .iter()
        .chain(program.having.iter())
        .chain(program.groups.iter())
        .chain(program.order.iter().map(|(t, _, _)| t))
        .all(|t| projectable(t, columns, alias))
}

/// Every used input field is selected, including predicates/order/grouping.
/// Unused fields remain subject to the physical provider's validation contract.
pub(super) fn selected(
    program: &Program,
    columns: &[String],
    alias: ColumnOwners<'_>,
    work: &mut Work<'_>,
) -> Result<Vec<usize>> {
    work.consume(0)?;
    if let Some(shared) = work.shared {
        shared.charge(crate::resource::Cost::retained(
            columns.len() * (1 + std::mem::size_of::<usize>()),
        ))?;
    }
    let mut selected = vec![false; columns.len()];
    for projection in &program.projection {
        if let Some(expr) = &projection.expr {
            joins::select(expr, columns, alias, &mut selected);
        } else {
            selected.fill(true);
        }
    }
    for term in program
        .filter
        .iter()
        .chain(program.order.iter().map(|(term, _, _)| term))
        .chain(program.groups.iter())
        .chain(program.having.iter())
    {
        joins::select(term, columns, alias, &mut selected);
    }
    Ok(selected
        .into_iter()
        .enumerate()
        .filter_map(|(i, yes)| yes.then_some(i))
        .collect())
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod provider_tests;
