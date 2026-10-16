//! Breadth-first UNION ALL against only the prior delta. Blocks own their rows
//! until the fixed point; no scalar payload is cloned to accumulate results.
use super::*;

pub(super) fn references(program: &Program, name: &str) -> usize {
    let mut count = 0;
    let mut pending = vec![program];
    while let Some(program) = pending.pop() {
        if program.bindings.iter().any(|(n, _)| equal_fold(n, name)) {
            continue;
        }
        for term in program.terms() {
            subquery::visit_queries(term, &mut |query| pending.push(query));
        }
        for (table, body) in std::iter::once((&program.table, &program.derived))
            .chain(program.joins.iter().map(|j| (&j.table, &j.derived)))
        {
            if let Some(body) = body {
                pending.push(body);
            } else {
                count += usize::from(table.catalog.is_none() && equal_fold(&table.name, name));
            }
        }
        pending.extend(program.branches.iter());
        pending.extend(program.bindings.iter().map(|(_, body)| body));
    }
    count
}

fn columns_cost(columns: &[String]) -> Result<usize> {
    columns.iter().try_fold(24usize * columns.len(), |n, s| {
        n.checked_add(s.len()).ok_or_else(budget)
    })
}
fn delta_scope<'a>(
    sources: &BTreeMap<String, &'a Source>,
    name: &str,
    delta: &'a Source,
    work: &mut Work<'_>,
) -> Result<BTreeMap<String, &'a Source>> {
    let bytes = sources.keys().try_fold(name.len() + 96, |n, k| {
        n.checked_add(k.len() + 96).ok_or_else(budget)
    })?;
    work.safety_retain(bytes)?;
    let mut inputs = sources.clone();
    let key = identifiers::lexical_key(name);
    inputs.retain(|n, _| identifier_lower(n) != key);
    inputs.insert(key, delta);
    Ok(inputs)
}

pub(super) fn execute(
    name: &str,
    program: &Program,
    sources: &BTreeMap<String, &Source>,
    scope: subquery::Scope<'_>,
    parameters: &BTreeMap<String, Scalar>,
    work: &mut Work<'_>,
) -> Result<QueryResult> {
    if program.branches.len() != 2 || !program.bindings.is_empty() {
        return Err(unsupported());
    }
    let seed = &program.branches[0];
    let step = &program.branches[1];
    if references(seed, name) != 0 || references(step, name) != 1 {
        return Err(binding());
    }
    if [program, seed, step]
        .iter()
        .any(|p| !p.order.is_empty() || p.limit.is_some() || p.offset.is_some())
    {
        return Err(unsupported());
    }
    let first = seed.execute_scoped_with(sources, scope, parameters, work)?;
    work.safety_retain(columns_cost(&first.columns)? + 96)?;
    let columns = first.columns.clone();
    let mut complete = first.complete;
    let mut blocks = vec![Source {
        columns: first.columns,
        rows: first.rows,
        complete: first.complete,
    }];
    // Bind and validate the member even if the seed is empty. Its computed
    // zero-delta rows are discarded: an aggregate cannot invent a starting row.
    if work.planning {
        let inputs = delta_scope(sources, name, &blocks[0], work)?;
        let checked = step.execute_scoped_with(&inputs, scope, parameters, work)?;
        if checked.columns.len() != columns.len() {
            return Err(binding());
        }
        complete &= checked.complete;
    }
    while !blocks.last().unwrap().rows.is_empty() {
        work.spend()?;
        let inputs = delta_scope(sources, name, blocks.last().unwrap(), work)?;
        let next = step.execute_scoped_with(&inputs, scope, parameters, work)?;
        if next.columns.len() != columns.len() {
            return Err(binding());
        }
        complete &= next.complete;
        work.retain(next.rows.len().checked_mul(24).ok_or_else(budget)?)?;
        if next.rows.is_empty() {
            break;
        }
        work.safety_retain(columns_cost(&columns)? + 96)?;
        blocks.push(Source {
            columns: columns.clone(),
            rows: next.rows,
            complete: next.complete,
        });
    }
    let length = blocks.iter().try_fold(0usize, |n, b| {
        n.checked_add(b.rows.len()).ok_or_else(budget)
    })?;
    work.safety_retain(length.checked_mul(24).ok_or_else(budget)?)?;
    let mut rows = Vec::with_capacity(length);
    for block in blocks {
        work.consume(0)?;
        rows.extend(block.rows);
    }
    Ok(QueryResult {
        plan: None,
        columns,
        rows,
        complete,
    })
}
