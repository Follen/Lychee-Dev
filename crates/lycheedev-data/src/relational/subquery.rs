//! Expression-local query scopes. Frames borrow the current callback/group
//! row; neither frames nor short-lived lexical Sources enter the shared Work.
//! Go 3.2.3 completes each expression query on each evaluation.
use super::*;

/// Bound total compiled syntax after removing the old 1024-column/group
/// caps. This is a conservative structural admission, not Go work accounting.
pub(super) fn validate_nodes(program: &Program) -> Result<()> {
    enum Node<'a> {
        Query(&'a Program),
        Term(&'a Term),
    }
    let mut pending = vec![Node::Query(program)];
    let mut count = 0usize;
    while let Some(node) = pending.pop() {
        count += 1;
        if count > 32768 {
            return Err(budget());
        }
        match node {
            Node::Query(query) => {
                // Physical/derived relation nodes and wildcard projections.
                if query.branches.is_empty() {
                    count += 1
                        + query.joins.len()
                        + query.projection.iter().filter(|p| p.expr.is_none()).count();
                } else {
                    // Rust's synthetic compound wrapper is not a Go retrieval;
                    // the actual branches carry those query/relation nodes.
                    count -= 1;
                }
                for (_, body) in &query.bindings {
                    pending.push(Node::Query(body));
                }
                for body in &query.branches {
                    pending.push(Node::Query(body));
                }
                if let Some(body) = &query.derived {
                    pending.push(Node::Query(body));
                }
                for link in &query.joins {
                    if let Some(body) = &link.derived {
                        pending.push(Node::Query(body));
                    }
                }
                pending.extend(query.terms().map(Node::Term));
            }
            Node::Term(term) => match term {
                Term::ScalarSubquery(query) | Term::Exists(query) => {
                    pending.push(Node::Query(query));
                }
                Term::InSubquery(input, query, _) => {
                    pending.push(Node::Query(query));
                    pending.push(Node::Term(input));
                }
                Term::Unary(_, t) | Term::NullTest(t, _) | Term::Cast(t, _) => {
                    pending.push(Node::Term(t))
                }
                Term::Binary(a, _, b) | Term::Like(a, b, _) => {
                    pending.push(Node::Term(a));
                    pending.push(Node::Term(b));
                }
                Term::Between(a, b, c, _) => {
                    pending.push(Node::Term(a));
                    pending.push(Node::Term(b));
                    pending.push(Node::Term(c));
                }
                Term::In(t, args, _) => {
                    pending.push(Node::Term(t));
                    pending.extend(args.iter().map(Node::Term));
                }
                Term::Call(_, args, _) | Term::Aggregate(_, args, _) => {
                    pending.extend(args.iter().map(Node::Term))
                }
                Term::Case(base, cases, other) => {
                    pending.extend(base.iter().chain(other.iter()).map(|t| Node::Term(t)));
                    for (a, b) in cases {
                        pending.push(Node::Term(a));
                        pending.push(Node::Term(b));
                    }
                }
                _ => {}
            },
        }
        if count.checked_add(pending.len()).is_none_or(|n| n > 32768) {
            return Err(budget());
        }
    }
    Ok(())
}

pub(super) fn cardinality() -> lycheedev_core::ToolkitError {
    problem(
        "relational.invalid_cardinality",
        "Subquery must return one column and scalar subqueries at most one row",
    )
}
#[derive(Clone, Copy)]
pub(super) struct Fields<'a> {
    pub columns: &'a [String],
    pub alias: ColumnOwners<'a>,
}
impl Fields<'_> {
    fn part(&self, index: usize) -> Result<(&str, &str)> {
        let column = self.columns.get(index).ok_or_else(binding)?;
        Ok((self.alias.owner(index), column.as_str()))
    }
}
#[derive(Clone, Copy)]
pub(super) struct Correlation<'a> {
    pub columns: &'a [String],
    pub alias: ColumnOwners<'a>,
    pub mapping: &'a [Option<usize>],
}
/// Go's saved evaluator state has a callback independently of an outer row.
/// A compound tail keeps this inherited callback, lexical scope and fields;
/// its current row resolver is installed separately by executeRows.
#[derive(Clone, Copy)]
pub(super) struct Scope<'a> {
    pub outer: Option<&'a Frame<'a>>,
    pub callback: Option<&'a BTreeMap<String, &'a Source>>,
    pub fields: Fields<'a>,
    pub allowed: Option<Correlation<'a>>,
}
impl Scope<'_> {
    pub(super) fn root() -> Self {
        Self {
            outer: None,
            callback: None,
            fields: Fields {
                columns: &[],
                alias: ColumnOwners::Single(""),
            },
            allowed: None,
        }
    }
}
impl<'a> Scope<'a> {
    pub(super) fn select<'b>(self, sources: &'b BTreeMap<String, &'b Source>) -> Scope<'b>
    where
        'a: 'b,
    {
        Scope {
            outer: self.outer,
            callback: Some(sources),
            fields: self.fields,
            allowed: None,
        }
    }
}
pub(super) struct Frame<'a> {
    row: &'a [Scalar],
    fields: Fields<'a>,
    resolver: Fields<'a>,
    mapping: Option<&'a [Option<usize>]>,
    allowed: Option<Correlation<'a>>,
    parent: Option<&'a Frame<'a>>,
}
impl Frame<'_> {
    pub(super) fn value(&self, index: usize) -> Result<&Scalar> {
        // A UNION-tail callback may bind using inherited outer fields while
        // its current resolver owns numeric-qualified compound columns.
        let (qualifier, name) = self.fields.part(index)?;
        let index = local_parts(self.resolver.columns, self.resolver.alias, qualifier, name)?
            .ok_or_else(binding)?;
        let index = self.mapping.map_or(Ok(index), |mapping| {
            mapping.get(index).copied().flatten().ok_or_else(binding)
        })?;
        self.row.get(index).ok_or_else(binding)
    }
    fn check_allowed(&self, index: usize) -> Result<()> {
        let Some(allowed) = self.allowed else {
            return Ok(());
        };
        let field = self.fields.part(index)?;
        let fields = Fields {
            columns: allowed.columns,
            alias: allowed.alias,
        };
        // Go groupAllowed is a map keyed by canonical field identity, not by
        // the ordinal of whatever schema a nested bind temporarily installs.
        for (index, entry) in allowed.mapping.iter().enumerate() {
            if entry.is_some() && fields.part(index)? == field {
                return Ok(());
            }
        }
        Err(binding())
    }
}
#[derive(Clone, Copy)]
pub(super) struct Context<'a> {
    pub scope: Scope<'a>,
    pub group: Option<Correlation<'a>>,
    pub complete: Option<&'a std::cell::Cell<bool>>,
}
impl<'a> Context<'a> {
    pub(super) fn new(
        sources: &'a BTreeMap<String, &'a Source>,
        outer: Option<&'a Frame<'a>>,
    ) -> Self {
        Self {
            scope: Scope {
                outer,
                ..Scope::root()
            }
            .select(sources),
            group: None,
            complete: None,
        }
    }
}
pub(super) enum Position<'a> {
    Local(usize),
    Outer(&'a Frame<'a>, usize),
}
fn local(columns: &[String], alias: ColumnOwners<'_>, name: &ColumnRef) -> Result<Option<usize>> {
    local_parts(columns, alias, &name.qualifier, &name.name)
}
fn local_parts(
    columns: &[String],
    alias: ColumnOwners<'_>,
    qualifier: &str,
    field: &str,
) -> Result<Option<usize>> {
    let mut found = None;
    let mut owned = false;
    for (index, column) in columns.iter().enumerate() {
        let owner = alias.owner(index);
        if !qualifier.is_empty() && !equal_fold(qualifier, owner) {
            continue;
        }
        owned |= !qualifier.is_empty();
        if !equal_fold(field, column) {
            continue;
        }
        owned = true;
        if found.replace(index).is_some() {
            return Err(binding());
        }
    }
    if owned && found.is_none() {
        let name = ColumnRef::qualified(qualifier, field);
        return Err(missing_column(columns, alias, &name));
    }
    Ok(found)
}
pub(super) fn resolve<'a>(
    columns: &[String],
    alias: ColumnOwners<'_>,
    name: &ColumnRef,
    mut outer: Option<&'a Frame<'a>>,
) -> Result<Position<'a>> {
    if let Some(index) = local(columns, alias, name)? {
        return Ok(Position::Local(index));
    }
    while let Some(frame) = outer {
        if let Some(index) = local(frame.fields.columns, frame.fields.alias, name)? {
            frame.check_allowed(index)?;
            return Ok(Position::Outer(frame, index));
        }
        outer = frame.parent;
    }
    Err(missing_column(columns, alias, name))
}
pub(super) fn display_column(
    columns: &[String],
    alias: ColumnOwners<'_>,
    name: &ColumnRef,
    outer: Option<&Frame<'_>>,
) -> Result<Option<String>> {
    let Position::Local(index) = resolve(columns, alias, name, outer)? else {
        // Go's bound outer-column node is an expression, not a local column.
        return Ok(None);
    };
    Ok(Some(columns[index].clone()))
}

