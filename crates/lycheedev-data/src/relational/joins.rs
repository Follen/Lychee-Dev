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
pub(super) fn lower_relation(
    relation: &mut TableFactor,
    virtual_name: &str,
    check: &dyn Fn() -> Result<()>,
) -> Result<(RelationName, String, Option<ast_owner::Owned<Query>>)> {
    check()?;
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
            .map(|a| syntax::identifier(&a.name).map(str::to_owned))
            .transpose()?
            .unwrap_or_default();
        return Ok((
            RelationName::internal(virtual_name),
            alias,
            Some(ast_owner::take_query(subquery)),
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
    let table = RelationName::from_object(&name)?;
    let alias = alias
        .as_ref()
        .map(|a| syntax::identifier(&a.name).map(str::to_owned))
        .transpose()?
        .unwrap_or_else(|| table.name.clone());
    Ok((table, alias, None))
}
/// Derived results stay in caller-owned slots. Sources are borrowed only after
/// this vector is complete, so no self-reference or provider replay is needed.
pub(super) enum Input<'a> {
    Borrowed(&'a Source),
    Derived(Source),
}
impl Input<'_> {
    pub(super) fn source(&self) -> &Source {
        match self {
            Self::Borrowed(source) => source,
            Self::Derived(source) => source,
        }
    }
}
pub(super) fn inputs<'a, 'w: 'a>(
    program: &Program,
    sources: &BTreeMap<String, &'a Source>,
    scope: subquery::Scope<'_>,
    params: &BTreeMap<String, Scalar>,
    work: &mut Work<'w>,
) -> Result<Vec<Input<'a>>> {
    work.safety_retain((program.joins.len() + 1) * std::mem::size_of::<Input<'_>>())?;
    let mut result = Vec::with_capacity(program.joins.len() + 1);
    let mut aliases = BTreeSet::new();
    let mut width = 0usize;
    for (table, alias, derived) in
        std::iter::once((&program.table, &program.alias, &program.derived)).chain(
            program
                .joins
                .iter()
                .map(|link| (&link.table, &link.alias, &link.derived)),
        )
    {
        work.spend()?;
        if !aliases.insert(identifier_lower(alias)) {
            return Err(binding());
        }
        let input = if let Some(query) = derived {
            let computed =
                query.execute_scoped_with(sources, scope.select(sources), params, work)?;
            Input::Derived(Source {
                columns: computed.columns,
                rows: computed.rows,
                complete: computed.complete,
            })
        } else {
            Input::Borrowed(compound::source(sources, table, work)?)
        };
        let source = input.source();
        width = width
            .checked_add(source.columns.len())
            .ok_or_else(binding)?;
        if source.columns.is_empty() || width > 4096 {
            return Err(binding());
        }
        let mut names = BTreeSet::new();
        for name in &source.columns {
            if name.is_empty() || !names.insert(identifier_lower(name)) {
                return Err(binding());
            }
        }
        result.push(input);
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
struct KeyIndex<'a> {
    left: usize,
    right: usize,
    example: Option<usize>,
    buckets: BTreeMap<String, Vec<usize>>,
    _allocation: Option<crate::resource::RetainedReservation<'a>>,
}
fn index<'w>(
    condition: Option<&Term>,
    columns: &[String],
    owners: &[String],
    boundary: usize,
    rows: &[Vec<Scalar>],
    work: &mut Work<'w>,
) -> Result<Option<KeyIndex<'w>>> {
    let Some((left, right)) = equality_indices(condition, columns, owners, boundary) else {
        return Ok(None);
    };
    let allowance = work.bytes / 2;
    let mut charged = 0usize;
    let mut example: Option<usize> = None;
    for (i, row) in rows.iter().enumerate() {
        work.spend()?;
        let value = row.get(right).ok_or_else(binding)?;
        if value == &Scalar::Null {
            continue;
        }
        if let Some(previous) = example {
            if compare(&rows[previous][right], value).is_err() {
                return Ok(None);
            }
        } else {
            example = Some(i);
        }
    }
    let shared = work.shared;
    let mut allocation = shared
        .map(|budget| budget.reserve_retained(0))
        .transpose()?;
    let mut buckets = BTreeMap::<String, Vec<usize>>::new();
    for (i, row) in rows.iter().enumerate() {
        work.spend()?;
        let value = &row[right];
        if value == &Scalar::Null {
            continue;
        }
        let payload = if let Scalar::String(text) = value {
            text.len()
        } else {
            0
        };
        let _temporary = shared
            .map(|budget| budget.reserve_retained(payload + 8192))
            .transpose()?;
        let key = results::equal_key(value)?;
        let cost = 32
            + if buckets.contains_key(&key) {
                0
            } else {
                96 + key.len()
            };
        if cost > allowance - charged {
            // Refund exactly this failed optional index's logical retention.
            // Its consumed work remains, and safety/stop errors never fallback.
            work.bytes += charged;
            return Ok(None);
        }
        if let Some(allocation) = &mut allocation {
            allocation.grow(cost * 2)?;
        }
        work.bytes -= cost;
        charged += cost;
        buckets.entry(key).or_default().push(i);
    }
    Ok(Some(KeyIndex {
        left,
        right,
        example,
        buckets,
    }))
}
struct Right<'a> {
    source: &'a Source,
    selected: Vec<usize>,
    link: &'a Link,
    start: usize,
    end: usize,
}
pub(super) struct Plan<'a, 'w> {
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
    _allocation: Option<crate::resource::RetainedReservation<'w>>,
}
pub(super) fn prepare_scoped<'a, 'w>(
    inputs: &'a [Input<'_>],
    alias: &str,
    links: &'a [Link],
    params: &BTreeMap<String, Scalar>,
    work: &mut Work<'w>,
    program: &Program,
    context: &subquery::Context<'_>,
) -> Result<Box<Plan<'a, 'w>>> {
    let first = inputs[0].source();
    work.safety_retain(
        first
            .columns
            .iter()
            .map(|s| alias.len() + s.len() + 2 * std::mem::size_of::<String>())
            .sum(),
    )?;
    let mut columns = first.columns.clone();
    let mut owners = vec![alias.to_owned(); first.columns.len()];
    work.safety_retain(links.len() * std::mem::size_of::<Right<'_>>())?;
    let mut rights = Vec::with_capacity(links.len());
    let mut complete = first.complete;
    for (link, input) in links.iter().zip(&inputs[1..]) {
        let right = input.source();
        let start = columns.len();
        complete &= right.complete;
        work.safety_retain(
            right
                .columns
                .iter()
                .map(|c| link.alias.len() + c.len() + 2 * std::mem::size_of::<String>())
                .sum(),
        )?;
        columns.extend(right.columns.iter().cloned());
        owners.extend(std::iter::repeat_n(link.alias.clone(), right.columns.len()));
        rights.push(Right {
            source: right,
            selected: vec![],
            link,
            start,
            end: columns.len(),
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
    work.safety_retain(
        columns.len()
            * (std::mem::size_of::<Scalar>()
                + std::mem::size_of::<String>()
                + std::mem::size_of::<usize>()),
    )?;
    work.safety_retain(columns.len())?;
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
    let allocation = work
        .shared
        .map(|budget| budget.reserve_retained(std::mem::size_of::<Plan<'_, '_>>()))
        .transpose()?;
    Ok(Box::new(Plan {
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
        _allocation: allocation,
    }))
}
pub(super) fn select(
    term: &Term,
    columns: &[String],
    alias: ColumnOwners<'_>,
    selected: &mut [bool],
) {
    let mut pending = vec![term];
    while let Some(term) = pending.pop() {
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
            Term::InSubquery(..) | Term::ScalarSubquery(_) | Term::Exists(_) | Term::Star(_) => {
                selected.fill(true);
            }
            Term::Aggregate(name, args, _) if name == "COUNT" => {
                pending.extend(args.iter().rev().filter(|t| !matches!(t, Term::Star(_))));
                continue;
            }
            _ => {}
        }
        term_tree::children(term, &mut pending);
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
                work.consume(0)?;
                let buffer = &mut strings[i];
                buffer.clear();
                // A lazy input cannot be pre-scanned for maximum string sizes.
                // Reuse the old capacity and admit only checked growth before
                // allocating. Failed admission cannot enlarge this buffer.
                if buffer.capacity() < text.len() {
                    work.safety_retain(text.len() - buffer.capacity())?;
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
impl Plan<'_, '_> {
    pub fn visit(
        &self,
        params: &BTreeMap<String, Scalar>,
        context: &subquery::Context<'_>,
        work: &mut Work<'_>,
        yield_row: &mut dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()>,
    ) -> Result<()> {
        let mut materialized = Vec::with_capacity(self.rights.len());
        for right in &self.rights {
            let mut rows = vec![];
            provider::visit(
                right.source,
                self.projected.then_some(right.selected.as_slice()),
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
            let keys = index(
                right.link.condition.as_ref(),
                &self.source.columns[..right.end],
                &self.owners[..right.end],
                right.start,
                &rows,
                work,
            )?;
            materialized.push((rows, keys));
        }
        // A flat JOIN chain is bounded by schema width, not logical AST depth.
        // Reserve one reusable heap DFS stack, rather than one native frame per
        // relation (and never a fresh retained allocation per provider row).
        let cursor_bytes = self
            .rights
            .len()
            .checked_mul(std::mem::size_of::<JoinCursor<'_>>())
            .ok_or_else(budget)?;
        let _cursor_retained = work
            .shared
            .map(|budget| budget.reserve_retained(cursor_bytes))
            .transpose()?;
        let mut cursors = Vec::new();
        cursors
            .try_reserve_exact(self.rights.len())
            .map_err(|_| budget())?;
        let mut values = vec![Scalar::Null; self.selected.len()];
        let mut strings = vec![String::new(); self.selected.len()];
        provider::visit(
            self.first,
            self.projected.then_some(self.first_selected.as_slice()),
            work,
            &mut |row, work| {
                if row.len() != self.first.columns.len() {
                    return Err(binding());
                }
                // A provider owns validation of unused physical fields. Go's
                // first-row callback itself checks width, not unused scalars.
                assign(&mut values, &mut strings, 0, row, work, &self.selected)?;
                self.link(
                    &materialized,
                    &mut cursors,
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
    fn link<'m>(
        &self,
        materialized: &'m [(Vec<Vec<Scalar>>, Option<KeyIndex<'_>>)],
        cursors: &mut Vec<JoinCursor<'m>>,
        values: &mut [Scalar],
        strings: &mut [String],
        params: &BTreeMap<String, Scalar>,
        context: &subquery::Context<'_>,
        work: &mut Work<'_>,
        yield_row: &mut dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()>,
    ) -> Result<()> {
        cursors.clear();
        let mut enter = true;
        loop {
            if enter {
                // Exactly the old recursive entry charge, including a leaf.
                work.spend()?;
                let at = cursors.len();
                if at == self.rights.len() {
                    yield_row(values, work)?;
                } else {
                    let (rows, keys) = &materialized[at];
                    let candidates = if let Some(index) = keys {
                        let value = &values[index.left];
                        if value == &Scalar::Null || index.example.is_none() {
                            None
                        } else {
                            compare(value, &rows[index.example.unwrap()][index.right])?;
                            let payload = if let Scalar::String(text) = value {
                                text.len()
                            } else {
                                0
                            };
                            let _temporary = work
                                .shared
                                .map(|budget| budget.reserve_retained(payload + 8192))
                                .transpose()?;
                            index
                                .buckets
                                .get(&results::equal_key(value)?)
                                .map(Vec::as_slice)
                        }
                    } else {
                        None
                    };
                    let count = if keys.is_some() {
                        candidates.map_or(0, <[usize]>::len)
                    } else {
                        rows.len()
                    };
                    cursors.push(JoinCursor {
                        candidates,
                        count,
                        next: 0,
                        matched: false,
                        fallback: false,
                    });
                }
                enter = false;
            }
            let Some(at) = cursors.len().checked_sub(1) else {
                return Ok(());
            };
            let cursor = &mut cursors[at];
            let right = &self.rights[at];
            if cursor.next < cursor.count {
                work.spend()?;
                let row = cursor
                    .candidates
                    .map_or(cursor.next, |indices| indices[cursor.next]);
                cursor.next += 1;
                assign(
                    values,
                    strings,
                    right.start,
                    &materialized[at].0[row],
                    work,
                    &self.selected,
                )?;
                let yes = if let Some(condition) = &right.link.condition {
                    // ON sees only this prefix, preserving outer-row lookup.
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
                    // An ON match remains matched even if a later join emits
                    // no rows. Parent cursors resume without another charge.
                    cursor.matched = true;
                    enter = true;
                }
            } else if right.link.left && !cursor.matched && !cursor.fallback {
                cursor.fallback = true;
                for index in right.start..right.end {
                    if let Scalar::String(text) =
                        std::mem::replace(&mut values[index], Scalar::Null)
                    {
                        strings[index] = text;
                    }
                }
                enter = true;
            } else {
                cursors.pop();
            }
        }
    }
}

struct JoinCursor<'a> {
    candidates: Option<&'a [usize]>,
    count: usize,
    next: usize,
    matched: bool,
    fallback: bool,
}

#[cfg(test)]
#[path = "join_index_contract_tests.rs"]
mod contract_tests;
