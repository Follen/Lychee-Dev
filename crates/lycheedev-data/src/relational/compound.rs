//! Lexical CTE scopes borrow physical rows and retain only computed results.
//! All definitions run under the caller's original work/memory/cancel budget.
use super::*;

pub(super) fn validate_all_keyword(
    sql: &str,
    tokens: &[sqlparser::tokenizer::TokenWithSpan],
    consumed: usize,
) -> Result<()> {
    use sqlparser::tokenizer::Token;
    for (i, token) in tokens.iter().enumerate() {
        // On a parser error, only an already-consumed UNION may supersede
        // that error. A malformed earlier projection keeps its original turn.
        if i >= consumed {
            break;
        }
        // Go reserves ALL everywhere except UNION ALL. sqlparser erases
        // SELECT ALL and LIMIT ALL, so reject those tokens before AST lowering.
        if matches!(&token.token, Token::Word(word) if word.quote_style.is_none() && equal_fold(&word.value, "ALL"))
            && (i == 0
                || !matches!(&tokens[i-1].token, Token::Word(word) if word.quote_style.is_none() && equal_fold(&word.value, "UNION")))
        {
            return Err(problem("relational.invalid_syntax", "Unexpected ALL"));
        }
        if !matches!(&token.token, Token::Word(word) if word.quote_style.is_none() && equal_fold(&word.value, "UNION"))
        {
            continue;
        }
        if tokens.get(i + 1).is_some_and(|t| matches!(&t.token, Token::Word(word) if word.quote_style.is_none() && equal_fold(&word.value, "ALL"))) {
            continue;
        }
        let (offset, line, column) = if let Some(next) = tokens.get(i + 1) {
            let location = next.span.start;
            let mut line = 1u64;
            let mut column = 1u64;
            let mut found = sql.len();
            for (at, ch) in sql.char_indices() {
                if line == location.line && column == location.column {
                    found = at;
                    break;
                }
                if ch == '\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += 1;
                }
            }
            (found, location.line, location.column)
        } else {
            let prefix = sql.rsplit('\n').next().unwrap();
            (
                sql.len(),
                sql.bytes().filter(|b| *b == b'\n').count() as u64 + 1,
                prefix.chars().count() as u64 + 1,
            )
        };
        let mut error = problem(
            "relational.invalid_syntax",
            &format!("relational.invalid_syntax at {line}:{column}: expected ALL"),
        );
        error.details =
            serde_json::json!({"location":{"offset":offset,"line":line,"column":column}});
        return Err(error);
    }
    Ok(())
}

/// Physical roots have one resolver/cache identity. Lexical CTE keys remain
/// unqualified, so an explicit catalog always bypasses lexical shadowing.
pub(super) fn physical_key(name: &str) -> Result<String> {
    Ok(RelationName::from_key(name)?.physical_key())
}
pub(super) fn source<'a, 'w: 'a>(
    inputs: &BTreeMap<String, &'a Source>,
    name: &RelationName,
    work: &mut Work<'w>,
) -> Result<&'a Source> {
    if let Some(key) = name.lexical_key() {
        // Internal scoped callers may provide lexical keys directly. Root
        // entrypoints canonicalize physical keys before entering this map.
        let mut lexical = inputs
            .iter()
            .filter(|(name, _)| identifier_lower(name) == key);
        if let Some((_, source)) = lexical.next() {
            if lexical.next().is_some() {
                return Err(binding());
            }
            return Ok(*source);
        }
    }
    if let Some(source) = inputs.get(&name.physical_key()) {
        work.measure_root(name)?;
        return Ok(*source);
    }
    work.resolve_root(name)
}
static RESERVED_SOURCE: Source = Source {
    columns: Vec::new(),
    rows: Vec::new(),
    complete: true,
};

