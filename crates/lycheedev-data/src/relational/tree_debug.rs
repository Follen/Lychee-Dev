//! Diagnostics traverse the same mixed scalar/query graph as equality and
//! teardown. Never delegate a tree-bearing field to its derived Debug: doing
//! so would reenter the formatter on the native stack at every query edge.
use super::{Program, Projection, Term, joins};
use std::fmt;

enum Task<'a> {
    Text(&'static str),
    // Only finite leaves (or collections of leaves), never tree owners.
    Leaf(&'a dyn fmt::Debug),
    Term(&'a Term),
    Query(&'a Program),
    OptionalTerm(Option<&'a Term>),
    OptionalQuery(Option<&'a Program>),
    Projection(&'a Projection),
    Link(&'a joins::Link),
    Binding(&'a (String, Program)),
    Order(&'a (Term, bool, Option<bool>)),
    Case(&'a (Term, Term)),
    Terms(&'a [Term], bool),
    Queries(&'a [Program], bool),
    Projections(&'a [Projection], bool),
    Links(&'a [joins::Link], bool),
    Bindings(&'a [(String, Program)], bool),
    Orders(&'a [(Term, bool, Option<bool>)], bool),
    Cases(&'a [(Term, Term)], bool),
}

impl fmt::Debug for Term {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        format(Task::Term(self), output)
    }
}

impl fmt::Debug for Program {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        format(Task::Query(self), output)
    }
}

// One list cursor stays live at a time. A wide collection therefore does not
// allocate one pending task per element; space follows nesting depth instead.
fn sequence<'a, T>(
    values: &'a [T],
    first: bool,
    rest: fn(&'a [T], bool) -> Task<'a>,
    item: fn(&'a T) -> Task<'a>,
    pending: &mut Vec<Task<'a>>,
    output: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    if first {
        output.write_str("[")?;
    }
    if let Some((value, values)) = values.split_first() {
        if !first {
            output.write_str(", ")?;
        }
        pending.push(rest(values, false));
        pending.push(item(value));
        Ok(())
    } else {
        output.write_str("]")
    }
}

fn format(root: Task<'_>, output: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut pending = vec![root];
    while let Some(task) = pending.pop() {
        match task {
            Task::Text(text) => output.write_str(text)?,
            // Deliberately compact even for {:#?}: indentation must not make
            // deep diagnostics quadratic or reintroduce recursive builders.
            Task::Leaf(value) => write!(output, "{value:?}")?,
            Task::OptionalTerm(value) => match value {
                Some(value) => {
                    output.write_str("Some(")?;
                    pending.extend([Task::Text(")"), Task::Term(value)]);
                }
                None => output.write_str("None")?,
            },
            Task::OptionalQuery(value) => match value {
                Some(value) => {
                    output.write_str("Some(")?;
                    pending.extend([Task::Text(")"), Task::Query(value)]);
                }
                None => output.write_str("None")?,
            },
            Task::Term(term) => {
                pending.push(Task::Text(")"));
                match term {
                    Term::Value(value) => {
                        output.write_str("Value(")?;
                        pending.push(Task::Leaf(value));
                    }
                    Term::Number(value) => {
                        output.write_str("Number(")?;
                        pending.push(Task::Leaf(value));
                    }
                    Term::Ordinal(value) => {
                        output.write_str("Ordinal(")?;
                        pending.push(Task::Leaf(value));
                    }
                    Term::Column(value) => {
                        output.write_str("Column(")?;
                        pending.push(Task::Leaf(value));
                    }
                    Term::Parameter(value) => {
                        output.write_str("Parameter(")?;
                        pending.push(Task::Leaf(value));
                    }
                    Term::Star(value) => {
                        output.write_str("Star(")?;
                        pending.push(Task::Leaf(value));
                    }
                    Term::Unary(operator, value) => {
                        output.write_str("Unary(")?;
                        pending.extend([Task::Term(value), Task::Text(", "), Task::Leaf(operator)]);
                    }
                    Term::Binary(left, operator, right) => {
                        output.write_str("Binary(")?;
                        pending.extend([
                            Task::Term(right),
                            Task::Text(", "),
                            Task::Leaf(operator),
                            Task::Text(", "),
                            Task::Term(left),
                        ]);
                    }
                    Term::NullTest(value, negated) => {
                        output.write_str("NullTest(")?;
                        pending.extend([Task::Leaf(negated), Task::Text(", "), Task::Term(value)]);
                    }
                    Term::Cast(value, kind) => {
                        output.write_str("Cast(")?;
                        pending.extend([Task::Leaf(kind), Task::Text(", "), Task::Term(value)]);
                    }
                    Term::Like(value, pattern, negated) => {
                        output.write_str("Like(")?;
                        pending.extend([
                            Task::Leaf(negated),
                            Task::Text(", "),
                            Task::Term(pattern),
                            Task::Text(", "),
                            Task::Term(value),
                        ]);
                    }
                    Term::Between(value, low, high, negated) => {
                        output.write_str("Between(")?;
                        pending.extend([
                            Task::Leaf(negated),
                            Task::Text(", "),
                            Task::Term(high),
                            Task::Text(", "),
                            Task::Term(low),
                            Task::Text(", "),
                            Task::Term(value),
                        ]);
                    }
                    Term::In(value, values, negated) => {
                        output.write_str("In(")?;
                        pending.extend([
                            Task::Leaf(negated),
                            Task::Text(", "),
                            Task::Terms(values, true),
                            Task::Text(", "),
                            Task::Term(value),
                        ]);
                    }
                    Term::InSubquery(value, query, negated) => {
                        output.write_str("InSubquery(")?;
                        pending.extend([
                            Task::Leaf(negated),
                            Task::Text(", "),
                            Task::Query(query),
                            Task::Text(", "),
                            Task::Term(value),
                        ]);
                    }
                    Term::ScalarSubquery(query) => {
                        output.write_str("ScalarSubquery(")?;
                        pending.push(Task::Query(query));
                    }
                    Term::Exists(query) => {
                        output.write_str("Exists(")?;
                        pending.push(Task::Query(query));
                    }
                    Term::Call(name, values, distinct)
                    | Term::Aggregate(name, values, distinct) => {
                        output.write_str(if matches!(term, Term::Call(..)) {
                            "Call("
                        } else {
                            "Aggregate("
                        })?;
                        pending.extend([
                            Task::Leaf(distinct),
                            Task::Text(", "),
                            Task::Terms(values, true),
                            Task::Text(", "),
                            Task::Leaf(name),
                        ]);
                    }
                    Term::Case(base, cases, other) => {
                        output.write_str("Case(")?;
                        pending.extend([
                            Task::OptionalTerm(other.as_deref()),
                            Task::Text(", "),
                            Task::Cases(cases, true),
                            Task::Text(", "),
                            Task::OptionalTerm(base.as_deref()),
                        ]);
                    }
                }
            }
            Task::Query(query) => {
                // Exhaustive destructuring makes new fields a compile error,
                // rather than silently omitting them from diagnostics.
                let Program {
                    explain,
                    analyze,
                    recursive,
                    bindings,
                    branches,
                    derived,
                    table,
                    alias,
                    projection,
                    filter,
                    order,
                    parameters,
                    limit,
                    offset,
                    distinct,
                    joins,
                    groups,
                    having,
                } = query;
                output.write_str("Program { explain: ")?;
                pending.extend([
                    Task::Text(" }"),
                    Task::OptionalTerm(having.as_ref()),
                    Task::Text(", having: "),
                    Task::Terms(groups, true),
                    Task::Text(", groups: "),
                    Task::Links(joins, true),
                    Task::Text(", joins: "),
                    Task::Leaf(distinct),
                    Task::Text(", distinct: "),
                    Task::OptionalTerm(offset.as_ref()),
                    Task::Text(", offset: "),
                    Task::OptionalTerm(limit.as_ref()),
                    Task::Text(", limit: "),
                    Task::Leaf(parameters),
                    Task::Text(", parameters: "),
                    Task::Orders(order, true),
                    Task::Text(", order: "),
                    Task::OptionalTerm(filter.as_ref()),
                    Task::Text(", filter: "),
                    Task::Projections(projection, true),
                    Task::Text(", projection: "),
                    Task::Leaf(alias),
                    Task::Text(", alias: "),
                    Task::Leaf(table),
                    Task::Text(", table: "),
                    Task::OptionalQuery(derived.as_deref()),
                    Task::Text(", derived: "),
                    Task::Queries(branches, true),
                    Task::Text(", branches: "),
                    Task::Bindings(bindings, true),
                    Task::Text(", bindings: "),
                    Task::Leaf(recursive),
                    Task::Text(", recursive: "),
                    Task::Leaf(analyze),
                    Task::Text(", analyze: "),
                    Task::Leaf(explain),
                ]);
            }
            Task::Projection(projection) => {
                let Projection { expr, name } = projection;
                output.write_str("Projection { expr: ")?;
                pending.extend([
                    Task::Text(" }"),
                    Task::Leaf(name),
                    Task::Text(", name: "),
                    Task::OptionalTerm(expr.as_ref()),
                ]);
            }
            Task::Link(link) => {
                let joins::Link {
                    table,
                    alias,
                    derived,
                    left,
                    condition,
                } = link;
                output.write_str("Link { table: ")?;
                pending.extend([
                    Task::Text(" }"),
                    Task::OptionalTerm(condition.as_ref()),
                    Task::Text(", condition: "),
                    Task::Leaf(left),
                    Task::Text(", left: "),
                    Task::OptionalQuery(derived.as_deref()),
                    Task::Text(", derived: "),
                    Task::Leaf(alias),
                    Task::Text(", alias: "),
                    Task::Leaf(table),
                ]);
            }
            Task::Binding((name, query)) => {
                output.write_str("(")?;
                pending.extend([
                    Task::Text(")"),
                    Task::Query(query),
                    Task::Text(", "),
                    Task::Leaf(name),
                ]);
            }
            Task::Order((term, descending, nulls_first)) => {
                output.write_str("(")?;
                pending.extend([
                    Task::Text(")"),
                    Task::Leaf(nulls_first),
                    Task::Text(", "),
                    Task::Leaf(descending),
                    Task::Text(", "),
                    Task::Term(term),
                ]);
            }
            Task::Case((condition, result)) => {
                output.write_str("(")?;
                pending.extend([
                    Task::Text(")"),
                    Task::Term(result),
                    Task::Text(", "),
                    Task::Term(condition),
                ]);
            }
            Task::Terms(values, first) => {
                sequence(values, first, Task::Terms, Task::Term, &mut pending, output)?;
            }
            Task::Queries(values, first) => {
                sequence(
                    values,
                    first,
                    Task::Queries,
                    Task::Query,
                    &mut pending,
                    output,
                )?;
            }
            Task::Projections(values, first) => sequence(
                values,
                first,
                Task::Projections,
                Task::Projection,
                &mut pending,
                output,
            )?,
            Task::Links(values, first) => {
                sequence(values, first, Task::Links, Task::Link, &mut pending, output)?;
            }
            Task::Bindings(values, first) => sequence(
                values,
                first,
                Task::Bindings,
                Task::Binding,
                &mut pending,
                output,
            )?,
            Task::Orders(values, first) => {
                sequence(
                    values,
                    first,
                    Task::Orders,
                    Task::Order,
                    &mut pending,
                    output,
                )?;
            }
            Task::Cases(values, first) => {
                sequence(values, first, Task::Cases, Task::Case, &mut pending, output)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{BinaryOperator, ColumnRef, RelationName, Scalar, UnaryOperator};
    use super::*;
    use std::{collections::BTreeSet, fmt::Write, sync::Arc};

    fn query() -> Program {
        Program {
            explain: false,
            analyze: false,
            recursive: false,
            bindings: Vec::new(),
            branches: Vec::new(),
            derived: None,
            table: RelationName::internal("source"),
            alias: "row".into(),
            projection: Vec::new(),
            filter: None,
            order: Vec::new(),
            parameters: BTreeSet::new(),
            limit: None,
            offset: None,
            distinct: false,
            joins: Vec::new(),
            groups: Vec::new(),
            having: None,
        }
    }

    fn number(value: &str) -> Term {
        Term::Number(value.into())
    }

    #[test]
    fn all_scalar_variants_preserve_names_fields_and_order() {
        let cases = [
            (Term::Value(Scalar::Int(7)), "Value(Int(7))"),
            (number("12"), "Number(\"12\")"),
            (Term::Ordinal(3), "Ordinal(3)"),
            (
                Term::Column(ColumnRef::qualified("t", "id")),
                "Column(ColumnRef { qualifier: \"t\", name: \"id\" })",
            ),
            (Term::Parameter("$1".into()), "Parameter(\"$1\")"),
            (Term::Star("t".into()), "Star(\"t\")"),
            (
                Term::Unary(UnaryOperator::Not, Box::new(number("1"))),
                "Unary(Not, Number(\"1\"))",
            ),
            (
                Term::Binary(
                    Box::new(number("1")),
                    BinaryOperator::Plus,
                    Box::new(number("2")),
                ),
                "Binary(Number(\"1\"), Plus, Number(\"2\"))",
            ),
            (
                Term::NullTest(Box::new(number("1")), true),
                "NullTest(Number(\"1\"), true)",
            ),
            (
                Term::Cast(Box::new(number("1")), "TEXT".into()),
                "Cast(Number(\"1\"), \"TEXT\")",
            ),
            (
                Term::Like(Box::new(number("1")), Box::new(number("2")), true),
                "Like(Number(\"1\"), Number(\"2\"), true)",
            ),
            (
                Term::Between(
                    Box::new(number("1")),
                    Box::new(number("2")),
                    Box::new(number("3")),
                    false,
                ),
                "Between(Number(\"1\"), Number(\"2\"), Number(\"3\"), false)",
            ),
            (
                Term::In(Box::new(number("1")), vec![number("2"), number("3")], true),
                "In(Number(\"1\"), [Number(\"2\"), Number(\"3\")], true)",
            ),
            (
                Term::Call("COALESCE".into(), vec![number("1"), number("2")], false),
                "Call(\"COALESCE\", [Number(\"1\"), Number(\"2\")], false)",
            ),
            (
                Term::Aggregate("COUNT".into(), vec![], true),
                "Aggregate(\"COUNT\", [], true)",
            ),
            (
                Term::Case(
                    Some(Box::new(number("1"))),
                    vec![(number("2"), number("3")), (number("4"), number("5"))],
                    Some(Box::new(number("6"))),
                ),
                "Case(Some(Number(\"1\")), [(Number(\"2\"), Number(\"3\")), (Number(\"4\"), Number(\"5\"))], Some(Number(\"6\")))",
            ),
            (Term::Case(None, vec![], None), "Case(None, [], None)"),
        ];
        for (term, expected) in cases {
            assert_eq!(format!("{term:?}"), expected);
            assert_eq!(format!("{term:#?}"), expected);
        }
        let query = Arc::new(query());
        let expected = format!("{query:?}");
        let cases = [
            (
                Term::ScalarSubquery(query.clone()),
                format!("ScalarSubquery({expected})"),
            ),
            (Term::Exists(query.clone()), format!("Exists({expected})")),
            (
                Term::InSubquery(Box::new(number("1")), query.clone(), true),
                format!("InSubquery(Number(\"1\"), {expected}, true)"),
            ),
        ];
        for (term, expected) in cases {
            let owners = Arc::strong_count(&query);
            assert_eq!(format!("{term:?}"), expected);
            assert_eq!(format!("{term:#?}"), expected);
            assert_eq!(Arc::strong_count(&query), owners);
        }
    }

    #[test]
    fn program_preserves_all_fields_and_nested_container_boundaries() {
        let empty = "Program { explain: false, analyze: false, recursive: false, bindings: [], branches: [], derived: None, table: RelationName { catalog: None, name: \"source\", key: \"source\" }, alias: \"row\", projection: [], filter: None, order: [], parameters: {}, limit: None, offset: None, distinct: false, joins: [], groups: [], having: None }";
        assert_eq!(format!("{:?}", query()), empty);
        let mut program = query();
        program.explain = true;
        program.analyze = true;
        program.recursive = true;
        program.bindings = vec![("a".into(), query()), ("b".into(), query())];
        program.branches = vec![query(), query()];
        program.derived = Some(Box::new(query()));
        program.projection = vec![
            Projection {
                expr: Some(number("1")),
                name: "one\n\"quoted\"".into(),
            },
            Projection {
                expr: None,
                name: "*".into(),
            },
        ];
        program.filter = Some(Term::Value(Scalar::Bool(true)));
        program.order = vec![(number("2"), true, Some(false)), (number("3"), false, None)];
        program.parameters = BTreeSet::from(["$2".into(), "$1".into()]);
        program.limit = Some(number("4"));
        program.offset = Some(number("5"));
        program.distinct = true;
        program.joins = vec![joins::Link {
            table: RelationName::internal("right"),
            alias: "r".into(),
            derived: Some(Box::new(query())),
            left: true,
            condition: Some(Term::Value(Scalar::Bool(false))),
        }];
        program.groups = vec![number("6"), number("7")];
        program.having = Some(number("8"));
        let expected = format!(
            "Program {{ explain: true, analyze: true, recursive: true, bindings: [(\"a\", {empty}), (\"b\", {empty})], branches: [{empty}, {empty}], derived: Some({empty}), table: RelationName {{ catalog: None, name: \"source\", key: \"source\" }}, alias: \"row\", projection: [Projection {{ expr: Some(Number(\"1\")), name: \"one\\n\\\"quoted\\\"\" }}, Projection {{ expr: None, name: \"*\" }}], filter: Some(Value(Bool(true))), order: [(Number(\"2\"), true, Some(false)), (Number(\"3\"), false, None)], parameters: {{\"$1\", \"$2\"}}, limit: Some(Number(\"4\")), offset: Some(Number(\"5\")), distinct: true, joins: [Link {{ table: RelationName {{ catalog: None, name: \"right\", key: \"right\" }}, alias: \"r\", derived: Some({empty}), left: true, condition: Some(Value(Bool(false))) }}], groups: [Number(\"6\"), Number(\"7\")], having: Some(Number(\"8\")) }}"
        );
        assert_eq!(format!("{program:?}"), expected);
        assert_eq!(format!("{program:#?}"), expected);
    }

    #[derive(Default)]
    struct BoundedOutput {
        bytes: usize,
        prefix: String,
    }
    impl fmt::Write for BoundedOutput {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.bytes += text.len();
            let mut keep = text.len().min(1024 - self.prefix.len());
            while !text.is_char_boundary(keep) {
                keep -= 1;
            }
            self.prefix.push_str(&text[..keep]);
            Ok(())
        }
    }

    #[test]
    fn long_term_debug_uses_default_stack_and_bounded_output_storage() {
        const DEPTH: usize = 32_768;
        let mut term = number("0");
        for _ in 0..DEPTH {
            term = Term::Binary(Box::new(term), BinaryOperator::Plus, Box::new(number("1")));
        }
        let mut compact = BoundedOutput::default();
        let mut pretty = BoundedOutput::default();
        write!(&mut compact, "{term:?}").unwrap();
        write!(&mut pretty, "{term:#?}").unwrap();
        assert!(compact.prefix.starts_with("Binary(Binary(Binary("));
        assert_eq!(
            compact.bytes,
            "Number(\"0\")".len() + DEPTH * "Binary(, Plus, Number(\"1\"))".len()
        );
        assert_eq!(compact.bytes, pretty.bytes);
        assert_eq!(compact.prefix, pretty.prefix);
        drop(term);
    }

    #[test]
    fn mixed_query_debug_follows_every_owned_and_shared_edge_on_default_stack() {
        const DEPTH: usize = 4096;
        let mut nested = query();
        for index in 0..DEPTH {
            let mut outer = query();
            match index % 14 {
                0 => outer.derived = Some(Box::new(nested)),
                1 => outer.bindings.push(("cte".into(), nested)),
                2 => outer.branches.push(nested),
                3 => outer.joins.push(joins::Link {
                    table: RelationName::internal("joined"),
                    alias: "j".into(),
                    derived: Some(Box::new(nested)),
                    left: true,
                    condition: None,
                }),
                4 => outer.projection.push(Projection {
                    expr: Some(Term::ScalarSubquery(Arc::new(nested))),
                    name: "scalar".into(),
                }),
                5 => outer.filter = Some(Term::Exists(Arc::new(nested))),
                6 => outer
                    .order
                    .push((Term::ScalarSubquery(Arc::new(nested)), true, Some(false))),
                7 => outer.limit = Some(Term::ScalarSubquery(Arc::new(nested))),
                8 => outer.offset = Some(Term::ScalarSubquery(Arc::new(nested))),
                9 => outer.groups.push(Term::ScalarSubquery(Arc::new(nested))),
                10 => outer.having = Some(Term::Exists(Arc::new(nested))),
                11 => outer.joins.push(joins::Link {
                    table: RelationName::internal("joined"),
                    alias: "j".into(),
                    derived: None,
                    left: false,
                    condition: Some(Term::Exists(Arc::new(nested))),
                }),
                12 => {
                    outer.filter = Some(Term::InSubquery(
                        Box::new(number("1")),
                        Arc::new(nested),
                        false,
                    ))
                }
                _ => outer.projection.push(Projection {
                    expr: Some(Term::Case(
                        None,
                        vec![(number("1"), Term::ScalarSubquery(Arc::new(nested)))],
                        None,
                    )),
                    name: "case".into(),
                }),
            }
            nested = outer;
        }
        let mut compact = BoundedOutput::default();
        let mut pretty = BoundedOutput::default();
        write!(&mut compact, "{nested:?}").unwrap();
        write!(&mut pretty, "{nested:#?}").unwrap();
        assert!(compact.prefix.starts_with("Program { explain: false"));
        assert!(compact.bytes > DEPTH * 300);
        assert_eq!(compact.bytes, pretty.bytes);
        assert_eq!(compact.prefix, pretty.prefix);
        // Derived wrappers may enter once, but must not reenter at each edge.
        let projection = Projection {
            expr: Some(Term::ScalarSubquery(Arc::new(nested))),
            name: "wrapped".into(),
        };
        write!(&mut BoundedOutput::default(), "{projection:#?}").unwrap();
        drop(projection);
    }

    #[test]
    fn output_errors_stop_without_traversing_or_cloning_remaining_nodes() {
        struct Rejected;
        impl fmt::Write for Rejected {
            fn write_str(&mut self, _: &str) -> fmt::Result {
                Err(fmt::Error)
            }
        }
        let query = Arc::new(query());
        let term = Term::InSubquery(Box::new(number("1")), query.clone(), false);
        assert!(write!(&mut Rejected, "{term:?}").is_err());
        assert!(write!(&mut Rejected, "{query:#?}").is_err());
        assert_eq!(Arc::strong_count(&query), 2);
    }
}
