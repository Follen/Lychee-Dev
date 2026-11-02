//! Newly reconstructed constrained typed correlation algebra. This is not a
//! recovered byte-identical copy of the lost sealed scope normalizer.
//! Only safe equality bridges and outer-only one-column projections are lifted.
use super::super::api::{BinaryOperator, Expression, NamedExpression, QueryPlan, WhenThen};
use std::collections::BTreeMap;

pub(super) fn normalize(plan: &QueryPlan) -> QueryPlan { normalize_plan(plan.clone()) }
fn normalize_named(mut item: NamedExpression) -> NamedExpression { item.expression = normalize_expression(item.expression); item }
fn normalize_plan(plan: QueryPlan) -> QueryPlan {
    match plan {
        QueryPlan::UnionAll { inputs } => QueryPlan::UnionAll { inputs: inputs.into_iter().map(normalize_plan).collect() },
        QueryPlan::Iterate { binding, seed, step } => QueryPlan::Iterate { binding, seed: Box::new(normalize_plan(*seed)), step: Box::new(normalize_plan(*step)) },
        QueryPlan::Filter { input, predicate } => QueryPlan::Filter { input: Box::new(normalize_plan(*input)), predicate: normalize_expression(predicate) },
        QueryPlan::Project { input, columns } => QueryPlan::Project { input: Box::new(normalize_plan(*input)), columns: columns.into_iter().map(normalize_named).collect() },
        QueryPlan::Join { left, right, kind, on } => QueryPlan::Join { left: Box::new(normalize_plan(*left)), right: Box::new(normalize_plan(*right)), kind, on },
        QueryPlan::Aggregate { input, groups, metrics } => QueryPlan::Aggregate {
            input: Box::new(normalize_plan(*input)), groups: groups.into_iter().map(normalize_named).collect(),
            metrics: metrics.into_iter().map(|mut metric| { metric.expression = metric.expression.map(normalize_expression); metric }).collect(),
        },
        QueryPlan::Sort { input, keys } => QueryPlan::Sort {
            input: Box::new(normalize_plan(*input)), keys: keys.into_iter().map(|mut key| { key.expression = normalize_expression(key.expression); key }).collect(),
        },
        QueryPlan::Limit { input, offset, count } => QueryPlan::Limit { input: Box::new(normalize_plan(*input)), offset, count },
        leaf => leaf,
    }
}
fn map_expression_children(value: Expression, mut map: impl FnMut(Expression) -> Expression) -> Expression {
    match value {
        Expression::Binary { left, operator, right } => Expression::Binary { left: Box::new(map(*left)), operator, right: Box::new(map(*right)) },
        Expression::Not { value } => Expression::Not { value: Box::new(map(*value)) },
        Expression::IsNull { value } => Expression::IsNull { value: Box::new(map(*value)) },
        Expression::In { value, values } => Expression::In { value: Box::new(map(*value)), values },
        Expression::Cast { value, target } => Expression::Cast { value: Box::new(map(*value)), target },
        Expression::Like { value, pattern, negated } => Expression::Like { value: Box::new(map(*value)), pattern: Box::new(map(*pattern)), negated },
        Expression::Between { value, low, high, negated } => Expression::Between { value: Box::new(map(*value)), low: Box::new(map(*low)), high: Box::new(map(*high)), negated },
        Expression::Case { branches, otherwise } => Expression::Case {
            branches: branches.into_iter().map(|branch| WhenThen { when: map(branch.when), then: map(branch.then) }).collect(),
            otherwise: otherwise.map(|value| Box::new(map(*value))),
        },
        Expression::Coalesce { values } => Expression::Coalesce { values: values.into_iter().map(&mut map).collect() },
        Expression::NullIf { left, right } => Expression::NullIf { left: Box::new(map(*left)), right: Box::new(map(*right)) },
        Expression::Call { function, arguments } => Expression::Call { function, arguments: arguments.into_iter().map(&mut map).collect() },
        Expression::Index { value, index } => Expression::Index { value: Box::new(map(*value)), index },
        Expression::InQuery { value, input, negated } => Expression::InQuery { value: Box::new(map(*value)), input, negated },
        leaf => leaf,
    }
}
fn normalize_expression(value: Expression) -> Expression {
    match value {
        Expression::InQuery { value, input, negated } => {
            let value = normalize_expression(*value);
            let input = normalize_plan(*input);
            rewrite_membership(value, input, negated)
        }
        Expression::ExistsQuery { input, negated } => Expression::ExistsQuery { input: Box::new(normalize_plan(*input)), negated },
        Expression::ScalarQuery { input } => Expression::ScalarQuery { input: Box::new(normalize_plan(*input)) },
        value => map_expression_children(value, normalize_expression),
    }
}
fn truth(value: bool) -> Expression { Expression::Literal { value: serde_json::Value::Bool(value) } }
fn equal(left: Expression, right: Expression) -> Expression {
    Expression::Binary { left: Box::new(left), operator: BinaryOperator::Equal, right: Box::new(right) }
}
fn conjunction(mut values: Vec<Expression>) -> Expression {
    if values.is_empty() { return truth(true); }
    while values.len() > 1 {
        let mut reduced = Vec::with_capacity(values.len().div_ceil(2));
        let mut values_iter = values.into_iter();
        while let Some(left) = values_iter.next() {
            reduced.push(if let Some(right) = values_iter.next() {
                Expression::Binary { left: Box::new(left), operator: BinaryOperator::And, right: Box::new(right) }
            } else { left });
        }
        values = reduced;
    }
    values.remove(0)
}
fn gate(condition: Expression, value: Expression, negated: bool) -> Expression {
    // UNKNOWN condition means the original RHS is empty: even NULL IN empty is
    // false and NULL NOT IN empty is true. Boolean AND would be incorrect.
    Expression::Case { branches: vec![WhenThen { when: condition, then: value }], otherwise: Some(Box::new(truth(negated))) }
}
fn rewrite_membership(value: Expression, input: QueryPlan, negated: bool) -> Expression {
    if let Some((projected, rows)) = outer_projected(&input) {
        let comparison = equal(value, shift_out(projected));
        let comparison = if negated { Expression::Not { value: Box::new(comparison) } } else { comparison };
        return gate(Expression::ExistsQuery { input: Box::new(rows), negated: false }, comparison, negated);
    }
    if !liftable_chain(&input) { return Expression::InQuery { value: Box::new(value), input: Box::new(input), negated }; }
    let (input, gates) = lift_chain(input);
    let membership = Expression::InQuery { value: Box::new(value), input: Box::new(input), negated };
    if gates.is_empty() { membership } else { gate(conjunction(gates), membership, negated) }
}
fn liftable_chain(plan: &QueryPlan) -> bool {
    match plan {
        QueryPlan::Project { input, .. } | QueryPlan::Filter { input, .. } => liftable_chain(input),
        QueryPlan::Scan { .. } | QueryPlan::Lookup { .. } | QueryPlan::Delta { .. } => true,
        _ => false,
    }
}
fn split_conjunction(value: Expression, output: &mut Vec<Expression>) {
    match value {
        Expression::Binary { left, operator: BinaryOperator::And, right } => {
            split_conjunction(*left, output); split_conjunction(*right, output);
        }
        value => output.push(value),
    }
}
fn column_outer(value: &Expression) -> Option<(&str, &str, usize)> {
    let Expression::Binary { left, operator: BinaryOperator::Equal, right } = value else { return None; };
    match (left.as_ref(), right.as_ref()) {
        (Expression::Column { name: local }, Expression::OuterColumn { name, depth })
        | (Expression::OuterColumn { name, depth }, Expression::Column { name: local }) => Some((local, name, *depth)),
        _ => None,
    }
}
fn outer_leaf(value: &Expression) -> bool { matches!(value, Expression::OuterColumn { .. } | Expression::Literal { .. }) }
fn outer_equality(value: &Expression) -> bool {
    match value {
        Expression::Binary { left, operator: BinaryOperator::Equal, right } => outer_leaf(left) && outer_leaf(right)
            && (matches!(left.as_ref(), Expression::OuterColumn { .. }) || matches!(right.as_ref(), Expression::OuterColumn { .. })),
        _ => false,
    }
}
fn lift_filter(predicate: Expression) -> (Option<Expression>, Vec<Expression>) {
    let mut terms = Vec::new();
    split_conjunction(predicate, &mut terms);
    let bridges = terms.iter().filter_map(|term| column_outer(term).filter(|(_, _, depth)| *depth == 1)
        .map(|(local, outer, _)| (local.to_owned(), outer.to_owned()))).collect::<BTreeMap<_, _>>();
    let mut retained = Vec::new();
    let mut gates = Vec::new();
    for term in terms {
        if outer_equality(&term) { gates.push(shift_out(term)); continue; }
        if let Some((local, ancestor, depth)) = column_outer(&term) {
            if depth > 1 {
                if let Some(immediate) = bridges.get(local) {
                    gates.push(equal(Expression::Column { name: immediate.clone() }, Expression::OuterColumn { name: ancestor.to_owned(), depth: depth - 1 }));
                    continue;
                }
            }
        }
        retained.push(term);
    }
    (if retained.is_empty() { None } else { Some(conjunction(retained)) }, gates)
}
fn lift_chain(plan: QueryPlan) -> (QueryPlan, Vec<Expression>) {
    match plan {
        QueryPlan::Project { input, columns } => {
            let (input, gates) = lift_chain(*input);
            (QueryPlan::Project { input: Box::new(input), columns }, gates)
        }
        QueryPlan::Filter { input, predicate } => {
            let (input, mut gates) = lift_chain(*input);
            let (predicate, added) = lift_filter(predicate);
            gates.extend(added);
            (match predicate { Some(predicate) => QueryPlan::Filter { input: Box::new(input), predicate }, None => input }, gates)
        }
        leaf => (leaf, Vec::new()),
    }
}
fn shift_out(value: Expression) -> Expression {
    match value {
        Expression::OuterColumn { name, depth: 1 } => Expression::Column { name },
        Expression::OuterColumn { name, depth } => Expression::OuterColumn { name, depth: depth.saturating_sub(1) },
        value => map_expression_children(value, shift_out),
    }
}
fn outer_only(value: &Expression) -> bool {
    let mut valid = true;
    let mut found = false;
    let mut pending = vec![super::Node::Expression(value)];
    while let Some(node) = pending.pop() {
        match node {
            super::Node::Expression(Expression::Column { .. } | Expression::InQuery { .. } | Expression::ExistsQuery { .. } | Expression::ScalarQuery { .. }) => { valid = false; break; }
            super::Node::Expression(Expression::OuterColumn { .. }) => found = true,
            _ => {},
        }
        super::children(node, &mut pending);
    }
    valid && found
}
/// Direct forwarding projections may rename an outer-only value; keep the
/// complete row-producing input, including grouping/limit, inside EXISTS.
fn outer_projected(plan: &QueryPlan) -> Option<(Expression, QueryPlan)> {
    let QueryPlan::Project { input, columns } = plan else { return None; };
    if columns.len() != 1 { return None; }
    let expression = &columns[0].expression;
    if outer_only(expression) { return Some((expression.clone(), input.as_ref().clone())); }
    let Expression::Column { name } = expression else { return None; };
    let QueryPlan::Project { columns: inner, .. } = input.as_ref() else { return None; };
    if inner.len() != 1 || name != &inner[0].name { return None; }
    outer_projected(input)
}
