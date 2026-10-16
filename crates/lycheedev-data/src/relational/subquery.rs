//! Expression-local query scopes. Frames borrow the current callback/group
//! row; neither frames nor short-lived lexical Sources enter the shared Work.
//! Go 3.2.3 completes each expression query on each evaluation.
use super::*;

/// Go inspect() counts logical retrieval, relation and expression nodes. The
/// Rust compound wrapper is synthetic; its branches model a right-linked Go
/// UNION chain and its tail expressions belong to that chain's last query.
pub(super) fn validate_nodes(program: &Program, check: &dyn Fn() -> Result<()>) -> Result<()> {
    enum Node<'a> {
        Query(&'a Program),
        Term(&'a Term),
        Leaf,
    }
    let mut pending = vec![(Node::Query(program), 1usize)];
    let mut count = 0usize;
    while let Some((node, depth)) = pending.pop() {
        check()?;
        if depth > 128 {
            return Err(budget());
        }
        if let Node::Query(query) = &node {
            if !query.branches.is_empty() {
                for (_, body) in &query.bindings {
                    pending.push((Node::Query(body), depth + 1));
                }
                for (index, branch) in query.branches.iter().enumerate() {
                    pending.push((Node::Query(branch), depth + index));
                }
                let tail = depth + query.branches.len();
                pending.extend(
                    query
                        .order
                        .iter()
                        .map(|(term, _, _)| (Node::Term(term), tail)),
                );
                pending.extend(
                    query
                        .limit
                        .iter()
                        .chain(query.offset.iter())
                        .map(|term| (Node::Term(term), tail)),
                );
                continue;
            }
        }
        count += 1;
        if count > 32768 {
            return Err(budget());
        }
        let next = depth + 1;
        match node {
            Node::Query(query) => {
                pending.push((Node::Leaf, next)); // primary relation
                pending.extend(
                    query
                        .projection
                        .iter()
                        .filter(|p| p.expr.is_none())
                        .map(|_| (Node::Leaf, next)),
                );
                for (_, body) in &query.bindings {
                    pending.push((Node::Query(body), next));
                }
                if let Some(body) = &query.derived {
                    pending.push((Node::Query(body), next + 1));
                }
                for link in &query.joins {
                    pending.push((Node::Leaf, next));
                    if let Some(body) = &link.derived {
                        pending.push((Node::Query(body), next + 1));
                    }
                }
                pending.extend(query.terms().map(|term| (Node::Term(term), next)));
            }
            Node::Term(term) => match term {
                Term::ScalarSubquery(query) | Term::Exists(query) => {
                    pending.push((Node::Query(query), next))
                }
                Term::InSubquery(input, query, _) => {
                    pending.push((Node::Query(query), next));
                    pending.push((Node::Term(input), next));
                }
                Term::Unary(_, t) | Term::NullTest(t, _) | Term::Cast(t, _) => {
                    pending.push((Node::Term(t), next))
                }
                Term::Binary(a, _, b) | Term::Like(a, b, _) => {
                    pending.push((Node::Term(a), next));
                    pending.push((Node::Term(b), next));
                }
                Term::Between(a, b, c, _) => {
                    pending.push((Node::Term(a), next));
                    pending.push((Node::Term(b), next));
                    pending.push((Node::Term(c), next));
                }
                Term::In(t, args, _) => {
                    pending.push((Node::Term(t), next));
                    pending.extend(args.iter().map(|term| (Node::Term(term), next)));
                }
                Term::Call(_, args, _) | Term::Aggregate(_, args, _) => {
                    pending.extend(args.iter().map(|term| (Node::Term(term), next)))
                }
                Term::Case(base, cases, other) => {
                    pending.extend(
                        base.iter()
                            .chain(other.iter())
                            .map(|t| (Node::Term(t), next)),
                    );
                    for (a, b) in cases {
                        pending.push((Node::Term(a), next));
                        pending.push((Node::Term(b), next));
                    }
                }
                _ => {}
            },
            Node::Leaf => {}
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
    pub outer: Option<&'a dyn OuterFrame>,
    pub callback: Option<Callback<'a>>,
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
            callback: Some(Callback::Map(sources)),
            fields: self.fields,
            allowed: None,
        }
    }
}
#[cfg(test)]
pub(super) struct Frame<'a> {
    row: &'a [Scalar],
    fields: Fields<'a>,
    resolver: Fields<'a>,
    mapping: Option<&'a [Option<usize>]>,
    allowed: Option<Correlation<'a>>,
    parent: Option<&'a dyn OuterFrame>,
}
#[derive(Clone, Copy)]
pub(super) enum Callback<'a> {
    Map(&'a BTreeMap<String, &'a Source>),
    Owned(&'a execution::Sources<'a>),
}
pub(super) trait OuterFrame {
    fn fields(&self) -> Fields<'_>;
    fn parent(&self) -> Option<&dyn OuterFrame>;
    fn allowed(&self) -> Option<Correlation<'_>>;
    fn clone_value(&self, index: usize, work: &mut Work<'_>) -> Result<Scalar>;
}
pub(super) fn resolved_index(
    fields: Fields<'_>,
    resolver: Fields<'_>,
    mapping: Option<&[Option<usize>]>,
    index: usize,
) -> Result<usize> {
    let (qualifier, name) = fields.part(index)?;
    let index =
        local_parts(resolver.columns, resolver.alias, qualifier, name)?.ok_or_else(binding)?;
    mapping.map_or(Ok(index), |mapping| {
        mapping.get(index).copied().flatten().ok_or_else(binding)
    })
}
fn check_allowed(fields: Fields<'_>, allowed: Option<Correlation<'_>>, index: usize) -> Result<()> {
    let Some(allowed) = allowed else {
        return Ok(());
    };
    let field = fields.part(index)?;
    let fields = Fields {
        columns: allowed.columns,
        alias: allowed.alias,
    };
    for (index, entry) in allowed.mapping.iter().enumerate() {
        if entry.is_some() && fields.part(index)? == field {
            return Ok(());
        }
    }
    Err(binding())
}
#[cfg(test)]
impl OuterFrame for Frame<'_> {
    fn fields(&self) -> Fields<'_> {
        self.fields
    }
    fn parent(&self) -> Option<&dyn OuterFrame> {
        self.parent
    }
    fn allowed(&self) -> Option<Correlation<'_>> {
        self.allowed
    }
    fn clone_value(&self, index: usize, work: &mut Work<'_>) -> Result<Scalar> {
        let index = resolved_index(self.fields, self.resolver, self.mapping, index)?;
        work.clone_scalar(self.row.get(index).ok_or_else(binding)?)
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
        outer: Option<&'a dyn OuterFrame>,
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
    Outer(&'a dyn OuterFrame, usize),
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
    mut outer: Option<&'a dyn OuterFrame>,
) -> Result<Position<'a>> {
    if let Some(index) = local(columns, alias, name)? {
        return Ok(Position::Local(index));
    }
    while let Some(frame) = outer {
        if let Some(index) = local(frame.fields().columns, frame.fields().alias, name)? {
            check_allowed(frame.fields(), frame.allowed(), index)?;
            return Ok(Position::Outer(frame, index));
        }
        outer = frame.parent();
    }
    Err(missing_column(columns, alias, name))
}
pub(super) fn display_column(
    columns: &[String],
    alias: ColumnOwners<'_>,
    name: &ColumnRef,
    outer: Option<&dyn OuterFrame>,
) -> Result<Option<String>> {
    let Position::Local(index) = resolve(columns, alias, name, outer)? else {
        // Go's bound outer-column node is an expression, not a local column.
        return Ok(None);
    };
    Ok(Some(columns[index].clone()))
}

#[cfg(test)]
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
#[cfg(test)]
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
    let callback = context.scope.callback.ok_or_else(unsupported)?;
    let owned;
    let sources = match callback {
        Callback::Map(sources) => sources,
        Callback::Owned(sources) => {
            owned = sources
                .iter()
                .map(|(name, source)| (name.clone(), &**source))
                .collect();
            &owned
        }
    };
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
    let mut pending = vec![term];
    while let Some(term) = pending.pop() {
        match term {
            Term::ScalarSubquery(query) | Term::Exists(query) | Term::InSubquery(_, query, _) => {
                emit(query)
            }
            // Preserve the original inspector's base/else/branches order,
            // which differs from evaluation's base/branches/else order.
            Term::Case(base, branches, other) => {
                for (a, b) in branches.iter().rev() {
                    pending.extend([b, a]);
                }
                pending.extend(other.as_deref());
                pending.extend(base.as_deref());
                continue;
            }
            _ => {}
        }
        term_tree::children(term, &mut pending);
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
    tree_equality::grouped(a, b, context)
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
