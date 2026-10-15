//! Ordered inner/left/cross joins. An optional equality index preserves the
//! evaluator's heterogeneous-type error timing; residual predicates use loops.
use super::*;
pub(super) const QUALIFIED: &str = "<joined>";
pub(super) struct Link {
    pub table: String,
    pub alias: String,
    pub derived: Option<Box<Program>>,
    pub(super) left: bool,
    pub(super) condition: Option<Term>,
}
pub(super) fn relation(
    relation: &TableFactor,
    virtual_name: &str,
    depth: usize,
) -> Result<(String, String, Option<Box<Program>>)> {
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
            virtual_name.into(),
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
    let table = name.to_string();
    if table.is_empty() || table.len() > 512 || table.contains('\0') {
        return Err(binding());
    }
    let alias = alias
        .as_ref()
        .map(|a| a.name.value.clone())
        .unwrap_or_else(|| table.rsplit('.').next().unwrap().into());
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
        condition: Some(term(on, parameters, 0)?),
    })
}
fn source<'a>(sources: &BTreeMap<String, &'a Source>, name: &str) -> Result<&'a Source> {
    let mut found = sources
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name));
    let result = found.next().map(|(_, v)| *v).ok_or_else(binding)?;
    if found.next().is_some() || result.columns.is_empty() || result.columns.len() > 1024 {
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
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(binding());
        }
    }
    Ok(result)
}
fn equality_indices(
    condition: Option<&Term>,
    columns: &[String],
    boundary: usize,
) -> Option<(usize, usize)> {
    let Term::Binary(a, BinaryOperator::Eq, b) = condition? else {
        return None;
    };
    let (Term::Column(a), Term::Column(b)) = (&**a, &**b) else {
        return None;
    };
    let (mut a, mut b) = (
        column(columns, QUALIFIED, a).ok()?,
        column(columns, QUALIFIED, b).ok()?,
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
    boundary: usize,
    right: &Source,
    work: &mut Work<'_>,
) -> Result<Option<KeyIndex>> {
    let Some((left, right_column)) = equality_indices(condition, columns, boundary) else {
        return Ok(None);
    };
    let mut example = None;
    for row in &right.rows {
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
    let required = right.rows.iter().try_fold(0usize, |n, row| {
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
    for (i, row) in right.rows.iter().enumerate() {
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
    link: &'a Link,
    start: usize,
    end: usize,
    keys: Option<KeyIndex>,
}
pub(super) struct Plan<'a> {
    pub source: Source,
    first: &'a Source,
    rights: Vec<Right<'a>>,
    // One bounded owned working row: string capacities are reused across
    // candidates. No cross product or intermediate join is materialized.
    capacities: Vec<usize>,
    selected: Vec<bool>,
}
pub(super) fn prepare<'a>(
    first: &str,
    alias: &str,
    links: &'a [Link],
    sources: &BTreeMap<String, &'a Source>,
    params: &BTreeMap<String, Scalar>,
    work: &mut Work<'_>,
    program: &Program,
) -> Result<Plan<'a>> {
    let first = source(sources, first)?;
    let mut aliases = BTreeSet::from([alias.to_ascii_lowercase()]);
    work.retain(
        first
            .columns
            .iter()
            .map(|s| alias.len() + s.len() + 1 + std::mem::size_of::<String>())
            .sum(),
    )?;
    let mut columns: Vec<_> = first
        .columns
        .iter()
        .map(|c| format!("{alias}.{c}"))
        .collect();
    work.retain(links.len() * std::mem::size_of::<Right<'_>>())?;
    let mut rights = Vec::with_capacity(links.len());
    let mut complete = first.complete;
    for link in links {
        work.spend()?;
        if !aliases.insert(link.alias.to_ascii_lowercase()) {
            return Err(binding());
        }
        let right = source(sources, &link.table)?;
        let start = columns.len();
        complete &= right.complete;
        work.retain(
            right
                .columns
                .iter()
                .map(|c| link.alias.len() + c.len() + 1 + std::mem::size_of::<String>())
                .sum(),
        )?;
        columns.extend(right.columns.iter().map(|c| format!("{}.{c}", link.alias)));
        if columns.len() > 1024 {
            return Err(budget());
        }
        rights.push(Right {
            source: right,
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
            bind_with_aggregates(condition, &columns[..right.end], QUALIFIED, params, false)?;
        }
    }
    if !work.planning {
        for right in &mut rights {
            if right
                .source
                .rows
                .iter()
                .any(|row| row.len() != right.source.columns.len())
            {
                return Err(binding());
            }
            right.keys = index(
                right.link.condition.as_ref(),
                &columns[..right.end],
                right.start,
                right.source,
                work,
            )?;
        }
    }
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
            select(expr, &columns, &mut selected);
        } else {
            for (i, name) in columns.iter().enumerate() {
                let qualifier = name.rsplit_once('.').map_or("", |(q, _)| q);
                if p.name.is_empty() || qualifier.eq_ignore_ascii_case(&p.name) {
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
        .chain(links.iter().filter_map(|l| l.condition.as_ref()))
    {
        select(expr, &columns, &mut selected);
    }
    let mut capacities = vec![0usize; columns.len()];
    let mut start = 0;
    for input in std::iter::once(first).chain(rights.iter().map(|r| r.source)) {
        if work.planning {
            continue;
        }
        for row in &input.rows {
            work.spend()?;
            // Projection does not authorize invalid unused scalar values.
            if row.len() != input.columns.len() {
                return Err(binding());
            }
            row_cost(row)?;
            for (column, value) in row.iter().enumerate() {
                if let Scalar::String(s) = value
                    && selected[start + column]
                {
                    capacities[start + column] = capacities[start + column].max(s.len());
                }
            }
        }
        start += input.columns.len();
    }
    let bytes = capacities
        .iter()
        .try_fold(0usize, |n, v| n.checked_add(*v).ok_or_else(budget))?;
    work.retain(bytes)?;
    Ok(Plan {
        source: Source {
            columns,
            rows: vec![],
            complete,
        },
        first,
        rights,
        capacities,
        selected,
    })
}
fn select(term: &Term, columns: &[String], selected: &mut [bool]) {
    match term {
        Term::Column(name) => {
            if let Ok(i) = column(columns, QUALIFIED, name) {
                selected[i] = true;
            }
        }
        Term::Unary(_, t) | Term::NullTest(t, _) => select(t, columns, selected),
        Term::Binary(a, _, b) => {
            select(a, columns, selected);
            select(b, columns, selected);
        }
        Term::Between(a, b, c, _) => {
            select(a, columns, selected);
            select(b, columns, selected);
            select(c, columns, selected);
        }
        Term::In(a, items, _) => {
            select(a, columns, selected);
            for t in items {
                select(t, columns, selected);
            }
        }
        Term::Aggregate(_, Some(input), _) => select(input, columns, selected),
        Term::Aggregate(_, None, _) => {}
        Term::Call(_, items) => {
            for t in items {
                select(t, columns, selected);
            }
        }
        Term::Case(base, branches, otherwise) => {
            if let Some(t) = base {
                select(t, columns, selected);
            }
            for (a, b) in branches {
                select(a, columns, selected);
                select(b, columns, selected);
            }
            if let Some(t) = otherwise {
                select(t, columns, selected);
            }
        }
        Term::Value(_) | Term::Number(_) | Term::Ordinal(_) | Term::Parameter(_) => {}
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
                // The capacity was admitted from every provider row before
                // allocation; this check prevents an unexpected realloc.
                if buffer.capacity() < text.len() {
                    return Err(budget());
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
        work: &mut Work<'_>,
        yield_row: &mut dyn FnMut(&[Scalar], &mut Work<'_>) -> Result<()>,
    ) -> Result<()> {
        let mut values = vec![Scalar::Null; self.capacities.len()];
        let mut strings: Vec<_> = self
            .capacities
            .iter()
            .map(|n| String::with_capacity(*n))
            .collect();
        for row in &self.first.rows {
            work.spend()?;
            assign(&mut values, &mut strings, 0, row, work, &self.selected)?;
            self.link(0, &mut values, &mut strings, params, work, yield_row)?;
        }
        Ok(())
    }
    fn link(
        &self,
        at: usize,
        values: &mut [Scalar],
        strings: &mut [String],
        params: &BTreeMap<String, Scalar>,
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
            right.source.rows.len()
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
                &right.source.rows[i],
                work,
                &self.selected,
            )?;
            let yes = if let Some(condition) = &right.link.condition {
                truth(&eval(
                    condition,
                    &values[..right.end],
                    &self.source.columns[..right.end],
                    QUALIFIED,
                    params,
                    work,
                )?)? == Some(true)
            } else {
                true
            };
            if yes {
                matched = true;
                self.link(at + 1, values, strings, params, work, yield_row)?;
            }
        }
        let right = &self.rights[at];
        if right.link.left && !matched {
            for i in right.start..right.end {
                if let Scalar::String(s) = std::mem::replace(&mut values[i], Scalar::Null) {
                    strings[i] = s;
                }
            }
            self.link(at + 1, values, strings, params, work, yield_row)?;
        }
        Ok(())
    }
}
