//! Nonrecursive operations on admitted scalar trees. Query handles remain
//! shared during scalar copies; owned query equality uses its own work list.
use super::*;

/// Reverse evaluation order for a LIFO visitor. This never descends into a
/// query: each caller decides explicitly whether that scope is visible.
pub(super) fn children<'a>(term: &'a Term, pending: &mut Vec<&'a Term>) {
    match term {
        Term::Unary(_, t) | Term::NullTest(t, _) | Term::Cast(t, _) => pending.push(t),
        Term::Binary(a, _, b) | Term::Like(a, b, _) => pending.extend([&**b, &**a]),
        Term::Between(a, b, c, _) => pending.extend([&**c, &**b, &**a]),
        Term::In(t, args, _) => {
            pending.extend(args.iter().rev());
            pending.push(t);
        }
        Term::InSubquery(t, _, _) => pending.push(t),
        Term::Call(_, args, _) | Term::Aggregate(_, args, _) => pending.extend(args.iter().rev()),
        Term::Case(base, cases, other) => {
            pending.extend(other.as_deref());
            for (condition, result) in cases.iter().rev() {
                pending.extend([result, condition]);
            }
            pending.extend(base.as_deref());
        }
        _ => {}
    }
}

/// A visitor may replace/prune a node; otherwise its scalar children are
/// rebuilt in left-to-right order. Pending outputs use Term's iterative Drop.
pub(super) fn rewrite<E>(
    term: &Term,
    mut visit: impl FnMut(&Term) -> std::result::Result<Option<Term>, E>,
) -> std::result::Result<Term, E> {
    enum Task<'a> {
        Visit(&'a Term),
        Build(&'a Term, usize),
    }
    let mut pending = vec![Task::Visit(term)];
    let mut values = Vec::new();
    let mut children_buf = Vec::new();
    while let Some(task) = pending.pop() {
        match task {
            Task::Visit(term) => {
                if let Some(value) = visit(term)? {
                    values.push(value);
                    continue;
                }
                pending.push(Task::Build(term, values.len()));
                children(term, &mut children_buf);
                pending.extend(children_buf.drain(..).map(Task::Visit));
            }
            Task::Build(term, start) => {
                let children = values.split_off(start);
                values.push(rebuild(term, children));
            }
        }
    }
    // One Visit creates exactly one result, even if it prunes a subtree.
    Ok(values.pop().expect("one scalar tree result"))
}
fn rebuild(term: &Term, children: Vec<Term>) -> Term {
    let mut children = children.into_iter();
    let mut next = || children.next().expect("one result per scalar child");
    match term {
        Term::Value(v) => Term::Value(v.clone()),
        Term::Number(v) => Term::Number(v.clone()),
        Term::Ordinal(v) => Term::Ordinal(*v),
        Term::Column(v) => Term::Column(v.clone()),
        Term::Parameter(v) => Term::Parameter(v.clone()),
        Term::Star(v) => Term::Star(v.clone()),
        Term::Unary(op, _) => Term::Unary(*op, Box::new(next())),
        Term::Binary(_, op, _) => Term::Binary(Box::new(next()), op.clone(), Box::new(next())),
        Term::NullTest(_, negated) => Term::NullTest(Box::new(next()), *negated),
        Term::Cast(_, kind) => Term::Cast(Box::new(next()), kind.clone()),
        Term::Like(_, _, negated) => Term::Like(Box::new(next()), Box::new(next()), *negated),
        Term::Between(_, _, _, negated) => Term::Between(
            Box::new(next()),
            Box::new(next()),
            Box::new(next()),
            *negated,
        ),
        Term::In(_, _, negated) => Term::In(Box::new(next()), children.collect(), *negated),
        Term::InSubquery(_, query, negated) => {
            Term::InSubquery(Box::new(next()), query.clone(), *negated)
        }
        Term::ScalarSubquery(query) => Term::ScalarSubquery(query.clone()),
        Term::Exists(query) => Term::Exists(query.clone()),
        Term::Call(name, _, distinct) => Term::Call(name.clone(), children.collect(), *distinct),
        Term::Aggregate(name, _, distinct) => {
            Term::Aggregate(name.clone(), children.collect(), *distinct)
        }
        Term::Case(base, cases, other) => Term::Case(
            base.as_ref().map(|_| Box::new(next())),
            cases.iter().map(|_| (next(), next())).collect(),
            other.as_ref().map(|_| Box::new(next())),
        ),
    }
}
impl Clone for Term {
    fn clone(&self) -> Self {
        match rewrite(self, |_| Ok::<_, std::convert::Infallible>(None)) {
            Ok(value) => value,
            Err(never) => match never {},
        }
    }
}

impl Clone for Program {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Visit(&'a Program),
            Build(&'a Program, usize),
        }
        let mut pending = vec![Task::Visit(self)];
        let mut values: Vec<Program> = Vec::new();
        while let Some(task) = pending.pop() {
            match task {
                Task::Visit(query) => {
                    pending.push(Task::Build(query, values.len()));
                    pending.extend(
                        query
                            .joins
                            .iter()
                            .rev()
                            .filter_map(|j| j.derived.as_deref())
                            .map(Task::Visit),
                    );
                    pending.extend(query.derived.as_deref().map(Task::Visit));
                    pending.extend(query.branches.iter().rev().map(Task::Visit));
                    pending.extend(
                        query
                            .bindings
                            .iter()
                            .rev()
                            .map(|(_, query)| Task::Visit(query)),
                    );
                }
                Task::Build(query, start) => {
                    let mut children = values.split_off(start).into_iter();
                    let mut next = || children.next().expect("one result per owned query child");
                    let output = Program {
                        explain: query.explain,
                        analyze: query.analyze,
                        recursive: query.recursive,
                        bindings: query
                            .bindings
                            .iter()
                            .map(|(name, _)| (name.clone(), next()))
                            .collect(),
                        branches: query.branches.iter().map(|_| next()).collect(),
                        derived: query.derived.as_ref().map(|_| Box::new(next())),
                        table: query.table.clone(),
                        alias: query.alias.clone(),
                        projection: query.projection.clone(),
                        filter: query.filter.clone(),
                        order: query.order.clone(),
                        parameters: query.parameters.clone(),
                        limit: query.limit.clone(),
                        offset: query.offset.clone(),
                        distinct: query.distinct,
                        joins: query
                            .joins
                            .iter()
                            .map(|join| joins::Link {
                                table: join.table.clone(),
                                alias: join.alias.clone(),
                                derived: join.derived.as_ref().map(|_| Box::new(next())),
                                left: join.left,
                                condition: join.condition.clone(),
                            })
                            .collect(),
                        groups: query.groups.clone(),
                        having: query.having.clone(),
                    };
                    values.push(output);
                }
            }
        }
        values.pop().expect("one owned query result")
    }
}
