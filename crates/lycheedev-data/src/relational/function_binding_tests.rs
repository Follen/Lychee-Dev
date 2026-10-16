//! Fixed Go323 rows.go/expressions.go/aggregate.go/explain.go contracts.
//! These assertions exercise the actual compiler/binder/executor, not a model.
use super::*;
fn work() -> Work<'static> {
    Work {
        left: 1000,
        bytes: 1 << 20,
        deadline: Instant::now() + Duration::from_secs(10),
        cancel: None,
        shared: None,
        planning: false,
        temporaries: Vec::new(),
        selection_sources: Vec::new(),
        providers: &[],
        resolver: None,
        roots: BTreeMap::new(),
        measure: false,
        scans: Vec::new(),
    }
}
fn input() -> BTreeMap<String, Source> {
    BTreeMap::from([(
        "t".into(),
        Source {
            columns: vec!["ID".into(), "ALL".into()],
            rows: vec![vec![Scalar::Int(1), Scalar::Int(7)]],
            complete: true,
        },
    )])
}
#[test]
fn call_and_star_binding_spend_only_the_nodes_go_visits() {
    for (expr, expected, units) in [
        ("COUNT(*)", None, 1),
        ("COUNT(((*)))", None, 1),
        ("COUNT(t.*)", Some("relational.type_mismatch"), 1),
        ("SUM(*)", Some("relational.type_mismatch"), 1),
        ("COUNT(DISTINCT)", Some("relational.type_mismatch"), 1),
        ("LOWER(*)", Some("relational.type_mismatch"), 1),
        ("UNKNOWN(*)", Some("relational.unsupported"), 1),
        ("UNKNOWN(DISTINCT *)", Some("relational.type_mismatch"), 1),
        ("SUM(COUNT(*))", Some("relational.type_mismatch"), 2),
        ("SUM(*+Missing)", Some("relational.type_mismatch"), 2),
        ("SUM(Missing+*)", Some("relational.invalid_binding"), 3),
        ("COALESCE(1, *)", Some("relational.type_mismatch"), 2),
        ("CAST(* AS INT)", Some("relational.type_mismatch"), 1),
        ("CAST(* AS Unsupported)", Some("relational.unsupported"), 1),
    ] {
        let program = Program::compile(&format!("SELECT {expr} FROM t")).unwrap();
        let node = program.projection[0].expr.as_ref().unwrap();
        let sources = BTreeMap::new();
        let context = subquery::Context::new(&sources, None);
        let mut work = work();
        let result = bind(
            node,
            &["ID".into()],
            ColumnOwners::Single("t"),
            &BTreeMap::new(),
            &context,
            &mut work,
        );
        assert_eq!(
            result.as_ref().err().map(|e| e.code.as_str()),
            expected,
            "{expr}: {result:?}"
        );
        assert_eq!(1000 - work.left, units, "{expr}");
    }
}
#[test]
fn all_requires_quotes_as_a_column_argument() {
    let result = Program::compile(r#"SELECT SUM("ALL"), COUNT(DISTINCT "ALL") FROM t"#)
        .unwrap()
        .execute(&input(), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(7), Scalar::Int(1)]]);
    for sql in [
        "SELECT SUM(ALL) FROM t",
        "SELECT COUNT(DISTINCT ALL) FROM t",
        "SELECT SUM(ALL ID) FROM t",
        "SELECT ALL ID FROM t",
        "SELECT ID FROM t LIMIT ALL",
    ] {
        assert_eq!(
            Program::compile(sql).unwrap_err().code,
            "relational.invalid_syntax",
            "{sql}"
        );
    }
}
#[test]
fn quoted_function_names_and_wildcard_qualifiers_remain_typed() {
    let program = Program::compile(r#"SELECT "count"("a.b".*) FROM t "a.b""#).unwrap();
    assert!(
        matches!(program.projection[0].expr.as_ref(), Some(Term::Aggregate(name, args, false))
        if name == "COUNT" && matches!(args.as_slice(), [Term::Star(qualifier)] if qualifier == "a.b"))
    );
    assert_eq!(
        program
            .execute(&input(), &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.type_mismatch"
    );
}
#[test]
fn parenthesized_count_star_stays_successful_and_projectable() {
    let program = Program::compile("SELECT COUNT(((*))) FROM t").unwrap();
    let node = program.projection[0].expr.as_ref().unwrap();
    assert!(provider::projectable(
        node,
        &["ID".into(), "ALL".into()],
        ColumnOwners::Single("t")
    ));
    let result = program
        .execute(&input(), &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
}
#[test]
fn explanation_counts_star_while_binding_does_not() {
    let program = Program::compile("EXPLAIN ANALYZE SELECT COUNT(*) FROM t").unwrap();
    let sources = BTreeMap::from([(
        "t".into(),
        Source {
            columns: vec!["ID".into()],
            rows: vec![vec![Scalar::Int(1)], vec![Scalar::Int(2)]],
            complete: true,
        },
    )]);
    let limits = Limits {
        work: 24,
        memory_bytes: 1236,
        ..Limits::default()
    };
    let result = program
        .execute(&sources, &BTreeMap::new(), limits, None)
        .unwrap();
    let measured = result.plan.unwrap().measured.unwrap();
    assert_eq!((measured.work_units, measured.charged_bytes), (24, 1236));
    assert_eq!(
        program
            .execute(
                &sources,
                &BTreeMap::new(),
                Limits { work: 23, ..limits },
                None
            )
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
}
#[test]
fn explanation_counts_plain_and_parenthesized_projection_stars() {
    for sql in [
        "SELECT * FROM t",
        "SELECT (*) FROM t",
        "SELECT ((t.*)) FROM t",
    ] {
        let program = Program::compile(sql).unwrap();
        let mut work = work();
        let plan = explain::build(&program, vec!["ID".into()], &mut work).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(1000 - work.left, 3, "{sql}");
        assert_eq!(
            program
                .execute(&input(), &BTreeMap::new(), Limits::default(), None)
                .unwrap()
                .rows,
            vec![vec![Scalar::Int(1), Scalar::Int(7)]]
        );
    }
}
#[test]
fn malformed_aggregate_keeps_subquery_dependencies_without_resolving_them() {
    let program = Program::compile("SELECT SUM(ID, (SELECT ID FROM hidden)) FROM t").unwrap();
    assert_eq!(program.source_tables(), ["hidden", "t"]);
}
#[test]
fn raw_function_distinct_error_is_separate_from_public_binding() {
    let program = Program::compile("SELECT LOWER(DISTINCT 'x') FROM t").unwrap();
    let node = program.projection[0].expr.as_ref().unwrap();
    assert_eq!(
        eval(
            node,
            &[],
            &[],
            ColumnOwners::Single(""),
            &BTreeMap::new(),
            &mut work()
        )
        .unwrap_err()
        .code,
        "relational.unsupported"
    );
    assert_eq!(
        program
            .execute(&input(), &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.type_mismatch"
    );
}

#[test]
fn quoted_reserved_function_names_are_not_reclassified_as_sql_keywords() {
    for sql in [
        r#"SELECT "ALL"(ID) FROM t"#,
        r#"SELECT "CAST"(ID) FROM t"#,
        r#"SELECT "ſELECT"(ID) FROM t"#,
    ] {
        let program = Program::compile(sql).unwrap();
        assert_eq!(
            program
                .execute(&input(), &BTreeMap::new(), Limits::default(), None)
                .unwrap_err()
                .code,
            "relational.unsupported",
            "{sql}"
        );
    }
    assert_eq!(
        Program::compile("SELECT ſELECT(ID) FROM t")
            .unwrap_err()
            .code,
        "relational.invalid_syntax"
    );
}

#[test]
fn reserved_words_in_named_parameters_are_not_sql_keywords() {
    let program = Program::compile("SELECT :ALL,:UNION FROM t LIMIT :ALL").unwrap();
    let result = program
        .execute(
            &input(),
            &BTreeMap::from([
                ("ALL".into(), Scalar::Int(1)),
                ("UNION".into(), Scalar::Int(9)),
            ]),
            Limits::default(),
            None,
        )
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1), Scalar::Int(9)]]);
    for sql in [
        "SELECT : ALL FROM t",
        "SELECT :1 FROM t",
        r#"SELECT :"ALL" FROM t"#,
    ] {
        assert_eq!(
            Program::compile(sql).unwrap_err().code,
            "relational.invalid_syntax",
            "{sql}"
        );
    }
}
