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
