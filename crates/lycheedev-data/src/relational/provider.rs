//! Synchronous query-borrowed inputs. A schema descriptor never owns a provider
//! or acquires one by name: computed lexical sources remain ordinary rows.
use super::*;

pub(crate) type RowYield<'visit> = dyn FnMut(&[Scalar]) -> Result<()> + 'visit;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EqualityOutcome {
    Declined,
    Handled,
}
/// A single opened producer. The current row and all decoding leases remain
/// owned here until advance or drop. Never recreate a scan to resume it.
pub(crate) trait RowCursor {
    fn advance(&mut self) -> Result<bool>;
    fn row(&self) -> &[Scalar];
}
pub(crate) struct SliceCursor<'a> {
    rows: &'a [Vec<Scalar>],
    next: usize,
    current: Option<usize>,
}
impl<'a> SliceCursor<'a> {
    pub(crate) fn new(rows: &'a [Vec<Scalar>]) -> Self {
        Self {
            rows,
            next: 0,
            current: None,
        }
    }
}
impl RowCursor for SliceCursor<'_> {
    fn advance(&mut self) -> Result<bool> {
        self.current = None;
        if self.next == self.rows.len() {
            return Ok(false);
        }
        self.current = Some(self.next);
        self.next += 1;
        Ok(true)
    }
    fn row(&self) -> &[Scalar] {
        self.current.map_or(&[], |i| self.rows[i].as_slice())
    }
}
pub(crate) enum EqualityCursor<'a> {
    Declined,
    Empty,
    Rows(Box<dyn RowCursor + 'a>),
}
pub(crate) trait RowProvider {
    fn cursor(&self, selected: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>>;
    fn equal_cursor(&self, _column: &str, _value: &Scalar) -> Result<EqualityCursor<'_>> {
        Ok(EqualityCursor::Declined)
    }

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
    fn cursor(&self, _: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>> {
        Ok(Box::new(SliceCursor::new(&self.rows)))
    }

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
/// Compatibility map entrypoints use the same lazy cache as a real resolver.
/// Like the frozen map oracle, a bare static key takes precedence; a qualified
/// key supplies catalogs and the static fallback. Unused aliases neither reject the query nor consume schema cost.
pub(super) enum MapResolver<'a> {
    Owned(&'a BTreeMap<String, Source>),
    Borrowed(&'a BTreeMap<String, &'a Source>),
}
impl MapResolver<'_> {
    fn lookup<'a>(
        entries: impl Iterator<Item = (&'a String, &'a Source)>,
        table: SourceTableUse<'_>,
    ) -> Result<&'a Source> {
        let mut selected = None;
        let mut qualified = false;
        let mut ambiguous = false;
        for (key, source) in entries {
            let Ok(name) = RelationName::from_key(key) else {
                continue;
            };
            if identifier_lower(&name.name) != identifier_lower(table.name)
                || identifier_lower(name.table_use().catalog) != identifier_lower(table.catalog)
            {
                continue;
            }
            let preferred = name.catalog.is_none();
            if selected.is_none() || preferred && !qualified {
                selected = Some(source);
                qualified = preferred;
                ambiguous = false;
            } else if preferred == qualified {
                ambiguous = true;
            }
        }
        if ambiguous {
            return Err(binding());
        }
        selected.ok_or_else(binding)
    }
}
impl RootResolver for MapResolver<'_> {
    fn resolve(&self, table: SourceTableUse<'_>) -> Result<&Source> {
        match self {
            Self::Owned(sources) => Self::lookup(sources.iter(), table),
            Self::Borrowed(sources) => {
                Self::lookup(sources.iter().map(|(name, source)| (name, *source)), table)
            }
        }
    }
    fn provider(&self, _: &Source) -> Option<&dyn RowProvider> {
        None
    }
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
    let measurement = work.scan_index(source);
    work.scan_call(measurement, false);
    if let Some(provider) = find(source, work) {
        let mut visit = |row: &[Scalar]| {
            work.scan_row(measurement);
            emit(row, work)
        };
        if let Some(selected) = selected {
            provider.scan_selected(selected, &mut visit)?;
        } else {
            provider.scan(&mut visit)?;
        }
    } else {
        for row in &source.rows {
            work.scan_row(measurement);
            emit(row, work)?;
        }
    }
    work.consume(0)
}

/// Instrument the real equality callback, including rows yielded before a
/// speculative IN buffer failure and subsequent fallback. No callback replay.
pub(super) fn equal(
    source: &Source,
    provider: &dyn RowProvider,
    column: &str,
    value: &Scalar,
    work: &mut Work<'_>,
    emit: &mut dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()>,
) -> Result<EqualityOutcome> {
    let measurement = work.scan_index(source);
    let result = provider.equal(column, value, &mut |row| {
        work.scan_row(measurement);
        emit(row, work)
    });
    // Error-only outcomes return no public plan, except the internal bounded
    // IN-buffer signal. That error came from a handled provider callback.
    if !matches!(&result, Ok(EqualityOutcome::Declined)) {
        work.scan_call(measurement, true);
    }
    result
}

/// Match Go neededColumns: stars, subqueries and unresolved output aliases
/// retain full rows and must not call ScanSelected, even with all ordinals.
pub(super) fn projectable(term: &Term, columns: &[String], alias: ColumnOwners<'_>) -> bool {
    let mut pending = vec![term];
    while let Some(term) = pending.pop() {
        match term {
            Term::InSubquery(..) | Term::ScalarSubquery(_) | Term::Exists(_) | Term::Star(_) => {
                return false;
            }
            Term::Column(name) if column(columns, alias, name).is_err() => return false,
            Term::Ordinal(index) if *index >= columns.len() => return false,
            Term::Aggregate(name, args, _) if name == "COUNT" => {
                pending.extend(args.iter().rev().filter(|t| !matches!(t, Term::Star(_))));
                continue;
            }
            _ => {}
        }
        term_tree::children(term, &mut pending);
    }
    true
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
