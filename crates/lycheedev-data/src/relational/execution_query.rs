//! Query-shell continuations. Only real query entries use the driver mailbox;
//! recursive wrappers and UNION tails remain bounded helpers without charges.
use super::*;
use execution::{Child, Control, Query, Scope, Shared, SourceRef, Sources};

fn source<'a>(result: QueryResult, control: &Control<'a, '_>) -> Result<SourceRef<'a>> {
    control
        .share(Source {
            columns: result.columns,
            rows: result.rows,
            complete: result.complete,
        })
        .map(SourceRef::Owned)
}
fn visible<'a>(
    inputs: &Sources<'a>,
    reserved: &BTreeSet<String>,
    results: &Sources<'a>,
    control: &Control<'a, '_>,
) -> Result<Shared<'a, Sources<'a>>> {
    let bytes = inputs
        .keys()
        .chain(results.keys())
        .try_fold(0usize, |n, name| {
            n.checked_add(name.len() + 96).ok_or_else(budget)
        })?;
    control.work(|w| {
        w.safety_retain(
            bytes
                .checked_add(reserved.iter().map(|n| n.len() + 96).sum::<usize>())
                .ok_or_else(budget)?,
        )
    })?;
    let mut map = BTreeMap::new();
    for (name, source) in inputs {
        if !reserved.contains(&identifier_lower(name)) {
            map.insert(name.clone(), source.clone());
        }
    }
    for name in reserved {
        map.insert(name.clone(), SourceRef::Borrowed(&RESERVED));
    }
    for (name, source) in results {
        map.insert(name.clone(), source.clone());
    }
    control.share(map)
}
static RESERVED: Source = Source {
    columns: Vec::new(),
    rows: Vec::new(),
    complete: true,
};
pub(super) async fn run<'a, 'w: 'a>(
    query: Query<'a>,
    sources: Shared<'a, Sources<'a>>,
    scope: Scope<'a>,
    control: &Control<'a, 'w>,
) -> Result<QueryResult> {
    if query.bindings.is_empty() {
        return body(&query, sources, scope, control).await;
    }
    control.work(|w| w.safety_retain(query.bindings.iter().map(|(n, _)| n.len() + 96).sum()))?;
    let reserved = query
        .bindings
        .iter()
        .map(|(n, _)| identifiers::lexical_key(n))
        .collect();
    let mut results = Sources::new();
    for (index, (name, program)) in query.bindings.iter().enumerate() {
        let inputs = visible(&sources, &reserved, &results, control)?;
        let child = query.child(Child::Binding(index), control)?;
        let result = if query.recursive && recursive::references(program, name) > 0 {
            recursive(name, child, inputs, scope.clone(), control).await?
        } else {
            control.query(child, inputs, scope.clone()).await?
        };
        control.work(|w| w.safety_retain(name.len() + 96))?;
        results.insert(identifiers::lexical_key(name), source(result, control)?);
    }
    let inputs = visible(&sources, &reserved, &results, control)?;
    let mut result = body(&query, inputs, scope, control).await?;
    result.complete &= results.values().all(|s| s.complete);
    Ok(result)
}
async fn body<'a, 'w: 'a>(
    query: &Query<'a>,
    sources: Shared<'a, Sources<'a>>,
    scope: Scope<'a>,
    control: &Control<'a, 'w>,
) -> Result<QueryResult> {
    if query.branches.is_empty() {
        return execution_select::run(
            query.clone(),
            sources,
            scope,
            ProjectionMode::NamedSource,
            control,
        )
        .await;
    }
    let mut combined: Option<Source> = None;
    for (index, branch) in query.branches.iter().enumerate() {
        if index + 1 < query.branches.len()
            && (!branch.order.is_empty() || branch.limit.is_some() || branch.offset.is_some())
        {
            return Err(problem(
                "relational.invalid_syntax",
                "Non-final UNION ALL branch has a suffix",
            ));
        }
        let result = control
            .query(
                query.child(Child::Branch(index), control)?,
                sources.clone(),
                scope.clone(),
            )
            .await?;
        control.work(|w| w.retain(result.rows.len().checked_mul(24).ok_or_else(budget)?))?;
        if let Some(combined) = combined.as_mut() {
            if result.columns.len() != combined.columns.len() {
                return Err(binding());
            }
            combined.rows.extend(result.rows);
            combined.complete &= result.complete;
        } else {
            combined = Some(Source {
                columns: result.columns,
                rows: result.rows,
                complete: result.complete,
            });
        }
    }
    let combined = combined.ok_or_else(binding)?;
    control.work(|w| {
        w.safety_retain(query.table.key().len() + 96)?;
        w.safety_retain(sources.keys().map(|name| name.len() + 96).sum())
    })?;
    let mut inputs = (**sources).clone();
    inputs.insert(
        query.table.key().to_owned(),
        SourceRef::Owned(control.share(combined)?),
    );
    execution_select::run(
        query.clone(),
        control.share(inputs)?,
        scope,
        ProjectionMode::CompoundOrdinals,
        control,
    )
    .await
}
fn columns_cost(columns: &[String]) -> Result<usize> {
    columns.iter().try_fold(24 * columns.len(), |n, s| {
        n.checked_add(s.len()).ok_or_else(budget)
    })
}
fn delta_scope<'a>(
    sources: &Sources<'a>,
    name: &str,
    delta: SourceRef<'a>,
    control: &Control<'a, '_>,
) -> Result<Shared<'a, Sources<'a>>> {
    let bytes = sources.keys().try_fold(name.len() + 96, |n, k| {
        n.checked_add(k.len() + 96).ok_or_else(budget)
    })?;
    control.work(|w| w.safety_retain(bytes))?;
    let mut inputs = sources.clone();
    let key = identifiers::lexical_key(name);
    inputs.retain(|n, _| identifier_lower(n) != key);
    inputs.insert(key, delta);
    control.share(inputs)
}
async fn recursive<'a, 'w: 'a>(
    name: &str,
    query: Query<'a>,
    sources: Shared<'a, Sources<'a>>,
    scope: Scope<'a>,
    control: &Control<'a, 'w>,
) -> Result<QueryResult> {
    if query.branches.len() != 2 || !query.bindings.is_empty() {
        return Err(unsupported());
    }
    let seed = &query.branches[0];
    let step = &query.branches[1];
    if recursive::references(seed, name) != 0 || recursive::references(step, name) != 1 {
        return Err(binding());
    }
    if [&*query, seed, step]
        .iter()
        .any(|p| !p.order.is_empty() || p.limit.is_some() || p.offset.is_some())
    {
        return Err(unsupported());
    }
    let first = control
        .query(
            query.child(Child::Branch(0), control)?,
            sources.clone(),
            scope.clone(),
        )
        .await?;
    control.work(|w| w.safety_retain(columns_cost(&first.columns)? + 96))?;
    let columns = first.columns.clone();
    let mut complete = first.complete;
    let mut blocks = vec![source(first, control)?];
    if control.planning() {
        let inputs = delta_scope(&sources, name, blocks[0].clone(), control)?;
        let checked = control
            .query(
                query.child(Child::Branch(1), control)?,
                inputs,
                scope.clone(),
            )
            .await?;
        if checked.columns.len() != columns.len() {
            return Err(binding());
        }
        complete &= checked.complete;
    }
    while !blocks.last().ok_or_else(binding)?.rows.is_empty() {
        control.work(|w| w.spend())?;
        let inputs = delta_scope(
            &sources,
            name,
            blocks.last().ok_or_else(binding)?.clone(),
            control,
        )?;
        let next = control
            .query(
                query.child(Child::Branch(1), control)?,
                inputs,
                scope.clone(),
            )
            .await?;
        if next.columns.len() != columns.len() {
            return Err(binding());
        }
        complete &= next.complete;
        control.work(|w| w.retain(next.rows.len().checked_mul(24).ok_or_else(budget)?))?;
        if next.rows.is_empty() {
            break;
        }
        control.work(|w| w.safety_retain(columns_cost(&columns)? + 96))?;
        blocks.push(source(
            QueryResult {
                columns: columns.clone(),
                ..next
            },
            control,
        )?);
    }
    let length = blocks.iter().try_fold(0usize, |n, b| {
        n.checked_add(b.rows.len()).ok_or_else(budget)
    })?;
    control.work(|w| w.safety_retain(length.checked_mul(24).ok_or_else(budget)?))?;
    let mut rows = Vec::with_capacity(length);
    for block in blocks {
        control.work(|w| w.consume(0))?;
        let SourceRef::Owned(block) = block else {
            return Err(unsupported());
        };
        let block = std::rc::Rc::try_unwrap(block).map_err(|_| unsupported())?;
        rows.extend(block.value.rows);
    }
    Ok(QueryResult {
        plan: None,
        columns,
        rows,
        complete,
    })
}