pub(super) fn dependencies<'a>(
    program: &'a Program,
    outer: &BTreeSet<String>,
    tables: &mut Vec<&'a RelationName>,
) {
    enum Task<'a> {
        Query(&'a Program, std::rc::Rc<BTreeSet<String>>),
        Table(&'a RelationName, std::rc::Rc<BTreeSet<String>>),
    }
    let mut pending = vec![Task::Query(program, std::rc::Rc::new(outer.clone()))];
    while let Some(task) = pending.pop() {
        match task {
            Task::Table(table, names) => {
                if !(table.catalog.is_none() && names.contains(&identifier_lower(&table.name)))
                    && !tables
                        .iter()
                        .any(|n| n.physical_key() == table.physical_key())
                {
                    tables.push(table);
                }
            }
            Task::Query(program, outer) => {
                let mut names = (*outer).clone();
                names.extend(
                    program
                        .bindings
                        .iter()
                        .map(|(name, _)| identifier_lower(name)),
                );
                let names = std::rc::Rc::new(names);
                let mut next = Vec::new();
                for (_, body) in &program.bindings {
                    next.push(Task::Query(body, names.clone()));
                }
                if program.branches.is_empty() {
                    for term in program.terms() {
                        subquery::visit_queries(term, &mut |query| {
                            next.push(Task::Query(query, names.clone()))
                        });
                    }
                    for (table, derived) in std::iter::once((&program.table, &program.derived))
                        .chain(program.joins.iter().map(|j| (&j.table, &j.derived)))
                    {
                        next.push(if let Some(body) = derived {
                            Task::Query(body, names.clone())
                        } else {
                            Task::Table(table, names.clone())
                        });
                    }
                } else {
                    // Inspection carries earlier branch WITH names into later
                    // chain nodes; execution deliberately uses another scope.
                    let mut inspected = names;
                    for body in &program.branches {
                        next.push(Task::Query(body, inspected.clone()));
                        let mut later = (*inspected).clone();
                        later.extend(body.bindings.iter().map(|(name, _)| identifier_lower(name)));
                        inspected = std::rc::Rc::new(later);
                    }
                    for term in program.terms() {
                        subquery::visit_queries(term, &mut |query| {
                            next.push(Task::Query(query, inspected.clone()))
                        });
                    }
                }
                pending.extend(next.into_iter().rev());
            }
        }
    }
}

fn visible<'a>(
    inputs: &BTreeMap<String, &'a Source>,
    reserved: &BTreeSet<String>,
    results: &'a BTreeMap<String, Source>,
    work: &mut Work<'_>,
) -> Result<BTreeMap<String, &'a Source>> {
    let bytes = inputs
        .keys()
        .chain(results.keys())
        .try_fold(0usize, |n, name| {
            n.checked_add(name.len() + 96).ok_or_else(budget)
        })?;
    work.safety_retain(
        bytes
            .checked_add(reserved.iter().map(|name| name.len() + 96).sum::<usize>())
            .ok_or_else(budget)?,
    )?;
    let mut map = BTreeMap::new();
    for (name, source) in inputs {
        if !reserved.contains(&identifier_lower(name)) {
            map.insert(name.clone(), *source);
        }
    }
    for name in reserved {
        map.insert(name.clone(), &RESERVED_SOURCE);
    }
    for (name, source) in results {
        map.insert(name.clone(), source);
    }
    Ok(map)
}

pub(super) fn execute(
    program: &Program,
    sources: &BTreeMap<String, &Source>,
    scope: subquery::Scope<'_>,
    parameters: &BTreeMap<String, Scalar>,
    work: &mut Work<'_>,
) -> Result<QueryResult> {
    if program.bindings.is_empty() {
        return execute_body(program, sources, scope, parameters, work);
    }
    work.safety_retain(program.bindings.iter().map(|(n, _)| n.len() + 96).sum())?;
    // Reserve every local name before any definition runs: a self or forward
    // reference must fail binding rather than resolve a physical/outer table.
    let reserved: BTreeSet<_> = program
        .bindings
        .iter()
        .map(|(n, _)| identifiers::lexical_key(n))
        .collect();
    let mut results = BTreeMap::new();
    for (name, body) in &program.bindings {
        let inputs = visible(sources, &reserved, &results, work)?;
        let result = if program.recursive && recursive::references(body, name) > 0 {
            recursive::execute(name, body, &inputs, scope, parameters, work)?
        } else {
            body.execute_scoped_with(&inputs, scope, parameters, work)?
        };
        work.safety_retain(name.len() + 96)?;
        results.insert(
            identifiers::lexical_key(name),
            Source {
                columns: result.columns,
                rows: result.rows,
                complete: result.complete,
            },
        );
    }
    let inputs = visible(sources, &reserved, &results, work)?;
    let mut result = execute_body(program, &inputs, scope, parameters, work)?;
    result.complete &= results.values().all(|s| s.complete);
    Ok(result)
}