pub(super) fn execute(
    query: &Program,
    row: &[Scalar],
    columns: &[String],
    alias: ColumnOwners<'_>,
    parameters: &BTreeMap<String, Scalar>,
    context: &Context<'_>,
    work: &mut Work<'_>,
) -> Result<QueryResult> {
    let result = execute_any(query, row, columns, alias, parameters, context, work)?;
    if result.columns.len() != 1 {
        return Err(cardinality());
    }
    Ok(result)
}
pub(super) fn execute_any(
    query: &Program,
    row: &[Scalar],
    columns: &[String],
    alias: ColumnOwners<'_>,
    parameters: &BTreeMap<String, Scalar>,
    context: &Context<'_>,
    work: &mut Work<'_>,
) -> Result<QueryResult> {
    work.consume(0)?;
    let sources = context.scope.callback.ok_or_else(unsupported)?;
    let resolver = context
        .group
        .map_or(Fields { columns, alias }, |group| Fields {
            columns: group.columns,
            alias: group.alias,
        });
    let fields = if work.planning {
        resolver
    } else {
        context.scope.fields
    };
    let allowed = if work.planning {
        context.group.or(context.scope.allowed)
    } else {
        context.scope.allowed
    };
    let frame = Frame {
        row,
        fields,
        resolver,
        mapping: context.group.map(|group| group.mapping),
        allowed,
        parent: context.scope.outer,
    };
    let scope = Scope {
        outer: Some(&frame),
        fields,
        allowed,
        ..context.scope
    };
    let result = query.execute_scoped_with(sources, scope, parameters, work)?;
    if !result.complete
        && let Some(complete) = context.complete
    {
        complete.set(false);
    }
    Ok(result)
}
pub(super) fn visit_queries<'a>(term: &'a Term, emit: &mut impl FnMut(&'a Program)) {
    match term {
        Term::ScalarSubquery(query) | Term::Exists(query) => emit(query),
        Term::InSubquery(input, query, _) => {
            emit(query);
            visit_queries(input, emit);
        }
        Term::Unary(_, term) | Term::NullTest(term, _) | Term::Cast(term, _) => {
            visit_queries(term, emit)
        }
        Term::Binary(a, _, b) | Term::Like(a, b, _) => {
            visit_queries(a, emit);
            visit_queries(b, emit);
        }
        Term::Between(a, b, c, _) => {
            visit_queries(a, emit);
            visit_queries(b, emit);
            visit_queries(c, emit);
        }
        Term::In(input, list, _) => {
            visit_queries(input, emit);
            for term in list {
                visit_queries(term, emit);
            }
        }
        Term::Call(_, list, _) | Term::Aggregate(_, list, _) => {
            for term in list {
                visit_queries(term, emit);
            }
        }
        Term::Case(base, branches, other) => {
            for term in base.iter().chain(other.iter()) {
                visit_queries(term, emit);
            }
            for (a, b) in branches {
                visit_queries(a, emit);
                visit_queries(b, emit);
            }
        }
        _ => {}
    }
}
pub(super) fn contains(term: &Term) -> bool {
    let mut found = false;
    visit_queries(term, &mut |_| found = true);
    found
}
/// Go binds column spellings to schema identities before comparing group
/// expressions. Keep query syntax immutable while making the same comparison.
pub(super) fn same_group_expression(a: &Term, b: &Term, context: &Context<'_>) -> bool {
    if a == b {
        return true;
    }
    let same = |a: &Term, b: &Term| same_group_expression(a, b, context);
    match (a, b) {
        (Term::Column(a), Term::Column(b)) => {
            if !equal_fold(&a.name, &b.name) {
                return false;
            }
            let Some(group) = context.group else {
                return a == b;
            };
            match (
                resolve(group.columns, group.alias, a, context.scope.outer),
                resolve(group.columns, group.alias, b, context.scope.outer),
            ) {
                (Ok(Position::Local(a)), Ok(Position::Local(b))) => a == b,
                (Ok(Position::Outer(a, ai)), Ok(Position::Outer(b, bi))) => {
                    std::ptr::eq(a, b) && ai == bi
                }
                _ => false,
            }
        }
        (Term::Unary(a, x), Term::Unary(b, y)) => a == b && same(x, y),
        (Term::Binary(a, op, b), Term::Binary(c, other, d)) => {
            op == other && same(a, c) && same(b, d)
        }
        (Term::Cast(a, kind), Term::Cast(b, other)) => kind == other && same(a, b),
        (Term::Like(a, b, n), Term::Like(c, d, m)) => n == m && same(a, c) && same(b, d),
        (Term::NullTest(a, n), Term::NullTest(b, m)) => n == m && same(a, b),
        (Term::Between(a, b, c, n), Term::Between(d, e, f, m)) => {
            n == m && same(a, d) && same(b, e) && same(c, f)
        }
        (Term::In(a, x, n), Term::In(b, y, m)) => {
            n == m && same(a, b) && x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Term::InSubquery(a, x, n), Term::InSubquery(b, y, m)) => n == m && x == y && same(a, b),
        (Term::Call(a, x, d), Term::Call(b, y, e))
        | (Term::Aggregate(a, x, d), Term::Aggregate(b, y, e)) => {
            d == e && a == b && x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Term::Case(a, x, b), Term::Case(c, y, d)) => {
            let optional = |a: &Option<Box<Term>>, b: &Option<Box<Term>>| match (a, b) {
                (Some(a), Some(b)) => same(a, b),
                (None, None) => true,
                _ => false,
            };
            optional(a, c)
                && optional(b, d)
                && x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((a, b), (c, d))| same(a, c) && same(b, d))
        }
        _ => a == b,
    }
}

pub(super) fn group_mapping(
    keys: &[Term],
    columns: &[String],
    alias: ColumnOwners<'_>,
    work: &mut Work<'_>,
) -> Result<Vec<Option<usize>>> {
    work.safety_retain(
        columns
            .len()
            .checked_mul(std::mem::size_of::<Option<usize>>())
            .ok_or_else(budget)?,
    )?;
    let mut mapping = vec![None; columns.len()];
    for (key, term) in keys.iter().enumerate() {
        if let Term::Column(name) = term
            && let Some(index) = local(columns, alias, name)?
        {
            mapping[index].get_or_insert(key);
        }
    }
    Ok(mapping)
}

#[cfg(test)]
#[path = "subquery_tests.rs"]
mod subquery_tests;

#[cfg(test)]
#[path = "union_tail_tests.rs"]
mod union_tail_tests;
