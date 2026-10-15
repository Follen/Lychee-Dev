//! Ordered inner/left/cross joins. An optional equality index preserves the
//! evaluator's heterogeneous-type error timing; residual predicates use loops.
use super::*;
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Link {
    pub table: RelationName,
    pub alias: String,
    pub derived: Option<Box<Program>>,
    pub(super) left: bool,
    pub(super) condition: Option<Term>,
}
pub(super) fn relation(
    relation: &TableFactor,
    virtual_name: &str,
    depth: usize,
) -> Result<(RelationName, String, Option<Box<Program>>)> {
    if let TableFactor::Derived {
        lateral,
        subquery,
        alias,
        sample,
    } = relation
    {
        if *lateral || sample.is_some() || alias.as_ref().is_some_and(|a| !a.columns.is_empty()) {
            return Err(unsupported());
        }
        let alias = alias
            .as_ref()
            .map(|a| a.name.value.clone())
            .unwrap_or_default();
        return Ok((
            RelationName::internal(virtual_name),
            alias,
            Some(Box::new(Program::compile_query(
                *subquery.clone(),
                depth + 1,
            )?)),
        ));
    }
    let TableFactor::Table {
        name,
        alias,
        args,
        with_hints,
        version,
        with_ordinality,
        partitions,
        json_path,
        sample,
        index_hints,
    } = relation
    else {
        return Err(unsupported());
    };
    if args.is_some()
        || !with_hints.is_empty()
        || version.is_some()
        || *with_ordinality
        || !partitions.is_empty()
        || json_path.is_some()
        || sample.is_some()
        || !index_hints.is_empty()
        || alias.as_ref().is_some_and(|a| !a.columns.is_empty())
    {
        return Err(unsupported());
    }
    let table = RelationName::from_object(name)?;
    let alias = alias
        .as_ref()
        .map(|a| a.name.value.clone())
        .unwrap_or_else(|| table.name.clone());
    Ok((table, alias, None))
}
pub(super) fn compile(
    join: &Join,
    parameters: &mut BTreeSet<String>,
    index: usize,
    depth: usize,
) -> Result<Link> {
    let (table, alias, derived) =
        relation(&join.relation, &format!("\0derived:join:{index}"), depth)?;
    let (left, constraint) = match &join.join_operator {
        JoinOperator::Join(c) | JoinOperator::Inner(c) => (false, c),
        JoinOperator::Left(c) | JoinOperator::LeftOuter(c) => (true, c),
        JoinOperator::CrossJoin(JoinConstraint::None) => {
            return Ok(Link {
                table,
                alias,
                derived,
                left: false,
                condition: None,
            });
        }
        _ => return Err(unsupported()),
    };
    let JoinConstraint::On(on) = constraint else {
        return Err(unsupported());
    };
    Ok(Link {
        table,
        alias,
        derived,
        left,
        condition: Some(term(on, parameters, depth)?),
    })
}
fn source<'a, 'w: 'a>(
    sources: &BTreeMap<String, &'a Source>,
    name: &RelationName,
    work: &mut Work<'w>,
) -> Result<&'a Source> {
    let result = compound::source(sources, name, work)?;
    if result.columns.is_empty() || result.columns.len() > 4096 {
        return Err(binding());
    }
    if result
        .columns
        .iter()
        .any(|s| s.is_empty() || s.len() > 4096)
    {
        return Err(binding());
    }
    let mut names = BTreeSet::new();
    for name in &result.columns {
        if !names.insert(identifier_lower(name)) {
            return Err(binding());
        }
    }
    Ok(result)
}
fn equality_indices(
    condition: Option<&Term>,
    columns: &[String],
    owners: &[String],
    boundary: usize,
) -> Option<(usize, usize)> {
    let Term::Binary(a, BinaryOperator::Eq, b) = condition? else {
        return None;
    };
    let (Term::Column(a), Term::Column(b)) = (&**a, &**b) else {
        return None;
    };
    let (mut a, mut b) = (
        column(columns, ColumnOwners::PerColumn(owners), a).ok()?,
        column(columns, ColumnOwners::PerColumn(owners), b).ok()?,
    );
    if a >= boundary && b < boundary {
        std::mem::swap(&mut a, &mut b);
    }
    (a < boundary && b >= boundary).then_some((a, b - boundary))
}
fn key(v: &Scalar, work: &mut Work<'_>) -> Result<String> {
    let result = match v {
        Scalar::String(s) => {
            work.retain(s.len() + 2)?;
            format!("s:{s}")
        }
        Scalar::Bool(v) => format!("b:{v}"),
        Scalar::Int(v) => format!("n:{v}/1"),
        Scalar::UInt(v) => format!("n:{v}/1"),
        Scalar::Float(_) => {
            work.retain(8192)?;
            let n = rational(v)?;
            format!("n:{}/{}", n.numer(), n.denom())
        }
        Scalar::Null => return Err(type_error()),
    };
    work.retain(result.len())?;
    Ok(result)
}
type KeyIndex = (usize, Option<Scalar>, BTreeMap<String, Vec<usize>>);
fn index(
    condition: Option<&Term>,
    columns: &[String],
    owners: &[String],
    boundary: usize,
    right: &[Vec<Scalar>],
    work: &mut Work<'_>,
) -> Result<Option<KeyIndex>> {
    let Some((left, right_column)) = equality_indices(condition, columns, owners, boundary) else {
        return Ok(None);
    };
    let mut example = None;
    for row in right {
        work.spend()?;
        let value = row.get(right_column).ok_or_else(binding)?;
        if value == &Scalar::Null {
            continue;
        }
        if let Some(example) = &example {
            if compare(example, value).is_err() {
                return Ok(None);
            }
        } else {
            example = Some(work.clone_scalar(value)?);
        }
    }
    let mut buckets = BTreeMap::<String, Vec<usize>>::new();
    // Optional acceleration may consume at most half the remaining local
    // allocation allowance. Fall back before allocating, so shared cumulative
    // admission never needs an inaccurate refund.
    let required = right.iter().try_fold(0usize, |n, row| {
        let extra = match &row[right_column] {
            Scalar::Null => 0,
            Scalar::String(s) => s.len().saturating_mul(2).saturating_add(256),
            Scalar::Float(_) => 8448,
            _ => 256,
        };
        n.checked_add(extra).ok_or_else(budget)
    })?;
    if required > work.bytes / 2 {
        return Ok(None);
    }
    for (i, row) in right.iter().enumerate() {
        work.spend()?;
        let value = &row[right_column];
        if value == &Scalar::Null {
            continue;
        }
        let key = key(value, work)?;
        work.retain(128 + std::mem::size_of::<usize>())?;
        buckets.entry(key).or_default().push(i);
    }
    Ok(Some((left, example, buckets)))
}
struct Right<'a> {
    source: &'a Source,
    // Even owned lexical inputs are copied here: the fixed Go executor owns
    // every retained right row and charges its full materialization cost.
    rows: Vec<Vec<Scalar>>,
    selected: Vec<usize>,
    link: &'a Link,
    start: usize,
    end: usize,
    keys: Option<KeyIndex>,
}
pub(super) struct Plan<'a> {
    pub source: Source,
    pub owners: Vec<String>,
    first: &'a Source,
    first_selected: Vec<usize>,
    projected: bool,
    rights: Vec<Right<'a>>,
    // One bounded owned working row: string capacities are reused across
    // candidates and admitted only as needed during the first input's scan.
    // No cross product or intermediate join is materialized.
    selected: Vec<bool>,
}
pub(super) fn prepare<'a, 'w: 'a>(
    first: &RelationName,
    alias: &str,
    links: &'a [Link],
    sources: &BTreeMap<String, &'a Source>,
    params: &BTreeMap<String, Scalar>,
    work: &mut Work<'w>,
    program: &Program,
) -> Result<Plan<'a>> {
    prepare_scoped(
        first,
        alias,
        links,
        sources,
        params,
        work,
        program,
        &subquery::Context::new(sources, None),
    )
}
pub(super) fn prepare_scoped<'a, 'w: 'a>(
    first: &RelationName,
    alias: &str,
    links: &'a [Link],
    sources: &BTreeMap<String, &'a Source>,
    params: &BTreeMap<String, Scalar>,
    work: &mut Work<'w>,
    program: &Program,
    context: &subquery::Context<'_>,
) -> Result<Plan<'a>> {
    let first = source(sources, first, work)?;
    let mut aliases = BTreeSet::from([identifier_lower(alias)]);
    work.retain(
        first
            .columns
            .iter()
            .map(|s| alias.len() + s.len() + 1 + std::mem::size_of::<String>())
            .sum(),
    )?;
    let mut columns: Vec<_> = first.columns.iter().cloned().collect();
    // The existing logical schema charge owns alias/name bytes. The extra
    // Rust String header is representation-only, not additional Go work.
    if let Some(shared) = work.shared {
        shared
            .safety_only()
            .charge(crate::resource::Cost::retained(
                first.columns.len() * std::mem::size_of::<String>(),
            ))?;
    }
    let mut owners = vec![alias.to_owned(); first.columns.len()];
    work.retain(links.len() * std::mem::size_of::<Right<'_>>())?;
    let mut rights = Vec::with_capacity(links.len());
    let mut complete = first.complete;
    for link in links {
        work.spend()?;
        if !aliases.insert(identifier_lower(&link.alias)) {
            return Err(binding());
        }
        let right = source(sources, &link.table, work)?;
        let start = columns.len();
        complete &= right.complete;
        work.retain(
            right
                .columns
                .iter()
                .map(|c| link.alias.len() + c.len() + 1 + std::mem::size_of::<String>())
                .sum(),
        )?;
        if let Some(shared) = work.shared {
            shared
                .safety_only()
                .charge(crate::resource::Cost::retained(
                    right.columns.len() * std::mem::size_of::<String>(),
                ))?;
        }
        columns.extend(right.columns.iter().cloned());
        owners.extend(std::iter::repeat_n(link.alias.clone(), right.columns.len()));
        if columns.len() > 4096 {
            return Err(binding());
        }
        rights.push(Right {
            source: right,
            rows: vec![],
            selected: vec![],
            link,
            start,
            end: columns.len(),
            keys: None,
        });
    }
    // Bind every ON expression against real schemas before touching any row.
    // Aggregates are invalid at every nesting depth, including unreachable
    // CASE branches, and cannot become valid when an input happens to be empty.
    for right in &rights {
        if let Some(condition) = &right.link.condition {
            bind_with_aggregates(
                condition,
                &columns[..right.end],
                ColumnOwners::PerColumn(&owners),
                params,
                false,
                context,
                work,
            )?;
        }
    }
    // Admit the joined scalar/string buffers and all per-input selected
    // ordinals before allocating them. Collect requirements before any scan.
    work.retain(
        columns.len()
            * (std::mem::size_of::<Scalar>()
                + std::mem::size_of::<String>()
                + std::mem::size_of::<usize>()),
    )?;
    work.retain(columns.len())?;
    let mut selected = vec![false; columns.len()];
    for p in &program.projection {
        if let Some(expr) = &p.expr {
            select(
                expr,
                &columns,
                ColumnOwners::PerColumn(&owners),
                &mut selected,
            );
        } else {
            for i in 0..columns.len() {
                let qualifier = owners[i].as_str();
                if p.name.is_empty() || equal_fold(qualifier, &p.name) {
                    selected[i] = true;
                }
            }
        }
    }
    for expr in program
        .filter
        .as_ref()
        .into_iter()
        .chain(program.order.iter().map(|(expr, _, _)| expr))
        .chain(program.groups.iter())
        .chain(program.having.iter())
    {
        select(
            expr,
            &columns,
            ColumnOwners::PerColumn(&owners),
            &mut selected,
        );
    }
    for right in &rights {
        if let Some(condition) = &right.link.condition {
            // ON binds against this prefix. A later input may introduce the
            // same unqualified name without invalidating an earlier ON.
            select(
                condition,
                &columns[..right.end],
                ColumnOwners::PerColumn(&owners),
                &mut selected[..right.end],
            );
        }
    }
    let first_selected = selected[..first.columns.len()]
        .iter()
        .enumerate()
        .filter_map(|(column, selected)| selected.then_some(column))
        .collect();
    for right in &mut rights {
        right.selected = selected[right.start..right.end]
            .iter()
            .enumerate()
            .filter_map(|(column, selected)| selected.then_some(column))
            .collect();
    }
    let projected = provider::can_project(program, &columns, ColumnOwners::PerColumn(&owners))
        && rights.iter().all(|right| {
            right.link.condition.as_ref().is_none_or(|condition| {
                provider::projectable(
                    condition,
                    &columns[..right.end],
                    ColumnOwners::PerColumn(&owners),
                )
            })
        });
    if !work.planning {
        for right in &mut rights {
            let mut rows = vec![];
            provider::visit(
                right.source,
                projected.then_some(right.selected.as_slice()),
                work,
                &mut |row, work| {
                    work.spend()?;
                    if row.len() != right.source.columns.len() {
                        return Err(binding());
                    }
                    // Validate every supplied scalar, including unused ones,
                    // before admitting and copying provider-owned scratch.
                    // row_cost includes 24 bytes; Go retains 64 per right row.
                    let bytes = row_cost(row)?.checked_add(40).ok_or_else(budget)?;
                    work.retain(bytes)?;
                    rows.push(row.to_vec());
                    Ok(())
                },
            )?;
            right.rows = rows;
            right.keys = index(
                right.link.condition.as_ref(),
                &columns[..right.end],
                &owners[..right.end],
                right.start,
                &right.rows,
                work,
            )?;
        }
    }
    Ok(Plan {
        source: Source {
            columns,
            rows: vec![],
            complete,
        },
        owners,
        first,
        first_selected,
        projected,
        rights,
        selected,
    })
}
pub(super) fn select(
    term: &Term,
    columns: &[String],
    alias: ColumnOwners<'_>,
    selected: &mut [bool],
) {
    match term {
        Term::Column(name) => {
            if let Ok(i) = column(columns, alias, name) {
                selected[i] = true;
            }
        }
        Term::Ordinal(index) => {
            if let Some(selected) = selected.get_mut(*index) {
                *selected = true;
            }
        }
        Term::Unary(_, t) | Term::NullTest(t, _) => select(t, columns, alias, selected),
        Term::Binary(a, _, b) => {
            select(a, columns, alias, selected);
            select(b, columns, alias, selected);
        }
        Term::Between(a, b, c, _) => {
            select(a, columns, alias, selected);
            select(b, columns, alias, selected);
            select(c, columns, alias, selected);
        }
        Term::In(a, items, _) => {
            select(a, columns, alias, selected);
            for t in items {
                select(t, columns, alias, selected);
            }
        }
        // Correlations can reference any current input field. Like Go's
        // neededColumns, a nested query disables projection of this scope.
        Term::InSubquery(..) => selected.fill(true),
        Term::Aggregate(_, Some(input), _) => select(input, columns, alias, selected),
        Term::Aggregate(_, None, _) => {}
        Term::Call(_, items) => {
            for t in items {
                select(t, columns, alias, selected);
            }
        }
        Term::Case(base, branches, otherwise) => {
            if let Some(t) = base {
                select(t, columns, alias, selected);
            }
            for (a, b) in branches {
                select(a, columns, alias, selected);
                select(b, columns, alias, selected);
            }
            if let Some(t) = otherwise {
                select(t, columns, alias, selected);
            }
        }
        Term::Value(_) | Term::Number(_) | Term::Parameter(_) => {}
    }
}
fn assign(
    values: &mut [Scalar],
    strings: &mut [String],
    start: usize,
    row: &[Scalar],
    work: &mut Work<'_>,
    selected: &[bool],
) -> Result<()> {
    for (offset, value) in row.iter().enumerate() {
        let i = start + offset;
        if !selected[i] {
            continue;
        }
        if let Scalar::String(old) = std::mem::replace(&mut values[i], Scalar::Null) {
            strings[i] = old;
        }
        values[i] = match value {
            Scalar::String(text) => {
                work.consume(text.len().div_ceil(32) + 1)?;
                let buffer = &mut strings[i];
                buffer.clear();
                // A lazy input cannot be pre-scanned for maximum string sizes.
                // Reuse the old capacity and admit only checked growth before
                // allocating. Failed admission cannot enlarge this buffer.
                if buffer.capacity() < text.len() {
                    work.retain(text.len() - buffer.capacity())?;
                    buffer.try_reserve_exact(text.len()).map_err(|_| budget())?;
                }
                buffer.push_str(text);
                Scalar::String(std::mem::take(buffer))
            }
            other => other.clone(),
        };
    }
    Ok(())
}
impl Plan<'_> {
    pub fn visit(
        &self,
        params: &BTreeMap<String, Scalar>,
        context: &subquery::Context<'_>,
        work: &mut Work<'_>,
        yield_row: &mut dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()>,
    ) -> Result<()> {
        let mut values = vec![Scalar::Null; self.selected.len()];
        let mut strings = vec![String::new(); self.selected.len()];
        provider::visit(
            self.first,
            self.projected.then_some(self.first_selected.as_slice()),
            work,
            &mut |row, work| {
                work.spend()?;
                if row.len() != self.first.columns.len() {
                    return Err(binding());
                }
                // Projection never waives scalar validation for owned sources;
                // projected providers must also validate their unused fields.
                row_cost(row)?;
                assign(&mut values, &mut strings, 0, row, work, &self.selected)?;
                self.link(
                    0,
                    &mut values,
                    &mut strings,
                    params,
                    context,
                    work,
                    yield_row,
                )
            },
        )
    }
    fn link(
        &self,
        at: usize,
        values: &mut [Scalar],
        strings: &mut [String],
        params: &BTreeMap<String, Scalar>,
        context: &subquery::Context<'_>,
        work: &mut Work<'_>,
        yield_row: &mut dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()>,
    ) -> Result<()> {
        work.spend()?;
        if at == self.rights.len() {
            return yield_row(values, work);
        }
        let right = &self.rights[at];
        let indexed = right.keys.is_some();
        let candidates = if let Some((left, example, buckets)) = &right.keys {
            let value = &values[*left];
            if value == &Scalar::Null || example.is_none() {
                None
            } else {
                compare(value, example.as_ref().unwrap())?;
                buckets.get(&key(value, work)?).map(Vec::as_slice)
            }
        } else {
            None
        };
        let count = if indexed {
            candidates.map_or(0, <[usize]>::len)
        } else {
            right.rows.len()
        };
        let mut matched = false;
        for n in 0..count {
            work.spend()?;
            let right = &self.rights[at];
            let i = if indexed {
                // The index remains immutable while deeper links run. Avoid a
                // per-left candidate vector or boxed iterator.
                let (left, _, buckets) = right.keys.as_ref().unwrap();
                // Candidate identity is rebuilt only for lookup, preserving
                // exact numeric equality and original provider order.
                let k = key(&values[*left], work)?;
                buckets[&k][n]
            } else {
                n
            };
            assign(
                values,
                strings,
                right.start,
                &right.rows[i],
                work,
                &self.selected,
            )?;
            let yes = if let Some(condition) = &right.link.condition {
                // Like Go's ON evaluator, expose only this join prefix to
                // nested callbacks. Later aliases must not hide an outer row.
                let context = subquery::Context {
                    scope: subquery::Scope {
                        fields: subquery::Fields {
                            columns: &self.source.columns[..right.end],
                            alias: ColumnOwners::PerColumn(&self.owners),
                        },
                        ..context.scope
                    },
                    ..*context
                };
                truth(&eval_scoped(
                    condition,
                    &values[..right.end],
                    &self.source.columns[..right.end],
                    ColumnOwners::PerColumn(&self.owners),
                    params,
                    &context,
                    work,
                )?)? == Some(true)
            } else {
                true
            };
            if yes {
                matched = true;
                self.link(at + 1, values, strings, params, context, work, yield_row)?;
            }
        }
        let right = &self.rights[at];
        if right.link.left && !matched {
            for i in right.start..right.end {
                if let Scalar::String(s) = std::mem::replace(&mut values[i], Scalar::Null) {
                    strings[i] = s;
                }
            }
            self.link(at + 1, values, strings, params, context, work, yield_row)?;
        }
        Ok(())
    }
}
