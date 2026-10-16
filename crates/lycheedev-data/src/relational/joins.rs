//! Ordered inner/left/cross joins. An optional equality index preserves the
//! evaluator's heterogeneous-type error timing; residual predicates use loops.
use super::*;
#[path = "join_execution.rs"]
pub(super) mod execution;
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
#[cfg(test)]
#[path = "join_index_contract_tests.rs"]
mod contract_tests;
