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
