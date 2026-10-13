//! Lexical CTE scopes borrow physical rows and retain only computed results.
//! All definitions run under the caller's original work/memory/cancel budget.
use super::*;

pub(super) fn validate_all_keyword(sql: &str) -> Result<()> {
    use sqlparser::tokenizer::{Token, Tokenizer};
    let tokens = Tokenizer::new(&GenericDialect {}, sql)
        .tokenize_with_location()
        .map_err(|e| problem("relational.invalid_syntax", &e.to_string()))?;
    let tokens: Vec<_> = tokens
        .iter()
        .filter(|t| !matches!(t.token, Token::Whitespace(_)))
        .collect();
    for (i, token) in tokens.iter().enumerate() {
        if !matches!(&token.token, Token::Word(word) if word.quote_style.is_none() && word.value.eq_ignore_ascii_case("UNION"))
        {
            continue;
        }
        if tokens.get(i + 1).is_some_and(|t| matches!(&t.token, Token::Word(word) if word.quote_style.is_none() && word.value.eq_ignore_ascii_case("ALL"))) {
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

pub(super) fn dependencies<'a>(
    program: &'a Program,
    outer: &BTreeSet<String>,
    tables: &mut Vec<&'a str>,
) {
    let mut names = outer.clone();
    for (name, _) in &program.bindings {
        names.insert(name.to_ascii_lowercase());
    }
    for (_, body) in &program.bindings {
        dependencies(body, &names, tables);
    }
    if !program.branches.is_empty() {
        for body in &program.branches {
            dependencies(body, &names, tables);
        }
        return;
    }
    for (table, derived) in std::iter::once((&program.table, &program.derived))
        .chain(program.joins.iter().map(|j| (&j.table, &j.derived)))
    {
        if let Some(body) = derived {
            dependencies(body, &names, tables);
            continue;
        }
        if !names.contains(&table.to_ascii_lowercase())
            && !tables.iter().any(|n| n.eq_ignore_ascii_case(table))
        {
            tables.push(table);
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
    work.retain(bytes)?;
    let mut map = BTreeMap::new();
    for (name, source) in inputs {
        if !reserved.contains(&name.to_ascii_lowercase()) {
            map.insert(name.clone(), *source);
        }
    }
    for (name, source) in results {
        map.insert(name.clone(), source);
    }
    Ok(map)
}

pub(super) fn execute(
    program: &Program,
    sources: &BTreeMap<String, &Source>,
    parameters: &BTreeMap<String, Scalar>,
    work: &mut Work<'_>,
) -> Result<QueryResult> {
    if program.bindings.is_empty()
        && program.derived.is_none()
        && program.joins.iter().all(|j| j.derived.is_none())
    {
        return execute_body(program, sources, parameters, work);
    }
    work.retain(program.bindings.iter().map(|(n, _)| n.len() + 96).sum())?;
    // Reserve every local name before any definition runs: a self or forward
    // reference must fail binding rather than resolve a physical/outer table.
    let mut reserved: BTreeSet<_> = program
        .bindings
        .iter()
        .map(|(n, _)| n.to_ascii_lowercase())
        .collect();
    reserved.extend(
        std::iter::once((&program.table, &program.derived))
            .chain(program.joins.iter().map(|j| (&j.table, &j.derived)))
            .filter(|(_, body)| body.is_some())
            .map(|(name, _)| name.to_ascii_lowercase()),
    );
    let mut results = BTreeMap::new();
    for (name, body) in &program.bindings {
        work.spend()?;
        let inputs = visible(sources, &reserved, &results, work)?;
        let result = if program.recursive && recursive::references(body, name) > 0 {
            recursive::execute(name, body, &inputs, parameters, work)?
        } else {
            body.execute_scoped(&inputs, parameters, work)?
        };
        work.retain(name.len() + 96)?;
        results.insert(
            name.clone(),
            Source {
                columns: result.columns,
                rows: result.rows,
                complete: result.complete,
            },
        );
    }
    for (name, body) in std::iter::once((&program.table, &program.derived))
        .chain(program.joins.iter().map(|j| (&j.table, &j.derived)))
    {
        let Some(body) = body else {
            continue;
        };
        work.spend()?;
        let inputs = visible(sources, &reserved, &results, work)?;
        let result = body.execute_scoped(&inputs, parameters, work)?;
        work.retain(name.len() + 96)?;
        results.insert(
            name.clone(),
            Source {
                columns: result.columns,
                rows: result.rows,
                complete: result.complete,
            },
        );
    }
    let inputs = visible(sources, &reserved, &results, work)?;
    let mut result = execute_body(program, &inputs, parameters, work)?;
    result.complete &= results.values().all(|s| s.complete);
    Ok(result)
}

pub(super) fn branches(body: SetExpr) -> Result<Vec<SetExpr>> {
    fn walk(body: SetExpr, result: &mut Vec<SetExpr>) -> Result<()> {
        match body {
            SetExpr::SetOperation {
                op: SetOperator::Union,
                set_quantifier: SetQuantifier::All,
                left,
                right,
            } => {
                walk(*left, result)?;
                walk(*right, result)
            }
            SetExpr::SetOperation { .. } => Err(problem(
                "relational.invalid_syntax",
                "Only UNION ALL is supported by the retrieval contract",
            )),
            SetExpr::Select(_) => {
                if result.len() >= 128 {
                    return Err(budget());
                }
                result.push(body);
                Ok(())
            }
            _ => Err(unsupported()),
        }
    }
    let mut result = vec![];
    walk(body, &mut result)?;
    Ok(result)
}

fn execute_body(
    program: &Program,
    sources: &BTreeMap<String, &Source>,
    parameters: &BTreeMap<String, Scalar>,
    work: &mut Work<'_>,
) -> Result<QueryResult> {
    if program.branches.is_empty() {
        return program.execute_select(sources, parameters, work);
    }
    let mut combined = None::<Source>;
    for branch in &program.branches {
        work.spend()?;
        let result = branch.execute_scoped(sources, parameters, work)?;
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
    work.retain(program.table.len() + 96)?;
    let inputs = BTreeMap::from([(program.table.clone(), &combined)]);
    // The synthetic output select binds ORDER BY against first-branch column
    // names and applies LIMIT/OFFSET once after the full union is validated.
    program.execute_select(&inputs, parameters, work)
}