/// Retrieval shells retain suffixes and WITH at their actual query node.
/// Flatten only UNION's right chain, transferring each local WITH to that
/// branch (not the subsequent branches), just as executeQuery's copied branch.
#[inline(never)]
pub(super) fn branches(
    body: Box<SetExpr>,
    check: &dyn Fn() -> Result<()>,
) -> Result<Vec<ast_owner::Owned<Query>>> {
    fn plain(body: Box<SetExpr>) -> ast_owner::Owned<Query> {
        ast_owner::Owned::new(Box::new(Query {
            with: None,
            body,
            order_by: None,
            limit_clause: None,
            fetch: None,
            locks: Vec::new(),
            for_clause: None,
            settings: None,
            format_clause: None,
            pipe_operators: Vec::new(),
        }))
    }
    enum Pending {
        Body(ast_owner::Owned<SetExpr>),
        Suffix {
            start: usize,
            query: ast_owner::Owned<Query>,
        },
    }
    let mut result: Vec<ast_owner::Owned<Query>> = vec![];
    let mut pending = vec![Pending::Body(ast_owner::Owned::new(body))];
    while let Some(next) = pending.pop() {
        check()?;
        match next {
            Pending::Suffix { start, mut query } => {
                let first = result.get_mut(start).ok_or_else(unsupported)?;
                ast_owner::replace_with(first, query.with.take());
                let tail = result.last_mut().ok_or_else(unsupported)?;
                if query.order_by.is_some() {
                    ast_owner::replace_order(tail, query.order_by.take());
                }
                if query.limit_clause.is_some() {
                    tail.limit_clause = query.limit_clause.take();
                }
            }
            Pending::Body(body) => {
                // Reject before detaching a foreign set operation so the
                // admitted finite AST owner also handles every error exit.
                if matches!(&*body, SetExpr::SetOperation { .. })
                    && !matches!(
                        &*body,
                        SetExpr::SetOperation {
                            op: SetOperator::Union,
                            set_quantifier: SetQuantifier::All,
                            ..
                        }
                    )
                {
                    return Err(problem(
                        "relational.invalid_syntax",
                        "Only UNION ALL is supported by the retrieval contract",
                    ));
                }
                match *body.into_box() {
                    SetExpr::SetOperation { left, right, .. } => {
                        pending.push(Pending::Body(ast_owner::Owned::new(right)));
                        pending.push(Pending::Body(ast_owner::Owned::new(left)));
                    }
                    SetExpr::Query(query) => {
                        let mut query = ast_owner::Owned::new(query);
                        if matches!(*query.body, SetExpr::SetOperation { .. }) {
                            let body = ast_owner::Owned::new(ast_owner::take_body(&mut query));
                            pending.push(Pending::Suffix {
                                start: result.len(),
                                query,
                            });
                            pending.push(Pending::Body(body));
                        } else {
                            result.push(query);
                        }
                    }
                    SetExpr::Select(select) => {
                        result.push(plain(Box::new(SetExpr::Select(select))))
                    }
                    _ => return Err(unsupported()),
                }
            }
        }
    }
    Ok(result)
}

fn execute_body(
    program: &Program,
    sources: &BTreeMap<String, &Source>,
    scope: subquery::Scope<'_>,
    parameters: &BTreeMap<String, Scalar>,
    work: &mut Work<'_>,
) -> Result<QueryResult> {
    if program.branches.is_empty() {
        return program.execute_select(sources, scope, parameters, work);
    }
    let mut combined = None::<Source>;
    for (index, branch) in program.branches.iter().enumerate() {
        // This is intentionally after the outer WITH definitions and after
        // every earlier branch, but before this branch's own WITH or source.
        // Go accepts these tokens at compile time and rejects them here in
        // both the real schema preflight and execution traversal.
        if index + 1 < program.branches.len()
            && (!branch.order.is_empty() || branch.limit.is_some() || branch.offset.is_some())
        {
            return Err(problem(
                "relational.invalid_syntax",
                "Non-final UNION ALL branch has a suffix",
            ));
        }
        let result = branch.execute_scoped_with(sources, scope, parameters, work)?;
        // Move retained rows; never deep-clone scalar payloads between branches.
        // The row-vector extension itself has a separate admission cost.
        work.retain(result.rows.len().checked_mul(24).ok_or_else(budget)?)?;
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
    // Ordinal owners are installed by the compound tail without changing
    // these raw public names (which may contain dots or escaped quotes).
    work.safety_retain(program.table.key().len() + 96)?;

    work.safety_retain(sources.keys().map(|name| name.len() + 96).sum())?;
    let mut inputs = sources.clone();
    inputs.insert(program.table.key().to_owned(), &combined);
    // Internal result columns may repeat. Project them by ordinal rather
    // than re-binding their names. ORDER BY still binds the first branch's
    // public names and must reject an ambiguous duplicate name.
    let result = program.execute_select_mode(
        &inputs,
        scope,
        parameters,
        work,
        ProjectionMode::CompoundOrdinals,
    )?;
    Ok(result)
}
