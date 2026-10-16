//! Structural equality uses borrowed-node pairs, including query edges. Group
//! column equivalence is local to the expression, never applied inside a query.
use super::*;
enum Pair<'a> {
    Term(&'a Term, &'a Term, bool),
    Query(&'a Program, &'a Program),
}
impl PartialEq for Term {
    fn eq(&self, other: &Self) -> bool {
        compare(vec![Pair::Term(self, other, false)], None)
    }
}
impl PartialEq for Program {
    fn eq(&self, other: &Self) -> bool {
        compare(vec![Pair::Query(self, other)], None)
    }
}
pub(super) fn grouped(a: &Term, b: &Term, context: &subquery::Context<'_>) -> bool {
    compare(vec![Pair::Term(a, b, true)], Some(context))
}
fn optional<'a>(
    a: Option<&'a Term>,
    b: Option<&'a Term>,
    grouped: bool,
    pending: &mut Vec<Pair<'a>>,
) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            pending.push(Pair::Term(a, b, grouped));
            true
        }
        (None, None) => true,
        _ => false,
    }
}
fn terms<'a>(a: &'a [Term], b: &'a [Term], grouped: bool, pending: &mut Vec<Pair<'a>>) -> bool {
    if a.len() != b.len() {
        return false;
    }
    pending.extend(
        a.iter()
            .zip(b)
            .rev()
            .map(|(a, b)| Pair::Term(a, b, grouped)),
    );
    true
}
fn compare(mut pending: Vec<Pair<'_>>, context: Option<&subquery::Context<'_>>) -> bool {
    while let Some(pair) = pending.pop() {
        match pair {
            Pair::Term(a, b, grouped) => {
                let mut child = |a, b| pending.push(Pair::Term(a, b, grouped));
                match (a, b) {
                    (Term::Value(a), Term::Value(b)) if a == b => {}
                    (Term::Number(a), Term::Number(b))
                    | (Term::Parameter(a), Term::Parameter(b))
                    | (Term::Star(a), Term::Star(b))
                        if a == b => {}
                    (Term::Ordinal(a), Term::Ordinal(b)) if a == b => {}
                    (Term::Column(a), Term::Column(b)) => {
                        if a == b {
                            continue;
                        }
                        if !grouped || !equal_fold(&a.name, &b.name) {
                            return false;
                        }
                        let Some(context) = context else {
                            return false;
                        };
                        let Some(group) = context.group else {
                            return false;
                        };
                        match (
                            subquery::resolve(group.columns, group.alias, a, context.scope.outer),
                            subquery::resolve(group.columns, group.alias, b, context.scope.outer),
                        ) {
                            (
                                Ok(subquery::Position::Local(a)),
                                Ok(subquery::Position::Local(b)),
                            ) if a == b => {}
                            (
                                Ok(subquery::Position::Outer(a, ai)),
                                Ok(subquery::Position::Outer(b, bi)),
                            ) if std::ptr::eq(a, b) && ai == bi => {}
                            _ => return false,
                        }
                    }
                    (Term::Unary(a, x), Term::Unary(b, y)) if a == b => child(x, y),
                    (Term::Binary(a, op, b), Term::Binary(c, other, d)) if op == other => {
                        child(b, d);
                        child(a, c);
                    }
                    (Term::Cast(a, k), Term::Cast(b, l)) if k == l => child(a, b),
                    (Term::NullTest(a, n), Term::NullTest(b, m)) if n == m => child(a, b),
                    (Term::Like(a, b, n), Term::Like(c, d, m)) if n == m => {
                        child(b, d);
                        child(a, c);
                    }
                    (Term::Between(a, b, c, n), Term::Between(d, e, f, m)) if n == m => {
                        child(c, f);
                        child(b, e);
                        child(a, d);
                    }
                    (Term::In(a, x, n), Term::In(b, y, m)) if n == m => {
                        child(a, b);
                        if !terms(x, y, grouped, &mut pending) {
                            return false;
                        }
                    }
                    (Term::InSubquery(a, x, n), Term::InSubquery(b, y, m)) if n == m => {
                        child(a, b);
                        pending.push(Pair::Query(x, y));
                    }
                    (Term::ScalarSubquery(a), Term::ScalarSubquery(b))
                    | (Term::Exists(a), Term::Exists(b)) => pending.push(Pair::Query(a, b)),
                    (Term::Call(a, x, d), Term::Call(b, y, e))
                    | (Term::Aggregate(a, x, d), Term::Aggregate(b, y, e))
                        if a == b && d == e =>
                    {
                        if !terms(x, y, grouped, &mut pending) {
                            return false;
                        }
                    }
                    (Term::Case(a, x, b), Term::Case(c, y, d)) => {
                        if x.len() != y.len()
                            || !optional(a.as_deref(), c.as_deref(), grouped, &mut pending)
                            || !optional(b.as_deref(), d.as_deref(), grouped, &mut pending)
                        {
                            return false;
                        }
                        for ((a, b), (c, d)) in x.iter().zip(y).rev() {
                            pending.extend([Pair::Term(b, d, grouped), Pair::Term(a, c, grouped)]);
                        }
                    }
                    _ => return false,
                }
            }
            Pair::Query(a, b) => {
                if a.explain != b.explain
                    || a.analyze != b.analyze
                    || a.recursive != b.recursive
                    || a.table != b.table
                    || a.alias != b.alias
                    || a.parameters != b.parameters
                    || a.distinct != b.distinct
                    || a.bindings.len() != b.bindings.len()
                    || a.branches.len() != b.branches.len()
                    || a.projection.len() != b.projection.len()
                    || a.order.len() != b.order.len()
                    || a.joins.len() != b.joins.len()
                {
                    return false;
                }
                for ((an, aq), (bn, bq)) in a.bindings.iter().zip(&b.bindings) {
                    if an != bn {
                        return false;
                    }
                    pending.push(Pair::Query(aq, bq));
                }
                for (a, b) in a.branches.iter().zip(&b.branches) {
                    pending.push(Pair::Query(a, b));
                }
                match (&a.derived, &b.derived) {
                    (Some(a), Some(b)) => pending.push(Pair::Query(a, b)),
                    (None, None) => {}
                    _ => return false,
                }
                for (a, b) in a.projection.iter().zip(&b.projection) {
                    if a.name != b.name
                        || !optional(a.expr.as_ref(), b.expr.as_ref(), false, &mut pending)
                    {
                        return false;
                    }
                }
                for (a, b) in [
                    (&a.filter, &b.filter),
                    (&a.limit, &b.limit),
                    (&a.offset, &b.offset),
                    (&a.having, &b.having),
                ] {
                    if !optional(a.as_ref(), b.as_ref(), false, &mut pending) {
                        return false;
                    }
                }
                for ((a, ad, an), (b, bd, bn)) in a.order.iter().zip(&b.order) {
                    if ad != bd || an != bn {
                        return false;
                    }
                    pending.push(Pair::Term(a, b, false));
                }
                for (a, b) in a.joins.iter().zip(&b.joins) {
                    if a.table != b.table
                        || a.alias != b.alias
                        || a.left != b.left
                        || !optional(
                            a.condition.as_ref(),
                            b.condition.as_ref(),
                            false,
                            &mut pending,
                        )
                    {
                        return false;
                    }
                    match (&a.derived, &b.derived) {
                        (Some(a), Some(b)) => pending.push(Pair::Query(a, b)),
                        (None, None) => {}
                        _ => return false,
                    }
                }
                if !terms(&a.groups, &b.groups, false, &mut pending) {
                    return false;
                }
            }
        }
    }
    true
}
